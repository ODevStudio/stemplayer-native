Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'qualification-helpers.ps1')

function Assert-Check([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Assert-Rejected([scriptblock]$Action, [string]$MessagePattern = '.') {
    try { & $Action | Out-Null }
    catch {
        if ($_.Exception.Message -notmatch $MessagePattern) { throw }
        return
    }
    throw 'Expected qualification rejection'
}

function Read-ScriptAst([string]$Name) {
    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile(
        (Join-Path $PSScriptRoot $Name), [ref]$tokens, [ref]$errors
    )
    Assert-Check ($errors.Count -eq 0) "PowerShell parse failed: $Name"
    $ast
}

$temporary = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$testRoot = Join-Path $temporary ('stemplayer-qualification-' + [Guid]::NewGuid().ToString('N'))
$workspace = Join-Path $testRoot 'workspace'
$outside = Join-Path $testRoot 'workspace-backup'
$link = Join-Path $workspace 'linked'
New-Item -ItemType Directory -Path $workspace, $outside | Out-Null
try {
    $sentinel = Join-Path $outside 'sentinel.txt'
    [IO.File]::WriteAllText($sentinel, 'preserve unrelated output')
    Assert-Rejected { New-QualificationDirectory $workspace (Join-Path $outside 'missing') }
    Assert-Rejected { New-QualificationDirectory $workspace $workspace }
    Assert-Rejected { Get-QualificationPath $workspace ($workspace + [IO.Path]::DirectorySeparatorChar) }
    Assert-Rejected { New-QualificationDirectory $workspace (Join-Path $workspace '../workspace-backup/corrupt') }
    Assert-Check ([IO.File]::ReadAllText($sentinel) -eq 'preserve unrelated output') 'Sibling output changed'
    $output = New-QualificationDirectory $workspace (Join-Path $workspace 'output with spaces')
    [IO.File]::WriteAllText((Join-Path $output 'sentinel.txt'), 'preserve previous run')
    Assert-Rejected { New-QualificationDirectory $workspace $output }
    Assert-Check ([IO.File]::ReadAllText((Join-Path $output 'sentinel.txt')) -eq 'preserve previous run') 'Previous run changed'
    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        New-Item -ItemType Junction -Path $link -Target $outside | Out-Null
    } else {
        New-Item -ItemType SymbolicLink -Path $link -Target $outside | Out-Null
    }
    Assert-Rejected { Get-QualificationPath $workspace $link }
    Assert-Rejected { New-QualificationDirectory $workspace (Join-Path $link 'missing') }
    Assert-Rejected { Get-QualificationPath $link (Join-Path $link 'corrupt') }
    Assert-Check (-not (Test-Path -LiteralPath (Join-Path $outside 'missing'))) 'Linked output was written'
} finally {
    if (Test-Path -LiteralPath $link) { Remove-Item -LiteralPath $link -Force }
    $cleanup = Get-QualificationPath $temporary $testRoot
    Remove-Item -LiteralPath $cleanup -Recurse -Force
}

$passing = @{ status = 'pass'; checks = @(@{ status = 'pass' }, @{ status = 'pass' }) }
$failed = @{ status = 'fail'; checks = @(@{ status = 'pass' }, @{ status = 'fail' }) }
$empty = @{ status = 'pass'; checks = @() }
$contradictory = @{ status = 'pass'; checks = @(@{ status = 'fail' }) }
Assert-Check (Test-SelfCheckReport $passing 0) 'Passing checks rejected'
Assert-Check (-not (Test-SelfCheckReport $passing 1)) 'Nonzero self-check exit accepted'
Assert-Check (-not (Test-SelfCheckReport $failed 0)) 'Failed checks accepted'
Assert-Check (-not (Test-SelfCheckReport $empty 0)) 'Empty checks accepted'
Assert-Check (-not (Test-SelfCheckReport $contradictory 0)) 'Contradictory check status accepted'

$sample = @{ private_bytes = 1024; cpu_percent_one_core = 0.0 }
Assert-Check (Test-ResourceEvidence @($sample) @(0)) 'Real zero-valued CPU/FPS evidence rejected'
Assert-Check (-not (Test-ResourceEvidence @() @(0))) 'Empty steady-state samples accepted'
Assert-Check (-not (Test-ResourceEvidence @($sample) @())) 'Missing repaint counters accepted'
Assert-Check (-not (Test-ResourceEvidence @(@{ private_bytes = $null; cpu_percent_one_core = 0.0 }) @(0))) 'Missing memory accepted'
Assert-Check (-not (Test-ResourceEvidence @(@{ private_bytes = 1024; cpu_percent_one_core = [double]::NaN }) @(0))) 'NaN CPU accepted'

$measure = Read-ScriptAst 'measure-phase8-windows.ps1'
$duration = [scriptblock]::Create($measure.ParamBlock.Extent.Text + "`n'accepted'")
Assert-Rejected { & $duration -Seconds 0 }
Assert-Rejected { & $duration -Seconds 1 }
Assert-Check ((& $duration -Seconds 6) -eq 'accepted') 'Valid resource duration rejected'

foreach ($name in @(
    'measure-windows.ps1', 'qualify-windows-codecs.ps1', 'qualify-windows-ui.ps1',
    'qualify-windows-nvda.ps1', 'qualify-phase8-windows-ui.ps1', 'qualify-phase9-windows.ps1',
    'qualify-phase10-windows.ps1', 'qualification-helpers.ps1'
)) {
    Read-ScriptAst $name | Out-Null
}

foreach ($name in 'measure-windows.ps1', 'qualify-windows-codecs.ps1', 'qualify-windows-ui.ps1', 'qualify-windows-nvda.ps1') {
    $ast = Read-ScriptAst $name
    $cargoArguments = if ($name -eq 'qualify-windows-nvda.ps1') {
        ($ast.ParamBlock.Parameters | Where-Object { $_.Name.VariablePath.UserPath -eq 'CargoArguments' }).DefaultValue.SafeGetValue()
    } else { '' }
    $build = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -eq 'cargo'
    }, $true)
    $guard = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.IfStatementAst] -and
            $node.Extent.StartLineNumber -eq $build.Extent.EndLineNumber + 1
    }, $true)
    Assert-Check ($null -ne $guard) "Missing build-result guard: $name"
    $buildText = $build.Extent.Text + ' ' + $cargoArguments
    Assert-Check ($buildText -match '-p stemplayer-phase0-probe') "Required probe package not selected: $name"
    & {
        function cargo { $global:LASTEXITCODE = 1 }
        $action = [scriptblock]::Create($build.Extent.Text + "`n" + $guard.Extent.Text)
        Assert-Rejected $action 'build failed'
    }
}

