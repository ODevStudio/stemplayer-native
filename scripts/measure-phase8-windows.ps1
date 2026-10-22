param(
    [string]$OutputPath = "artifacts/phase8/windows/resource-report.json",
    [ValidateRange(6, 60)]
    [int]$Seconds = 6
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot 'qualification-helpers.ps1')
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root $OutputPath
$profile = Join-Path $env:TEMP ("stemplayer-phase8-resource-" + [DateTime]::UtcNow.Ticks)
$oldAppData = $env:APPDATA
$oldLocalAppData = $env:LOCALAPPDATA
$oldBackend = $env:SLINT_BACKEND
$oldPerformance = $env:SLINT_DEBUG_PERFORMANCE
$env:APPDATA = Join-Path $profile "roaming"
$env:LOCALAPPDATA = Join-Path $profile "local"
$env:SLINT_BACKEND = "winit-software"
$env:SLINT_DEBUG_PERFORMANCE = "refresh_lazy,console"

function Get-Percentile95([double[]]$Values) {
    if ($Values.Count -eq 0) { return $null }
    $sorted = @($Values | Sort-Object)
    $sorted[[Math]::Floor(($sorted.Count - 1) * 0.95)]
}

function Measure-Scenario([string]$Name, [string]$Argument) {
    $binary = Join-Path $root "target/release/stemplayer.exe"
    $startInfo = [Diagnostics.ProcessStartInfo]::new($binary)
    $startInfo.Arguments = "$Argument $($Seconds * 1000)"
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($startInfo)
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $watch = [Diagnostics.Stopwatch]::StartNew()
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
            cpu_percent_one_core = [Math]::Round(100 * $cpuDelta / $wallDelta, 3)
            handles = $process.HandleCount
        }
    }
    $process.WaitForExit()
    $stderr = $stderrTask.GetAwaiter().GetResult()
    $steady = @($samples | Where-Object { $_.elapsed_ms -ge 1000 })
    $fps = @([regex]::Matches($stderr, "average frames per second: (\d+)") | ForEach-Object { [int]$_.Groups[1].Value })
    $steadyFps = @($fps | Select-Object -Skip 1)
    $children = @(Get-CimInstance Win32_Process | Where-Object { $_.ParentProcessId -eq $process.Id } | ForEach-Object Name)
    [ordered]@{
        name = $Name
        exit_code = $process.ExitCode
        samples = @($samples)
        evidence_complete = Test-ResourceEvidence $steady $steadyFps
        steady_max_private_bytes = ($steady | Measure-Object private_bytes -Maximum).Maximum
        steady_cpu_percent_one_core_average = if ($steady.Count) { [Math]::Round(($steady | Measure-Object cpu_percent_one_core -Average).Average, 3) } else { $null }
        steady_cpu_percent_one_core_p95 = Get-Percentile95 @($steady | ForEach-Object { [double]$_.cpu_percent_one_core })
        reported_frames_per_second = $fps
        reported_frames_per_second_max = ($steadyFps | Measure-Object -Maximum).Maximum
        child_processes = $children
    }
}

New-Item -ItemType Directory -Force -Path (Split-Path -Parent $output), $env:APPDATA, $env:LOCALAPPDATA | Out-Null
Push-Location $root
try {
    cargo build --release --locked -p stemplayer --no-default-features --features development
    if ($LASTEXITCODE -ne 0) { throw "Development player build failed" }
    $binary = Join-Path $root "target/release/stemplayer.exe"
    $scenarios = @(
        Measure-Scenario "ready-idle" "--ui-idle"
        Measure-Scenario "playing" "--ui-smoke"
        Measure-Scenario "playing-minimized" "--ui-minimized"
        Measure-Scenario "paused-minimized" "--ui-paused-minimized"
    )
    $computer = Get-CimInstance Win32_ComputerSystem
    $processor = Get-CimInstance Win32_Processor | Select-Object -First 1
    $os = Get-CimInstance Win32_OperatingSystem
    $idle = $scenarios | Where-Object { $_.name -eq "ready-idle" }
    $playing = $scenarios | Where-Object { $_.name -eq "playing" }
    $playingMinimized = $scenarios | Where-Object { $_.name -eq "playing-minimized" }
    $pausedMinimized = $scenarios | Where-Object { $_.name -eq "paused-minimized" }
    $nativeTree = @($scenarios.child_processes | Where-Object { $_ -match "(?i)(chrome|edge|webview|node|electron)" }).Count -eq 0
    $passed = @($scenarios | Where-Object { $_.exit_code -ne 0 -or -not $_.evidence_complete }).Count -eq 0 -and
        $idle.steady_max_private_bytes -le 52428800 -and
        $idle.steady_cpu_percent_one_core_p95 -le 1.0 -and
        $playing.steady_cpu_percent_one_core_average -le 1.5 -and
        $idle.reported_frames_per_second_max -le 1 -and
        $playing.reported_frames_per_second_max -le 4 -and
        $playingMinimized.reported_frames_per_second_max -le 1 -and
        $pausedMinimized.reported_frames_per_second_max -le 1 -and
        $nativeTree
    $report = [ordered]@{
        schema = "stemplayer-native-phase8-resource-v1"
        status = if ($passed) { "passed" } else { "failed" }
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
            architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
            cpu = $processor.Name.Trim()
            logical_cores = [Environment]::ProcessorCount
            memory_bytes = [int64]$computer.TotalPhysicalMemory
            renderer = $env:SLINT_BACKEND
        }
        budgets = [ordered]@{
            idle_private_bytes = 52428800
            idle_cpu_percent_one_core_p95 = 1.0
            playing_ui_cpu_percent_one_core_average = 1.5
            idle_or_minimized_repaints_per_second = 1
            playing_repaints_per_second = 4
        }
        scenarios = $scenarios
        native_process_tree = $nativeTree
        scope = "Deterministic mock backend isolates native UI overhead from decoder and audio-device CPU"
    }
    $report | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 $output
    $report | ConvertTo-Json -Depth 3
    if (-not $passed) { exit 1 }
} finally {
    Pop-Location
    $env:APPDATA = $oldAppData
    $env:LOCALAPPDATA = $oldLocalAppData
    $env:SLINT_BACKEND = $oldBackend
    $env:SLINT_DEBUG_PERFORMANCE = $oldPerformance
    Remove-Item -Recurse -Force -LiteralPath $profile -ErrorAction SilentlyContinue
}
