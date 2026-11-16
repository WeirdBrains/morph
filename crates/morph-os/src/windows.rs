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
/// Settings > Default apps, on Morph's page.
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
/// `weirdnet-morph://` handler.
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
    ]
}

/// Whether the user's choices (the ProgIDs for http and https) are Morph's.
pub fn is_default(http: Option<&str>, https: Option<&str>) -> bool {
    http.is_some_and(|p| p.eq_ignore_ascii_case(PROG_ID)) && https.is_some_and(|p| p.eq_ignore_ascii_case(PROG_ID))
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

    /// Open Settings > Default apps on Morph's page (or the list).
    pub fn open_settings() -> Result<(), String> {
        let open = |uri: &str| std::process::Command::new("explorer.exe").arg(uri).spawn().map(|_| ());
        open(SETTINGS_URI).or_else(|_| open(SETTINGS_FALLBACK_URI)).map_err(|e| format!("could not open Windows Settings: {e}"))
    }
}

#[cfg(windows)]
pub use imp::{open_settings, register, user_choice};

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
    }

    #[test]
    fn morph_is_default_only_for_both_schemes() {
        assert!(is_default(Some("MorphHTML"), Some("morphhtml")));
        assert!(!is_default(Some("MorphHTML"), Some("ChromeHTML")));
        assert!(!is_default(None, Some("MorphHTML")));
        assert_eq!(user_choice_key("https"), r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice");
        assert!(SETTINGS_URI.starts_with(SETTINGS_FALLBACK_URI) && SETTINGS_URI.ends_with("registeredAppUser=Morph"));
    }
}
