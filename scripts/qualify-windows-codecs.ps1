param(
    [string]$OutputDirectory = "artifacts/phase0/windows-codecs"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root $OutputDirectory
$temporary = Join-Path $root "artifacts/tmp"
New-Item -ItemType Directory -Force -Path $output, $temporary | Out-Null

function New-CodecFixture {
    param(
        [string]$Codec,
        [int]$Seconds
    )

    $path = Join-Path $temporary "synthetic-$Codec-$Seconds.m4a"
    $source = "aevalsrc=sin(2*PI*440*t)|sin(2*PI*660*t):s=44100:d=$Seconds"
    $arguments = @("-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", $source)
    if ($Codec -eq "aac") {
        $arguments += @("-c:a", "aac", "-b:a", "192k")
    } else {
        $arguments += @("-c:a", "alac")
    }
    $arguments += @("-movflags", "+faststart", "-y", $path)
    & ffmpeg @arguments
    if ($LASTEXITCODE -ne 0) {
        throw "FFmpeg failed for $Codec $Seconds seconds"
    }
    $path
}

function Invoke-CodecProbe {
    param(
        [string]$Codec,
        [int]$Seconds,
        [string]$Path,
        [string]$Binary
    )

    $expectedFrames = 44100 * $Seconds
    $seekFrame = [int64]($expectedFrames / 2) + 17
    $quote = [char]34
    $startInfo = [Diagnostics.ProcessStartInfo]::new($Binary)
    $startInfo.Arguments = "--codec-check $quote$Path$quote $expectedFrames $seekFrame"
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($startInfo)
    $peakPrivate = 0L
    $peakResident = 0L
    while (-not $process.HasExited) {
        $process.Refresh()
        if (-not $process.HasExited) {
            $peakPrivate = [Math]::Max($peakPrivate, $process.PrivateMemorySize64)
            $peakResident = [Math]::Max($peakResident, $process.WorkingSet64)
        }
        Start-Sleep -Milliseconds 1
    }
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    if ($process.ExitCode -ne 0) {
        throw "Codec probe failed: $stderr"
    }
    $qualification = $stdout | ConvertFrom-Json
    [ordered]@{
        codec = $Codec
        duration_seconds = $Seconds
        fixture_bytes = (Get-Item $Path).Length
        fixture_sha256 = (Get-FileHash -Algorithm SHA256 $Path).Hash.ToLowerInvariant()
        expected_frames = $expectedFrames
        peak_private_bytes = $peakPrivate
        peak_resident_bytes = $peakResident
        qualification = $qualification
    }
}

Push-Location $root
try {
    cargo build --release --locked
    $binary = Join-Path $root "target/release/stemplayer-phase0-probe.exe"
    $results = @(
        foreach ($codec in @("aac", "alac")) {
            foreach ($seconds in @(10, 60)) {
                $fixture = New-CodecFixture -Codec $codec -Seconds $seconds
                Invoke-CodecProbe -Codec $codec -Seconds $seconds -Path $fixture -Binary $binary
            }
        }
    )
    $memoryGrowth = @(
        foreach ($codec in @("aac", "alac")) {
            $short = $results | Where-Object { $_.codec -eq $codec -and $_.duration_seconds -eq 10 }
            $long = $results | Where-Object { $_.codec -eq $codec -and $_.duration_seconds -eq 60 }
            [ordered]@{
                codec = $codec
                private_growth_bytes = $long.peak_private_bytes - $short.peak_private_bytes
                within_16_mib = ($long.peak_private_bytes - $short.peak_private_bytes) -le 16MB
            }
        }
    )
    $passed = ($results | Where-Object { $_.qualification.status -ne "pass" }).Count -eq 0 -and
        ($memoryGrowth | Where-Object { -not $_.within_16_mib }).Count -eq 0
    $report = [ordered]@{
        schema = "stemplayer-windows-codec-qualification-v1"
        status = if ($passed) { "passed" } else { "failed" }
        timestamp_utc = [DateTime]::UtcNow.ToString("o")
        rustc = (rustc --version)
        ffmpeg = (& ffmpeg -version | Select-Object -First 1)
        fixture_generator = "stereo 440/660 Hz sine, 44.1 kHz, deterministic duration"
        results = $results
        duration_memory_comparison = $memoryGrowth
        limitations = @(
            "FFmpeg generates qualification fixtures but is not a native build or runtime dependency",
            "Phase 7 retains the broader adversarial fixture matrix"
        )
    }
    $report | ConvertTo-Json -Depth 10 | Set-Content -Encoding utf8 (Join-Path $output "codec-qualification.json")
    if (-not $passed) {
        exit 1
    }
} finally {
    Pop-Location
}
