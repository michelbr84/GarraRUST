# 16. Garra Mobile — Termux como camada de execução (local-first no Android)

- **Status:** Accepted
- **Deciders:** @michelbr84 + Claude (sessão 2026-09-02)
- **Date:** 2026-09-02
- **Tags:** mobile, android, termux, local-first, installers, llm-providers
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - ROADMAP: §4.5 Garra Mobile Local (Termux → nativo)
  - Antecedentes: ADR 0001 (inference backend), ADR 0015 (toolchain de pacotes/regra 15 de assets), ADR 0004 (storage), `docs/security/threat-model.md` §5.6 (SSRF guard)
  - Implementação Fase 0: job `build-android-arm64` no `release.yml`; branch Termux no `install.sh` + suíte `tests/install_sh/detect_platform.sh`; `garra doctor` + provider `llamacpp` no `garraia-cli`; asset `garraia-android-aarch64`

---

## Context and Problem Statement

O Garra existe em desktop (CLI + Tauri) e num app Flutter (`apps/garraia-mobile`)
que é **cliente** do gateway — exige um servidor rodando em algum lugar e rede.
O dono do projeto quer o Garra como assistente **no próprio Android**: executar
localmente no aparelho, conversar com uma LLM que pode estar (a) na nuvem, (b)
na LAN do PC/LAN-serve, ou (c) — futuramente — no próprio dispositivo. Sem
dependência de "deixar o PC ligado", sem pagar runtime de terceiros, e sem
abandonar a base Rust existente.

A pergunta arquitetural: **como executar o core Rust no Android em gerações,
sem reescrever Termux nem adotar um fork fragmentado do ecossistema Android?**

## Decision Drivers

1. **★★★★★ Nunca recriar um Termux** — bootstrap/compiladores/`pkg`/W^X
   liberado é ecossistema mantido por outras pessoas; usar, não duplicar.
2. **★★★★★ Core agnóstico de capabilities** — o core Rust nunca fala com API
   Android diretamente; capabilities (notificação, storage, camera) entram por
   camadas acima (Termux hoje, RUN_COMMAND no v1, JNI no v2, APIs nativas no v3).
3. **★★★★★ Bionic dinâmico, musl proibido** — musl estático quebra DNS no
   Android (`/etc/resolv.conf` inexistente; resolver interno do musl não passa
   pelo `dnsproxyd`; `LD_PRELOAD` não intercepta binário estático). Todo target
   Android usa `aarch64-linux-android` via NDK.
4. **★★★★ LLM externa opcional desde o dia 1** — o CLI já aceita
   OpenAI-compatible em `--base-url` (llama.cpp/LM Studio/vLLM/Ollama na LAN) e
   providers cloud; nada na Fase 0 depende de inferência no aparelho.
5. **★★★ Canais separados do core** — Telegram/Discord/etc. são wiring do
   gateway; no mobile eles podem nem subir (o doctor reporta, não impede).
6. **★★★ Assets aditivos (regra 15)** — `garraia-android-aarch64` entra ao lado
   dos crus existentes; `garra update` resolve por nome exato.

## Considered Options

| Opção | Avaliação |
|---|---|
| **v0: Termux + binário bionic dedicado (escolhida)** | Compila hoje sem mudança de código no core (recon 2); distribuição por `curl install.sh \| bash` funciona de fábrica no Termux oficial (bootstrap traz curl/bash/tar/xz/sha256sum; ELF em `$PREFIX/bin` executa — W^X não se aplica no SDK 28); custo = um job de CI e um branch no installer. |
| Flutter app como runtime (executar LLM no app) | `apps/garraia-mobile` é cliente HTTP; hospedar o core dentro do Flutter exigiria FFI imediato + distribuição de binário no APK (Play proíbe download de código executável). Pula a distância toda de uma vez. |
| Termux + compilar no aparelho (`pkg install rust`) | rustc 1.97 + rust-std-android existem no Termux, mas o phantom process killer mata builds paralelos longos; não é canal de distribuição, é fallback de emergência. |
| musl estático "universal" | Violação direta do driver 3 (DNS quebra). Proibido neste contexto. |
| PWA / WebView local | Sem execução de binário nativo; não atende "executar o core no aparelho". |

## Decision Outcome

**Quatro gerações, cada uma entregável isoladamente:**

- **v0 (Fase 0, esta ADR — release v0.3.6):** binário full
  `aarch64-linux-android` (bionic, `cargo-ndk -t arm64-v8a -p 21`) com openssl
  vendido; asset `garraia-android-aarch64`; branch Termux no `install.sh`
  (`$TERMUX_VERSION` ou `$PREFIX` *com.termux* → default `$PREFIX/bin`, skip do
  preflight glibc, notice de phantom process killer/bateria); `garra update`
  resolve o asset no Android; `garra doctor` como passo de onboarding;
  `llamacpp` wired no CLI como segundo provider keyless local.
