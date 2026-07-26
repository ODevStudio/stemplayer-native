param(
    [string]$OutputPath = "artifacts/phase3/windows/wakeup-report.json",
    [int]$Seconds = 10,
    [string]$DeviceId = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root $OutputPath
$probe = Join-Path $root "target/release/stemplayer-probe.exe"

function Get-ContextSwitches([int]$ProcessId) {
    $threads = Get-CimInstance Win32_PerfRawData_PerfProc_Thread -Filter "IDProcess = $ProcessId"
    [uint64](($threads | Measure-Object ContextSwitchesPersec -Sum).Sum)
}

Push-Location $root
try {
    $devices = & $probe devices | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw "Device enumeration failed" }
    $device = if ($DeviceId) {
        $devices.devices | Where-Object id -eq $DeviceId | Select-Object -First 1
    } else {
        $devices.devices | Where-Object is_default | Select-Object -First 1
    }
    if ($null -eq $device) { throw "Output device not found" }
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new($probe)
    $startInfo.Arguments = "output-test --device $($device.id) --rate 48000 --seconds $Seconds"
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.CreateNoWindow = $true
    $process = [System.Diagnostics.Process]::Start($startInfo)
    Start-Sleep -Seconds 1
    $startSwitches = Get-ContextSwitches $process.Id
    $samples = while (-not $process.HasExited) {
        Start-Sleep -Seconds 1
        if ($process.HasExited) { break }
        $process.Refresh()
        if ($process.HasExited) { break }
        [ordered]@{
            elapsed_seconds = [Math]::Round(([DateTime]::UtcNow - $process.StartTime.ToUniversalTime()).TotalSeconds, 3)
            context_switches = Get-ContextSwitches $process.Id
            private_bytes = $process.PrivateMemorySize64
            cpu_ms = $process.TotalProcessorTime.TotalMilliseconds
        }
    }
    $endSwitches = if ($samples.Count -gt 0) { $samples[-1].context_switches } else { $startSwitches }
    $stdout = $process.StandardOutput.ReadToEnd()
    $process.WaitForExit()
    $report = [ordered]@{
        schema = "stemplayer-native-phase3-wakeup-v1"
        status = if ($process.ExitCode -eq 0) { "passed" } else { "failed" }
        timestamp_utc = [DateTime]::UtcNow.ToString("o")
        duration_seconds = $Seconds
        context_switches = $endSwitches - $startSwitches
        context_switches_per_second = [Math]::Round(($endSwitches - $startSwitches) / [Math]::Max(1, $Seconds - 2), 3)
        samples = @($samples)
        output = $stdout | ConvertFrom-Json
        limitation = "Raw process-thread context switches are a wakeup proxy; kernel ETW requires administrator policy"
    }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $output) | Out-Null
    $report | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 $output
    $report | ConvertTo-Json -Depth 3
    if ($process.ExitCode -ne 0) { exit 1 }
} finally {
    Pop-Location
}
