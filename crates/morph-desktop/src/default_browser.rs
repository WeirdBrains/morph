//! "Make Morph the default browser". On macOS and Windows the system
//! decides (crate `morph-os`: Launch Services, and Windows' Default apps
//! with Morph's registered capabilities). On Linux Morph writes the user's
//! `mimeapps.list` itself (the freedesktop MIME applications spec) instead of
//! calling `xdg-settings`, whose generic mode fails on desktops it does not
//! know (Hyprland: exit 4, with `mimeapps.list` left half-written). It sets
//! [`TYPES`] to `morph.desktop` in `$XDG_CONFIG_HOME/mimeapps.list` and in
//! every existing `$XDG_CONFIG_HOME/<desktop>-mimeapps.list` for the desktops
//! in `$XDG_CURRENT_DESKTOP` (those win over `mimeapps.list`), then asks
//! `xdg-mime` whether it took. The command runner is a parameter so the
//! rules are tested without touching the desktop.

use std::path::{Path, PathBuf};

/// What the settings say once Morph is the default.
pub const MADE_DEFAULT: &str = "Links from other apps now open in Morph.";

/// Why there is no answer on a system Morph has no default-browser path for.
pub const OTHER_SYSTEMS: &str = "Morph cannot be the default browser on this system yet";

/// What the settings say when the system did not answer in time.
pub const NO_ANSWER: &str = "The system did not answer; try again.";

/// The longest the settings wait for an answer (the calls inside have
/// their own limit, [`morph_os::DESKTOP_CALL_LIMIT`]).
const CHECK_LIMIT: std::time::Duration = std::time::Duration::from_secs(45);

/// The desktop entry the packages install.
pub const DESKTOP_ID: &str = "morph.desktop";

/// What a browser handles.
pub const TYPES: [&str; 4] = ["text/html", "application/xhtml+xml", "x-scheme-handler/http", "x-scheme-handler/https"];

const DEFAULTS: &str = "Default Applications";
const ADDED: &str = "Added Associations";

/// Runs a command; `Ok(stdout)` when it exited successfully, else
/// `Err(stderr or a description)`.
pub type Runner<'a> = &'a dyn Fn(&str, &[&str]) -> Result<String, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Morph opens links (http and https both).
    Default,
    /// Another browser does (its desktop id).
    Other(String),
    /// Nothing says (no default set, or no home).
    Unknown(String),
}

/// Where the lists live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    pub config: PathBuf,
    pub data: PathBuf,
    /// `$XDG_CURRENT_DESKTOP`, lowercased, in order.
    pub desktops: Vec<String>,
}

impl Dirs {
    /// The user's: XDG directories and the current desktops.
    pub fn system() -> Option<Self> {
        #[cfg(target_os = "linux")]
        let (data, config) = crate::legacy_desktop::xdg_dirs()?;
        // Elsewhere there are no lists: the system settings choose.
        #[cfg(not(target_os = "linux"))]
        let (data, config): (PathBuf, PathBuf) = None?;
        let desktops =
            std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().split(':').map(|d| d.trim().to_lowercase()).filter(|d| !d.is_empty()).collect();
        Some(Self { config, data, desktops })
    }

    /// The lists, most important first: each desktop's, then the common one.
    fn lists(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = self.desktops.iter().map(|d| self.config.join(format!("{d}-mimeapps.list"))).collect();
        out.push(self.config.join("mimeapps.list"));
        out
    }
}

/// The first desktop id `text`'s `[Default Applications]` gives `mime`.
fn default_in(text: &str, mime: &str) -> Option<String> {
    let mut section = "";
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name;
        } else if section == DEFAULTS
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == mime
        {
            return value.split(';').map(str::trim).find(|v| !v.is_empty()).map(str::to_string);
        }
    }
    None
}

