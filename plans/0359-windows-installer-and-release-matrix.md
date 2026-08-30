# Plan 0359 — Instalador Windows (`install.ps1`) + matriz de artefatos

**Status:** ✅ Entregue 2026-08-30 (horário da Flórida)
**Branch:** `claude/garraia-windows-installer-3wjjb2`

## Problema

A pergunta que originou o plano foi "como se instala o GarraIA no Windows?".
A resposta apurada era: **não se instala** — não de forma comparável a
Linux/macOS.

- **`install.ps1` nunca existiu.** Nem no repo, nem nas releases, nem
  hospedado. O único bootstrap era o `install.sh` (POSIX), e o `README.md:104`
  o rotulava "Install via script (**Linux, macOS**)".
  `docs/installation.md:42-44` resolvia o Windows com uma frase: *"Download the
  pre-compiled binary from GitHub Releases"* — sem PATH, sem verificação de
  checksum, sem `init`.
- **O `.exe` publicado não é instalador.** `garraia-windows-x86_64.exe` é o
  binário CLI cru renomeado por `release.yml:116`.
- **`scripts/build-installer.ps1` estava órfão** — funcionava conceitualmente
  mas nenhum workflow o invocava. Nenhum `.msi` saía desde a v0.2.1
  (2026-05-14), enquanto `ROADMAP.md:47` e `:84` afirmavam cobertura "Windows
  MSI" que a automação não sustentava.
- **As releases só tinham binários crus** — zero `.zip`, `.tar.gz`, `.msi`.

## Restrição estruturante

`crates/garraia-cli/src/update.rs:42-48` resolve o asset por **nome exato** e
`:127` exige o `<asset>.sha256` irmão. Substituir binário cru por archive
quebraria o `garra update` de **toda instalação já existente** no instante em
que ela pulasse para essa versão. Logo: tudo é **aditivo**.

Provado empiricamente antes do commit: rodando o `select_checksum_line` real do
`install.sh` contra um `SHA256SUMS` gerado pelo pipeline novo,
`garraia-linux-x86_64` continua selecionando a linha do binário cru e não a do
`.tar.gz`, e a verificação `sha256sum -c` passa.

## Entregue

### 1. `install.ps1` (699 linhas, raiz do repo)

Paridade função a função com o `install.sh`. Decisões que não são óbvias:

- **`param()`, não `$args`.** Sob `irm | iex` o script roda no escopo da sessão
  do usuário, onde `$args` referencia os argumentos *do chamador* — parsing
  manual seria ativamente incorreto. Um `param()` block recebe defaults sob
  `iex` e liga corretamente sob
  `& ([scriptblock]::Create((irm ...))) -SkipSetup`, que é o análogo PowerShell
  do `sh -s --`.
- **`throw`, não `exit`.** `exit` dentro de um scriptblock avaliado por `iex`
  termina o *host* — fecharia a janela do terminal do usuário em qualquer erro.
  O rodapé captura, imprime, e só converte em exit code quando
  `$MyInvocation.MyCommand.Path` indica execução como arquivo de script.
- **Sem `Set-StrictMode`/`$ErrorActionPreference` em escopo de arquivo.** Sob
  `iex` isso reconfiguraria silenciosamente a sessão do usuário e sobreviveria à
  instalação. Ficam dentro de `Invoke-Main`, onde o escopo do PowerShell os
  reverte sozinho.
- **PATH escrito direto no registro** (`HKCU\Environment`, `REG_EXPAND_SZ`).
  `[Environment]::SetEnvironmentVariable(...,'User')` no .NET Framework — que é
  o que o Windows PowerShell 5.1 usa — reescreve o valor como `REG_SZ`,
  convertendo permanentemente um PATH `REG_EXPAND_SZ` em literal e quebrando
  toda entrada `%USERPROFILE%` já existente.
- **Duas escritas de PATH**, registro + `$env:Path`. Sem a segunda, `garraia`
  não é encontrado no próprio terminal que rodou o instalador.
- **Alvo Windows PowerShell 5.1**, não só pwsh 7: TLS 1.2 forçado,
  `-UseBasicParsing`, `$ProgressPreference='SilentlyContinue'` (o renderizador
  de progresso do 5.1 torna um download de 45 MB ~10× mais lento), retry
  manual.
- **Resolução de versão pelo redirect** de `/releases/latest`, com a API REST
  como fallback — mesma estratégia anti-rate-limit do `install.sh:209-213`.
- **ARM64 avisa e prossegue** com o binário x86_64 (o Windows 11 ARM emula x64),
  em vez de bloquear uma instalação que funcionaria.

### 2. `tests/install_ps1/` — 84 asserções, 5 suítes

Harnesses escritos à mão, **não Pester**: `tests/install_sh/` já usa esse
padrão, e assim o job de CI não depende do PSGallery. Cobrem precedência de
flags/env, os três formatos de `SHA256SUMS` (text/binary/CRLF), a ancoragem
anti-prefixo, a escada de resolução de versão (com `Invoke-GhRequest`
sobrescrito, suíte 100% offline), a guarda de path de sistema e os ramos do
bootstrap.

