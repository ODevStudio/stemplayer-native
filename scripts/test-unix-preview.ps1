Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'package-unix-preview.ps1')

function Assert-UnixCheck {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

function Assert-UnixFailure {
    param([scriptblock]$Action, [string]$Pattern)
    try { & $Action | Out-Null }
    catch {
        if ($_.Exception.Message -notmatch $Pattern) { throw }
        return
    }
    throw "Expected failure: $Pattern"
}

function New-UnixTestIdentity {
    param([string]$Target, [string]$Binary,
        [string]$version = '0.1.0-preview.1',
        [string]$source_commit = 'f589012f48c8a340609e61a751001a28cdbd33b9',
        [bool]$source_dirty = $false,
        [bool]$publisher_signed = $false,
        [string]$signature_kind = (Get-UnixPreviewProfile $Target).signing,
        [string]$executable_sha256 = (Get-FileHash $Binary -Algorithm SHA256).Hash.ToLowerInvariant(),
        [string[]]$features = @('default', 'product', 'local-wav', 'local-mp4', 'aac', 'alac', 'hls'))

    @{
        product = 'Stemplayer Native'; version = $version; source_commit = $source_commit
        packaging_commit = '1' * 40; source_dirty = $source_dirty; target = $Target
        minimum_os = (Get-UnixPreviewProfile $Target).minimum_os; rustc = 'rustc 1.97.1 (synthetic check)'
        features = $features; cargo_lock_sha256 = '2' * 64; executable_sha256 = $executable_sha256
        publisher_signed = $publisher_signed; signature_kind = $signature_kind; qualification = 'not-qualified'
    }
}

$root = Split-Path -Parent $PSScriptRoot
$testRoot = Join-Path $root ('artifacts/unix-preview-tests-' + [Guid]::NewGuid().ToString('N'))
$source = 'f589012f48c8a340609e61a751001a28cdbd33b9'
Assert-UnixFailure { Get-UnixPreviewProfile 'unreviewed-target' } 'Unsupported'
Assert-UnixFailure {
    New-UnixCorrespondingSource ([pscustomobject]@{ packages = @() }) $testRoot 'unused' 'unused'
} 'Unreviewed Wayland source version'
$fakeRegistry = Join-Path $testRoot 'registry'
$fakeCache = Join-Path $fakeRegistry 'cache/test-index'
[IO.Directory]::CreateDirectory($fakeCache) | Out-Null
[IO.File]::WriteAllText((Join-Path $fakeCache 'wayland-protocols-plasma-0.3.12.crate'), 'changed source archive')
$fakeMetadata = [pscustomobject]@{ packages = @([pscustomobject]@{
    name = 'wayland-protocols-plasma'; version = '0.3.12'
    manifest_path = Join-Path $fakeRegistry 'src/test-index/wayland-protocols-plasma-0.3.12/Cargo.toml'
}) }
Assert-UnixFailure { New-UnixCorrespondingSource $fakeMetadata $testRoot 'unused' 'unused' } 'source checksum mismatch'
foreach ($target in @('x86_64-unknown-linux-gnu', 'aarch64-apple-darwin')) {
    $profile = Get-UnixPreviewProfile $target
    $name = "stemplayer-native-0.1.0-preview.1-$($profile.slug)"
    $directory = Join-Path $testRoot $name
    foreach ($file in Get-UnixPreviewFiles $target) {
        $path = Join-Path $directory $file
        [IO.Directory]::CreateDirectory((Split-Path -Parent $path)) | Out-Null
        [IO.File]::WriteAllText($path, 'synthetic packaging input, not executable media')
    }
    $binary = Join-Path $directory $profile.executable
    if (-not $IsWindows) { Invoke-PreviewCommand 'chmod' @('755', $binary) }
    $identity = Join-Path $directory 'BUILD-INFO.json'
    $info = New-UnixTestIdentity $target $binary
    [IO.File]::WriteAllText($identity, ($info | ConvertTo-Json -Depth 5))
    $notices = 'Slint Royalty-free Desktop, Mobile, and Web Applications License; MPL-2.0; Mozilla Public License; ' +
        'Copyright 2015 The Chromium Authors. All rights reserved.; Neither the name of Google Inc.' +
        "`nrealfft 3.5.0 upstream MIT declaration`nUpstream README: License: MIT`n" +
        'https://github.com/HEnquist/realfft/blob/d0d4eee0525fd27c96c8a046d6d107acd5ed84a6/README.md' +
        "`nModified symphonia-codec-aac 0.6.0 (MPL-2.0)`nFull modified source: https://github.com/ODevStudio/stemplayer-native/tree/$source/vendor/symphonia-codec-aac"
    $noticesPath = Join-Path $directory 'THIRD-PARTY-NOTICES.txt'
    [IO.File]::WriteAllText($noticesPath, $notices)
    $bom = @{
        bomFormat = 'CycloneDX'; specVersion = '1.5'
        metadata = @{ component = @{ name = 'stemplayer'; version = $info.version } }
        components = @(@{ name = 'synthetic-dependency'; version = '1.0.0' })
    }
    $bomPath = Join-Path $directory 'sbom.cdx.json'
    [IO.File]::WriteAllText($bomPath, ($bom | ConvertTo-Json -Depth 8))
    Assert-UnixPreviewPayload $directory $target | Out-Null
    foreach ($file in Get-UnixPreviewFiles $target) {
        $path = Join-Path $directory $file
        $bytes = [IO.File]::ReadAllBytes($path)
        Remove-Item -LiteralPath $path
        Assert-UnixFailure { Assert-UnixPreviewPayload $directory $target } 'allowlist'
        [IO.File]::WriteAllBytes($path, $bytes)
        if ($file -ceq $profile.executable -and -not $IsWindows) { Invoke-PreviewCommand 'chmod' @('755', $path) }
    }
    foreach ($change in @(
        @{ version = '0.1.0' }, @{ source_commit = ('3' * 40) }, @{ source_dirty = $true },
        @{ publisher_signed = $true }, @{ signature_kind = 'unreviewed' },
        @{ features = @('default', 'development') }, @{ executable_sha256 = ('0' * 64) }
    )) {
        [IO.File]::WriteAllText($identity, ((New-UnixTestIdentity $target $binary @change) | ConvertTo-Json -Depth 5))
        Assert-UnixFailure { Assert-UnixPreviewPayload $directory $target } 'identity|checksum'
    }
    [IO.File]::WriteAllText($identity, ($info | ConvertTo-Json -Depth 5))
    [IO.File]::WriteAllText($noticesPath, $notices + 'Copyright (c) <year> <copyright holders>')
    Assert-UnixFailure { Assert-UnixPreviewPayload $directory $target } 'copyright/attribution'
    [IO.File]::WriteAllText($noticesPath, $notices.Replace($source, ('4' * 40)))
    Assert-UnixFailure { Assert-UnixPreviewPayload $directory $target } 'modified AAC source'
    [IO.File]::WriteAllText($noticesPath, $notices)
    $extra = Join-Path $directory 'private.debug'
    [IO.File]::WriteAllText($extra, 'must not ship')
    Assert-UnixFailure { Assert-UnixPreviewPayload $directory $target } 'allowlist'
    Remove-Item -LiteralPath $extra
    $emptyDirectory = Join-Path $directory 'private-empty-folder'
    [IO.Directory]::CreateDirectory($emptyDirectory) | Out-Null
    Assert-UnixFailure { Assert-UnixPreviewPayload $directory $target } 'directory allowlist'
    Remove-Item -LiteralPath $emptyDirectory
    if (-not $IsWindows) {
        Invoke-PreviewCommand 'chmod' @('644', $binary)
        Assert-UnixFailure { Assert-UnixPreviewPayload $directory $target } 'permission'
        Invoke-PreviewCommand 'chmod' @('755', $binary)
        $link = Join-Path $directory 'linked-input'
        New-Item -ItemType SymbolicLink -Path $link -Target $binary | Out-Null
        Assert-UnixFailure { Assert-UnixPreviewPayload $directory $target } 'Links'
        Remove-Item -LiteralPath $link
    }
    if (($IsLinux -and $target -ceq 'x86_64-unknown-linux-gnu') -or ($IsMacOS -and $target -ceq 'aarch64-apple-darwin')) {
        $output = Join-Path $testRoot "output-$($profile.slug)"
        $assets = @(New-UnixPreviewArchive $directory $output $target)
        Assert-UnixCheck ($assets.Count -eq 2) 'Expected archive and checksum only'
        $hash = (Get-FileHash $assets[0] -Algorithm SHA256).Hash.ToLowerInvariant()
        Assert-UnixCheck ([IO.File]::ReadAllText($assets[1]) -ceq "$hash  $(Split-Path -Leaf $assets[0])`n") 'Archive checksum mismatch'
        Assert-UnixFailure { New-UnixPreviewArchive $directory $output $target } 'overwrite'
        $extract = Join-Path $testRoot ('extracted ' + [char]0x00dc + $profile.slug)
        [IO.Directory]::CreateDirectory($extract) | Out-Null
        if ($IsLinux) { Invoke-PreviewCommand 'tar' @('-xzf', $assets[0], '-C', $extract) }
        else { Invoke-PreviewCommand 'ditto' @('-x', '-k', $assets[0], $extract) }
        $unpacked = Assert-UnixPreviewPayload (Join-Path $extract $name) $target
        Assert-UnixCheck ($unpacked.source_commit -ceq $source -and $unpacked.version -ceq $info.version) 'Archive build identity mismatch'
    }
}
Write-Output 'Unix preview packaging checks passed'
exit 0
