# Shared assertion helpers for the install.ps1 test suites.
#
# Deliberately NOT Pester. `tests/install_sh/` uses hand-rolled bash harnesses
# with pass/fail counters, and mirroring that here keeps the two installers'
# test suites structurally identical. It also keeps CI free of a PSGallery
# dependency, which is one less network dependency in a job whose whole point
# is to be fast and deterministic.
#
# Every suite dot-sources this file, then dot-sources install.ps1 with
# GARRAIA_INSTALL_PS1_LIBRARY=1 so Invoke-Main never runs.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:Passed = 0
$script:Failed = 0

# Returns the path to install.ps1 with library mode already armed, so the
# CALLER can dot-source it: `. (Get-InstallerPath)`.
#
# The dot-source has to happen at the suite's own scope. Doing it inside this
# function would load install.ps1's functions into the function scope, where
# they evaporate the moment it returns.
function Get-InstallerPath {
    # Join-Path aninhado de proposito. Windows PowerShell 5.1 nao tem o parametro
    # -AdditionalChildPath (chegou no PowerShell 6), entao a forma de tres
    # argumentos `Join-Path $x '..' '..'` falha ali com "A positional parameter
    # cannot be found that accepts argument '..'". Nao "simplifique" de volta: a
    # perna windows-latest do CI roda 5.1 justamente para pegar isto, e ja pegou uma
    # vez. O guarda em run_lint.ps1 rejeita a forma de tres argumentos.
    $repoRoot = (Resolve-Path (Join-Path (Join-Path $PSScriptRoot '..') '..')).Path
    $installer = Join-Path $repoRoot 'install.ps1'
    if (-not (Test-Path $installer)) {
        throw "install.ps1 not found at $installer"
    }
    $env:GARRAIA_INSTALL_PS1_LIBRARY = '1'
    return $installer
}

function Assert-Pass { param([string]$Label) $script:Passed++; Write-Host "  PASS: $Label" }
function Assert-Fail { param([string]$Label) $script:Failed++; Write-Host "  FAIL: $Label" -ForegroundColor Red }

function Assert-Equal {
    param([string]$Label, $Expected, $Actual)
    # Normalize $null and empty string: PowerShell returns $null where the
    # shell would return "", and the distinction is never meaningful here.
    $e = if ($null -eq $Expected) { '' } else { [string]$Expected }
    $a = if ($null -eq $Actual)   { '' } else { [string]$Actual }
    if ($e -ceq $a) { Assert-Pass "$Label ($a)" } else { Assert-Fail "${Label}: expected '$e', got '$a'" }
}

function Assert-True {
    param([string]$Label, $Condition)
    if ($Condition) { Assert-Pass $Label } else { Assert-Fail "${Label}: expected true" }
}

function Assert-False {
    param([string]$Label, $Condition)
    if (-not $Condition) { Assert-Pass $Label } else { Assert-Fail "${Label}: expected false" }
}

function Assert-Throws {
    param([string]$Label, [scriptblock]$Action)
    try {
        & $Action | Out-Null
        Assert-Fail "${Label}: expected a throw, none happened"
    } catch {
        Assert-Pass "$Label (threw: $($_.Exception.Message))"
    }
}

# Clear every installer env var so one suite cannot leak state into the next.
function Clear-InstallerEnvironment {
    foreach ($name in @(
        'GARRAIA_SKIP_INIT', 'GARRAIA_SKIP_START', 'GARRAIA_BOOTSTRAP_LOCAL',
        'GARRAIA_VERSION', 'GARRAIA_INSTALL_DIR', 'GARRAIA_NO_PATH'
    )) {
        if (Test-Path "Env:$name") { Remove-Item "Env:$name" }
    }
}

$script:Skipped = 0

# Some behavior is only observable on Windows: [IO.Path]::GetFullPath does not
# understand 'C:\Windows' on Linux, and HKCU\Environment does not exist there.
# Those assertions are skipped rather than faked, and the Windows leg of the CI
# matrix is what actually covers them.
function Assert-Skip {
    param([string]$Label, [string]$Reason)
    $script:Skipped++
    Write-Host "  SKIP: $Label ($Reason)"
}

function Exit-WithSummary {
    param([string]$Suite)
    Write-Host ''
    Write-Host "$Suite : $script:Passed passed, $script:Failed failed, $script:Skipped skipped"
    if ($script:Failed -gt 0) { exit 1 }
    exit 0
}
