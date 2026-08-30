# PSScriptAnalyzer gate for the Windows installer and its test suites.
#
# Lives in a script rather than inline in the workflow for two reasons. The
# practical one: it can be run locally, which is how it should have been
# validated in the first place. The structural one: with the logic here, no
# workflow step needs a `shell:` key, and `shell:` is the one place an
# expression is NOT allowed -- `shell: ${{ matrix.shell }}` is what stopped
# ci.yml from starting at all (actionlint: "context matrix is not allowed
# here"). Every step can now be a plain `run:` that launches an interpreter.
#
# Usage:
#   pwsh       -NoProfile -File tests/install_ps1/run_lint.ps1   # PowerShell 7
#   powershell -NoProfile -File tests/install_ps1/run_lint.ps1   # Windows PS 5.1
#
# Exits 1 on any finding. The two settings files carry their exclusions with
# written justifications, so anything that reaches here is a real regression.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..' '..')).Path
$settingsShipping = Join-Path $PSScriptRoot 'PSScriptAnalyzerSettings.psd1'
$settingsTests    = Join-Path $PSScriptRoot 'PSScriptAnalyzerSettings.Tests.psd1'

Import-Module PSScriptAnalyzer -ErrorAction Stop

$failed = $false

function Invoke-Gate {
    param(
        [Parameter(Mandatory)][string]$Label,
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Settings
    )

    # @() so an empty result is an empty array rather than $null -- $null.Count
    # throws under StrictMode, and "no findings" is the expected case.
    $findings = @(Invoke-ScriptAnalyzer -Path $Path -Settings $Settings)
    if ($findings.Count -eq 0) {
        Write-Host "  OK   $Label"
        return $false
    }

    Write-Host "  FAIL $Label"
    $findings | Format-Table -AutoSize RuleName, Severity, ScriptName, Line, Message |
        Out-String -Width 200 | Write-Host
    return $true
}

Write-Host 'PSScriptAnalyzer:'

# Shipping scripts: the stricter ruleset.
foreach ($relative in @('install.ps1', 'scripts/build-installer.ps1')) {
    $full = Join-Path $repoRoot $relative
    if (Invoke-Gate -Label $relative -Path $full -Settings $settingsShipping) { $failed = $true }
}

# The suites themselves, under a ruleset appropriate to test doubles. Linting
# test code matters: it is exactly where style debt accumulates unnoticed.
if (Invoke-Gate -Label 'tests/install_ps1' -Path $PSScriptRoot -Settings $settingsTests) { $failed = $true }

if ($failed) {
    Write-Host ''
    Write-Host 'PSScriptAnalyzer found issues.'
    exit 1
}

Write-Host ''
Write-Host 'PSScriptAnalyzer: clean'
exit 0
