use morph_webview_control::endpoint::ControlEndpoint;
use morph_webview_control::proto::client_message::Body as ClientBody;
use morph_webview_control::proto::server_message::Body as ServerBody;
use morph_webview_control::proto::{
    ClientMessage, EvaluateJavaScript, ImeEvent, KeyEvent, Navigate, PointerEvent, web_view_control_client::WebViewControlClient,
};
use morph_webview_control::server::{ControlBackend, ControlServer, connect};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Default)]
struct Recording {
    navigates: Mutex<Vec<(i64, String)>>,
    pointers: Mutex<Vec<(i64, PointerEvent)>>,
    keys: Mutex<Vec<(i64, KeyEvent)>>,
    imes: Mutex<Vec<(i64, String)>>,
    scripts: Mutex<Vec<(i64, u64, String)>>,
}

impl ControlBackend for Recording {
    fn navigate(&self, view_id: i64, url: String) {
        self.navigates.lock().unwrap().push((view_id, url));
    }
    fn go_back(&self, _view_id: i64) {}
    fn go_forward(&self, _view_id: i64) {}
    fn reload(&self, _view_id: i64) {}
    fn pointer(&self, view_id: i64, event: morph_webview_control::proto::PointerEvent) {
        self.pointers.lock().unwrap().push((view_id, event));
    }
    fn key(&self, view_id: i64, event: morph_webview_control::proto::KeyEvent) {
        self.keys.lock().unwrap().push((view_id, event));
    }
    fn ime(&self, view_id: i64, event: morph_webview_control::proto::ImeEvent) {
        self.imes.lock().unwrap().push((view_id, format!("{}:{}", event.phase, event.text)));
    }
    fn evaluate_js(&self, view_id: i64, request_id: u64, script: String) {
        self.scripts.lock().unwrap().push((view_id, request_id, script));
    }
}

fn test_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().enable_all().worker_threads(2).build().unwrap()
}

fn temp_endpoint() -> ControlEndpoint {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    #[cfg(unix)]
    {
        let path = std::env::temp_dir().join(format!("morph-control-test-{}-{}.sock", std::process::id(), n));
        let _ = std::fs::remove_file(&path);
        ControlEndpoint::Unix(path)
    }
    #[cfg(windows)]
    {
        ControlEndpoint::NamedPipe(format!(r"\\.\pipe\morph-control-test-{}-{}", std::process::id(), n))
    }
    #[cfg(not(any(unix, windows)))]
    {
        ControlEndpoint::Loopback { port: 0 }
    }
}

fn client_msg(seq: u64, view_id: i64, body: ClientBody) -> ClientMessage {
    ClientMessage { seq, view_id, body: Some(body) }
}

#[test]
fn session_orders_navigation_and_input_and_handles_page_bridge() {
    let backend = Arc::new(Recording::default());
    let endpoint = temp_endpoint();
    let server = ControlServer::start(Some(endpoint.clone()), backend.clone()).expect("start server");

    let rt = test_runtime();
    rt.block_on(async {
        let mut client: WebViewControlClient<_> =
            timeout(Duration::from_secs(3), connect(server.endpoint())).await.expect("connect timeout").expect("connect");
        let (tx, rx) = mpsc::channel(16);
        let inbound = ReceiverStream::new(rx);
        let response = client.session(inbound).await.expect("session");
        let mut outbound = response.into_inner();

        tx.send(client_msg(1, 7, ClientBody::Navigate(Navigate { url: "https://example.com/".into() }))).await.unwrap();
        tx.send(client_msg(
            2,
            7,
            ClientBody::Pointer(PointerEvent { phase: "down".into(), x: 10.0, y: 20.0, button: 1, buttons: 1, ..Default::default() }),
        ))
        .await
        .unwrap();
        tx.send(client_msg(
            3,
            7,
            ClientBody::Key(KeyEvent {
                phase: "down".into(),
                key: "A".into(),
                text: "a".into(),
                hardware_keycode: 0,
                ctrl: false,
                alt: false,
                shift: false,
                meta: false,
                repeat: false,
            }),
        ))
        .await
        .unwrap();

        let mut acks = Vec::new();
        while acks.len() < 3 {
            let msg = outbound.next().await.expect("server message").expect("ok");
            if let Some(ServerBody::Ack(ack)) = msg.body {
                acks.push(ack.client_seq);
            }
        }
        assert_eq!(acks, vec![1, 2, 3]);

        server.emit_navigation(
            morph_webview_control::proto::NavigationEvent {
                url: "https://example.com/".into(),
                title: "Example Domain".into(),
                can_go_back: false,
                can_go_forward: false,
                is_loading: false,
                progress: 1.0,
            },
            7,
        );
        let nav = loop {
            let msg = outbound.next().await.expect("nav").expect("ok");
            if let Some(ServerBody::Navigation(n)) = msg.body {
                break n;
            }
        };
        assert_eq!(nav.url, "https://example.com/");
        assert_eq!(nav.title, "Example Domain");

        server.emit_page_message(7, r#"{"id":"1","type":"ping"}"#);
        let page = loop {
            let msg = outbound.next().await.expect("page").expect("ok");
            if let Some(ServerBody::PageMessage(p)) = msg.body {
                break p;
            }
        };
        assert!(page.handled);
        assert!(page.json.contains("pong"), "reply {}", page.json);

        // Out-of-order seq is rejected.
        tx.send(client_msg(3, 7, ClientBody::Reload(Default::default()))).await.unwrap();
        let err = loop {
            let msg = outbound.next().await.expect("err").expect("ok");
            if let Some(ServerBody::Error(e)) = msg.body {
                break e;
            }
        };
        assert!(err.message.contains("out-of-order"), "{}", err.message);

        drop(tx);
    });

    assert_eq!(*backend.navigates.lock().unwrap(), vec![(7, "https://example.com/".into())]);
    {
        let pointers = backend.pointers.lock().unwrap();
        assert_eq!(pointers.len(), 1);
        assert_eq!(pointers[0].0, 7);
        assert_eq!(pointers[0].1.phase, "down");
        assert_eq!(pointers[0].1.button, 1);
        assert_eq!(pointers[0].1.buttons, 1);
    }
    {
        let keys = backend.keys.lock().unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].0, 7);
        assert_eq!(keys[0].1.phase, "down");
        assert_eq!(keys[0].1.key, "A");
        assert_eq!(keys[0].1.text, "a");
    }
    let scripts = backend.scripts.lock().unwrap();
    assert!(scripts.iter().any(|(_, _, s)| s.contains("MorphBridge.deliver") && s.contains("pong")), "{scripts:?}");

    server.shutdown();
}

