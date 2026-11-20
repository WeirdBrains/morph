//! Asking the operating system who is at the keyboard: the password saver's
//! proof on a profile whose password is off (`docs/profile-lock.md`,
//! "Password saver"). Every call blocks until the user answers, so callers
//! run it off the UI thread.
//!
//! * **Linux:** polkit. Morph's action ([`POLKIT_ACTION`], installed by the
//!   packages from `packaging/linux/com.weirdbrains.morph.policy`) asks for
//!   the user's own password (`auth_self`, never kept) through the
//!   desktop's polkit agent; `pkcheck` reports the answer. Without polkit,
//!   the action or an agent there is no system authentication, and the
//!   saver is not offered on such profiles.
//! * **macOS:** LocalAuthentication (`LAPolicyDeviceOwnerAuthentication`):
//!   Touch ID, a paired watch, or the account password.
//! * **Windows:** Windows Hello (`UserConsentVerifier`: face, fingerprint or
//!   PIN). Where Hello is not set up, the Windows credential dialog
//!   (CredUI), whose answer is checked with `LogonUser` and must be the
//!   account Morph runs as.
//!
//! Nothing here sees a password longer than the check needs: polkit and
//! LocalAuthentication never hand one over, and the CredUI buffers are
//! zeroed after `LogonUser`.

use std::time::Duration;

/// How long a system prompt may stay open before Morph stops waiting.
pub const PROMPT_LIMIT: Duration = Duration::from_secs(180);

/// Morph's polkit action (packaging/linux/com.weirdbrains.morph.policy).
pub const POLKIT_ACTION: &str = "com.weirdbrains.morph.authenticate";

/// Why the system did not vouch for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// There is no way to ask here (no polkit action or agent, no Touch ID
    /// or password policy, Hello and CredUI both missing). The text says why.
    Unavailable(String),
    /// The user cancelled, or did not authenticate.
    Denied,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Unavailable(why) => write!(f, "system authentication is not available: {why}"),
            Failure::Denied => f.write_str("authentication was cancelled or failed"),
        }
    }
}

/// What `pkcheck` exiting with `code` means (polkit's documented codes:
/// 0 authorized, 1 not authorized, 2 needs a challenge, 3 dismissed, 4 error).
pub fn pkcheck_outcome(code: Option<i32>) -> Result<(), Failure> {
    match code {
        Some(0) => Ok(()),
        Some(1..=3) => Err(Failure::Denied),
        Some(c) => Err(Failure::Unavailable(format!("pkcheck failed ({c})"))),
        None => Err(Failure::Unavailable("pkcheck was stopped".into())),
    }
}

/// The `--process` argument for `pkcheck`: `pid,start-time,uid`, the form
/// polkit takes without a race on a reused pid. `stat` and `status` are the
/// contents of `/proc/<pid>/stat` and `/proc/<pid>/status`.
pub fn pkcheck_subject(pid: u32, stat: &str, status: &str) -> Option<String> {
    // The command name (field 2) may hold spaces and parentheses: fields
    // after it start after the last ')'. Start time is field 22.
    let after = stat.rsplit_once(')')?.1;
    let start: u64 = after.split_whitespace().nth(19)?.parse().ok()?;
    let uid: u32 = status.lines().find_map(|l| l.strip_prefix("Uid:"))?.split_whitespace().next()?.parse().ok()?;
    Some(format!("{pid},{start},{uid}"))
}

/// Whether this system can ask the user to authenticate. Cheap after the
/// first call (the answer is kept for the process).
pub fn available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(imp::available)
}

