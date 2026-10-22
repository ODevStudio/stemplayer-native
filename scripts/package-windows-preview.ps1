Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-PreviewCommand {
    param([string]$Command, [string[]]$Arguments)

    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command failed with exit code $LASTEXITCODE"
    }
}

function Assert-PreviewSystemLibraries {
    param([string[]]$Libraries)

    $system = @('advapi32.dll', 'bcrypt.dll', 'bcryptprimitives.dll', 'combase.dll',
        'comctl32.dll', 'crypt32.dll', 'dwrite.dll', 'dwmapi.dll', 'gdi32.dll',
        'imm32.dll', 'kernel32.dll', 'kernelbase.dll', 'mmdevapi.dll', 'ncrypt.dll',
        'ntdll.dll', 'ole32.dll', 'oleaut32.dll', 'secur32.dll', 'shell32.dll',
        'shlwapi.dll', 'uiautomationcore.dll', 'user32.dll', 'userenv.dll',
        'ucrtbase.dll', 'uxtheme.dll', 'version.dll', 'winmm.dll', 'ws2_32.dll')
    $unexpected = @($Libraries | Where-Object {
        $_ -notin $system -and $_ -notmatch '^api-ms-win-[a-z0-9-]+\.dll$'
    })
    if ($Libraries.Count -eq 0 -or $unexpected.Count -gt 0) {
        throw "Unapproved Windows imports: $($unexpected -join ', ')"
    }
}

function Assert-PreviewImports {
    param([string]$Binary)

    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $dumpbin = Invoke-PreviewCommand $vswhere @('-latest', '-products', '*', '-requires',
        'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-find', 'VC/Tools/MSVC/**/bin/Hostx64/x64/dumpbin.exe') | Select-Object -Last 1
    if (-not $dumpbin) { throw 'MSVC Build Tools with dumpbin are required' }
    $imports = @(Invoke-PreviewCommand $dumpbin @('/dependents', $Binary) | ForEach-Object {
        if ($_ -match '^\s+([a-zA-Z0-9_.-]+\.dll)\s*$') { $Matches[1].ToLowerInvariant() }
    })
    Assert-PreviewSystemLibraries $imports
}

function Get-PreviewFiles {
    @('stemplayer.exe', 'README.txt', 'LICENSE-MIT', 'LICENSE-APACHE',
      'THIRD-PARTY-NOTICES.txt', 'sbom.cdx.json', 'BUILD-INFO.json')
}

