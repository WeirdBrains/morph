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
