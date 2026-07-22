param(
    [string]$OutputDirectory = "artifacts/phase0/windows",
    [string]$PrivateContainer = "",
    [int]$UiSeconds = 20
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root $OutputDirectory
New-Item -ItemType Directory -Force -Path $output | Out-Null
Push-Location $root
try {
    cargo build --release --locked
    $binary = Join-Path $root "target/release/stemplayer-phase0-probe.exe"
    $probeArguments = @("--self-check")
    if ($PrivateContainer) { $probeArguments += $PrivateContainer }
    & $binary @probeArguments | Set-Content -Encoding utf8 (Join-Path $output "self-check.json")

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new($binary)
    $startInfo.Arguments = "--ui-smoke $UiSeconds"
    $startInfo.UseShellExecute = $false
    $process = [System.Diagnostics.Process]::Start($startInfo)
    $started = [System.Diagnostics.Stopwatch]::StartNew()
    $previousCpu = [TimeSpan]::Zero
    $previousElapsed = 0.0
    $samples = while (-not $process.HasExited) {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
        if ($process.HasExited) { break }
        $elapsed = $started.Elapsed.TotalMilliseconds
        $cpu = $process.TotalProcessorTime
        $wallDelta = [Math]::Max(1.0, $elapsed - $previousElapsed)
        $cpuDelta = ($cpu - $previousCpu).TotalMilliseconds
        $previousCpu = $cpu
        $previousElapsed = $elapsed
        [ordered]@{
            elapsed_ms = [Math]::Round($elapsed, 3)
            private_bytes = $process.PrivateMemorySize64
            resident_bytes = $process.WorkingSet64
            cpu_normalized_percent = [Math]::Round(100 * $cpuDelta / $wallDelta / [Environment]::ProcessorCount, 4)
            handles = $process.HandleCount
        }
    }
    $process.WaitForExit()
    $computer = Get-CimInstance Win32_ComputerSystem
    $processor = Get-CimInstance Win32_Processor | Select-Object -First 1
    $os = Get-CimInstance Win32_OperatingSystem
    $report = [ordered]@{
        schema = "stemplayer-native-measurement-v1"
        status = if ($process.ExitCode -eq 0) { "passed" } else { "failed" }
        timestamp_utc = [DateTime]::UtcNow.ToString("o")
        build = [ordered]@{
            rustc = (rustc --version)
            profile = "release"
            binary_bytes = (Get-Item $binary).Length
            binary_sha256 = (Get-FileHash -Algorithm SHA256 $binary).Hash.ToLowerInvariant()
            lock_sha256 = (Get-FileHash -Algorithm SHA256 (Join-Path $root "Cargo.lock")).Hash.ToLowerInvariant()
        }
        machine = [ordered]@{
            os = "$($os.Caption) $($os.Version)"
            architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
            cpu = $processor.Name.Trim()
            logical_cores = [Environment]::ProcessorCount
            memory_bytes = [int64]$computer.TotalPhysicalMemory
            renderer = if ($env:SLINT_BACKEND) { $env:SLINT_BACKEND } else { "default" }
            audio_device = $null
            sample_rate = $null
        }
        scenario = "phase0-ui-idle"
        fixture = $null
        samples = @($samples)
        limitations = @("GPU process memory not measured", "Wakeups and timer activity not measured", "Minimized state not automated", "Screen-reader exposure requires manual Accessibility Insights and NVDA evidence")
    }
    $report | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 (Join-Path $output "ui-idle.json")
} finally {
    Pop-Location
}
