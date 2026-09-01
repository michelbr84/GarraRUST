# Runbook de Release

Como cortar uma release `vX.Y.Z` do GarraIA. Tudo depois do tag é automático.

## 1. Preparar (via PR — o `main` é protegido)

1. Bump da versão do workspace em `Cargo.toml` (`[workspace.package] version`).
2. Rodar `cargo check` para o `Cargo.lock` acompanhar (nunca editar o lock à mão).
3. `CHANGELOG.md`: mover o conteúdo de `## [Unreleased]` para uma nova seção
   `## [X.Y.Z] - AAAA-MM-DD` (data narrativa em horário da Flórida,
   ver CLAUDE.md §Convenção de datas). `[Unreleased]` volta vazio.
4. Abrir PR, aguardar CI verde (4 checks obrigatórios da ruleset) e mergear.

## 2. Tag

```bash
git fetch origin main
git tag vX.Y.Z <sha-do-merge>   # ou no HEAD de origin/main
git push origin vX.Y.Z
```

**Caminho alternativo (usado nas v0.3.3/v0.3.4): `workflow_dispatch`.**
Actions → Release → Run workflow em `main` com `version=vX.Y.Z`. O
`softprops/action-gh-release` cria o tag no commit do run. **Atenção:** um tag
criado assim nasce do `GITHUB_TOKEN`, que **não dispara** workflows de
tag-push — o `deploy.yml` (imagem ghcr) precisa então de dispatch manual com
`tag=vX.Y.Z` (passo 6 da verificação). Nesse modo a imagem sai com as tags
`vX.Y.Z` + `latest` + sha, sem as derivadas semver `X.Y.Z`/`X.Y`.

## 3. O que o tag dispara (automático)

| Workflow | Publica |
|---|---|
| `release.yml` | Binários linux-x86_64/arm64, windows x86_64/arm64, macos-intel/arm64 + os archives correspondentes + pacotes Linux (`.deb`/`.rpm`/AppImage) + `install.sh` + `install.ps1` + os instaladores desktop do Windows + `SHA256SUMS` **e** um `<asset>.sha256` por asset (obrigatório: `garra update` lê o per-asset — `crates/garraia-cli/src/update.rs`) numa GitHub Release com notas geradas |
| `deploy.yml` | Imagem `ghcr.io/michelbr84/garraia` multi-arch (amd64+arm64), tags `X.Y.Z`, `X.Y` e sha |

Assets da `release.yml`, em detalhe:

| Asset | Origem | Obrigatório? |
|---|---|---|
| `garraia-{linux,macos}-{x86_64,aarch64}`, `garraia-windows-x86_64.exe` | os 5 jobs de build principais | x86_64 sim; aarch64 best-effort |
| `garraia-windows-aarch64.exe` | job `build-windows-aarch64` | **best-effort** (wasmtime é Tier 3 nesse alvo) |
| `garraia-*.tar.gz` / `garraia-windows-{x86_64,aarch64}.zip` | step `Package archives` | acompanha o binário correspondente |
| `garraia-linux-{x86_64,aarch64}.deb` / `.rpm` | job `package-linux` (nfpm pinado, ADR 0015) | **best-effort**; aarch64 depende do binário aarch64 |
| `garraia-linux-x86_64.AppImage` | job `package-linux` (appimagetool pinado) | **best-effort** |
| `install.sh`, `install.ps1` | copiados do repo | sim |
| `garraia-desktop-windows-x86_64.msi`, `…-setup.exe` | job `build-windows-installer` | **best-effort** |
| `garraia-desktop-linux-x86_64.deb` / `.AppImage` | job `build-linux-desktop` (bundler do Tauri via `scripts/build-desktop-linux.sh`) | **best-effort**; AppImage ~80-100MB (embute webkit2gtk) |
| `SHA256SUMS` + um `<asset>.sha256` por asset | step `Generate checksums` | sim |

**Por que os binários crus continuam publicados.** `garra update` resolve o
asset por nome exato (`update.rs:42-48`) e exige o `<asset>.sha256` irmão
(`:127`). Os archives são **aditivos**: trocá-los pelos binários quebraria o
auto-update de toda instalação já existente no momento em que ela pulasse para
essa versão. Não renomeie nem remova os assets crus.

