# Plan 0361 — Pacotes Linux (.deb/.rpm/AppImage) + Windows ARM64 nativo + release v0.3.4

**Status:** ✅ Entregue 2026-08-31 (horário da Flórida)
**Branch:** `claude/releases-nova-versao-hpyhx3`

## Problema

A v0.3.3 (2026-08-28) saiu **antes** do plan 0359, então a matriz nova de
artefatos (archives, MSI/NSIS, `install.ps1`) existia no `main` mas nunca tinha
sido exercitada numa release publicada — a sonda `install-endpoints.yml` do
`install.ps1` no CDN de release falhava por design, e três trechos de docs
afirmavam coisas "a partir da v0.3.4" de uma tag que não existia. Além disso,
o backlog explícito do 0359 (§"Fora de escopo") seguia aberto: **`.deb`,
`.rpm`, AppImage e Windows ARM64 nativo** — usuários de Windows ARM64 rodavam
o binário x86_64 sob emulação, e Linux não tinha integração com
`apt`/`dnf`/`zypper`.

## Restrição estruturante

A mesma do 0359 (CLAUDE.md regra 15): `crates/garraia-cli/src/update.rs`
resolve assets por **nome exato** e exige o `.sha256` irmão. Todo formato novo
entra **aditivamente** — os 5 nomes crus da v0.3.3 existem byte-idênticos na
v0.3.4. A âncora fim-de-linha do `select_checksum_line` (install.sh) /
`Select-ChecksumLine` (install.ps1) garante que `garraia-linux-x86_64` nunca
case com `garraia-linux-x86_64.{tar.gz,deb,rpm,AppImage,sha256}` — agora
coberto por teste dos dois lados (regra 16).

## Entregue

### 1. Job `build-windows-aarch64` (release.yml)

Cross-compile `aarch64-pc-windows-msvc` no runner `windows-latest`
(`dtolnay/rust-toolchain@stable` com `targets:`), produzindo
`garraia-windows-aarch64.exe` + `garraia-windows-aarch64.zip` (via `pack()`
existente). Best-effort no padrão needs-mas-fora-do-`if:` — wasmtime é Tier 3
nesse alvo e é o build com mais chance de quebrar; a falha nunca bloqueia a
release.

### 2. `garra update` + `install.ps1` resolvem o ARM64 nativo

- `update.rs`: `platform_asset_name()` refatorado para `asset_name_for(os,
  arch)` pura + arm `("windows","aarch64")`; primeiro módulo de testes do
  arquivo fixa os 6 nomes como superfície de compatibilidade, o `bail!` em
  plataforma não suportada e o contrato do irmão `.sha256`.
- `install.ps1` (`Get-GarraiaPlatform`): hosts ARM64 recebem
  `garraia-windows-aarch64.exe` em vez do x86_64 emulado. **Sem fallback**:
  pinar `GARRAIA_VERSION < v0.3.4` em ARM64 dá 404 — caveat documentado, com
  precedente no install.sh (Apple Silicon pré-v0.2.1).

### 3. Job `package-linux` (release.yml) + `packaging/`

- **nfpm v2.47.0** empacota os binários já buildados em
  `garraia-linux-{x86_64,aarch64}.{deb,rpm}` (aarch64 best-effort);
  **appimagetool 1.9.1** gera `garraia-linux-x86_64.AppImage`. Ambos pinados
  por versão + SHA-256 no workflow. Decisão de toolchain:
  [ADR 0015](../docs/adr/0015-linux-packaging-toolchain.md).
- Job separado do `release` de propósito (falha de tooling novo não pode
  bloquear a release); smoke com `dpkg-deb --info/--contents`, `rpm -qip` e
  execução do AppImage (`--version`), que de quebra prova o binário de 22.04
  rodando em 24.04.
- Config em `packaging/nfpm.yaml` (env-expandido) e
  `packaging/appimage/garraia.desktop`; ícone reusado do Tauri.
- Pacotes instalam `/usr/bin/garraia`; `.deb` depende de `libc6 (>= 2.35)`.

### 4. Testes (paridade regra 16)

- Nova suíte `tests/install_ps1/platform.ps1` (arquiteturas, WOW64, rejeição
  de x86, default) registrada no job `installer-powershell` do ci.yml (a
  lista é explícita — suíte nova não roda sozinha).
- `checksum_format.ps1` **e** `checksum_format.sh` ganham o caso mixed com
  `.deb`/`.rpm`/`.AppImage` e o par `windows-aarch64` — fechando o gap de
  paridade (o lado sh só cobria o irmão `.sha256`).

### 5. Corte da v0.3.4 + docs

`CHANGELOG.md` com `[Unreleased]` fundido na seção `[0.3.4]` (data
2026-08-31); matriz de assets e verificação novas em `docs/releasing.md`
(incluindo o passo manual do Deploy — ver abaixo); `docs/installation.md` com
a linha windows-aarch64 na tabela + seção "Linux packages" (com o caveat do
`garra update` sob pacote root-owned); READMEs (incl. reescrita da linha
estale do pt-BR sobre MSI/APK "até a v0.2.1"); wiki; ROADMAP §4.1 (restantes:
só DMG notarizado + AppImage aarch64); TODO.md; CLAUDE.md regra 15 com a
lista aditiva estendida.

## Decisão operacional: Deploy manual pós-dispatch

A release v0.3.3/v0.3.4 é criada via `workflow_dispatch` do Release, e a tag
resultante nasce do `GITHUB_TOKEN` — que **não dispara** workflows de
tag-push. Logo o `deploy.yml` (imagem ghcr multi-arch) precisa de dispatch
manual com `tag=vX.Y.Z` após a release. Documentado no runbook §4; o
dispatch não produz as tags semver derivadas (`0.3.4`/`0.3`) da imagem —
aceito e registrado.

## Verificação executada

| Verificação | Resultado |
|---|---|
| `cargo check -p garraia` + `cargo clippy -p garraia` | ✅ limpo (rustc 1.98) |
| `cargo test -p garraia update::` | ✅ 4/4 (módulo novo) |
| `bash tests/install_sh/checksum_format.sh` | ✅ 8/8 (3 casos novos) |
| Demais suítes `tests/install_sh/` | ✅ (ver PR) |
| YAML de `release.yml`/`ci.yml` parseado + estrutura de jobs/needs/if conferida | ✅ |
| Suítes `tests/install_ps1/` | CI (matriz pwsh 7 + PS 5.1; sem pwsh no ambiente local) |
| Hashes dos pins | nfpm: digest da release + `checksums.txt` (2 fontes); appimagetool: digest da release do GitHub |

Só o dispatch real prova: o build `aarch64-pc-windows-msvc` (wasmtime Tier
3), o fluxo de artifacts pelo `package-linux`, e o segundo exercício do
caminho archives + MSI do 0359.

## Fora de escopo / pendente do mantenedor

- DMG notarizado (exige Apple Developer cert + notarização — segredos do
  mantenedor); APK Android assinado (sem keystore e sem CI Flutter).
- AppImage aarch64 (`--runtime-file` + segundo pin de runtime).
- Assinatura GPG de `.deb`/`.rpm` e repositórios apt/dnf hospedados.
- Code-signing Windows (OV/EV) — inalterado desde o 0359.
- `garraia.org`: nenhuma URL pública nova foi introduzida (tudo é CDN de
  release), então o repo do site não precisa de mudança (regra 17).
