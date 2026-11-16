//! Downloads as the shell shows them. The backend writes a download's
//! bytes to a staging file ([`morph_webview::download_staging`]) and
//! reports it; the shell asks where the file goes (a save dialog with the
//! name the server suggested) and the file moves there once both the bytes
//! and the answer are in. Cancelling the dialog stops the download.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use morph_webview::{Download, DownloadChange};

/// How a download stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadState {
    /// Bytes are coming (or in, waiting for the save dialog).
    Running,
    /// The file is at this path.
    Saved(PathBuf),
    /// It stopped: why.
    Failed(String),
    /// The user cancelled the save dialog.
    Cancelled,
}

/// One download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadItem {
    pub id: u64,
    /// The file name the server suggested.
    pub name: String,
    pub url: String,
    pub received: u64,
    pub total: Option<u64>,
    pub state: DownloadState,
    staged: PathBuf,
    /// Where the user wants it, once the dialog answered.
    target: Option<PathBuf>,
    /// All bytes are in `staged`.
    complete: bool,
}

impl DownloadItem {
    /// Progress from 0 to 1, when the size is known.
    pub fn fraction(&self) -> Option<f32> {
        match self.total {
            Some(total) if total > 0 => Some((self.received as f64 / total as f64).clamp(0.0, 1.0) as f32),
            _ => None,
        }
    }

    /// Whether the user still waits on it (bytes or the move).
    pub fn running(&self) -> bool {
        self.state == DownloadState::Running
    }

    /// The folder its save dialog opens in.
    pub fn folder(&self) -> PathBuf {
        self.staged.parent().map(Path::to_path_buf).unwrap_or_default()
    }
}

/// What a change did, for the shell's event log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Saved(PathBuf),
    Failed(String),
}

/// Every download of the window, newest last.
#[derive(Debug, Default)]
pub struct Downloads {
    items: Vec<DownloadItem>,
    /// Downloads whose save dialog has not been shown yet.
    asks: VecDeque<u64>,
    /// The downloads folder the settings name, where save dialogs open.
    folder: Option<PathBuf>,
}

impl Downloads {
    pub fn items(&self) -> &[DownloadItem] {
        &self.items
    }

    pub fn get(&self, id: u64) -> Option<&DownloadItem> {
        self.items.iter().find(|d| d.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut DownloadItem> {
        self.items.iter_mut().find(|d| d.id == id)
    }

    /// The downloads folder from the settings (`None`: the system's).
    pub fn set_folder(&mut self, folder: Option<PathBuf>) {
        self.folder = folder;
    }

    /// The next download to ask a save path for: its id, name and the
    /// folder to open in (the settings' downloads folder, else where the
    /// bytes are staged).
    pub fn take_ask(&mut self) -> Option<(u64, String, PathBuf)> {
        let id = self.asks.pop_front()?;
        let folder = self.folder.clone();
        self.get(id).map(|d| (d.id, d.name.clone(), folder.unwrap_or_else(|| d.folder())))
    }

    /// The backend reported `event`.
    pub fn apply(&mut self, event: Download) -> Outcome {
        let id = event.id;
        match event.change {
            DownloadChange::Started { url, name, staged } => {
                self.items.retain(|d| d.id != id);
                self.items.push(DownloadItem {
                    id,
                    name,
                    url,
                    received: 0,
                    total: None,
                    state: DownloadState::Running,
                    staged,
                    target: None,
                    complete: false,
                });
                self.asks.push_back(id);
                Outcome::Nothing
            }
            DownloadChange::Progress { received, total } => {
                if let Some(d) = self.get_mut(id) {
                    d.received = received;
                    d.total = total.or(d.total);
                }
                Outcome::Nothing
            }
            DownloadChange::Finished => {
                let Some(d) = self.get_mut(id) else {
                    return Outcome::Nothing;
                };
                d.complete = true;
                if d.state == DownloadState::Cancelled {
                    let _ = std::fs::remove_file(&d.staged);
                    return Outcome::Nothing;
                }
                Self::settle(d)
            }
            DownloadChange::Failed(why) => {
                let Some(d) = self.get_mut(id) else {
                    return Outcome::Nothing;
                };
                let _ = std::fs::remove_file(&d.staged);
                if d.state != DownloadState::Running {
                    return Outcome::Nothing;
                }
                d.state = DownloadState::Failed(why.clone());
                Outcome::Failed(why)
            }
        }
    }

    /// The save dialog for `id` answered: a path, or `None` (cancelled).
    /// Returns whether the backend must stop the download.
    pub fn answer(&mut self, id: u64, path: Option<PathBuf>) -> (bool, Outcome) {
        self.asks.retain(|a| *a != id);
        let Some(d) = self.get_mut(id) else {
            return (false, Outcome::Nothing);
        };
        if d.state != DownloadState::Running {
            return (false, Outcome::Nothing);
        }
        match path {
            Some(path) => {
                d.target = Some(path);
                (false, Self::settle(d))
            }
            None => {
                d.state = DownloadState::Cancelled;
                if d.complete {
                    let _ = std::fs::remove_file(&d.staged);
                }
                (!d.complete, Outcome::Nothing)
            }
        }
    }

    /// Move the file into place once the bytes and the target are in.
    fn settle(d: &mut DownloadItem) -> Outcome {
        let (true, Some(target)) = (d.complete, d.target.clone()) else {
            return Outcome::Nothing;
        };
        match move_file(&d.staged, &target) {
            Ok(()) => {
                d.state = DownloadState::Saved(target.clone());
                Outcome::Saved(target)
            }
            Err(e) => {
                let why = format!("could not save {}: {e}", target.display());
                let _ = std::fs::remove_file(&d.staged);
                d.state = DownloadState::Failed(why.clone());
                Outcome::Failed(why)
            }
        }
    }

    /// Forget the downloads that ended.
    pub fn clear_done(&mut self) {
        self.items.retain(DownloadItem::running);
    }
}

/// Rename `from` to `to`, or copy it across file systems.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    std::fs::remove_file(from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("morph-dl-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn started(d: &mut Downloads, folder: &Path, id: u64) -> PathBuf {
        let staged = morph_webview::download_staging(folder, "a.bin", id);
        std::fs::write(&staged, b"bytes").unwrap();
        d.apply(Download { id, change: DownloadChange::Started { url: "https://x/a.bin".into(), name: "a.bin".into(), staged: staged.clone() } });
        staged
    }