#[test]
fn evaluate_javascript_result_is_correlated_by_request_id() {
    let backend = Arc::new(Recording::default());
    let endpoint = temp_endpoint();
    let server = ControlServer::start(Some(endpoint.clone()), backend.clone()).unwrap();
    let rt = test_runtime();
    rt.block_on(async {
        let mut client = timeout(Duration::from_secs(5), connect(server.endpoint())).await.expect("connect timeout").unwrap();
        let (tx, rx) = mpsc::channel(8);
        let mut outbound =
            timeout(Duration::from_secs(5), client.session(ReceiverStream::new(rx))).await.expect("session timeout").unwrap().into_inner();
        tx.send(client_msg(1, 4, ClientBody::EvaluateJavascript(EvaluateJavaScript { request_id: 99, script: "1+1".into() }))).await.unwrap();
        // Wait for ack so the backend saw the call.
        loop {
            let msg = outbound.next().await.unwrap().unwrap();
            if matches!(msg.body, Some(ServerBody::Ack(_))) {
                break;
            }
        }
        server.complete_evaluate_js(4, 99, "2", "");
        let result = loop {
            let msg = outbound.next().await.unwrap().unwrap();
            if let Some(ServerBody::EvaluateJavascriptResult(r)) = msg.body {
                break r;
            }
        };
        assert_eq!(result.request_id, 99);
        assert_eq!(result.result, "2");
        drop(tx);
    });
    server.shutdown();
}

#[test]
fn loopback_endpoint_accepts_a_session() {
    let backend = Arc::new(Recording::default());
    let server = ControlServer::start(Some(ControlEndpoint::Loopback { port: 0 }), backend).unwrap();
    match server.endpoint() {
        ControlEndpoint::Loopback { port } => assert!(*port != 0),
        other => panic!("expected loopback, got {other:?}"),
    }
    let rt = test_runtime();
    rt.block_on(async {
        timeout(Duration::from_secs(3), connect(server.endpoint())).await.expect("connect loopback timeout").expect("connect loopback");
    });
    server.shutdown();
}

#[cfg(windows)]
#[test]
fn named_pipe_endpoint_accepts_a_session() {
    let backend = Arc::new(Recording::default());
    let name = format!(r"\\.\pipe\morph-control-test-{}", std::process::id());
    let server = ControlServer::start(Some(ControlEndpoint::NamedPipe(name)), backend).expect("start named pipe");
    match server.endpoint() {
        ControlEndpoint::NamedPipe(path) => assert!(path.contains("morph-control-test-")),
        other => panic!("expected named pipe, got {other:?}"),
    }
    let rt = test_runtime();
    rt.block_on(async {
        timeout(Duration::from_secs(5), connect(server.endpoint())).await.expect("connect named pipe timeout").expect("connect named pipe");
    });
    server.shutdown();
}

