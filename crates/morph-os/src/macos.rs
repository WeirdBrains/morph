//! Morph as the default browser on macOS. The app bundle declares the
//! http/https URL types and HTML documents (packaging/macos/Info.plist), so
//! System Settings lists it; Morph asks to become the default with
//! NSWorkspace (macOS 12+; the system confirms with the user) and falls
//! back to Launch Services. Which app opens links is read with
//! NSWorkspace's URLForApplicationToOpenURL (Launch Services before 12).

/// Morph's bundle identifier (stable across versions; Info.plist).
pub const BUNDLE_ID: &str = "com.weirdbrains.morph";

/// What Morph says when it runs outside its app bundle.
pub const NOT_BUNDLED: &str = "Open Morph from Morph.app to make it the default browser";

/// Whether the bundle id Launch Services names is Morph's.
pub fn is_morph(bundle_id: Option<&str>) -> bool {
    bundle_id.is_some_and(|b| b.eq_ignore_ascii_case(BUNDLE_ID))
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send, sel};
    use std::time::Duration;

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn LSCopyDefaultHandlerForURLScheme(scheme: CFStringRef) -> CFStringRef;
        fn LSSetDefaultHandlerForURLScheme(scheme: CFStringRef, handler: CFStringRef) -> i32;
    }

    fn ns_string(text: &str) -> Option<Retained<AnyObject>> {
        let c = std::ffi::CString::new(text).ok()?;
        // SAFETY: stringWithUTF8String: copies the NUL-terminated buffer.
        unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
    }

    fn to_string(ns: &AnyObject) -> Option<String> {
        // SAFETY: UTF8String returns a buffer owned by the string, read at once.
        let ptr: *const std::ffi::c_char = unsafe { msg_send![ns, UTF8String] };
        (!ptr.is_null()).then(|| unsafe { std::ffi::CStr::from_ptr(ptr) }.to_string_lossy().into_owned())
    }

    /// This app's bundle URL, when it runs from an app bundle that is Morph.
    fn own_bundle_url() -> Option<Retained<AnyObject>> {
        // SAFETY: plain NSBundle getters.
        unsafe {
            let bundle: Option<Retained<AnyObject>> = msg_send![class!(NSBundle), mainBundle];
            let bundle = bundle?;
            let id: Option<Retained<AnyObject>> = msg_send![&*bundle, bundleIdentifier];
            if !is_morph(id.as_deref().and_then(to_string).as_deref()) {
                return None;
            }
            msg_send![&*bundle, bundleURL]
        }
    }

    fn workspace() -> Option<Retained<AnyObject>> {
        // SAFETY: the shared workspace singleton.
        unsafe { msg_send![class!(NSWorkspace), sharedWorkspace] }
    }

    /// The bundle id of the app that opens `scheme` links.
    fn handler(scheme: &str) -> Option<String> {
        // NSWorkspace first (URLForApplicationToOpenURL:).
        if let (Some(ws), Some(text)) = (workspace(), ns_string(&format!("{scheme}://example.com/"))) {
            // SAFETY: NSURL and NSWorkspace getters; nil results are handled.
            unsafe {
                let url: Option<Retained<AnyObject>> = msg_send![class!(NSURL), URLWithString: &*text];
                if let Some(url) = url {
                    let app: Option<Retained<AnyObject>> = msg_send![&*ws, URLForApplicationToOpenURL: &*url];
                    if let Some(app) = app {
                        let bundle: Option<Retained<AnyObject>> = msg_send![class!(NSBundle), bundleWithURL: &*app];
                        let id: Option<Retained<AnyObject>> = bundle.and_then(|b| msg_send![&*b, bundleIdentifier]);
                        if let Some(id) = id.as_deref().and_then(to_string) {
                            return Some(id);
                        }
                    }
                }
            }
        }
        let scheme = CFString::new(scheme);
        // SAFETY: the Copy rule: we own the returned string.
        let got = unsafe { LSCopyDefaultHandlerForURLScheme(scheme.as_concrete_TypeRef()) };
        (!got.is_null()).then(|| unsafe { CFString::wrap_under_create_rule(got) }.to_string())
    }

    /// Whether Morph opens http and https links.
    pub fn is_default() -> bool {
        is_morph(handler("http").as_deref()) && is_morph(handler("https").as_deref())
    }

    /// Ask to make Morph the default for http and https. The system asks the
    /// user to confirm; this waits for the answer up to `limit`.
    pub fn make_default(limit: Duration) -> Result<(), String> {
        let Some(app) = own_bundle_url() else {
            return Err(NOT_BUNDLED.into());
        };
        let ws = workspace().ok_or("no NSWorkspace")?;
        let modern: Bool = unsafe { msg_send![&*ws, respondsToSelector: sel!(setDefaultApplicationAtURL:toOpenURLsWithScheme:completionHandler:)] };
        if modern.as_bool() {
            for scheme in ["http", "https"] {
                let (tx, rx) = std::sync::mpsc::channel();
                let done = block2::RcBlock::new(move |error: *mut AnyObject| {
                    let _ = tx.send(error.is_null());
                });
                let s = ns_string(scheme).ok_or("no scheme string")?;
                // SAFETY: the block lives until the call returns; the
                // workspace copies it for the completion.
                unsafe {
                    let _: () = msg_send![&*ws, setDefaultApplicationAtURL: &*app, toOpenURLsWithScheme: &*s, completionHandler: &*done];
                }
                match rx.recv_timeout(limit) {
                    Ok(true) => {}
                    Ok(false) => return Err("macOS kept the other browser (the change was declined)".into()),
                    Err(_) => return Err(format!("macOS did not answer within {} s", limit.as_secs())),
                }
            }
            return Ok(());
        }
        let id = CFString::new(BUNDLE_ID);
        for scheme in ["http", "https"] {
            let s = CFString::new(scheme);
            // SAFETY: both strings outlive the call.
            let status = unsafe { LSSetDefaultHandlerForURLScheme(s.as_concrete_TypeRef(), id.as_concrete_TypeRef()) };
            if status != 0 {
                return Err(format!("Launch Services refused ({status})"));
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub use imp::{is_default, make_default};

#[cfg(test)]
mod tests {
    #[test]
    fn only_morphs_bundle_id_counts() {
        assert!(super::is_morph(Some("com.weirdbrains.morph")));
        assert!(super::is_morph(Some("com.WeirdBrains.Morph")));
        assert!(!super::is_morph(Some("com.apple.Safari")));
        assert!(!super::is_morph(None));
    }

    #[test]
    fn the_bundle_id_matches_info_plist() {
        let plist = include_str!("../../../packaging/macos/Info.plist");
        assert!(plist.contains(&format!("<string>{}</string>", super::BUNDLE_ID)));
    }
}