function Assert-PreviewPayload {
    param([string]$Directory)

    $files = @(Get-ChildItem -LiteralPath $Directory -Force)
    if ($files.Count -ne (Get-PreviewFiles).Count) {
        throw 'Preview payload must contain only the approved files'
    }
    foreach ($name in Get-PreviewFiles) {
        $file = Get-Item -LiteralPath (Join-Path $Directory $name)
        $limit = if ($name -eq 'stemplayer.exe') { 128MB } elseif ($name -eq 'BUILD-INFO.json') { 64KB } else { 16MB }
        if ($file.PSIsContainer -or $file.Length -eq 0 -or $file.Length -gt $limit -or
            ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "Invalid preview input: $name"
        }
    }
    $info = Get-Content -LiteralPath (Join-Path $Directory 'BUILD-INFO.json') -Raw | ConvertFrom-Json
    if ($info.product -cne 'Stemplayer Native' -or $info.version -cnotmatch '^\d+\.\d+\.\d+-preview\.\d+$' -or
        $info.source_commit -cnotmatch '^[0-9a-f]{40}$' -or $info.source_dirty -isnot [bool] -or $info.source_dirty -or
        $info.target -cne 'x86_64-pc-windows-msvc' -or $info.signed -isnot [bool] -or $info.signed -or
        $info.qualification -cne 'not-qualified' -or $info.rustc -notmatch '^rustc ' -or
        $info.cargo_lock_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
        (@($info.features | Sort-Object) -join ',') -cne 'aac,alac,default,hls,local-mp4,local-wav,product') {
        throw 'Invalid preview build identity or feature set'
    }
    $hash = (Get-FileHash -LiteralPath (Join-Path $Directory 'stemplayer.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -cne $info.executable_sha256) {
        throw 'Executable checksum does not match build identity'
    }
    $bom = Get-Content -LiteralPath (Join-Path $Directory 'sbom.cdx.json') -Raw | ConvertFrom-Json
    if ($bom.bomFormat -cne 'CycloneDX' -or $bom.specVersion -cne '1.5' -or
        $bom.metadata.component.name -cne 'stemplayer' -or
        $bom.metadata.component.version -cne $info.version -or @($bom.components).Count -eq 0) {
        throw 'Invalid preview SBOM or product version'
    }
    $notices = Get-Content -LiteralPath (Join-Path $Directory 'THIRD-PARTY-NOTICES.txt') -Raw
    if (-not $notices.Contains('Slint Royalty-free Desktop, Mobile, and Web Applications License') -or
        -not $notices.Contains('MPL-2.0') -or -not $notices.Contains('Mozilla Public License')) {
        throw 'Required dependency license text is missing'
    }
    return $info
}

function New-PreviewArchive {
    param([string]$Directory, [string]$OutputDirectory)

    $info = Assert-PreviewPayload $Directory
    $name = "stemplayer-native-$($info.version)-windows-x86_64"
    if ((Split-Path -Leaf $Directory) -cne $name) {
        throw 'Payload directory does not match the product version'
    }
    $zip = Join-Path $OutputDirectory "$name.zip"
    $checksum = Join-Path $OutputDirectory 'SHA256SUMS.txt'
    if ((Test-Path -LiteralPath $zip) -or (Test-Path -LiteralPath $checksum)) {
        throw 'Refusing to overwrite existing preview assets'
    }
    [IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null
    Compress-Archive -LiteralPath $Directory -DestinationPath $zip -CompressionLevel Optimal
    $hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText($checksum, "$hash  $name.zip`n", [Text.UTF8Encoding]::new($false))
    Write-Output $zip
    Write-Output $checksum
}

function Add-PreviewSlintLicense {
    param([object]$Metadata, [string]$Notices)

    $slint = @($Metadata.packages | Where-Object name -EQ 'slint')
    if ($slint.Count -ne 1 -or $slint[0].version -cne '1.17.1') {
        throw 'Slint license clarification must be reviewed for this version'
    }
    $license = Join-Path (Split-Path -Parent $slint[0].manifest_path) 'LICENSES/LicenseRef-Slint-Royalty-free-2.0.md'
    $hash = (Get-FileHash -LiteralPath $license -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -cne '5167f5056e850419106ab6265efbdca7cba4d99c849d1445ca0bbf6a1e2315fe') {
        throw 'Slint royalty-free license text changed'
    }
    [IO.File]::AppendAllText($Notices, "`n" + [IO.File]::ReadAllText($license), [Text.UTF8Encoding]::new($false))
}

function New-PreviewSbom {
    param([object]$Metadata, [string]$Tool, [string]$Destination, [string]$Root)

    $basename = 'preview-sbom-' + [Guid]::NewGuid().ToString('N')
    $paths = @($Metadata.packages | Where-Object { $_.id -in $Metadata.workspace_members } | ForEach-Object {
        $path = [IO.Path]::GetFullPath((Join-Path (Split-Path -Parent $_.manifest_path) "$basename.json"))
        if (-not $path.StartsWith($Root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
            (Test-Path -LiteralPath $path)) {
            throw 'Unsafe SBOM output path'
        }
        $path
    })
    $lock = (Get-FileHash -LiteralPath (Join-Path $Root 'Cargo.lock') -Algorithm SHA256).Hash
    try {
        Invoke-PreviewCommand $Tool @('cyclonedx', '--manifest-path', 'apps/stemplayer/Cargo.toml',
            '--target', 'x86_64-pc-windows-msvc', '--format', 'json', '--spec-version', '1.5',
            '--all', '--override-filename', $basename)
        $generated = Join-Path $Root "apps/stemplayer/$basename.json"
        if ((Get-Item -LiteralPath $generated).Length -gt 16MB) {
            throw 'Generated SBOM exceeds the preview limit'
        }
        $bom = Get-Content -LiteralPath $generated -Raw | ConvertFrom-Json -AsHashtable
        $packages = @($Metadata.packages | Where-Object { $_.id -in $Metadata.workspace_members })
        $sanitized = Convert-PreviewSbomRefs $bom $packages
        [IO.File]::WriteAllText($Destination, (ConvertTo-Json -InputObject $sanitized -Depth 100), [Text.UTF8Encoding]::new($false))
        if ((Get-FileHash -LiteralPath (Join-Path $Root 'Cargo.lock') -Algorithm SHA256).Hash -cne $lock) {
            throw 'SBOM generation changed Cargo.lock'
        }
    }
    finally {
        foreach ($path in $paths) {
            if (Test-Path -LiteralPath $path) {
                Remove-Item -LiteralPath $path
            }
        }
    }
}

function Convert-PreviewSbomRefs {
    param([AllowNull()][object]$Value, [object[]]$Packages)

    if ($Value -is [Collections.IDictionary]) {
        $result = [System.Collections.Immutable.ImmutableDictionary[string,object]]::Empty
        foreach ($key in $Value.Keys) {
            $result = $result.Add($key, (Convert-PreviewSbomRefs $Value[$key] $Packages))
        }
        return $result
    }
    if ($Value -is [string]) {
        foreach ($package in $Packages) {
            if ($Value -ceq $package.id -or $Value.StartsWith($package.id + ' ', [StringComparison]::Ordinal)) {
                return "workspace:$($package.name)@$($package.version)" + $Value.Substring($package.id.Length)
            }
        }
        return $Value
    }
    if ($Value -is [Collections.IEnumerable]) {
        return ,@($Value | ForEach-Object { Convert-PreviewSbomRefs $_ $Packages })
    }
    return $Value
}

function Invoke-WindowsPreviewPackage {
    if ($PSVersionTable.PSVersion.Major -lt 7 -or -not $IsWindows) {
        throw 'Windows and PowerShell 7 or newer are required'
    }
    $root = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
    Push-Location $root
    $previous = @{
        CARGO_TARGET_DIR = $env:CARGO_TARGET_DIR
        CARGO_ENCODED_RUSTFLAGS = $env:CARGO_ENCODED_RUSTFLAGS
        CARGO_NET_OFFLINE = $env:CARGO_NET_OFFLINE
    }
    try {
        if (Invoke-PreviewCommand 'git' @('status', '--porcelain', '--untracked-files=normal')) {
            throw 'Preview packaging requires a clean committed checkout'
        }
        $commit = Invoke-PreviewCommand 'git' @('rev-parse', 'HEAD')
        $metadata = Invoke-PreviewCommand 'cargo' @('metadata', '--locked', '--format-version', '1',
            '--filter-platform', 'x86_64-pc-windows-msvc', '--manifest-path', 'apps/stemplayer/Cargo.toml') | ConvertFrom-Json
        $package = @($metadata.packages | Where-Object name -EQ 'stemplayer')[0]
        if ($package.version -cnotmatch '^\d+\.\d+\.\d+-preview\.\d+$') {
            throw 'Only preview versions may use this packaging path'
        }
        $tools = Join-Path $root 'artifacts/release-tools/bin'
        $about = Join-Path $tools 'cargo-about.exe'
        $cyclonedx = Join-Path $tools 'cargo-cyclonedx.exe'
        if ((Invoke-PreviewCommand $about @('--version')) -cne 'cargo-about 0.9.2' -or
            (Invoke-PreviewCommand $cyclonedx @('cyclonedx', '--version')) -cne 'cargo-cyclonedx-cyclonedx 0.5.9') {
            throw 'Install the pinned release tools from the preview runbook'
        }
        $pinned = (Select-String -LiteralPath 'rust-toolchain.toml' -Pattern '^channel = "([^"]+)"$').Matches.Groups[1].Value
        $rustc = Invoke-PreviewCommand 'rustc' @('--version')
        if (-not $pinned -or -not $rustc.StartsWith("rustc $pinned ", [StringComparison]::Ordinal)) {
            throw 'The active Rust compiler does not match rust-toolchain.toml'
        }
        $work = Join-Path $root ('artifacts/windows-preview/work-' + [Guid]::NewGuid().ToString('N'))
        $payload = Join-Path $work "stemplayer-native-$($package.version)-windows-x86_64"
        [IO.Directory]::CreateDirectory($payload) | Out-Null
        $env:CARGO_TARGET_DIR = Join-Path $work 'target'
        $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $HOME '.cargo' }
        $env:CARGO_ENCODED_RUSTFLAGS = @("--remap-path-prefix=$root=.", "--remap-path-prefix=$cargoHome=cargo", '-Ctarget-feature=+crt-static') -join [char]0x1f
        Invoke-PreviewCommand 'cargo' @('build', '--locked', '--release', '-p', 'stemplayer', '--target', 'x86_64-pc-windows-msvc')
        Assert-PreviewImports (Join-Path $env:CARGO_TARGET_DIR 'x86_64-pc-windows-msvc/release/stemplayer.exe')
        Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'x86_64-pc-windows-msvc/release/stemplayer.exe') -Destination $payload
        Copy-Item -LiteralPath 'docs/release/README.txt' -Destination $payload
        Copy-Item -LiteralPath 'LICENSE-MIT', 'LICENSE-APACHE' -Destination $payload
        $notices = Join-Path $payload 'THIRD-PARTY-NOTICES.txt'
        Invoke-PreviewCommand $about @('generate', '--locked', '--offline', '--fail', '--manifest-path',
            'apps/stemplayer/Cargo.toml', '--target', 'x86_64-pc-windows-msvc', '--config', 'about.toml',
            '--output-file', $notices, 'scripts/preview-notices.hbs')
        Add-PreviewSlintLicense $metadata $notices
        $env:CARGO_NET_OFFLINE = 'true'
        New-PreviewSbom $metadata $cyclonedx (Join-Path $payload 'sbom.cdx.json') $root
        $info = [ordered]@{
            product = 'Stemplayer Native'
            version = $package.version
            source_commit = $commit
            source_dirty = $false
            target = 'x86_64-pc-windows-msvc'
            crt_linkage = 'static'
            features = @($metadata.resolve.nodes | Where-Object id -EQ $package.id | ForEach-Object features | Sort-Object)
            rustc = $rustc
            cargo_lock_sha256 = (Get-FileHash -LiteralPath 'Cargo.lock' -Algorithm SHA256).Hash.ToLowerInvariant()
            executable_sha256 = (Get-FileHash -LiteralPath (Join-Path $payload 'stemplayer.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
            signed = $false
            qualification = 'not-qualified'
        }
        [IO.File]::WriteAllText((Join-Path $payload 'BUILD-INFO.json'), ($info | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
        foreach ($file in Get-PreviewFiles | Where-Object { $_ -ne 'stemplayer.exe' }) {
            $text = [IO.File]::ReadAllText((Join-Path $payload $file))
            foreach ($privatePath in @($root, $HOME, $cargoHome)) {
                if ($text.Contains($privatePath, [StringComparison]::OrdinalIgnoreCase) -or
                    $text.Contains($privatePath.Replace('\', '/'), [StringComparison]::OrdinalIgnoreCase) -or
                    $text.Contains($privatePath.Replace('\', '\\'), [StringComparison]::OrdinalIgnoreCase)) {
                    throw "Private build path in $file"
                }
            }
        }
        if (Invoke-PreviewCommand 'git' @('status', '--porcelain', '--untracked-files=normal')) {
            throw 'Source checkout changed during preview build'
        }
        New-PreviewArchive $payload (Join-Path $root "artifacts/windows-preview/$($package.version)")
    }
    finally {
        $env:CARGO_TARGET_DIR = $previous.CARGO_TARGET_DIR
        $env:CARGO_ENCODED_RUSTFLAGS = $previous.CARGO_ENCODED_RUSTFLAGS
        $env:CARGO_NET_OFFLINE = $previous.CARGO_NET_OFFLINE
        Pop-Location
    }
}

if ($MyInvocation.InvocationName -ne '.') {
    Invoke-WindowsPreviewPackage
}