/// Ask the user to authenticate; `reason` says why in the system prompt
/// where the system shows one (macOS, Windows; polkit shows the action's
/// own message). Blocks until the user answers.
pub fn authenticate(reason: &str) -> Result<(), Failure> {
    imp::authenticate(reason)
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
mod imp {
    use super::*;

    fn pkcheck_found() -> bool {
        std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join("pkcheck").is_file()))
    }

    pub fn available() -> bool {
        pkcheck_found() && crate::run("pkaction", &["--action-id", POLKIT_ACTION], crate::DESKTOP_CALL_LIMIT).is_ok()
    }

    pub fn authenticate(_reason: &str) -> Result<(), Failure> {
        if !pkcheck_found() {
            return Err(Failure::Unavailable("polkit (pkcheck) is not installed".into()));
        }
        let pid = std::process::id();
        let read = |name: &str| std::fs::read_to_string(format!("/proc/{pid}/{name}")).unwrap_or_default();
        let subject =
            pkcheck_subject(pid, &read("stat"), &read("status")).ok_or_else(|| Failure::Unavailable("cannot name this process to polkit".into()))?;
        let mut child = std::process::Command::new("pkcheck")
            .args(["--action-id", POLKIT_ACTION, "--process", &subject, "--allow-user-interaction"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| Failure::Unavailable(format!("pkcheck: {e}")))?;
        let end = std::time::Instant::now() + PROMPT_LIMIT;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return pkcheck_outcome(status.code()),
                Ok(None) if std::time::Instant::now() >= end => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Failure::Denied);
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => return Err(Failure::Unavailable(format!("pkcheck: {e}"))),
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};

    /// LAPolicyDeviceOwnerAuthentication: biometrics, watch or password.
    const POLICY: isize = 2;

    #[link(name = "LocalAuthentication", kind = "framework")]
    unsafe extern "C" {}

    fn context() -> Option<Retained<AnyObject>> {
        // SAFETY: LAContext's designated initializer, through `new`.
        unsafe { msg_send![class!(LAContext), new] }
    }

    pub fn available() -> bool {
        let Some(ctx) = context() else { return false };
        // SAFETY: a nil error out-pointer is allowed.
        let ok: Bool = unsafe { msg_send![&*ctx, canEvaluatePolicy: POLICY, error: std::ptr::null_mut::<*mut AnyObject>()] };
        ok.as_bool()
    }

    pub fn authenticate(reason: &str) -> Result<(), Failure> {
        let ctx = context().ok_or_else(|| Failure::Unavailable("no LocalAuthentication".into()))?;
        let text = std::ffi::CString::new(reason.replace('\0', "")).map_err(|_| Failure::Denied)?;
        // SAFETY: stringWithUTF8String: copies the NUL-terminated buffer.
        let reason: Option<Retained<AnyObject>> = unsafe { msg_send![class!(NSString), stringWithUTF8String: text.as_ptr()] };
        let reason = reason.ok_or_else(|| Failure::Unavailable("no reason string".into()))?;
        let (tx, rx) = std::sync::mpsc::channel();
        let reply = block2::RcBlock::new(move |success: Bool, _error: *mut AnyObject| {
            let _ = tx.send(success.as_bool());
        });
        // SAFETY: the context copies the block; the reply runs once on a
        // private queue and only sends on the channel.
        unsafe {
            let _: () = msg_send![&*ctx, evaluatePolicy: POLICY, localizedReason: &*reason, reply: &*reply];
        }
        match rx.recv_timeout(PROMPT_LIMIT) {
            Ok(true) => Ok(()),
            Ok(false) => Err(Failure::Denied),
            Err(_) => {
                // SAFETY: invalidate cancels a prompt still showing.
                unsafe {
                    let _: () = msg_send![&*ctx, invalidate];
                }
                Err(Failure::Denied)
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::Security::Credentials::UI::{UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability};
    use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
    use windows::core::HSTRING;
    use windows_future::IAsyncOperation;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::Credentials::{
        CREDUI_INFOW, CREDUIWIN_ENUMERATE_CURRENT_USER, CREDUIWIN_GENERIC, CredUIPromptForWindowsCredentialsW, CredUnPackAuthenticationBufferW,
    };
    use windows_sys::Win32::Security::{
        EqualSid, GetTokenInformation, LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT, LogonUserW, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    /// ERROR_CANCELLED from CredUI.
    const CANCELLED: u32 = 1223;

    fn com() {
        // SAFETY: per-thread COM setup; a second call on the thread is harmless.
        unsafe {
            CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32);
        }
    }

    fn hello_available() -> bool {
        com();
        UserConsentVerifier::CheckAvailabilityAsync().and_then(|op| op.join()).is_ok_and(|a| a == UserConsentVerifierAvailability::Available)
    }

    pub fn available() -> bool {
        // CredUI is part of every desktop Windows: the fallback is always there.
        true
    }

    pub fn authenticate(reason: &str) -> Result<(), Failure> {
        if hello_available() {
            return hello(reason);
        }
        cred_ui(reason)
    }

    fn hello(reason: &str) -> Result<(), Failure> {
        let fail = |e: windows::core::Error| Failure::Unavailable(format!("Windows Hello: {e}"));
        // SAFETY: the foreground window (Morph's, as the user just asked) owns the prompt.
        let window = windows::Win32::Foundation::HWND(unsafe { GetForegroundWindow() });
        let interop = windows::core::factory::<UserConsentVerifier, IUserConsentVerifierInterop>().map_err(fail)?;
        // SAFETY: a valid (or null) window handle and a live message string.
        let op: IAsyncOperation<UserConsentVerificationResult> =
            unsafe { interop.RequestVerificationForWindowAsync(window, &HSTRING::from(reason)) }.map_err(fail)?;
        match op.join().map_err(fail)? {
            UserConsentVerificationResult::Verified => Ok(()),
            _ => Err(Failure::Denied),
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The user SID of `token`, as bytes.
    fn token_user(token: HANDLE) -> Option<Vec<u8>> {
        let mut len = 0u32;
        // SAFETY: the first call only reports the size.
        unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len) };
        let mut buf = vec![0u8; len as usize];
        // SAFETY: `buf` is `len` bytes.
        (unsafe { GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) } != 0).then_some(buf)
    }

    /// Whether the two TOKEN_USER buffers name the same account.
    fn same_user(a: &[u8], b: &[u8]) -> bool {
        // SAFETY: both buffers are TOKEN_USER structures filled by GetTokenInformation.
        unsafe {
            let a = &*(a.as_ptr() as *const TOKEN_USER);
            let b = &*(b.as_ptr() as *const TOKEN_USER);
            EqualSid(a.User.Sid, b.User.Sid) != 0
        }
    }

    fn cred_ui(reason: &str) -> Result<(), Failure> {
        let (caption, message) = (wide("Morph"), wide(reason));
        let info = CREDUI_INFOW {
            cbSize: std::mem::size_of::<CREDUI_INFOW>() as u32,
            // SAFETY: plain getter.
            hwndParent: unsafe { GetForegroundWindow() },
            pszMessageText: message.as_ptr(),
            pszCaptionText: caption.as_ptr(),
            hbmBanner: std::ptr::null_mut(),
        };
        let (mut package, mut out, mut out_len, mut save) = (0u32, std::ptr::null_mut(), 0u32, 0);
        // SAFETY: every out-pointer is a live local; the buffer is freed below.
        let status = unsafe {
            CredUIPromptForWindowsCredentialsW(
                &info,
                0,
                &mut package,
                std::ptr::null(),
                0,
                &mut out,
                &mut out_len,
                &mut save,
                CREDUIWIN_GENERIC | CREDUIWIN_ENUMERATE_CURRENT_USER,
            )
        };
        if status == CANCELLED {
            return Err(Failure::Denied);
        }
        if status != 0 {
            return Err(Failure::Unavailable(format!("the Windows credential dialog failed ({status})")));
        }
        let (mut user, mut domain, mut pass) = (vec![0u16; 514], vec![0u16; 340], vec![0u16; 1024]);
        let (mut ul, mut dl, mut pl) = (user.len() as u32, domain.len() as u32, pass.len() as u32);
        // SAFETY: the buffers and their lengths match; `out` is CredUI's.
        let unpacked = unsafe {
            CredUnPackAuthenticationBufferW(0, out, out_len, user.as_mut_ptr(), &mut ul, domain.as_mut_ptr(), &mut dl, pass.as_mut_ptr(), &mut pl)
        } != 0;
        // SAFETY: zero CredUI's copy before handing it back.
        unsafe {
            std::ptr::write_bytes(out.cast::<u8>(), 0, out_len as usize);
            CoTaskMemFree(out);
        }
        let result = if !unpacked {
            Err(Failure::Denied)
        } else {
            let domain_ptr = if dl > 1 { domain.as_ptr() } else { std::ptr::null() };
            let mut token: HANDLE = std::ptr::null_mut();
            // SAFETY: NUL-terminated buffers from CredUnPack; the token is closed below.
            let ok =
                unsafe { LogonUserW(user.as_ptr(), domain_ptr, pass.as_ptr(), LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT, &mut token) } != 0;
            if ok {
                let mut own: HANDLE = std::ptr::null_mut();
                // SAFETY: the current process's token, opened to read and closed below.
                let same = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut own) } != 0
                    && matches!((token_user(token), token_user(own)), (Some(a), Some(b)) if same_user(&a, &b));
                // SAFETY: both handles are ours.
                unsafe {
                    CloseHandle(token);
                    if !own.is_null() {
                        CloseHandle(own);
                    }
                }
                if same { Ok(()) } else { Err(Failure::Denied) }
            } else {
                Err(Failure::Denied)
            }
        };
        pass.iter_mut().for_each(|c| *c = 0);
        // Keep the zeroing from being optimized away.
        std::hint::black_box(&pass);
        result
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
mod imp {
    use super::*;

    pub fn available() -> bool {
        false
    }

    pub fn authenticate(_reason: &str) -> Result<(), Failure> {
        Err(Failure::Unavailable("the phone app asks through its own biometric prompt".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkcheck_codes_map_to_answers() {
        assert_eq!(pkcheck_outcome(Some(0)), Ok(()));
        for denied in [1, 2, 3] {
            assert_eq!(pkcheck_outcome(Some(denied)), Err(Failure::Denied));
        }
        assert!(matches!(pkcheck_outcome(Some(4)), Err(Failure::Unavailable(_))));
        assert!(matches!(pkcheck_outcome(None), Err(Failure::Unavailable(_))));
    }

    #[test]
    fn the_polkit_subject_is_pid_start_time_and_uid() {
        // A command name with spaces and a parenthesis must not shift the fields.
        let stat = "4242 (morph (x) y) S 1 4242 4242 0 -1 4194560 100 0 0 0 5 3 0 0 20 0 9 0 987654 1000 200 18446744073709551615";
        let status = "Name:\tmorph\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\n";
        assert_eq!(pkcheck_subject(4242, stat, status).as_deref(), Some("4242,987654,1000"));
        assert_eq!(pkcheck_subject(1, "garbage", status), None);
        assert_eq!(pkcheck_subject(1, stat, "Name:\tx\n"), None);
    }

    #[test]
    fn the_policy_file_names_the_action_and_never_keeps_the_answer() {
        let policy = include_str!("../../../packaging/linux/com.weirdbrains.morph.policy");
        assert!(policy.contains(&format!("<action id=\"{POLKIT_ACTION}\">")));
        assert!(policy.contains("<allow_active>auth_self</allow_active>"));
        assert!(!policy.contains("_keep"), "every fill asks again");
        assert!(policy.contains("<allow_any>no</allow_any>"));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn this_process_has_a_polkit_subject() {
        let pid = std::process::id();
        let read = |n: &str| std::fs::read_to_string(format!("/proc/{pid}/{n}")).unwrap();
        let subject = pkcheck_subject(pid, &read("stat"), &read("status")).unwrap();
        assert!(subject.starts_with(&format!("{pid},")), "{subject}");
    }
}
