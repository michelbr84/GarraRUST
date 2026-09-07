# Garra Mobile — arquitetura (v0.4.0)

> Decisão: [ADR 0016](../adr/0016-mobile-termux-local-first.md) + amendment
> 2026-09-07. Estratégia completa (Termux vs. alternativas, fases 0–6):
> sessão de planejamento 2026-09-07. Este documento descreve **o que está no
> repositório**, não o que está planejado.

## Onde cada coisa vive

```text
┌──────────────────────── Android ────────────────────────┐
│  Garra Mobile (apps/garraia-mobile, Flutter)             │
│  UI ─► GarraConnection (lib/runtime/) ─┬─ GatewayConnection ── /api/* ──► 127.0.0.1:3888 (Termux)
│                                        ├─ GatewayConnection ── /api/* ──► http://<pc>:3888 (LAN)
│                                        └─ CloudConnection ──── /chat  ──► api.garraia.org (JWT)
│  RuntimeConfig: SharedPreferences (modo, URL, nome) + flutter_secure_storage (API key / JWT)
└──────────────────────────────────────────────────────────┘
        Termux: `garra start` — binário `garraia-android-aarch64` (bionic, ADR 0016 v0)
```

A regra que sustenta tudo: **a UI não sabe onde o Garra está.** Toda chamada
passa por `GarraConnection` (`lib/runtime/garra_connection.dart`). Um runtime
embutido (Rust no APK, ADR 0016 v2) entra como uma quarta implementação sem
tocar em tela nenhuma.

## Modos de runtime

| Modo | URL padrão | Auth | Chat |
|---|---|---|---|
| On this phone (`local`) | `http://127.0.0.1:3888` | nenhuma (loopback) | `/api/sessions` |
| Another Garra (`remote`) | digitada (`192.168.x.x:3888` vira `http://…`) | `gateway.api_key` opcional → `Authorization: Bearer` | `/api/sessions` |
| Garra Cloud (`cloud`) | `https://api.garraia.org` | JWT (`/auth/login`) | `/chat` (Cloud Alpha, thread única) |

O gate do router (`lib/router/app_router.dart`) é "há runtime configurado?":
sem runtime → `/onboarding`; cloud sem JWT → `/login`; senão → `/home`.
Login **não existe** nos modos local e LAN.

Sessão do gateway: `POST /api/sessions` devolve `session_id` (persistido em
`garraia_session_id`) e, quando `session_tokens_required`, um cookie
`garraia_session` que o Dio não guarda sozinho — `GatewayConnection` captura o
`Set-Cookie` e o repete como `Cookie` (`session_auth.rs` aceita cookie ou
Bearer).

## Negociação de capabilities

A home lê `GET /api/capabilities` e cada tile declara a feature de que
depende (`GarraFeature` em `lib/runtime/models.dart`):

| Tile | Feature | Endpoints |
|---|---|---|
| Chat | `chat` | `/api/sessions`, `/api/sessions/{id}/messages`, `/api/sessions/{id}/history` |
| Memory | `memory` | `/api/memory/recent`, `/api/memory/search` |
| Skills | `learning-skills` | `/api/learning/skills`, `/api/slash-commands` |
| Files | `projects` | `/api/projects`, `/api/projects/{id}/files` |
| Agents | `modes` | `/api/modes`, `/api/mcp` |
| Automations | `automations` | **nenhum** — o gateway não expõe scheduling; o tile mostra *Unavailable* |

O lado Rust é `crates/garraia-gateway/src/health.rs::feature_flags` (com
teste que trava o contrato). Adicionar feature é seguro; renomear ou remover
quebra o app.

## O que fica onde (dados)

