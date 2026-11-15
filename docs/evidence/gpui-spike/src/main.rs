use gpui::{App, Bounds, Context, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size};
use std::time::Duration;
use wry::{
    Rect, WebViewBuilder,
    dpi::{LogicalPosition, LogicalSize},
};

struct Shell {
    webview: wry::WebView,
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let vp = window.viewport_size();
        let _ = self.webview.set_bounds(Rect {
            position: LogicalPosition::new(0.0, 40.0).into(),
            size: LogicalSize::new(f32::from(vp.width) as f64, (f32::from(vp.height) - 40.0) as f64).into(),
        });
        div().size_full().bg(rgb(0x202020)).child(div().h(px(40.)).text_color(rgb(0xffffff)).child("Morph spike"))
    }
}

fn main() {
    gtk::init().expect("gtk");
    let url = std::env::args().nth(1).unwrap_or("https://example.com".into());
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
        cx.open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() }, |window, cx| {
            let webview = WebViewBuilder::new()
                .with_url(&url)
                .with_bounds(Rect { position: LogicalPosition::new(0.0, 40.0).into(), size: LogicalSize::new(1000.0, 660.0).into() })
                .with_document_title_changed_handler(|t| eprintln!("TITLE {t}"))
                .with_new_window_req_handler(|url, features| {
                    use gtk::prelude::*;
                    use webkit2gtk::WebViewExt;
                    use wry::{WebViewBuilderExtUnix, WebViewExtUnix};
                    eprintln!("POPUP {url}");
                    let win = gtk::Window::new(gtk::WindowType::Toplevel);
                    win.set_default_size(400, 300);
                    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
                    win.add(&vbox);
                    let wv = WebViewBuilder::new()
                        .with_related_view(features.opener.webview.clone())
                        .with_document_title_changed_handler(|t| eprintln!("PTITLE {t}"))
                        .build_gtk(&vbox)
                        .expect("popup");
                    let inner = wv.webview();
                    win.show_all();
                    let w2 = win.clone();
                    inner.connect_close(move |_| unsafe { w2.destroy() });
                    Box::leak(Box::new(wv));
                    wry::NewWindowResponse::Create { webview: inner }
                })
                .build_as_child(&XlibParent::from(&*window))
                .expect("webview");
            cx.new(|_| Shell { webview })
        })
        .unwrap();
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(8)).await;
                while gtk::events_pending() {
                    gtk::main_iteration_do(false);
                }
            }
        })
        .detach();
    });
}

struct XlibParent(raw_window_handle::RawWindowHandle);
impl From<&Window> for XlibParent {
    fn from(w: &Window) -> Self {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle, XlibWindowHandle};
        let raw = HasWindowHandle::window_handle(w).expect("handle").as_raw();
        match raw {
            RawWindowHandle::Xcb(h) => XlibParent(RawWindowHandle::Xlib(XlibWindowHandle::new(h.window.get() as _))),
            other => XlibParent(other),
        }
    }
}
impl raw_window_handle::HasWindowHandle for XlibParent {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(self.0) })
    }
}
