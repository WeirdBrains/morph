//! The page bridge in a real WebKitGTK page (docs/bridge-api.md): page
//! scripts can't make host calls, host scripts can, handlers get the
//! engine's context, and page reads run in Morph's own script world even on
//! a page that poisons its builtins. Off-screen views, so it needs a display
//! but shows nothing. It runs only with `MORPH_BRIDGE_WEBKIT=1`, under
//! Xvfb in a clean environment (docs/bridge-api.md); otherwise it says so
//! and passes, so plain `cargo test` never touches a desktop session.

fn main() {
    #[cfg(target_os = "linux")]
    linux::run();
}

#[cfg(target_os = "linux")]
mod linux {
    use std::cell::RefCell;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use morph_webview::offscreen::OffscreenBackend;
    use morph_webview::page_read::{PageReads, Pending, ReadResult};
    use morph_webview::{Backend, BridgeContext, Event, FrameKind, Isolation, PageBridge, ReadRequest, Rect, Spec, Trust, ViewId, Want, WebView};

    const PAGE_BRIDGE: &str = include_str!("../../../packages/morph_webview_inapp/assets/morph-bridge.js");

    const PAGE: &str = r#"<!doctype html><html><head><title>Bridge test</title><meta name="author" content="Ada"></head>
<body><article><h1>Hello bridge</h1><p>The first paragraph of the article.</p><p>The second one.</p></article>
<a href="/frame">A frame</a> <a href="https://example.com/x">Elsewhere</a>
<iframe src="/frame"></iframe>
<script>
(async () => {
  const r = {};
  r.morphHostOnWindow = typeof window.MorphHost !== "undefined";
  r.hostHandlerInPageWorld = !!(window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.morphHost);
  r.deliverOnWindow = Object.getOwnPropertyNames(window).some((n) => n.startsWith("__morphHost"));
  try { await MorphBridge.post({ type: "host.read", payload: { steal: true } }); r.hostReadRefused = false; }
  catch (e) { r.hostReadRefused = /refused/.test(String(e.message)); }
  // A forged host envelope on the page's own handler: just a page message.
  window.webkit.messageHandlers.morphBridge.postMessage(JSON.stringify({ t: "guess", f: "main", o: location.origin, m: JSON.stringify({ id: "x", type: "host.read" }) }));
  r.ping = (await MorphBridge.post({ type: "ping" })).type;
  // Poison what a read in the page's world would use.
  const stringify = JSON.stringify, post = (m) => window.webkit.messageHandlers.morphBridge.postMessage(stringify(m));
  JSON.stringify = () => "\"poisoned\"";
  Array.prototype.push = function () { return 0; };
  Object.defineProperty(document, "title", { get: () => "Spoofed title" });
  post({ id: "report", type: "report", payload: r });
})();
</script></body></html>"#;

    const FRAME: &str = r#"<!doctype html><html><body><p>frame</p><script>
// Subframes have no MorphBridge, but can still post on the page channel.
window.webkit.messageHandlers.morphBridge.postMessage(JSON.stringify({ id: "frame", type: "frame-report", payload: {
  hostHandler: !!(window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.morphHost),
  morphHost: typeof window.MorphHost !== "undefined", morphBridge: typeof window.MorphBridge !== "undefined" } }));
// A host type from a subframe: refused like any page's.
window.webkit.messageHandlers.morphBridge.postMessage(JSON.stringify({ id: "frame-host", type: "host.read" }));
</script></body></html>"#;

