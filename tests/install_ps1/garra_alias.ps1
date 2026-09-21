# Issue #1328 -- unit tests for Install-GarraAlias in install.ps1, the
# counterpart of tests/install_sh/garra_alias.sh. Same cases, one for one, with
# the `garra.cmd` shim standing in for the `garra -> garraia` symlink:
#
#   sh: nothing there -> relative symlink      ps1: nothing there -> shim
#   sh: stale symlink -> repointed             ps1: older shim of ours -> rewritten
#   sh: real file -> kept, warning             ps1: foreign .cmd or any .exe -> kept, warning
#   sh: link survives moving the directory     ps1: %~dp0 survives moving the directory
#   sh: `garra --version` runs the stub        ps1: `garra.cmd` runs garraia.exe (Windows leg)
#   sh: ln fails -> warning, rc 0              ps1: Set-Content fails -> warning, no throw
#   sh: install_binary wires it in             ps1: Install-Binary wires it in
#   sh: next steps cite both names             ps1: next steps cite both names
#
# The execution cases need a real executable behind garraia.exe, so on the
# Windows leg the sandbox gets a copy of cmd.exe under that name; on Linux they
# report as SKIP, exactly like the registry and drive-root cases elsewhere.

. (Join-Path $PSScriptRoot '_harness.ps1')
. (Get-InstallerPath)

$onWindows = [IO.Path]::DirectorySeparatorChar -eq '\'

