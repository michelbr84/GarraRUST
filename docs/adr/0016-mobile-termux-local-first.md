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

---

## Amendment 2026-09-03 — o que o campo devolveu (issues #909–#913)

Cinco issues de um único usuário rodando a v0.3.6 num Samsung A16 (Android 13,
Termux), com o Garra em produção orquestrado por outro agente via MCP. O
retorno confirmou a v0 e corrigiu duas premissas.

**`rustls-platform-verifier` está fora do grafo do `garra`, e agora sob
guarda.** O reporter viu o panic conhecido do crate (`Expect
rustls-platform-verifier to be initialized`, `android.rs:90`) — ele exige um
Context JNI que não existe num binário standalone. Não é o nosso binário:

```
cargo tree -p garraia --target aarch64-linux-android --invert rustls-platform-verifier
# error: package ID specification `rustls-platform-verifier` did not match any packages
```

O crate entra no `Cargo.lock` só via `reqwest 0.13`, cuja feature `rustls` é
ativada pelo `tauri-plugin-updater` do `garraia-desktop` — que nunca é
buildado para Android. É um acidente feliz de resolução de features, não uma
decisão, então virou uma guarda no `ci.yml` (job `android`).

Corolário que vale escrever: **`SSL_CERT_FILE` não afeta o HTTPS do `garra`.**
Todo o tráfego vai por `reqwest 0.12` com `rustls-tls`, cujos roots são os do
webpki compilados no binário; nenhum trust store do sistema é lido. A única
exceção é o `sqlx` (native-roots), ou seja TLS de Postgres. O `garra doctor`
passou a dizer isso explicitamente.

**O exec do Termux é problema do pai, não do processo.** Hosts MCP externos
spawnam com ambiente filtrado e removem `LD_PRELOAD`; sem ele o exec de ELF
falha *antes* de o binário rodar, então nenhuma correção dentro do `garraia`
alcança o caso. Duas peças, em camadas diferentes:

- `install.sh` escreve `$PREFIX/bin/garra-mcp-server` (wrapper que exporta o
  shim e faz exec do CLI) — é o `command` que um host externo deve usar;
- `McpManager::connect` injeta `LD_PRELOAD` nos filhos MCP sob
  `cfg(target_os = "android")`, o que cobre a direção cliente (servidores
  npm/pip com shebang `/usr/bin/env`). Nunca sobrescreve um valor explícito.

**Bug de cfg encontrado no caminho:** `apply_parent_death_signal` era
`#[cfg(target_os = "linux")]`, e `target_os = "android"` **não** é coberto por
isso em Rust — todo filho MCP no Termux ficava órfão. O bionic expõe o mesmo
`prctl(PR_SET_PDEATHSIG)`.

**Sobre distribuição:** o relato "o binário de release não roda no Termux" era
sobre uma v0.3.6 que *já publica* `garraia-android-aarch64`. A causa é a do
Amendment acima — um `install.sh` estale servido pelo `garraia.org`, cujo
publish é manual. A sonda do `install-endpoints.yml` ganhou uma guarda de
frescor para o modo de falha que passava em todas as outras: endpoint que
existe, responde 200, parseia — e está velho.

---

## Amendment 2026-09-04 — o exec do Termux tem dois modos de falha, não um

Segundo lote de campo do mesmo usuário (issues #920–#925, v0.3.8). A #920 é
retorno direto sobre o wrapper que o Amendment anterior introduziu: ele **não
fechou o caso**, e a razão é que "o exec falha no Termux" são na verdade duas
falhas com o mesmo sintoma.

```
A. o host não consegue exec'ar o wrapper *script*
   timeout: failed to run command 'garra-mcp-server': Permission denied

B. o wrapper roda e o exec interno do ELF falha
   garra-mcp-server: 8: exec: /data/.../garraia: Permission denied
```

O `LD_PRELOAD` do Amendment anterior endereça B — e só quando o shim está
instalado. **A não tem solução dentro de um wrapper**, porque o wrapper também
precisa ser exec'ado: qualquer script que escrevêssemos herda a mesma falha.
Foi o próprio relator que achou o caminho que sobrevive aos dois:

```bash
/system/bin/linker64 /data/data/com.termux/files/usr/bin/garraia mcp-server
```

O loader dinâmico do Android mapeia o ELF direto — sem shim, sem `LD_PRELOAD`,
sem nenhuma variável de ambiente. Validado por ele fim a fim (handshake MCP +
`garra_ask`) com `env -i`.

Consequência para a decisão, em duas camadas com escopos honestos:

- `install.sh` escreve um **segundo** wrapper, `garra-mcp-server-linker`, que
  cobre B sem depender do termux-exec. Arquivo separado do primeiro, não uma
  linha a mais nele: a suíte afirma `grep -c '^exec '` == 1 em cada um, o que
  faz uma futura fusão dos dois derrubar os testes em vez de apagar o fallback
  em silêncio.
- Para **A**, a resposta é documental e não tem wrapper: o host aponta
  `command: /system/bin/linker64` e passa o binário como argumento. É o que
  `docs/installation.md`, `docs/cli-mcp-server.md` e o `garraia doctor` agora
  dizem — o doctor com o caminho real preenchido.

A generalização que vale guardar: **no Termux, `command:` apontando para
qualquer coisa nossa é frágil por construção.** A única invocação que não
depende de nada do Termux é a que usa um binário do sistema Android como
`argv[0]`. Wrappers são conveniência para o caso comum, não a garantia.

A #925, do mesmo lote, é a contrapartida em diagnóstico: o aviso de
`GARRAIA_JWT_SECRET` ausente dizia o que estava errado sem dizer se importava.
Num gateway local single-user não importa — o console web, `/ws`,
`/v1/chat/completions`, o `mcp-server` e os canais não passam por `/auth/*` —,
mas `/chat` (mobile, via `MobileAuth`) e o workspace multi-tenant passam. O
aviso passa a dizer as duas coisas e o comando de correção.
