//! Native open and save dialogs for Morph's own file fields: the desktop
//! portal on Linux (parented to the window), the system panels on macOS and
//! Windows. Tests answer them through `MORPH_TEST_PICK_OPEN` (the paths,
//! `;`-separated) and `MORPH_TEST_PICK_DIR` (the folder a Save or folder
//! dialog answers with), so no dialog ever opens under a test.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use gpui::{App, PathPromptOptions};

/// What to ask for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// Files to open.
    Open { multiple: bool },
    /// A folder to save into.
    Folder,
    /// Where to save a new file, suggesting `name`.
    Save { name: String },
    /// Where to save a download named `name`, starting in `dir`.
    Download { dir: PathBuf, name: String },
}

/// The chosen paths (empty when the user cancelled), or why no dialog could open.
pub type Answer = Pin<Box<dyn Future<Output = Result<Vec<PathBuf>, String>>>>;

/// What a test hook answers, if one is set.
fn test_answer(ask: &Ask, open: Option<String>, dir: Option<String>) -> Option<Vec<PathBuf>> {
    match ask {
        Ask::Open { .. } => open.map(|v| v.split(';').filter(|p| !p.is_empty()).map(PathBuf::from).collect()),
        Ask::Folder => dir.map(|d| vec![PathBuf::from(d)]),
        Ask::Save { name } | Ask::Download { name, .. } => dir.map(|d| vec![PathBuf::from(d).join(name)]),
    }
}

/// Where Save dialogs start: the home folder.
fn start_dir() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn why(e: &dyn std::fmt::Display) -> String {
    format!("No file dialog could open ({e}); type the path instead.")
}

/// Show the dialog for `ask`.
pub fn ask(cx: &App, ask: Ask) -> Answer {
    if let Some(paths) = test_answer(&ask, std::env::var("MORPH_TEST_PICK_OPEN").ok(), std::env::var("MORPH_TEST_PICK_DIR").ok()) {
        return Box::pin(async move { Ok(paths) });
    }
    match ask {
        Ask::Save { .. } | Ask::Download { .. } => {
            let (dir, name) = match ask {
                Ask::Download { dir, name } if dir.is_dir() => (dir, name),
                Ask::Save { name } | Ask::Download { name, .. } => (start_dir(), name),
                _ => unreachable!(),
            };
            let rx = cx.prompt_for_new_path(&dir, Some(&name));
            Box::pin(async move {
                match rx.await {
                    Ok(Ok(Some(path))) => Ok(vec![path]),
                    Ok(Ok(None)) | Err(_) => Ok(Vec::new()),
                    Ok(Err(e)) => Err(why(&e)),
                }
            })
        }
        Ask::Open { .. } | Ask::Folder => {
            let folder = matches!(ask, Ask::Folder);
            let multiple = matches!(ask, Ask::Open { multiple: true });
            let rx = cx.prompt_for_paths(PathPromptOptions {
                files: !folder,
                directories: folder,
                multiple: !folder && multiple,
                prompt: Some(if folder { "Save here" } else { "Open" }.into()),
            });
            Box::pin(async move {
                match rx.await {
                    Ok(Ok(Some(paths))) => Ok(paths),
                    Ok(Ok(None)) | Err(_) => Ok(Vec::new()),
                    Ok(Err(e)) => Err(why(&e)),
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_hook_answers_every_kind() {
        let open = Some("/a/x.txt;/b/y.txt".to_string());
        let dir = Some("/tmp/pick".to_string());
        assert_eq!(test_answer(&Ask::Open { multiple: true }, open.clone(), None), Some(vec![PathBuf::from("/a/x.txt"), PathBuf::from("/b/y.txt")]));
        assert_eq!(test_answer(&Ask::Folder, None, dir.clone()), Some(vec![PathBuf::from("/tmp/pick")]));
        assert_eq!(test_answer(&Ask::Save { name: "w.txt".into() }, None, dir), Some(vec![PathBuf::from("/tmp/pick/w.txt")]));
        assert_eq!(
            test_answer(&Ask::Download { dir: "/d".into(), name: "a.bin".into() }, None, Some("/tmp/pick".into())),
            Some(vec![PathBuf::from("/tmp/pick/a.bin")])
        );
        assert_eq!(test_answer(&Ask::Save { name: "w.txt".into() }, open, None), None, "no dir hook: a real dialog");
    }
}
