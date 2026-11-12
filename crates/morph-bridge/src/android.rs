//! Android only: gives ndk-context the process's JavaVM and application
//! Context. hickory-resolver (libp2p's DNS transport inside weirdnet-net) asks
//! the ConnectivityManager for DNS servers through it, and panics if it was
//! never set, which takes down every real-network node (QR device linking).
//! `RustContext.kt` calls this once, before Dart touches the bridge.
use std::sync::Once;

use jni_sys::{JNI_OK, JNIEnv, JavaVM, jclass, jobject};

static INIT: Once = Once::new();

/// # Safety
/// Called by the JVM with a valid `env` and a live `context` reference.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_org_weirdbrains_morph_RustContext_nativeInit(env: *mut JNIEnv, _class: jclass, context: jobject) {
    INIT.call_once(|| {
        // SAFETY: `env` is the calling thread's JNIEnv; the global reference
        // keeps the application Context alive for the life of the process,
        // which is what ndk-context expects.
        unsafe {
            let fns = &**env;
            let mut vm: *mut JavaVM = std::ptr::null_mut();
            if (fns.v1_2.GetJavaVM)(env, &mut vm) != JNI_OK || vm.is_null() {
                return;
            }
            let global = (fns.v1_2.NewGlobalRef)(env, context);
            if global.is_null() {
                return;
            }
            ndk_context::initialize_android_context(vm.cast(), global.cast());
        }
    });
}
