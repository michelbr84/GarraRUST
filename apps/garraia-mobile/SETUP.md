# Garra Mobile — Setup

## Pré-requisitos

- Flutter **3.47.2** (stable) — a versão fixada no CI (`.github/workflows/mobile.yml`).
  O `pubspec.lock` é resolvido contra ela; outra versão pode pedir `flutter pub get`
  sem `--enforce-lockfile` e mudar o lock.
- JDK 17 (o Gradle do app compila com `JavaVersion.VERSION_17`).
- Android SDK / Android Studio para o APK. **Não** fixe `org.gradle.java.home` em
  `android/gradle.properties`: use `JAVA_HOME` ou `~/.gradle/gradle.properties`.

## Passos

```bash
cd apps/garraia-mobile
flutter pub get
dart run build_runner build --delete-conflicting-outputs
flutter analyze && flutter test
```

## Backend para desenvolver

Um gateway local basta:

```bash
cargo run -p garraia -- start --port 3888          # no PC
```

- Emulador: no onboarding escolha *Another Garra* e use `10.0.2.2:3888`.
- Aparelho físico na mesma Wi-Fi: `garra start --host 0.0.0.0 --port 3888` no PC e o IP da
  máquina no app. Com `gateway.api_key` configurada, informe-a no onboarding.
- Termux no próprio aparelho: `curl -fsSL https://garraia.org/install.sh | bash`,
  `garra doctor`, `garra start`; no app escolha *On this phone*.

Sem gateway nenhum, a home mostra o card **Runtime** vermelho com a instrução — é o
comportamento esperado, não um bug.

## APK

```bash
flutter build apk --release            # debug keystore se nao houver android/key.properties
```

Para assinar com a keystore de upload, crie `android/key.properties` (gitignored):

```properties
storeFile=upload-keystore.jks
storePassword=...
keyAlias=...
keyPassword=...
```

No CI os mesmos valores vêm dos secrets `ANDROID_KEYSTORE_*`.

## Prova visual sem Android SDK

```bash
flutter build web --release --no-web-resources-cdn
python3 -m http.server 8088 --directory build/web
```

Abra `http://127.0.0.1:8088/#/home` num navegador com viewport de celular. Para a
home mostrar dados, aponte o runtime para um gateway real ou para um stub que
responda `/api/health` e `/api/capabilities` com CORS.
