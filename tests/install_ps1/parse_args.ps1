# Unit tests for Get-GarraiaConfig in install.ps1 (the PowerShell counterpart of
# tests/install_sh/parse_args.sh).
#
# The flags are a contract, not a convenience: `irm | iex` cannot carry
# arguments, so the documented way to drive an unattended Windows install is
# `& ([scriptblock]::Create((irm ...))) -SkipSetup`. A silent regression here
# breaks every scripted install.
#
# The rule this file mostly exists to protect: an environment variable already
# set by the caller ALWAYS wins over the matching flag (install.sh:47).

. (Join-Path $PSScriptRoot '_harness.ps1')
. (Get-InstallerPath)

Write-Host 'Get-GarraiaConfig: flag -> option mapping'
Clear-InstallerEnvironment

$c = Get-GarraiaConfig -SkipSetup
Assert-True  '-SkipSetup sets SkipInit'  $c.SkipInit
Assert-True  '-SkipSetup sets SkipStart' $c.SkipStart

$c = Get-GarraiaConfig -SkipInit
Assert-True  '-SkipInit sets SkipInit'          $c.SkipInit
Assert-False '-SkipInit leaves SkipStart alone' $c.SkipStart

$c = Get-GarraiaConfig -SkipStart
Assert-False '-SkipStart leaves SkipInit alone' $c.SkipInit
Assert-True  '-SkipStart sets SkipStart'        $c.SkipStart

$c = Get-GarraiaConfig -NoLocal
Assert-True '-NoLocal sets NoLocal' $c.NoLocal

$c = Get-GarraiaConfig -NoModifyPath
Assert-True '-NoModifyPath sets NoModifyPath' $c.NoModifyPath

$c = Get-GarraiaConfig -Version 'v0.3.4'
Assert-Equal '-Version passes through' 'v0.3.4' $c.Version

$c = Get-GarraiaConfig -InstallDir 'C:\tools\garraia'
Assert-Equal '-InstallDir passes through' 'C:\tools\garraia' $c.InstallDir

Write-Host ''
Write-Host 'Get-GarraiaConfig: defaults'
Clear-InstallerEnvironment
$c = Get-GarraiaConfig
Assert-False 'default SkipInit'     $c.SkipInit
Assert-False 'default SkipStart'    $c.SkipStart
Assert-False 'default NoLocal'      $c.NoLocal
Assert-False 'default NoModifyPath' $c.NoModifyPath
Assert-Equal 'default Version'    '' $c.Version
Assert-Equal 'default InstallDir' '' $c.InstallDir

Write-Host ''
Write-Host 'Get-GarraiaConfig: env vars alone'
Clear-InstallerEnvironment
$env:GARRAIA_SKIP_INIT = '1'
Assert-True 'GARRAIA_SKIP_INIT=1 sets SkipInit' (Get-GarraiaConfig).SkipInit

Clear-InstallerEnvironment
$env:GARRAIA_SKIP_START = '1'
Assert-True 'GARRAIA_SKIP_START=1 sets SkipStart' (Get-GarraiaConfig).SkipStart

Clear-InstallerEnvironment
# Note the polarity: BOOTSTRAP_LOCAL=0 is what suppresses the prompts, matching
# install.sh:36-38. wiki/Instalacao-e-Primeiros-Passos.md documented this
# backwards until it was corrected alongside this suite.
$env:GARRAIA_BOOTSTRAP_LOCAL = '0'
Assert-True 'GARRAIA_BOOTSTRAP_LOCAL=0 sets NoLocal' (Get-GarraiaConfig).NoLocal

Clear-InstallerEnvironment
$env:GARRAIA_BOOTSTRAP_LOCAL = '1'
Assert-False 'GARRAIA_BOOTSTRAP_LOCAL=1 does not set NoLocal' (Get-GarraiaConfig).NoLocal

Clear-InstallerEnvironment
$env:GARRAIA_NO_PATH = '1'
Assert-True 'GARRAIA_NO_PATH=1 sets NoModifyPath' (Get-GarraiaConfig).NoModifyPath

Write-Host ''
Write-Host 'Get-GarraiaConfig: env wins over flag'
Clear-InstallerEnvironment
$env:GARRAIA_VERSION = 'v9.9.9'
Assert-Equal 'env GARRAIA_VERSION beats -Version' 'v9.9.9' (Get-GarraiaConfig -Version 'v0.3.4').Version

Clear-InstallerEnvironment
$env:GARRAIA_INSTALL_DIR = 'D:\from-env'
Assert-Equal 'env GARRAIA_INSTALL_DIR beats -InstallDir' 'D:\from-env' (Get-GarraiaConfig -InstallDir 'C:\from-flag').InstallDir

# The switch direction is one-way on purpose: an env var can turn a skip ON,
# but never off. Passing -SkipInit with the env var unset must still skip.
Clear-InstallerEnvironment
Assert-True 'flag still works when env is unset' (Get-GarraiaConfig -SkipInit).SkipInit

Clear-InstallerEnvironment
Exit-WithSummary 'parse_args'
