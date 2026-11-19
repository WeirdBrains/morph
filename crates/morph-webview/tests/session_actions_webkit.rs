//! Throwaway storage and page actions in real WebKitGTK pages
//! (docs/bridge-api.md): an ephemeral view never sees the profile's cookies
//! or storage (nor another ephemeral view's), and click, fill, read back and
//! scroll work from Morph's script world on a page that poisons the
//! builtins they would use in its own. Off-screen views: it needs a display
//! but shows nothing. Runs only with `MORPH_BRIDGE_WEBKIT=1` under Xvfb in a
//! clean environment, like `bridge_webkit`; otherwise it says so and passes.

fn main() {
    #[cfg(target_os = "linux")]
    linux::run();
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    use morph_webview::offscreen::OffscreenBackend;
    use morph_webview::page_action::{Action, ActionError, ActionRequest, ActionResult, Locator, PageActions};
    use morph_webview::page_read::{PageReads, ReadResult};
    use morph_webview::{Backend, Event, Isolation, ReadRequest, Rect, Spec, Storage, ViewId, Want, WebView};

    const FORM: &str = r#"<!doctype html><html><head><title>Form</title><style>
#cover { position: fixed; left: 0; top: 0; width: 300px; height: 120px; background: #ccc; z-index: 10; }
#tall { height: 3000px; } body { margin: 0; }
</style></head><body>
<div id="cover">a cookie banner</div>
<button id="under" style="position:fixed;left:20px;top:20px">Hidden by the banner</button>
<div style="margin-top:160px">
<label for="email">Email</label> <input id="email" type="email">
<label>Password <input id="pw" type="password"></label>
<select id="size" aria-label="Size"><option value="s">Small</option><option value="l">Large</option></select>
<div id="notes" contenteditable="true" aria-label="Notes"></div>
<button id="send">Send</button> <button disabled>Disabled</button>
<p id="out">not clicked</p><p id="echo"></p><p id="trusted"></p>
</div>
<div id="tall"></div><p id="bottom">The end</p>
<script>
let clicks = 0;
document.getElementById("send").addEventListener("click", (e) => { clicks++; document.getElementById("out").textContent = "clicked " + clicks; document.getElementById("trusted").textContent = String(e.isTrusted); });
const realValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").get;
document.getElementById("email").addEventListener("input", (e) => { document.getElementById("echo").textContent = "typed " + realValue.call(e.target); });
document.getElementById("size").addEventListener("change", (e) => { document.title = "size " + e.target.value; });
// Poison everything an action in the page's own world would use.
Object.defineProperty(HTMLInputElement.prototype, "value", { set() { this.setAttribute("data-hacked", "1"); }, get() { return "hacked"; } });
EventTarget.prototype.dispatchEvent = function () { return false; };
Document.prototype.querySelectorAll = function () { return []; };
JSON.stringify = () => "\"poisoned\"";
Element.prototype.getBoundingClientRect = function () { return { x: 0, y: 0, left: 0, top: 0, width: 0, height: 0 }; };
</script></body></html>"#;

    /// Serves `/set` (a cookie and localStorage), `/show` (what the page
    /// has, in its title) and `/form` until the test ends.
    fn serve() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let mut cookie = String::new();
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).map(|n| n == 0).unwrap_or(true) || h == "\r\n" {
                        break;
                    }
                    if let Some(v) = h.strip_prefix("Cookie: ").or_else(|| h.strip_prefix("cookie: ")) {
                        cookie = v.trim().to_string();
                    }
                }
                let target = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (path, query) = target.split_once('?').unwrap_or((&target, ""));
                let (extra, body) = match path {
                    "/set" => (
                        format!("Set-Cookie: who={query}; Max-Age=3600; Path=/\r\n"),
                        format!("<!doctype html><title>set</title><script>localStorage.setItem('who', '{query}')</script>"),
                    ),
                    "/show" => (
                        String::new(),
                        format!(
                            "<!doctype html><title>pending</title><script>document.title = 'cookie=[' + {cookie:?} + '] storage=[' + (localStorage.getItem('who') || '') + ']'</script>"
                        ),
                    ),
                    "/form" => (String::new(), FORM.to_string()),
                    _ => (String::new(), String::new()),
                };
                let mut stream = stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(body.as_bytes());
            }
        });
        port
    }

    fn create(backend: &mut OffscreenBackend, url: &str, storage: Storage, data_dir: Option<std::path::PathBuf>) -> Box<dyn WebView> {
        backend
            .create(Spec {
                url: url.into(),
                data_dir,
                bounds: Rect::new(0.0, 0.0, 1024.0, 700.0),
                visible: false,
                background: true,
                storage,
                ..Spec::default()
            })
            .expect("view")
    }

    fn load(backend: &mut OffscreenBackend, view: &dyn WebView, url: Option<&str>) {
        if let Some(url) = url {
            view.load_url(url).unwrap();
        }
        let id: ViewId = view.id();
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if backend.poll().into_iter().any(|(v, e)| v == id && e == Event::LoadFinished) {
                // Let the page's own scripts run.
                let settle = Instant::now() + Duration::from_millis(300);
                while Instant::now() < settle {
                    backend.poll();
                    std::thread::sleep(Duration::from_millis(5));
                }
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("{url:?} never loaded");
    }

    fn title(backend: &mut OffscreenBackend, reads: &mut PageReads, view: &dyn WebView) -> String {
        let pending = reads.read(view, ReadRequest::new(Want { title: true, ..Want::default() }));
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            backend.poll();
            reads.tick(Instant::now(), |id| (id == view.id()).then_some(view));
            if let Some(r) = pending.try_take() {
                let r: ReadResult = r;
                return r.expect("read").title.unwrap_or_default();
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the read never answered");
    }

    fn act(backend: &mut OffscreenBackend, actions: &mut PageActions, view: &dyn WebView, action: Action) -> ActionResult {
        let pending = actions.run(view, ActionRequest::new(action));
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            backend.poll();
            actions.tick(Instant::now(), |id| (id == view.id()).then_some(view));
            if let Some(r) = pending.try_take() {
                return r;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the action never answered");
    }

    pub fn run() {
        if std::env::var_os("MORPH_BRIDGE_WEBKIT").is_none_or(|v| v != "1") {
            eprintln!("session_actions_webkit: skipped (set MORPH_BRIDGE_WEBKIT=1 and run it under xvfb-run, see docs/bridge-api.md)");
            return;
        }
        if std::env::var_os("DISPLAY").is_none() {
            eprintln!("session_actions_webkit: MORPH_BRIDGE_WEBKIT=1 but no DISPLAY: run it under xvfb-run");
            std::process::exit(1);
        }
        morph_webview::wry_linux::init().expect("GTK");
        let port = serve();
        let base = format!("http://127.0.0.1:{port}");
        let profile_dir = std::env::temp_dir().join(format!("morph-session-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&profile_dir);
        let mut backend = OffscreenBackend::new(1.0);
        assert!(backend.ephemeral_storage());
        let mut reads = PageReads::new();

        // The profile signs in: a cookie and site storage.
        let profile = create(&mut backend, &format!("{base}/set?profile"), Storage::Profile, Some(profile_dir.clone()));
        load(&mut backend, profile.as_ref(), None);
        load(&mut backend, profile.as_ref(), Some(&format!("{base}/show")));
        let seen = title(&mut backend, &mut reads, profile.as_ref());
        assert_eq!(seen, "cookie=[who=profile] storage=[profile]", "the profile keeps its own");

        // A throwaway page sees none of it, and keeps its own to itself.
        let eph = create(&mut backend, &format!("{base}/show"), Storage::Ephemeral, Some(profile_dir.clone()));
        load(&mut backend, eph.as_ref(), None);
        assert_eq!(title(&mut backend, &mut reads, eph.as_ref()), "cookie=[] storage=[]", "signed out");
        load(&mut backend, eph.as_ref(), Some(&format!("{base}/set?eph")));
        load(&mut backend, eph.as_ref(), Some(&format!("{base}/show")));
        assert_eq!(title(&mut backend, &mut reads, eph.as_ref()), "cookie=[who=eph] storage=[eph]", "its own store works");
        load(&mut backend, profile.as_ref(), Some(&format!("{base}/show")));
        assert_eq!(title(&mut backend, &mut reads, profile.as_ref()), "cookie=[who=profile] storage=[profile]", "nothing leaked back");
        let other = create(&mut backend, &format!("{base}/show"), Storage::Ephemeral, None);
        load(&mut backend, other.as_ref(), None);
        assert_eq!(title(&mut backend, &mut reads, other.as_ref()), "cookie=[] storage=[]", "one throwaway store per page");
        drop(eph);
        drop(other);
        backend.poll();
        eprintln!("session_actions_webkit: ephemeral pages are signed out and isolated");

        // Actions in a page that poisons its own world.
        let mut actions = PageActions::new();
        let view = create(&mut backend, &format!("{base}/form"), Storage::Ephemeral, None);
        load(&mut backend, view.as_ref(), None);
        let v = view.as_ref();
        let page = act(&mut backend, &mut actions, v, Action::Read(Locator::selector("html"))).unwrap().element.unwrap().rect.unwrap();
        assert_eq!(page.width, 1024.0, "a hidden page lays out at its bounds");
        let fill =
            act(&mut backend, &mut actions, v, Action::Fill { target: Locator::role("textbox", Some("email")), value: "ada@example.com".into() })
                .expect("fill");
        assert_eq!(fill.isolation, Isolation::ScriptWorld);
        let el = fill.element.unwrap();
        assert_eq!(
            (el.tag.as_str(), el.name.as_str(), el.value.as_deref()),
            ("input", "Email", Some("ada@example.com")),
            "read back from Morph's world"
        );
        let echo = act(&mut backend, &mut actions, v, Action::Read(Locator::selector("#echo"))).unwrap().element.unwrap();
        assert_eq!(echo.text, "typed ada@example.com", "the page's own input listener saw the value");
        let hacked = act(&mut backend, &mut actions, v, Action::Read(Locator::selector("#email[data-hacked]")));
        assert_eq!(hacked, Err(ActionError::NotFound), "the page's poisoned setter never ran");

        let pw =
            act(&mut backend, &mut actions, v, Action::Fill { target: Locator::name("Password"), value: "hunter2".into() }).unwrap().element.unwrap();
        assert!(pw.secret && pw.value.is_none(), "password values are not read back: {pw:?}");
        let size =
            act(&mut backend, &mut actions, v, Action::Fill { target: Locator::role("combobox", Some("Size")), value: "Large".into() }).unwrap();
        assert_eq!(size.element.unwrap().value.as_deref(), Some("l"), "a <select> by option text");
        let notes = act(&mut backend, &mut actions, v, Action::Fill { target: Locator::name("Notes"), value: "hello notes".into() }).unwrap();
        assert_eq!(notes.element.unwrap().value.as_deref(), Some("hello notes"), "contenteditable");

        let click = act(&mut backend, &mut actions, v, Action::Click(Locator::role("button", Some("Send")))).expect("click");
        assert_eq!(click.matches, 1);
        let out = act(&mut backend, &mut actions, v, Action::Read(Locator::selector("#out"))).unwrap().element.unwrap();
        assert_eq!(out.text, "clicked 1", "the page's click listener ran");
        let trusted = act(&mut backend, &mut actions, v, Action::Read(Locator::selector("#trusted"))).unwrap().element.unwrap();
        assert_eq!(trusted.text, "false", "synthetic events say so");

        assert!(
            matches!(act(&mut backend, &mut actions, v, Action::Click(Locator::text("Hidden by the banner"))), Err(ActionError::NotInteractable(why)) if why.starts_with("covered by div#cover"))
        );
        assert_eq!(
            act(&mut backend, &mut actions, v, Action::Click(Locator::role("button", Some("Disabled")))),
            Err(ActionError::NotInteractable("disabled".into()))
        );
        assert_eq!(act(&mut backend, &mut actions, v, Action::Click(Locator::role("button", Some("Nope")))), Err(ActionError::NotFound));
        assert!(matches!(act(&mut backend, &mut actions, v, Action::Click(Locator::selector("[[["))), Err(ActionError::BadRequest(_))));
        eprintln!("session_actions_webkit: fill, click, read back and refusals work in the script world");

        // Scroll: by a distance, then to an element.
        act(&mut backend, &mut actions, v, Action::ScrollBy { dx: 0.0, dy: -100000.0 }).unwrap();
        let by = act(&mut backend, &mut actions, v, Action::ScrollBy { dx: 0.0, dy: 500.0 }).expect("scroll by");
        assert!(by.scroll.moved && (by.scroll.y - 500.0).abs() < 1.0 && !by.scroll.inner, "{:?}", by.scroll);
        let to = act(&mut backend, &mut actions, v, Action::ScrollTo(Locator::selector("#bottom"))).expect("scroll to");
        assert!(to.scroll.moved && to.scroll.y > 2000.0, "{:?}", to.scroll);
        let r = to.element.unwrap().rect.unwrap();
        assert!(r.y >= 0.0 && r.y < 700.0, "the element is in view: {r:?}");
        eprintln!("session_actions_webkit: scroll {} then {}", by.scroll.y, to.scroll.y);

        drop(view);
        drop(profile);
        backend.poll();
        let _ = std::fs::remove_dir_all(&profile_dir);
        eprintln!("session_actions_webkit: ok");
    }
}
