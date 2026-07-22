param(
    [string]$OutputDirectory = "artifacts/phase0/windows-ui",
    [int]$StateSeconds = 5
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root $OutputDirectory
New-Item -ItemType Directory -Force -Path $output | Out-Null

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class NativeWindow {
    [DllImport("user32.dll")]
    public static extern bool ShowWindowAsync(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")]
    public static extern uint GetDpiForWindow(IntPtr hWnd);
}
"@

function Wait-MainWindow {
    param([Diagnostics.Process]$Process)

    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while ([DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 100
        $Process.Refresh()
        if ($Process.MainWindowHandle -ne [IntPtr]::Zero) {
            return $Process.MainWindowHandle
        }
    }
    throw "Native window was not created"
}

function Get-AccessibilityTree {
    param([IntPtr]$Handle)

    $window = [Windows.Automation.AutomationElement]::FromHandle($Handle)
    $elements = $window.FindAll(
        [Windows.Automation.TreeScope]::Descendants,
        [Windows.Automation.Condition]::TrueCondition
    )
    @(
        [pscustomobject][ordered]@{
            name = $window.Current.Name
            control_type = $window.Current.ControlType.ProgrammaticName
            keyboard_focusable = $window.Current.IsKeyboardFocusable
            enabled = $window.Current.IsEnabled
        }
        foreach ($element in $elements) {
            [pscustomobject][ordered]@{
                name = $element.Current.Name
                control_type = $element.Current.ControlType.ProgrammaticName
                keyboard_focusable = $element.Current.IsKeyboardFocusable
                enabled = $element.Current.IsEnabled
            }
        }
    )
}

function Get-GpuMemory {
    param([int]$ProcessId)

    try {
        $counters = Get-Counter @(
            "\GPU Process Memory(*)\Dedicated Usage",
            "\GPU Process Memory(*)\Shared Usage"
        ) -ErrorAction Stop
        $matching = $counters.CounterSamples | Where-Object {
            $_.InstanceName -like "pid_$($ProcessId)_*"
        }
        [pscustomobject][ordered]@{
            available = $true
            dedicated_bytes = [int64](($matching | Where-Object { $_.Path -like "*Dedicated Usage" } | Measure-Object CookedValue -Sum).Sum)
            shared_bytes = [int64](($matching | Where-Object { $_.Path -like "*Shared Usage" } | Measure-Object CookedValue -Sum).Sum)
        }
    } catch {
        [pscustomobject][ordered]@{
            available = $false
            dedicated_bytes = $null
            shared_bytes = $null
        }
    }
}

function Measure-State {
    param(
        [Diagnostics.Process]$Process,
        [string]$Name,
        [int]$Seconds
    )

    $watch = [Diagnostics.Stopwatch]::StartNew()
    $previousCpu = $Process.TotalProcessorTime
    $previousElapsed = 0.0
    $samples = while ($watch.Elapsed.TotalSeconds -lt $Seconds -and -not $Process.HasExited) {
        Start-Sleep -Milliseconds 250
        $Process.Refresh()
        if ($Process.HasExited) {
            break
        }
        $elapsed = $watch.Elapsed.TotalMilliseconds
        $cpu = $Process.TotalProcessorTime
        $wallDelta = [Math]::Max(1.0, $elapsed - $previousElapsed)
        $cpuDelta = ($cpu - $previousCpu).TotalMilliseconds
        $previousCpu = $cpu
        $previousElapsed = $elapsed
        $threads = Get-CimInstance Win32_PerfFormattedData_PerfProc_Thread |
            Where-Object { $_.IDProcess -eq $Process.Id }
        [pscustomobject][ordered]@{
            elapsed_ms = [Math]::Round($elapsed, 3)
            private_bytes = $Process.PrivateMemorySize64
            resident_bytes = $Process.WorkingSet64
            cpu_normalized_percent = [Math]::Round(100 * $cpuDelta / $wallDelta / [Environment]::ProcessorCount, 4)
            handles = $Process.HandleCount
            threads = $Process.Threads.Count
            context_switches_per_second = [int64](($threads | Measure-Object ContextSwitchesPersec -Sum).Sum)
        }
    }
    $cpuAverage = ($samples | Measure-Object cpu_normalized_percent -Average).Average
    $contextAverage = ($samples | Measure-Object context_switches_per_second -Average).Average
    [pscustomobject][ordered]@{
        name = $Name
        cpu_normalized_average_percent = [Math]::Round($cpuAverage, 4)
        context_switches_average_per_second = [Math]::Round($contextAverage, 2)
        gpu_memory = Get-GpuMemory -ProcessId $Process.Id
        samples = @($samples)
    }
}

Push-Location $root
try {
    cargo build --release --locked
    $binary = Join-Path $root "target/release/stemplayer-phase0-probe.exe"
    $totalSeconds = 3 * $StateSeconds + 120
    $process = Start-Process -FilePath $binary -ArgumentList "--ui-smoke", $totalSeconds -PassThru
    $handle = Wait-MainWindow -Process $process
    $accessibility = Get-AccessibilityTree -Handle $handle
    $dpi = [NativeWindow]::GetDpiForWindow($handle)
    $themeKey = Get-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"
    $idle = Measure-State -Process $process -Name "visible-idle" -Seconds $StateSeconds
    [NativeWindow]::ShowWindowAsync($handle, 6) | Out-Null
    Start-Sleep -Seconds 1
    $minimized = Measure-State -Process $process -Name "minimized" -Seconds $StateSeconds
    [NativeWindow]::ShowWindowAsync($handle, 9) | Out-Null
    Start-Sleep -Seconds 1
    $restored = Measure-State -Process $process -Name "restored" -Seconds $StateSeconds
    if (-not $process.HasExited) {
        $process.Kill()
    }
    $process.WaitForExit()
    $names = @($accessibility | ForEach-Object { $_.name })
    $requiredNames = @(
        "Waveform placeholder",
        "Drums gain",
        "Bass gain",
        "Other gain",
        "Vocals gain",
        "Play",
        "Stop"
    )
    $missingNames = @($requiredNames | Where-Object { $_ -notin $names })
    $focusable = @($accessibility | Where-Object { $_.keyboard_focusable }).Count
    $passed = $missingNames.Count -eq 0 -and $focusable -ge 10 -and $dpi -ge 96
    $report = [pscustomobject][ordered]@{
        schema = "stemplayer-windows-ui-qualification-v1"
        status = if ($passed) { "passed" } else { "failed" }
        timestamp_utc = [DateTime]::UtcNow.ToString("o")
        build = [pscustomobject][ordered]@{
            rustc = (rustc --version)
            binary_bytes = (Get-Item $binary).Length
            binary_sha256 = (Get-FileHash -Algorithm SHA256 $binary).Hash.ToLowerInvariant()
        }
        display = [pscustomobject][ordered]@{
            dpi = $dpi
            scale_percent = [Math]::Round(100 * $dpi / 96, 2)
            system_apps_theme = if ($themeKey.AppsUseLightTheme -eq 0) { "dark" } else { "light" }
            system_transparency = [bool]$themeKey.EnableTransparency
        }
        accessibility = [pscustomobject][ordered]@{
            provider = "Windows UI Automation"
            status = if ($missingNames.Count -eq 0 -and $focusable -ge 10) { "passed" } else { "failed" }
            missing_required_names = $missingNames
            keyboard_focusable_elements = $focusable
            elements = $accessibility
        }
        states = @($idle, $minimized, $restored)
        renderer = "winit-software"
        limitations = @(
            "Windows UI Automation proves screen-reader exposure; human NVDA task completion remains a Phase 12 optional qualification",
            "DPI evidence covers the connected display scale",
            "Theme evidence covers the active system theme",
            "Context switches per second are the Windows wakeup proxy",
            "The rejected FemtoVG renderer retains its separate Phase 0 memory report"
        )
    }
    $report | ConvertTo-Json -Depth 12 | Set-Content -Encoding utf8 (Join-Path $output "ui-qualification.json")
    if (-not $passed) {
        exit 1
    }
} finally {
    Pop-Location
}
