param(
    [string]$OutputPath = "artifacts/phase3/windows/qualification.json",
    [int]$SoakSeconds = 3600,
    [int]$StressSeconds = 10
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root $OutputPath
$probe = Join-Path $root "target/release/stemplayer-probe.exe"
$allocationAudit = Join-Path $root "target/release/stemplayer-callback-audit.exe"

function Invoke-Json([string]$Binary, [string[]]$Arguments) {
    $result = & $Binary @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Binary failed with exit code $LASTEXITCODE"
    }
    $result | ConvertFrom-Json
}

function Measure-Output([string]$Name, [string[]]$Arguments) {
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new($probe)
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.CreateNoWindow = $true
    $startInfo.Arguments = $Arguments -join " "
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
    $stdout = $process.StandardOutput.ReadToEnd()
    $process.WaitForExit()
    $steady = @($samples | Where-Object { $_.elapsed_ms -ge 1000 })
    $firstPrivate = if ($steady.Count -gt 0) { $steady[0].private_bytes } else { 0 }
    $lastPrivate = if ($steady.Count -gt 0) { $steady[-1].private_bytes } else { 0 }
    [ordered]@{
        name = $Name
        exit_code = $process.ExitCode
        samples = @($samples)
        steady_max_private_bytes = ($steady | Measure-Object private_bytes -Maximum).Maximum
        steady_private_growth_bytes = $lastPrivate - $firstPrivate
        steady_average_cpu_normalized_percent = [Math]::Round(($steady | Measure-Object cpu_normalized_percent -Average).Average, 4)
        output = $stdout | ConvertFrom-Json
    }
}

Push-Location $root
try {
    cargo build --release --locked -p stemplayer-probe -p stemplayer-callback-audit
    if ($LASTEXITCODE -ne 0) { throw "Release build failed" }

    $devices = Invoke-Json $probe @("devices")
    $defaultDevice = $devices.devices | Where-Object is_default | Select-Object -First 1
    if ($null -eq $defaultDevice) { throw "No default output device" }

    $allocation = Invoke-Json $allocationAudit @()
    $playing = Measure-Output "playing" @("output-test", "--device", $defaultDevice.id, "--rate", "48000", "--seconds", "5")
    $pauseResume = Measure-Output "pause-resume" @("output-test", "--device", $defaultDevice.id, "--rate", "48000", "--seconds", "5", "--pause-cycles", "20")
    $reopen = Invoke-Json $probe @("device-reopen-test")

    $stressJobs = @()
    try {
        $workerCount = [Math]::Max(1, [Environment]::ProcessorCount - 1)
        $stressJobs = 1..$workerCount | ForEach-Object {
            Start-Job -ScriptBlock {
                param($Seconds)
                $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
                while ([DateTime]::UtcNow -lt $deadline) {
                    for ($index = 1; $index -le 10000; $index++) {
                        [Math]::Sqrt($index) | Out-Null
                    }
                }
            } -ArgumentList ($StressSeconds + 2)
        }
        $stressed = Measure-Output "cpu-stressed" @("output-test", "--device", $defaultDevice.id, "--rate", "48000", "--seconds", "$StressSeconds")
    } finally {
        $stressJobs | Stop-Job -ErrorAction SilentlyContinue
        $stressJobs | Remove-Job -Force -ErrorAction SilentlyContinue
    }

    $soak = Measure-Output "soak" @("output-test", "--device", $defaultDevice.id, "--rate", "48000", "--seconds", "$SoakSeconds")
    $scenarios = @($playing, $pauseResume, $stressed, $soak)
    $failedScenario = $scenarios | Where-Object {
        $_.exit_code -ne 0 -or
        $_.output.metrics.underflows -ne 0 -or
        $_.output.metrics.overflows -ne 0 -or
        $_.output.stream_errors -ne 0
    }
    $status = if (
        $allocation.status -eq "pass" -and
        $reopen.failed_cycles -eq 0 -and
        $reopen.underflows -eq 0 -and
        $reopen.overflows -eq 0 -and
        $reopen.stream_errors -eq 0 -and
        $null -eq $failedScenario
    ) { "passed" } else { "failed" }

    $computer = Get-CimInstance Win32_ComputerSystem
    $processor = Get-CimInstance Win32_Processor | Select-Object -First 1
    $os = Get-CimInstance Win32_OperatingSystem
    $report = [ordered]@{
        schema = "stemplayer-native-phase3-qualification-v1"
        status = $status
        timestamp_utc = [DateTime]::UtcNow.ToString("o")
        machine = [ordered]@{
            os = "$($os.Caption) $($os.Version)"
            architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
            cpu = $processor.Name.Trim()
            logical_cores = [Environment]::ProcessorCount
            memory_bytes = [int64]$computer.TotalPhysicalMemory
        }
        build = [ordered]@{
            rustc = (rustc --version)
            probe_bytes = (Get-Item $probe).Length
            probe_sha256 = (Get-FileHash -Algorithm SHA256 $probe).Hash.ToLowerInvariant()
            lock_sha256 = (Get-FileHash -Algorithm SHA256 (Join-Path $root "Cargo.lock")).Hash.ToLowerInvariant()
        }
        default_device = [ordered]@{
            id = $defaultDevice.id
            name = $defaultDevice.name
        }
        allocation_audit = $allocation
        reopen_test = $reopen
        scenarios = $scenarios
        limitations = @(
            "Device disconnect and default-device switch require an operator"
            "Suspend and resume require an operator"
            "Thread wakeups require external ETW evidence"
        )
    }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $output) | Out-Null
    $report | ConvertTo-Json -Depth 10 | Set-Content -Encoding utf8 $output
    [ordered]@{
        schema = $report.schema
        status = $report.status
        output = $output
        allocation_status = $allocation.status
        reopen_failures = $reopen.failed_cycles
        scenarios = @($scenarios | ForEach-Object {
            [ordered]@{
                name = $_.name
                underflows = $_.output.metrics.underflows
                overflows = $_.output.metrics.overflows
                stream_errors = $_.output.stream_errors
                callback_max_ns = $_.output.metrics.callback_max_ns
                private_growth_bytes = $_.steady_private_growth_bytes
            }
        })
    } | ConvertTo-Json -Depth 4
    if ($status -ne "passed") { exit 1 }
} finally {
    Pop-Location
}
