//! Morph as a browser on Windows. Windows 10 and 11 let no app make itself
//! the default: an app registers *capabilities* (URL and file associations
//! under its `StartMenuInternet` key, listed in `RegisteredApplications`)
//! and the user picks it in Settings > Default apps, which Morph opens for
//! them. The MSI writes the same keys under HKLM for every user
//! (packaging/windows/morph.wxs); a zip install gets them under HKCU here.
//!
//! The key list is plain data so it is tested on every system; only the
//! writing and reading are Windows code.

/// The ProgID http, https, .htm and .html point at.
pub const PROG_ID: &str = "MorphHTML";
/// Morph's name under `StartMenuInternet` and in `RegisteredApplications`.
pub const APP_NAME: &str = "Morph";
/// Settings > Default apps, on Morph's page. `registeredAppUser` must be
/// the value name under `RegisteredApplications` ([`APP_NAME`]), or Settings
/// ignores it.
pub const SETTINGS_URI: &str = "ms-settings:defaultapps?registeredAppUser=Morph";
/// Settings > Default apps (older Windows ignore the app parameter).
pub const SETTINGS_FALLBACK_URI: &str = "ms-settings:defaultapps";
/// What Morph says once Settings is open.
pub const CHOOSE_IN_SETTINGS: &str = "Choose Morph in Windows Settings (Default apps), then come back.";
/// Where Windows keeps the user's choice for a scheme.
pub const USER_CHOICE: &str = r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\{scheme}\UserChoice";

const CLIENT: &str = r"Software\Clients\StartMenuInternet\Morph";

/// One registry value: key path under HKCU (or HKLM), value name (`""` for
/// the key's default value) and data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    pub key: String,
    pub name: String,
    pub data: String,
}

fn value(key: &str, name: &str, data: &str) -> Value {
    Value { key: key.to_string(), name: name.to_string(), data: data.to_string() }
}

/// Every value that registers Morph (at `exe`) as a browser and as the
/// `weirdnet-morph://` handler, and the `weirdnet-ymir://` one (Ymir is a
/// headless node app; its device-link codes open in Morph).
pub fn registration(exe: &str) -> Vec<Value> {
    let icon = format!("{exe},0");
    let open = format!("\"{exe}\" \"%1\"");
    let caps = format!(r"{CLIENT}\Capabilities");
    vec![
        value(CLIENT, "", APP_NAME),
        value(&format!(r"{CLIENT}\DefaultIcon"), "", &icon),
        value(&format!(r"{CLIENT}\shell\open\command"), "", &format!("\"{exe}\"")),
        value(&caps, "ApplicationName", APP_NAME),
        value(&caps, "ApplicationDescription", "A browser that keeps your data yours."),
        value(&caps, "ApplicationIcon", &icon),
        value(&format!(r"{caps}\StartMenu"), "StartMenuInternet", APP_NAME),
        value(&format!(r"{caps}\URLAssociations"), "http", PROG_ID),
        value(&format!(r"{caps}\URLAssociations"), "https", PROG_ID),
        value(&format!(r"{caps}\URLAssociations"), "weirdnet-morph", "weirdnet-morph"),
        value(&format!(r"{caps}\URLAssociations"), "weirdnet-ymir", "weirdnet-ymir"),
        value(&format!(r"{caps}\FileAssociations"), ".htm", PROG_ID),
        value(&format!(r"{caps}\FileAssociations"), ".html", PROG_ID),
        value(r"Software\RegisteredApplications", APP_NAME, &caps),
        value(&format!(r"Software\Classes\{PROG_ID}"), "", "Morph HTML Document"),
        value(&format!(r"Software\Classes\{PROG_ID}\DefaultIcon"), "", &icon),
        value(&format!(r"Software\Classes\{PROG_ID}\shell\open\command"), "", &open),
        value(r"Software\Classes\weirdnet-morph", "", "URL:weirdnet-morph"),
        value(r"Software\Classes\weirdnet-morph", "URL Protocol", ""),
        value(r"Software\Classes\weirdnet-morph\DefaultIcon", "", &icon),
        value(r"Software\Classes\weirdnet-morph\shell\open\command", "", &open),
        value(r"Software\Classes\weirdnet-ymir", "", "URL:weirdnet-ymir"),
        value(r"Software\Classes\weirdnet-ymir", "URL Protocol", ""),
        value(r"Software\Classes\weirdnet-ymir\DefaultIcon", "", &icon),
        value(r"Software\Classes\weirdnet-ymir\shell\open\command", "", &open),
    ]
}

/// Whether the user's choices (the ProgIDs for http and https) are Morph's.
pub fn is_default(http: Option<&str>, https: Option<&str>) -> bool {
    http.is_some_and(|p| p.eq_ignore_ascii_case(PROG_ID)) && https.is_some_and(|p| p.eq_ignore_ascii_case(PROG_ID))
}

/// `s` as NUL-terminated UTF-16, for the wide Windows calls.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Whether ShellExecuteW's result means it launched (above 32; 32 and
/// below are error codes).
pub fn launched(code: isize) -> bool {
    code > 32
}

