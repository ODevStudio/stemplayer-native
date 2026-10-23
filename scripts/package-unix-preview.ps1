param([string]$SourceDirectory)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'package-windows-preview.ps1')

function Get-UnixPreviewProfile {
    param([string]$Target)

    switch -CaseSensitive ($Target) {
        'x86_64-unknown-linux-gnu' {
            @{ slug = 'linux-x86_64'; extension = '.tar.gz'; minimum_os = 'Ubuntu 24.04';
                executable = 'stemplayer'; signing = 'none' }
        }
        'aarch64-apple-darwin' {
            @{ slug = 'macos-aarch64'; extension = '.zip'; minimum_os = 'macOS 14';
                executable = 'Stemplayer Native.app/Contents/MacOS/stemplayer'; signing = 'ad-hoc' }
        }
        default { throw 'Unsupported Unix preview target' }
    }
}

function Get-UnixPreviewFiles {
    param([string]$Target)

    @('README.txt', 'LICENSE-MIT', 'LICENSE-APACHE', 'THIRD-PARTY-NOTICES.txt',
        'sbom.cdx.json', 'BUILD-INFO.json', 'RUNTIME-DEPENDENCIES.txt')
    (Get-UnixPreviewProfile $Target).executable
    if ($Target -ceq 'x86_64-unknown-linux-gnu') { 'CORRESPONDING-SOURCE.tar.gz' }
    else {
        'Stemplayer Native.app/Contents/Info.plist'
        'Stemplayer Native.app/Contents/Resources/stemplayer.icns'
        'Stemplayer Native.app/Contents/_CodeSignature/CodeResources'
    }
}

