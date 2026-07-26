param(
    [Parameter(Mandatory = $true)]
    [string]$NvdaPath,
    [string]$OutputDirectory = "artifacts/phase0/windows-nvda",
    [string]$BinaryPath = "target/release/stemplayer-phase0-probe.exe",
    [string]$CargoArguments = "--release --locked",
    [string]$UiArguments = "--ui-smoke 120",
    [string[]]$Required = @("Drums gain", "Bass gain", "Other gain", "Vocals gain", "Play", "Stop")
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root $OutputDirectory
$temporary = Join-Path $env:TEMP "stemplayer-phase0-nvda"
$log = Join-Path $temporary "nvda.log"
$config = Join-Path $temporary "config"
New-Item -ItemType Directory -Force -Path $output, $temporary, $config | Out-Null

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

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

function Stop-Nvda {
    Start-Process -FilePath $NvdaPath -ArgumentList "--quit" -WindowStyle Hidden -Wait
    Start-Sleep -Seconds 2
}

Push-Location $root
$probe = $null
try {
    cargo build @($CargoArguments -split " ")
    $nvdaArguments = @(
        "--minimal",
        "--disable-addons",
        "--no-sr-flag",
        "--debug-logging",
        "--log-file=$log",
        "--config-path=$config",
        "--lang=en"
    )
    Start-Process -FilePath $NvdaPath -ArgumentList $nvdaArguments -WindowStyle Hidden | Out-Null
    Start-Sleep -Seconds 8
    $binary = Join-Path $root $BinaryPath
    $probe = Start-Process -FilePath $binary -ArgumentList $UiArguments -PassThru
    $handle = Wait-MainWindow -Process $probe
    $window = [Windows.Automation.AutomationElement]::FromHandle($handle)
    $elements = $window.FindAll(
        [Windows.Automation.TreeScope]::Descendants,
        [Windows.Automation.Condition]::TrueCondition
    )
    foreach ($name in $Required) {
        $element = $elements | Where-Object { $_.Current.Name -eq $name } | Select-Object -First 1
        if (-not $element) {
            throw "Accessible control missing: $name"
        }
        $element.SetFocus()
        Start-Sleep -Milliseconds 700
    }
    $probe.Kill()
    $probe.WaitForExit()
    Stop-Nvda
    $speech = @(Get-Content -Path $log | Where-Object { $_ -match '^Speaking \[' })
    $announced = @(
        foreach ($name in $Required) {
            if ($speech | Where-Object { $_ -like "*'$name'*" }) {
                $name
            }
        }
    )
    $missing = @($Required | Where-Object { $_ -notin $announced })
    $passed = $missing.Count -eq 0
    $nvdaVersion = (Get-Item $NvdaPath).VersionInfo.ProductVersion
    $report = [pscustomobject][ordered]@{
        schema = "stemplayer-windows-nvda-qualification-v1"
        status = if ($passed) { "passed" } else { "failed" }
        timestamp_utc = [DateTime]::UtcNow.ToString("o")
        nvda_version = $nvdaVersion
        nvda_sha256 = (Get-FileHash -Algorithm SHA256 $NvdaPath).Hash.ToLowerInvariant()
        required_announcements = $Required
        announced = $announced
        missing = $missing
        limitations = @(
            "NVDA debug speech events prove named control exposure and focus announcements",
            "This automated smoke test is not a blind-user usability study",
            "NVDA is an external qualification tool and is not a build or runtime dependency"
        )
    }
    $report | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 (Join-Path $output "nvda-qualification.json")
    if (-not $passed) {
        exit 1
    }
} finally {
    if ($probe -and -not $probe.HasExited) {
        $probe.Kill()
        $probe.WaitForExit()
    }
    if (Get-Process -Name "nvda" -ErrorAction SilentlyContinue) {
        Stop-Nvda
    }
    Pop-Location
}
