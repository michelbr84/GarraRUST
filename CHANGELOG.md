# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- **Instalador de uma linha para Windows.** Novo `install.ps1`, irmão do
  `install.sh` e em paridade de comportamento com ele:
  `irm https://garraia.org/install.ps1 | iex` detecta a plataforma, resolve a
  release, verifica o SHA-256 contra o `SHA256SUMS`, instala `garraia.exe` em
  `%LOCALAPPDATA%\Programs\GarraIA`, registra no PATH do usuário e encadeia
  `init` + `start` — sem exigir administrador. Antes, a instrução para Windows
  era "baixe o binário na página de releases", sem PATH nem verificação.
  Flags via `& ([scriptblock]::Create((irm ...))) -SkipSetup`, já que
  `irm | iex` não recebe argumentos; toda flag tem uma `GARRAIA_*`
  equivalente, e a env var setada pelo chamador vence a flag.
- **Archives em todas as plataformas.** Cada release passa a trazer
  `.tar.gz` (Linux/macOS) e `.zip` (Windows) contendo o binário com o nome
  simples `garraia`/`garraia.exe` mais `LICENSE` e `README.md`. **Aditivo**:
  os binários crus continuam publicados sem alteração, porque o
  `garra update` resolve assets por nome exato.
- **Instaladores desktop do Windows (MSI + NSIS) de volta ao pipeline.**
  Nenhum `.msi` era publicado desde a v0.2.1 — `scripts/build-installer.ps1`
  existia mas nenhum workflow o invocava. Agora há o job best-effort
  `build-windows-installer` no `release.yml` e o `desktop.yml`, que constrói o
  crate Tauri nos PRs que o tocam (a primeira cobertura de CI que esse crate
  já teve). Os instaladores **não são assinados** — o SmartScreen avisa.
- **Suítes de teste do `install.ps1`** em `tests/install_ps1/` (84 asserções)
  e o job `installer-powershell` no CI, com PSScriptAnalyzer bloqueante e
  matriz PowerShell 7 (Linux) + Windows PowerShell 5.1.

### Fixed
- **Sidecar do Garra Desktop nunca era encontrado.** `src/gateway.rs:14` chama
  `.sidecar("garraia")` enquanto o `tauri.conf.json` declarava
  `externalBin: ["binaries/garra"]`. Como o Tauri resolve o sidecar pelo
  basename do `externalBin`, o app instalava e **nunca subia o gateway**,
  registrando apenas "gateway sidecar not found" no stderr. O `desktop.yml`
  passa a asseverar que os dois nomes coincidem.
- **`scripts/build-installer.ps1` podia passar sem produzir nada.** Usava
  `-ErrorAction SilentlyContinue` e só imprimia quando encontrava o bundle, de
  modo que um `cargo tauri build` que saísse 0 sem emitir MSI deixava o script
  verde. Agora falha alto.
- **Drift de versão do crate desktop.** `tauri.conf.json` e
  `src-tauri/Cargo.toml` estavam presos em `0.3.0` com a workspace em `0.3.4`.
  O Cargo passa a herdar (`version.workspace`) e o campo `version` do JSON foi
  removido, já que o Tauri v2 cai para a versão do crate.
- **`GARRAIA_BOOTSTRAP_LOCAL` documentado ao contrário no wiki.**
  `wiki/Instalacao-e-Primeiros-Passos.md` dizia `=1` "usa artefatos locais em
  vez de baixar"; o valor é `=0` e ele suprime os prompts de GPU/Ollama do
  wizard (`install.sh:35-38`).
- **`ROADMAP.md` afirmava cobertura "Windows MSI"** em duas linhas (`:47`,
  `:84`) que a automação não sustentava. Reconciliado agora que sustenta.

- **`garraia --model <tag>` abre o chat direto no modelo local.** Rodar o
  binário só com flags (sem subcomando) agora entra no REPL — antes o clap
  respondia *unexpected argument* porque o subcomando `chat` só era injetado
  quando o argv estava totalmente vazio. A tag é normalizada (`qwen3.8` →
  `qwen3.8:latest`) e procurada no daemon Ollama local via `GET /api/tags`;
  em acerto exato o provider Ollama é escolhido, mesmo que o `config.yml`
  aponte para outro default. A sonda tem timeout de 2 s e só vence com acerto
  exato, então `--model gpt-4o` nunca é sequestrado para um provider local.
- **Download do modelo faltante.** Com terminal, o GarraIA pergunta antes de
  baixar; com `-y`/`--yes` (novo em `chat` e `ask`) baixa direto; sem
  terminal nunca pergunta — imprime `ollama pull <tag>` no stderr e segue.
  Usa `POST /api/pull` do daemon (não `ollama pull`), então funciona sem o
  binário no `PATH` e respeita `OLLAMA_BASE_URL` remoto. O progresso vai
  para o stderr, preservando o stdout de uma linha do `ask --json`.
