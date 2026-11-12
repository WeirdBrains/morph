use crate::endpoint::{self, ControlEndpoint};
use crate::proto::web_view_control_server::{WebViewControl, WebViewControlServer};
use crate::proto::{
    Ack, ClientMessage, Error as ProtoError, EvaluateJavaScriptResult, ImeEvent, KeyEvent, NavigationEvent, PageMessage, PointerEvent, ServerMessage,
    client_message, server_message,
};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc as std_mpsc;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_stream::Stream;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("transport: {0}")]
    Transport(String),
    #[error("already stopped")]
    Stopped,
}

/// Backend that applies control messages to a concrete web view (WebKit, etc.).
pub trait ControlBackend: Send + Sync + 'static {
    fn navigate(&self, view_id: i64, url: String);
    fn go_back(&self, view_id: i64);
    fn go_forward(&self, view_id: i64);
    fn reload(&self, view_id: i64);
    fn pointer(&self, view_id: i64, event: PointerEvent);
    fn key(&self, view_id: i64, event: KeyEvent);
    fn ime(&self, view_id: i64, event: ImeEvent);
    fn evaluate_js(&self, view_id: i64, request_id: u64, script: String);
}

#[derive(Clone)]
struct ServiceState {
    backend: Arc<dyn ControlBackend>,
    outbound: broadcast::Sender<ServerMessage>,
    server_seq: Arc<AtomicU64>,
}

impl ServiceState {
    fn next_seq(&self) -> u64 {
        self.server_seq.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn emit(&self, view_id: i64, body: server_message::Body) {
        let msg = ServerMessage { seq: self.next_seq(), view_id, body: Some(body) };
        let _ = self.outbound.send(msg);
    }
}

struct ControlService {
    state: ServiceState,
}

#[tonic::async_trait]
impl WebViewControl for ControlService {
    type SessionStream = Pin<Box<dyn Stream<Item = Result<ServerMessage, Status>> + Send>>;

