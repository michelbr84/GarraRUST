# Plan 0126 — `curl | sh` auto-bootstrap **PR-A**: `garraia init` foundation

> **For agentic workers:** implement task-by-task following the checkbox steps in §M1.

**Linear issue:** TBD — to be created when opening the PR ("Installer auto-bootstrap PR-A: wizard env detection + GPU/local-stack prompts + safe config preservation"). Parent: none (operator-facing onboarding work).

**Status:** ⏳ Draft — approved 2026-05-14 (Florida).

**Goal:** Upgrade `garraia init` (the existing onboarding wizard at `crates/garraia-cli/src/wizard.rs`) so a fresh Ubuntu/RunPod GPU machine that just downloaded the `garraia` binary can run `garraia init` and reach a usable, server-friendly `config.yml` with one walk-through. Adds environment detection (OS/root/RunPod/systemd/NVIDIA/Ollama/ports), GPU-gated local-AI-stack prompts (Ollama + Qwen3 GGUF), Whisper/Chatterbox endpoint configuration, OpenRouter cloud fallback, and **safe handling of an existing `config.yml`**. Strictly no `install.sh` changes in this PR — that lives in **PR-B** (plan 0127, depends on this PR merging).

---

## Decisions (locked 2026-05-14)

1. **PR shape — two PRs.** This PR is **PR-A** (wizard foundation). PR-B (installer auto-bootstrap) ships only after PR-A is green and merged.
2. **Service supervision — foreground by default.** `garraia init` does **not** spawn `garraia start`; that's PR-B's job. For local AI services (Ollama / TTS / STT) the wizard prefers `systemctl` only when `systemd-run` / `/run/systemd/system` is present, otherwise `nohup … >> ~/.garraia/<svc>.log 2>&1 &` with a PID file under `~/.garraia/`. No custom supervisor invented.
3. **Local AI stack scope — practical and safe.** Detect NVIDIA GPU via `nvidia-smi`; prompt before installing Ollama (`curl -fsSL https://ollama.com/install.sh | sh`), pulling Qwen3-14B GGUF (`hf.co/MaziyarPanahi/Qwen3-14B-GGUF:Q4_K_M`), or starting TTS/STT. **Never** install NVIDIA drivers or CUDA blindly. CPU-only / no-GPU defaults to cloud-only (OpenRouter) with no Ollama prompts. `GARRAIA_BOOTSTRAP_LOCAL=0` disables the GPU/local stack prompts even when a GPU is present.
4. **Existing config — preserve, never overwrite silently.** When `~/.config/garraia/config.yml` already exists, the wizard:
   - prompts before any write,
   - offers backup to `config.yml.bak-YYYYMMDD-HHMMSS` (UTC suffix, deterministic),
   - offers a merge/update mode that updates only the new sections (gateway host/port/local LLM provider) without clobbering existing `llm`, `channels`, `agent`, `voice`, etc.,
   - in non-interactive mode (existing `!stdin.is_terminal()` guard), **never overwrites** — prints the same "edit your config.yml" hint the current wizard already prints, supplemented with concrete next steps.

Validation gates: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, plus `shellcheck` on any new shell snippets (none expected in PR-A — only docs). PR-A does **not** modify `install.sh`.

---

## Architecture

### File layout

```
crates/garraia-cli/
  Cargo.toml                              [+1 dep: `hostname` or rely on libc; +1 dep: `dirs` already in tree]
  src/
    wizard.rs                             [REWRITE in place — split internals into submodules below]
    wizard/
      mod.rs                              [NEW — top-level `run_wizard` orchestrator]
      env_detect.rs                       [NEW — env probes]
      local_stack.rs                      [NEW — Ollama/TTS/STT detection + install helpers]
      config_writer.rs                    [NEW — config-yml emission + backup/merge logic]
      prompts.rs                          [NEW — dialoguer wrappers used by mod.rs]
docs/
  installation.md                         [UPDATE — describe the new wizard flow at the bottom]
  deployment-runpod.md                    [UPDATE — RunPod fresh-machine recipe pointing at `garraia init`]
  voice.md                                [UPDATE — Chatterbox/Whisper endpoint expectations]
plans/
  0126-curl-sh-auto-bootstrap-pra-wizard.md   [this file]
  0127-curl-sh-auto-bootstrap-prb-installer.md [stub created in §M1 step 1]
  README.md                               [+ row 0126, + row 0127 stub]
```

