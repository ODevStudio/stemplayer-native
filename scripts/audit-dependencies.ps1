param(
    [string]$OutputDirectory = "artifacts/phase0/dependencies"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root $OutputDirectory
New-Item -ItemType Directory -Force -Path $output | Out-Null

function Invoke-CargoCapture {
    param([string]$Arguments)

    $startInfo = [Diagnostics.ProcessStartInfo]::new("cargo")
    $startInfo.Arguments = $Arguments
    $startInfo.WorkingDirectory = $root
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($startInfo)
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    [pscustomobject][ordered]@{
        command = "cargo $Arguments"
        exit_code = $process.ExitCode
        stdout = $stdout
        stderr = $stderr
    }
}

$auditProcess = Invoke-CargoCapture -Arguments "audit --json"
$licenseProcess = Invoke-CargoCapture -Arguments "deny list --format json"
$metadataProcess = Invoke-CargoCapture -Arguments "metadata --locked --format-version 1"
$audit = $auditProcess.stdout | ConvertFrom-Json
$licenses = $licenseProcess.stdout | ConvertFrom-Json
$metadata = $metadataProcess.stdout | ConvertFrom-Json
$passed = $auditProcess.exit_code -eq 0 -and
    $licenseProcess.exit_code -eq 0 -and
    $metadataProcess.exit_code -eq 0 -and
    $audit.vulnerabilities.count -eq 0 -and
    $licenses.unlicensed.Count -eq 0
$report = [ordered]@{
    schema = "stemplayer-dependency-audit-v1"
    status = if ($passed) { "passed" } else { "failed" }
    timestamp_utc = [DateTime]::UtcNow.ToString("o")
    lock_sha256 = (Get-FileHash -Algorithm SHA256 (Join-Path $root "Cargo.lock")).Hash.ToLowerInvariant()
    dependency_count = $audit.lockfile."dependency-count"
    advisory_database = $audit.database
    vulnerabilities = $audit.vulnerabilities
    informational_warnings = $audit.warnings
    licenses = $licenses
    unlicensed_count = $licenses.unlicensed.Count
    metadata_package_count = $metadata.packages.Count
    commands = @(
        [ordered]@{
            command = $auditProcess.command
            exit_code = $auditProcess.exit_code
            stderr = $auditProcess.stderr
        },
        [ordered]@{
            command = $licenseProcess.command
            exit_code = $licenseProcess.exit_code
            stderr = $licenseProcess.stderr
        },
        [ordered]@{
            command = $metadataProcess.command
            exit_code = $metadataProcess.exit_code
            stderr = $metadataProcess.stderr
        }
    )
    limitations = @(
        "The audit reports four unmaintained transitive crates from the provisional Slint graph",
        "Release notices and final legal approval remain Phase 12 work",
        "No advisory was ignored"
    )
}
$report | ConvertTo-Json -Depth 20 | Set-Content -Encoding utf8 (Join-Path $output "audit.json")
if (-not $passed) {
    exit 1
}
