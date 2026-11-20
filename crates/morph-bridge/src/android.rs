//! Android only: the JNI init `RustContext.kt` calls once, before Dart touches the
//! bridge. It hands ndk-context the JavaVM and application Context, which
//! hickory-resolver (DNS inside weirdnet-net) and the Android keystore need.
weirdnet_net::android_context_init!(Java_org_weirdbrains_morph_RustContext_nativeInit);
