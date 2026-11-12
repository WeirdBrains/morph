use std::io;
use std::path::{Path, PathBuf};

/// Where the in-process control server listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlEndpoint {
    /// Unix domain socket (Linux, macOS).
    Unix(PathBuf),
    /// Windows named pipe path, e.g. `\\.\pipe\morph-webview-control`.
    NamedPipe(String),
    /// Loopback TCP, used on Windows when a named pipe is not available to a client.
    Loopback { port: u16 },
}

impl ControlEndpoint {
    /// A default, user-private endpoint for this process.
    pub fn default_for_process() -> Self {
        #[cfg(unix)]
        {
            Self::Unix(default_unix_path())
        }
        #[cfg(windows)]
        {
            Self::NamedPipe(format!(r"\\.\pipe\morph-webview-control-{}", std::process::id()))
        }
        #[cfg(not(any(unix, windows)))]
        {
            Self::Loopback { port: 0 }
        }
    }

    /// A per-process endpoint inside `dir`, a directory only this app can
    /// write to. Android needs this: apps there have no `XDG_RUNTIME_DIR` or
    /// `TMPDIR`, and `/tmp` is not theirs, so [`Self::default_for_process`]
    /// cannot bind. Windows ignores `dir` (named pipes have no directory).
    pub fn in_dir(dir: impl AsRef<Path>) -> Self {
        #[cfg(unix)]
        {
            Self::Unix(dir.as_ref().join(format!("webview-control-{}.sock", std::process::id())))
        }
        #[cfg(not(unix))]
        {
            let _ = dir;
            Self::default_for_process()
        }
    }

    pub fn from_path(path: impl AsRef<Path>) -> Self {
        #[cfg(unix)]
        {
            Self::Unix(path.as_ref().to_path_buf())
        }
        #[cfg(windows)]
        {
            Self::NamedPipe(path.as_ref().to_string_lossy().into_owned())
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            Self::Loopback { port: 0 }
        }
    }

    /// URI a Dart (or other) gRPC client can parse: `unix:///path` or `tcp://127.0.0.1:port`.
    pub fn uri(&self) -> String {
        match self {
            Self::Unix(path) => format!("unix://{}", path.display()),
            Self::NamedPipe(name) => format!("pipe://{name}"),
            Self::Loopback { port } => format!("tcp://127.0.0.1:{port}"),
        }
    }

    pub fn unix_path(&self) -> Option<&Path> {
        match self {
            Self::Unix(path) => Some(path.as_path()),
            _ => None,
        }
    }
}

#[cfg(unix)]
fn default_unix_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("TMPDIR").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let dir = base.join("morph");
    dir.join(format!("webview-control-{}.sock", std::process::id()))
}

#[cfg(unix)]
pub fn prepare_unix_socket(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(unix)]
pub fn restrict_unix_socket(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
pub fn prepare_unix_socket(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(not(unix))]
pub fn restrict_unix_socket(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn in_dir_puts_the_socket_in_that_dir() {
        let dir = Path::new("/data/user/0/app/files/run");
        let endpoint = ControlEndpoint::in_dir(dir);
        let path = endpoint.unix_path().unwrap();
        assert_eq!(path.parent(), Some(dir));
        assert!(path.file_name().unwrap().to_string_lossy().ends_with(".sock"));
    }

    #[test]
    fn prepare_creates_a_private_dir() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = std::env::temp_dir().join(format!("morph-endpoint-test-{}", std::process::id()));
        let path = ControlEndpoint::in_dir(tmp.join("run")).unix_path().unwrap().to_path_buf();
        prepare_unix_socket(&path).unwrap();
        let mode = std::fs::metadata(tmp.join("run")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