**Dois bugs reais foram encontrados pelas suítes durante o desenvolvimento:**
1. `(... | Where-Object {...}).Count` estourava sob `Set-StrictMode` quando o
   filtro não casava nada — e "não casa nada" é o **caso comum** (o diretório
   ainda não está no PATH). Corrigido com `@(...)`.
2. A guarda de path de sistema comparava com `\` hardcoded.

### 3. Job `installer-powershell` no `ci.yml`

Matriz `ubuntu-latest`/pwsh 7 + `windows-latest`/PowerShell 5.1. As duas pernas
se justificam: a do Linux é o gate rápido; a do Windows é o único lugar onde os
riscos do 5.1 aparecem, e onde a guarda de path e a escrita de PATH são
observáveis (no Linux essas asserções reportam SKIP). PSScriptAnalyzer roda como
gate bloqueante — qualquer finding falha o job.

### 4. Archives aditivos no `release.yml`

`.tar.gz` (Linux/macOS) e `.zip` (Windows), cada um com o binário renomeado para
`garraia`/`garraia.exe` + `LICENSE` + `README.md`, num único diretório de topo.
Staging em `$RUNNER_TEMP`, **nunca** dentro de `release/`: o loop de checksums é
`for f in *` sob `set -euo pipefail` e `sha256sum` num diretório sai não-zero,
o que derrubaria o job inteiro.

### 5. Instaladores desktop MSI + NSIS

Job `build-windows-installer` best-effort — no `needs:` do job `release` mas
**fora** da condição `if:`, mesmo padrão do `build-linux-arm64`. Zero
`continue-on-error` (proibido pelo CLAUDE.md).

**Bug crítico corrigido junto:** `src/gateway.rs:14` chama
`.sidecar("garraia")` mas o `tauri.conf.json` declarava
`externalBin: ["binaries/garra"]`. O Tauri resolve `sidecar(nome)` contra os
basenames do `externalBin`, então a busca falhava e o app instalava mas
**nunca subia o gateway** (`gateway.rs:24`: "gateway sidecar not found").
Reviver o MSI sem isso teria publicado um instalador cuja função principal
falha em silêncio. Corrigido renomeando a config, não o Rust.

Novo `.github/workflows/desktop.yml` roda o mesmo build em PRs que tocam o
desktop — a primeira vez que esse crate é construído em CI. Inclui uma asserção
que compara o basename do `externalBin` com o `sidecar(...)` do `gateway.rs`;
verificado que ela falharia contra o estado anterior.

### 6. Drift de versão

`src-tauri/Cargo.toml` → `version.workspace` + `rust-version.workspace`; campo
`version` removido do `tauri.conf.json` (o Tauri v2 cai para a versão do Cargo,
que agora herda da workspace). Uma fonte de verdade, sem step de sincronização.

## Decisão revista durante a implementação: o updater do Tauri

O plano original recomendava **neutralizar** o bloco `updater` morto. Revertido
para **manter e documentar**, por dois fatos apurados ao ler o código:

1. A falha é **visível, não silenciosa**: `commands.rs:102-112` retorna `Err` e
   `tray.rs:158-166` imprime o erro. Nada se auto-atualiza em background. A
   premissa do plano ("falha silenciosa") estava errada.
2. Remover exigiria editar `lib.rs:25`, `commands.rs`, `tray.rs` e
   `capabilities/default.json` — quatro pontos de fiação em Rust, num crate que
   o CI nunca havia compilado, empilhando duas mudanças não verificadas na
   entrega que liga esse CI pela primeira vez.

Receita de reativação registrada em `docs/releasing.md`. Depende de secrets que
só o mantenedor pode criar.

## Verificação executada

| O quê | Como | Resultado |
|---|---|---|
| Sintaxe do PowerShell | `[Parser]::ParseFile` em ambos os `.ps1` | limpo |
| Lint | PSScriptAnalyzer 1.22 com o settings file | limpo |
| Suítes de teste | 5 suítes, pwsh 7.4.6 local | 84 passed, 0 failed, 2 skipped |
| Empacotamento | step `Package archives` real, extraído do YAML e executado | 5 archives, shape correto |
| Extração | `tar -xzf` + executar o binário | `garraia 0.3.4` |
| Loop de checksums | step `Generate checksums` real sobre os assets novos | exit 0, 12 `.sha256` |
| **Invariante aditiva** | `select_checksum_line` real do `install.sh` sobre o `SHA256SUMS` novo | binário cru selecionado, `sha256sum -c` OK |
| Asserção de sidecar | contra o estado anterior via `git show HEAD:` | falharia, como pretendido |
| YAML | `yaml.safe_load` nos 3 workflows | válidos |
| `continue-on-error` | grep por diretivas reais | **nenhuma** no repo |

## Fora de escopo / pendente do mantenedor

- **Publicar `install.ps1` em `garraia.org/install.ps1`.** O site é hospedado
  fora deste repositório. Até lá o espelho do release CDN funciona a partir da
  primeira release — por isso a documentação lista os dois desde já.
- **Secrets de assinatura do updater** (`TAURI_SIGNING_PRIVATE_KEY` etc.).
- **Certificado de code signing** para eliminar o aviso do SmartScreen.
- **Smoke test numa máquina Windows real** — nada em CI substitui isso para a
  persistência de PATH no registro.
- DMG notarizado, AppImage, `.deb`, `.rpm`; Windows ARM64 nativo.