- **Seletor de modelo no `garraia init`.** O wizard passou de um `Confirm`
  fixo no Qwen3-14B para uma lista (Qwen 3.8 27B, Qwen 3 8B, Qwen3-14B GGUF,
  Llama 3.1) mais a opção de **digitar qualquer tag do Ollama**, incluindo
  referências de registry (`hf.co/user/repo:Q4_K_M`).
- **`garraia config set-model`** — configuração headless: grava uma entrada
  no `llm:` e a torna `agent.default_provider`, sem prompt algum. O resto do
  `config.yml` fica intacto e o default anterior é rebaixado para
  `fallback_providers` em vez de descartado. Arquivo gravado com modo `0600`.
- **Flags no `install.sh`** — `--skip-setup`, `--skip-init`, `--skip-start`,
  `--no-local`, `--version <tag>`, `--install-dir <dir>`, `--help`. `curl …
  | sh` não consegue passar variáveis de ambiente para o shell do pipe;
  `curl … | sh -s -- --skip-setup` consegue. Uma env var já definida pelo
  chamador ganha da flag correspondente. Nova suíte
  `tests/install_sh/parse_args.sh` (18 casos) ligada ao CI.
- Nova página `docs/integrations/ollama-launch.md` cobrindo o modelo padrão,
  a resolução do `--model` e o que falta para `ollama launch garraia`.
- **`contrib/ollama-launch/`** — a integração Go do `ollama launch`, pronta
  para virar PR no `ollama/ollama`: `garraia.go` (implementa `Runner` +
  `ManagedSingleModel`, espelhando `cmd/launch/hermes.go`), `garraia_test.go`
  (10 testes) e o patch do `registry.go`. Escrita contra o `ollama/ollama`
  real e validada lá dentro — `gofmt` e `go vet` limpos, `go build ./...` ok
  e a suíte `./cmd/launch/` **inteira** verde. Não é código de runtime deste
  repositório: o registro de integrações do Ollama é uma slice Go compilada
  dentro do binário, sem manifesto nem plugin, então a integração só passa a
  existir quando o PR upstream for aceito.
- Teste `test_lopdf_roundtrip_smoke` no `garraia-media` — os 4 testes reais de PDF
  estão `#[ignore]`d desde abril, então até agora o `cargo test` só provava que o
  crate compila contra o lopdf. O novo teste escreve um PDF de uma página com o
  writer do lopdf e o lê de volta por `extract_text_from_bytes`, exercendo
  writer → reader → xref → content stream → extração. Verde na 0.42 e na 0.44.

### Changed
- **`rmcp` 1.7 → 2.2**, alinhando os tipos de modelo à spec MCP 2025-11-25
  (upstream `rust-sdk#927`). O SDK dissolveu duas camadas: `Content`
  (`= Annotated<RawContent>`) e o próprio `RawContent` viraram o enum achatado
  `ContentBlock`, e `PromptMessageRole`/`PromptMessageContent` viraram
  `Role`/`ContentBlock`. Migrados os três call sites — a ponte de tools MCP, o
  formatador de prompts e o servidor `garra mcp-server`. O **formato de wire não
  mudou**: o handshake real continua emitindo `{"type":"text","text":…}`, e o
  transcript de evidência em `docs/integrations/hermes-mcp.md` foi regravado a
  partir de uma execução de verdade contra a 2.2.0. Detalhes em `plans/0358`.
- `sse-stream` 0.2.3 → 0.2.5 no lock. O rmcp 2.2.0 declara `sse-stream = "0.2"`
  mas usa `SseStream::from_bytes_stream`, que só existe a partir da 0.2.4 —
  under-specification upstream que quebrava o build com `--features mcp-http`.
- **Modelo Ollama padrão passa a ser `qwen3.8:latest`** (resolve para
  `qwen3.8:27b` — Q4_K_M, ~18 GB, 262 144 tokens de contexto, visão +
  tools), no lugar de `llama3.1`. Atualizado no provider, na CLI, no wizard,
  nos configs de exemplo e na documentação.
- **`detect_provider` recebe o `--model`.** Todos os ramos da cadeia
  (Ollama, Anthropic, OpenAI, OpenRouter, fallback offline e o caminho
  `--url`) passaram a resolver o modelo por `resolve_provider_model`, então
  `detect_provider` e `select_explicit_provider` concordam. Efeito colateral
  intencional: os ramos de nuvem agora também varrem `config.llm[*]` por
  `provider == kind`, e não só `config.llm[<kind>]`.