#[test]
fn pointer_hover_leave_cursor_and_keyboard_roundtrip() {
    let backend = Arc::new(Recording::default());
    let endpoint = temp_endpoint();
    let server = ControlServer::start(Some(endpoint.clone()), backend.clone()).expect("start server");

    let rt = test_runtime();
    rt.block_on(async {
        let mut client: WebViewControlClient<_> =
            timeout(Duration::from_secs(3), connect(server.endpoint())).await.expect("connect timeout").expect("connect");
        let (tx, rx) = mpsc::channel(32);
        let mut outbound = client.session(ReceiverStream::new(rx)).await.expect("session").into_inner();

        tx.send(client_msg(1, 3, ClientBody::Pointer(PointerEvent { phase: "enter".into(), x: 12.0, y: 24.0, ..Default::default() }))).await.unwrap();
        tx.send(client_msg(2, 3, ClientBody::Pointer(PointerEvent { phase: "hover".into(), x: 40.0, y: 50.0, ctrl: true, ..Default::default() })))
            .await
            .unwrap();
        tx.send(client_msg(3, 3, ClientBody::Pointer(PointerEvent { phase: "leave".into(), x: 40.0, y: 50.0, ..Default::default() }))).await.unwrap();
        tx.send(client_msg(
            4,
            3,
            ClientBody::Pointer(PointerEvent { phase: "scroll".into(), x: 10.0, y: 10.0, scroll_delta_y: 80.0, ..Default::default() }),
        ))
        .await
        .unwrap();
        tx.send(client_msg(
            5,
            3,
            ClientBody::Pointer(PointerEvent { phase: "down".into(), x: 8.0, y: 9.0, button: 1, buttons: 1, shift: true, ..Default::default() }),
        ))
        .await
        .unwrap();
        tx.send(client_msg(
            6,
            3,
            ClientBody::Pointer(PointerEvent { phase: "move".into(), x: 18.0, y: 19.0, buttons: 1, shift: true, ..Default::default() }),
        ))
        .await
        .unwrap();
        tx.send(client_msg(
            7,
            3,
            ClientBody::Pointer(PointerEvent { phase: "up".into(), x: 18.0, y: 19.0, button: 1, shift: true, ..Default::default() }),
        ))
        .await
        .unwrap();
        tx.send(client_msg(
            8,
            3,
            ClientBody::Key(KeyEvent { phase: "down".into(), key: "A".into(), text: "A".into(), shift: true, ..Default::default() }),
        ))
        .await
        .unwrap();
        tx.send(client_msg(9, 3, ClientBody::Key(KeyEvent { phase: "up".into(), key: "A".into(), shift: true, ..Default::default() })))
            .await
            .unwrap();
        tx.send(client_msg(
            10,
            3,
            ClientBody::Key(KeyEvent { phase: "down".into(), key: "V".into(), text: "v".into(), ctrl: true, ..Default::default() }),
        ))
        .await
        .unwrap();
        tx.send(client_msg(11, 3, ClientBody::Ime(ImeEvent { phase: "compose_update".into(), text: "´".into() }))).await.unwrap();
        tx.send(client_msg(12, 3, ClientBody::Ime(ImeEvent { phase: "commit".into(), text: "é".into() }))).await.unwrap();

        let mut acks = 0_u32;
        while acks < 12 {
            let msg = outbound.next().await.expect("server message").expect("ok");
            if matches!(msg.body, Some(ServerBody::Ack(_))) {
                acks += 1;
            }
        }

        server.emit_cursor(3, "pointer");
        let cursor = loop {
            let msg = outbound.next().await.expect("cursor").expect("ok");
            if let Some(ServerBody::Cursor(c)) = msg.body {
                break c;
            }
        };
        assert_eq!(cursor.shape, "pointer");
        drop(tx);
    });

    let phases: Vec<String> = backend.pointers.lock().unwrap().iter().map(|(_, e)| e.phase.clone()).collect();
    assert_eq!(phases, vec!["enter", "hover", "leave", "scroll", "down", "move", "up"]);
    let pointers = backend.pointers.lock().unwrap();
    assert_eq!(pointers[1].1.x, 40.0);
    assert!(pointers[1].1.ctrl);
    assert_eq!(pointers[1].1.buttons, 0);
    assert_eq!(pointers[3].1.scroll_delta_y, 80.0);
    assert_eq!(pointers[4].1.button, 1);
    assert_eq!(pointers[4].1.buttons, 1);
    assert!(pointers[4].1.shift);
    assert_eq!(pointers[5].1.phase, "move");
    assert_eq!(pointers[5].1.buttons, 1);

    let keys = backend.keys.lock().unwrap();
    assert_eq!(keys[0].1.key, "A");
    assert_eq!(keys[0].1.text, "A");
    assert!(keys[0].1.shift);
    assert!(!keys[0].1.repeat);
    assert_eq!(keys[2].1.key, "V");
    assert!(keys[2].1.ctrl);

    assert_eq!(*backend.imes.lock().unwrap(), vec![(3, "compose_update:´".into()), (3, "commit:é".into())]);

    server.shutdown();
}
