# Runbook de Release

Como cortar uma release `vX.Y.Z` do GarraIA. Tudo depois do tag é automático.

## 1. Preparar (via PR — o `main` é protegido)

1. Bump da versão do workspace em `Cargo.toml` (`[workspace.package] version`).
2. Rodar `cargo check` para o `Cargo.lock` acompanhar (nunca editar o lock à mão).
3. Juntar os fragmentos de changelog acumulados desde a última versão:

   ```bash
   python3 scripts/changelog/assemble.py            # confere o que vai entrar
   python3 scripts/changelog/assemble.py --write    # insere no [Unreleased] e apaga os fragmentos
   ```

   Os PRs não editam mais o `CHANGELOG.md` direto — cada um deixa um arquivo em
   `changelog.d/<seção>/` (ver `changelog.d/README.md`), justamente para dois PRs
   paralelos não colidirem na mesma seção. Este passo é onde os fragmentos viram
   changelog de verdade. Revise o texto agregado antes de seguir: o script junta,
   não escreve por você.
4. `CHANGELOG.md`: mover o conteúdo de `## [Unreleased]` para uma nova seção
   `## [X.Y.Z] - AAAA-MM-DD` (data narrativa em horário da Flórida,
   ver CLAUDE.md §Convenção de datas). `[Unreleased]` volta vazio.
5. Abrir PR, aguardar CI verde (6 checks obrigatórios da ruleset: Format Check, Clippy Linting, Test ubuntu e windows, Security Gate, Auth Integration) e mergear.

## 1.5 Gate de dogfood — obrigatório antes do tag (#1439)

O CI prova que o código compila e que os testes passam; **não** prova que uma
instalação limpa funciona para uma pessoa. A v0.4.4 e a v0.4.5 saíram verdes
e quebraram no caminho real (WhatsApp numa instalação nova, `NoRoots`,
status que mentia). Por isso, antes de empurrar o tag, alguém com máquina e
telefone executa a matriz abaixo **contra o candidato** e registra data, sistema
e quem executou. Linha sem data = release não sai.

**Onde o candidato existe — e onde ele NÃO existe.** O `release.yml` **não tem
modo de teste**: seu `workflow_dispatch` aceita um `version`, mas o job `release`
usa `softprops/action-gh-release` com `draft: false` — ele **cria o tag no commit
do run e publica a Release de verdade**. Dispará-lo esperando artefato de teste
publica a release sem querer. (Esta seção afirmava o contrário até 2026-09-28.)

Fora dele, cada grupo de linhas tem uma origem de candidato diferente, e três
delas **não precisam de tag nenhum**:

| Linhas | Candidato, sem publicar nada |
|---|---|
| D1, D3a, D9-linux | `scripts/dogfood/linux-clean-install.sh --source local` no checkout do candidato — empacota com o mesmo `packaging/nfpm.yaml` da release e instala num `ubuntu:24.04` cru |
| **D5, D6** | **`gh workflow run desktop.yml --ref <sha>`** — o `desktop.yml` tem `workflow_dispatch`, `permissions: contents: read` e **nenhum passo de publicação**: ele constrói MSI + NSIS no `windows-latest` e `.deb` + AppImage no `ubuntu-22.04` e sobe os bundles como *artifacts* (`garraia-desktop-windows-pr`, `garraia-desktop-linux-pr`). Baixar o artifact do run e instalar à mão é o candidato de desktop |
| D2 | precisa de *asset publicado*, mas **aceita um tag pinado**: `install.ps1 -Version <tag>` / `$env:GARRAIA_VERSION` (e `install.sh --version <tag>`) baixam a tag pedida em vez da `latest`. Um prerelease `vX.Y.Z-rcN` serve |
| **D4** | **não tem candidato pré-tag, por desenho do código.** `crates/garraia-cli/src/update.rs:9` fixa `RELEASES_API` em `.../releases/latest` — sem flag, sem env var, sem override. Testar `garra update` da versão anterior para a nova exige que a nova **seja** a `latest`, isto é, exige a publicação |

Daí duas consequências que valem como regra, não como opinião:

- **D4 e D8 são pós-publicação por desenho**, não pré-requisitos do tag. No D8 as
  URLs do `garraia.org` resolvem a release `latest`; no D4 é o próprio
  `update.rs` que só conhece a `latest`. Exigir os dois antes do tag é uma
  dependência circular — eles são verificação do §4.8, e uma falha ali vira a
  versão seguinte.
- **D1b, D3b, D7b, D10** e o trecho WhatsApp do D1 continuam manuais por desenho:
  precisam de dois telefones reais e de leitura humana da resposta do modelo.
  Nenhum caminho de build os automatiza.

Para o que sobra, os caminhos reais, e a escolha é do dono:

