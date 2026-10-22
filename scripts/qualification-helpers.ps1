function Get-QualificationPath([string]$Workspace, [string]$Path) {
    $base = [IO.Path]::GetFullPath($Workspace).TrimEnd([char[]]"\/")
    $target = [IO.Path]::GetFullPath($Path).TrimEnd([char[]]"\/")
    $prefix = $base + [IO.Path]::DirectorySeparatorChar
    $comparison = if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        [StringComparison]::OrdinalIgnoreCase
    } else { [StringComparison]::Ordinal }
    if (-not $target.StartsWith($prefix, $comparison)) {
        throw 'Qualification output must be a child of the native workspace'
    }
    for ($current = $target; $current; $current = [IO.Path]::GetDirectoryName($current)) {
        try {
            $attributes = [IO.File]::GetAttributes($current)
            if ($attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw 'Qualification paths must not contain links or reparse points'
            }
        } catch [IO.FileNotFoundException] {
        } catch [IO.DirectoryNotFoundException] {
        }
    }
    $target
}

function New-QualificationDirectory([string]$Workspace, [string]$Path) {
    $target = Get-QualificationPath $Workspace $Path
    New-Item -ItemType Directory -Path $target -ErrorAction Stop | Out-Null
    Get-QualificationPath $Workspace $target
}

function Test-SelfCheckReport([object]$Report, [int]$ExitCode) {
    $checks = @($Report.checks)
    $ExitCode -eq 0 -and $Report.status -eq 'pass' -and $checks.Count -gt 0 -and
        @($checks | Where-Object { $_.status -ne 'pass' }).Count -eq 0
}

function Test-ResourceEvidence([object[]]$Steady, [object[]]$SteadyFps) {
    if ($null -eq $Steady -or $null -eq $SteadyFps) { return $false }
    $Steady.Count -gt 0 -and $SteadyFps.Count -gt 0 -and
        @($Steady | Where-Object {
            $null -eq $_.private_bytes -or $null -eq $_.cpu_percent_one_core -or
            [double]::IsNaN([double]$_.cpu_percent_one_core) -or
            [double]::IsInfinity([double]$_.cpu_percent_one_core) -or
            $_.private_bytes -lt 0 -or $_.cpu_percent_one_core -lt 0
        }).Count -eq 0 -and
        @($SteadyFps | Where-Object { $null -eq $_ -or $_ -lt 0 }).Count -eq 0
}