> Why a `wizard/` submodule split? The current single-file `wizard.rs` is already 299 LOC and this PR adds env detection + local-stack helpers + config preservation logic that won't fit comfortably as inline functions. Smaller files keep the wizard testable per concern (CLAUDE.md "Smaller, well-bounded units" guidance). Module boundary is purely internal to `garraia-cli` — no new public crate surface.

### Module responsibilities

#### `wizard::env_detect` — pure probes, no side effects

```rust
pub struct EnvSnapshot {
    pub os: OsId,                  // Linux { distro, version } | MacOs | Unknown
    pub is_root: bool,             // geteuid() == 0 on unix; false on windows
    pub is_runpod: bool,           // RUNPOD_POD_ID env var set, OR /proc/1/environ contains "RUNPOD_"
    pub has_systemd: bool,         // /run/systemd/system exists AND `systemctl is-system-running` returns
    pub has_nvidia: bool,          // `nvidia-smi -L` exits 0
    pub gpu_summary: Option<String>, // first line of `nvidia-smi -L` when has_nvidia
    pub ollama: OllamaState,       // NotFound | InstalledNotRunning | Running { models: Vec<String> }
    pub ports: PortReport,         // status per [3888, 8080, 11434, 7860, 9090]
}

pub fn detect() -> EnvSnapshot;
```