- A tabela de modelos padrão vive num único lugar
  (`chat::hardcoded_default_model`); as 11 cópias inline dos literais foram
  removidas.
- **lopdf 0.42 → 0.44 com a feature `time` desligada** — a partir da 0.43 o
  `time_impl` do lopdf chama `BorrowedFormatItem::StringLiteral` com um padrão
  estilo strftime; a variante só existe no `time` >= 0.3.49 (fixamos 0.3.47), então
  o build morria com `error[E0599]` em 7 jobs de CI. Upstream J-F-Liu/lopdf#518,
  corrigido no master em `1efa2702` mas ainda sem release. O módulo é inteiramente
  `#[cfg(feature = "time")]` e o `garraia-media` não faz nenhuma interop de data/hora
  com o lopdf — `CreationDate`/`ModDate` saem como bytes crus — então
  `default-features = false` + `features = ["chrono", "jiff", "rayon"]` destrava o
  bump sem mudança de comportamento e sem alterar código de produção.

### Fixed
- **`--model` sem `--provider` não trocava o provider.** `run_chat` só
  substituía a string exibida no banner — o `Arc<dyn LlmProvider>` continuava
  sendo o que o autodetect construiu, com o modelo interno obsoleto. Valia
  para `garraia chat` e `garraia ask`.
- `/model <nome>` no REPL agora normaliza a tag quando o provider é Ollama e
  avisa quando o nome não aparece em `/models`.

### Security
- **RUSTSEC-2026-0192 fechado estruturalmente** — na 0.44 o `ttf-parser` passou a ser
  opcional atrás da feature `font_embedding`, que não usamos (nem `FontData` nem
  `add_font`). O `ttf-parser 0.25.1` (não mantido, sem upgrade seguro) saiu do
  `Cargo.lock` e o ignore correspondente foi removido do `deny.toml`.

## [0.3.4] - 2026-08-29