    #[test]
    fn the_file_moves_once_bytes_and_answer_are_in_either_order() {
        let folder = dir("order");
        let mut d = Downloads::default();
        let staged = started(&mut d, &folder, 1);
        assert_eq!(d.take_ask(), Some((1, "a.bin".into(), folder.clone())));
        assert_eq!(d.take_ask(), None);
        let target = folder.join("chosen.bin");
        assert_eq!(d.answer(1, Some(target.clone())), (false, Outcome::Nothing), "bytes still coming");
        d.apply(Download { id: 1, change: DownloadChange::Progress { received: 5, total: Some(10) } });
        assert_eq!(d.get(1).unwrap().fraction(), Some(0.5));
        assert_eq!(d.apply(Download { id: 1, change: DownloadChange::Finished }), Outcome::Saved(target.clone()));
        assert_eq!(std::fs::read(&target).unwrap(), b"bytes");
        assert!(!staged.exists());

        let staged = started(&mut d, &folder, 2);
        assert_eq!(d.apply(Download { id: 2, change: DownloadChange::Finished }), Outcome::Nothing, "waits for the dialog");
        let target = folder.join("second.bin");
        assert_eq!(d.answer(2, Some(target.clone())), (false, Outcome::Saved(target.clone())));
        assert!(target.exists() && !staged.exists());
    }

    #[test]
    fn cancelling_the_dialog_stops_the_download_and_deletes_the_bytes() {
        let folder = dir("cancel");
        let mut d = Downloads::default();
        let staged = started(&mut d, &folder, 1);
        assert_eq!(d.answer(1, None), (true, Outcome::Nothing), "still running: stop it");
        d.apply(Download { id: 1, change: DownloadChange::Failed("cancelled".into()) });
        assert_eq!(d.get(1).unwrap().state, DownloadState::Cancelled);
        assert!(!staged.exists());

        let staged = started(&mut d, &folder, 2);
        d.apply(Download { id: 2, change: DownloadChange::Finished });
        assert_eq!(d.answer(2, None), (false, Outcome::Nothing), "already done: nothing to stop");
        assert!(!staged.exists());
    }

    #[test]
    fn a_failed_download_reports_why() {
        let folder = dir("fail");
        let mut d = Downloads::default();
        let staged = started(&mut d, &folder, 1);
        assert_eq!(d.apply(Download { id: 1, change: DownloadChange::Failed("net".into()) }), Outcome::Failed("net".into()));
        assert!(!staged.exists());
        assert_eq!(d.answer(1, Some(folder.join("x"))), (false, Outcome::Nothing));
        d.clear_done();
        assert!(d.items().is_empty());
    }
}
