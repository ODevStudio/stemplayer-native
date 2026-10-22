param(
    [int]$Seconds = 30,
    [int]$SampleSeconds = 8,
    [string]$OutputDirectory = "artifacts/phase9/windows"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root $OutputDirectory
$profile = Join-Path $env:TEMP ("stemplayer-phase9-profile-" + [DateTime]::UtcNow.Ticks)
$fixture = Join-Path $env:TEMP ("stemplayer-phase9-source-" + [DateTime]::UtcNow.Ticks)
$oldAppData = $env:APPDATA
$oldLocalAppData = $env:LOCALAPPDATA
$oldBackend = $env:SLINT_BACKEND
$env:APPDATA = Join-Path $profile "roaming"
$env:LOCALAPPDATA = Join-Path $profile "local"
$env:SLINT_BACKEND = "winit-software"
$playerPath = Join-Path $root "target/release/stemplayer.exe"
$sampleRate = 48000
$frames = [int64]$sampleRate * $Seconds
$frequencies = [ordered]@{ drums = 110.0; bass = 220.0; other = 330.0; vocals = 440.0 }

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Phase9Window {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }
    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hWnd, out Rect rect);
    [DllImport("user32.dll")]
    public static extern bool MoveWindow(IntPtr hWnd, int x, int y, int width, int height, bool repaint);
    [DllImport("user32.dll")]
    public static extern uint GetDpiForWindow(IntPtr hWnd);
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);
}
"@

function Write-Text([IO.BinaryWriter]$Writer, [string]$Value) {
    $Writer.Write([Text.Encoding]::ASCII.GetBytes($Value))
}

function Write-Wave([string]$Path, [double[]]$Tones) {
    $stream = [IO.File]::Open($Path, [IO.FileMode]::Create, [IO.FileAccess]::Write)
    $writer = [IO.BinaryWriter]::new($stream)
    try {
        $dataBytes = [uint32]($frames * 4)
        Write-Text $writer "RIFF"
        $writer.Write([uint32](36 + $dataBytes))
        Write-Text $writer "WAVE"
        Write-Text $writer "fmt "
        $writer.Write([uint32]16)
        $writer.Write([uint16]1)
        $writer.Write([uint16]2)
        $writer.Write([uint32]$sampleRate)
        $writer.Write([uint32]($sampleRate * 4))
        $writer.Write([uint16]4)
        $writer.Write([uint16]16)
        Write-Text $writer "data"
        $writer.Write($dataBytes)
        for ($frame = 0; $frame -lt $frames; $frame++) {
            $sample = 0.0
            foreach ($tone in $Tones) {
                $sample += [Math]::Sin(2.0 * [Math]::PI * $tone * $frame / $sampleRate) * 0.04
            }
            $pcm = [int16][Math]::Round($sample * 32767.0)
            $writer.Write($pcm)
            $writer.Write($pcm)
        }
    } finally {
        $writer.Dispose()
        $stream.Dispose()
    }
}