### Fixed
- **Binário Linux x86_64 roda em Ubuntu 22.04+ de novo** — o job
  `build-linux-x86_64` usava `runs-on: ubuntu-latest`, que passou a mapear
  para Ubuntu 24.04 (glibc 2.39); os binários da v0.3.2 **e da v0.3.3**
  abortavam com ``version `GLIBC_2.39' not found`` em qualquer sistema com
  glibc mais antiga (ex.: containers Ubuntu 22.04, Debian 12). O runner
  agora é pinado em `ubuntu-22.04`, estabelecendo a baseline suportada:
  **glibc ≥ 2.35** (Ubuntu 22.04+, Debian 12+).
- **`install.sh` checa a glibc antes de baixar** — novo preflight
  `check_glibc` (`MIN_GLIBC=2.35`) falha cedo com mensagem acionável
  (atualizar a distro ou `cargo install --git`) em vez do erro críptico do
  loader depois da instalação. Detecta musl (Alpine) e aponta para build
  from source. Coberto por `tests/install_sh/check_glibc.sh`, registrado no
  job `installer-shellcheck` do `ci.yml` (sem isso a suíte nunca rodaria: o
  job lista cada arquivo de teste explicitamente).

### Changed
- **OpenSSL agora é vendored/estático** (`native-tls/vendored` via
  garraia-channels, feature discord) — o binário de release não linka mais
  `libssl.so.x` do sistema, e o `pre-build` do `Cross.toml` deixou de
  instalar `libssl-dev:$CROSS_DEB_ARCH` (receita canônica do FAQ do cross).
  Em contrapartida o `openssl-src` compila o OpenSSL do zero, o que exige
  `perl` + `make` no ambiente de build: adicionados ao `Cross.toml` e ao
  `Dockerfile` (que roda em `rust:1.98-slim`, sem nenhum dos dois). O
  `security-gate-bola.yml` — único job que restaura `target/` do cache —
  ganhou o mesmo passo de liberação de disco que o job `coverage` do
  `ci.yml` já usava: com o OpenSSL dentro de `target/`, o cache restaurado
  estourava os ~14 GB do runner e o matava sem logs. O
  `native-tls` só usa OpenSSL em Linux — Windows (schannel) e macOS
  (Security.framework) não são afetados. Nota: migrar o serenity para
  `rustls_backend` está bloqueado — o 0.12.5 mapeia a feature para
  `reqwest/rustls-tls`, removida no reqwest 0.13, que o tauri ^0.13 prende
  no lock; reavaliar quando o serenity suportar reqwest 0.13.

## [0.3.3] - 2026-08-27

### Added
- **Wiki versionado e publicado automaticamente** (#855) — a fonte de verdade do
  GitHub Wiki agora vive em `wiki/` neste repo (Home + 7 páginas: Instalação,
  Referência da CLI, Configuração, Guias de Integração, Arquitetura+ADRs,
  Segurança+Operação, Contribuir/Roadmap/FAQ), revisável por PR; o workflow
  `wiki-sync.yml` publica no `GarraRUST.wiki` via `GITHUB_TOKEN` a cada push
  no `main`.

### Fixed
- **Clippy do stable 1.98 destravado** (#854) — o lint novo
  `clippy::chunks_exact_to_as_chunks` derrubava o job Clippy no `main` e em
  todos os PRs do dependabot. Migrados os 3 usos de `chunks_exact(4)` para
  `as_chunks::<4>()` (garraia-db ×2, garraia-embeddings ×1 — este último
  eliminando um `try_into().expect()`), mais `allow` pontual documentado de
  `result_large_err` no padrão axum de `issue_token_pair`.

### Changed
- Imagem base Docker atualizada para `rust:1.98-slim` (#849).
- `aws-smithy-runtime-api` 1.14.0 → 1.15.0 (#850).
- Documentação (README/ROADMAP/TODO) sincronizada com o cleanup de 2026-08-18 (#847).

## [0.3.2] - 2026-08-18

### Fixed
- **Binário Linux ARM64 de verdade desta vez** — dois defeitos empilhados:
  (1) o cross 0.2.5 de crates.io usa imagem base Ubuntu 16.04, cujo archive só
  tem OpenSSL 1.0.2 (abaixo do mínimo ≥ 1.1.0 do openssl-sys) — o release.yml
  agora instala o cross da git (imagens modernas); (2) o proc-macro do sqlx
  compilava openssl-sys para o HOST dentro do container e quebrava o
  cross-compile — resolvido movendo o sqlx para rustls (abaixo). Validado
  localmente: `cross build --target aarch64-unknown-linux-gnu` produz o ELF
  aarch64. v0.3.1 saiu com 4 binários (sem regressão vs v0.3.0); esta
  adiciona o quinto.

### Changed
- **sqlx: native-tls → rustls (`tls-rustls-ring-native-roots`)** — mantém as
  CAs do sistema para TLS com Postgres remoto. O sqlx-macros era o único
  consumidor de OpenSSL no lado host dos builds; reqwest/tungstenite seguem
  em native-tls (OpenSSL só no target, via `Cross.toml`). Suites de
  integração do garraia-auth verdes contra pgvector/pg16 real.

## [0.3.1] - 2026-08-18

### Fixed
- **`POST /v1/me/anonymize` funciona pela primeira vez** (plan 0354, PR #843) —
  o endpoint LGPD/GDPR retornava 500 em toda chamada: o código atualizava uma
  coluna `user_identities.login` que não existe neste schema. A anonimização
  agora cobre os três lugares onde o email vive — `user_identities.provider_sub`
  (chave de login), `users.email` e `group_invites.invited_email` — com token
  determinístico `anon-<uuid-32-hex>@garraanon.local` (UUID completo: o prefixo
  de 8 hex colidia sob UUIDv7). Revisado por security-auditor, sem blockers.
- **Release Linux ARM64 volta ao ar** (PR #842) — o build `cross` aarch64
  falhava por falta de OpenSSL do target e a v0.3.0 saiu sem o binário
  linux-aarch64. `Cross.toml` novo instala `libssl-dev:$CROSS_DEB_ARCH` no
  container de build; esta é a primeira release com os 5 binários.
- **h2 0.4.16** (PR #842) — RUSTSEC-2026-0258.
- **Vault-passphrase casing trap eliminated** (issue #824) — the two
  near-identical env vars `GARRAIA_VAULT_PASSPHRASE` (credential vault) and
  `GarraIA_VAULT_PASSPHRASE` (legacy JWT-secret fallback) no longer fail
  silently when the operator picks the "wrong" one. Every consumer now accepts
  both spellings: the credential vault (`garraia-security`) falls back to the
  mixed-case alias with a deprecation warning at boot, and
  `AuthConfig::from_env` accepts the all-caps spelling as its last JWT-secret
  fallback. Precedence is backwards-compatible — `GARRAIA_JWT_SECRET` >
  `GarraIA_VAULT_PASSPHRASE` > `GARRAIA_VAULT_PASSPHRASE` for auth, and
  all-caps > mixed-case for the vault — so deploys that set both spellings
  with different values keep their exact pre-fix behavior. `garraia config
  check` gains two warnings: a deprecation notice whenever the mixed-case
  spelling is set, and a split-values alert when both spellings are set with
  different values (presence/equality only — values are never emitted).

### Changed
- **Dependências consolidadas** (PR #844, cleanup 2026-08-18): serial_test
  4.0.1, base64 0.23, jsonwebtoken 11.0.0, validator 0.21, itertools 0.15,
  uuid 1.24.1, tauri 2.11.5 e **wasmtime + wasmtime-wasi 47.0.3** — o par
  agora também viaja junto no dependabot via grupo dedicado (PR #842).

### CI
- Jobs `e2e`/`playwright` ganham `timeout-minutes` (PR #842) — dois runs de 6h
  em 2026-08-17, travados no download do Chromium, motivaram o teto.
- Job novo `auth-integration` (PR #843): os 16 binários de integração do
  garraia-auth (matriz RLS incluída) agora rodam em todo PR — antes eram
  pulados silenciosamente pelo `cargo test --workspace` e só o cargo-mutants
  semanal os executava.

## [0.3.0] - 2026-08-16

### Onboarding: `install.sh` → `garraia init` → `garraia start` now actually works

The path the website documents was broken by construction on any fresh machine.
`garraia init` offered "Store in encrypted vault (**recommended**)" as the
default, encrypted the API key into `credentials/vault.json`, and left
`llm.<name>.api_key` as `null`. `install.sh` then ran `exec garraia start` in the
same shell, with no `GARRAIA_VAULT_PASSPHRASE` — so `try_vault_get` could not
open the vault, no key resolved, and the gateway came up with
`0 active / 1 configured` and `skipping openrouter provider main: no API key`.
The key was on disk, encrypted, and unreadable by the server that needed it.

#### Fixed
- **Wizard defaults to `config.yml`** (`wizard/mod.rs`) — the storage prompt now
  offers config first and defaults to it; the vault option's label states that
  it requires `GARRAIA_VAULT_PASSPHRASE` on *every* start instead of calling
  itself "recommended". When the vault is chosen, the passphrase reminder is
  printed as a block instead of a single line that scrolled away unseen. The
  Telegram bot token had the identical defect and got the identical fix.
- **`garraia init` can repair a broken config** (`wizard/config_writer.rs`) —
  `merge_update` was additive-only, so re-running the wizard over a config that
  already had a keyless `llm.main` *added* `llm.openrouter` and left `main`
  broken. A freshly-supplied OpenRouter key is now backfilled into pre-existing
  `openrouter` entries that have no key. A key the operator already set is never
  overwritten, and the local-Ollama placeholder is never leaked into a real
  `openai` entry.
- **`.env` was loaded after the providers were built** (`server.rs`,
  `bootstrap/channels.rs`) — the only `dotenvy::dotenv()` in the gateway ran
  inside `build_channels`, ~20 lines *after* `build_agent_runtime` had already
  read every provider's API-key env var. Anything embedding `Server` directly got
  working channels and dead providers. The load moved to the top of
  `Server::run`.
- **`POST /api/providers` silently discarded persistence failures**
  (`router.rs`) — the response was `201 {"status":"ok"}` even when
  `try_vault_set` no-opped for lack of a passphrase, so a provider added through
  the web console worked until the next restart and then vanished. The handler
  now reports `"persisted": bool`, says so in the message, and logs a WARN with
  the remedy.

#### Changed
- **`config.yml` is written with mode `0600`** (`garraia_config::harden_secret_file`)
  — it now carries `llm.*.api_key` by default, and `std::fs::write` alone left it
  at the umask default (commonly `0644`). Applied by `ConfigLoader::save` and by
  all three wizard write strategies.
- **One source of truth for API-key resolution**
  (`garraia_config::provider_keys`) — the question "does this provider have a
  usable key?" had three different answers: the boot path walked
  vault → config → env, `/health` checked config `||` env and ignored the vault,
  and the admin providers list reported `has_secret` from an AES-GCM SQLite store
  the boot path never reads. All three now share `resolve_api_key_source` and the
  `provider_key_env` table, which also replaced fifteen hardcoded
  `("X_API_KEY", "X_API_KEY")` pairs in `build_agent_runtime` and fourteen more in
  the provider-activation handler. `/api/providers` gained `key_source` and
  `has_admin_stored_secret` so the store's own state stays visible.
- **An empty provider env var now counts as absent.** `OPENROUTER_API_KEY=""`
  previously registered a provider with an empty credential that failed on the
  first call with an opaque upstream 401; it now reports the actionable "no API
  key" warning, consistent with the config tier which already ignored empty
  strings.
- **The "no API key" warning names all three remedies** — config, env var, *and*
  unlocking the vault. It previously omitted the vault, which is precisely where
  the wizard had put the key.
- **The startup banner stops overstating readiness** (`banner.rs`) — it printed
  the configured provider name unconditionally, directly above a log line saying
  that provider had been skipped. It now marks the state
  (`main ⚠ no API key`) and adds a `File` row naming the config file actually in
  force, since `ConfigLoader::load` prefers `config.yml` and silently ignores
  `config.toml` when both exist.
- Workspace `version = "0.3.0"` (`Cargo.toml`,
  `crates/garraia-desktop/src-tauri/Cargo.toml`, `tauri.conf.json`). The previous
  release left `main` at `0.2.1`, so a binary built from `main` announced itself
  as the released `v0.2.1` and the two were indistinguishable.

#### Added
- **`garraia config check` validates `llm:`** (`garraia-config/src/check.rs`) —
  it had no equivalent of the channel token warning, so the failure that took the
  whole gateway down was the one thing it would not report. It now emits an
  **Error** for any provider whose key resolves nowhere (consulting the vault, so
  it cannot disagree with the boot path), an Error for an unrecognized `provider`
  type, and a Warning when `llm:` is populated but `agent.default_provider` is
  unset — which silently disables provider auto-fallback.

#### Documentation
- `docs/installation.md` told operators to edit `~/.garraia/config.yml` while
  `ConfigLoader::default_config_dir` prefers `~/.config/garraia` whenever it
  exists, so readers were editing a file the gateway never read. It now documents
  the real resolution order, the `config.yml` over `config.toml` precedence, the
  vault passphrase requirement, and `GARRAIA_CONFIG_DIR` as the supported way to
  consolidate everything under one directory.
- `README.md` claimed `install.sh` verifies each binary against a per-asset
  `<asset>.sha256`; it verifies against the aggregate `SHA256SUMS` (the per-asset
  form is what `garraia update` uses). The `llm:` example now carries the vault
  passphrase caveat.

## [0.2.1] - 2026-05-14

### Auto-update pipeline — fixes 404 on `garraia update`

#### Fixed
- **`/releases/latest` 404** — Every prior tag (`v0.1.0-beta`, `v0.1.0-beta.1`, `v0.2.0-beta`) shipped as a prerelease, so the GitHub endpoint that `garraia update` calls (`GET /repos/{owner}/{repo}/releases/latest`) returned 404. `v0.2.1` is the first **non-prerelease** tag — the workflow auto-flips `prerelease: true` only when the tag contains `alpha`/`beta`/`rc`. From now on, installed `0.2.0` binaries find an updatable release.
- **Asset-name mismatch (`arm64` ↔ `aarch64`)** — `crates/garraia-cli/src/update.rs:43-50` selects assets by Rust's `std::env::consts::ARCH`, which on Apple Silicon and Linux ARMv8 is `aarch64`. The release workflow named those binaries `garraia-linux-arm64` / `garraia-macos-arm64`, so even if a non-prerelease existed the updater would have bailed with "release has no asset for this platform". Renamed to `garraia-linux-aarch64` / `garraia-macos-aarch64`.
- **Missing per-asset `.sha256` files** — `update.rs:127` reads `<asset>.sha256` siblings for tamper-detection. The previous workflow only emitted a single aggregate `SHA256SUMS`. The "Generate checksums" step now emits both: aggregate `SHA256SUMS` (kept for `install.sh` + human verification) **and** one `<asset>.sha256` per binary, gathered into the release via `release/*.sha256` glob.

#### Changed
- Workspace `version = "0.2.1"` (Cargo.toml, `crates/garraia-desktop/src-tauri/Cargo.toml`, `tauri.conf.json`).
- Prerelease gate widened to also detect `rc` in the tag string (was `alpha|beta`).

## [0.1.12] - 2026-02-27

### Fase 5: Delivery and Ecosystem

#### Added
- **README overhaul** - Updated architecture documentation with 14-crate workspace, runtime flow diagrams, voice pipeline (STT→LLM→TTS), multi-agent architecture, and MCP support
- **GitHub Actions release workflow** - Multi-platform binary builds for Linux (x86_64, ARM64), Windows (x86_64), macOS (x86_64, ARM64)
- **Website structure** - Initial documentation site with technical documentation, architecture overview, and integration guides

### Fase 4: Advanced Integrations

#### Added
- **Admin Console** - Full-featured web admin panel with user management, RBAC, audit logs, and billing
- **A2A Protocol** - Agent-to-agent communication with agent cards (`/.well-known/agent.json`) and task endpoints
- **Multi-agent routing** - Named agent registry with priority-based routing and session continuity
- **Media processing** - PDF extraction and image processing capabilities
- **Runtime state machine** - Executor with state management, meta-controller, and turn-based execution
- **Voice E2E pipeline** - Complete STT→LLM→TTS voice pipeline with Whisper, Chatterbox, and Hibiki support
- **Stateful commands** - Command registry with state management and persistent command state

#### Fixed
- Various stability improvements and bug fixes across all crates

### Fase 3: Runtime Integration & Voice E2E

#### Added
- **garraia-runtime crate** - State machine executor with IDLE→RUNNING→DONE transitions
- **Meta controller** - Execution budget management, max turns, retry with exponential backoff
- **Turn execution** - Complete message receive → tool execute → stream response flow
- **Voice pipeline E2E** - Full end-to-end voice processing from audio input to TTS output
- **Whisper STT** - Local and API-based speech-to-text
- **Chatterbox TTS** - GPU-accelerated multilingual text-to-speech
- **Hibiki TTS** - Additional GPU TTS option
- **Audio conversion** - FFmpeg-based audio format conversion

#### Changed
- Improved voice mode activation and health checks

### Fase 2: Stateful Commands

#### Added
- **Command registry** - Dynamic command registration with stateful support
- **Built-in commands** - /help, /clear, /model, /pair, /users, /voz, /health, /providers, /stats, /config, /mcp
- **Channel command integration** - Unified command system across Telegram, Discord, Slack, WhatsApp
- **Command aliases** - Multi-language aliases (e.g., /voz and /voice)
- **Command state** - Persistent command state across sessions

### Fase 1: Stabilization Fixes

#### Fixed
- Daemon mode stability and PID management
- Hot-reload configuration issues
- Memory leaks in long-running sessions
- WebSocket connection handling
- Health check timeouts

#### Changed
- Improved error handling and logging
- Optimized memory usage
- Better error messages for debugging

## [0.1.11] - 2026-02-23

### Fixed
- Fix daemon mode panic and wire live MCP panel in webchat

### Changed
- Update docs: add all 14 LLM providers, MCP page, and tools page

## [0.1.10] - 2026-02-22

### Added
- Add 8 OpenAI-compatible LLM providers (Gemini, Falcon, Jais, Qwen, Yi, Cohere, MiniMax, Moonshot)

### Fixed
- Fix Windows build: remove unix gate on anyhow::Context import

## [0.1.9] - 2026-02-21

### Added
- Add DeepSeek and Mistral providers

### Fixed
- Fix Ollama Docker port binding

## [0.1.8] - 2026-02-20

### Added
- Render markdown and tables in webchat responses
- Implement persistent 3-column webchat layout
- Add structural mockups for MCPs and Extensions views
- Enable hot-reloading for webchat.html during local dev

### Changed
- Add SECURITY.md, CODEOWNERS, and pin all GitHub Actions to commit SHAs

### Fixed
- Fix restart/stop when multiple PIDs on port
- Add Windows support for daemon management

## [0.1.7] - 2026-02-18

### Fixed
- Fix XSS in webchat and make update checksum mandatory

### Changed
- Add Discord invite link to README

## [0.1.6] - 2026-02-18

### Added
- Startup banner with Ferris logo and config summary
- Sandy theme and dark mode toggle for webchat
- Telegram group chat support, session mapping, and anonymous admin support
- Docker Compose deployment examples and .env.example
- mdBook documentation structure

### Fixed
- Fix restart/stop failing when no PID file exists
- Fix daemon stop logic on Windows to avoid unsafe PID termination
- Fix plugin path helper and Windows STILL_ACTIVE import

### Changed
- Improve native Windows compatibility and CLI path handling
- Refine webchat layout and sidebar hierarchy

## [0.1.5] - 2026-02-21

### Changed
- Feature-gate wasmtime/plugins behind opt-in `--features plugins` cargo feature
- Default release binary reduced from 22 MB to 16 MB (27% smaller)
- Removed unused `garraia-plugins` dependency from gateway crate

## [0.1.4] - 2026-02-21

### Added
- `garraia restart` command - gracefully stops daemon (if running) then starts a new one
- `try_stop_daemon()` helper that silently handles "no daemon running" case

### Changed
- Post-update message now suggests `garraia restart` instead of `garraia stop && garraia start`

## [0.1.3] - 2026-02-21

### Fixed
- Bumped workspace version to match release tags (was stuck at 0.1.0, causing false update notices)

## [0.1.2] - 2026-02-21

### Added
- `garraia update` command - downloads latest release from GitHub with SHA-256 checksum verification, atomic binary replacement, and backup
- `garraia rollback` command - restores the previous binary from `.old` backup
- Background version check with 24h cached TTL (`~/.garraia/update-check.json`)
- CLI update notice printed on every command when a newer version is available
- Webchat dismissible update banner when `/api/status` reports a newer version
- `version` and `latest_version` fields in `/api/status` response

## [0.1.1] - 2026-02-20

### Added
- Runtime LLM provider switching via webchat dropdown and REST API (`GET/POST /api/providers`)
- `AgentRuntime` interior mutability (`RwLock<Vec<Arc<dyn LlmProvider>>>`) for adding providers after startup
- `OpenAiProvider::with_name()` builder for OpenAI-compatible APIs with distinct provider IDs
- `try_vault_set()` for best-effort API key persistence at runtime
- WebSocket messages accept optional `provider` field for per-message provider routing
- Webchat sidebar with provider dropdown, API key input, and "Save & Activate" button

## [0.1.0] - 2026-02-20

### Added
- **A2A protocol** - Agent-to-agent communication with agent card (`/.well-known/agent.json`), task CRUD endpoints, and outbound `A2AClient` (#71)
- **Multi-agent routing** - Named agent configs, priority-based agent router, and REST session API (`POST /api/sessions`, `POST /api/sessions/:id/messages`, `GET /api/sessions/:id/history`) (#108)
- **MCP enhancements** - Resources, prompts, HTTP transport (`mcp-http` feature), auto-reconnect health monitor, `mcp resources` and `mcp prompts` CLI commands (#80)
- **Security hardening** - Shared log redaction crate, configurable HTTP rate limits, per-WebSocket sliding window throttle (30 msg/min) (#74)
- **Security documentation** - Architecture overview, vendor-neutral audit checklist, AI agent attack surfaces guide (#113)
- **Install script** - `curl -fsSL` one-liner with OS/arch detection, SHA-256 verification, smart install directory (#109)
- **Release matrix** - Linux aarch64 (via `cross`) and Windows x86_64 CI targets (#110)
- **Scheduling hardening** - Recursive self-scheduling guard, delay cap (30 days), per-session pending limit (5) (#107)
- **Built-in skills** - 6 starter skills: summarize, translate, code-review, explain, rewrite, brainstorm (#106)
- **README overhaul** - Competitive positioning, benchmark numbers, updated Quick Start (#103)
- **iMessage channel** - macOS-native iMessage adapter with group chats, attachments, reconnect backoff, deployment docs (#100, #101)
- **Sansa LLM provider** - Integration with Sansa AI (#98)
- **Security & sandbox fixes** - Path traversal prevention, SSRF blocking, WASM sandbox limits, test coverage (#97)
- **OpenClaw migration** - Migration tool for conversations and credentials from OpenClaw (TypeScript predecessor) (#103)
- **Discord channel** - Bot integration with streaming, slash command mapping, callback pipeline (#95)
- **Scheduling system** - Persistent task scheduling with heartbeat execution (#96)
- **WASM plugin sandbox** - Hot-reload registry, epoch deadlines, sandbox resource limits (#94)
- **Chat persistence** - Session hydration and history persistence across channels
- **WebSocket authentication** - API key auth for WebSocket handler with query param and header support
- **MCP client** - Model Context Protocol support with stdio transport, tool bridging, namespaced tools
- **Slack channel** - Socket Mode integration with markdown formatting
- **WhatsApp channel** - Webhook-based integration with verification endpoint
- **SKILL.md support** - Skill file parser, scanner, and installer
- **OpenAI streaming** - Streaming response support for OpenAI provider
- **Telegram channel** - Bot with allowlist, commands, typing indicator, streaming, markdown formatting, context window management
- **Ollama provider** - Local LLM support with tool calling
- **Agent orchestration** - Conversation loop with tool execution (max 10 iterations) and memory recall
- **Cohere embeddings** - Embedding provider for vector search in memory store
- **Memory store** - SQLite-backed memory with sqlite-vec for vector search, cross-channel continuity
- **Core providers** - Anthropic and OpenAI LLM providers with tool support
- **Core tools** - Bash, file read, file write, web fetch
- **Credential vault** - AES-256-GCM encrypted secret storage with PBKDF2-SHA256
- **CLI** - `garraia init` wizard, daemon mode, MCP/skill/channel/plugin commands
- **Gateway** - Axum-based WebSocket gateway with HTTP API, session management, config hot-reload
- **Security** - Allowlists, pairing codes, prompt injection detection (14 patterns), input validation
- **CI/CD** - GitHub Actions: check, test, clippy, fmt, cargo-deny, release pipeline

### Changed
- Repository moved to `garraia-org` organization
- Config loading follows XDG standards with backward compatibility
- `cargo-deny` migrated to v2 config format
- `garraia.dev` references updated to `garraia.org`

### Fixed
- Dangling symlink vulnerability in media processor
- Insecure file operation in `MediaProcessor`
- Clippy warnings and formatting across workspace

### Security
- Path traversal prevention in WASM plugin sandbox (post-canonicalize boundary check)
- SSRF blocking (private IP range rejection in plugins)
- Log redaction for API keys (Anthropic, OpenAI, Slack tokens)
- Rate limiting on HTTP and WebSocket endpoints
- Prompt injection detection with 14 pattern categories
