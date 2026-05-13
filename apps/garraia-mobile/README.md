# garraia_mobile — Garra Cloud Alpha

Cliente Flutter (Android / iOS / web preview) do gateway [GarraIA](../../README.md).
Conversa com o agente local ou remoto via JWT + Dio sobre as rotas
`/auth/*` e `/chat/*` expostas por `garraia-gateway`.

## Stack

- **Flutter** 3.41+ (Dart 3.11+)
- **Riverpod 2** + code generation para state management
- **go_router** com redirect baseado em JWT
- **Dio** + `_AuthInterceptor` (Bearer)
- **flutter_secure_storage** para persistir o token (`garraia_jwt`)
- **rive** para o mascote animado (placeholder até `assets/garra_mascot.riv`)

## Início rápido

```bash
cd apps/garraia-mobile
flutter pub get
dart run build_runner build --delete-conflicting-outputs

# Emulador Android → backend local na porta 3888 (10.0.2.2 = host)
flutter run

# Apontar para o gateway na nuvem
flutter run --dart-define=API_BASE_URL=https://api.garraia.org
```

Setup completo (pré-requisitos, build de APK, mascote Rive, variáveis de
ambiente do backend): ver [`SETUP.md`](SETUP.md).

## Endpoints consumidos

| Endpoint           | Origem                         |
|--------------------|--------------------------------|
| `POST /auth/register` | `garraia-gateway` mobile auth (GAR-335) |
| `POST /auth/login`    | idem                                   |
| `GET  /me`            | idem                                   |
| `POST /chat`          | `garraia-gateway` mobile chat (GAR-339) |
| `GET  /chat/history`  | idem                                   |

A base URL default é `http://10.0.2.2:3888` (loopback do emulador
Android). Override via `--dart-define=API_BASE_URL=...`.

## Estrutura

```text
lib/
├── main.dart              # MaterialApp.router + ProviderScope
├── router/app_router.dart # GoRouter + auth redirect
├── services/api_service.dart
├── providers/             # AuthState, ChatMessages, MascotState
├── screens/               # splash, login, register, chat, settings
└── widgets/               # MascotWidget (4 estados), ChatBubble
assets/                    # ativos empacotados (Rive, ícones, fontes)
test/                      # widget tests (Riverpod overrides)
```

## Testes

```bash
flutter analyze
flutter test
```

Os widget tests injetam um stub de `ApiService` via
`apiServiceProvider.overrideWithValue(...)`, mantendo a árvore offline
e determinística.

## Convenções

- Riverpod com code generation (`*.g.dart` gerado, não commitado).
- Nunca usar `withOpacity()` — usar `withValues(alpha:)` (CLAUDE.md raiz).
- JWT armazenado em `flutter_secure_storage`, chave `garraia_jwt`.

## Referências

- Projeto raiz e arquitetura: [`README.md`](../../README.md)
- Setup detalhado: [`SETUP.md`](SETUP.md)
- ROADMAP e Linear: [`ROADMAP.md`](../../ROADMAP.md)