$root = Split-Path -Parent $PSScriptRoot
$ui = [IO.File]::ReadAllText((Join-Path $root 'apps/stemplayer/ui/main.slint'))
$deck = [IO.File]::ReadAllText((Join-Path $root 'apps/stemplayer/ui/deck.slint'))
$uiScript = Read-ScriptAst 'qualify-phase8-windows-ui.ps1'
$routingDefinition = $uiScript.Find({
    param($node)
    $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Test-MasterRouting'
}, $true)
$routing = [scriptblock]::Create($routingDefinition.Extent.Text + "`nTest-MasterRouting @args")
Assert-Check (& $routing 'Ready. Stem container. Automatic: Master reference. System default' $true) 'Current master route rejected'
Assert-Check (& $routing 'Ready. Stem container. Automatic: Stem mix. System default' $false) 'Current stem route rejected'
Assert-Check (-not (& $routing 'Ready. Stem container. Automatic: Stem mix. System default' $true)) 'Incorrect master route accepted'
$masterWait = $uiScript.Find({
    param($node)
    $node -is [Management.Automation.Language.CommandAst] -and
        $node.GetCommandName() -like 'Wait-*' -and $node.Extent.Text.Contains('Overall gain')
}, $true)
Assert-Check ($null -eq $masterWait) 'Qualification requires an unavailable master-only control'
foreach ($function in 'Get-PlayerControlNames', 'Get-SettingsControlNames') {
    $definition = $uiScript.Find({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $function
    }, $true)
    foreach ($name in (& ($definition.Body.GetScriptBlock()))) {
        if ($name -match '^(Drums|Bass|Other|Vocals) (gain|enabled)$') {
            Assert-Check ($ui.Contains('role-name: "' + $Matches[1] + '"')) "Missing stem: $name"
            Assert-Check ($deck.Contains('accessible-label: root.role-name + " ' + $Matches[2] + '"')) "Missing stem control: $name"
        } else {
            Assert-Check ($ui.Contains('"' + $name + '"')) "Stale UI control: $name"
        }
    }
}
$waveformScript = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'qualify-phase9-windows.ps1'))
Assert-Check (-not $waveformScript.Contains('Zoom waveform in')) 'Removed waveform zoom is still required'
Write-Output 'Synthetic qualification checks passed'
