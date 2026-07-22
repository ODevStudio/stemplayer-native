param(
    [string]$OutputPath = "artifacts/phase2/windows/ui-qualification.json"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root $OutputPath
$env:SLINT_BACKEND = "winit-software"
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms

function Wait-MainWindow([Diagnostics.Process]$Process) {
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
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
    $deadline = [DateTime]::UtcNow.AddSeconds(3)
    while ([DateTime]::UtcNow -lt $deadline) {
        $match = Descendants $Window | Where-Object { $_.Current.Name -eq $Name } | Select-Object -First 1
        if ($match) { return $match }
        Start-Sleep -Milliseconds 100
    }
    throw "Accessible element missing: $Name"
}

function Send-Key([string]$Keys) {
    [System.Windows.Forms.SendKeys]::SendWait($Keys)
    Start-Sleep -Milliseconds 250
}

Push-Location $root
$player = $null
try {
    cargo build --release --locked -p stemplayer --no-default-features --features development
    $binary = Join-Path $root "target/release/stemplayer.exe"
    $player = Start-Process -FilePath $binary -PassThru
    $window = Wait-MainWindow $player
    $required = @(
        "Open source", "Playback position",
        "Drums gain", "Mute Drums", "Solo Drums",
        "Bass gain", "Mute Bass", "Solo Bass",
        "Other gain", "Mute Other", "Solo Other",
        "Vocals gain", "Mute Vocals", "Solo Vocals",
        "Play", "Pause", "Stop", "Reset", "Diagnostics"
    )
    $requiredElements = @($required | ForEach-Object { Wait-Name $window $_ })
    $requiredElements[0].SetFocus()
    Send-Key "^o"
    $loaded = Wait-Name $window "Phase 2 mock.stem.m4a"
    Send-Key "p"
    $playing = Wait-Name $window "Playing"
    Send-Key "p"
    $paused = Wait-Name $window "Paused"
    Send-Key "d"
    $diagnostics = Wait-Name $window "Diagnostics"
    Send-Key "%1"
    $mute = Wait-Name $window "Mute Drums"
    $toggle = $mute.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern)
    $muted = $toggle.Current.ToggleState -eq [Windows.Automation.ToggleState]::On
    $focusOrder = @(
        Descendants $window |
            Where-Object { $_.Current.IsKeyboardFocusable } |
            ForEach-Object {
                [pscustomobject][ordered]@{
                    name = $_.Current.Name
                    control_type = $_.Current.ControlType.ProgrammaticName
                }
            }
    )
    $windowPattern = $window.GetCurrentPattern([Windows.Automation.WindowPattern]::Pattern)
    $windowPattern.SetWindowVisualState([Windows.Automation.WindowVisualState]::Minimized)
    Start-Sleep -Milliseconds 500
    $minimized = $windowPattern.Current.WindowVisualState -eq [Windows.Automation.WindowVisualState]::Minimized
    $windowPattern.SetWindowVisualState([Windows.Automation.WindowVisualState]::Normal)
    $passed = $requiredElements.Count -eq $required.Count -and $loaded -and $playing -and $paused -and $diagnostics -and $muted -and $minimized
    $report = [pscustomobject][ordered]@{
        schema = "stemplayer-native-phase2-ui-v1"
        status = if ($passed) { "passed" } else { "failed" }
        timestamp_utc = [DateTime]::UtcNow.ToString("o")
        renderer = $env:SLINT_BACKEND
        required_accessible_names = $required
        keyboard_sequence = @("Control+O", "P", "P", "D", "Alt+1")
        observed_states = @("Phase 2 mock.stem.m4a", "Playing", "Paused", "Diagnostics", "Mute Drums on")
        focus_order = $focusOrder
        minimize_restore = if ($minimized) { "passed" } else { "failed" }
        limitations = @(
            "UI Automation proves names, focusability, keyboard commands, and window state"
            "NVDA speech output is not covered by this script"
        )
    }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $output) | Out-Null
    $report | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 $output
    $report | ConvertTo-Json -Depth 4
    if (-not $passed) { exit 1 }
} finally {
    if ($player -and -not $player.HasExited) {
        $player.Kill()
        $player.WaitForExit()
    }
    Pop-Location
}