    async fn session(&self, request: Request<tonic::Streaming<ClientMessage>>) -> Result<Response<Self::SessionStream>, Status> {
        let mut inbound = request.into_inner();
        let mut rx = self.state.outbound.subscribe();
        let (tx, out_rx) = mpsc::channel::<Result<ServerMessage, Status>>(128);
        let state = self.state.clone();

        tokio::spawn(async move {
            let mut last_seq = 0_u64;
            loop {
                tokio::select! {
                    client = inbound.next() => {
                        match client {
                            Some(Ok(msg)) => {
                                if let Err(err) = handle_client(&state, &mut last_seq, msg) {
                                    let _ = tx.send(Ok(err)).await;
                                }
                            }
                            Some(Err(status)) => {
                                let _ = tx.send(Err(status)).await;
                                break;
                            }
                            None => break,
                        }
                    }
                    outbound = rx.recv() => {
                        match outbound {
                            Ok(msg) => {
                                if tx.send(Ok(msg)).await.is_err() {
                                    break;
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(out_rx))))
    }
}

fn handle_client(state: &ServiceState, last_seq: &mut u64, msg: ClientMessage) -> Result<(), ServerMessage> {
    if msg.seq == 0 || msg.seq <= *last_seq {
        return Err(ServerMessage {
            seq: state.next_seq(),
            view_id: msg.view_id,
            body: Some(server_message::Body::Error(ProtoError {
                client_seq: msg.seq,
                message: format!("out-of-order or missing seq (last={last_seq}, got={})", msg.seq),
            })),
        });
    }
    *last_seq = msg.seq;
    let view_id = msg.view_id;
    match msg.body {
        Some(client_message::Body::Navigate(n)) => state.backend.navigate(view_id, n.url),
        Some(client_message::Body::GoBack(_)) => state.backend.go_back(view_id),
        Some(client_message::Body::GoForward(_)) => state.backend.go_forward(view_id),
        Some(client_message::Body::Reload(_)) => state.backend.reload(view_id),
        Some(client_message::Body::Pointer(p)) => state.backend.pointer(view_id, p),
        Some(client_message::Body::Key(k)) => state.backend.key(view_id, k),
        Some(client_message::Body::Ime(i)) => state.backend.ime(view_id, i),
        Some(client_message::Body::EvaluateJavascript(e)) => state.backend.evaluate_js(view_id, e.request_id, e.script),
        Some(client_message::Body::PageMessage(p)) => {
            dispatch_page_message(state, view_id, &p.json);
        }
        None => {
            return Err(ServerMessage {
                seq: state.next_seq(),
                view_id,
                body: Some(server_message::Body::Error(ProtoError { client_seq: msg.seq, message: "empty client message".into() })),
            });
        }
    }
    state.emit(view_id, server_message::Body::Ack(Ack { client_seq: msg.seq }));
    Ok(())
}

fn dispatch_page_message(state: &ServiceState, view_id: i64, json: &str) {
    match morph_core::page_bridge::handle_page_message(json) {
        Ok(reply) => {
            let script = format!("window.MorphBridge && MorphBridge.deliver({reply});");
            state.backend.evaluate_js(view_id, 0, script);
            state.emit(view_id, server_message::Body::PageMessage(PageMessage { json: reply, handled: true }));
        }
        Err(err) => {
            state.emit(view_id, server_message::Body::Error(ProtoError { client_seq: 0, message: err }));
        }
    }
}

/// A running in-process control server.
pub struct ControlServer {
    endpoint: ControlEndpoint,
    state: ServiceState,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

pub type SharedServer = Arc<ControlServer>;

impl ControlServer {
    pub fn start(endpoint: Option<ControlEndpoint>, backend: Arc<dyn ControlBackend>) -> Result<Self, ControlError> {
        let endpoint = endpoint.unwrap_or_else(ControlEndpoint::default_for_process);
        if let ControlEndpoint::Unix(path) = &endpoint {
            endpoint::prepare_unix_socket(path)?;
        }

        let (outbound, _) = broadcast::channel(256);
        let state = ServiceState { backend, outbound, server_seq: Arc::new(AtomicU64::new(0)) };
        let (ready_tx, ready_rx) = std_mpsc::channel::<Result<ControlEndpoint, ControlError>>();
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let service_state = state.clone();
        let bind = endpoint.clone();

        let thread = std::thread::Builder::new()
            .name("morph-webview-control".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().thread_name("morph-control-rt").build() {
                    Ok(rt) => rt,
                    Err(err) => {
                        let _ = ready_tx.send(Err(ControlError::Io(err)));
                        return;
                    }
                };
                rt.block_on(async move {
                    if let Err(err) = serve(bind, service_state, ready_tx, stop_rx).await {
                        eprintln!("morph-webview-control: server exited: {err}");
                    }
                });
            })
            .map_err(ControlError::Io)?;

        let bound = match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Ok(bound)) => bound,
            Ok(Err(err)) => return Err(err),
            Err(std_mpsc::RecvTimeoutError::Timeout) => {
                return Err(ControlError::Transport("control server timed out during bind".into()));
            }
            Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                return Err(ControlError::Transport("control server thread exited during bind".into()));
            }
        };

        Ok(Self { endpoint: bound, state, shutdown: Some(stop_tx), thread: Some(thread) })
    }

    pub fn endpoint(&self) -> &ControlEndpoint {
        &self.endpoint
    }

    pub fn emit_navigation(&self, event: NavigationEvent, view_id: i64) {
        self.state.emit(view_id, server_message::Body::Navigation(event));
    }

    pub fn emit_page_message(&self, view_id: i64, json: &str) {
        dispatch_page_message(&self.state, view_id, json);
    }

    pub fn complete_evaluate_js(&self, view_id: i64, request_id: u64, result: &str, error: &str) {
        self.state.emit(
            view_id,
            server_message::Body::EvaluateJavascriptResult(EvaluateJavaScriptResult {
                request_id,
                result: result.to_string(),
                error: error.to_string(),
            }),
        );
    }

    pub fn shutdown(mut self) {
        self.request_stop_and_join();
    }

    fn request_stop_and_join(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(thread) = self.thread.take() {
            let (done_tx, done_rx) = std_mpsc::channel();
            std::thread::spawn(move || {
                let _ = thread.join();
                let _ = done_tx.send(());
            });
            if done_rx.recv_timeout(std::time::Duration::from_secs(3)).is_err() {
                eprintln!("morph-webview-control: server thread did not exit after shutdown");
            }
        }
        if let Some(path) = self.endpoint.unix_path() {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.request_stop_and_join();
    }
}

async fn serve(
    endpoint: ControlEndpoint,
    state: ServiceState,
    ready: std_mpsc::Sender<Result<ControlEndpoint, ControlError>>,
    stop: oneshot::Receiver<()>,
) -> Result<(), ControlError> {
    let svc = WebViewControlServer::new(ControlService { state });
    match endpoint {
        ControlEndpoint::Unix(path) => {
            let uds = tokio::net::UnixListener::bind(&path).map_err(ControlError::Io)?;
            if let Err(err) = endpoint::restrict_unix_socket(&path) {
                let _ = ready.send(Err(ControlError::Io(err)));
                return Ok(());
            }
            let incoming = tokio_stream::wrappers::UnixListenerStream::new(uds);
            let _ = ready.send(Ok(ControlEndpoint::Unix(path.clone())));
            tonic::transport::Server::builder()
                .add_service(svc)
                .serve_with_incoming_shutdown(incoming, async {
                    let _ = stop.await;
                })
                .await
                .map_err(|e| ControlError::Transport(e.to_string()))?;
            let _ = std::fs::remove_file(&path);
            Ok(())
        }
        ControlEndpoint::Loopback { port } => {
            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
            let listener = tokio::net::TcpListener::bind(addr).await.map_err(ControlError::Io)?;
            let bound = listener.local_addr().map_err(ControlError::Io)?;
            let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
            let _ = ready.send(Ok(ControlEndpoint::Loopback { port: bound.port() }));
            tonic::transport::Server::builder()
                .add_service(svc)
                .serve_with_incoming_shutdown(incoming, async {
                    let _ = stop.await;
                })
                .await
                .map_err(|e| ControlError::Transport(e.to_string()))?;
            Ok(())
        }
        ControlEndpoint::NamedPipe(name) => {
            #[cfg(windows)]
            {
                crate::named_pipe::serve(name, svc, ready, stop).await
            }
            #[cfg(not(windows))]
            {
                drop(svc);
                drop(stop);
                let _ = ready.send(Err(ControlError::Transport(format!("named-pipe transport is only available on Windows (got {name})"))));
                Ok(())
            }
        }
    }
}

/// Connect a tonic client to a bound endpoint (used by tests and future Rust callers).
pub async fn connect(
    endpoint: &ControlEndpoint,
) -> Result<crate::proto::web_view_control_client::WebViewControlClient<tonic::transport::Channel>, ControlError> {
    use crate::proto::web_view_control_client::WebViewControlClient;
    #[cfg(windows)]
    use hyper_util::rt::TokioIo;
    #[cfg(windows)]
    use tonic::transport::Uri;
    #[cfg(windows)]
    use tower::service_fn;

    match endpoint {
        ControlEndpoint::Unix(path) => {
            let url = format!("unix://{}", path.display());
            let channel = tonic::transport::Endpoint::from_shared(url)
                .map_err(|e| ControlError::Transport(e.to_string()))?
                .connect_timeout(std::time::Duration::from_secs(3))
                .connect()
                .await
                .map_err(|e| ControlError::Transport(e.to_string()))?;
            Ok(WebViewControlClient::new(channel))
        }
        ControlEndpoint::Loopback { port } => {
            let url = format!("http://127.0.0.1:{port}");
            let channel = tonic::transport::Endpoint::from_shared(url)
                .map_err(|e| ControlError::Transport(e.to_string()))?
                .connect_timeout(std::time::Duration::from_secs(3))
                .connect()
                .await
                .map_err(|e| ControlError::Transport(e.to_string()))?;
            Ok(WebViewControlClient::new(channel))
        }
        ControlEndpoint::NamedPipe(name) => {
            #[cfg(windows)]
            {
                let name = name.clone();
                let channel = tonic::transport::Endpoint::try_from("http://[::]:1")
                    .map_err(|e| ControlError::Transport(e.to_string()))?
                    .connect_timeout(std::time::Duration::from_secs(3))
                    .connect_with_connector(service_fn(move |_: Uri| {
                        let name = name.clone();
                        async move {
                            let pipe = tokio::net::windows::named_pipe::ClientOptions::new().open(&name)?;
                            Ok::<_, std::io::Error>(TokioIo::new(pipe))
                        }
                    }))
                    .await
                    .map_err(|e| ControlError::Transport(e.to_string()))?;
                Ok(WebViewControlClient::new(channel))
            }
            #[cfg(not(windows))]
            {
                Err(ControlError::Transport(format!("named pipes are only available on Windows (got {name})")))
            }
        }
    }
}
