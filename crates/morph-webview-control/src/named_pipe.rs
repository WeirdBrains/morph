//! Windows named-pipe accept loop for the in-process control server.

#![cfg(windows)]

use crate::endpoint::ControlEndpoint;
use crate::proto::web_view_control_server::WebViewControlServer;
use crate::server::{ControlError, ControlService};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::server::Connected;

/// Tokio named pipe wrapped so tonic can accept it as an incoming IO resource.
pub struct NamedPipeConn {
    inner: NamedPipeServer,
}

impl Connected for NamedPipeConn {
    type ConnectInfo = ();

    fn connect_info(&self) -> Self::ConnectInfo {}
}

impl AsyncRead for NamedPipeConn {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for NamedPipeConn {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, std::io::Error>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

fn open_server(name: &str, first: bool) -> std::io::Result<NamedPipeServer> {
    ServerOptions::new().first_pipe_instance(first).reject_remote_clients(true).create(name)
}

/// Accept connections on `name`, signal `ready`, and serve until `stop`.
pub async fn serve(
    name: String,
    svc: WebViewControlServer<ControlService>,
    ready: std::sync::mpsc::Sender<Result<ControlEndpoint, ControlError>>,
    stop: oneshot::Receiver<()>,
) -> Result<(), ControlError> {
    let first = match open_server(&name, true) {
        Ok(server) => server,
        Err(err) => {
            let _ = ready.send(Err(ControlError::Io(err)));
            return Ok(());
        }
    };
    let (tx, rx) = mpsc::channel::<std::io::Result<NamedPipeConn>>(1);
    let accept_name = name.clone();
    tokio::spawn(async move {
        let mut server = first;
        loop {
            if let Err(err) = server.connect().await {
                let _ = tx.send(Err(err)).await;
                break;
            }
            let connected = server;
            match open_server(&accept_name, false) {
                Ok(next) => server = next,
                Err(err) => {
                    let _ = tx.send(Ok(NamedPipeConn { inner: connected })).await;
                    let _ = tx.send(Err(err)).await;
                    break;
                }
            }
            if tx.send(Ok(NamedPipeConn { inner: connected })).await.is_err() {
                break;
            }
        }
    });
    let _ = ready.send(Ok(ControlEndpoint::NamedPipe(name)));
    tonic::transport::Server::builder()
        .add_service(svc)
        .serve_with_incoming_shutdown(ReceiverStream::new(rx), async {
            let _ = stop.await;
        })
        .await
        .map_err(|e| ControlError::Transport(e.to_string()))?;
    Ok(())
}
