//! "Make Morph the default browser" on Linux: `xdg-settings` with the
//! `morph.desktop` entry the packages install (its `MimeType` lists
//! `x-scheme-handler/http`, `https` and `text/html`). The command runner is
//! a parameter so the rules are tested without touching the desktop.

/// The desktop entry the packages install.
pub const DESKTOP_ID: &str = "morph.desktop";

/// Runs a command; `Ok(stdout)` when it exited successfully, else
/// `Err(stderr or a description)`.
pub type Runner<'a> = &'a dyn Fn(&str, &[&str]) -> Result<String, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Morph opens links.
    Default,
    /// Another browser does (its desktop id).
    Other(String),
    /// The desktop cannot say (no `xdg-settings`, or it failed).
    Unknown(String),
}

/// Ask the desktop which browser opens links.
pub fn status(run: Runner) -> Status {
    match run("xdg-settings", &["get", "default-web-browser"]) {
        Ok(out) => {
            let id = out.trim();
            if id.eq_ignore_ascii_case(DESKTOP_ID) {
                Status::Default
            } else if id.is_empty() {
                Status::Unknown("no default browser is set".into())
            } else {
                Status::Other(id.to_string())
            }
        }
        Err(e) => Status::Unknown(e),
    }
}

/// Make Morph the default browser, then check that it took.
pub fn make_default(run: Runner) -> Result<(), String> {
    run("xdg-settings", &["set", "default-web-browser", DESKTOP_ID]).map_err(|e| explain(&e))?;
    match status(run) {
        Status::Default => Ok(()),
        Status::Other(id) => Err(format!("the desktop still opens links with {id}")),
        Status::Unknown(e) => Err(e),
    }
}

/// A friendlier word for xdg-settings' usual failures.
fn explain(error: &str) -> String {
    if error.contains("BROWSER is set") {
        "$BROWSER is set in your environment, which overrides the desktop's choice; unset it first".into()
    } else if error.contains("exit status: 2") {
        "the desktop could not find morph.desktop with a `morph` on PATH; install Morph's package first".into()
    } else {
        error.to_string()
    }
}

/// The real runner: run the command and capture its output.
pub fn system(program: &str, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new(program).args(args).output().map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { format!("{program} failed ({})", out.status) } else { format!("{err} ({})", out.status) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn status_reads_xdg_settings() {
        assert_eq!(status(&|_, _| Ok("morph.desktop\n".into())), Status::Default);
        assert_eq!(status(&|_, _| Ok("firefox.desktop\n".into())), Status::Other("firefox.desktop".into()));
        assert!(matches!(status(&|_, _| Ok("\n".into())), Status::Unknown(_)));
        assert_eq!(status(&|_, _| Err("xdg-settings: not found".into())), Status::Unknown("xdg-settings: not found".into()));
    }

    #[test]
    fn make_default_sets_then_checks() {
        let calls = RefCell::new(Vec::new());
        let current = RefCell::new("firefox.desktop".to_string());
        let run = |p: &str, a: &[&str]| -> Result<String, String> {
            calls.borrow_mut().push(format!("{p} {}", a.join(" ")));
            if a[0] == "set" {
                *current.borrow_mut() = a[2].to_string();
                Ok(String::new())
            } else {
                Ok(current.borrow().clone())
            }
        };
        assert_eq!(make_default(&run), Ok(()));
        assert_eq!(*calls.borrow(), ["xdg-settings set default-web-browser morph.desktop", "xdg-settings get default-web-browser"]);
    }

    #[test]
    fn failures_are_explained() {
        let err = make_default(&|_, _| Err("xdg-settings: $BROWSER is set and can't be changed with xdg-settings (exit status: 4)".into()));
        assert!(err.unwrap_err().contains("unset it"));
        let err = make_default(&|_, _| Err("(exit status: 2)".into()));
        assert!(err.unwrap_err().contains("install Morph's package"));
        let stubborn = |_: &str, a: &[&str]| -> Result<String, String> { Ok(if a[0] == "get" { "chromium.desktop".into() } else { String::new() }) };
        assert!(make_default(&stubborn).unwrap_err().contains("chromium.desktop"));
    }
}
