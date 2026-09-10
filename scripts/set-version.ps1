param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$')]
    [string] $Version
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Write-Utf8NoBom {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path,
        [Parameter(Mandatory = $true)]
        [string] $Content
    )

    $encoding = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllText($Path, $Content, $encoding)
}

function Replace-Required {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path,
        [Parameter(Mandatory = $true)]
        [string] $Pattern,
        [Parameter(Mandatory = $true)]
        [string] $Replacement,
        [Parameter(Mandatory = $true)]
        [string] $Description
    )

    $content = [System.IO.File]::ReadAllText($Path)
    $match = [regex]::Match($content, $Pattern)
    if (-not $match.Success) {
        throw "Could not find $Description in $Path."
    }

    $updated = [regex]::Replace($content, $Pattern, $Replacement, 1)
    Write-Utf8NoBom -Path $Path -Content $updated
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$cargoToml = Join-Path $repoRoot "Cargo.toml"
$cargoLock = Join-Path $repoRoot "Cargo.lock"

Replace-Required `
    -Path $cargoToml `
    -Pattern '(?ms)(^\[package\]\s*.*?^version\s*=\s*")([^"]+)(")' `
    -Replacement "`${1}$Version`${3}" `
    -Description "the package version"

if (Test-Path $cargoLock) {
    Replace-Required `
        -Path $cargoLock `
        -Pattern '(?ms)(\[\[package\]\]\s*name\s*=\s*"mega-win-alt-tab"\s*version\s*=\s*")([^"]+)(")' `
        -Replacement "`${1}$Version`${3}" `
        -Description "the mega-win-alt-tab lockfile package version"
}
