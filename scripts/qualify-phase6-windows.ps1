param(
    [int]$Seconds = 30,
    [string]$OutputDirectory = "artifacts/phase6/windows"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root $OutputDirectory
$fixture = Join-Path $env:TEMP ("stemplayer-phase6-" + [DateTime]::UtcNow.Ticks)
$snapshotPath = Join-Path $output "playback-snapshot.json"
$resourcePath = Join-Path $output "resource-report.json"
$qualificationPath = Join-Path $output "qualification.json"
$player = Join-Path $root "target/release/stemplayer.exe"
$probe = Join-Path $root "target/release/stemplayer-probe.exe"
$allocationAudit = Join-Path $root "target/release/stemplayer-callback-audit.exe"
$sampleRate = 48000
$frames = [int64]$sampleRate * $Seconds
$frequencies = [ordered]@{
    drums = 110.0
    bass = 220.0
    other = 330.0
    vocals = 440.0
}

function Write-Text {
    param([IO.BinaryWriter]$Writer, [string]$Value)
    $Writer.Write([Text.Encoding]::ASCII.GetBytes($Value))
}

function Write-Wave {
    param(
        [string]$Path,
        [double[]]$Frequencies,
        [int]$Rate,
        [int64]$FrameCount
    )

    $stream = [IO.File]::Open($Path, [IO.FileMode]::Create, [IO.FileAccess]::Write)
    $writer = [IO.BinaryWriter]::new($stream)
    try {
        $dataBytes = [uint32]($FrameCount * 4)
        Write-Text $writer "RIFF"
        $writer.Write([uint32](36 + $dataBytes))
        Write-Text $writer "WAVE"
        Write-Text $writer "fmt "
        $writer.Write([uint32]16)
        $writer.Write([uint16]1)
        $writer.Write([uint16]2)
        $writer.Write([uint32]$Rate)
        $writer.Write([uint32]($Rate * 4))
        $writer.Write([uint16]4)
        $writer.Write([uint16]16)
        Write-Text $writer "data"
        $writer.Write($dataBytes)
        for ($frame = 0; $frame -lt $FrameCount; $frame++) {
            $sample = 0.0
            foreach ($frequency in $Frequencies) {
                $sample += [Math]::Sin(2.0 * [Math]::PI * $frequency * $frame / $Rate) * 0.04
            }
            $pcm = [int16][Math]::Round([Math]::Max(-1.0, [Math]::Min(1.0, $sample)) * 32767.0)
            $writer.Write($pcm)
            $writer.Write($pcm)
        }
    } finally {
        $writer.Dispose()
        $stream.Dispose()
    }
}

function Get-Percentile95 {
    param([double[]]$Values)
    if ($Values.Count -eq 0) {
        return 0.0
    }
    $sorted = @($Values | Sort-Object)
    return $sorted[[Math]::Floor(($sorted.Count - 1) * 0.95)]
}

New-Item -ItemType Directory -Force -Path $output, $fixture | Out-Null
try {
    foreach ($entry in $frequencies.GetEnumerator()) {
        Write-Wave -Path (Join-Path $fixture ($entry.Key + ".wav")) -Frequencies @($entry.Value) -Rate $sampleRate -FrameCount $frames
    }
    Write-Wave -Path (Join-Path $fixture "master.wav") -Frequencies @($frequencies.Values) -Rate $sampleRate -FrameCount $frames
    $roles = @($frequencies.Keys) + "master"
    $outputs = @($roles | ForEach-Object {
        [ordered]@{ logical_type = $_; filename = "$_`.wav" }
    })
    $manifest = [ordered]@{ schema_version = 2; outputs = $outputs } | ConvertTo-Json -Depth 4
    [IO.File]::WriteAllText(
        (Join-Path $fixture "manifest.json"),
        $manifest,
        [Text.UTF8Encoding]::new($false)
    )

    $timeoutMilliseconds = ($Seconds + 15) * 1000
    $arguments = @("--play-source", ('"' + $fixture + '"'), $timeoutMilliseconds, ('"' + $snapshotPath + '"'))
    $process = Start-Process -FilePath $player -ArgumentList $arguments -PassThru -WindowStyle Hidden
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $previousCpu = 0.0
    $previousTime = 0.0
    $samples = @()
    while (-not $process.HasExited) {
        Start-Sleep -Milliseconds 1000
        $process.Refresh()
        if ($process.HasExited) {
            break
        }
        $time = $clock.Elapsed.TotalSeconds
        $cpu = $process.TotalProcessorTime.TotalSeconds
        $elapsed = $time - $previousTime
        $samples += [pscustomobject][ordered]@{
            elapsed_seconds = [Math]::Round($time, 3)
            cpu_percent_one_core = [Math]::Round((($cpu - $previousCpu) / $elapsed) * 100.0, 3)
            private_bytes = $process.PrivateMemorySize64
            working_set_bytes = $process.WorkingSet64
        }
        $previousCpu = $cpu
        $previousTime = $time
    }
    $process.WaitForExit()
    $snapshot = Get-Content -Raw -LiteralPath $snapshotPath | ConvertFrom-Json
    $cpuP95 = Get-Percentile95 @($samples | ForEach-Object { [double]$_.cpu_percent_one_core })
    $peakPrivate = ($samples | Measure-Object -Property private_bytes -Maximum).Maximum
    $resourceReport = [ordered]@{
        schema = "stemplayer-phase6-resource-report-v1"
        timestamp_utc = [DateTime]::UtcNow.ToString("o")
        exit_code = $process.ExitCode
        wall_seconds = [Math]::Round($clock.Elapsed.TotalSeconds, 3)
        cpu_percent_one_core_p95 = $cpuP95
        peak_private_bytes = $peakPrivate
        samples = $samples
    }
    $resourceReport | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 -LiteralPath $resourcePath

    $allocation = & $allocationAudit | ConvertFrom-Json
    $simulated44100 = & $probe playback-sim $fixture --output-rate 44100 | ConvertFrom-Json
    $simulated96000 = & $probe playback-sim $fixture --output-rate 96000 | ConvertFrom-Json
    $resampler = $snapshot.details.resampler
    $expectedOutputFrames = [int64][Math]::Ceiling($frames * [double]$resampler.output_rate / $sampleRate)
    $hardwarePassed = $process.ExitCode -eq 0 -and
        $snapshot.transport_state -eq "ENDED" -and
        $snapshot.position.absolute_frame -eq $frames -and
        $resampler.input_rate -eq $sampleRate -and
        $resampler.output_rate -ne $sampleRate -and
        $resampler.input_frames -eq $frames -and
        $resampler.output_frames -eq $expectedOutputFrames -and
        $snapshot.details.underrun_count -eq 0 -and
        $snapshot.details.overflow_count -eq 0
    $resourcesPassed = $cpuP95 -le 5.0 -and $peakPrivate -le 78643200
    $simulatedPassed =
        $simulated44100.rendered_frames -eq ([int64][Math]::Ceiling($frames * 44100.0 / $sampleRate)) -and
        $simulated96000.rendered_frames -eq ([int64][Math]::Ceiling($frames * 96000.0 / $sampleRate))
    $passed = $hardwarePassed -and $resourcesPassed -and $allocation.status -eq "pass" -and $simulatedPassed
    $computer = Get-CimInstance Win32_ComputerSystem
    $processor = Get-CimInstance Win32_Processor | Select-Object -First 1
    $qualification = [ordered]@{
        schema = "stemplayer-phase6-qualification-v1"
        status = if ($passed) { "pass" } else { "fail" }
        timestamp_utc = [DateTime]::UtcNow.ToString("o")
        host = [ordered]@{
            os = [Environment]::OSVersion.VersionString
            architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
            cpu = $processor.Name
            logical_cores = $computer.NumberOfLogicalProcessors
            memory_bytes = $computer.TotalPhysicalMemory
            rustc = (& rustc --version)
        }
        fixture = [ordered]@{
            source_rate_hz = $sampleRate
            duration_seconds = $Seconds
            source_frames = $frames
            master = $true
        }
        hardware_playback = [ordered]@{
            result = if ($hardwarePassed) { "pass" } else { "fail" }
            device = $snapshot.details.device_name
            device_rate_hz = $resampler.output_rate
            resampler_delay_frames = $resampler.delay_frames
            rendered_device_frames = $resampler.output_frames
            expected_device_frames = $expectedOutputFrames
            final_source_frame = $snapshot.position.absolute_frame
            final_state = $snapshot.transport_state
            underruns = $snapshot.details.underrun_count
            overflows = $snapshot.details.overflow_count
            snapshot = "artifacts/phase6/windows/playback-snapshot.json"
        }
        resources = [ordered]@{
            result = if ($resourcesPassed) { "pass" } else { "fail" }
            cpu_percent_one_core_p95 = $cpuP95
            cpu_budget = 5.0
            peak_private_bytes = $peakPrivate
            private_memory_budget_bytes = 78643200
            report = "artifacts/phase6/windows/resource-report.json"
        }
        allocation_audit = $allocation
        simulated_rates = @($simulated44100, $simulated96000)
        listening = [ordered]@{ result = "pending"; requirement = "Operator confirmation of click-free gain, mute, solo, A/B, channel order, and device-change behavior" }
    }
    $qualification | ConvertTo-Json -Depth 12 | Set-Content -Encoding utf8 -LiteralPath $qualificationPath
    if (-not $passed) {
        exit 1
    }
} finally {
    Remove-Item -Recurse -Force -LiteralPath $fixture -ErrorAction SilentlyContinue
}
