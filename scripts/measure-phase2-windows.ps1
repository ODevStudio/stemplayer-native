param(
    [string]$OutputPath = "artifacts/phase2/windows/resource-report.json",
    [int]$Seconds = 5
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root $OutputPath
$env:SLINT_BACKEND = "winit-software"

function Measure-Scenario([string]$Name, [string]$Argument) {
    $binary = Join-Path $root "target/release/stemplayer.exe"
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new($binary)
    $startInfo.Arguments = "$Argument $($Seconds * 1000)"
    $startInfo.UseShellExecute = $false
    $process = [System.Diagnostics.Process]::Start($startInfo)
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $previousCpu = [TimeSpan]::Zero
    $previousElapsed = 0.0
    $samples = while (-not $process.HasExited) {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
        if ($process.HasExited) { break }
        $elapsed = $watch.Elapsed.TotalMilliseconds
        $cpu = $process.TotalProcessorTime
        $wallDelta = [Math]::Max(1.0, $elapsed - $previousElapsed)
        $cpuDelta = ($cpu - $previousCpu).TotalMilliseconds
        $previousCpu = $cpu
        $previousElapsed = $elapsed
        [pscustomobject][ordered]@{
            elapsed_ms = [Math]::Round($elapsed, 3)
            private_bytes = $process.PrivateMemorySize64
            resident_bytes = $process.WorkingSet64
            cpu_normalized_percent = [Math]::Round(100 * $cpuDelta / $wallDelta / [Environment]::ProcessorCount, 4)
            handles = $process.HandleCount
        }
    }
    $process.WaitForExit()
    $steady = @($samples | Where-Object { $_.elapsed_ms -ge 1000 })
    [ordered]@{
        name = $Name
        exit_code = $process.ExitCode
        samples = @($samples)
        steady_max_private_bytes = ($steady | Measure-Object private_bytes -Maximum).Maximum
        steady_average_cpu_normalized_percent = [Math]::Round(($steady | Measure-Object cpu_normalized_percent -Average).Average, 4)
    }
}

Push-Location $root
try {
    cargo build --release --locked -p stemplayer --no-default-features --features development
    $binary = Join-Path $root "target/release/stemplayer.exe"
    $scenarios = @(
        Measure-Scenario "ready-idle" "--ui-idle"
        Measure-Scenario "playing" "--ui-smoke"
        Measure-Scenario "playing-minimized" "--ui-minimized"
    )
    $computer = Get-CimInstance Win32_ComputerSystem
    $processor = Get-CimInstance Win32_Processor | Select-Object -First 1
    $os = Get-CimInstance Win32_OperatingSystem
    $report = [ordered]@{
        schema = "stemplayer-native-phase2-resource-v1"
        status = if ($scenarios.exit_code -contains 1) { "failed" } else { "passed" }
        timestamp_utc = [DateTime]::UtcNow.ToString("o")
        build = [ordered]@{
            rustc = (rustc --version)
            profile = "release-development"
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
            renderer = $env:SLINT_BACKEND
        }
        scenarios = $scenarios
        limitations = @(
            "GPU allocations are not measured"
            "Windows timer wakeups require external ETW evidence"
            "Screen-reader exposure requires NVDA or Accessibility Insights evidence"
        )
    }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $output) | Out-Null
    $report | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 $output
    $report | ConvertTo-Json -Depth 3
} finally {
    Pop-Location
}
