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
#
# The override covers the BRANCHES. The PROBE itself is covered at the end of
# this file, in child processes that dot-source the real install.ps1 -- the
# mirror of cases (a) and (g) in tests/install_sh/bootstrap_phase.sh, which
# pin install.sh's has_usable_tty after the v0.4.4 smoke run caught it
# trusting /dev/tty's permission bits inside a container.

. (Join-Path $PSScriptRoot '_harness.ps1')
$installerPath = Get-InstallerPath
. $installerPath

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

    # ---- the REAL probe, in child processes -------------------------------
    #
    # Each child dot-sources the real install.ps1 (so it gets the real
    # Test-InteractiveSession, not the override above), prints what its stdin
    # looks like and what the probe decided, then runs Invoke-BootstrapPhase
    # against a stub. CI is set or removed INSIDE the child: GitHub Actions
    # exports CI=true, and inheriting it would let the CI short-circuit answer
    # for the stdin probe and hide a regression in it.
    $probeStub = New-GarraiaStub -Directory (New-Item -ItemType Directory -Force -Path (Join-Path $sandbox 'probe')).FullName
    $child = Join-Path $sandbox 'probe-child.ps1'
    $quotedInstaller = $installerPath -replace "'", "''"
    $quotedStub = $probeStub.Path -replace "'", "''"
    Set-Content -Path $child -Value @"
