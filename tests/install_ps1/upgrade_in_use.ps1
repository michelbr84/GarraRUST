# Issue #1865 -- the upgrade over a running gateway, Windows side. Counterpart
# of tests/install_sh/upgrade_etxtbsy.sh (#1862), CLAUDE.md rule 16.
#
#   sh: cp over a running binary -> ETXTBSY     ps1: Copy-Item -Force over a running .exe -> refused
#       (negative control)                          (negative control, Windows leg)
#   sh: stage + rename(2) while it runs         ps1: stage, rename the running .exe aside, rename in
#   sh: clean install / plain upgrade           ps1: clean install / plain upgrade
#   sh: failure leaves no stage behind          ps1: every failing step is undone (injected failures)
#   sh: gateway_running -> restart notice       ps1: Test-GatewayRunning -> Write-RestartNotice
#   sh: GARRAIA_SKIP_RUNNING_GATEWAY_NOTICE=1   ps1: same kill-switch
#
# The running-executable cases need a real Windows image to hold open, so on
# the Windows leg the sandbox runs a copy of ping.exe under the name
# garraia.exe; on Linux they report as SKIP, like garra_alias.ps1 does.

. (Join-Path $PSScriptRoot '_harness.ps1')
. (Get-InstallerPath)

$onWindows = [IO.Path]::DirectorySeparatorChar -eq '\'

# Failure injection. Install-BinaryFile does every rename through
# Move-InstallFile precisely so this override can fail the Nth one; with
# FailMoveAt = 0 it is the real rename.
$script:MoveCalls = 0
$script:FailMoveAt = 0
function Move-InstallFile {
    param([string]$Path, [string]$Destination)
    $script:MoveCalls++
    if ($script:MoveCalls -eq $script:FailMoveAt) {
        throw "injected failure on rename #$($script:MoveCalls)"
    }
    [IO.File]::Move($Path, $Destination)
}

function Reset-MoveInjection {
    param([int]$FailAt = 0)
    $script:MoveCalls = 0
    $script:FailMoveAt = $FailAt
}

function New-Sandbox {
    $dir = Join-Path ([IO.Path]::GetTempPath()) ('garraia-upgrade-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    return $dir
}

function New-Payload {
    param([string]$Directory, [string]$Name, [string]$Text)
    $path = Join-Path $Directory $Name
    Set-Content -LiteralPath $path -Value $Text -NoNewline
    return $path
}

function Get-Text {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }
    return [IO.File]::ReadAllText($Path)
}

# Every file in the install dir except the destination itself: the stage and
# the aside are the leftovers a broken path would leave.
function Get-Leftover {
    param([string]$Directory)
    return ,@(Get-ChildItem -LiteralPath $Directory -File |
        Where-Object { $_.Name -ne 'garraia.exe' } |
        ForEach-Object { $_.Name })
}