function Wait-MainWindow([Diagnostics.Process]$Process) {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ([DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 50
        $Process.Refresh()
        if ($Process.MainWindowHandle -ne [IntPtr]::Zero) {
            return [Windows.Automation.AutomationElement]::FromHandle($Process.MainWindowHandle)
        }
    }
    throw "Native window was not created"
}

function Descendants([Windows.Automation.AutomationElement]$Window) {
    $Window.FindAll(
        [Windows.Automation.TreeScope]::Descendants,
        [Windows.Automation.Condition]::TrueCondition
    )
}

function Find-Name([Windows.Automation.AutomationElement]$Window, [string]$Name) {
    Descendants $Window | Where-Object { $_.Current.Name -eq $Name } | Select-Object -First 1
}

function Wait-Name([Windows.Automation.AutomationElement]$Window, [string]$Name) {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ([DateTime]::UtcNow -lt $deadline) {
        $match = Find-Name $Window $Name
        if ($match) { return $match }
        Start-Sleep -Milliseconds 50
    }
    throw "Accessible element missing: $Name"
}

function Save-WindowImage([Diagnostics.Process]$Process, [string]$Path) {
    [Phase9Window]::SetForegroundWindow($Process.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 250
    $rect = [Phase9Window+Rect]::new()
    if (-not [Phase9Window]::GetWindowRect($Process.MainWindowHandle, [ref]$rect)) {
        throw "Window bounds are unavailable"
    }
    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top
    $bitmap = [Drawing.Bitmap]::new($width, $height)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, [Drawing.Size]::new($width, $height))
        $bitmap.Save($Path, [Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

function Get-Sample([Diagnostics.Process]$Process, [Diagnostics.Stopwatch]$Clock, [double]$PreviousCpu) {
    $Process.Refresh()
    $io = Get-CimInstance Win32_Process -Filter "ProcessId=$($Process.Id)"
    [pscustomobject][ordered]@{
        elapsed_seconds = $Clock.Elapsed.TotalSeconds
        cpu_seconds = $Process.TotalProcessorTime.TotalSeconds
        cpu_delta_seconds = $Process.TotalProcessorTime.TotalSeconds - $PreviousCpu
        private_bytes = $Process.PrivateMemorySize64
        working_set_bytes = $Process.WorkingSet64
        read_bytes = [int64]$io.ReadTransferCount
        write_bytes = [int64]$io.WriteTransferCount
    }
}

function Wait-Cache(
    [Diagnostics.Process]$Process,
    [string]$Directory,
    [Diagnostics.Stopwatch]$Clock,
    [Collections.ArrayList]$Samples
) {
    $deadline = [DateTime]::UtcNow.AddSeconds([Math]::Max(20, [Math]::Min(58, $Seconds)))
    $previousCpu = 0.0
    while ([DateTime]::UtcNow -lt $deadline) {
        if (Test-Path -LiteralPath $Directory) {
            $entry = Get-ChildItem -LiteralPath $Directory -Filter "*.spwf" | Select-Object -First 1
            if ($entry) { return $Clock.Elapsed.TotalMilliseconds }
        }
        [void]$Samples.Add((Get-Sample $Process $Clock $previousCpu))
        $previousCpu = $Process.TotalProcessorTime.TotalSeconds
        Start-Sleep -Milliseconds 200
    }
    throw "Waveform cache write timed out"
}

function Sample-Until(
    [Diagnostics.Process]$Process,
    [Diagnostics.Stopwatch]$Clock,
    [Collections.ArrayList]$Samples,
    [int]$Seconds
) {
    $previousCpu = $Process.TotalProcessorTime.TotalSeconds
    while ($Clock.Elapsed.TotalSeconds -lt $Seconds -and -not $Process.HasExited) {
        Start-Sleep -Milliseconds 250
        [void]$Samples.Add((Get-Sample $Process $Clock $previousCpu))
        $previousCpu = $Process.TotalProcessorTime.TotalSeconds
    }
}

function Stop-Player([Diagnostics.Process]$Process) {
    if (-not $Process.WaitForExit(($Seconds + 5) * 1000)) {
        throw "Player did not finish"
    }
}

function Summarize-Samples([Collections.ArrayList]$Samples) {
    $cpu = @($Samples | ForEach-Object { [double]$_.cpu_delta_seconds * 400.0 } | Sort-Object)
    $last = $Samples[$Samples.Count - 1]
    [ordered]@{
        cpu_percent_one_core_p95 = if ($cpu.Count) { [Math]::Round($cpu[[Math]::Floor(($cpu.Count - 1) * 0.95)], 3) } else { 0.0 }
        peak_private_bytes = ($Samples | Measure-Object -Property private_bytes -Maximum).Maximum
        peak_working_set_bytes = ($Samples | Measure-Object -Property working_set_bytes -Maximum).Maximum
        read_bytes = $last.read_bytes
        write_bytes = $last.write_bytes
        samples = $Samples
    }
}

New-Item -ItemType Directory -Force -Path $output, $fixture, $env:APPDATA, $env:LOCALAPPDATA | Out-Null
$players = @()
try {
    foreach ($entry in $frequencies.GetEnumerator()) {
        Write-Wave (Join-Path $fixture ($entry.Key + ".wav")) @($entry.Value)
    }
    Write-Wave (Join-Path $fixture "master.wav") @($frequencies.Values)
    $roles = @($frequencies.Keys) + "master"
    $outputs = @($roles | ForEach-Object { [ordered]@{ logical_type = $_; filename = "$_`.wav" } })
    [IO.File]::WriteAllText(
        (Join-Path $fixture "manifest.json"),
        ([ordered]@{ schema_version = 2; outputs = $outputs } | ConvertTo-Json -Depth 4),
        [Text.UTF8Encoding]::new($false)
    )
    $settingsDirectory = Join-Path $env:APPDATA "stemplayer-native"
    New-Item -ItemType Directory -Force -Path $settingsDirectory | Out-Null
    [IO.File]::WriteAllText(
        (Join-Path $settingsDirectory "settings.json"),
        ([ordered]@{
            schema_version = 2
            diagnostics_visible = $true
            waveform_cache_enabled = $true
            waveform_cache_limit_mb = 64
            window_width = 900
            window_height = 760
        } | ConvertTo-Json),
        [Text.UTF8Encoding]::new($false)
    )

    Push-Location $root
    try {
        cargo build --release --locked -p stemplayer
        if ($LASTEXITCODE -ne 0) { throw "Player build failed" }
    } finally { Pop-Location }

    $missSnapshotPath = Join-Path $output "analysis-snapshot.json"
    $miss = Start-Process -FilePath $playerPath -ArgumentList @(
        "--play-source", ('"' + $fixture + '"'), 60000, ('"' + $missSnapshotPath + '"')
    ) -PassThru
    $players += $miss
    $missClock = [Diagnostics.Stopwatch]::StartNew()
    $missWindow = Wait-MainWindow $miss
    $missSamples = [Collections.ArrayList]::new()
    $cacheDirectory = Join-Path $env:LOCALAPPDATA "stemplayer-native/cache/waveforms-v1"
    $analysisReadyMs = Wait-Cache $miss $cacheDirectory $missClock $missSamples
    [Phase9Window]::SetForegroundWindow($miss.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 300
    $missWindow = [Windows.Automation.AutomationElement]::FromHandle($miss.MainWindowHandle)
    $dpi = [Phase9Window]::GetDpiForWindow($miss.MainWindowHandle)
    $scale = $dpi / 96.0
    $copy = Wait-Name $missWindow "Copy"
    $copy.SetFocus()
    $copyVisible = [bool]$copy
    $exportVisible = [bool](Wait-Name $missWindow "Export JSON")
    Save-WindowImage $miss (Join-Path $output "normal.png")
    [Phase9Window]::MoveWindow($miss.MainWindowHandle, 20, 20, [int](740 * $scale), [int](760 * $scale), $true) | Out-Null
    Start-Sleep -Milliseconds 400
    Save-WindowImage $miss (Join-Path $output "compact.png")
    Sample-Until $miss $missClock $missSamples $SampleSeconds
    Stop-Player $miss
    $cacheEntries = @(Get-ChildItem -LiteralPath $cacheDirectory -Filter "*.spwf")
    if ($cacheEntries.Count -ne 1) { throw "Expected one waveform cache entry" }
    $cacheText = [Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes($cacheEntries[0].FullName))
    $cachePrivate = -not $cacheText.Contains($fixture) -and -not $cacheText.Contains("drums.wav")

    $hitSnapshotPath = Join-Path $output "cached-snapshot.json"
    $hit = Start-Process -FilePath $playerPath -ArgumentList @(
        "--play-source", ('"' + $fixture + '"'), 60000, ('"' + $hitSnapshotPath + '"')
    ) -PassThru
    $players += $hit
    $hitClock = [Diagnostics.Stopwatch]::StartNew()
    $hitWindow = Wait-MainWindow $hit
    [Phase9Window]::MoveWindow($hit.MainWindowHandle, 20, 20, [int](900 * $scale), [int](760 * $scale), $true) | Out-Null
    Start-Sleep -Milliseconds 500
    $hitSamples = [Collections.ArrayList]::new()
    $cachedReadyMs = $hitClock.Elapsed.TotalMilliseconds
    Save-WindowImage $hit (Join-Path $output "cached-reopen.png")
    Sample-Until $hit $hitClock $hitSamples $SampleSeconds
    Stop-Player $hit

    $missSnapshot = Get-Content -Raw -LiteralPath $missSnapshotPath | ConvertFrom-Json
    $hitSnapshot = Get-Content -Raw -LiteralPath $hitSnapshotPath | ConvertFrom-Json
    $missSummary = Summarize-Samples $missSamples
    $hitSummary = Summarize-Samples $hitSamples
    $passed = $missSnapshot.details.underrun_count -eq 0 -and
        $missSnapshot.details.overflow_count -eq 0 -and
        $hitSnapshot.details.underrun_count -eq 0 -and
        $hitSnapshot.details.overflow_count -eq 0 -and
        $hitSnapshot.details.waveform_cache -eq "hit" -and
        $cachePrivate -and $copyVisible -and $exportVisible -and
        $missSummary.peak_private_bytes -le 78643200 -and
        $hitSummary.peak_private_bytes -le 78643200
    $report = [ordered]@{
        schema = "stemplayer-native-phase9-windows-v1"
        status = if ($passed) { "pass" } else { "fail" }
        timestamp_utc = [DateTime]::UtcNow.ToString("o")
        fixture = [ordered]@{ source_rate_hz = $sampleRate; duration_seconds = $Seconds; source_frames = $frames; master = $true }
        analysis = [ordered]@{
            ready_ms = [Math]::Round($analysisReadyMs, 3)
            cpu_percent_one_core_p95 = $missSummary.cpu_percent_one_core_p95
            read_bytes = $missSummary.read_bytes
            write_bytes = $missSummary.write_bytes
            peak_private_bytes = $missSummary.peak_private_bytes
            underruns = $missSnapshot.details.underrun_count
            overflows = $missSnapshot.details.overflow_count
        }
        cached_reopen = [ordered]@{
            ready_ms = [Math]::Round($cachedReadyMs, 3)
            cpu_percent_one_core_p95 = $hitSummary.cpu_percent_one_core_p95
            read_bytes = $hitSummary.read_bytes
            write_bytes = $hitSummary.write_bytes
            peak_private_bytes = $hitSummary.peak_private_bytes
            underruns = $hitSnapshot.details.underrun_count
            overflows = $hitSnapshot.details.overflow_count
        }
        analysis_overhead = [ordered]@{
            cpu_percent_one_core_p95 = [Math]::Round($missSummary.cpu_percent_one_core_p95 - $hitSummary.cpu_percent_one_core_p95, 3)
            read_bytes = $missSummary.read_bytes - $hitSummary.read_bytes
            write_bytes = $missSummary.write_bytes - $hitSummary.write_bytes
            private_bytes = $missSummary.peak_private_bytes - $hitSummary.peak_private_bytes
        }
        cache = [ordered]@{ entries = $cacheEntries.Count; bytes = $cacheEntries[0].Length; privacy = if ($cachePrivate) { "pass" } else { "fail" } }
        ui = [ordered]@{
            dpi = $dpi
            copy_action = $copyVisible
            export_action = $exportVisible
            screenshots = @("normal.png", "compact.png", "cached-reopen.png")
        }
        samples = [ordered]@{ analysis = $missSummary.samples; cached_reopen = $hitSummary.samples }
    }
    $report | ConvertTo-Json -Depth 10 | Set-Content -Encoding utf8 -LiteralPath (Join-Path $output "qualification.json")
    [ordered]@{
        status = $report.status
        analysis = $report.analysis
        cached_reopen = $report.cached_reopen
        analysis_overhead = $report.analysis_overhead
        cache = $report.cache
        ui = $report.ui
    } | ConvertTo-Json -Depth 5
    if (-not $passed) { exit 1 }
} finally {
    foreach ($running in $players) {
        if ($running -and -not $running.HasExited) {
            $running.Kill()
            $running.WaitForExit()
        }
    }
    $env:APPDATA = $oldAppData
    $env:LOCALAPPDATA = $oldLocalAppData
    $env:SLINT_BACKEND = $oldBackend
    Remove-Item -Recurse -Force -LiteralPath $profile, $fixture -ErrorAction SilentlyContinue
}
