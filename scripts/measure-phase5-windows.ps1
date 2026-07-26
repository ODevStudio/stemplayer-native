param(
    [Parameter(Mandatory = $true)]
    [string]$Source,
    [string]$Output = "artifacts/phase5/windows/resource-report.json"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root "target/release/stemplayer.exe"
$outputPath = Join-Path $root $Output
$outputDirectory = Split-Path -Parent $outputPath
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
$snapshotPath = Join-Path $env:TEMP "stemplayer-phase5-resource-snapshot.json"
$arguments = @(
    "--play-source",
    ('"' + $Source + '"'),
    "120000",
    ('"' + $snapshotPath + '"')
)
$process = Start-Process -FilePath $exe -ArgumentList $arguments -PassThru -WindowStyle Hidden
$clock = [Diagnostics.Stopwatch]::StartNew()
$previousCpu = 0.0
$previousTime = 0.0
$samples = @()
while (-not $process.HasExited) {
    Start-Sleep -Milliseconds 1000
    $process.Refresh()
    $time = $clock.Elapsed.TotalSeconds
    $cpu = $process.TotalProcessorTime.TotalSeconds
    $elapsed = $time - $previousTime
    $cpuPercent = if ($elapsed -gt 0) { (($cpu - $previousCpu) / $elapsed) * 100.0 } else { 0.0 }
    $samples += [pscustomobject][ordered]@{
        elapsed_seconds = [Math]::Round($time, 3)
        cpu_percent_one_core = [Math]::Round($cpuPercent, 3)
        private_bytes = $process.PrivateMemorySize64
        working_set_bytes = $process.WorkingSet64
    }
    $previousCpu = $cpu
    $previousTime = $time
}
$process.WaitForExit()
$cpuValues = @($samples | ForEach-Object { [double]$_.cpu_percent_one_core } | Sort-Object)
$p95Index = if ($cpuValues.Count -eq 0) { 0 } else { [Math]::Floor(($cpuValues.Count - 1) * 0.95) }
$finalSnapshot = if (Test-Path -LiteralPath $snapshotPath) {
    Get-Content -Raw -LiteralPath $snapshotPath | ConvertFrom-Json
} else {
    $null
}
$report = [ordered]@{
    schema = "stemplayer-phase5-resource-report-v1"
    timestamp_utc = [DateTime]::UtcNow.ToString("o")
    source_display_name = Split-Path -Leaf $Source
    executable = $exe
    exit_code = $process.ExitCode
    wall_seconds = [Math]::Round($clock.Elapsed.TotalSeconds, 3)
    sample_count = $samples.Count
    cpu_percent_one_core_p95 = if ($cpuValues.Count -eq 0) { 0.0 } else { $cpuValues[$p95Index] }
    cpu_percent_one_core_average = if ($samples.Count -eq 0) { 0.0 } else {
        [Math]::Round((($samples | Measure-Object -Property cpu_percent_one_core -Average).Average), 3)
    }
    peak_private_bytes = if ($samples.Count -eq 0) { 0 } else {
        ($samples | Measure-Object -Property private_bytes -Maximum).Maximum
    }
    peak_working_set_bytes = if ($samples.Count -eq 0) { 0 } else {
        ($samples | Measure-Object -Property working_set_bytes -Maximum).Maximum
    }
    final_snapshot = $finalSnapshot
    samples = $samples
}
$report | ConvertTo-Json -Depth 10 | Set-Content -Encoding utf8 -LiteralPath $outputPath