- Probes shell out via `std::process::Command` with **tight 2s timeouts** (use `subprocess` already implicit via std + a manual `kill_after` helper). `nvidia-smi` is the only probe that can take >100 ms on slow GPUs; cap at 5 s.
- All probes are **infallible** at the type level — failures become `false` / `None`. The wizard must not abort because a probe failed.
- Port check uses `TcpListener::bind(("127.0.0.1", port))`: `Ok(_)` → free, `Err(_)` → in use. Listener dropped immediately.
- `OllamaState::Running` performs `GET http://127.0.0.1:11434/api/tags` with a 1 s timeout (reuses `reqwest` already in `garraia-cli`'s tree via `update.rs`).
- Unit tests cover the `EnvSnapshot` rendering helpers; the probes themselves are mocked via thin trait shims (one `EnvProbe` trait that defaults to real exec; tests inject `FakeProbe`).

#### `wizard::local_stack` — gated installers + service-start helpers

```rust
pub enum InstallChoice { Install, Skip }

pub fn prompt_install_ollama(env: &EnvSnapshot) -> Result<InstallChoice>;
pub fn install_ollama() -> Result<()>;              // curl … | sh, captures stderr
pub fn prompt_pull_qwen3(env: &EnvSnapshot) -> Result<InstallChoice>;
pub fn pull_qwen3() -> Result<()>;                  // `ollama pull hf.co/MaziyarPanahi/Qwen3-14B-GGUF:Q4_K_M`
pub fn prompt_start_ollama(env: &EnvSnapshot) -> Result<InstallChoice>;
pub fn start_ollama_systemd_or_nohup(env: &EnvSnapshot) -> Result<()>;

pub fn print_tts_install_hints();                   // Chatterbox install copy-paste
pub fn print_stt_install_hints();                   // faster-whisper install copy-paste
```

- **Auto-installs (with confirmation gate):** Ollama itself and the Qwen3 model pull. Both are well-known idempotent curl/CLI installs and match the spec.
- **Detect + endpoint-config only:** Chatterbox TTS (port 7860) and faster-whisper STT (port 9090). The wizard writes their endpoints into `voice.tts_endpoint` / `voice.stt_endpoint` and prints exact `pip install` / `git clone` instructions; it does **not** auto-install Python stacks in PR-A. This keeps blast radius tight; a follow-up plan (post-PR-B) can extend `local_stack` with full TTS/STT install flows once the install.sh path is proven.
- `start_ollama_systemd_or_nohup` prefers `systemctl --user start ollama` when systemd is present and ollama ships a unit (most distros do as of Ollama 0.5+); falls back to `nohup ollama serve >> ~/.garraia/ollama.log 2>&1 &` with `echo $! > ~/.garraia/ollama.pid`.
- All shell-outs use `std::process::Command` (no shell interpolation; argv-only) — security review already covered the pattern in `crates/garraia-cli/src/update.rs`.

#### `wizard::config_writer` — emit + backup + merge

```rust
pub struct WizardOutcome {
    pub default_provider: String,            // "openrouter" | "ollama-qwen3"
    pub fallback_providers: Vec<String>,
    pub openrouter_key: Option<SecretString>,
    pub openrouter_model: Option<String>,
    pub local_llm: Option<LocalLlmChoice>,   // populated only when GPU + user accepted
                                             //   LocalLlmChoice { provider_key: String = "ollama-qwen3",
                                             //                    base_url: String = "http://127.0.0.1:11434/v1",
                                             //                    model: String = "hf.co/MaziyarPanahi/Qwen3-14B-GGUF:Q4_K_M" }
    pub voice_enabled: bool,
    pub system_prompt: Option<String>,
    pub telegram: Option<TelegramChoice>,
    pub host: String, port: u16,             // "0.0.0.0":3888 on server/RunPod, "127.0.0.1":3888 otherwise
}

pub fn write_config(
    config_dir: &Path,
    outcome: &WizardOutcome,
    existing: ExistingConfigStrategy,        // FirstWrite | Backup{ path } | MergeUpdate
) -> Result<PathBuf>;
```

- **Backup** path: `config_dir.join(format!("config.yml.bak-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S")))`. Renames the existing file atomically (`fs::rename`) before the new file is written. Backup happens only after the wizard has serialized the new config successfully into memory — never leave the user with no `config.yml`.
- **MergeUpdate** loads the existing `AppConfig` via `garraia_config::ConfigLoader`, then patches: `gateway.host`, `gateway.port`, **adds** `llm.openrouter` / `llm.ollama-qwen3` entries (only when missing — does not stomp existing custom entries with the same key), sets `agent.default_provider` / `agent.fallback_providers` (only when unset or empty), updates `voice.*` only when the user explicitly enabled voice this run.
- The serialized YAML retains key ordering by writing fields in a deterministic order via `serde_yaml::to_string` on the canonical `AppConfig` struct (already deterministic in the existing wizard).
- **Secret-redaction invariant:** the OpenRouter API key is **never** echoed back in the wizard's stdout summary. Vault path remains the existing flow.

#### `wizard::prompts` — dialoguer wrappers

Thin layer so unit tests for `mod.rs` can inject a `Prompter` trait double. Wraps `Select`, `Confirm`, `Input`, `Password` with `with_prompt_default` helpers. Pure code-organization, no UX changes.

### Wizard flow (high-level)

```
1.  Non-interactive guard            (unchanged — early return + hint)
2.  Print banner
3.  env_detect::detect()
4.  Show env summary (one line per check)
5.  Existing config policy
       FirstWrite        → continue
       Exists & TTY      → ask: [Backup&overwrite | Merge/update | Cancel]
       Exists & no-TTY   → error path matches non-interactive guard
6.  Provider mode prompt
       a) Cloud-only (OpenRouter)               ← default when no GPU
       b) Local-first (Ollama + cloud fallback) ← default when GPU + LOCAL_BOOTSTRAP=1
       c) Both (cloud primary, local fallback)
7.  Cloud branch (if a/c):
       → OpenRouter API key (Password, may be empty)
       → Vault storage prompt (unchanged from current wizard)
       → OpenRouter model default = "deepseek/deepseek-chat-v3.5"
         (configurable via OPENROUTER_DEFAULT_MODEL env var; matches memory note
          [reference_pr_b_sql_injection_sites] etc. — current chat.rs default)
8.  Local branch (if b/c, GPU detected, LOCAL_BOOTSTRAP!=0):
       → confirm install Ollama (skips if already running)
       → confirm pull Qwen3-14B GGUF Q4_K_M
       → confirm start Ollama service
9.  Voice prompt (skip unless GPU present):
       "Enable voice (Chatterbox TTS @ :7860 + Whisper STT @ :9090)?"
       → only flips voice.enabled = true; endpoints prefilled from defaults
       → print Chatterbox/Whisper install hints
10. System prompt input (unchanged)
11. Telegram setup (unchanged)
12. config_writer::write_config(...)
13. Final summary:
       - config path
       - what was installed (Ollama? Qwen3?)
       - "Next: `garraia start` to launch the gateway in the foreground."
       - "To run later in background: `garraia start -d`"
       - "Logs: <config_dir>/garraia.log"
       - if voice configured but TTS/STT not running: link to docs/voice.md
```

### Server-friendly config (RunPod / 0.0.0.0)

Host/port selection inside `WizardOutcome`:

| Condition                                                     | host         | port |
|---------------------------------------------------------------|--------------|------|
| `env.is_runpod == true` OR `env.is_root == true` (server)     | `0.0.0.0`    | 3888 |
| Otherwise (laptop/dev)                                        | `127.0.0.1`  | 3888 |
| `PORT` env var set (RunPod LB Serverless)                     | as above     | from `PORT` |

(`PORT` and `HOST` env are already honored by `garra start` per `start_subcommand_honors_port_and_host_env_with_cli_flag_precedence` test in `crates/garraia-cli/src/main.rs:1517`; the wizard writes the **config defaults** that take effect when no env var is present.)

### Generated `config.yml` skeleton (cloud + local example)

```yaml
gateway:
  host: "0.0.0.0"
  port: 3888

llm:
  openrouter:
    provider: openrouter
    model: deepseek/deepseek-chat-v3.5
    base_url: https://openrouter.ai/api/v1
    api_key: ${OPENROUTER_API_KEY}        # or vault — same as today
  ollama-qwen3:
    provider: openai
    base_url: http://127.0.0.1:11434/v1
    api_key: ollama
    model: hf.co/MaziyarPanahi/Qwen3-14B-GGUF:Q4_K_M

agent:
  default_provider: ollama-qwen3          # or "openrouter" for cloud-first
  fallback_providers: ["openrouter"]      # or ["ollama-qwen3"] for cloud-first
  system_prompt: "You are a helpful personal AI assistant."

voice:
  enabled: true                           # only when user opted in
  tts_provider: chatterbox
  tts_endpoint: http://127.0.0.1:7860
  stt_endpoint: http://127.0.0.1:9090
  language: pt
```

---

## Testing strategy

| Layer | Test | Location |
|------|------|----------|
| Unit | `EnvSnapshot::detect` with `FakeProbe` injecting each branch (no GPU / GPU+no Ollama / GPU+running Ollama / RunPod / systemd absent) | `wizard/env_detect.rs` `#[cfg(test)]` |
| Unit | `config_writer::write_config` round-trip — `FirstWrite`, `Backup`, `MergeUpdate` paths each emit deterministic YAML; backup file exists; existing `llm` entries preserved in `MergeUpdate` | `wizard/config_writer.rs` `#[cfg(test)]` |
| Unit | `WizardOutcome → AppConfig` mapping — local-first vs cloud-first puts the right keys in `agent.default_provider` / `fallback_providers` | `wizard/config_writer.rs` |
| Unit | Non-interactive guard preserves today's exit behavior | `wizard/mod.rs` `#[cfg(test)]` |
| Integration | `garraia init` smoke under `assert_cmd` with `GARRAIA_BOOTSTRAP_LOCAL=0` to skip GPU prompts; verifies `config.yml` written to a tempdir | `crates/garraia-cli/tests/wizard_smoke.rs` (NEW) |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | CI |
| Format | `cargo fmt --all -- --check` | CI |
| Workspace tests | `cargo test --workspace` | CI |

> No `shellcheck` step yet — PR-A introduces no new shell scripts. Shellcheck enters in PR-B.

### Out-of-scope explicitly

- `install.sh` changes (PR-B).
- Auto-installing faster-whisper / Chatterbox Python stacks (deferred follow-up — wizard only writes endpoints + prints install hints).
- Touching `garraia start` flag plumbing (already env-driven via plan 0026 + GAR-603).
- Telegram flow changes.
- Adding new dependencies beyond what's already in `garraia-cli`'s Cargo.toml (we already have `dialoguer`, `serde_yaml`, `anyhow`, `tracing`, `reqwest`, `tokio`, `chrono` transitively).

---

## §M1 — Implementation checklist (subagent-executable tasks)

> Each step is independently reviewable. Stop after step 9 and run validation gates (step 10) before opening the PR.

1. **Stub `plans/0127-...md`** with header + "Status: 🕐 Blocked on plan 0126 (PR-A) merging" + the §Decisions section copied verbatim. Add rows for 0126 and 0127 to `plans/README.md`.
2. **Add `wizard/` submodule scaffolding** — create `crates/garraia-cli/src/wizard/{mod.rs,env_detect.rs,local_stack.rs,config_writer.rs,prompts.rs}`. Move current `wizard.rs` content into `mod.rs` verbatim (rename `wizard.rs` → delete; declare `mod wizard;` already in `main.rs`). No behavior change. **Gate:** `cargo check -p garraia-cli` clean.
3. **Implement `env_detect`** with `EnvSnapshot` + `EnvProbe` trait + `RealProbe`. Probes: OS via `/etc/os-release`, root via `nix::unistd::geteuid()` (already transitively available) or `libc::geteuid()`, RunPod via env, systemd via `/run/systemd/system` existence + `systemctl is-system-running` (2s cap), nvidia-smi via `nvidia-smi -L` (5s cap), Ollama via `which ollama` + `GET /api/tags` (1s cap), ports via `TcpListener::bind`. Add 6 unit tests covering each branch. **Gate:** `cargo test -p garraia-cli env_detect`.
4. **Implement `local_stack`** — `prompt_install_ollama`, `install_ollama`, `prompt_pull_qwen3`, `pull_qwen3`, `prompt_start_ollama`, `start_ollama_systemd_or_nohup`, `print_tts_install_hints`, `print_stt_install_hints`. All shell-outs use argv-only `Command` (no `sh -c`). PID file at `~/.garraia/ollama.pid`. **Gate:** `cargo clippy -p garraia-cli -- -D warnings`. Unit test for `print_*_hints` returning the expected text (via a `Sink` trait or `Vec<u8>` writer).
5. **Implement `config_writer`** — `WizardOutcome` struct + `ExistingConfigStrategy` enum + `write_config`. Unit tests:
   - `FirstWrite` → YAML written, no backup file.
   - `Backup{ path }` → existing renamed, new written, both readable.
   - `MergeUpdate` → existing `llm.custom_provider` preserved, new `llm.openrouter` added, `agent.default_provider` filled only when unset.
6. **Implement `prompts`** wrappers — `Prompter` trait + `DialoguerPrompter` + `MockPrompter` for tests. Wire `mod.rs` to use the trait.
7. **Rewrite `mod.rs`** orchestrator — replace the current monolithic `run_wizard` with the flow in §"Wizard flow". Reuses existing vault logic (`garraia_security::CredentialVault`). Keeps the existing non-interactive guard at the top. **Gate:** `cargo test -p garraia-cli`.
8. **Add `tests/wizard_smoke.rs`** under `garraia-cli` with `assert_cmd` invoking `garraia init` with `GARRAIA_BOOTSTRAP_LOCAL=0` in a `tempfile::tempdir`. Verifies `config.yml` is written and parses back via `garraia_config::ConfigLoader`.
9. **Update docs**:
   - `docs/installation.md` — add "Onboarding wizard" section pointing at `garraia init` and the four prompts (provider, GPU/local stack, voice, Telegram).
   - `docs/deployment-runpod.md` — add a "Fresh RunPod GPU pod" recipe: `wget … garraia → chmod +x → ./garraia init → ./garraia start`.
   - `docs/voice.md` — note the wizard prefills `tts_endpoint`/`stt_endpoint` and shows install hints; document that auto-install of TTS/STT is deferred.
10. **Validation gates locally** before opening PR:
    ```
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    ```
    Then commit with `feat(cli): plan 0126 — `garraia init` env detection + GPU/local stack prompts + safe config preservation` and push as branch `feat/0126-init-wizard-pra`.
11. **Open PR** with title `feat(cli): plan 0126 — \`garraia init\` env-aware bootstrap (PR-A)` and body containing the §Decisions block + §Architecture summary + the validation log. **Wait for CI green and explicit user approval before merging.**

---

## Risks & mitigations

| Risk | Mitigation |
|------|------------|
| `nvidia-smi` hangs on a stuck driver | 5 s wall-clock cap via `wait_timeout` crate (already transitive) or manual `kill -KILL` |
| `ollama install` script changes URL or signature | Pin documentation link; do not vendor; surface the upstream URL to the user before exec |
| User cancels mid-wizard with Ctrl-C after backup but before new write | `write_config` writes new file FIRST then renames backup; on Ctrl-C the original remains in place. `fs::rename` is atomic on POSIX. |
| `dialoguer` panics on non-TTY | Already guarded by the existing `IsTerminal` check at the top of `run_wizard`; preserved in the rewrite. |
| Two wizards running concurrently corrupt the config | Acquire a `flock` on `config.yml` (new — `fs2` crate) before backup/write. Opt-in; if `flock` unavailable, log a warning and continue. |
| Existing `MergeUpdate` clobbers a user's custom `llm.openrouter` entry | Merge logic checks key existence per provider; only **adds** keys, never replaces. Surfaced in unit test §5 above. |

---

## Cross-references

- CLAUDE.md §"Convenções de código" — `?` operator, no `unwrap()`, `cargo check -p` gates.
- CLAUDE.md §"Regras absolutas" — never log secrets (rule 6); the wizard already redacts via vault.
- ADR 0005 §"Anti-patterns" — does not apply here; wizard touches no `garraia_login` paths.
- Memory note `[CI concurrency pattern (GarraRUST canon)]` — branch name `feat/0126-init-wizard-pra` matches the convention.
- Memory note `[Strip user surname]` — commit author = `166889728+michelbr84@users.noreply.github.com`.
- `crates/garraia-cli/src/wizard.rs` (current 299 LOC) — to be split into `wizard/` submodule per §"File layout".
- `crates/garraia-config/src/model.rs:289` (`GatewayConfig`), `:400` (`LlmProviderConfig`), `:444` (`AgentConfig`), `:678` (`VoiceConfig`) — schema this plan emits.
- Plan 0127 (PR-B) — depends on this PR merging; wires `install.sh → garraia init </dev/tty → garraia start`.

---

## Acceptance criteria

- [ ] On a fresh Ubuntu/RunPod machine **with** an NVIDIA GPU, `garraia init` walks through env detect → asks to install Ollama + pull Qwen3 + enable voice → writes `~/.config/garraia/config.yml` with `gateway.host: 0.0.0.0`, both `llm.openrouter` and `llm.ollama-qwen3` entries, `agent.default_provider: ollama-qwen3`, `agent.fallback_providers: ["openrouter"]`, `voice.enabled: true`. User exits at "Run `garra start` to launch the gateway."
- [ ] On a CPU-only laptop, the same `garraia init` skips Ollama prompts, writes `llm.openrouter` only, `agent.default_provider: openrouter`, no `voice` section beyond defaults.
- [ ] Re-running `garraia init` against an existing `config.yml` prompts to backup or merge; never silently overwrites.
- [ ] Non-interactive (`echo y | garraia init` or `garraia init < /dev/null`) prints the existing "edit your config.yml" hint and exits 0, **does not** wait on stdin.
- [ ] `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` all clean.
- [ ] CI green on the PR. User-explicit approval before merge.
