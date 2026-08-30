# Unit tests for Test-SystemPath and Install-Binary in install.ps1 -- the
# counterpart of the system-path guard at install.sh:354-359, which refuses to
# write into /bin, /sbin, /usr/bin, /usr/sbin or /etc.
#
# The prefix-matching algorithm is exercised on every platform by pointing
# SystemRoot at a directory that exists there. The genuinely Windows-only cases
# (drive roots, backslash normalization through [IO.Path]::GetFullPath) are
# skipped elsewhere and covered by the Windows leg of the CI matrix, because
# GetFullPath resolves 'C:\Windows' to a relative path on Linux.

. (Join-Path $PSScriptRoot '_harness.ps1')
. (Get-InstallerPath)

$onWindows = [IO.Path]::DirectorySeparatorChar -eq '\'

Write-Host 'Test-SystemPath: rejects the system root and anything under it'

$savedRoot = $env:SystemRoot
$savedWinDir = $env:windir
try {
    if ($onWindows) {
        $fakeRoot = 'C:\Windows'
        $under = 'C:\Windows\System32'
        $sibling = 'C:\WindowsApps'
        $safe = 'C:\Users\someone\AppData\Local\Programs\GarraIA'
    } else {
        $fakeRoot = Join-Path ([IO.Path]::GetTempPath()) 'garraia-fake-sysroot'
        $under = Join-Path $fakeRoot 'system32'
        $sibling = "${fakeRoot}Apps"
        $safe = Join-Path ([IO.Path]::GetTempPath()) 'garraia-safe-install'
    }
    $env:SystemRoot = $fakeRoot
    $env:windir = $fakeRoot

    Assert-True  'the system root itself is rejected'   (Test-SystemPath -Path $fakeRoot)
    Assert-True  'a directory under it is rejected'     (Test-SystemPath -Path $under)
    Assert-True  'a trailing separator does not evade'  (Test-SystemPath -Path ($fakeRoot + [IO.Path]::DirectorySeparatorChar))
    # The guard must compare path SEGMENTS, not raw string prefixes: a sibling
    # directory whose name merely starts with the system root is legitimate.
    Assert-False 'a same-prefix sibling is allowed'     (Test-SystemPath -Path $sibling)
    Assert-False 'an ordinary user directory is allowed' (Test-SystemPath -Path $safe)

    if ($onWindows) {
        Assert-True 'a bare drive root is rejected' (Test-SystemPath -Path 'C:\')
        # GetFullPath collapses the traversal, so the guard still fires.
        Assert-True 'traversal back into the system root is rejected' `
            (Test-SystemPath -Path 'C:\Users\..\Windows\System32')
    } else {
        Assert-Skip 'a bare drive root is rejected' 'Windows-only path syntax'
        Assert-Skip 'traversal back into the system root is rejected' 'GetFullPath is POSIX here'
    }
} finally {
    if ($null -eq $savedRoot)   { Remove-Item Env:SystemRoot -ErrorAction SilentlyContinue } else { $env:SystemRoot = $savedRoot }
    if ($null -eq $savedWinDir) { Remove-Item Env:windir     -ErrorAction SilentlyContinue } else { $env:windir = $savedWinDir }
}

Write-Host ''
Write-Host 'Install-Binary: honors an explicit directory and refuses system paths'

$sandbox = Join-Path ([IO.Path]::GetTempPath()) ('garraia-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $sandbox | Out-Null
try {
    $fakeBinary = Join-Path $sandbox 'downloaded.bin'
    Set-Content -Path $fakeBinary -Value 'not really a binary' -NoNewline

    $target = Join-Path $sandbox 'install-here'
    # -NoModifyPath keeps the test off the registry; PATH registration is
    # covered on the Windows leg by the smoke test, not by a unit test that
    # would have to mutate the runner's own environment.
    $installed = Install-Binary -SourcePath $fakeBinary -Version 'v0.0.0-test' `
        -RequestedDir $target -NoModifyPath $true

    Assert-Equal 'installs as garraia.exe' 'garraia.exe' (Split-Path $installed -Leaf)
    Assert-True  'the file really landed'  (Test-Path $installed)
    Assert-Equal 'contents copied verbatim' 'not really a binary' (Get-Content $installed -Raw)
    Assert-True  'creates the directory when missing' (Test-Path $target)

    # Re-installing over an existing copy must succeed, not fail on "exists".
    $again = Install-Binary -SourcePath $fakeBinary -Version 'v0.0.0-test' `
        -RequestedDir $target -NoModifyPath $true
    Assert-Equal 'reinstall is idempotent' $installed $again

    $savedRoot2 = $env:SystemRoot
    try {
        $env:SystemRoot = if ($onWindows) { 'C:\Windows' } else { $sandbox }
        $blocked = if ($onWindows) { 'C:\Windows\System32\garraia' } else { Join-Path $sandbox 'nested' }
        Assert-Throws 'refuses to install into a system path' {
            Install-Binary -SourcePath $fakeBinary -Version 'v0.0.0-test' `
                -RequestedDir $blocked -NoModifyPath $true
        }
    } finally {
        if ($null -eq $savedRoot2) { Remove-Item Env:SystemRoot -ErrorAction SilentlyContinue } else { $env:SystemRoot = $savedRoot2 }
    }
} finally {
    Remove-Item -Path $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}

Exit-WithSummary 'install_dir'