$sandboxes = @()
try {
    Write-Host 'Install-BinaryFile: clean install (nothing at the destination)'
    Reset-MoveInjection
    $box = New-Sandbox; $sandboxes += $box
    $srcBox = New-Sandbox; $sandboxes += $srcBox
    $src = New-Payload -Directory $srcBox -Name 'download.exe' -Text 'v2-bytes'
    $dest = Join-Path $box 'garraia.exe'
    Install-BinaryFile -SourcePath $src -Destination $dest
    Assert-Equal 'the destination holds the new bytes' 'v2-bytes' (Get-Text $dest)
    Assert-Equal 'no stage or aside is left behind' 0 (Get-Leftover $box).Count
    Assert-Equal 'nothing to move aside: one rename (stage -> dest)' 1 $script:MoveCalls

    Write-Host ''
    Write-Host 'Install-BinaryFile: plain upgrade (old binary present, not running)'
    Reset-MoveInjection
    Set-Content -LiteralPath $dest -Value 'v1-bytes' -NoNewline
    # @() around the call: Install-Binary returns its path on the success
    # stream, so Install-BinaryFile must emit nothing there.
    $emitted = @(Install-BinaryFile -SourcePath $src -Destination $dest)
    Assert-Equal 'nothing on the success stream' 0 $emitted.Count
    Assert-Equal 'the destination holds the new bytes' 'v2-bytes' (Get-Text $dest)
    Assert-Equal 'the aside of a stopped binary is deleted' 0 (Get-Leftover $box).Count
    Assert-Equal 'two renames (dest -> aside, stage -> dest)' 2 $script:MoveCalls

    Write-Host ''
    Write-Host 'Install-BinaryFile: the rollback backup of `garraia update` is not touched'
    Reset-MoveInjection
    $rollback = New-Payload -Directory $box -Name 'garraia.old' -Text 'update-rollback'
    Set-Content -LiteralPath $dest -Value 'v1-bytes' -NoNewline
    Install-BinaryFile -SourcePath $src -Destination $dest
    Assert-Equal 'garraia.old survives the upgrade' 'update-rollback' (Get-Text $rollback)
    Remove-Item -LiteralPath $rollback -Force

    Write-Host ''
    Write-Host 'Install-BinaryFile: moving the current binary aside fails'
    Reset-MoveInjection -FailAt 1
    Set-Content -LiteralPath $dest -Value 'v1-bytes' -NoNewline
    $threw = $false; $msg = ''
    try { Install-BinaryFile -SourcePath $src -Destination $dest } catch { $threw = $true; $msg = $_.Exception.Message }
    Assert-True  'the install fails' $threw
    Assert-True  'the message says what to do' ($msg -match "garraia stop")
    Assert-Equal 'the destination is untouched' 'v1-bytes' (Get-Text $dest)
    Assert-Equal 'the stage is cleaned up' 0 (Get-Leftover $box).Count

    Write-Host ''
    Write-Host 'Install-BinaryFile: renaming the new binary in fails -> the old one comes back'
    Reset-MoveInjection -FailAt 2
    $threw = $false; $msg = ''
    try { Install-BinaryFile -SourcePath $src -Destination $dest } catch { $threw = $true; $msg = $_.Exception.Message }
    Assert-True  'the install fails' $threw
    Assert-True  'the message names the destination' ($msg -match [regex]::Escape($dest))
    Assert-Equal 'the previous binary is back at the destination' 'v1-bytes' (Get-Text $dest)
    Assert-Equal 'neither stage nor aside is left' 0 (Get-Leftover $box).Count

    Write-Host ''
    Write-Host 'Install-BinaryFile: rename in AND the undo both fail -> says where the binary is'
    Reset-MoveInjection -FailAt 2
    function Move-InstallFile {
        param([string]$Path, [string]$Destination)
        $script:MoveCalls++
        if ($script:MoveCalls -ge $script:FailMoveAt) { throw "injected failure on rename #$($script:MoveCalls)" }
        [IO.File]::Move($Path, $Destination)
    }
    $threw = $false; $msg = ''
    try { Install-BinaryFile -SourcePath $src -Destination $dest } catch { $threw = $true; $msg = $_.Exception.Message }
    $aside = "$dest.old-$PID"
    $stage = "$dest.tmp.$PID"
    Assert-True  'the install fails' $threw
    Assert-True  'the message points at the aside copy' ($msg -match [regex]::Escape($aside))
    Assert-True  'the message points at the staged copy' ($msg -match [regex]::Escape($stage))
    Assert-Equal 'the previous binary is intact at the aside path' 'v1-bytes' (Get-Text $aside)
    Assert-Equal 'the new binary is kept at the stage path' 'v2-bytes' (Get-Text $stage)
    # Back to the single-failure override for the remaining cases.
    function Move-InstallFile {
        param([string]$Path, [string]$Destination)
        $script:MoveCalls++
        if ($script:MoveCalls -eq $script:FailMoveAt) { throw "injected failure on rename #$($script:MoveCalls)" }
        [IO.File]::Move($Path, $Destination)
    }
    [IO.File]::Move($aside, $dest)
    Remove-Item -LiteralPath $stage -Force

    Write-Host ''
    Write-Host 'Install-BinaryFile: the aside name is taken (second install, same session)'
    Reset-MoveInjection
    Set-Content -LiteralPath $dest -Value 'v1-bytes' -NoNewline
    # Stands in for the aside a still-running gateway holds from the first
    # install. A real one cannot be deleted while that gateway runs; this
    # placeholder could, so the sweep is stubbed out for this one call to
    # reproduce "the name is taken and stays taken".
    $taken = New-Payload -Directory $box -Name "garraia.exe.old-$PID" -Text 'held-by-old-gateway'
    $realSweep = ${function:Clear-StaleAsideBinary}
    function Clear-StaleAsideBinary { param([string]$Destination) $null = $Destination }
    Install-BinaryFile -SourcePath $src -Destination $dest
    Set-Item -Path function:Clear-StaleAsideBinary -Value $realSweep
    Assert-Equal 'the upgrade still lands' 'v2-bytes' (Get-Text $dest)
    Assert-Equal 'the taken aside is left alone' 'held-by-old-gateway' (Get-Text $taken)
    Clear-StaleAsideBinary -Destination $dest
    Assert-Equal 'the sweep covers the suffixed name too' 0 (Get-Leftover $box).Count

    Write-Host ''
    Write-Host 'Install-BinaryFile: a stale aside from an earlier upgrade is swept'
    Reset-MoveInjection
    $stale = New-Payload -Directory $box -Name 'garraia.exe.old-99999' -Text 'from-an-old-run'
    Install-BinaryFile -SourcePath $src -Destination $dest
    Assert-False 'garraia.exe.old-99999 is gone' (Test-Path -LiteralPath $stale)
    Assert-Equal 'nothing else is left' 0 (Get-Leftover $box).Count

    Write-Host ''
    Write-Host 'Install-BinaryFile: upgrade with garraia.exe RUNNING'
    if ($onWindows) {
        Reset-MoveInjection
        $runBox = New-Sandbox; $sandboxes += $runBox
        $runDest = Join-Path $runBox 'garraia.exe'
        Copy-Item -LiteralPath (Join-Path $env:SystemRoot 'System32\PING.EXE') -Destination $runDest
        $proc = Start-Process -FilePath $runDest -ArgumentList '-n', '600', '127.0.0.1' `
            -NoNewWindow -PassThru
        try {
            Start-Sleep -Milliseconds 500
            Assert-False 'the stand-in gateway is running' $proc.HasExited

            # Negative control: what the installer did before #1865. If
            # Windows ever stops refusing this, the case below proves nothing.
            $refused = $false
            try {
                Copy-Item -LiteralPath $src -Destination $runDest -Force -ErrorAction Stop
            } catch {
                $refused = $true
            }
            Assert-True 'control: Copy-Item -Force over the running .exe is refused' $refused

            Assert-True 'Test-GatewayRunning sees the running garraia' (Test-GatewayRunning)
            $env:GARRAIA_SKIP_RUNNING_GATEWAY_NOTICE = '1'
            Assert-False 'GARRAIA_SKIP_RUNNING_GATEWAY_NOTICE=1 disables the probe' (Test-GatewayRunning)
            Remove-Item Env:GARRAIA_SKIP_RUNNING_GATEWAY_NOTICE

            try {
                Install-BinaryFile -SourcePath $src -Destination $runDest
            } catch {
                Assert-Fail "Install-BinaryFile over the running .exe threw: $($_.Exception.Message)"
            }
            Assert-Equal 'the destination holds the new bytes' 'v2-bytes' (Get-Text $runDest)
            Assert-False 'the running process was not disturbed' $proc.HasExited
            # Whether Windows lets the aside of a running image be deleted
            # depends on the build (POSIX delete semantics); either way, the
            # stage must be gone and the only possible leftover is the aside.
            $left = Get-Leftover $runBox
            Assert-False 'the stage is gone' (Test-Path -LiteralPath "$runDest.tmp.$PID")
            Assert-True  'any leftover is the aside <dest>.old-<pid>' `
                (($left.Count -eq 0) -or (($left.Count -eq 1) -and ($left[0] -eq "garraia.exe.old-$PID")))
        } finally {
            if (-not $proc.HasExited) { $proc.Kill() }
            $proc.WaitForExit(10000) | Out-Null
        }

        # Defender can hold a transient handle on a freshly written .exe right
        # after the process exits; retry the sweep briefly before asserting.
        for ($i = 0; $i -lt 10 -and (Get-Leftover $runBox).Count -gt 0; $i++) {
            Clear-StaleAsideBinary -Destination $runDest
            Start-Sleep -Milliseconds 200
        }
        Assert-Equal 'once the gateway stopped, the next install sweeps the aside' 0 (Get-Leftover $runBox).Count
    } else {
        Assert-Skip 'upgrade over a running garraia.exe' 'needs a Windows image (windows-latest leg)'
    }

    Write-Host ''
    Write-Host 'Invoke-BootstrapPhase: gateway running -> restart notice, no wizard, no start'
    $realProbe = ${function:Test-GatewayRunning}
    function Test-GatewayRunning { return $true }
    # A path that does not exist: if the bootstrap tried `& $path init` or
    # `& $path start`, the call itself would throw and fail this case.
    $missing = Join-Path $box 'never-invoked.exe'
    $out = Invoke-BootstrapPhase -InstallPath $missing -SkipInit $false -SkipStart $false `
        -Version 'v9.9.9' 6>&1 | Out-String -Width 4096
    Assert-True  'prints the upgrade note'          ($out -match 'Upgrade note')
    Assert-True  'names the version now on disk'    ($out -match 'v9\.9\.9')
    Assert-True  'gives the restart command'        ($out -match [regex]::Escape('garraia stop; garraia start'))
    Assert-False 'does not run the wizard'          ($out -match 'Running interactive setup wizard')
    Assert-False 'does not start a second gateway'  ($out -match 'Starting GarraIA in the foreground')
    Set-Item -Path function:Test-GatewayRunning -Value $realProbe

    # The kill-switch is asserted inside the running-exe case above, where a
    # garraia IS running; on a host with none it would pass vacuously.
} finally {
    foreach ($dir in $sandboxes) {
        Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Exit-WithSummary 'upgrade_in_use'