**Instaladores desktop, ARM64 do Windows e pacotes Linux são best-effort.**
Os jobs `build-windows-installer`, `build-windows-aarch64`, `package-linux` e
`build-linux-desktop` estão no `needs:` do job `release` mas deliberadamente
**fora** da condição `if:` — mesmo padrão do `build-linux-arm64`. Uma falha emite `::warning::` e a
release sai sem o asset correspondente, sem `continue-on-error` (proibido pelo
CLAUDE.md). Modo de falha conhecido: o `ProductVersion` do WiX é numérico de
três partes e não expressa prerelease semver, então **tags `-rc` fazem o job
do MSI falhar** — o gating absorve isso. O `package-linux` roda separado do
job `release` de propósito: nfpm e appimagetool são pinados por versão +
SHA-256, e uma falha deles não pode bloquear a release (ADR 0015).

Pré-releases: versões com `alpha`/`beta`/`rc` no nome são marcadas como
prerelease automaticamente.

## Débito conhecido: auto-updater do desktop Tauri

`crates/garraia-desktop/src-tauri/tauri.conf.json` declara um endpoint de
updater apontando para um `latest.json` que **nenhum workflow gera**, com
`pubkey` vazio. A falha é visível, não silenciosa: `commands.rs:102-112` retorna
`Err` e `tray.rs:158-166` imprime o erro — quem clicar em "Check for Updates"
vê uma mensagem de erro, e nada se atualiza sozinho em background. O caminho de
atualização suportado do produto é o `garra update` da CLI, que não é afetado.

Para reativá-lo, numa PR própria e nesta ordem:

1. `cargo tauri signer generate -w ~/.tauri/garraia.key` (local).
2. Criar os secrets `TAURI_SIGNING_PRIVATE_KEY` e
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` no repositório — **ação manual**, um
   agente não cria secrets.
3. Preencher `plugins.updater.pubkey`, ligar `bundle.createUpdaterArtifacts`,
   exportar os dois secrets como `env:` no job do Tauri e gerar o `latest.json`
   a partir dos `.sig` assinados.

Não foi feito junto com a revival do MSI de propósito: removê-lo exigiria editar
`lib.rs:25`, `commands.rs`, `tray.rs` e `capabilities/default.json` num crate que
o CI nunca havia compilado, empilhando duas mudanças não verificadas na mesma
entrega.

## 4. Verificar

1. Actions: run `Release` verde para o tag (com no máximo os jobs best-effort
   em vermelho: linux-arm64, macos-arm64, windows-aarch64, installer,
   package-linux).
2. `https://github.com/michelbr84/GarraRUST/releases/latest` aponta para a
   versão nova, com todos os assets e seus `.sha256`, e os 5 nomes crus da
   release anterior presentes **byte-idênticos** (superfície do `garra
   update`, regra 15).
3. `garra update` a partir da versão anterior encontra e instala a nova.
   Este é o teste que prova que os formatos novos continuaram aditivos.
4. Windows: `irm https://github.com/michelbr84/GarraRUST/releases/latest/download/install.ps1 | iex`
   numa máquina real; abrir um terminal **novo** e confirmar `garraia --version`
   (prova que o PATH persistiu no registro, não só na sessão).
5. Pacotes Linux (quando o `package-linux` passou):
   `docker run --rm -v $PWD:/pkg ubuntu:22.04 bash -c "apt-get update -q && apt install -y /pkg/garraia-linux-x86_64.deb && garraia --version"`;
   `docker run --rm -v $PWD:/pkg fedora rpm -qip /pkg/garraia-linux-x86_64.rpm`;
   `chmod +x garraia-linux-x86_64.AppImage && ./garraia-linux-x86_64.AppImage --version`.
6. Deploy da imagem: com tag criado via push, o run `Deploy` dispara sozinho;
   com release via `workflow_dispatch`, disparar Actions → Deploy com
   `tag=vX.Y.Z` (ver §2). Depois `docker pull ghcr.io/michelbr84/garraia:<tag>`.
7. No dia seguinte, o `install-endpoints.yml` agendado deve estar verde —
   inclusive a sonda `release-cdn/install.ps1`, que ficava vermelha por design
   enquanto nenhuma release publicava o `install.ps1`.

## Rollback

Release ruim: apagar a Release e o tag (`git push origin :refs/tags/vX.Y.Z`),
corrigir via PR e cortar `vX.Y.Z+1`. Nunca reutilizar um tag já publicado —
`garra update` verifica SHA-256 e clientes podem ter cacheado os assets.
