//! The password saver's host script in a real WebKitGTK page
//! (morph_webview::login_form): a shown password field is reported once, a
//! submitted login reaches Rust on the host channel only, a login form in a
//! frame never does, and the fill actions put a saved login into the page
//! from Morph's script world. Off-screen, so it needs a display but shows
//! nothing; it runs only with `MORPH_BRIDGE_WEBKIT=1` under Xvfb in a clean
//! environment (docs/bridge-api.md), and otherwise says so and passes.

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

    use morph_webview::login_form;
    use morph_webview::offscreen::OffscreenBackend;
    use morph_webview::page_action::{ActionResult, PageActions};
    use morph_webview::page_read::Pending;
    use morph_webview::{Backend, BridgeContext, FrameKind, PageBridge, Rect, Spec, Trust, WebView};

    const PAGE_BRIDGE: &str = include_str!("../../../packages/morph_webview_inapp/assets/morph-bridge.js");

    const PAGE: &str = r#"<!doctype html><html><head><title>Login test</title></head><body>
<form id="login" action="/done" method="post">
  <input id="who" name="username" type="text" autocomplete="username">
  <input id="pw" name="password" type="password" autocomplete="current-password">
  <button type="submit">Sign in</button>
</form>
<iframe src="/frame"></iframe>
<script>
const post = (m) => window.webkit.messageHandlers.morphBridge.postMessage(JSON.stringify(m));
document.getElementById("login").addEventListener("submit", (e) => e.preventDefault());
document.getElementById("pw").addEventListener("input", (e) => post({ id: "f", type: "pw-input", payload: { len: e.target.value.length } }));
// A page script claiming a login on the page channel: never a host message.
post({ id: "forged", type: "host.saver.submit", payload: { username: "eve", password: "forged" } });
window.addEventListener("message", (e) => {
  if (e.data !== "sign-in") return;
  document.getElementById("who").value = "amy";
  document.getElementById("pw").value = "s3cret-pass";
  document.getElementById("login").requestSubmit();
});
setTimeout(() => window.postMessage("sign-in", "*"), 300);
</script></body></html>"#;

    const FRAME: &str = r#"<!doctype html><html><body>
