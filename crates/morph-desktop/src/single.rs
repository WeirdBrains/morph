//! One Morph per Morph home: the first launch listens on a Unix socket, and
//! a later launch hands its URL to it (a new tab in the running window) and
//! exits. Links from other apps (`morph %u` in the desktop file) open in
//! the window that is already there.
//!
//! The protocol is one line each way: `open <url>` or `show`, answered by
//! `ok`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

/// What a later launch asks of the running window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Open this address or search in a new tab.
    Open(String),
    /// Just bring the window forward.
    Show,
}

impl Request {
    fn line(&self) -> String {
        match self {
            Request::Open(url) => format!("open {}\n", url.replace(['\n', '\r'], " ")),
            Request::Show => "show\n".into(),
        }
    }

    fn parse(line: &str) -> Option<Self> {
        let line = line.trim_end_matches(['\n', '\r']);
        if line == "show" {
            return Some(Request::Show);
        }
        let url = line.strip_prefix("open ")?.trim();
        Some(if url.is_empty() { Request::Show } else { Request::Open(url.to_string()) })
    }
}

/// FNV-1a: a hash that stays the same across builds, so two Morph versions
/// agree on the socket name.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

/// The socket for Morph home `home`, in `runtime` (`$XDG_RUNTIME_DIR`,
/// else the temp directory).
pub fn socket_path(home: &Path, runtime: Option<&Path>) -> PathBuf {
    let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    let dir = runtime.map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
    dir.join(format!("morph-{:016x}.sock", fnv1a(home.as_os_str().as_encoded_bytes())))
}

/// The socket path for this process's environment.
pub fn socket_for(home: &Path) -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()).map(PathBuf::from);
    socket_path(home, runtime.as_deref())
}

/// Hand `request` to a running Morph at `path`. `true` when it took it.
pub fn hand_off(path: &Path, request: &Request) -> bool {
    let Ok(mut stream) = UnixStream::connect(path) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
    if stream.write_all(request.line().as_bytes()).is_err() {
        return false;
    }
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer).is_ok() && answer.trim() == "ok"
}

/// The running Morph's end: requests from later launches arrive on
/// [`Listener::requests`]. The socket file goes away when this is dropped.
pub struct Listener {
    path: PathBuf,
    requests: Receiver<Request>,
}

impl Listener {
    /// Listen at `path`. A socket file nobody answers on (a Morph that
    /// crashed) is replaced; a live one is an error.
    pub fn bind(path: &Path) -> std::io::Result<Self> {
        if path.exists() {
            if UnixStream::connect(path).is_ok() {
                return Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, "another Morph is running"));
            }
            std::fs::remove_file(path)?;
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let listener = UnixListener::bind(path)?;
        let (tx, requests) = channel();
        std::thread::Builder::new().name("morph-single-instance".into()).spawn(move || serve(listener, tx))?;
        Ok(Self { path: path.to_path_buf(), requests })
    }

    /// Requests that arrived since the last call.
    pub fn take(&self) -> Vec<Request> {
        self.requests.try_iter().collect()
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn serve(listener: UnixListener, tx: Sender<Request>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
        let mut line = String::new();
        let mut reader = BufReader::new(&stream);
        if reader.read_line(&mut line).is_err() {
            continue;
        }
        let Some(request) = Request::parse(&line) else { continue };
        if tx.send(request).is_err() {
            return;
        }
        let _ = (&stream).write_all(b"ok\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip_as_lines() {
        for r in [Request::Open("https://example.com/?q=a b".into()), Request::Show] {
            assert_eq!(Request::parse(&r.line()), Some(r));
        }
        assert_eq!(Request::parse("open line\nbreak"), Some(Request::Open("line\nbreak".into())));
        assert_eq!(Request::Open("a\nb".into()).line(), "open a b\n");
        assert_eq!(Request::parse("open "), Some(Request::Show));
        assert_eq!(Request::parse("nonsense"), None);
    }

    #[test]
    fn each_home_has_its_own_stable_socket() {
        let run = Path::new("/run/user/1000");
        let a = socket_path(Path::new("/nonexistent/a"), Some(run));
        assert_eq!(a, socket_path(Path::new("/nonexistent/a"), Some(run)));
        assert_ne!(a, socket_path(Path::new("/nonexistent/b"), Some(run)));
        assert!(a.starts_with(run) && a.extension().is_some_and(|e| e == "sock"));
    }

    #[test]
    fn a_second_launch_hands_its_url_to_the_first() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("m.sock");
        assert!(!hand_off(&path, &Request::Show), "nobody listens yet");
        let listener = Listener::bind(&path).unwrap();
        assert!(Listener::bind(&path).is_err(), "only one listener");
        assert!(hand_off(&path, &Request::Open("example.com".into())));
        assert!(hand_off(&path, &Request::Show));
        assert_eq!(listener.take(), [Request::Open("example.com".into()), Request::Show]);
        drop(listener);
        assert!(!path.exists());
    }

    #[test]
    fn a_stale_socket_file_is_replaced() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("m.sock");
        drop(UnixListener::bind(&path).unwrap());
        assert!(path.exists());
        let listener = Listener::bind(&path).unwrap();
        assert!(hand_off(&path, &Request::Show));
        assert_eq!(listener.take(), [Request::Show]);
    }
}