1. **Prerelease `vX.Y.Z-rcN`** por push de tag — produz assets reais e é marcado
   prerelease automaticamente, então não vira `latest` e nem o `install.sh` nem
   o `garra update` o alcançam. Habilita D2 (via `-Version vX.Y.Z-rcN`). Limite
   conhecido: o `ProductVersion` do WiX é numérico de três partes, então **tag
   `-rc` faz o job do MSI falhar** — mas o D5 não depende disso, porque o
   `desktop.yml` acima entrega o MSI sem tag.
2. **Aceite de risco registrado**, como na v0.4.6 (PR #1522): tagear com a
   cobertura que existe e rodar as linhas manuais depois, com o compromisso
   escrito de que uma falha vira a versão seguinte. A decisão vai no corpo da PR
   de release e no corpo da Release.

| # | Cenário | Onde | O que prova | Automatizado? |
|---|---|---|---|---|
| D1 | Instalação limpa da CLI (`curl \| sh`), `garraia init` com um provedor, `garraia whatsapp link`, um número autorizado, "oi" → resposta real | Ubuntu 22.04 limpo, com Node 20+ | o caminho que o usuário faz | parcial — **Linux automatizado até "resposta real"** por `scripts/dogfood/linux-clean-install.sh` (`.deb` num `ubuntu:24.04` cru, provedor Ollama local, sessão REST, `restart`, `stop`); o `whatsapp link` e o número autorizado continuam manuais |
| D2 | Mesmo cenário via `irm \| iex` | Windows 10/11 | paridade dos instaladores (regra 16) | não |
| D3 | `garraia restart` → nova mensagem responde **sem** QR; `allow` sobrevive | Ubuntu + Windows | persistência da sessão e da política | fixture `serve-echo` + manual |
| D4 | `garraia update` da release anterior para o candidato; `garraia rollback` | Ubuntu | o contrato dos assets crus (regra 15) e o `.old` | não — e **pós-publicação por desenho**: `update.rs:9` só conhece `releases/latest` |
| D5 | Desktop: MSI instala, papagaio e Chat Bar aparecem, `garraia status` no terminal mostra o sidecar, sair encerra o sidecar | Windows 11 | o bundle e o sidecar | manual, mas o **bundle sai sem tag** pelo `desktop.yml` (dispatch → artifact `garraia-desktop-windows-pr`) |
| D6 | Desktop: `.deb` idem | Ubuntu 22.04 (X11) | idem | manual, mas o **bundle sai sem tag** pelo `desktop.yml` (artifact `garraia-desktop-linux-pr`) |
| D7 | Duas identidades autorizadas pedem `list_dir`; nenhuma vê os arquivos da outra | Ubuntu | isolamento do workspace por sessão (#1449) | teste de integração + manual |
| D8 | `install-endpoints.yml` verde depois de publicar | — | `garraia.org` serve os instaladores (regra 17) | sim (workflow) |
| D9 | `GET /api/diagnostics` numa instalação limpa sem nenhum aviso espúrio; `garraia doctor` exit 0 | Ubuntu + Windows | honestidade do status (#1437, #1387) | parcial — **Linux automatizado** pelo mesmo script (`doctor --json` exit 0, `doctor whatsapp --json` exit 69 com a linha `whatsapp.linked`, `/api/health` `healthy`); Windows manual |
| D10 | Perguntas diretas de capacidade ao agente ("Você tem MCP?", "Pode escrever arquivos?", "Por que não lê esta pasta?") numa sessão `standard` e numa `isolated-pod` | Ubuntu + Windows | a resposta do modelo bate com o registro de capacidades — nada de "não tenho MCP" com MCP escondido nem "não tenho filesystem" com workspace só esperando seleção (#1428, #1387) | não — os fundamentos (nota do prompt e registro de capacidades) são cobertos em CI por `nota_garra_status.rs` e pelos testes do registro; a resposta do modelo continua manual |

**D1/D9 Linux — como rodar a automação (#1426).** Numa máquina com
Docker e Ollama (`ollama pull qwen3.5:0.8b`), a partir do checkout do
candidato:

```bash
scripts/dogfood/linux-clean-install.sh --source local        # empacota este checkout com o nfpm da release
scripts/dogfood/linux-clean-install.sh --run-id <run-id>     # ou o .deb de um run já verde do release.yml (testa a release ANTERIOR)
```

O script sobe um `ubuntu:24.04` sem nada de desenvolvimento, instala o
`.deb` com `apt-get install`, roda os doctors, aponta o provedor para o
Ollama do host (sem chave paga), sobe o gateway na **3899** (nunca na 3888
do host), exige uma resposta real do modelo por REST, reinicia e prova que
a sessão e a config sobreviveram, e encerra com `garraia stop`. A evidência
inteira (logs, JSONs, resposta do modelo, `.deb` testado) fica em
`dogfood/linux/<data-hora>/` (gitignored) com um `resumo.txt` PASSOU/FALHOU
por passo — é esse resumo que vai na tabela da PR de release. Sem
`--source`/`--run-id`, o default baixa o pacote do **último run verde do
`release.yml`**, o que testa a release anterior e não o candidato: serve
para reproduzir um bug de campo, não para o gate.

Regras do gate:

- **Contra o candidato, não contra `main`**: o que se testa é o que vai ser
  publicado.
- **Segredos fora da evidência**: transcrições redigidas, telefones só pelos
  quatro últimos dígitos, nenhuma chave em screenshot.
- **Falhou, não sai**: uma linha vermelha vira issue com o log e a release
  espera a correção — nunca "sai e conserta na próxima".
- A tabela preenchida vai no corpo da PR de release (bump + CHANGELOG), para
  ficar ao lado do que ela libera.

A automação do que dá para automatizar (D1 com ponte falsa, D3, D7, D9) é a
issue #1426; a parte Linux de D1/D9 já roda pelo script acima, o resto (D2,
D4, D5, D6 e o trecho WhatsApp de D1) segue manual e está aqui de propósito
— um gate que só existe numa issue não gateia nada. O desenho completo, com
o que o CI cobre e o que só pessoa cobre, está em
[`plans/0364-release-v0.4.6-preflight.md`](../plans/0364-release-v0.4.6-preflight.md) §6.

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
| `garraia-mobile-android.apk` | job `build-android-apk` (Flutter, `apps/garraia-mobile`) | **best-effort**; assinado com os secrets `ANDROID_KEYSTORE_*` quando existem, senão com a keystore de debug do runner (o job avisa) |
| `SHA256SUMS` + um `<asset>.sha256` por asset | step `Generate checksums` | sim |

**Por que os binários crus continuam publicados.** `garra update` resolve o
asset por nome exato (`update.rs:42-48`) e exige o `<asset>.sha256` irmão
(`:127`). Os archives são **aditivos**: trocá-los pelos binários quebraria o
auto-update de toda instalação já existente no momento em que ela pulasse para
essa versão. Não renomeie nem remova os assets crus.

**Instaladores desktop, ARM64 do Windows, pacotes Linux e o APK são best-effort.**
Os jobs `build-windows-installer`, `build-windows-aarch64`, `package-linux`,
`build-linux-desktop` e `build-android-apk` estão no `needs:` do job `release` mas deliberadamente
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
3. Attestations: `gh attestation verify garraia-linux-x86_64
   -R michelbr84/GarraRUST --tag vX.Y.Z` — o step `Attest build provenance`
   do `release.yml` assina cada asset com a identidade do workflow; verificar
   ao menos o binário principal de cada plataforma. Falhou = a release saiu
   sem a attestacao (o step roda antes do `Create Release`, entao um run
   vermelho ali significa release sem assets novos).
4. `garra update` a partir da versão anterior encontra e instala a nova.
   Este é o teste que prova que os formatos novos continuaram aditivos.
5. Windows: `irm https://github.com/michelbr84/GarraRUST/releases/latest/download/install.ps1 | iex`
   numa máquina real; abrir um terminal **novo** e confirmar `garraia --version`
   (prova que o PATH persistiu no registro, não só na sessão).
6. Pacotes Linux (quando o `package-linux` passou):
   `docker run --rm -v $PWD:/pkg ubuntu:22.04 bash -c "apt-get update -q && apt install -y /pkg/garraia-linux-x86_64.deb && garraia --version"`;
   `docker run --rm -v $PWD:/pkg fedora rpm -qip /pkg/garraia-linux-x86_64.rpm`;
   `chmod +x garraia-linux-x86_64.AppImage && ./garraia-linux-x86_64.AppImage --version`.
7. Deploy da imagem: com tag criado via push, o run `Deploy` dispara sozinho;
   com release via `workflow_dispatch`, disparar Actions → Deploy com
   `tag=vX.Y.Z` (ver §2). Depois `docker pull ghcr.io/michelbr84/garraia:<tag>`.
8. Garra Mobile: baixar `garraia-mobile-android.apk`, conferir o `.sha256`, instalar por
   sideload num Android 11+ e rodar a seção 1 e a seção *Runtime* do
   `docs/mobile-qa-checklist.md`. Sem os secrets `ANDROID_KEYSTORE_*` o APK não atualiza
   por cima de uma instalação assinada por outra chave — desinstale antes.
9. No dia seguinte, o `install-endpoints.yml` agendado deve estar verde —
   inclusive a sonda `release-cdn/install.ps1`, que ficava vermelha por design
   enquanto nenhuma release publicava o `install.ps1`.

## Rollback

Release ruim: apagar a Release e o tag (`git push origin :refs/tags/vX.Y.Z`),
corrigir via PR e cortar `vX.Y.Z+1`. Nunca reutilizar um tag já publicado —
`garra update` verifica SHA-256 e clientes podem ter cacheado os assets.