function Assert-UnixPreviewPayload {
    param([string]$Directory, [string]$Target)

    $profile = Get-UnixPreviewProfile $Target
    $entries = @(Get-ChildItem -LiteralPath $Directory -Recurse -Force)
    if (@($entries | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }).Count) {
        throw 'Links are not approved preview inputs'
    }
    $files = @($entries | Where-Object { -not $_.PSIsContainer })
    $actual = @($files | ForEach-Object { [IO.Path]::GetRelativePath($Directory, $_.FullName).Replace('\', '/') } | Sort-Object)
    $expected = @(Get-UnixPreviewFiles $Target | Sort-Object)
    if (($actual -join ',') -cne ($expected -join ',')) { throw 'Unix preview archive allowlist mismatch' }
    $directories = @($entries | Where-Object PSIsContainer | ForEach-Object {
        [IO.Path]::GetRelativePath($Directory, $_.FullName).Replace('\', '/')
    } | Sort-Object)
    $approvedDirectories = if ($Target -ceq 'aarch64-apple-darwin') {
        @('Stemplayer Native.app', 'Stemplayer Native.app/Contents', 'Stemplayer Native.app/Contents/MacOS',
            'Stemplayer Native.app/Contents/Resources', 'Stemplayer Native.app/Contents/_CodeSignature') | Sort-Object
    } else { @() }
    if (($directories -join ',') -cne ($approvedDirectories -join ',')) { throw 'Unix directory allowlist mismatch' }
    foreach ($file in $files) {
        $relative = [IO.Path]::GetRelativePath($Directory, $file.FullName).Replace('\', '/')
        $limit = if ($relative -ceq $profile.executable) { 128MB } elseif ($relative -ceq 'CORRESPONDING-SOURCE.tar.gz') { 64MB }
            elseif ($relative -ceq 'BUILD-INFO.json') { 64KB } else { 16MB }
        if ($file.Length -le 0 -or $file.Length -gt $limit) { throw 'Invalid or unbounded preview input' }
    }
    $info = Get-Content -LiteralPath (Join-Path $Directory 'BUILD-INFO.json') -Raw | ConvertFrom-Json
    if ($info.product -cne 'Stemplayer Native' -or $info.version -cne '0.1.0-preview.1' -or
        $info.source_commit -cne 'f589012f48c8a340609e61a751001a28cdbd33b9' -or
        $info.packaging_commit -cnotmatch '^[0-9a-f]{40}$' -or
        $info.source_dirty -isnot [bool] -or $info.source_dirty -or $info.target -cne $Target -or
        $info.minimum_os -cne $profile.minimum_os -or $info.signature_kind -cne $profile.signing -or
        $info.publisher_signed -isnot [bool] -or $info.publisher_signed -or
        $info.qualification -cne 'not-qualified' -or $info.rustc -notmatch '^rustc 1\.97\.1 ' -or
        $info.cargo_lock_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
        (@($info.features | Sort-Object) -join ',') -cne 'aac,alac,default,hls,local-mp4,local-wav,product') {
        throw 'Invalid Unix preview build identity'
    }
    $binary = Join-Path $Directory $profile.executable
    if ((Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant() -cne $info.executable_sha256) {
        throw 'Unix executable checksum mismatch'
    }
    if (-not $IsWindows -and -not ([IO.File]::GetUnixFileMode($binary) -band [IO.UnixFileMode]::UserExecute)) {
        throw 'Unix executable permission is missing'
    }
    $bom = Get-Content -LiteralPath (Join-Path $Directory 'sbom.cdx.json') -Raw | ConvertFrom-Json
    if ($bom.bomFormat -cne 'CycloneDX' -or $bom.specVersion -cne '1.5' -or
        $bom.metadata.component.name -cne 'stemplayer' -or $bom.metadata.component.version -cne $info.version -or
        @($bom.components).Count -eq 0) { throw 'Invalid Unix preview SBOM' }
    Assert-PreviewNotices (Join-Path $Directory 'THIRD-PARTY-NOTICES.txt') $info.source_commit
    return $info
}

function New-UnixPreviewArchive {
    param([string]$Directory, [string]$Output, [string]$Target)

    $info = Assert-UnixPreviewPayload $Directory $Target
    $profile = Get-UnixPreviewProfile $Target
    $name = "stemplayer-native-$($info.version)-$($profile.slug)"
    if ((Split-Path -Leaf $Directory) -cne $name) { throw 'Unix payload name does not match build identity' }
    $archive = Join-Path $Output ($name + $profile.extension)
    $checksum = "$archive.sha256"
    if ((Test-Path -LiteralPath $archive) -or (Test-Path -LiteralPath $checksum)) {
        throw 'Refusing to overwrite existing preview assets'
    }
    [IO.Directory]::CreateDirectory($Output) | Out-Null
    if ($Target -ceq 'x86_64-unknown-linux-gnu') {
        Invoke-PreviewCommand 'tar' @('-czf', $archive, '-C', (Split-Path -Parent $Directory), $name)
    } else {
        Invoke-PreviewCommand 'ditto' @('-c', '-k', '--keepParent', '--norsrc', '--noextattr', $Directory, $archive)
    }
    $hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText($checksum, "$hash  $(Split-Path -Leaf $archive)`n", [Text.UTF8Encoding]::new($false))
    $archive
    $checksum
}

function New-MacosPreviewBundle {
    param([string]$Binary, [string]$Directory, [string]$Icon)

    $contents = Join-Path $Directory 'Stemplayer Native.app/Contents'
    [IO.Directory]::CreateDirectory((Join-Path $contents 'MacOS')) | Out-Null
    [IO.Directory]::CreateDirectory((Join-Path $contents 'Resources')) | Out-Null
    Copy-Item -LiteralPath $Binary -Destination (Join-Path $contents 'MacOS/stemplayer')
    Invoke-PreviewCommand 'chmod' @('755', (Join-Path $contents 'MacOS/stemplayer'))
    $iconset = Join-Path (Split-Path -Parent $Directory) 'stemplayer.iconset'
    [IO.Directory]::CreateDirectory($iconset) | Out-Null
    foreach ($size in @(16, 32, 128, 256, 512)) {
        Invoke-PreviewCommand 'sips' @('-z', "$size", "$size", $Icon, '--out', (Join-Path $iconset "icon_${size}x${size}.png"))
        Invoke-PreviewCommand 'sips' @('-z', "$($size * 2)", "$($size * 2)", $Icon, '--out', (Join-Path $iconset "icon_${size}x${size}@2x.png"))
    }
    Invoke-PreviewCommand 'iconutil' @('-c', 'icns', '-o', (Join-Path $contents 'Resources/stemplayer.icns'), $iconset)
    $plist = '<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0"><dict>' +
        '<key>CFBundleExecutable</key><string>stemplayer</string><key>CFBundleIdentifier</key><string>stemplayer-native</string>' +
        '<key>CFBundleName</key><string>Stemplayer Native</string><key>CFBundleDisplayName</key><string>Stemplayer Native</string>' +
        '<key>CFBundlePackageType</key><string>APPL</string><key>CFBundleShortVersionString</key><string>0.1.0</string>' +
        '<key>CFBundleVersion</key><string>1</string><key>CFBundleIconFile</key><string>stemplayer.icns</string>' +
        '<key>CFBundleGetInfoString</key><string>Stemplayer Native 0.1.0-preview.1</string>' +
        '<key>LSMinimumSystemVersion</key><string>14.0</string><key>NSHighResolutionCapable</key><true/></dict></plist>'
    [IO.File]::WriteAllText((Join-Path $contents 'Info.plist'), $plist, [Text.UTF8Encoding]::new($false))
    Invoke-PreviewCommand 'plutil' @('-lint', (Join-Path $contents 'Info.plist'))
    Invoke-PreviewCommand 'codesign' @('--force', '--sign', '-', '--timestamp=none', (Split-Path -Parent $contents))
    Invoke-PreviewCommand 'codesign' @('--verify', '--deep', '--strict', (Split-Path -Parent $contents))
}

function Get-UnixPreviewImports {
    param([string]$Binary, [string]$Target)

    if ($Target -ceq 'aarch64-apple-darwin') {
        if ((Invoke-PreviewCommand 'lipo' @('-archs', $Binary)).Trim() -cne 'arm64') { throw 'Unexpected macOS architecture' }
        $imports = @(Invoke-PreviewCommand 'otool' @('-L', $Binary) | Select-Object -Skip 1 | ForEach-Object {
            if ($_ -match '^\s+(/.+?) \(') { $Matches[1] } else { throw 'Unrecognized macOS dependency' }
        })
        if (-not $imports.Count -or @($imports | Where-Object { $_ -cnotmatch '^/(System/Library/|usr/lib/)' }).Count) {
            throw 'Unapproved non-system macOS dependency'
        }
        return $imports
    }
    $header = Invoke-PreviewCommand 'readelf' @('-h', $Binary)
    if (($header -join "`n") -notmatch 'Machine:\s+Advanced Micro Devices X86-64') { throw 'Unexpected Linux architecture' }
    $versions = Invoke-PreviewCommand 'readelf' @('--version-info', $Binary)
    $glibc = @([regex]::Matches(($versions -join "`n"), '\bGLIBC_(\d+\.\d+)\b') | ForEach-Object { [version]$_.Groups[1].Value } | Sort-Object -Unique)
    if (-not $glibc.Count -or $glibc[-1] -gt [version]'2.39') { throw 'Linux binary exceeds the glibc 2.39 baseline' }
    $imports = @(Invoke-PreviewCommand 'ldd' @($Binary))
    if (($imports -join "`n") -match 'not found' -or -not $imports.Count) { throw 'Missing Linux runtime dependency' }
    foreach ($line in $imports) {
        if ($line -match '=>\s+(\S+)' -and $Matches[1] -cnotmatch '^/(usr/)?lib(64)?/') {
            throw 'Unapproved non-system Linux dependency'
        }
    }
    @("Maximum required glibc version: $($glibc[-1])") + @($imports | ForEach-Object { $_ -replace '\s+\(0x[0-9a-f]+\)', '' })
}

function New-UnixCorrespondingSource {
    param([object]$Metadata, [string]$Work, [string]$Destination, [string]$Notices)

    $package = @($Metadata.packages | Where-Object name -CEQ 'wayland-protocols-plasma')
    if ($package.Count -ne 1 -or $package[0].version -cne '0.3.12') { throw 'Unreviewed Wayland source version' }
    $crateRoot = Split-Path -Parent $package[0].manifest_path
    $registry = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $crateRoot))
    $index = Split-Path -Leaf (Split-Path -Parent $crateRoot)
    $crate = Join-Path $registry "cache/$index/wayland-protocols-plasma-0.3.12.crate"
    if ((Get-FileHash -LiteralPath $crate -Algorithm SHA256).Hash.ToLowerInvariant() -cne
        '2b6d8cf1eb2c1c31ed1f5643c88a6e53538129d4af80030c8cabd1f9fa884d91') {
        throw 'Wayland corresponding source checksum mismatch'
    }
    foreach ($xml in Get-ChildItem (Join-Path $crateRoot 'plasma-wayland-protocols/src/protocols') -Filter '*.xml') {
        if ($xml.Length -gt 1MB) { throw 'Unbounded protocol notice input' }
        $settings = [Xml.XmlReaderSettings]::new()
        $settings.DtdProcessing = [Xml.DtdProcessing]::Prohibit
        $settings.XmlResolver = $null
        $settings.MaxCharactersInDocument = 1MB
        $reader = [Xml.XmlReader]::Create($xml.FullName, $settings)
        $document = [Xml.XmlDocument]::new()
        try { $document.Load($reader) } finally { $reader.Dispose() }
        $copyright = $document.SelectSingleNode('/protocol/copyright')
        if ($copyright) {
            [IO.File]::AppendAllText($Notices, "`nProtocol specification: $($xml.Name)`n$($copyright.InnerText)`n", [Text.UTF8Encoding]::new($false))
        }
    }
    $sourceRoot = Join-Path $Work 'corresponding'
    [IO.Directory]::CreateDirectory($sourceRoot) | Out-Null
    $tar = Join-Path $Work 'source.tar'
    Invoke-PreviewCommand 'git' @('archive', '--format=tar', '--prefix=stemplayer-native-source/', '--output', $tar, 'HEAD')
    Invoke-PreviewCommand 'tar' @('-xf', $tar, '-C', $sourceRoot)
    $source = Join-Path $sourceRoot 'stemplayer-native-source'
    [IO.Directory]::CreateDirectory((Join-Path $source 'third-party')) | Out-Null
    Copy-Item -LiteralPath $crate -Destination (Join-Path $source 'third-party')
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot '../docs/release/REBUILD-LINUX.txt') -Destination $source
    Invoke-PreviewCommand 'tar' @('-czf', $Destination, '-C', $sourceRoot, 'stemplayer-native-source')
    [IO.File]::AppendAllText($Notices, "`nComplete application and wayland-protocols-plasma source: CORRESPONDING-SOURCE.tar.gz.`nSee its REBUILD-LINUX.txt to rebuild and replace the library. No restriction on modification or debugging is imposed.`n", [Text.UTF8Encoding]::new($false))
}

function Test-UnixPreviewStartup {
    param([string]$Binary, [string]$HomeDirectory)

    [IO.Directory]::CreateDirectory($HomeDirectory) | Out-Null
    $start = [Diagnostics.ProcessStartInfo]::new($Binary)
    $start.UseShellExecute = $false
    $start.Environment['HOME'] = $HomeDirectory
    foreach ($name in @('XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME')) {
        $start.Environment[$name] = Join-Path $HomeDirectory $name
    }
    $process = [Diagnostics.Process]::Start($start)
    try {
        if ($process.WaitForExit(5000)) { throw "Extracted product exited early with code $($process.ExitCode)" }
        Write-Output 'Extracted release executable remained running for the startup check'
    }
    finally {
        if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
        $process.Dispose()
    }
}

function Invoke-UnixPreviewPackage {
    if ($IsWindows -or $PSVersionTable.PSVersion.Major -lt 7) { throw 'Linux or macOS with PowerShell 7 is required' }
    if ([string]::IsNullOrEmpty($SourceDirectory)) { throw 'SourceDirectory is required' }
    $packagingRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
    $root = [IO.Path]::GetFullPath($SourceDirectory)
    $target = if ($IsMacOS) { 'aarch64-apple-darwin' } else { 'x86_64-unknown-linux-gnu' }
    $profile = Get-UnixPreviewProfile $target
    $packagingCommit = Invoke-PreviewCommand 'git' @('-C', $packagingRoot, 'rev-parse', 'HEAD')
    if (Invoke-PreviewCommand 'git' @('-C', $packagingRoot, 'status', '--porcelain', '--untracked-files=normal')) {
        throw 'Packaging checkout must be clean and committed'
    }
    $work = Join-Path $packagingRoot ('artifacts/unix-preview/work-' + [Guid]::NewGuid().ToString('N'))
    $directory = Join-Path $work "stemplayer-native-0.1.0-preview.1-$($profile.slug)"
    $tools = Join-Path $packagingRoot 'artifacts/release-tools/bin'
    $previous = @{
        CARGO_TARGET_DIR = $env:CARGO_TARGET_DIR; CARGO_ENCODED_RUSTFLAGS = $env:CARGO_ENCODED_RUSTFLAGS
        CARGO_NET_OFFLINE = $env:CARGO_NET_OFFLINE; MACOSX_DEPLOYMENT_TARGET = $env:MACOSX_DEPLOYMENT_TARGET
        LC_ALL = $env:LC_ALL
    }
    Push-Location $root
    try {
        $commit = Invoke-PreviewCommand 'git' @('rev-parse', 'HEAD')
        if ($commit -cne 'f589012f48c8a340609e61a751001a28cdbd33b9') { throw 'Use the unchanged published preview source commit' }
        Assert-PreviewSourceUnchanged $commit
        $pinned = (Select-String rust-toolchain.toml -Pattern '^channel = "([^"]+)"$').Matches.Groups[1].Value
        $rustc = Invoke-PreviewCommand 'rustc' @('--version')
        if (-not $rustc.StartsWith("rustc $pinned ", [StringComparison]::Ordinal)) { throw 'Wrong Rust toolchain' }
        $about = Join-Path $tools 'cargo-about'
        $cyclonedx = Join-Path $tools 'cargo-cyclonedx'
        if ((Invoke-PreviewCommand $about @('--version')) -cne 'cargo-about 0.9.2' -or
            (Invoke-PreviewCommand $cyclonedx @('cyclonedx', '--version')) -cne 'cargo-cyclonedx-cyclonedx 0.5.9') {
            throw 'Install the pinned release tools'
        }
        [IO.Directory]::CreateDirectory($directory) | Out-Null
        $env:CARGO_TARGET_DIR = Join-Path $work 'target'
        $env:LC_ALL = 'C'
        $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $HOME '.cargo' }
        $env:CARGO_ENCODED_RUSTFLAGS = @("--remap-path-prefix=$root=.", "--remap-path-prefix=$cargoHome=cargo") -join [char]0x1f
        if ($IsMacOS) { $env:MACOSX_DEPLOYMENT_TARGET = '14.0' }
        $metadata = Invoke-PreviewCommand 'cargo' @('metadata', '--locked', '--format-version', '1', '--filter-platform', $target,
            '--manifest-path', 'apps/stemplayer/Cargo.toml') | ConvertFrom-Json
        Invoke-PreviewCommand 'cargo' @('build', '--locked', '--release', '-p', 'stemplayer', '--target', $target)
        $built = Join-Path $env:CARGO_TARGET_DIR "$target/release/stemplayer"
        if ($IsMacOS) { New-MacosPreviewBundle $built $directory (Join-Path $root 'apps/stemplayer/assets/stemplayer.png') }
        else { Copy-Item -LiteralPath $built -Destination (Join-Path $directory 'stemplayer'); Invoke-PreviewCommand 'chmod' @('755', (Join-Path $directory 'stemplayer')) }
        Copy-Item -LiteralPath 'LICENSE-MIT', 'LICENSE-APACHE' -Destination $directory
        Copy-Item -LiteralPath (Join-Path $packagingRoot 'docs/release/README-UNIX.txt') -Destination (Join-Path $directory 'README.txt')
        $binary = Join-Path $directory $profile.executable
        $imports = Get-UnixPreviewImports $binary $target
        [IO.File]::WriteAllText((Join-Path $directory 'RUNTIME-DEPENDENCIES.txt'), ($imports -join "`n") + "`n", [Text.UTF8Encoding]::new($false))
        $notices = Join-Path $directory 'THIRD-PARTY-NOTICES.txt'
        $diagnostics = @(& $about @('generate', '--locked', '--fail', '--manifest-path', 'apps/stemplayer/Cargo.toml',
            '--target', $target, '--config', (Join-Path $packagingRoot 'about.toml'), '--output-file', $notices,
            (Join-Path $packagingRoot 'scripts/preview-notices.hbs')) 2>&1)
        $aboutExit = $LASTEXITCODE
        $diagnostics | Write-Output
        if ($aboutExit -ne 0) { throw "cargo-about failed with exit code $aboutExit" }
        Assert-PreviewCollectorDiagnostics $diagnostics
        Add-PreviewSlintLicense $metadata $notices
        Add-PreviewAacNotice $metadata $notices $commit
        if ($IsLinux) { New-UnixCorrespondingSource $metadata $work (Join-Path $directory 'CORRESPONDING-SOURCE.tar.gz') $notices }
        $env:CARGO_NET_OFFLINE = 'true'
        New-PreviewSbom $metadata $cyclonedx (Join-Path $directory 'sbom.cdx.json') $root $target
        $package = @($metadata.packages | Where-Object name -CEQ 'stemplayer')[0]
        $info = @{
            product = 'Stemplayer Native'; version = $package.version; source_commit = $commit; source_dirty = $false
            packaging_commit = $packagingCommit; target = $target; minimum_os = $profile.minimum_os; rustc = $rustc
            features = @($metadata.resolve.nodes | Where-Object id -CEQ $package.id | ForEach-Object features | Sort-Object)
            cargo_lock_sha256 = (Get-FileHash Cargo.lock -Algorithm SHA256).Hash.ToLowerInvariant()
            executable_sha256 = (Get-FileHash $binary -Algorithm SHA256).Hash.ToLowerInvariant()
            publisher_signed = $false; signature_kind = $profile.signing; qualification = 'not-qualified'
        }
        [IO.File]::WriteAllText((Join-Path $directory 'BUILD-INFO.json'), ($info | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
        foreach ($file in Get-ChildItem $directory -Recurse -File | Where-Object { $_.Extension -in @('.txt', '.json', '.plist') }) {
            $text = [IO.File]::ReadAllText($file.FullName)
            foreach ($path in @($root, $packagingRoot, $HOME, $cargoHome)) {
                if ($text.Contains($path, [StringComparison]::Ordinal)) { throw 'Private build path in Unix package' }
            }
        }
        Assert-PreviewSourceUnchanged $commit
        if ((Invoke-PreviewCommand 'git' @('-C', $packagingRoot, 'rev-parse', 'HEAD')) -cne $packagingCommit -or
            (Invoke-PreviewCommand 'git' @('-C', $packagingRoot, 'status', '--porcelain', '--untracked-files=normal'))) {
            throw 'Packaging checkout changed during the build'
        }
        $assets = @(New-UnixPreviewArchive $directory (Join-Path $packagingRoot "artifacts/unix-preview/$($profile.slug)/0.1.0-preview.1") $target)
        $extracted = Join-Path $work ('extracted path ' + [char]0x00dc)
        [IO.Directory]::CreateDirectory($extracted) | Out-Null
        if ($IsMacOS) { Invoke-PreviewCommand 'ditto' @('-x', '-k', $assets[0], $extracted) }
        else { Invoke-PreviewCommand 'tar' @('-xzf', $assets[0], '-C', $extracted) }
        if (@(Get-ChildItem -LiteralPath $extracted -Force).Count -ne 1) { throw 'Unexpected archive root entries' }
        $unpacked = Join-Path $extracted (Split-Path -Leaf $directory)
        Assert-UnixPreviewPayload $unpacked $target | Out-Null
        if ($IsMacOS) { Invoke-PreviewCommand 'codesign' @('--verify', '--deep', '--strict', (Join-Path $unpacked 'Stemplayer Native.app')) }
        Test-UnixPreviewStartup (Join-Path $unpacked $profile.executable) (Join-Path $work 'isolated-home')
        $assets
    }
    finally {
        foreach ($name in $previous.Keys) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
        Pop-Location
    }
}

if ($MyInvocation.InvocationName -ne '.') { Invoke-UnixPreviewPackage }