# The exact bytes the installer must write. Hardcoded on purpose rather than
# read back from install.ps1: a test that compares the shim against the template
# that produced it cannot catch the template changing. CRLF because cmd.exe is
# the interpreter; %~dp0 so the shim follows its own directory; quotes for
# spaces in the path; %* to forward every argument.
$expectedShim = "@echo off`r`n`"%~dp0garraia.exe`" %*`r`n"

function New-Sandbox {
    $dir = Join-Path ([IO.Path]::GetTempPath()) ('garraia-alias-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    # Stands in for garraia.exe. On Windows a real executable (a copy of cmd.exe)
    # so the shim can actually be run; elsewhere a placeholder file.
    $binary = Join-Path $dir 'garraia.exe'
    if ($onWindows) {
        Copy-Item -LiteralPath (Join-Path $env:SystemRoot 'System32\cmd.exe') -Destination $binary
    } else {
        Set-Content -LiteralPath $binary -Value 'not really a binary' -NoNewline
    }
    return $dir
}

function Get-ShimText {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }
    return [IO.File]::ReadAllText($Path)
}

function Invoke-Alias {
    param([string]$Directory)
    # 6>&1 folds the Write-Host stream into the output so the messages can be
    # asserted on; the function itself returns nothing on the success stream.
    return (Install-GarraAlias -Directory $Directory 6>&1 | Out-String)
}

# Runs `garra.cmd <args>` and returns its stdout. Only meaningful on Windows,
# where garraia.exe is a copy of cmd.exe, so `/c echo ...` proves both that the
# shim found the binary through %~dp0 and that %* forwarded the arguments.
function Invoke-Shim {
    param([string]$ShimPath, [string]$Marker)
    # No 2>&1: with $ErrorActionPreference = 'Stop' (set by the harness) a
    # redirected native stderr line becomes a terminating NativeCommandError on
    # Windows PowerShell 5.1, and echo never writes to stderr anyway.
    return (& $ShimPath /c echo $Marker second-arg | Out-String)
}

$sandboxes = @()
try {
    # ---- case (a): nothing there -> shim, no warning ----------------------------
    Write-Host 'case (a): no garra -> writes the shim'
    $dir = New-Sandbox; $sandboxes += $dir
    $shimPath = Join-Path $dir 'garra.cmd'
    $out = Invoke-Alias -Directory $dir

    Assert-True  'writes garra.cmd next to garraia.exe' (Test-Path -LiteralPath $shimPath -PathType Leaf)
    Assert-True  'content is exactly the documented shim' ((Get-ShimText $shimPath) -ceq $expectedShim)
    Assert-Equal 'no BOM: the first byte is @' 64 ([IO.File]::ReadAllBytes($shimPath)[0])
    Assert-True  'reports the alias' ($out -match 'Alias .*garra\.cmd -> garraia\.exe')
    Assert-False 'no warning' ($out -match 'warning:')
    # The "relative link" invariant: the shim must never bake in the directory it
    # was written to, or moving the folder would break it.
    Assert-True  'dispatch is relative to the .cmd, not the install dir' `
        ((Get-ShimText $shimPath) -notmatch [regex]::Escape($dir))
    if ($onWindows) {
        Assert-True 'garra.cmd runs garraia.exe with all arguments' `
            ((Invoke-Shim -ShimPath $shimPath -Marker 'shim-ok') -match 'shim-ok second-arg')
    } else {
        Assert-Skip 'garra.cmd runs garraia.exe with all arguments' 'needs cmd.exe'
    }

    # ---- case (b): older shim of ours -> rewritten to the current template ------
    Write-Host ''
    Write-Host 'case (b): older shim of ours -> rewritten'
    $dir = New-Sandbox; $sandboxes += $dir
    $shimPath = Join-Path $dir 'garra.cmd'
    # Dispatches to %~dp0garraia.exe, so it is recognizably ours, but not in the
    # current shape -- the analogue of a stale symlink that still needs repointing.
    Set-Content -LiteralPath $shimPath -Value '@"%~dp0garraia.exe" %*' -NoNewline
    $out = Invoke-Alias -Directory $dir

    Assert-True  'rewritten to the current template' ((Get-ShimText $shimPath) -ceq $expectedShim)
    Assert-True  'says it repointed' ($out -match 'Repointed alias')
    Assert-False 'no warning' ($out -match 'warning:')

    # ---- case (c): foreign garra.cmd -> kept, warning ---------------------------
    Write-Host ''
    Write-Host 'case (c): foreign garra.cmd -> kept, warning'
    $dir = New-Sandbox; $sandboxes += $dir
    $shimPath = Join-Path $dir 'garra.cmd'
    $foreignBody = "@echo off`r`necho from-source-build`r`n"
    Set-Content -LiteralPath $shimPath -Value $foreignBody -NoNewline
    $out = Invoke-Alias -Directory $dir

    Assert-True  'content untouched' ((Get-ShimText $shimPath) -ceq $foreignBody)
    Assert-True  'warns it was left alone' `
        ($out -match 'already exists and was not created by this installer - left untouched')
    Assert-True  'names the installed binary' ($out -match [regex]::Escape("The installed binary is $(Join-Path $dir 'garraia.exe')"))
    # -cmatch: -match is case-insensitive and would also hit "alias" inside the
    # could-not-create warning of a different branch.
    Assert-False 'does not claim an alias' ($out -cmatch 'Alias ')

    # ---- case (c2): foreign garra.exe -> never touched, no shim beside it --------
    Write-Host ''
    Write-Host 'case (c2): foreign garra.exe -> kept, no shim'
    $dir = New-Sandbox; $sandboxes += $dir
    $exePath = Join-Path $dir 'garra.exe'
    Set-Content -LiteralPath $exePath -Value 'someone else''s program' -NoNewline
    $out = Invoke-Alias -Directory $dir

    Assert-Equal 'garra.exe untouched' 'someone else''s program' (Get-Content -LiteralPath $exePath -Raw)
    Assert-False 'no garra.cmd written beside a foreign garra.exe' (Test-Path -LiteralPath (Join-Path $dir 'garra.cmd'))
    Assert-True  'warns about the exe' ($out -match [regex]::Escape("$exePath already exists and was not created by this installer"))

    # ---- case (d): %~dp0 keeps working after the directory moves ----------------
    Write-Host ''
    Write-Host 'case (d): shim survives moving the directory'
    $dir = New-Sandbox
    Invoke-Alias -Directory $dir | Out-Null
    $moved = Join-Path ([IO.Path]::GetTempPath()) ('garraia-alias-moved-' + [Guid]::NewGuid().ToString('N'))
    Move-Item -LiteralPath $dir -Destination $moved
    $sandboxes += $moved
    $movedShim = Join-Path $moved 'garra.cmd'

    Assert-True 'shim moved with the directory' (Test-Path -LiteralPath $movedShim -PathType Leaf)
    Assert-True 'shim text still names no directory' ((Get-ShimText $movedShim) -notmatch [regex]::Escape($dir))
    if ($onWindows) {
        Assert-True 'garra.cmd still runs garraia.exe from the new location' `
            ((Invoke-Shim -ShimPath $movedShim -Marker 'moved-ok') -match 'moved-ok second-arg')
    } else {
        Assert-Skip 'garra.cmd still runs garraia.exe from the new location' 'needs cmd.exe'
    }

    # ---- case (e): re-running is idempotent -------------------------------------
    Write-Host ''
    Write-Host 'case (e): running twice is idempotent'
    $dir = New-Sandbox; $sandboxes += $dir
    $shimPath = Join-Path $dir 'garra.cmd'
    Invoke-Alias -Directory $dir | Out-Null
    $out = Invoke-Alias -Directory $dir

    Assert-True  'still the documented shim' ((Get-ShimText $shimPath) -ceq $expectedShim)
    Assert-False 'no warning on the second run' ($out -match 'warning:')

    # ---- case (f): the write fails -> warning, never a throw --------------------
    Write-Host ''
    Write-Host 'case (f): write fails -> warning, no throw'
    if ($onWindows) {
        # The read-only attribute does not stop file creation on Windows and a
        # deny ACL is more machinery than this case is worth; the Linux leg covers
        # the branch.
        Assert-Skip 'warns instead of throwing when the shim cannot be written' 'read-only directories are not enforced on Windows'
    } elseif ((& id -u) -eq '0') {
        Assert-Skip 'warns instead of throwing when the shim cannot be written' 'running as root'
    } else {
        $dir = New-Sandbox; $sandboxes += $dir
        & chmod 555 $dir
        try {
            $threw = $false
            $out = ''
            try { $out = Invoke-Alias -Directory $dir } catch { $threw = $true }
            Assert-False 'does not throw' $threw
            Assert-True  'warns and points at garraia' ($out -match "could not create the 'garra' alias")
            Assert-False 'no shim appeared' (Test-Path -LiteralPath (Join-Path $dir 'garra.cmd'))
        } finally {
            & chmod 755 $dir
        }
    }

    # ---- case (g): Install-Binary wires the alias in ----------------------------
    Write-Host ''
    Write-Host 'case (g): Install-Binary writes the shim right after the binary'
    $dir = New-Sandbox; $sandboxes += $dir
    $source = Join-Path $dir 'downloaded.bin'
    Set-Content -LiteralPath $source -Value 'not really a binary' -NoNewline
    $target = Join-Path $dir 'install-here'
    $installed = Install-Binary -SourcePath $source -Version 'v0.0.0-test' `
        -RequestedDir $target -NoModifyPath $true

    Assert-True  'garraia.exe installed' (Test-Path -LiteralPath (Join-Path $target 'garraia.exe'))
    Assert-True  'garra.cmd written alongside' (Test-Path -LiteralPath (Join-Path $target 'garra.cmd'))
    Assert-True  'the shim is the documented one' ((Get-ShimText (Join-Path $target 'garra.cmd')) -ceq $expectedShim)
    # Install-GarraAlias must stay off the success stream, or Install-Binary's
    # return value turns into an array and Invoke-Main breaks.
    Assert-True  'Install-Binary still returns a single path' ($installed -is [string])
    Assert-Equal 'and it is the binary' 'garraia.exe' (Split-Path $installed -Leaf)

    # ---- case (h): the next-steps summary cites both names ----------------------
    Write-Host ''
    Write-Host 'case (h): next steps mention the alias'
    $out = Write-NextStepsLegacy 6>&1 | Out-String
    Assert-True 'still prints Next steps' ($out -match 'Next steps')
    Assert-True 'explains the alias' ($out -match "'garra' is an alias for 'garraia'")
} finally {
    foreach ($s in $sandboxes) {
        Remove-Item -LiteralPath $s -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Exit-WithSummary 'garra_alias'
