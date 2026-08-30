# Unit tests for Invoke-BootstrapPhase in install.ps1 (the counterpart of
# tests/install_sh/bootstrap_phase.sh).
#
# Two behaviors matter more than the happy path:
#
#   1. In a non-interactive context (scheduled task, service, CI) the installer
#      must print next steps and return 0. It must NEVER block waiting for input
#      that nobody can supply -- a hung `irm | iex` inside a container build is
#      indistinguishable from a crash.
#   2. When the wizard exits non-zero, the installer stops rather than starting
#      a gateway against a config the wizard did not finish writing.
#
# Test-InteractiveSession is overridden here for the same reason
# Invoke-GhRequest is overridden in resolve_version.ps1: the real probe depends
# on how the runner attaches stdio, which would make these branches untestable.

. (Join-Path $PSScriptRoot '_harness.ps1')
. (Get-InstallerPath)

$onWindows = [IO.Path]::DirectorySeparatorChar -eq '\'
$script:Interactive = $true
function Test-InteractiveSession { return $script:Interactive }

# Build a stub `garraia` that records the subcommands it was called with and
# exits with a controllable code -- the analogue of
# tests/install_sh/fixtures/garraia-stub.sh.
function New-GarraiaStub {
    param([string]$Directory, [int]$ExitCode = 0)

    $log = Join-Path $Directory 'invocations.log'
    if ($onWindows) {
        $stub = Join-Path $Directory 'garraia.cmd'
        Set-Content -Path $stub -Value @"
@echo off
echo %* >> "$log"
exit /b $ExitCode
"@
    } else {
        $stub = Join-Path $Directory 'garraia.sh'
        Set-Content -Path $stub -Value @"
#!/bin/sh
echo "`$@" >> "$log"
exit $ExitCode
"@
        & chmod +x $stub
    }
    return @{ Path = $stub; Log = $log }
}

function Get-StubInvocations {
    param([string]$LogPath)
    # The leading comma keeps PowerShell from unrolling the array on return.
    # Without it an empty result comes back as $null, and $null.Count throws
    # under Set-StrictMode -- which is exactly the assertion we need most.
    if (-not (Test-Path $LogPath)) { return ,@() }
    return ,@(Get-Content $LogPath | ForEach-Object { $_.Trim() } | Where-Object { $_ })
}

$sandbox = Join-Path ([IO.Path]::GetTempPath()) ('garraia-bootstrap-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $sandbox | Out-Null

try {
    Write-Host 'Invoke-BootstrapPhase: both skips set (the --skip-setup contract)'
    $stub = New-GarraiaStub -Directory $sandbox
    $script:Interactive = $true
    $out = Invoke-BootstrapPhase -InstallPath $stub.Path -SkipInit $true -SkipStart $true 6>&1 | Out-String
    Assert-Equal 'the binary is never invoked' 0 (Get-StubInvocations $stub.Log).Count
    Assert-True  'prints the next-steps hint' ($out -match 'Next steps')
    Remove-Item $stub.Log -ErrorAction SilentlyContinue

    Write-Host ''
    Write-Host 'Invoke-BootstrapPhase: non-interactive context'
    $script:Interactive = $false
    $out = Invoke-BootstrapPhase -InstallPath $stub.Path -SkipInit $false -SkipStart $false 6>&1 | Out-String
    Assert-Equal 'the binary is never invoked' 0 (Get-StubInvocations $stub.Log).Count
    Assert-True  'says why it stopped'   ($out -match 'Non-interactive install detected')
    Assert-True  'still prints next steps' ($out -match 'Next steps')
    Remove-Item $stub.Log -ErrorAction SilentlyContinue

    Write-Host ''
    Write-Host 'Invoke-BootstrapPhase: interactive, wizard succeeds'
    $script:Interactive = $true
    $out = Invoke-BootstrapPhase -InstallPath $stub.Path -SkipInit $false -SkipStart $false 6>&1 | Out-String
    $calls = Get-StubInvocations $stub.Log
    Assert-Equal 'runs init then start' 2 $calls.Count
    Assert-Equal 'init runs first' 'init'  $calls[0]
    Assert-Equal 'start runs second' 'start' $calls[1]
    Assert-True  'warns about the firewall prompt' ($out -match 'Firewall')
    Remove-Item $stub.Log -ErrorAction SilentlyContinue

    Write-Host ''
    Write-Host 'Invoke-BootstrapPhase: interactive, -SkipInit only'
    $out = Invoke-BootstrapPhase -InstallPath $stub.Path -SkipInit $true -SkipStart $false 6>&1 | Out-String
    $calls = Get-StubInvocations $stub.Log
    Assert-Equal 'only start runs' 1 $calls.Count
    Assert-Equal 'and it is start'  'start' $calls[0]
    Remove-Item $stub.Log -ErrorAction SilentlyContinue

    Write-Host ''
    Write-Host 'Invoke-BootstrapPhase: interactive, -SkipStart only'
    $out = Invoke-BootstrapPhase -InstallPath $stub.Path -SkipInit $false -SkipStart $true 6>&1 | Out-String
    $calls = Get-StubInvocations $stub.Log
    Assert-Equal 'only init runs' 1 $calls.Count
    Assert-Equal 'and it is init'  'init' $calls[0]
    Assert-True  'falls through to next steps' ($out -match 'Next steps')
    Remove-Item $stub.Log -ErrorAction SilentlyContinue

    Write-Host ''
    Write-Host 'Invoke-BootstrapPhase: wizard exits non-zero'
    $failing = New-GarraiaStub -Directory (New-Item -ItemType Directory -Force -Path (Join-Path $sandbox 'failing')).FullName -ExitCode 3
    $out = Invoke-BootstrapPhase -InstallPath $failing.Path -SkipInit $false -SkipStart $false 6>&1 | Out-String
    $calls = Get-StubInvocations $failing.Log
    Assert-Equal 'stops after init -- start is never reached' 1 $calls.Count
    Assert-Equal 'the one call was init' 'init' $calls[0]
    Assert-True  'explains the config may need edits' ($out -match 'may need manual edits')
} finally {
    Remove-Item -Path $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}

Exit-WithSummary 'bootstrap_phase'
