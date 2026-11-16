//! Clean-up after the old Flutter desktop build on Linux. It wrote its own
//! desktop entry, `$XDG_DATA_HOME/applications/org.weirdbrains.morph.desktop`
//! (marked `X-Morph-Generated=true`, its `Exec` pointing at a build that is
//! gone), and made that id the browser in `$XDG_CONFIG_HOME/mimeapps.list`.
//! Launchers then showed a broken Morph without an icon. At start-up, off
//! the UI thread, Morph removes that entry (only when it carries the marker)
//! and drops the id from `mimeapps.list`; where it was the default, the
//! packaged `morph.desktop` takes its place. Nothing else is touched.

use std::path::{Path, PathBuf};

/// The id the Flutter build used.
pub const OLD_ID: &str = "org.weirdbrains.morph.desktop";

/// What a clean-up changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Cleaned {
    pub removed_entry: bool,
    pub rewrote_mimeapps: bool,
}

/// `$XDG_DATA_HOME` and `$XDG_CONFIG_HOME`, with their defaults under the
/// home directory. `None` without a home.
pub fn xdg_dirs() -> Option<(PathBuf, PathBuf)> {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    let dir = |var: &str, fallback: &str| -> Option<PathBuf> {
        match std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from) {
            Some(p) if p.is_absolute() => Some(p),
            _ => home.as_ref().map(|h| h.join(fallback)),
        }
    };
    Some((dir("XDG_DATA_HOME", ".local/share")?, dir("XDG_CONFIG_HOME", ".config")?))
}

/// Remove the old generated entry and its `mimeapps.list` associations.
pub fn clean(data_home: &Path, config_home: &Path) -> std::io::Result<Cleaned> {
    let mut out = Cleaned::default();
    let entry = data_home.join("applications").join(OLD_ID);
    if let Ok(text) = std::fs::read_to_string(&entry)
        && generated(&text)
    {
        std::fs::remove_file(&entry)?;
        out.removed_entry = true;
    }
    let list = config_home.join("mimeapps.list");
    if let Ok(text) = std::fs::read_to_string(&list)
        && let Some(new) = rewrite_mimeapps(&text)
    {
        // Written beside it and renamed, so a crash never leaves half a file.
        let tmp = list.with_extension("list.morph-tmp");
        std::fs::write(&tmp, new)?;
        std::fs::rename(&tmp, &list)?;
        out.rewrote_mimeapps = true;
    }
    Ok(out)
}

/// Whether a desktop entry is one Morph generated itself.
fn generated(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.trim();
        l.split_once('=').is_some_and(|(k, v)| k.trim() == "X-Morph-Generated" && v.trim().eq_ignore_ascii_case("true"))
    })
}

/// `mimeapps.list` without [`OLD_ID`]: in `[Default Applications]` it
/// becomes `morph.desktop` (once), elsewhere it is dropped, and a key left
/// with no ids goes. `None` when nothing mentions it.
pub fn rewrite_mimeapps(text: &str) -> Option<String> {
    let mut section = String::new();
    let mut changed = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed.to_string();
            out.push(line.to_string());
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            out.push(line.to_string());
            continue;
        };
        if trimmed.starts_with('#') {
            out.push(line.to_string());
            continue;
        }
        let ids: Vec<&str> = value.split(';').map(str::trim).filter(|s| !s.is_empty()).collect();
        if !ids.contains(&OLD_ID) {
            out.push(line.to_string());
            continue;
        }
        changed = true;
        let mut kept: Vec<&str> = Vec::new();
        for id in ids {
            let id = if id == OLD_ID {
                if section != "[Default Applications]" {
                    continue;
                }
                crate::default_browser::DESKTOP_ID
            } else {
                id
            };
            if !kept.contains(&id) {
                kept.push(id);
            }
        }
        if !kept.is_empty() {
            out.push(format!("{}={};", key.trim(), kept.join(";")));
        }
    }
    if !changed {
        return None;
    }
    let mut new = out.join("\n");
    if text.ends_with('\n') {
        new.push('\n');
    }
    Some(new)
}

