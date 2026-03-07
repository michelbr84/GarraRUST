# build-installer.ps1
# Gera o instalador MSI do Garra Desktop (gateway + overlay num unico pacote)
#
# Uso:
#   .\scripts\build-installer.ps1
#
# Pre-requisitos:
#   - Rust 1.85+ com target x86_64-pc-windows-msvc
#   - cargo-tauri instalado: cargo install tauri-cli
#   - WiX Toolset (instalado automaticamente pelo Tauri se ausente)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Write-Host "==> [1/3] Compilando gateway (release)..." -ForegroundColor Cyan
cargo build -p garraia --release
if ($LASTEXITCODE -ne 0) { throw "cargo build falhou" }

Write-Host "==> [2/3] Copiando sidecar para binaries/..." -ForegroundColor Cyan
$arch = "x86_64-pc-windows-msvc"
$binDir = "crates\garraia-desktop\src-tauri\binaries"
New-Item -ItemType Directory -Force -Path $binDir | Out-Null
Copy-Item "target\release\garraia.exe" "$binDir\garraia-$arch.exe" -Force
Write-Host "    Copiado: $binDir\garraia-$arch.exe"

Write-Host "==> [3/3] Gerando MSI com cargo tauri build..." -ForegroundColor Cyan
Push-Location "crates\garraia-desktop\src-tauri"
cargo tauri build
$buildExit = $LASTEXITCODE
Pop-Location
if ($buildExit -ne 0) { throw "cargo tauri build falhou" }

Write-Host ""
Write-Host "MSI gerado com sucesso!" -ForegroundColor Green
$msi = Get-ChildItem "target\release\bundle\msi\*.msi" -ErrorAction SilentlyContinue | Select-Object -First 1
if ($msi) {
    Write-Host "  $($msi.FullName)" -ForegroundColor Green
}
$nsis = Get-ChildItem "target\release\bundle\nsis\*-setup.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
if ($nsis) {
    Write-Host "  $($nsis.FullName)" -ForegroundColor Green
}
