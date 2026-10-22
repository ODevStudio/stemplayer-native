Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'package-windows-preview.ps1')

function Assert-Check {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

function Assert-Failure {
    param([scriptblock]$Action, [string]$Pattern)
    try { & $Action | Out-Null }
    catch {
        if ($_.Exception.Message -notmatch $Pattern) { throw }
        return
    }
    throw "Expected failure: $Pattern"
}

function Write-TestJson {
    param([string]$Path, [object]$Value)
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 20), [Text.UTF8Encoding]::new($false))
}

function New-TestBuildInfo {
    param(
        [string]$product = 'Stemplayer Native',
        [string]$version = '0.1.0-preview.1',
        [string]$source_commit = ('1' * 40),
        [bool]$source_dirty = $false,
        [string]$target = 'x86_64-pc-windows-msvc',
        [bool]$signed = $false,
        [string]$qualification = 'not-qualified',
        [string]$executable_sha256 = $binaryHash,
        [string[]]$features = @('default', 'product', 'local-wav', 'local-mp4', 'aac', 'alac', 'hls')
    )
    @{
        product = $product
        version = $version
        source_commit = $source_commit
        source_dirty = $source_dirty
        target = $target
        signed = $signed
        qualification = $qualification
        rustc = 'rustc 1.97.1 (synthetic check)'
        cargo_lock_sha256 = '2' * 64
        executable_sha256 = $executable_sha256
        features = $features
    }
}

& {
    function Invoke-PreviewCommand {
        param([string]$Command, [string[]]$Arguments)

        if ($Command -cne 'git') { throw 'Unexpected source-check command' }
        switch -CaseSensitive ($Arguments -join ' ') {
            'status --porcelain --untracked-files=normal' { $source.status }
            'rev-parse HEAD' { $source.revision }
            default { throw 'Unexpected source-check arguments' }
        }
    }

    $capturedRevision = '1' * 40
    foreach ($source in @(
        @{ revision = $capturedRevision; status = ''; failure = $null },
        @{ revision = ('2' * 40); status = ''; failure = 'Source revision changed' },
        @{ revision = $capturedRevision; status = ' M synthetic.rs'; failure = 'Source checkout changed' }
    )) {
        if ($source.failure) {
            Assert-Failure { Assert-PreviewSourceUnchanged $capturedRevision } $source.failure
        } else {
            Assert-PreviewSourceUnchanged $capturedRevision
        }
    }
}

$root = Split-Path -Parent $PSScriptRoot
$testRoot = Join-Path $root ('artifacts/preview-tests-' + [Guid]::NewGuid().ToString('N'))
$name = 'stemplayer-native-0.1.0-preview.1-windows-x86_64'
$payload = Join-Path $testRoot $name
$output = Join-Path $testRoot 'output'
[IO.Directory]::CreateDirectory($payload) | Out-Null
foreach ($file in Get-PreviewFiles) {
    [IO.File]::WriteAllText((Join-Path $payload $file), 'synthetic packaging input, not executable media')
}
$noticesPath = Join-Path $payload 'THIRD-PARTY-NOTICES.txt'
$notices = 'Slint Royalty-free Desktop, Mobile, and Web Applications License; MPL-2.0; Mozilla Public License' +
    "`nModified symphonia-codec-aac 0.6.0 (MPL-2.0)`nFull modified source: https://github.com/ODevStudio/stemplayer-native/tree/" +
    ('1' * 40) + '/vendor/symphonia-codec-aac'