/// Run [`clean`] for this user on a thread of its own; one line on stderr
/// says what changed.
pub fn spawn() {
    let _ = std::thread::Builder::new().name("morph-legacy-desktop".into()).spawn(|| {
        let Some((data, config)) = xdg_dirs() else { return };
        match clean(&data, &config) {
            Ok(c) if c.removed_entry || c.rewrote_mimeapps => {
                eprintln!("morph: removed the old Flutter build's desktop entry ({OLD_ID}) and its associations")
            }
            Ok(_) => {}
            Err(e) => eprintln!("morph: could not remove the old {OLD_ID}: {e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD_ENTRY: &str =
        "[Desktop Entry]\nType=Application\nName=Morph\nExec=/tmp/build-123/morph %u\nIcon=org.weirdbrains.morph\nX-Morph-Generated=true\n";

    fn dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let (data, config) = (tmp.path().join("data"), tmp.path().join("config"));
        std::fs::create_dir_all(data.join("applications")).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        (tmp, data, config)
    }

    #[test]
    fn the_generated_entry_and_its_associations_go() {
        let (_tmp, data, config) = dirs();
        std::fs::write(data.join("applications").join(OLD_ID), OLD_ENTRY).unwrap();
        std::fs::write(data.join("applications/other.desktop"), "[Desktop Entry]\nX-Morph-Generated=true\n").unwrap();
        let list = "[Default Applications]\ntext/html=org.weirdbrains.morph.desktop\nx-scheme-handler/http=org.weirdbrains.morph.desktop;\nx-scheme-handler/https=org.weirdbrains.morph.desktop;firefox.desktop;\nimage/png=org.gnome.Loupe.desktop\n\n[Added Associations]\ntext/html=org.weirdbrains.morph.desktop;firefox.desktop;\nx-scheme-handler/http=org.weirdbrains.morph.desktop;\n";
        std::fs::write(config.join("mimeapps.list"), list).unwrap();

        assert_eq!(clean(&data, &config).unwrap(), Cleaned { removed_entry: true, rewrote_mimeapps: true });
        assert!(!data.join("applications").join(OLD_ID).exists());
        assert!(data.join("applications/other.desktop").exists(), "other entries stay");
        assert_eq!(
            std::fs::read_to_string(config.join("mimeapps.list")).unwrap(),
            "[Default Applications]\ntext/html=morph.desktop;\nx-scheme-handler/http=morph.desktop;\nx-scheme-handler/https=morph.desktop;firefox.desktop;\nimage/png=org.gnome.Loupe.desktop\n\n[Added Associations]\ntext/html=firefox.desktop;\n"
        );
        assert_eq!(clean(&data, &config).unwrap(), Cleaned::default(), "a second run changes nothing");
    }

    #[test]
    fn an_entry_without_the_marker_is_never_touched() {
        let (_tmp, data, config) = dirs();
        let mine = "[Desktop Entry]\nName=Morph (mine)\nExec=/opt/morph/morph %u\n";
        std::fs::write(data.join("applications").join(OLD_ID), mine).unwrap();
        std::fs::write(data.join("applications/x.desktop"), "X-Morph-Generated=false\n").unwrap();
        assert!(!clean(&data, &config).unwrap().removed_entry);
        assert_eq!(std::fs::read_to_string(data.join("applications").join(OLD_ID)).unwrap(), mine);
        assert!(!generated("X-Morph-Generated=false\n") && !generated("# X-Morph-Generated=true\n") && generated(OLD_ENTRY));
    }

    #[test]
    fn mimeapps_without_the_old_id_is_left_as_it_is() {
        let text = "[Default Applications]\n# a comment\ntext/html=morph.desktop;\nx-scheme-handler/http=firefox.desktop\n";
        assert_eq!(rewrite_mimeapps(text), None);
        let (_tmp, data, config) = dirs();
        std::fs::write(config.join("mimeapps.list"), text).unwrap();
        assert_eq!(clean(&data, &config).unwrap(), Cleaned::default());
        assert_eq!(std::fs::read_to_string(config.join("mimeapps.list")).unwrap(), text);
    }

    #[test]
    fn the_packaged_entry_is_not_listed_twice() {
        let got = rewrite_mimeapps("[Default Applications]\ntext/html=org.weirdbrains.morph.desktop;morph.desktop;\n").unwrap();
        assert_eq!(got, "[Default Applications]\ntext/html=morph.desktop;\n");
    }

    #[test]
    fn nothing_there_is_fine() {
        let (_tmp, data, config) = dirs();
        assert_eq!(clean(&data, &config).unwrap(), Cleaned::default());
    }

    #[test]
    fn the_default_browser_is_the_packaged_entry() {
        assert_eq!(crate::default_browser::DESKTOP_ID, "morph.desktop");
    }
}