/// `text` with Morph the default for [`TYPES`] and first among their added
/// associations; every other line stays as it was.
pub fn with_morph_default(text: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    for (section, set) in [(DEFAULTS, true), (ADDED, false)] {
        let start = match lines.iter().position(|l| l.trim() == format!("[{section}]")) {
            Some(i) => i,
            None => {
                if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                    lines.push(String::new());
                }
                lines.push(format!("[{section}]"));
                lines.len() - 1
            }
        };
        let end = lines[start + 1..].iter().position(|l| l.trim().starts_with('[')).map(|i| start + 1 + i).unwrap_or(lines.len());
        let mut missing: Vec<&str> = TYPES.to_vec();
        for line in &mut lines[start + 1..end] {
            let Some((key, value)) = line.split_once('=') else { continue };
            let key = key.trim().to_string();
            if !missing.contains(&key.as_str()) {
                continue;
            }
            missing.retain(|t| *t != key);
            *line = if set {
                format!("{key}={DESKTOP_ID}")
            } else {
                let others: Vec<&str> = value.split(';').map(str::trim).filter(|v| !v.is_empty() && *v != DESKTOP_ID).collect();
                format!("{key}={DESKTOP_ID};{}", others.iter().map(|o| format!("{o};")).collect::<String>())
            };
        }
        // New keys go after the section's last entry, before blank lines.
        let mut at = end;
        while at > start + 1 && lines[at - 1].trim().is_empty() {
            at -= 1;
        }
        for (n, mime) in missing.iter().enumerate() {
            lines.insert(at + n, if set { format!("{mime}={DESKTOP_ID}") } else { format!("{mime}={DESKTOP_ID};") });
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Replace `path` with `text` in one step (a temporary file, then rename).
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_file_name(format!(".{}.morph-tmp", path.file_name().and_then(|n| n.to_str()).unwrap_or("mimeapps.list")));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Which browser the lists say opens links: Morph only when http and https
/// both name it.
pub fn status(dirs: &Dirs) -> Status {
    let texts: Vec<String> = dirs.lists().iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect();
    let lookup = |mime: &str| texts.iter().find_map(|t| default_in(t, mime));
    match (lookup("x-scheme-handler/http"), lookup("x-scheme-handler/https")) {
        (Some(a), Some(b)) if a == DESKTOP_ID && b == DESKTOP_ID => Status::Default,
        (Some(a), _) if a != DESKTOP_ID => Status::Other(a),
        (_, Some(b)) if b != DESKTOP_ID => Status::Other(b),
        _ => Status::Unknown("no default browser is set".into()),
    }
}

/// Make Morph the default browser, then check every type with `xdg-mime`
/// (or the lists, without it). The error names the types that did not take.
pub fn make_default(dirs: &Dirs, run: Runner) -> Result<(), String> {
    let common = dirs.config.join("mimeapps.list");
    for path in dirs.lists() {
        if path != common && !path.exists() {
            continue;
        }
        let old = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("could not read {}: {e}", path.display())),
        };
        write_atomic(&path, &with_morph_default(&old)).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    }
    let apps = dirs.data.join("applications");
    if apps.is_dir() {
        let _ = run("update-desktop-database", &[&apps.to_string_lossy()]);
    }
    let texts: Vec<String> = dirs.lists().iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect();
    let failed: Vec<String> = TYPES
        .iter()
        .filter_map(|mime| {
            let got = match run("xdg-mime", &["query", "default", mime]) {
                Ok(out) => out.trim().to_string(),
                Err(_) => texts.iter().find_map(|t| default_in(t, mime)).unwrap_or_default(),
            };
            (got != DESKTOP_ID).then(|| format!("{mime} ({})", if got.is_empty() { "nothing" } else { &got }))
        })
        .collect();
    if failed.is_empty() { Ok(()) } else { Err(format!("these still open with another app: {}", failed.join(", "))) }
}

/// Morph's own scheme (`weirdnet-morph://`), which no other app handles.
pub const SCHEME_TYPE: &str = "x-scheme-handler/weirdnet-morph";