[IO.File]::WriteAllText($noticesPath, $notices)
$binaryHash = (Get-FileHash -LiteralPath (Join-Path $payload 'stemplayer.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
$info = New-TestBuildInfo
$infoPath = Join-Path $payload 'BUILD-INFO.json'
Write-TestJson $infoPath $info
$bom = @{
    bomFormat = 'CycloneDX'
    specVersion = '1.5'
    metadata = @{ component = @{ name = 'stemplayer'; version = $info.version } }
    components = @(@{ name = 'synthetic-dependency'; version = '1.0.0' })
}
$bomPath = Join-Path $payload 'sbom.cdx.json'
Write-TestJson $bomPath $bom

foreach ($file in Get-PreviewFiles) {
    $path = Join-Path $payload $file
    $bytes = [IO.File]::ReadAllBytes($path)
    Remove-Item -LiteralPath $path
    Assert-Failure { Assert-PreviewPayload $payload } 'approved files'
    [IO.File]::WriteAllBytes($path, $bytes)
}
$extra = Join-Path $payload 'private.pdb'
[IO.File]::WriteAllText($extra, 'must not ship')
Assert-Failure { Assert-PreviewPayload $payload } 'approved files'
Remove-Item -LiteralPath $extra

foreach ($change in @(
    @{ product = 'Another product' },
    @{ version = '0.1.0' },
    @{ version = '../escape-preview.1' },
    @{ source_commit = 'not-a-commit' },
    @{ source_commit = ('3' * 40) },
    @{ source_dirty = $true },
    @{ target = 'aarch64-pc-windows-msvc' },
    @{ signed = $true },
    @{ qualification = 'passed' },
    @{ executable_sha256 = '0' * 64 },
    @{ features = @('development') }
)) {
    Write-TestJson $infoPath (New-TestBuildInfo @change)
    Assert-Failure { Assert-PreviewPayload $payload } 'identity|checksum'
}
Write-TestJson $infoPath $info
Write-TestJson $bomPath @{ bomFormat = 'CycloneDX'; specVersion = '1.5'; metadata = @{ component = @{ name = 'stemplayer'; version = '9.0.0' } }; components = $bom.components }
Assert-Failure { Assert-PreviewPayload $payload } 'SBOM'
Write-TestJson $bomPath $bom
[IO.File]::WriteAllText($noticesPath, 'incomplete license notice')
Assert-Failure { Assert-PreviewPayload $payload } 'license text'
[IO.File]::WriteAllText($noticesPath, $notices + "`nCopyright (c) <year> <copyright holders>")
Assert-Failure { Assert-PreviewPayload $payload } 'copyright/attribution'
[IO.File]::WriteAllText($noticesPath, $notices)
$aacMetadata = [pscustomobject]@{
    workspace_root = $root
    packages = @([pscustomobject]@{
        name = 'symphonia-codec-aac'
        version = '0.6.0'
        source = $null
        manifest_path = (Join-Path $root 'vendor/symphonia-codec-aac/Cargo.toml')
    })
}
$aacNoticePath = Join-Path $testRoot 'modified-aac-notice.txt'
Add-PreviewAacNotice $aacMetadata $aacNoticePath ('1' * 40)
Assert-Check ([IO.File]::ReadAllText($aacNoticePath).Contains('Full modified source: https://github.com/ODevStudio/stemplayer-native/tree/' + ('1' * 40))) 'Modified AAC source URL missing'
Assert-Failure { Add-PreviewAacNotice $aacMetadata $aacNoticePath 'invalid' } 'source provenance'
Assert-Failure { Add-PreviewAacNotice ([pscustomobject]@{ workspace_root = $root; packages = @() }) $aacNoticePath ('1' * 40) } 'source provenance'
Assert-Failure { Add-PreviewSlintLicense ([pscustomobject]@{ packages = @() }) $noticesPath } 'Slint license clarification'
$slintRoot = Join-Path $testRoot 'slint'
[IO.Directory]::CreateDirectory((Join-Path $slintRoot 'LICENSES')) | Out-Null
[IO.File]::WriteAllText((Join-Path $slintRoot 'LICENSES/LicenseRef-Slint-Royalty-free-2.0.md'), 'changed terms')
$changedLicense = [pscustomobject]@{ packages = @([pscustomobject]@{ name = 'slint'; version = '1.17.1'; manifest_path = (Join-Path $slintRoot 'Cargo.toml') }) }
Assert-Failure { Add-PreviewSlintLicense $changedLicense $noticesPath } 'license text changed'
Assert-Failure { Invoke-PreviewCommand 'pwsh' @('-NoProfile', '-Command', 'exit 7') } 'exit code 7'
Assert-PreviewSystemLibraries @('kernel32.dll', 'USER32.dll', 'api-ms-win-core-synch-l1-2-0.dll')
Assert-Failure { Assert-PreviewSystemLibraries @('kernel32.dll', 'VCRUNTIME140.dll') } 'Unapproved Windows imports'
Assert-Failure { Assert-PreviewSystemLibraries @('browser-helper.dll') } 'Unapproved Windows imports'
Assert-Failure { Assert-PreviewSystemLibraries @() } 'Unapproved Windows imports'

$packageId = 'path+file:///private-build/apps/stemplayer#0.1.0-preview.1'
$vendorId = 'path+file:///private-build/vendor/symphonia-codec-aac#0.6.0'
$references = @{
    metadata = @{ component = @{ 'bom-ref' = $packageId; components = @(@{ 'bom-ref' = "$packageId bin-target-0" }, @{ 'bom-ref' = $vendorId }) } }
    dependencies = @(@{ ref = $packageId; dependsOn = @($packageId, 'registry:dependency') })
}
$packages = @(
    [pscustomobject]@{ id = $packageId; name = 'stemplayer'; version = $info.version },
    [pscustomobject]@{ id = $vendorId; name = 'symphonia-codec-aac'; version = '0.6.0' }
)
$sanitized = ConvertTo-Json -InputObject (Convert-PreviewSbomRefs $references $packages) -Depth 20 | ConvertFrom-Json
Assert-Check ($references.metadata.component.'bom-ref' -ceq $packageId) 'SBOM conversion mutated its input'
Assert-Check ($sanitized.metadata.component.'bom-ref' -ceq "workspace:stemplayer@$($info.version)") 'SBOM root reference still exposes a private path'
Assert-Check ($sanitized.metadata.component.components[0].'bom-ref' -ceq "workspace:stemplayer@$($info.version) bin-target-0") 'SBOM target reference mismatch'
Assert-Check ($sanitized.metadata.component.components[1].'bom-ref' -ceq 'workspace:symphonia-codec-aac@0.6.0') 'Vendored AAC reference still exposes a private path'
Assert-Check ($sanitized.dependencies[0].dependsOn.Count -eq 2 -and $sanitized.dependencies[0].dependsOn[0] -ceq $sanitized.dependencies[0].ref) 'SBOM graph references or array shape changed'

$assets = @(New-PreviewArchive $payload $output)
Assert-Check ($assets.Count -eq 2) 'Expected ZIP and checksum assets'
$zipPath = Join-Path $output "$name.zip"
$checksum = [IO.File]::ReadAllText((Join-Path $output 'SHA256SUMS.txt'))
$hash = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
Assert-Check ($checksum -ceq "$hash  $name.zip`n") 'Published checksum does not match ZIP'
$zip = [IO.Compression.ZipFile]::OpenRead($zipPath)
try {
    $actual = @($zip.Entries | ForEach-Object FullName | Sort-Object)
    $expected = @(Get-PreviewFiles | ForEach-Object { "$name/$_" } | Sort-Object)
    Assert-Check (($actual -join ',') -ceq ($expected -join ',')) 'Archive allowlist mismatch'
    $entry = $zip.GetEntry("$name/BUILD-INFO.json")
    $reader = [IO.StreamReader]::new($entry.Open())
    try {
        $identity = $reader.ReadToEnd() | ConvertFrom-Json
        Assert-Check ($identity.version -ceq $info.version -and $identity.source_commit -ceq $info.source_commit) 'Archive identity mismatch'
    }
    finally { $reader.Dispose() }
}
finally { $zip.Dispose() }
Assert-Failure { New-PreviewArchive $payload $output } 'overwrite'
Write-Output 'Windows preview packaging checks passed'
