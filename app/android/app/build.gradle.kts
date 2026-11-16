plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

// Each build must install over the one before it: Android refuses an APK
// whose versionCode is lower than the installed one, and earlier split-ABI
// builds shipped 2001. Unless --build-number sets one, the code is 10000 plus
// the commit count, which only grows on main (history is never rewritten).
val commitCount: Int? = try {
    providers.exec {
        commandLine("git", "rev-list", "--count", "HEAD")
        isIgnoreExitValue = true
    }.standardOutput.asText.get().trim().toIntOrNull()
} catch (e: Exception) {
    null
}

// Package only the ABIs Flutter built (`flutter build apk --target-platform`
// passes them as -Ptarget-platform). Plugins (mobile_scanner's CameraX and
// ML Kit) ship native libraries for every ABI; an arm64 APK that also carries x86_64 or armeabi-v7a folders makes
// those devices pick that ABI and then fail to find libflutter.so.
val flutterAbis: List<String> = (findProperty("target-platform") as String?)
    ?.split(",")
    ?.mapNotNull {
        mapOf("android-arm" to "armeabi-v7a", "android-arm64" to "arm64-v8a", "android-x64" to "x86_64")[it.trim()]
    }
    ?.takeIf { it.isNotEmpty() }
    ?: listOf("armeabi-v7a", "arm64-v8a", "x86_64")

android {
    namespace = "org.weirdbrains.morph"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        // TODO: Specify your own unique Application ID (https://developer.android.com/studio/build/application-id.html).
        applicationId = "org.weirdbrains.morph"
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        minSdk = flutter.minSdkVersion
        targetSdk = flutter.targetSdkVersion
        // Uses the version code from pubspec.yaml. When using split APKs, 1000 * ABI_VERSION
        // is added automatically by Flutter. (https://developer.android.com/studio/build/configure-apk-splits#configure-APK-versions)
        // You can force using the value of versionCode by specifying the `-P force-version-code-ignoring-abi=true`
        // flag during build.
        versionCode = flutter.versionCode.let { if (it > 1) it else 10000 + (commitCount ?: 0) }
        versionName = flutter.versionName
        ndk {
            abiFilters.clear()
            abiFilters += flutterAbis
        }
    }

    // Native libraries are compressed inside the APK (and extracted on
    // install). Stored uncompressed they were 18 MB of a 25 MB budget.
    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }

    // A fixed test key (not a Play release key) when MORPH_ANDROID_KEYSTORE
    // names a keystore: CI decodes it from the ANDROID_TEST_* secrets so
    // every build installs over the last one. Without it (forks, local
    // builds) the default debug key signs, as before.
    val testKeystore = System.getenv("MORPH_ANDROID_KEYSTORE")
        ?.takeIf { it.isNotBlank() }
        ?.let { file(it) }
        ?.takeIf { it.isFile }
    signingConfigs {
        if (testKeystore != null) {
            create("morphTest") {
                storeFile = testKeystore
                storeType = "pkcs12"
                storePassword = System.getenv("MORPH_ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("MORPH_ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("MORPH_ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        val signing = signingConfigs.findByName("morphTest") ?: signingConfigs.getByName("debug")
        debug {
            signingConfig = signing
        }
        release {
            // No Play release key yet: the test key (or the debug key).
            signingConfig = signing
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}

dependencies {
    // Profile API for per-profile WebView storage (see WebViewProfiles.kt).
    // Same version flutter_inappwebview_android already ships.
    implementation("androidx.webkit:webkit:1.12.0")
}
