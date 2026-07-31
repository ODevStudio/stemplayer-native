param(
    [string]$OutputDirectory = "artifacts/phase10/windows",
    [int]$Port = 18710,
    [string]$PrivateUrl = $env:STEMPLAYER_PRIVATE_HLS_URL
)

$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PWD $OutputDirectory))
$workspace = [IO.Path]::GetFullPath($PWD.Path)
if (-not $root.StartsWith($workspace, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Output directory must remain inside the native workspace'
}
$fixture = Join-Path $root 'fixture'
New-Item -ItemType Directory -Force -Path $fixture | Out-Null
$utf8 = [Text.UTF8Encoding]::new($false)

$roles = [ordered]@{ drums = 330; bass = 110; other = 550; vocals = 660; master = 220 }
foreach ($entry in $roles.GetEnumerator()) {
    $roleDirectory = Join-Path $fixture $entry.Key
    New-Item -ItemType Directory -Force -Path $roleDirectory | Out-Null
    $source = Join-Path $roleDirectory 'source.wav'
    & ffmpeg -hide_banner -loglevel error -y -f lavfi -i "sine=frequency=$($entry.Value):duration=4:sample_rate=48000" -ac 2 -c:a pcm_f32le $source
    if ($LASTEXITCODE -ne 0) { throw "Failed to generate $($entry.Key) source" }
    & ffmpeg -hide_banner -loglevel error -y -i $source -af 'atrim=start_sample=0:end_sample=96000,asetpts=PTS-STARTPTS' -c:a aac -profile:a aac_low -b:a 96k -movflags +faststart (Join-Path $roleDirectory 'segment-0.m4a')
    if ($LASTEXITCODE -ne 0) { throw "Failed to encode $($entry.Key) segment 0" }
    & ffmpeg -hide_banner -loglevel error -y -i $source -af 'atrim=start_sample=94976:end_sample=192000,asetpts=PTS-STARTPTS' -c:a aac -profile:a aac_low -b:a 96k -movflags +faststart (Join-Path $roleDirectory 'segment-1.m4a')
    if ($LASTEXITCODE -ne 0) { throw "Failed to encode $($entry.Key) segment 1" }
    [IO.File]::WriteAllLines((Join-Path $roleDirectory 'track.m3u8'), @('#EXTM3U', '#EXT-X-VERSION:7', '#EXT-X-TARGETDURATION:2', '#EXT-X-MEDIA-SEQUENCE:0', '#EXT-X-PLAYLIST-TYPE:VOD', '#EXTINF:2.000000,', 'segment-0.m4a', '#EXTINF:2.000000,', 'segment-1.m4a', '#EXT-X-ENDLIST'), $utf8)
    [IO.File]::WriteAllText((Join-Path $roleDirectory 'waveform.json'), (@{ schema = 'stem-waveform-v1'; samples_per_peak = 48000; min = @(-0.5, -0.5, -0.5, -0.5); max = @(0.5, 0.5, 0.5, 0.5) } | ConvertTo-Json -Compress), $utf8)
}

function Route([string]$role) {
    @{ playlist = "$role/track.m3u8"; waveform = "$role/waveform.json" }
}

$manifest = [ordered]@{
    schema = 'stem-hls-v1'
    manifest_version = 1
    format_version = 'stem-hls-v1'
    title = 'Synthetic Phase 10 HLS'
    duration_us = 4000000
    sample_rate = 48000
    channels = 2
    codec = [ordered]@{ name = 'AAC-LC'; codec_string = 'mp4a.40.2'; segment_format = 'M4A'; bitrate_bps = 96000 }
    segment_duration_us = 2000000
    segment_count = 2
    master_present = $true
    master = Route 'master'
    stems = [ordered]@{ drums = Route 'drums'; bass = Route 'bass'; other = Route 'other'; vocals = Route 'vocals' }
    technical = @{ segment_preroll_frames = 1024 }
}
[IO.File]::WriteAllText((Join-Path $fixture 'stemset.json'), ($manifest | ConvertTo-Json -Depth 6), $utf8)
Get-ChildItem $fixture -Filter source.wav -Recurse | Remove-Item -Force
foreach ($case in 'missing', 'corrupt', 'oversize-segment') {
    $casePath = Join-Path $root $case
    Remove-Item $casePath -Recurse -Force -ErrorAction SilentlyContinue
    Copy-Item $fixture $casePath -Recurse
}
Remove-Item (Join-Path $root 'missing/bass/segment-1.m4a')
[IO.File]::WriteAllBytes((Join-Path $root 'corrupt/bass/segment-0.m4a'), [byte[]](0, 1, 2, 3))
[IO.File]::WriteAllBytes((Join-Path $root 'oversize-segment/bass/segment-0.m4a'), [byte[]]::new(300000))

& cargo build --quiet -p stemplayer-probe --features hls
if ($LASTEXITCODE -ne 0) { throw 'Probe build failed' }
$certificate = Join-Path $root 'self-signed.crt'
$key = Join-Path $root 'self-signed.key'
$openssl = Get-Command openssl -ErrorAction SilentlyContinue
if (-not $openssl) {
    $openssl = @('C:\Program Files\Git\usr\bin\openssl.exe', 'C:\Program Files\Git\mingw64\bin\openssl.exe') | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $openssl) { throw 'OpenSSL is required for the TLS qualification gate' }
$ErrorActionPreference = 'Continue'
& $openssl req -x509 -newkey rsa:2048 -nodes -keyout $key -out $certificate -days 1 -subj '/CN=localhost' -addext 'subjectAltName=IP:127.0.0.1' 2>$null
$ErrorActionPreference = 'Stop'
if ($LASTEXITCODE -ne 0) { throw 'Failed to create TLS qualification certificate' }
$python = (Get-Command python -ErrorAction Stop).Source
$server = Start-Process -FilePath $python -ArgumentList @((Join-Path $workspace 'scripts/phase10_http_server.py'), $root, $Port) -WindowStyle Hidden -PassThru
$tlsPort = $Port + 1
$tlsServer = Start-Process -FilePath $python -ArgumentList @((Join-Path $workspace 'scripts/phase10_http_server.py'), $root, $tlsPort, '--certificate', $certificate, '--key', $key) -WindowStyle Hidden -PassThru
try {
    Start-Sleep -Milliseconds 500
    $url = "http://127.0.0.1:$Port/fixture/stemset.json"
    $probe = Join-Path $workspace 'target/debug/stemplayer-probe.exe'
    & $probe hls-validate $url | Set-Content -Encoding utf8 (Join-Path $root 'validation.json')
    if ($LASTEXITCODE -ne 0) { throw 'HLS validation failed' }
    & $probe hls-playback-sim $url | Set-Content -Encoding utf8 (Join-Path $root 'playback.json')
    if ($LASTEXITCODE -ne 0) { throw 'HLS playback simulation failed' }
    $playback = Get-Content -Raw (Join-Path $root 'playback.json') | ConvertFrom-Json
    if ($playback.frames -ne 192000 -or $playback.rendered_frames -ne 192000 -or $playback.end_frame -ne 192000) { throw 'HLS playback did not end on the exact frame' }
    if ($playback.diagnostics.inter_stem_offset_frames -ne 0 -or $playback.diagnostics.cache_bytes -gt 67108864) { throw 'HLS synchronization or cache bound failed' }
    if ($playback.diagnostics.fetch_failures -lt 1) { throw 'HLS bounded retry recovery was not exercised' }
    $security = [ordered]@{}
    $securityCases = [ordered]@{
        redirect = "http://127.0.0.1:$Port/redirect"
        oversize = "http://127.0.0.1:$Port/oversize"
        credentials = "http://user:secret@127.0.0.1:$Port/fixture/stemset.json"
        tls = "https://127.0.0.1:$tlsPort/fixture/stemset.json"
    }
    foreach ($case in $securityCases.GetEnumerator()) {
        $ErrorActionPreference = 'Continue'
        $output = & $probe hls-validate $case.Value 2>&1
        $ErrorActionPreference = 'Stop'
        if ($LASTEXITCODE -eq 0) { throw "HLS $($case.Key) security check unexpectedly passed" }
        $security[$case.Key] = ($output[-1].ToString() | ConvertFrom-Json).code
    }
    if ($security.redirect -ne 'E_HLS_REDIRECT_REJECTED' -or $security.oversize -ne 'E_HLS_RESPONSE_TOO_LARGE' -or $security.credentials -ne 'E_HLS_URL_UNSAFE' -or $security.tls -ne 'E_HLS_TLS_FAILED') { throw "HLS security error classification failed: $($security | ConvertTo-Json -Compress)" }
    $security | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $root 'security.json')
    $recovery = [ordered]@{}
    $recoveryCases = [ordered]@{ missing = 'E_HLS_HTTP_FAILED'; corrupt = 'E_HLS_CODEC_UNSUPPORTED'; 'oversize-segment' = 'E_HLS_RESPONSE_TOO_LARGE' }
    foreach ($case in $recoveryCases.GetEnumerator()) {
        $ErrorActionPreference = 'Continue'
        $output = & $probe hls-playback-sim "http://127.0.0.1:$Port/$($case.Key)/stemset.json" 2>&1
        $ErrorActionPreference = 'Stop'
        if ($LASTEXITCODE -eq 0) { throw "HLS $($case.Key) recovery check unexpectedly passed" }
        $recovery[$case.Key] = ($output[-1].ToString() | ConvertFrom-Json).code
        if ($recovery[$case.Key] -ne $case.Value) { throw "HLS $($case.Key) recovery classification failed: $($recovery[$case.Key])" }
    }
    $recovery | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $root 'recovery.json')
    if ($PrivateUrl) {
        & $probe hls-playback-sim $PrivateUrl | Set-Content -Encoding utf8 (Join-Path $root 'private-playback.json')
        if ($LASTEXITCODE -ne 0) { throw 'Private HLS playback simulation failed' }
    }
    [ordered]@{
        status = 'passed'
        url = 'http://127.0.0.1:<redacted>/fixture/stemset.json'
        frames = $playback.frames
        rendered_frames = $playback.rendered_frames
        end_frame = $playback.end_frame
        fetch_count = $playback.diagnostics.fetch_count
        fetch_failures = $playback.diagnostics.fetch_failures
        peak_cache_bytes = $playback.diagnostics.peak_cache_bytes
        discarded_preroll_frames = $playback.diagnostics.discarded_preroll_frames
        inter_stem_offset_frames = $playback.diagnostics.inter_stem_offset_frames
        private_fixture = [bool]$PrivateUrl
    } | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $root 'qualification.json')
} finally {
    Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue
    Stop-Process -Id $tlsServer.Id -Force -ErrorAction SilentlyContinue
}
