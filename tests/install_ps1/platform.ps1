# Unit tests for Get-GarraiaPlatform in install.ps1.
#
# The function maps the host architecture onto the release asset name, which
# is compatibility surface (CLAUDE.md, regra 15): these exact strings must
# match what release.yml publishes. From v0.3.4 the ARM64 branch returns the
# native garraia-windows-aarch64.exe instead of falling back to x86_64 under
# emulation — pinning that mapping here keeps a future refactor from silently
# reviving the fallback (or breaking the WOW64 detection).
#
# PROCESSOR_ARCHITECTURE / PROCESSOR_ARCHITEW6432 are plain env vars, so the
# cases run identically on the ubuntu (pwsh 7) and windows (PS 5.1) CI legs.

. (Join-Path $PSScriptRoot '_harness.ps1')
. (Get-InstallerPath)

# Save the real values so the suite leaves the host untouched.
$savedArch = $env:PROCESSOR_ARCHITECTURE
$savedWow  = $env:PROCESSOR_ARCHITEW6432

function Set-ArchEnvironment {
    param([string]$Architecture, [string]$Wow6432)
    if ($Architecture) { $env:PROCESSOR_ARCHITECTURE = $Architecture }
    elseif (Test-Path 'Env:PROCESSOR_ARCHITECTURE') { Remove-Item 'Env:PROCESSOR_ARCHITECTURE' }
    if ($Wow6432) { $env:PROCESSOR_ARCHITEW6432 = $Wow6432 }
    elseif (Test-Path 'Env:PROCESSOR_ARCHITEW6432') { Remove-Item 'Env:PROCESSOR_ARCHITEW6432' }
}

try {
    Write-Host 'Get-GarraiaPlatform: x86_64 hosts'
    Set-ArchEnvironment -Architecture 'AMD64'
    Assert-Equal 'AMD64 resolves to the x86_64 asset' 'garraia-windows-x86_64.exe' (Get-GarraiaPlatform)
    Set-ArchEnvironment -Architecture 'amd64'
    Assert-Equal 'lowercase amd64 also resolves (ToUpperInvariant)' 'garraia-windows-x86_64.exe' (Get-GarraiaPlatform)

    Write-Host ''
    Write-Host 'Get-GarraiaPlatform: ARM64 hosts get the native binary (v0.3.4+)'
    Set-ArchEnvironment -Architecture 'ARM64'
    Assert-Equal 'ARM64 resolves to the native aarch64 asset' 'garraia-windows-aarch64.exe' (Get-GarraiaPlatform)
    Set-ArchEnvironment -Architecture 'arm64'
    Assert-Equal 'lowercase arm64 also resolves' 'garraia-windows-aarch64.exe' (Get-GarraiaPlatform)
    # A 32-bit PowerShell on an ARM64 host reports x86 in PROCESSOR_ARCHITECTURE
    # and the truth in PROCESSOR_ARCHITEW6432 — the WOW64 var must win.
    Set-ArchEnvironment -Architecture 'x86' -Wow6432 'ARM64'
    Assert-Equal '32-bit shell on ARM64 host still gets aarch64' 'garraia-windows-aarch64.exe' (Get-GarraiaPlatform)

    Write-Host ''
    Write-Host 'Get-GarraiaPlatform: rejected architectures'
    Set-ArchEnvironment -Architecture 'x86'
    Assert-Throws '32-bit Windows is rejected' { Get-GarraiaPlatform }
    Set-ArchEnvironment -Architecture 'IA64'
    Assert-Throws 'unknown architecture is rejected' { Get-GarraiaPlatform }

    Write-Host ''
    Write-Host 'Get-GarraiaPlatform: defaults'
    Set-ArchEnvironment
    Assert-Equal 'no arch env vars defaults to x86_64' 'garraia-windows-x86_64.exe' (Get-GarraiaPlatform)
} finally {
    Set-ArchEnvironment -Architecture $savedArch -Wow6432 $savedWow
}

Exit-WithSummary 'platform'
