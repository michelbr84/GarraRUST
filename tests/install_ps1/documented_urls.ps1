# Contract tests for the install URLs documented inside install.ps1 (the
# PowerShell counterpart of tests/install_sh/documented_urls.sh).
#
# Regression guard for the garraia.org Windows-installer outage: the site
# served /install.sh but had no route for /install.ps1, so the SPA fallback
# answered the PowerShell one-liner with HTML and HTTP 200 -- `irm` downloaded
# the homepage and `iex` tried to execute it. The endpoint fix lives in the
# website repo; the canonical URL and the mirror list are documented HERE, and
# the two must not drift apart.
#
# These assertions read the script text -- never the network -- so CI stays
# hermetic. The live-endpoint probe is .github/workflows/install-endpoints.yml,
# which is scheduled, not a PR gate.

. (Join-Path $PSScriptRoot '_harness.ps1')

$repoRoot   = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$installPs1 = Join-Path $repoRoot 'install.ps1'
$installSh  = Join-Path $repoRoot 'install.sh'

$ps1Body = Get-Content -Path $installPs1 -Raw
$shBody  = Get-Content -Path $installSh  -Raw

Write-Host 'install.ps1: canonical URL and mirrors'

Assert-True 'documenta a URL canonica garraia.org' `
    $ps1Body.Contains('https://garraia.org/install.ps1')

Assert-True 'documenta a forma irm | iex' `
    $ps1Body.Contains('irm https://garraia.org/install.ps1 | iex')

foreach ($mirror in @(
    'https://github.com/michelbr84/GarraRUST/releases/latest/download/install.ps1',
    'https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.ps1',
    'https://cdn.jsdelivr.net/gh/michelbr84/GarraRUST@main/install.ps1'
)) {
    Assert-True "lista o espelho $mirror" $ps1Body.Contains($mirror)
}

Write-Host ''
Write-Host 'install.ps1: nunca ensina a forma Unix'

# `curl -fsSL` no Windows PowerShell resolve para Invoke-WebRequest, que nao
# aceita essas flags -- o usuario levaria um erro de parametro antes de baixar
# qualquer coisa. A unica mencao tolerada e a que explica o analogo shell.
$curlLines = @(
    Get-Content -Path $installPs1 |
        Where-Object { $_ -match 'curl -fsSL' } |
        Where-Object { $_ -notmatch 'analogue|an.logo' }
)
Assert-Equal 'nenhuma linha ensina curl -fsSL como comando Windows' 0 $curlLines.Count

Write-Host ''
Write-Host 'paridade com install.sh'

# CLAUDE.md 16: os dois instaladores sao o mesmo contrato em dois sistemas.
Assert-True 'install.sh documenta a URL canonica garraia.org' `
    $shBody.Contains('https://garraia.org/install.sh')
Assert-False 'install.sh nao ensina irm/iex' $shBody.Contains('| iex')

foreach ($mirror in @(
    'https://github.com/michelbr84/GarraRUST/releases/latest/download/install.sh',
    'https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh',
    'https://cdn.jsdelivr.net/gh/michelbr84/GarraRUST@main/install.sh'
)) {
    Assert-True "install.sh lista o espelho $mirror" $shBody.Contains($mirror)
}

Write-Host ''
Write-Host 'nenhum download por http:// puro'

Assert-False 'install.ps1 sem http:// em raw.githubusercontent' `
    $ps1Body.Contains('http://raw.githubusercontent.com')
Assert-False 'install.sh sem http:// em raw.githubusercontent' `
    $shBody.Contains('http://raw.githubusercontent.com')

Exit-WithSummary 'documented_urls'
