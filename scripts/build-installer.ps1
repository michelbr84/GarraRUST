# build-installer.ps1
# Gera os instaladores do Garra Desktop (gateway + overlay num unico pacote):
# MSI (WiX) e, quando o bundler produz, o setup NSIS.
#
# Uso:
#   .\scripts\build-installer.ps1
#   .\scripts\build-installer.ps1 -StageDir "C:\out"   # copia os bundles para la
#
# Pre-requisitos:
#   - Rust 1.96+ (MSRV da workspace) com target x86_64-pc-windows-msvc
#   - cargo-tauri instalado: cargo install tauri-cli --version "^2"
#   - WiX Toolset e NSIS (baixados automaticamente pelo Tauri se ausentes)
#
# Este script e a fonte unica de verdade de "como se constroi um instalador
# do Garra Desktop": o job `build-windows-installer` do .github/workflows/
# release.yml o invoca em vez de repetir os passos.

param(
    # Quando informado, os bundles encontrados sao copiados para este diretorio
    # com nomes estaveis, sem a versao embutida, para o CI consumir por glob.
    [string]$StageDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Write-Host "==> [1/4] Compilando gateway (release)..." -ForegroundColor Cyan
cargo build -p garraia --release
if ($LASTEXITCODE -ne 0) { throw "cargo build falhou" }

Write-Host "==> [2/4] Copiando sidecar para binaries/..." -ForegroundColor Cyan
$arch = "x86_64-pc-windows-msvc"
$binDir = "crates\garraia-desktop\src-tauri\binaries"
New-Item -ItemType Directory -Force -Path $binDir | Out-Null
# Tauri externalBin = "binaries/garraia" expects "binaries/garraia-<triple>.exe".
# O nome do arquivo tem de casar com o basename do externalBin em
# tauri.conf.json, que por sua vez tem de casar com o `.sidecar("garraia")`
# de src/gateway.rs:14 -- se divergir, o app instala mas o gateway nunca sobe.
# O binario da CLI em si chama-se `garra` (crates/garraia-cli/Cargo.toml).
Copy-Item "target\release\garra.exe" "$binDir\garraia-$arch.exe" -Force
Write-Host "    Copiado: $binDir\garraia-$arch.exe"

Write-Host "==> [3/4] Gerando bundles com cargo tauri build..." -ForegroundColor Cyan
Push-Location "crates\garraia-desktop\src-tauri"
cargo tauri build
$buildExit = $LASTEXITCODE
Pop-Location
if ($buildExit -ne 0) { throw "cargo tauri build falhou" }

Write-Host "==> [4/4] Localizando bundles..." -ForegroundColor Cyan
# Falha alto de proposito. A versao anterior usava -ErrorAction SilentlyContinue
# e so imprimia quando encontrava: um `cargo tauri build` que saisse 0 sem
# emitir bundle deixava o script verde e o CI publicava uma release sem
# instalador nenhum, sem nada no log dizendo por que.
$msi = Get-ChildItem "target\release\bundle\msi\*.msi" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $msi) {
    throw "nenhum MSI encontrado em target\release\bundle\msi\ apos o build"
}
Write-Host "    MSI:  $($msi.FullName)" -ForegroundColor Green

# O NSIS e opcional: nem toda combinacao de toolchain o produz, e o MSI ja e
# um instalador completo. Ausencia vira aviso, nao erro.
$nsis = Get-ChildItem "target\release\bundle\nsis\*-setup.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
if ($nsis) {
    Write-Host "    NSIS: $($nsis.FullName)" -ForegroundColor Green
} else {
    Write-Host "    NSIS: nao produzido (opcional)" -ForegroundColor Yellow
}

if ($StageDir) {
    New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
    # Nomes estaveis, sem versao: assim a URL
    # /releases/latest/download/garraia-desktop-windows-x86_64.msi e permanente
    # e pode ser fixada na documentacao. A versao continua nos metadados do MSI.
    Copy-Item $msi.FullName (Join-Path $StageDir "garraia-desktop-windows-x86_64.msi") -Force
    Write-Host "    Staged: garraia-desktop-windows-x86_64.msi"
    if ($nsis) {
        Copy-Item $nsis.FullName (Join-Path $StageDir "garraia-desktop-windows-x86_64-setup.exe") -Force
        Write-Host "    Staged: garraia-desktop-windows-x86_64-setup.exe"
    }

    # Artefactos de updater (#1568): com `createUpdaterArtifacts: true` o
    # Tauri 2.x emite o `.sig` vizinho do PROPRIO bundle (`.msi` /
    # `-setup.exe`), sem zip intermediario — o formato `.nsis.zip` e legado
    # (v1/v1Compatible) e neste projeto nao e produzido. O `latest-json.py`
    # reconhece os dois formatos; estagiamos os dois para cobrir qualquer
    # mudanca de comportamento do bundler.
    foreach ($bundle in @($msi, $nsis)) {
        if ($bundle -and (Test-Path "$($bundle.FullName).sig")) {
            $sigAlvo = Join-Path $StageDir "$($bundle.Name).sig"
            Copy-Item "$($bundle.FullName).sig" $sigAlvo -Force
            Write-Host "    Staged: $($bundle.Name).sig (updater)"
        } elseif ($bundle) {
            Write-Host "    AVISO: $($bundle.Name).sig ausente - latest.json vai recusar esta plataforma (fail-closed)" -ForegroundColor Yellow
        }
    }
    $nsisZip = Get-ChildItem "target\release\bundle\nsis\*.nsis.zip" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($nsisZip) {
        Copy-Item $nsisZip.FullName (Join-Path $StageDir "garraia-desktop-windows-x86_64.nsis.zip") -Force
        Write-Host "    Staged: garraia-desktop-windows-x86_64.nsis.zip (updater, formato legado)"
        if (Test-Path "$($nsisZip.FullName).sig") {
            Copy-Item "$($nsisZip.FullName).sig" (Join-Path $StageDir "garraia-desktop-windows-x86_64.nsis.zip.sig") -Force
            Write-Host "    Staged: garraia-desktop-windows-x86_64.nsis.zip.sig (updater, formato legado)"
        }
    }
}

Write-Host ""
Write-Host "Instaladores gerados com sucesso!" -ForegroundColor Green