- **v1 — Companion Kotlin/Compose (app novo, decisão do dono):** o app declara
  `com.termux.permission.RUN_COMMAND` + `<queries>` com.termux; exige
  `allow-external-apps=true` (setup guiado); stdout/exit via
  `RUN_COMMAND_PENDING_INTENT` (cap 100KB); detecta o fork do Play do Termux
  (sem RUN_COMMAND) e degrada com aviso; foreground service para "Always On".
  O código sobrevive até o v2.
- **v2 — Híbrido JNI:** core como `cdylib` no APK via NDK; app assume
  capabilities do Termux (notificação, storage, location); distribuição:
  F-Droid (download de runtime com opt-in explícito) ou Play com jniLibs
  embutidos — **nunca** download de executável no Play.
- **v3 — Nativo:** sem Termux; capabilities 100% Android (CameraX,
  LocationManager, NotificationManager); descoberta LAN com permissões
  (`NEARBY_WIFI_DEVICES` hoje; `ACCESS_LOCAL_NETWORK` blocked-by-default
  targetSdk 37+ ~2027) — entrada manual de URL é UX primária desde o dia 1.

### Decisões de Fase 0 e rationale

- **Binário FULL com openssl vendido** em vez de feature-gating channels/
  telemetry: zero mudança de código no core (recon 2 confirmou que
  `wasmtime` e `aws-sdk-s3` NÃO estão no grafo do binário; os C-deps
  rusqlite/sqlite-vec compilam com CC/AR do NDK). Feature-gate/pin `ring`
  fica para a Fase 1 como otimização — tocar no provider TLS é mudança
  crypto no binário desktop inteiro.
- **`garra doctor`** (sysexits 0/2/65, `--json`/`--strict`) como passo 2 do
  onboarding (`install.sh` → `doctor` → `chat`): sobrevive a config
  ausente/não-parseável, reporta plataforma (com detecção Termux), dirs,
  config (reuso de `run_check`), fonte de credencial presence-only e probe
  TCP vetado por `garra_common::ssrf` (`IpScope::AllowPrivate` — o alvo
  local é legítimo, mas link-local/CGNAT/multicast continuam bloqueados,
  regra 14). Probes são diagnóstico e nunca afetam o exit code.
- **`llamacpp` keyless no CLI e no gateway** (`http://localhost:8080`):
  llama-server é o daemon local natural de quem já tem a LLM na LAN via
  OpenAI-compatible; mirroring do ollama arm mantém `config check` truthful
  (lockstep `provider_key_env`).

### Consequences

- **Positivas:** execução local no Android já na v0.3.6 sem tocar no core;
  trilho de migração explícito que reaproveita cada camada (v0 binário → v1
  app → v2 lib → v3 nativo); regra 15 intacta; o guard SSRF é o mesmo do
  desktop.
- **Negativas / assumidas:**
  - OpenSSL vendido alonga o job android (minutos extras; aceitável).
  - Fork do Play do Termux **removeu** RUN_COMMAND e tem W^X diferente — a
    v1 detecta e degrada; a Fase 0 não funciona nele de forma oficial (o
    usuário pode instalar o Termux F-Droid/GitHub ao lado).
  - Phantom process killer + gestão de bateria matam processos longos em
    background — o installer imprime o notice; o "Always On" real chega no
    v1 (foreground service). `garra start -d` (fork/setsid, `cfg(unix)`)
    funciona dentro do Termux.
  - Android 16/17 (LNP) restringem scan de LAN — port-probe nunca foi o
    caminho (nenhum servidor LLM anuncia mDNS); URL manual + `--base-url`
    é a UX primária.
  - Sem assinatura de código (mesmo status do resto da matriz; integridade
    via `SHA256SUMS`).
  - `garraia-android-aarch64` é best-effort (job fora do `if:` de gate do
    `release.yml`, padrão needs-mas-fora-do-`if:`, `continue-on-error`
    proibido).

---

## Amendment 2026-09-02 — distribuição e o site

O `install.sh` servido por `garraia.org` é cópia estática sincronizada do
`main` deste repo (regra 17): o cron de sync roda no repo do site, mas o
**publish na Lovable é manual**. O merge deste ADR + branch Termux não muda
o que `irm https://garraia.org/install.sh` serve até o publish do dono. Até
lá, o Termux resolve o script pelos mirrors diretos (GitHub release CDN,
raw, jsDelivr) documentados no README.
