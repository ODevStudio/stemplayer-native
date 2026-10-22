param(
    [string]$OutputDirectory = "artifacts/phase8/windows",
    [ValidateRange(120, 600)]
    [int]$TimeoutSeconds = 120
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root $OutputDirectory
$profile = Join-Path $env:TEMP ("stemplayer-phase8-ui-" + [DateTime]::UtcNow.Ticks)
$oldAppData = $env:APPDATA
$oldLocalAppData = $env:LOCALAPPDATA
$oldBackend = $env:SLINT_BACKEND
$env:APPDATA = Join-Path $profile "roaming"
$env:LOCALAPPDATA = Join-Path $profile "local"
$env:SLINT_BACKEND = "winit-software"
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Phase8Window {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }
    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hWnd, out Rect rect);
    [DllImport("user32.dll")]
    public static extern bool MoveWindow(IntPtr hWnd, int x, int y, int width, int height, bool repaint);
    [DllImport("user32.dll")]
    public static extern uint GetDpiForWindow(IntPtr hWnd);
}
"@

function Wait-MainWindow([Diagnostics.Process]$Process) {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ([DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 100
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

function Wait-Name([Windows.Automation.AutomationElement]$Window, [string]$Name) {
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    while ([DateTime]::UtcNow -lt $deadline) {
        $match = Descendants $Window | Where-Object { $_.Current.Name -eq $Name } | Select-Object -First 1
        if ($match) { return $match }
        Start-Sleep -Milliseconds 100
    }
    throw "Accessible element missing: $Name"
}

function Wait-NamePrefix([Windows.Automation.AutomationElement]$Window, [string]$Prefix) {
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    while ([DateTime]::UtcNow -lt $deadline) {
        $match = Descendants $Window | Where-Object { $_.Current.Name.StartsWith($Prefix) } | Select-Object -First 1
        if ($match) { return $match }
        Start-Sleep -Milliseconds 100
    }
    throw "Accessible element missing: $Prefix"
}

function Wait-Control([Windows.Automation.AutomationElement]$Window, [string]$Name, [Windows.Automation.ControlType]$Type) {
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    while ([DateTime]::UtcNow -lt $deadline) {
        $match = Descendants $Window | Where-Object { $_.Current.Name -eq $Name -and $_.Current.ControlType -eq $Type } | Select-Object -First 1
        if ($match) { return $match }
        Start-Sleep -Milliseconds 100
    }
    throw "Accessible control missing: $Name"
}

function Wait-PlaybackSlider([Windows.Automation.AutomationElement]$Window) {
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    while ([DateTime]::UtcNow -lt $deadline) {
        $match = Descendants $Window | Where-Object {
            $_.Current.Name.EndsWith('. Playback position') -and
            $_.Current.ControlType -eq [Windows.Automation.ControlType]::Slider
        } | Select-Object -First 1
        if ($match) { return $match }
        Start-Sleep -Milliseconds 100
    }
    throw 'Accessible playback slider missing'
}

function Get-PlayerControlNames {
    @('Open track folder', 'Open network URL', 'Player settings', 'Play', 'Reset mixer')
    foreach ($role in 'Drums', 'Bass', 'Other', 'Vocals') {
        "$role gain"
        "$role enabled"
    }
}

function Get-SettingsControlNames {
    @('Output device', 'Theme', 'Recent source', 'Remember', 'Waveform cache', 'Waveform cache limit', 'Diagnostics')
}

function Test-MasterRouting([string]$Status, [bool]$ExpectedMaster) {
    $mode = if ($ExpectedMaster) { 'Master reference' } else { 'Stem mix' }
    $Status.Contains(". Automatic: $mode.")
}

function Send-Key([string]$Keys) {
    [System.Windows.Forms.SendKeys]::SendWait($Keys)
    Start-Sleep -Milliseconds 250
}

function Tab-Through([Windows.Automation.AutomationElement]$Element, [int]$Count) {
    $Element.SetFocus()
    for ($index = 0; $index -lt $Count; $index++) {
        [System.Windows.Forms.SendKeys]::SendWait("{TAB}")
        Start-Sleep -Milliseconds 40
    }
}

function Invoke-Element([Windows.Automation.AutomationElement]$Element) {
    $pattern = $Element.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern)
    $pattern.Invoke()
    Start-Sleep -Milliseconds 250
}

function Toggle-Element([Windows.Automation.AutomationElement]$Element) {
    $pattern = $Element.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern)
    $pattern.Toggle()
    Start-Sleep -Milliseconds 150
    return $pattern.Current.ToggleState
}

function Save-WindowImage([Diagnostics.Process]$Process, [string]$Path) {
    $rect = [Phase8Window+Rect]::new()
    if (-not [Phase8Window]::GetWindowRect($Process.MainWindowHandle, [ref]$rect)) {
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

function Start-Player([string[]]$Arguments) {
    Start-Process -FilePath (Join-Path $root "target/release/stemplayer.exe") -ArgumentList $Arguments -PassThru
}

New-Item -ItemType Directory -Force -Path $output, $env:APPDATA, $env:LOCALAPPDATA | Out-Null
Push-Location $root
$players = @()
try {
    cargo build --release --locked -p stemplayer --no-default-features --features development
    if ($LASTEXITCODE -ne 0) { throw 'Development player build failed' }
    $player = Start-Player @("--ui-idle", ($TimeoutSeconds * 1000))
    $players += $player
    $window = Wait-MainWindow $player
    $requiredVisible = @(Get-PlayerControlNames)
    $requiredSettings = @(Get-SettingsControlNames)
    $required = $requiredVisible + $requiredSettings
    $elements = @($requiredVisible | ForEach-Object { Wait-Name $window $_ })
    $container = Wait-Name $window 'Open track folder'
    $container.SetFocus()
    Send-Key 'd'
    $ready = Wait-NamePrefix $window 'Ready.'
    $masterPresent = Test-MasterRouting $ready.Current.Name $true
    Invoke-Element (Wait-Name $window 'Player settings')
    $elements += @($requiredSettings | ForEach-Object { Wait-Name $window $_ })
    $cacheState = Toggle-Element (Wait-Control $window "Waveform cache" ([Windows.Automation.ControlType]::CheckBox))
    $recentState = Toggle-Element (Wait-Control $window "Remember" ([Windows.Automation.ControlType]::CheckBox))
    $theme = Wait-Control $window "Theme" ([Windows.Automation.ControlType]::ComboBox)
    $theme.SetFocus()
    Send-Key "{DOWN}"
    Send-Key "{DOWN}"
    Invoke-Element (Wait-Name $window 'Player settings')
    $container = Wait-Name $window "Open track folder"
    $container.SetFocus()
    $diagnostics = Wait-Control $window 'Diagnostics' ([Windows.Automation.ControlType]::Group)
    $ready = Wait-NamePrefix $window "Ready."
    Send-Key "^o"
    Wait-NamePrefix $window "Ready." | Out-Null
    Send-Key "p"
    $playing = Wait-NamePrefix $window "Playing."
    Wait-Name $window 'Pause' | Out-Null
    $seek = Wait-PlaybackSlider $window
    $seek.GetCurrentPattern([Windows.Automation.RangeValuePattern]::Pattern).SetValue(500)
    foreach ($role in @("Drums", "Bass", "Other", "Vocals")) {
        $audibleState = Toggle-Element (Wait-Name $window "$role enabled")
        if ($audibleState -ne [Windows.Automation.ToggleState]::Off) {
            throw "$role audible state did not toggle"
        }
    }
    $container.SetFocus()
    Send-Key "r"
    Send-Key "p"
    $paused = Wait-NamePrefix $window "Paused."
    Tab-Through $container 40
    $childProcesses = @(Get-CimInstance Win32_Process | Where-Object { $_.ParentProcessId -eq $player.Id } | ForEach-Object Name)
    $nativeProcessTree = @($childProcesses | Where-Object { $_ -match "(?i)(chrome|edge|webview|node|electron)" }).Count -eq 0
    $dpi = [Phase8Window]::GetDpiForWindow($player.MainWindowHandle)
    $scale = $dpi / 96.0
    [Phase8Window]::MoveWindow($player.MainWindowHandle, 20, 20, [int](740 * $scale), [int](400 * $scale), $true) | Out-Null
    Start-Sleep -Milliseconds 500
    Save-WindowImage $player (Join-Path $output "compact.png")
    $windowPattern = $window.GetCurrentPattern([Windows.Automation.WindowPattern]::Pattern)
    $windowPattern.SetWindowVisualState([Windows.Automation.WindowVisualState]::Minimized)
    Start-Sleep -Milliseconds 500
    $minimized = $windowPattern.Current.WindowVisualState -eq [Windows.Automation.WindowVisualState]::Minimized
    $windowPattern.SetWindowVisualState([Windows.Automation.WindowVisualState]::Normal)
    Invoke-Element (Wait-Name $window 'Play')
    Wait-NamePrefix $window "Playing." | Out-Null
    $windowPattern.Close()
    $player.WaitForExit(5000) | Out-Null
    $closeDuringPlayback = $player.HasExited
    $savedSettings = Get-Content -Raw (Join-Path $env:APPDATA "stemplayer-native/settings.json") | ConvertFrom-Json
    $settingsPersisted = $savedSettings.waveform_cache_enabled -and
        -not $savedSettings.recent_sources_enabled -and
        $savedSettings.theme -eq 2 -and
        $savedSettings.window_width -ge 560 -and
        $savedSettings.window_height -ge 256

    $player = Start-Player @("--ui-no-master", ($TimeoutSeconds * 1000))
    $players += $player
    $window = Wait-MainWindow $player
    $container = Wait-Name $window 'Open track folder'
    $container.SetFocus()
    $readyWithoutMaster = Wait-NamePrefix $window 'Ready.'
    $masterAbsent = Test-MasterRouting $readyWithoutMaster.Current.Name $false
    $player.Kill()
    $player.WaitForExit()

    $player = Start-Player @("--ui-error", ($TimeoutSeconds * 1000))
    $players += $player
    $window = Wait-MainWindow $player
    $errorElement = Wait-NamePrefix $window "Output device became unavailable"
    $errorActions = @("Retry output", "Choose source", "Reset player", "Details") | ForEach-Object { Wait-Name $window $_ }
    Invoke-Element (Wait-Name $window "Details")
    Invoke-Element (Wait-Name $window "Retry output")
    $recovered = Wait-NamePrefix $window "Ready."
    $player.Kill()
    $player.WaitForExit()

    $passed = $elements.Count -eq $required.Count -and $ready -and $playing -and $paused -and
        $diagnostics -and $masterPresent -and $masterAbsent -and $errorElement -and
        $errorActions.Count -eq 4 -and $recovered -and $minimized -and
        $closeDuringPlayback -and $settingsPersisted -and
        $cacheState -eq [Windows.Automation.ToggleState]::On -and
        $recentState -eq [Windows.Automation.ToggleState]::Off -and
        $nativeProcessTree -and $dpi -ge 96
    $report = [pscustomobject][ordered]@{
        schema = "stemplayer-native-phase8-ui-v1"
        status = if ($passed) { "passed" } else { "failed" }
        timestamp_utc = [DateTime]::UtcNow.ToString("o")
        renderer = $env:SLINT_BACKEND
        dpi = $dpi
        required_accessible_names = $required
        keyboard_flows = @("Control+O", "P", "R", "D")
        mixer_roles_qualified = @("Drums", "Bass", "Other", "Vocals")
        master_present = $masterPresent
        master_absent = $masterAbsent
        device_error_recovery = [bool]$recovered
        minimize_restore = $minimized
        close_during_playback = $closeDuringPlayback
        settings_persisted = $settingsPersisted
        settings = [ordered]@{
            theme = $savedSettings.theme
            recent_sources_enabled = $savedSettings.recent_sources_enabled
            waveform_cache_enabled = $savedSettings.waveform_cache_enabled
            window_width = $savedSettings.window_width
            window_height = $savedSettings.window_height
        }
        child_processes = $childProcesses
        native_process_tree = $nativeProcessTree
        screenshots = @("compact.png")
        unicode_source_name = 'not_tested'
        limitations = @(
            "Windows UI Automation verifies the screen-reader contract; NVDA speech is qualified separately"
            "The mock UI does not expose its source-name property as a control; Unicode source names need product UI qualification"
        )
    }
    $report | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 (Join-Path $output "ui-qualification.json")
    $report | ConvertTo-Json -Depth 4
    if (-not $passed) { exit 1 }
} finally {
    foreach ($player in $players) {
        if ($player -and -not $player.HasExited) {
            $player.Kill()
            $player.WaitForExit()
        }
    }
    Pop-Location
    $env:APPDATA = $oldAppData
    $env:LOCALAPPDATA = $oldLocalAppData
    $env:SLINT_BACKEND = $oldBackend
    Remove-Item -Recurse -Force -LiteralPath $profile -ErrorAction SilentlyContinue
}