<form id="f"><input name="user" value="frame-user"><input id="fp" type="password" value="frame-secret"><button>Go</button></form>
<script>
document.getElementById("f").addEventListener("submit", (e) => e.preventDefault());
setTimeout(() => document.getElementById("f").requestSubmit(), 200);
</script></body></html>"#;

    fn serve() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                // One thread a connection: a preconnected socket that never
                // sends a request must not hold up the next one.
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() {
                        return;
                    }
                    loop {
                        let mut h = String::new();
                        if reader.read_line(&mut h).map(|n| n == 0).unwrap_or(true) || h == "\r\n" {
                            break;
                        }
                    }
                    let body = match line.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap_or("/") {
                        "/" => PAGE,
                        "/frame" => FRAME,
                        _ => "",
                    };
                    let mut stream = stream;
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                });
            }
        });
        port
    }

    type Seen = Rc<RefCell<Vec<(BridgeContext, serde_json::Value)>>>;

    fn pump(backend: &mut OffscreenBackend, actions: &mut PageActions, view: &dyn WebView, what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            backend.poll();
            actions.tick(Instant::now(), |id| (id == view.id()).then_some(view));
            if done() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for {what}");
    }

    pub fn run() {
        if std::env::var_os("MORPH_BRIDGE_WEBKIT").is_none_or(|v| v != "1") {
            eprintln!("login_webkit: skipped (set MORPH_BRIDGE_WEBKIT=1 and run it under xvfb-run, see docs/bridge-api.md)");
            return;
        }
        if std::env::var_os("DISPLAY").is_none() {
            eprintln!("login_webkit: MORPH_BRIDGE_WEBKIT=1 but no DISPLAY: run it under xvfb-run");
            std::process::exit(1);
        }
        morph_webview::wry_linux::init().expect("GTK");
        let port = serve();
        let origin = format!("http://127.0.0.1:{port}");
        let seen: Seen = Rc::default();
        let log = seen.clone();
        let bridge = PageBridge::with_context(
            PAGE_BRIDGE,
            Rc::new(move |ctx: &BridgeContext, body: &str| {
                let msg: serde_json::Value = serde_json::from_str(body).ok()?;
                log.borrow_mut().push((ctx.clone(), msg));
                if login_form::is_saver_message(body) { login_form::reply(body) } else { None }
            }),
        )
        .host_script(login_form::HOST_SCRIPT);
        let mut backend = OffscreenBackend::new(1.0);
        backend.set_page_bridge(bridge);
        let view = backend
            .create(Spec {
                url: format!("{origin}/"),
                bounds: Rect::new(0.0, 0.0, 1024.0, 768.0),
                visible: false,
                background: true,
                ..Spec::default()
            })
            .expect("view");
        let mut actions = PageActions::new();
        let count = |kind: &str| seen.borrow().iter().filter(|(c, m)| c.trust == Trust::Host && m["type"] == kind).count();
        pump(&mut backend, &mut actions, view.as_ref(), "the form and the submitted login", || {
            count(login_form::FORM) > 0 && count(login_form::SUBMIT) > 0
        });
        // Give the frame's submit time to arrive, if it were going to.
        let settle = Instant::now() + Duration::from_millis(800);
        pump(&mut backend, &mut actions, view.as_ref(), "settling", || Instant::now() >= settle);

        {
            let seen = seen.borrow();
            for (ctx, msg) in seen.iter() {
                let kind = msg["type"].as_str().unwrap_or("");
                if ctx.trust == Trust::Host {
                    assert_eq!(ctx.frame, FrameKind::Main);
                    assert_eq!(ctx.view, view.id());
                    assert_eq!(ctx.origin, origin);
                    assert!(kind == login_form::FORM || kind == login_form::SUBMIT, "{msg}");
                } else {
                    assert!(!kind.starts_with("host."), "the page channel never carries a host message: {msg}");
                }
            }
            let host: Vec<_> = seen.iter().filter(|(c, _)| c.trust == Trust::Host).map(|(_, m)| m.clone()).collect();
            assert_eq!(host.iter().filter(|m| m["type"] == login_form::FORM).count(), 1, "the form is reported once");
            let submits: Vec<_> = host.iter().filter(|m| m["type"] == login_form::SUBMIT).collect();
            assert_eq!(submits.len(), 1, "one login, and none from the frame: {submits:?}");
            assert_eq!(submits[0]["payload"]["username"], "amy");
            assert_eq!(submits[0]["payload"]["password"], "s3cret-pass");
            assert!(!host.iter().any(|m| m.to_string().contains("frame-secret")), "a frame's login never reaches Rust");
        }
        eprintln!("login_webkit: form and login heard on the host channel only, not from the frame");

        // Fill a saved login back in, from Morph's script world.
        view.load_url(&format!("{origin}/?again")).unwrap();
        seen.borrow_mut().clear();
        pump(&mut backend, &mut actions, view.as_ref(), "the page again", || count(login_form::FORM) > 0);
        let pending: Vec<Pending<ActionResult>> =
            login_form::fill_actions(&origin, "bob", "filled-pass").into_iter().map(|r| actions.run(view.as_ref(), r)).collect();
        pump(&mut backend, &mut actions, view.as_ref(), "the fill", || pending.iter().all(|p| p.is_ready()));
        let results: Vec<ActionResult> = pending.iter().map(|p| p.try_take().unwrap()).collect();
        let user = results[0].as_ref().expect("username filled");
        assert_eq!(user.element.as_ref().and_then(|e| e.value.as_deref()), Some("bob"));
        let pw = results[1].as_ref().expect("password filled");
        let el = pw.element.as_ref().unwrap();
        assert!(el.secret && el.value.is_none(), "a password field is never read back");
        pump(&mut backend, &mut actions, view.as_ref(), "the page seeing the password", || {
            seen.borrow().iter().any(|(_, m)| m["type"] == "pw-input" && m["payload"]["len"] == "filled-pass".len())
        });
        // Pinned to the origin: another origin's fill is refused.
        let wrong = actions.run(view.as_ref(), login_form::fill_actions("https://example.com", "", "x").remove(0));
        pump(&mut backend, &mut actions, view.as_ref(), "the refusal", || wrong.is_ready());
        assert!(wrong.try_take().unwrap().is_err(), "a fill for another origin is refused");
        drop(view);
        backend.poll();
        eprintln!("login_webkit: ok");
    }
}