| Dado | Onde | Por quê |
|---|---|---|
| modo, URL, nome de exibição, `session_id` | SharedPreferences | não sensível |
| `gateway.api_key` (LAN), JWT (cloud) | `flutter_secure_storage` | nunca em preferences (`test/runtime_config_test.dart` garante) |
| memória, skills, arquivos, config do agente, chaves de provider | **no runtime** (Termux / PC / cloud) | o app é cliente; enquanto o runtime for outro app (UID do Termux ≠ UID do app), o Keystore do app não alcança o cofre do Garra |

## Rede (Android)

`android:usesCleartextTraffic` global foi trocado por
`res/xml/network_security_config.xml`: cleartext permitido na base (não há
como expressar "só faixas privadas" — `domain-config` aceita hosts, não
CIDR), e **HTTPS forçado** em `garraia.org`. Permissões declaradas só as que
os plugins usam (`RECORD_AUDIO`, `CAMERA`, `POST_NOTIFICATIONS`,
`USE_BIOMETRIC`, `ACCESS_NETWORK_STATE`); cada uma é pedida pela feature que
a usa. Deep links `garraia://chat/<id>` e `garraia://session/<id>`.

## Build e distribuição

- **Local:** `flutter pub get` → `dart run build_runner build
  --delete-conflicting-outputs` (todo `*.g.dart` é gitignored) → `flutter
  analyze` → `flutter test`. `flutter build web --no-web-resources-cdn` serve
  para provar o visual num navegador (CanvasKit embutido).
- **CI:** `.github/workflows/mobile.yml` — `flutter analyze` + `flutter test`
  em PRs que tocam `apps/garraia-mobile/**`, e um job que constrói o APK e o
  publica como artefato do run. Flutter fixado em 3.47.2 (o `pubspec.lock` é
  resolvido contra ele; `--enforce-lockfile`).
- **Release:** job `build-android-apk` no `release.yml` (best-effort; em
  `needs:` do `release`, fora do `if:`) → asset `garraia-mobile-android.apk`
  + `.sha256`. Assinatura: secrets `ANDROID_KEYSTORE_BASE64`,
  `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`
  quando existirem (ação manual do dono); senão keystore de debug do runner,
  com `::warning::` — instala, mas não atualiza por cima de uma instalação
  assinada por outra chave.
- **Não faz parte:** Google Play (o app não baixa executável, mas depende do
  Termux instalado ao lado; a trilha Play é a v2 com runtime embutido).

## Dependências que merecem nota

- **Riverpod 3 / riverpod_generator 4.** O 2.6.x prende o `analyzer` na
  linguagem 3.9 e quebra em Dart ≥ 3.13 (`visitDotShorthandPropertyAccess`).
  `custom_lint` e `riverpod_lint` saíram (contradizem-se em
  `analyzer_plugin`); `flutter_lints` continua sendo o gate.
- **`record` 6.2.1.** O 5.1.2 resolvia `record_web` e
  `record_platform_interface` incompatíveis e quebrava o build web; o
  override antigo de `record_linux` saiu junto.
- **`flutter_markdown` 0.7.x.** Descontinuado upstream; funciona. Dívida para
  a 0.4.x: trocar por um fork mantido.
- **Fontes bundladas** (Inter variável + JetBrains Mono, OFL, licenças ao lado
  dos arquivos). Inter é fonte variável: o peso é selecionado por
  `FontVariation('wght', …)` — ver `garraText()` em `lib/theme/garra_theme.dart`.
  Nada é buscado em runtime, para o app renderizar igual offline.

## O que a v0.5.x precisa (Fase 1 da estratégia, fechamento)

1. `TermuxBridge.kt` (`com.termux.permission.RUN_COMMAND`, `<queries>`
   com.termux, detecção do fork Play) — instalar/iniciar/parar o runtime sem
   o usuário abrir o Termux.
2. `garra mobile {bootstrap,pair,serve,status} --json` no CLI Rust e token de
   pairing local obrigatório mesmo em loopback.
3. Foreground service para "Always On" e supervisor com `/health`.
4. Trocar `flutter_markdown`.