/// The user's choice for `scheme`'s key path.
pub fn user_choice_key(scheme: &str) -> String {
    USER_CHOICE.replace("{scheme}", scheme)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};

    /// Write [`registration`] for this executable under HKCU and tell the
    /// shell associations changed.
    pub fn register() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| format!("where is morph.exe? {e}"))?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for v in registration(&exe.to_string_lossy()) {
            let (key, _) = hkcu.create_subkey(&v.key).map_err(|e| format!("could not write {}: {e}", v.key))?;
            key.set_value(&v.name, &v.data).map_err(|e| format!("could not write {}\\{}: {e}", v.key, v.name))?;
        }
        // SAFETY: SHCNE_ASSOCCHANGED takes no items; both pointers may be null.
        unsafe {
            windows_sys::Win32::UI::Shell::SHChangeNotify(
                windows_sys::Win32::UI::Shell::SHCNE_ASSOCCHANGED as i32,
                windows_sys::Win32::UI::Shell::SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            );
        }
        Ok(())
    }

    /// The ProgID the user chose for `scheme`.
    pub fn user_choice(scheme: &str) -> Option<String> {
        RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(user_choice_key(scheme), KEY_READ).ok()?.get_value("ProgId").ok()
    }

    /// Launch `uri` with the shell (ShellExecuteW, verb `open`), the way
    /// Start > Run does. Not through explorer.exe: it treats the argument as
    /// a path and garbles the query, so Settings never opened and Explorer
    /// searched Documents for it instead.
    fn launch(uri: &str) -> Result<(), String> {
        let verb = wide("open");
        let file = wide(uri);
        // SAFETY: both strings are NUL-terminated UTF-16 that outlive the
        // call; no window, parameters or directory.
        let code = unsafe {
            windows_sys::Win32::UI::Shell::ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
            )
        } as isize;
        if launched(code) { Ok(()) } else { Err(format!("ShellExecuteW({uri}) failed with {code}")) }
    }

    /// Restore the top-level window `hwnd` from maximized (GPUI's zoom on
    /// Windows only maximizes).
    pub fn restore_window(hwnd: isize) {
        // SAFETY: ShowWindowAsync takes any window handle; a stale one fails.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::ShowWindowAsync(hwnd as _, windows_sys::Win32::UI::WindowsAndMessaging::SW_RESTORE);
        }
    }

    /// Open Settings > Default apps on Morph's page (or the list, when that
    /// launch fails).
    pub fn open_settings() -> Result<(), String> {
        launch(SETTINGS_URI).or_else(|_| launch(SETTINGS_FALLBACK_URI)).map_err(|e| format!("could not open Windows Settings: {e}"))
    }
}

#[cfg(windows)]
pub use imp::{open_settings, register, restore_window, user_choice};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn morph_registers_capabilities_classes_and_the_protocol() {
        let exe = r"C:\Program Files\Morph\morph.exe";
        let values = registration(exe);
        let get = |key: &str, name: &str| values.iter().find(|v| v.key == key && v.name == name).map(|v| v.data.clone());
        let caps = r"Software\Clients\StartMenuInternet\Morph\Capabilities";
        assert_eq!(get(&format!(r"{caps}\URLAssociations"), "http").as_deref(), Some(PROG_ID));
        assert_eq!(get(&format!(r"{caps}\URLAssociations"), "https").as_deref(), Some(PROG_ID));
        assert_eq!(get(&format!(r"{caps}\FileAssociations"), ".html").as_deref(), Some(PROG_ID));
        assert_eq!(get(&format!(r"{caps}\FileAssociations"), ".htm").as_deref(), Some(PROG_ID));
        assert_eq!(get(r"Software\RegisteredApplications", "Morph").as_deref(), Some(caps));
        assert_eq!(get(r"Software\Classes\MorphHTML\shell\open\command", "").unwrap(), format!("\"{exe}\" \"%1\""));
        assert_eq!(get(r"Software\Classes\weirdnet-morph", "URL Protocol").as_deref(), Some(""));
        assert!(get(r"Software\Classes\weirdnet-morph\shell\open\command", "").unwrap().ends_with("\"%1\""));
        assert_eq!(get(r"Software\Classes\weirdnet-ymir", "URL Protocol").as_deref(), Some(""));
        assert!(get(r"Software\Classes\weirdnet-ymir\shell\open\command", "").unwrap().ends_with("\"%1\""));
    }

    #[test]
    fn morph_is_default_only_for_both_schemes() {
        assert!(is_default(Some("MorphHTML"), Some("morphhtml")));
        assert!(!is_default(Some("MorphHTML"), Some("ChromeHTML")));
        assert!(!is_default(None, Some("MorphHTML")));
        assert_eq!(user_choice_key("https"), r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice");
        assert!(SETTINGS_URI.starts_with(SETTINGS_FALLBACK_URI) && SETTINGS_URI.ends_with("registeredAppUser=Morph"));
    }

    #[test]
    fn settings_opens_on_the_name_morph_registers() {
        // Settings matches registeredAppUser against the value names under
        // RegisteredApplications, from here (HKCU) and from the MSI (HKLM).
        let (_, param) = SETTINGS_URI.split_once("?registeredAppUser=").unwrap();
        let registered: Vec<_> = registration(r"C:\m.exe").into_iter().filter(|v| v.key == r"Software\RegisteredApplications").collect();
        assert_eq!(registered.len(), 1);
        assert_eq!(registered[0].name, param);
        assert_eq!(param, APP_NAME);
        let wxs = include_str!("../../../packaging/windows/morph.wxs");
        assert!(wxs.contains(&format!(r#"Key="Software\RegisteredApplications" Name="{param}""#)), "the MSI registers another name");
    }

    #[test]
    fn shell_launch_results_and_wide_strings() {
        assert!(launched(42));
        assert!(!launched(32) && !launched(2) && !launched(0));
        assert_eq!(wide("ms"), vec![b'm' as u16, b's' as u16, 0]);
    }
}