    /// Serves `/`, `/frame` and `/big` (3 MB of text) until the test ends.
    fn serve() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let big = format!(
                "<!doctype html><html><head><title>Big</title></head><body><pre>{}</pre></body></html>",
                "0123456789abcdef\n".repeat(3 * 1024 * 1024 / 17)
            );
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).map(|n| n == 0).unwrap_or(true) || h == "\r\n" {
                        break;
                    }
                }
                let path = line.split_whitespace().nth(1).unwrap_or("/");
                let body: &str = match path {
                    "/" => PAGE,
                    "/frame" => FRAME,
                    "/big" => &big,
                    _ => "",
                };
                let mut stream = stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(body.as_bytes());
            }
        });
        port
    }

    type Seen = Rc<RefCell<Vec<(BridgeContext, serde_json::Value)>>>;

    fn bridge(seen: &Seen) -> PageBridge {
        let log = seen.clone();
        PageBridge::with_context(
            PAGE_BRIDGE,
            Rc::new(move |ctx: &BridgeContext, body: &str| {
                let msg: serde_json::Value = serde_json::from_str(body).ok()?;
                log.borrow_mut().push((ctx.clone(), msg.clone()));
                let id = msg.get("id")?.as_str()?.to_string();
                let kind = if msg["type"] == "ping" { "pong" } else { "ok" };
                Some(serde_json::json!({"id": id, "type": kind}).to_string())
            }),
        )
        .host_script("MorphHost.post({ type: 'host.hello', payload: { origin: location.origin } }).then((r) => MorphHost.post({ type: 'host.answered', payload: r.type }));")
    }

    fn pump_until(backend: &mut OffscreenBackend, view: ViewId, what: &str, seen: &Seen, mut done: impl FnMut(&[(ViewId, Event)]) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut events = Vec::new();
        while Instant::now() < deadline {
            events.extend(backend.poll().into_iter().filter(|(v, _)| *v == view));
            if done(&events) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        for (ctx, msg) in seen.borrow().iter() {
            eprintln!("seen: {:?} {:?} {msg}", ctx.trust, ctx.frame);
        }
        panic!("timed out waiting for {what}; events: {events:?}");
    }

    fn read(backend: &mut OffscreenBackend, reads: &mut PageReads, view: &dyn WebView, request: ReadRequest) -> ReadResult {
        let pending: Pending<ReadResult> = reads.read(view, request);
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            backend.poll();
            reads.tick(Instant::now(), |id| (id == view.id()).then_some(view));
            if let Some(r) = pending.try_take() {
                return r;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the read never answered");
    }

    pub fn run() {
        // Never on a desktop session by accident: only when asked, on a display of its own.
        if std::env::var_os("MORPH_BRIDGE_WEBKIT").is_none_or(|v| v != "1") {
            eprintln!("bridge_webkit: skipped (set MORPH_BRIDGE_WEBKIT=1 and run it under xvfb-run, see docs/bridge-api.md)");
            return;
        }
        if std::env::var_os("DISPLAY").is_none() {
            eprintln!("bridge_webkit: MORPH_BRIDGE_WEBKIT=1 but no DISPLAY: run it under xvfb-run");
            std::process::exit(1);
        }
        morph_webview::wry_linux::init().expect("GTK");
        let port = serve();
        let origin = format!("http://127.0.0.1:{port}");
        let seen: Seen = Rc::default();
        let mut backend = OffscreenBackend::new(1.0);
        backend.set_page_bridge(bridge(&seen));
        // A hidden host page: background, never shown.
        let view = backend
            .create(Spec {
                url: format!("{origin}/"),
                bounds: Rect::new(0.0, 0.0, 1024.0, 768.0),
                visible: false,
                background: true,
                ..Spec::default()
            })
            .expect("view");
        let id = view.id();
        let has = |seen: &Seen, kind: &str| seen.borrow().iter().any(|(_, m)| m["type"] == kind);
        pump_until(&mut backend, id, "the page's report and the host's answer", &seen, |events| {
            events.iter().any(|(_, e)| *e == Event::LoadFinished) && has(&seen, "report") && has(&seen, "host.answered") && has(&seen, "frame-report")
        });

        let seen = seen.borrow();
        for (ctx, msg) in seen.iter() {
            eprintln!("bridge_webkit: {:?} {:?} {}", ctx.trust, ctx.frame, msg);
        }
        // The forgery: nothing a page script sent reached a handler as a host
        // message, and no host-only type got through on the page channel.
        for (ctx, msg) in seen.iter() {
            let kind = msg["type"].as_str().unwrap_or("");
            if ctx.trust == Trust::Page {
                assert!(!kind.starts_with("host."), "a page made a host call: {msg}");
                assert_eq!(ctx.frame, FrameKind::Unknown);
            } else {
                assert!(kind.starts_with("host."), "{msg}");
                assert_eq!(ctx.frame, FrameKind::Main);
            }
            assert_eq!(ctx.view, id);
            assert_eq!(ctx.origin, origin);
            assert!(ctx.hidden && ctx.background);
        }
        let report = &seen.iter().find(|(_, m)| m["type"] == "report").unwrap().1["payload"];
        assert_eq!(report["morphHostOnWindow"], false, "{report}");
        assert_eq!(report["hostHandlerInPageWorld"], false, "{report}");
        assert_eq!(report["deliverOnWindow"], false, "{report}");
        assert_eq!(report["hostReadRefused"], true, "{report}");
        assert_eq!(report["ping"], "pong", "{report}");
        let frame = &seen.iter().find(|(_, m)| m["type"] == "frame-report").unwrap().1["payload"];
        assert_eq!(frame["hostHandler"], false, "{frame}");
        assert_eq!(frame["morphHost"], false, "{frame}");
        let (hello, msg) = seen.iter().find(|(_, m)| m["type"] == "host.hello").unwrap();
        assert_eq!(hello.trust, Trust::Host);
        assert_eq!(hello.url, format!("{origin}/"));
        assert_eq!(msg["payload"]["origin"], origin);
        assert_eq!(seen.iter().find(|(_, m)| m["type"] == "host.answered").unwrap().1["payload"], "ok");
        drop(seen);
        eprintln!("bridge_webkit: forgery refused, host channel works");

        // A read of the poisoned page: Morph's world doesn't see the poison.
        let mut reads = PageReads::new();
        let data = read(&mut backend, &mut reads, view.as_ref(), ReadRequest::new(Want::ALL)).expect("read");
        assert_eq!(data.isolation, Some(Isolation::ScriptWorld));
        assert_eq!(data.title.as_deref(), Some("Bridge test"), "the page's own title, not its spoof");
        assert!(data.text.as_deref().unwrap_or("").contains("Hello bridge"));
        let readable = data.readable.expect("readable");
        assert_eq!(readable.title, "Hello bridge");
        assert_eq!(readable.byline.as_deref(), Some("Ada"));
        assert!(readable.text.contains("The first paragraph of the article.\n\nThe second one."), "{}", readable.text);
        let links = data.links.expect("links");
        assert!(links.iter().any(|l| l.href == format!("{origin}/frame") && l.text == "A frame"), "{links:?}");
        assert!(data.html.as_deref().unwrap_or("").contains("<h1>Hello bridge</h1>"));
        assert_eq!(data.url, format!("{origin}/"));
        assert!(!data.truncated);
        eprintln!("bridge_webkit: read {} bytes from the poisoned page", data.bytes);

        // A large page: chunked, and cut to the bound when asked.
        view.load_url(&format!("{origin}/big")).unwrap();
        pump_until(&mut backend, id, "the big page", &Seen::default(), |events| events.iter().any(|(_, e)| *e == Event::LoadFinished));
        let whole = read(&mut backend, &mut reads, view.as_ref(), ReadRequest::new(Want { text: true, ..Want::default() })).expect("big read");
        let text = whole.text.unwrap_or_default();
        assert!(text.len() > 3_000_000, "{} bytes", text.len());
        assert!(!whole.truncated);
        let cut = read(
            &mut backend,
            &mut reads,
            view.as_ref(),
            ReadRequest { max_bytes: 1024 * 1024, ..ReadRequest::new(Want { text: true, html: true, ..Want::default() }) },
        )
        .expect("bounded read");
        assert!(cut.truncated && cut.bytes <= 1024 * 1024, "{} bytes", cut.bytes);
        eprintln!("bridge_webkit: {} bytes in chunks, {} when bounded", whole.bytes, cut.bytes);

        // Done: the hidden view goes.
        drop(view);
        backend.poll();
        eprintln!("bridge_webkit: ok");
    }
}
