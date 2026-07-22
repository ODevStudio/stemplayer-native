param(
    [string]$Distribution = "Ubuntu-24.04",
    [string]$OutputDirectory = "artifacts/phase0/wsl-ubuntu"
)

$ErrorActionPreference = "Continue"
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root $OutputDirectory
New-Item -ItemType Directory -Force -Path $output | Out-Null
$drive = $root.Substring(0, 1).ToLowerInvariant()
$linuxRoot = "/mnt/$drive/" + $root.Substring(3).Replace([char]92, [char]47)
$prefix = "cd '$linuxRoot'; export CARGO_TARGET_DIR=/tmp/stemplayer-native-phase0-target; export RUSTC=/root/.cargo/bin/rustc"
$identity = & wsl -d $Distribution --user root -- sh -lc "cd /tmp; cat /etc/os-release; uname -a; /root/.cargo/bin/rustc --version; /root/.cargo/bin/cargo --version" 2>&1
if ($LASTEXITCODE -ne 0) {
    throw "WSL identity check failed"
}
$testOutput = & wsl -d $Distribution --user root -- sh -lc "$prefix; /root/.cargo/bin/cargo test --locked" 2>&1
$testExit = $LASTEXITCODE
$metadataOutput = & wsl -d $Distribution --user root -- sh -lc "$prefix; /root/.cargo/bin/cargo metadata --locked --no-deps --format-version 1" 2>&1
$metadataExit = $LASTEXITCODE
$passed = $testExit -eq 0 -and $metadataExit -eq 0
$report = [ordered]@{
    schema = "stemplayer-wsl-headless-qualification-v1"
    status = if ($passed) { "passed" } else { "failed" }
    timestamp_utc = [DateTime]::UtcNow.ToString("o")
    distribution = $Distribution
    identity = @($identity | ForEach-Object { $_.ToString() })
    workspace = $linuxRoot
    cargo_target_dir = "/tmp/stemplayer-native-phase0-target"
    tests = [ordered]@{
        command = "cargo test --locked"
        exit_code = $testExit
        output = @($testOutput | ForEach-Object { $_.ToString() })
    }
    metadata = [ordered]@{
        command = "cargo metadata --locked --no-deps --format-version 1"
        exit_code = $metadataExit
        output = @($metadataOutput | ForEach-Object { $_.ToString() })
    }
    limitations = @(
        "WSL proves Linux compilation and headless logic only",
        "Linux GUI, native audio, renderer, accessibility, packaging, and runtime performance remain optional Phase 12 qualification"
    )
}
$report | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 (Join-Path $output "headless.json")
if (-not $passed) {
    exit 1
}