param([string]`$Ci = '', [switch]`$ProbeOnly)
Set-StrictMode -Version Latest
`$ErrorActionPreference = 'Stop'
if (`$Ci) { `$env:CI = `$Ci } else { Remove-Item Env:CI -ErrorAction SilentlyContinue }
`$env:GARRAIA_INSTALL_PS1_LIBRARY = '1'
. '$quotedInstaller'
Write-Output ('__redirected__=' + [Console]::IsInputRedirected)
Write-Output ('__probe__=' + (Test-InteractiveSession))
if (-not `$ProbeOnly) {
    Invoke-BootstrapPhase -InstallPath '$quotedStub' -SkipInit `$false -SkipStart `$false 6>&1 | Out-String
}
"@
    $psExe = (Get-Process -Id $PID).Path

    # Run a process with every stdio handle redirected and stdin closed at
    # once, bounded in time: a child that blocks is killed and reported as a
    # failure instead of hanging the CI job -- the very failure mode these
    # cases exist to rule out.
    function Invoke-BoundedChild {
        param([string]$FileName, [string]$Arguments)
        $psi = New-Object Diagnostics.ProcessStartInfo
        $psi.FileName = $FileName
        $psi.Arguments = $Arguments
        $psi.UseShellExecute = $false
        $psi.RedirectStandardInput = $true
        $psi.RedirectStandardOutput = $true
        $psi.RedirectStandardError = $true
        $proc = [Diagnostics.Process]::Start($psi)
        $proc.StandardInput.Close()
        $stdoutTask = $proc.StandardOutput.ReadToEndAsync()
        $stderrTask = $proc.StandardError.ReadToEndAsync()
        $exited = $proc.WaitForExit(120000)
        if (-not $exited) {
            try { $proc.Kill() } catch { Write-Host "    could not kill the child: $($_.Exception.Message)" }
            $proc.WaitForExit()
        }
        return @{
            Output   = $stdoutTask.Result + $stderrTask.Result
            ExitCode = $proc.ExitCode
            TimedOut = -not $exited
        }
    }

    Write-Host ''
    Write-Host 'Test-InteractiveSession (real probe): stdin redirected, CI unset'
    # The container/CI condition behind the v0.4.4 smoke failure, in its
    # Windows form: there is a stdin handle, but no console behind it.
    # [Console]::IsInputRedirected is not a permission check -- it asks
    # GetFileType for a character device AND GetConsoleMode to accept the
    # handle, so even NUL (a character device that is not a console) counts
    # as redirected. That is the Windows analogue of actually opening /dev/tty.
    $failedBefore = $script:Failed
    $run = Invoke-BoundedChild -FileName $psExe `
        -Arguments ('-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + $child + '"')
    $out = $run.Output
    Assert-False 'the child does not hang' $run.TimedOut
    Assert-Equal 'the child exits 0' 0 $run.ExitCode
    Assert-True  'precondition: the child sees a redirected stdin' ($out -match '__redirected__=True')
    Assert-True  'the probe answers non-interactive' ($out -match '__probe__=False')
    Assert-True  'takes the non-interactive path' ($out -match 'Non-interactive install detected')
    Assert-False 'never announces the wizard' ($out -match 'Running interactive setup wizard')
    Assert-False "no misleading 'wizard exited non-zero'" ($out -match 'Wizard exited non-zero')
    Assert-True  'still prints next steps' ($out -match 'Next steps')
    Assert-Equal 'the binary is never invoked' 0 (Get-StubInvocations $probeStub.Log).Count
    if ($script:Failed -gt $failedBefore) { Write-Host ($out -replace '(?m)^', '    ') }
    Remove-Item $probeStub.Log -ErrorAction SilentlyContinue

    # The two pty cases need a console the probe WILL accept, so that only the
    # thing under test can say no. util-linux `script` gives the child a real
    # pty on Linux; the windows-latest runner has no way to hand a child an
    # interactive console, so they are skipped there, as the registry cases are
    # in install_dir.ps1.
    $ptyTool = if ($onWindows) { $null } else { Get-Command script -CommandType Application -ErrorAction SilentlyContinue }

    Write-Host ''
    Write-Host 'Test-InteractiveSession (real probe): real terminal, CI unset'
    if ($null -eq $ptyTool) {
        Assert-Skip 'positive control under a pty' 'needs util-linux script (Linux leg only)'
    } else {
        $inner = "'" + $psExe + "' -NoProfile -NonInteractive -File '" + $child + "' -ProbeOnly"
        $failedBefore = $script:Failed
        $run = Invoke-BoundedChild -FileName $ptyTool.Source -Arguments ('-qec "' + $inner + '" /dev/null')
        $out = $run.Output
        Assert-False 'the child does not hang' $run.TimedOut
        Assert-True  'precondition: stdin is a terminal' ($out -match '__redirected__=False')
        # Without this the redirected case above would pass against a probe
        # that always says no.
        Assert-True  'the probe answers interactive' ($out -match '__probe__=True')
        if ($script:Failed -gt $failedBefore) { Write-Host ($out -replace '(?m)^', '    ') }
    }

    Write-Host ''
    Write-Host 'Test-InteractiveSession (real probe): real terminal, CI is set'
    # Mirror of case (g) in bootstrap_phase.sh: a CI job can hand the installer
    # a pty, but nobody is going to type into it.
    if ($null -eq $ptyTool) {
        Assert-Skip 'CI short-circuit under a pty' 'needs util-linux script (Linux leg only)'
    } else {
        $inner = "'" + $psExe + "' -NoProfile -NonInteractive -File '" + $child + "' -Ci true"
        $failedBefore = $script:Failed
        $run = Invoke-BoundedChild -FileName $ptyTool.Source -Arguments ('-qec "' + $inner + '" /dev/null')
        $out = $run.Output
        Assert-False 'the child does not hang' $run.TimedOut
        Assert-True  'precondition: stdin is a terminal' ($out -match '__redirected__=False')
        Assert-True  'the probe answers non-interactive' ($out -match '__probe__=False')
        Assert-True  'takes the non-interactive path' ($out -match 'Non-interactive install detected')
        Assert-False 'never announces the wizard' ($out -match 'Running interactive setup wizard')
        Assert-Equal 'the binary is never invoked' 0 (Get-StubInvocations $probeStub.Log).Count
        if ($script:Failed -gt $failedBefore) { Write-Host ($out -replace '(?m)^', '    ') }
    }
} finally {
    Remove-Item -Path $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}

Exit-WithSummary 'bootstrap_phase'
