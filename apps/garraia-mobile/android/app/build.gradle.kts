import java.io.FileInputStream
import java.util.Properties

plugins {
    id("com.android.application")
    id("kotlin-android")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

// Release signing (Garra Mobile v0.4.0):
//   - `android/key.properties` present → real upload keystore (CI writes it
//     from the ANDROID_KEYSTORE_* secrets; locally you create it by hand).
//   - absent → fall back to the debug keystore so `flutter build apk --release`
//     still produces an installable APK. Each machine/runner has its own debug
//     key, so a debug-signed build cannot upgrade an install signed elsewhere
//     (INSTALL_FAILED_UPDATE_INCOMPATIBLE) — the CI job prints a ::warning::
//     when it takes this path. Never commit a keystore (see android/.gitignore).
val keystorePropertiesFile = rootProject.file("key.properties")
val hasReleaseKeystore = keystorePropertiesFile.exists()
val keystoreProperties = Properties().apply {
    if (hasReleaseKeystore) {
        FileInputStream(keystorePropertiesFile).use { load(it) }
    }
}

android {
    namespace = "org.garraia.mobile"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
        isCoreLibraryDesugaringEnabled = true
    }

    kotlinOptions {
        jvmTarget = JavaVersion.VERSION_17.toString()
    }

    defaultConfig {
        // Canonical application id — `org.garraia.*` reverse-DNS of garraia.org
        // matches the install.sh bootstrap domain and the `garraia` CLI binary
        // name. Replaces the Flutter scaffold default `com.example.garraia_mobile`
        // (was: ROADMAP §1.1 item 7 débito).
        applicationId = "org.garraia.mobile"
        // For more information on the values below, see:
        //   https://flutter.dev/to/review-gradle-config
        minSdk = flutter.minSdkVersion
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

    signingConfigs {
        if (hasReleaseKeystore) {
            create("release") {
                keyAlias = keystoreProperties.getProperty("keyAlias")
                keyPassword = keystoreProperties.getProperty("keyPassword")
                storeFile = file(keystoreProperties.getProperty("storeFile"))
                storePassword = keystoreProperties.getProperty("storePassword")
            }
        }
    }

    buildTypes {
        release {
            signingConfig = if (hasReleaseKeystore) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
        }
    }
}

flutter {
    source = "../.."
}

dependencies {
    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.4")
}
