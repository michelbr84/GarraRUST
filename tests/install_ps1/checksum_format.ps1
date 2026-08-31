# Unit tests for Select-ChecksumLine / Get-ExpectedHash in install.ps1
# (the PowerShell counterpart of tests/install_sh/checksum_format.sh).
#
# Three real hazards are covered, all of which have bitten a published release:
#
#   1. SHA256SUMS may be text mode (`<hash>  <file>`, two spaces) or binary mode
#      (`<hash> *<file>`, one space + asterisk). release.yml produces text mode
#      today, but the v0.2.1 Tauri toolchain produced binary mode.
#   2. Windows-generated sums files carry CR line endings.
#   3. The filename must be anchored to end-of-line. Without the anchor,
#      `garraia-windows-x86_64.exe` also matches its own `.sha256` sibling --
#      and, since the archives landed, `garraia-linux-x86_64` would match
#      `garraia-linux-x86_64.tar.gz`. That is the additive-release invariant:
#      adding archives must never change which line an existing client selects.

. (Join-Path $PSScriptRoot '_harness.ps1')
. (Get-InstallerPath)

$fixtures = Join-Path $PSScriptRoot 'fixtures'

Write-Host 'Select-ChecksumLine: text mode (two spaces)'
$lines = Get-Content (Join-Path $fixtures 'SHA256SUMS.text-mode')
Assert-Equal 'linux x86_64'  '1111111111111111111111111111111111111111111111111111111111111111' `
    (Get-ExpectedHash (Select-ChecksumLine -Artifact 'garraia-linux-x86_64' -Lines $lines))
Assert-Equal 'windows .exe'  '2222222222222222222222222222222222222222222222222222222222222222' `
    (Get-ExpectedHash (Select-ChecksumLine -Artifact 'garraia-windows-x86_64.exe' -Lines $lines))

Write-Host ''
Write-Host 'Select-ChecksumLine: binary mode (space + asterisk)'
$lines = Get-Content (Join-Path $fixtures 'SHA256SUMS.binary-mode')
Assert-Equal 'linux x86_64'  '3333333333333333333333333333333333333333333333333333333333333333' `
    (Get-ExpectedHash (Select-ChecksumLine -Artifact 'garraia-linux-x86_64' -Lines $lines))
Assert-Equal 'windows .exe'  '4444444444444444444444444444444444444444444444444444444444444444' `
    (Get-ExpectedHash (Select-ChecksumLine -Artifact 'garraia-windows-x86_64.exe' -Lines $lines))

Write-Host ''
Write-Host 'Select-ChecksumLine: CRLF line endings'
$lines = Get-Content (Join-Path $fixtures 'SHA256SUMS.crlf')
Assert-Equal 'linux x86_64'  '5555555555555555555555555555555555555555555555555555555555555555' `
    (Get-ExpectedHash (Select-ChecksumLine -Artifact 'garraia-linux-x86_64' -Lines $lines))
Assert-Equal 'windows .exe'  '6666666666666666666666666666666666666666666666666666666666666666' `
    (Get-ExpectedHash (Select-ChecksumLine -Artifact 'garraia-windows-x86_64.exe' -Lines $lines))

Write-Host ''
Write-Host 'Select-ChecksumLine: end-of-line anchoring (the additive-release invariant)'
# A realistic post-v0.3.4 SHA256SUMS: the bare binary, its own .sha256
# sibling, the .tar.gz / .zip archives and the Linux packages
# (.deb / .rpm / .AppImage) all share a filename prefix, and the native
# Windows ARM64 pair sits beside the x86_64 one.
$mixed = @(
    'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  garraia-linux-x86_64.sha256',
    'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  garraia-linux-x86_64.tar.gz',
    'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc  garraia-linux-x86_64',
    'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd  garraia-windows-x86_64.exe.sha256',
    'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee  garraia-windows-x86_64.zip',
    'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff  garraia-windows-x86_64.exe',
    '0000000000000000000000000000000000000000000000000000000000000000  garraia-linux-x86_64.deb',
    '1234123412341234123412341234123412341234123412341234123412341234  garraia-linux-x86_64.rpm',
    '5678567856785678567856785678567856785678567856785678567856785678  garraia-linux-x86_64.AppImage',
    '9999999999999999999999999999999999999999999999999999999999999999  garraia-windows-aarch64.exe',
    '8888888888888888888888888888888888888888888888888888888888888888  garraia-windows-aarch64.zip'
)
Assert-Equal 'bare binary does not match .sha256/.tar.gz/.deb/.rpm/.AppImage' `
    'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc' `
    (Get-ExpectedHash (Select-ChecksumLine -Artifact 'garraia-linux-x86_64' -Lines $mixed))
Assert-Equal 'the .tar.gz still selectable on its own' `
    'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' `
    (Get-ExpectedHash (Select-ChecksumLine -Artifact 'garraia-linux-x86_64.tar.gz' -Lines $mixed))
Assert-Equal 'windows .exe does not match .exe.sha256 or .zip' `
    'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff' `
    (Get-ExpectedHash (Select-ChecksumLine -Artifact 'garraia-windows-x86_64.exe' -Lines $mixed))
Assert-Equal 'the .deb still selectable on its own' `
    '0000000000000000000000000000000000000000000000000000000000000000' `
    (Get-ExpectedHash (Select-ChecksumLine -Artifact 'garraia-linux-x86_64.deb' -Lines $mixed))
Assert-Equal 'the .AppImage still selectable on its own' `
    '5678567856785678567856785678567856785678567856785678567856785678' `
    (Get-ExpectedHash (Select-ChecksumLine -Artifact 'garraia-linux-x86_64.AppImage' -Lines $mixed))
Assert-Equal 'native ARM64 .exe does not match its .zip' `
    '9999999999999999999999999999999999999999999999999999999999999999' `
    (Get-ExpectedHash (Select-ChecksumLine -Artifact 'garraia-windows-aarch64.exe' -Lines $mixed))

Write-Host ''
Write-Host 'Select-ChecksumLine: absent artifact'
Assert-Equal 'missing artifact yields nothing' '' (Select-ChecksumLine -Artifact 'garraia-macos-aarch64' -Lines $mixed)
Assert-Equal 'empty sums file yields nothing'  '' (Select-ChecksumLine -Artifact 'garraia-linux-x86_64' -Lines @())

Write-Host ''
Write-Host 'Get-ExpectedHash: validation and normalization'
Assert-Equal 'uppercase hash is lowercased' 'abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789' `
    (Get-ExpectedHash -ChecksumLine 'ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789  garraia-linux-x86_64')
Assert-Throws 'non-hex token rejected'   { Get-ExpectedHash -ChecksumLine 'zzzz  garraia-linux-x86_64' }
Assert-Throws 'short hash rejected'      { Get-ExpectedHash -ChecksumLine 'abc123  garraia-linux-x86_64' }

Exit-WithSummary 'checksum_format'
