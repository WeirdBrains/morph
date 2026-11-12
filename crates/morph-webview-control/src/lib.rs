//! In-process gRPC control layer for Morph web views.
//!
//! The server lives in the Flutter embedder process and speaks tonic over a Unix domain
//! socket on Linux and macOS, a named pipe on Windows (loopback TCP remains available). A single
//! bidirectional `Session` stream carries navigation, input, and page-bridge messages
//! with monotonic sequence numbers.
pub mod endpoint;
pub mod ffi;
pub mod proto {
    #![allow(clippy::result_large_err)]
    tonic::include_proto!("morph.webview.v1");
}
#[cfg(windows)]
mod named_pipe;
pub mod server;

pub use endpoint::ControlEndpoint;
pub use server::{ControlBackend, ControlServer, SharedServer};

use std::sync::{Mutex, OnceLock};

static SERVER: OnceLock<Mutex<Option<ControlServer>>> = OnceLock::new();

fn global() -> &'static Mutex<Option<ControlServer>> {
    SERVER.get_or_init(|| Mutex::new(None))
}

/// Start (or replace) the process-wide control server.
pub fn start_global(endpoint: Option<ControlEndpoint>, backend: std::sync::Arc<dyn ControlBackend>) -> Result<ControlEndpoint, server::ControlError> {
    let server = ControlServer::start(endpoint, backend)?;
    let bound = server.endpoint().clone();
    let mut slot = global().lock().expect("control server mutex");
    if let Some(previous) = slot.take() {
        previous.shutdown();
    }
    *slot = Some(server);
    Ok(bound)
}

pub fn stop_global() {
    if let Ok(mut slot) = global().lock()
        && let Some(previous) = slot.take()
    {
        previous.shutdown();
    }
}

pub fn with_global<R>(f: impl FnOnce(&ControlServer) -> R) -> Option<R> {
    let slot = global().lock().ok()?;
    slot.as_ref().map(f)
}
