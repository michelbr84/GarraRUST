# Garra Mobile (`garraia_mobile`)

Assistente de IA **local-first** para Android. A memória, as skills, os
arquivos e os agentes vivem no runtime Garra que você escolhe — no próprio
telefone (Termux), num PC da sua rede ou no Garra Cloud. O modelo pode estar
em qualquer lugar. Arquitetura: [`docs/mobile/architecture.md`](../../docs/mobile/architecture.md);
decisão: [ADR 0016](../../docs/adr/0016-mobile-termux-local-first.md) + amendment 2026-09-07.

## Stack

- Flutter 3.47.x / Dart 3.13 (o `pubspec.lock` é resolvido contra 3.47.2 — é o que o CI usa)
- **Riverpod 3** + `riverpod_annotation` 4 + `riverpod_generator` 4 (codegen)
- `go_router` com gate por runtime (não por JWT)
- `dio` (`GatewayConnection` para `/api/*`; `ApiService` para o Cloud Alpha)
- `flutter_secure_storage` para segredos (JWT, `gateway.api_key`); `shared_preferences` para o resto
- Fontes bundladas: Inter (variável) + JetBrains Mono — o app renderiza igual offline

## Rodar

```bash
flutter pub get
dart run build_runner build --delete-conflicting-outputs   # *.g.dart sao gitignored
flutter run
```

Na primeira abertura o app pede seu nome e **onde o Garra roda**:

| Modo | O que precisa |
|---|---|
| On this phone | Termux com `garra` instalado: `curl -fsSL https://garraia.org/install.sh \| bash && garra doctor && garra start` (escuta em `127.0.0.1:3888`) |
| Another Garra | Um gateway na LAN: `garra start --host 0.0.0.0` no PC; digite `192.168.x.x:3888` (e a `gateway.api_key`, se configurada) |
| Garra Cloud | Conta no `api.garraia.org` (login) |

Emulador Android → PC hospedeiro: use `10.0.2.2:3888` como "Another Garra".

## Verificar

```bash
flutter analyze          # zero issues e a regra do CI
flutter test             # home (capabilities, estados), runtime store, settings, versao, sugestoes de /
flutter build web --no-web-resources-cdn   # prova visual no navegador; nao e alvo de produto
```

`lib/app_version.dart` precisa bater com o `version:` do `pubspec.yaml` —
`test/app_version_test.dart` falha quando divergem.

## APK

O APK é construído no GitHub Actions (o SDK do Android vem do `dl.google.com`,
que nem todo ambiente alcança):

- `.github/workflows/mobile.yml` — `analyze` + `test` + APK como artefato em todo PR que toca `apps/garraia-mobile/**`.
- `release.yml` (job `build-android-apk`) — publica `garraia-mobile-android.apk` + `.sha256` na Release (best-effort).

Assinatura: com os secrets `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`,
`ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD` o Gradle usa a keystore de upload
(`android/key.properties`, gitignored); sem eles cai na keystore de debug do
runner — instala, mas não atualiza por cima de uma instalação assinada por
outra chave. Build local de release: `flutter build apk --release`.

## Estrutura

```text
lib/
├── app_version.dart          # versao unica (teste vs pubspec)
├── main.dart                 # bootstrap de servicos, tema
├── router/app_router.dart    # gate por runtime; rotas dos tiles e tabs
├── runtime/                  # GarraConnection + implementacoes + config persistida
├── providers/                # chat (sessao do gateway), auth (cloud)
├── screens/                  # home, onboarding, chat, memory, skills, files, agents,
│                             # automations, providers, activity, notifications, profile, settings
├── services/                 # api_service (cloud), offline_queue, sync, biometric, notifications
├── theme/                    # garra_tokens (cores/raios) + garra_theme (ThemeData, garraText)
└── widgets/                  # brand/ (WolfMark, NightRidge), home/ (tiles, cards), bottom nav
```

## Convenções

- Nunca `withOpacity()` — `withValues(alpha:)`.
- Cor nova entra em `theme/garra_tokens.dart`, não hard-coded no widget.
- Endpoint novo entra em `GarraConnection` (interface) **e** em `GatewayConnection`;
  o Cloud herda o que não sobrescreve.
- Feature nova de tile: constante em `GarraFeature` **e** no `feature_flags` do gateway
  (`crates/garraia-gateway/src/capabilities.rs`) — o teste Rust trava o contrato.
