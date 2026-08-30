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

# Join-Path aninhado de proposito. Windows PowerShell 5.1 nao tem o parametro
# -AdditionalChildPath (chegou no PowerShell 6), entao a forma de tres
# argumentos `Join-Path $x '..' '..'` falha ali com "A positional parameter
# cannot be found that accepts argument '..'". Nao "simplifique" de volta: a
# perna windows-latest do CI roda 5.1 justamente para pegar isto, e ja pegou uma
# vez. O guarda em run_lint.ps1 rejeita a forma de tres argumentos.
$repoRoot = (Resolve-Path (Join-Path (Join-Path $PSScriptRoot '..') '..')).Path
$settingsShipping = Join-Path $PSScriptRoot 'PSScriptAnalyzerSettings.psd1'
$settingsTests    = Join-Path $PSScriptRoot 'PSScriptAnalyzerSettings.Tests.psd1'

Import-Module PSScriptAnalyzer -ErrorAction Stop

$failed = $false

# Single list, consumed by both the arity guard and the analyzer below, so a
# new script cannot be added to one and forgotten in the other.
$shippingScripts = @('install.ps1', 'scripts/build-installer.ps1')
$allScripts = $shippingScripts + @(
    Get-ChildItem -Path $PSScriptRoot -Filter '*.ps1' |
        ForEach-Object { "tests/install_ps1/$($_.Name)" }
)

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

# PowerShell 5.1 has no -AdditionalChildPath on Join-Path (it arrived in
# PowerShell 6), so `Join-Path $a 'b' 'c'` dies there with "A positional
# parameter cannot be found that accepts argument 'c'". PSScriptAnalyzer's
# compatibility rules do NOT catch this -- they check command existence and
# named parameters, not positional arity (verified: PSUseCompatibleSyntax and
# PSUseCompatibleCommands both report nothing for the three-argument form).
#
# That gap cost a CI cycle: two of these shipped and only the windows-latest
# leg caught them. This walks the AST so a `pwsh 7` run catches the class
# locally, which is the only way to see it without a Windows machine.
function Get-IncompatibleJoinPath {
    param([Parameter(Mandatory)][string]$Path)

    $parseErrors = $null
    $tokens = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile($Path, [ref]$tokens, [ref]$parseErrors)
    if ($parseErrors) { return ,@() }

    $calls = $ast.FindAll({
        param($node)
        $node -is [System.Management.Automation.Language.CommandAst] -and
        $node.GetCommandName() -eq 'Join-Path'
    }, $true)

    $bad = @()
    foreach ($call in $calls) {
        # Skip the command name, then keep only bare arguments -- a
        # CommandParameterAst is an explicit -Name, which is fine at any count.
        $positional = @(
            $call.CommandElements |
                Select-Object -Skip 1 |
                Where-Object { $_ -isnot [System.Management.Automation.Language.CommandParameterAst] }
        )
        if ($positional.Count -ge 3) {
            $bad += [pscustomobject]@{
                Line = $call.Extent.StartLineNumber
                Text = $call.Extent.Text
            }
        }
    }
    return ,$bad
}

Write-Host 'PowerShell 5.1 compatibility (Join-Path arity):'
$arityFailed = $false
foreach ($relative in $allScripts) {
    $full = Join-Path $repoRoot $relative
    $bad = Get-IncompatibleJoinPath -Path $full
    if ($bad.Count -eq 0) { continue }
    $arityFailed = $true
    foreach ($b in $bad) {
        Write-Host ("  FAIL {0}:{1}  {2}" -f $relative, $b.Line, $b.Text)
    }
}
if ($arityFailed) {
    Write-Host '  Join-Path takes at most two positional arguments on Windows PowerShell 5.1.'
    Write-Host "  Nest instead: Join-Path (Join-Path `$a '..') '..'"
    $failed = $true
} else {
    Write-Host '  OK   no Join-Path call exceeds two positional arguments'
}

Write-Host ''
Write-Host 'PSScriptAnalyzer:'

# Shipping scripts: the stricter ruleset.
foreach ($relative in $shippingScripts) {
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
