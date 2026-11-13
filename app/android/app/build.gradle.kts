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
// passes them as -Ptarget-platform). CameraX ships native libraries for every
// ABI; an arm64 APK that also carries x86_64 or armeabi-v7a folders makes
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
        ndk { abiFilters += flutterAbis }
    }

    // Native libraries are compressed inside the APK (and extracted on
    // install). Stored uncompressed they were 18 MB of a 25 MB budget.
    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }

    buildTypes {
        release {
            // TODO: Add your own signing config for the release build.
            // Signing with the debug keys for now, so `flutter run --release` works.
            signingConfig = signingConfigs.getByName("debug")
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
    // QR login scanner (QrScanActivity): CameraX for the camera, ZXing core
    // for decoding. Both FOSS, so builds stay F-Droid friendly.
    implementation("androidx.activity:activity:1.10.1")
    implementation("androidx.camera:camera-camera2:1.4.2")
    implementation("androidx.camera:camera-lifecycle:1.4.2")
    implementation("androidx.camera:camera-view:1.4.2")
    implementation("com.google.zxing:core:3.5.3")
    testImplementation("junit:junit:4.13.2")
}