/// `text` with Morph handling [`SCHEME_TYPE`] when nothing does yet;
/// `None` when the list already names a handler (the user's choice stays).
pub fn with_scheme(text: &str) -> Option<String> {
    if default_in(text, SCHEME_TYPE).is_some() {
        return None;
    }
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    match lines.iter().position(|l| l.trim() == format!("[{DEFAULTS}]")) {
        Some(i) => lines.insert(i + 1, format!("{SCHEME_TYPE}={DESKTOP_ID}")),
        None => {
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(format!("[{DEFAULTS}]"));
            lines.push(format!("{SCHEME_TYPE}={DESKTOP_ID}"));
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    Some(out)
}

/// Register `weirdnet-morph://` for Morph in the user's `mimeapps.list`
/// when no list names a handler for it (at start, off the UI thread).
pub fn ensure_scheme(dirs: &Dirs) -> Result<bool, String> {
    let lists = dirs.lists();
    if lists.iter().filter_map(|p| std::fs::read_to_string(p).ok()).any(|t| default_in(&t, SCHEME_TYPE).is_some()) {
        return Ok(false);
    }
    let common = dirs.config.join("mimeapps.list");
    let old = std::fs::read_to_string(&common).unwrap_or_default();
    match with_scheme(&old) {
        Some(new) => write_atomic(&common, &new).map(|_| true).map_err(|e| format!("could not write {}: {e}", common.display())),
        None => Ok(false),
    }
}

/// The real runner: run the command, with a time limit, and capture its
/// output. Off Linux there is nothing to run.
pub fn system(program: &str, args: &[&str]) -> Result<String, String> {
    if !cfg!(target_os = "linux") {
        return Err(OTHER_SYSTEMS.into());
    }
    morph_os::run(program, args, morph_os::DESKTOP_CALL_LIMIT)
}

/// Whether Morph is the default, asked of macOS or Windows.
#[cfg(any(target_os = "macos", windows))]
fn platform_status() -> Status {
    #[cfg(target_os = "macos")]
    let default = morph_os::macos::is_default();
    #[cfg(windows)]
    let default =
        morph_os::windows::is_default(morph_os::windows::user_choice("http").as_deref(), morph_os::windows::user_choice("https").as_deref());
    if default { Status::Default } else { Status::Other("another browser".into()) }
}

/// Make Morph the default on macOS (the system asks the user), or register
/// it and open Windows Settings, where the user picks it.
#[cfg(any(target_os = "macos", windows))]
fn platform_make_default() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    return morph_os::macos::make_default(morph_os::DESKTOP_CALL_LIMIT).map(|()| MADE_DEFAULT.to_string());
    #[cfg(windows)]
    return morph_os::windows::register()
        .and_then(|()| morph_os::windows::open_settings())
        .map(|()| morph_os::windows::CHOOSE_IN_SETTINGS.to_string());
}

/// A runner a background thread can own.
pub type SystemRunner = fn(&str, &[&str]) -> Result<String, String>;

/// The work off the UI thread (`xdg-mime` and `update-desktop-database`
/// can take seconds). [`Check::take`] gives the result once it is there.
/// A status, and for [`Check::make_default`] what the attempt says
/// (`Ok(note)` or `Err(why)`).
pub type Outcome = (Status, Option<Result<String, String>>);

pub struct Check {
    rx: std::sync::mpsc::Receiver<Outcome>,
    /// Past this the settings stop waiting and say the system did not answer.
    deadline: std::time::Instant,
    making: bool,
}

impl Check {
    /// Read the current default browser.
    pub fn status(run: SystemRunner) -> Self {
        #[cfg(any(target_os = "macos", windows))]
        {
            let _ = run;
            Self::spawn(false, || (platform_status(), None))
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        Self::status_in(Dirs::system(), run)
    }

    /// Make Morph the default, then read the result back.
    pub fn make_default(run: SystemRunner) -> Self {
        #[cfg(any(target_os = "macos", windows))]
        {
            let _ = run;
            Self::spawn(true, || {
                let made = platform_make_default();
                (platform_status(), Some(made))
            })
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        Self::make_default_in(Dirs::system(), run)
    }

    #[cfg_attr(any(target_os = "macos", windows), allow(dead_code))]
    fn status_in(dirs: Option<Dirs>, run: SystemRunner) -> Self {
        Self::spawn(false, move || match &dirs {
            Some(d) => (status(d), None),
            None => (Status::Unknown(no_answer(run)), None),
        })
    }

    #[cfg_attr(any(target_os = "macos", windows), allow(dead_code))]
    fn make_default_in(dirs: Option<Dirs>, run: SystemRunner) -> Self {
        Self::spawn(true, move || match &dirs {
            Some(d) => {
                let made = make_default(d, &run).map(|()| MADE_DEFAULT.to_string());
                (status(d), Some(made))
            }
            None => {
                let why = no_answer(run);
                (Status::Unknown(why.clone()), Some(Err(why)))
            }
        })
    }

    fn spawn(making: bool, work: impl FnOnce() -> Outcome + Send + 'static) -> Self {
        Self::spawn_with_limit(making, CHECK_LIMIT, work)
    }

    fn spawn_with_limit(making: bool, limit: std::time::Duration, work: impl FnOnce() -> Outcome + Send + 'static) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        Self { rx, deadline: std::time::Instant::now() + limit, making }
    }

    /// The status (and, for [`Check::make_default`], what it says) once the
    /// work finished, or a "no answer" once it took too long; `None` while
    /// it runs.
    pub fn take(&self) -> Option<Outcome> {
        match self.rx.try_recv() {
            Ok(outcome) => Some(outcome),
            Err(_) if std::time::Instant::now() >= self.deadline => {
                Some((Status::Unknown(NO_ANSWER.into()), self.making.then(|| Err(NO_ANSWER.into()))))
            }
            Err(_) => None,
        }
    }
}

fn no_answer(_run: SystemRunner) -> String {
    if cfg!(target_os = "linux") { "no home directory".into() } else { OTHER_SYSTEMS.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn dirs(desktops: &[&str]) -> (tempfile::TempDir, Dirs) {
        let tmp = tempfile::tempdir().unwrap();
        let d = Dirs { config: tmp.path().join("config"), data: tmp.path().join("data"), desktops: desktops.iter().map(|s| s.to_string()).collect() };
        (tmp, d)
    }

    /// `xdg-mime query default` answered from the lists, as the real one does.
    fn from_lists(d: &Dirs) -> impl Fn(&str, &[&str]) -> Result<String, String> + '_ {
        move |p, a| {
            if p != "xdg-mime" {
                return Ok(String::new());
            }
            let texts: Vec<String> = d.lists().iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect();
            Ok(texts.iter().find_map(|t| default_in(t, a[2])).unwrap_or_default())
        }
    }

    fn morph_everywhere(text: &str) {
        for mime in TYPES {
            assert_eq!(default_in(text, mime).as_deref(), Some(DESKTOP_ID), "{mime} in\n{text}");
        }
    }

    #[test]
    fn an_empty_file_gets_both_sections() {
        let out = with_morph_default("");
        morph_everywhere(&out);
        assert!(out.contains("[Added Associations]\n") && out.contains("x-scheme-handler/https=morph.desktop;\n"), "{out}");
        assert_eq!(with_morph_default(&out), out, "a second run changes nothing");
    }

    #[test]
    fn chrome_as_default_is_replaced_and_kept_as_an_association() {
        let chrome = "[Default Applications]\ntext/html=google-chrome.desktop\nx-scheme-handler/http=google-chrome.desktop\nx-scheme-handler/https=google-chrome.desktop\n\n[Added Associations]\nx-scheme-handler/http=google-chrome.desktop;\n";
        let out = with_morph_default(chrome);
        morph_everywhere(&out);
        assert!(out.contains("x-scheme-handler/http=morph.desktop;google-chrome.desktop;\n"), "{out}");
        assert!(!out.contains("=google-chrome.desktop\n"), "{out}");
    }

    #[test]
    fn the_half_written_hyprland_state_is_finished() {
        let half = "[Default Applications]\ntext/html=morph.desktop\nx-scheme-handler/http=google-chrome.desktop\n";
        let out = with_morph_default(half);
        morph_everywhere(&out);
        assert_eq!(out.matches("text/html=").count(), 2, "one default and one association: {out}");
    }

    #[test]
    fn other_sections_and_lines_stay() {
        let text = "# mine\n[Default Applications]\napplication/pdf=org.gnome.Evince.desktop\n\n[Removed Associations]\ntext/html=bad.desktop;\n\n[Added Associations]\nimage/png=eog.desktop;\n";
        let out = with_morph_default(text);
        morph_everywhere(&out);
        for kept in
            ["# mine\n", "application/pdf=org.gnome.Evince.desktop\n", "[Removed Associations]\ntext/html=bad.desktop;\n", "image/png=eog.desktop;\n"]
        {
            assert!(out.contains(kept), "{kept:?} lost:\n{out}");
        }
        // New defaults land in their own section, not after [Removed Associations].
        let defaults = &out[out.find("[Default Applications]").unwrap()..out.find("[Removed Associations]").unwrap()];
        assert!(defaults.contains("x-scheme-handler/https=morph.desktop"), "{out}");
    }

    #[test]
    fn make_default_writes_the_list_and_checks_each_type() {
        let (_t, d) = dirs(&[]);
        std::fs::create_dir_all(d.data.join("applications")).unwrap();
        let calls = RefCell::new(Vec::new());
        let lists = from_lists(&d);
        let run = |p: &str, a: &[&str]| {
            calls.borrow_mut().push(format!("{p} {}", a.join(" ")));
            lists(p, a)
        };
        assert_eq!(status(&d), Status::Unknown("no default browser is set".into()));
        assert_eq!(make_default(&d, &run), Ok(()));
        assert_eq!(status(&d), Status::Default);
        let calls = calls.borrow();
        assert!(calls[0].starts_with("update-desktop-database "), "{calls:?}");
        assert_eq!(calls.iter().filter(|c| c.starts_with("xdg-mime query default")).count(), 4);
    }

    #[test]
    fn a_desktop_specific_list_wins_and_is_updated_too() {
        let (_t, d) = dirs(&["hyprland"]);
        std::fs::create_dir_all(&d.config).unwrap();
        let chrome = "[Default Applications]\nx-scheme-handler/http=google-chrome.desktop\nx-scheme-handler/https=google-chrome.desktop\n";
        std::fs::write(d.config.join("hyprland-mimeapps.list"), chrome).unwrap();
        std::fs::write(d.config.join("mimeapps.list"), with_morph_default("")).unwrap();
        assert_eq!(status(&d), Status::Other("google-chrome.desktop".into()), "hyprland's list wins");
        assert_eq!(make_default(&d, &from_lists(&d)), Ok(()));
        morph_everywhere(&std::fs::read_to_string(d.config.join("hyprland-mimeapps.list")).unwrap());
        assert_eq!(status(&d), Status::Default);
        assert!(!d.config.join("sway-mimeapps.list").exists(), "only lists that exist");
    }

    #[test]
    fn a_failed_check_names_the_types() {
        let (_t, d) = dirs(&[]);
        let stubborn = |p: &str, a: &[&str]| -> Result<String, String> {
            Ok(if p == "xdg-mime" && a[2].ends_with("https") { "google-chrome.desktop\n".into() } else { DESKTOP_ID.into() })
        };
        let err = make_default(&d, &stubborn).unwrap_err();
        assert!(err.contains("x-scheme-handler/https (google-chrome.desktop)") && !err.contains("text/html"), "{err}");
        // Without xdg-mime the lists answer.
        assert_eq!(make_default(&d, &|_, _| Err("not found".into())), Ok(()));
    }

    #[test]
    fn the_scheme_is_registered_once_and_a_choice_is_kept() {
        let (_t, d) = dirs(&[]);
        assert_eq!(ensure_scheme(&d), Ok(true));
        let text = std::fs::read_to_string(d.config.join("mimeapps.list")).unwrap();
        assert_eq!(default_in(&text, SCHEME_TYPE).as_deref(), Some(DESKTOP_ID));
        assert_eq!(ensure_scheme(&d), Ok(false), "a second run changes nothing");
        let other = "[Default Applications]\nx-scheme-handler/weirdnet-morph=other.desktop\n";
        assert_eq!(with_scheme(other), None, "another handler stays");
        let kept = with_scheme("[Default Applications]\ntext/html=firefox.desktop\n").unwrap();
        assert!(kept.contains("text/html=firefox.desktop\n") && kept.contains("weirdnet-morph=morph.desktop\n"), "{kept}");
    }

    #[test]
    fn morph_is_default_only_when_http_and_https_both_say_so() {
        let (_t, d) = dirs(&[]);
        std::fs::create_dir_all(&d.config).unwrap();
        std::fs::write(d.config.join("mimeapps.list"), "[Default Applications]\nx-scheme-handler/http=morph.desktop\n").unwrap();
        assert!(matches!(status(&d), Status::Unknown(_)));
        std::fs::write(
            d.config.join("mimeapps.list"),
            "[Default Applications]\nx-scheme-handler/http=morph.desktop\nx-scheme-handler/https=firefox.desktop\n",
        )
        .unwrap();
        assert_eq!(status(&d), Status::Other("firefox.desktop".into()));
    }

    #[test]
    fn checks_run_off_the_calling_thread() {
        fn slow(_: &str, _: &[&str]) -> Result<String, String> {
            std::thread::sleep(std::time::Duration::from_millis(200));
            Ok(DESKTOP_ID.into())
        }
        let (_t, d) = dirs(&[]);
        let started = std::time::Instant::now();
        let check = Check::make_default_in(Some(d), slow);
        assert!(check.take().is_none());
        assert!(started.elapsed() < std::time::Duration::from_millis(100), "the caller waited");
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let done = loop {
            if let Some(done) = check.take() {
                break done;
            }
            assert!(std::time::Instant::now() < end, "the check never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(done, (Status::Default, Some(Ok(MADE_DEFAULT.to_string()))));
    }

    #[test]
    fn a_check_that_hangs_gives_up_at_its_limit() {
        let check = Check::spawn_with_limit(true, std::time::Duration::from_millis(50), || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            (Status::Default, None)
        });
        assert!(check.take().is_none());
        std::thread::sleep(std::time::Duration::from_millis(80));
        assert_eq!(check.take(), Some((Status::Unknown(NO_ANSWER.into()), Some(Err(NO_ANSWER.into())))));
    }
}
