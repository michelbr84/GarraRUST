# Garra Mobile — Setup Guide

## Pré-requisitos

1. **Flutter SDK** ≥ 3.22
   Download: https://docs.flutter.dev/get-started/install/windows/mobile

2. **Android Studio** (ou VSCode + extensão Flutter)

3. **Java 17+** (bundled no Android Studio)

---

## Primeiros passos

```bash
# 1. Instalar dependências
cd apps/garraia-mobile
flutter pub get

# 2. Gerar código Riverpod (*.g.dart)
dart run build_runner build --delete-conflicting-outputs
```

> Os arquivos `*.g.dart` são gerados automaticamente e não devem ser commitados.
> Adicione ao `.gitignore`: `**/*.g.dart`

---

## Rodar no emulador Android

```bash
# Inicie o gateway local na porta 3888
cargo run -p garraia -- --port 3888

# Em outro terminal, rode o app (emulador aponta para 10.0.2.2:3888 = localhost)
cd apps/garraia-mobile
flutter run
```

---

## Rodar apontando para a nuvem

```bash
flutter run --dart-define=API_BASE_URL=https://api.garraia.org
```

---

## Gerar APK de debug

```bash
flutter build apk --debug
# APK em: build/app/outputs/flutter-apk/app-debug.apk
```

## Gerar APK de release

```bash
flutter build apk --release --dart-define=API_BASE_URL=https://api.garraia.org
# APK em: build/app/outputs/flutter-apk/app-release.apk
```

---

## Estrutura do projeto

```
lib/
├── main.dart                  # Entrada, MaterialApp.router
├── router/
│   └── app_router.dart        # GoRouter + redirect auth
├── services/
│   └── api_service.dart       # Dio HTTP client, models
├── providers/
│   ├── auth_provider.dart     # AuthState (Riverpod)
│   └── chat_provider.dart     # ChatMessages + MascotState
├── screens/
│   ├── splash_screen.dart
│   ├── login_screen.dart
│   ├── register_screen.dart
│   └── chat_screen.dart
└── widgets/
    ├── mascot_widget.dart     # Placeholder → trocar por Rive
    └── chat_bubble.dart
```

---

## Adicionar mascote Rive

1. Exporte o arquivo `.riv` do Rive Studio como `assets/garra_mascot.riv`
2. Em `mascot_widget.dart`, substitua o `Container` por:

```dart
RiveAnimation.asset(
  'assets/garra_mascot.riv',
  stateMachines: const ['GarraStateMachine'],
  onInit: (artboard) {
    final ctrl = StateMachineController.fromArtboard(
      artboard, 'GarraStateMachine',
    )!;
    artboard.addController(ctrl);
    // Trigger inputs: idle, thinking, talking, happy
  },
)
```

---

## Variáveis de ambiente relevantes (backend)

```bash
GARRAIA_JWT_SECRET=<segredo-forte-256bits>  # obrigatório em produção
GARRAIA_PORT=3888
```
