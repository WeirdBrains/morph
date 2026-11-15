//! "Make Morph the default browser" on Linux: `xdg-settings` with the
//! `morph.desktop` entry the packages install (its `MimeType` lists
//! `x-scheme-handler/http`, `https` and `text/html`). The command runner is
//! a parameter so the rules are tested without touching the desktop.

/// Why there is no answer on macOS and Windows: Morph does not register
/// as a browser there yet (no URL handler in the app bundle or registry).
pub const OTHER_SYSTEMS: &str = "Morph cannot be the default browser on this system yet";

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

/// The real runner: run the command and capture its output. Off Linux
/// there is no `xdg-settings`; the system settings choose the browser.
pub fn system(program: &str, args: &[&str]) -> Result<String, String> {
    if !cfg!(target_os = "linux") {
        return Err(OTHER_SYSTEMS.into());
    }
    let out = std::process::Command::new(program).args(args).output().map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { format!("{program} failed ({})", out.status) } else { format!("{err} ({})", out.status) })
    }
}

/// A runner a background thread can own.
pub type SystemRunner = fn(&str, &[&str]) -> Result<String, String>;

/// `xdg-settings` off the UI thread: it is a shell script that can take
/// seconds (it probes the desktop), and the window must keep answering keys
/// meanwhile. [`Check::take`] gives the result once it is there.
pub struct Check {
    rx: std::sync::mpsc::Receiver<(Status, Option<Result<(), String>>)>,
}

impl Check {
    /// Read the current default browser.
    pub fn status(run: SystemRunner) -> Self {
        Self::spawn(move || (status(&run), None))
    }

    /// Make Morph the default, then read the result back.
    pub fn make_default(run: SystemRunner) -> Self {
        Self::spawn(move || {
            let made = make_default(&run);
            (status(&run), Some(made))
        })
    }

    fn spawn(work: impl FnOnce() -> (Status, Option<Result<(), String>>) + Send + 'static) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        Self { rx }
    }

    /// The status (and, for [`Check::make_default`], whether it worked) once
    /// the command finished; `None` while it runs.
    pub fn take(&self) -> Option<(Status, Option<Result<(), String>>)> {
        self.rx.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn wait(check: Check) -> (Status, Option<Result<(), String>>) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(done) = check.take() {
                return done;
            }
            assert!(std::time::Instant::now() < end, "the check never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn checks_run_off_the_calling_thread() {
        fn slow(_: &str, a: &[&str]) -> Result<String, String> {
            std::thread::sleep(std::time::Duration::from_millis(200));
            Ok(if a[0] == "get" { "morph.desktop".into() } else { String::new() })
        }
        let started = std::time::Instant::now();
        let check = Check::make_default(slow);
        assert!(check.take().is_none());
        assert!(started.elapsed() < std::time::Duration::from_millis(100), "the caller waited for xdg-settings");
        assert_eq!(wait(check), (Status::Default, Some(Ok(()))));
        assert_eq!(wait(Check::status(slow)), (Status::Default, None));
    }

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
