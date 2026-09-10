$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Command,
        [string[]] $Arguments = @()
    )

    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command $($Arguments -join ' ') failed with exit code $LASTEXITCODE."
    }
}

Push-Location $repoRoot
try {
    Invoke-Checked -Command cargo -Arguments @("fmt", "--check")
    Invoke-Checked -Command cargo -Arguments @("test")
    Invoke-Checked -Command cargo -Arguments @("clippy", "--all-targets", "--", "-D", "warnings")
    Invoke-Checked -Command cargo -Arguments @("build")
} finally {
    Pop-Location
}
