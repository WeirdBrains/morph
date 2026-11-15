//! The child side of Morph's start-up render check (`morph --probe-render`).
//!
//! Morph never decides how WebKit renders from the GPU's vendor. Instead a
//! child process does what the shell is about to do, with the environment
//! the parent is considering, and reports what happened:
//!
//! - **Off-screen windows and GL.** On GTK 3 an off-screen window cannot
//!   make a GL context ("The current backend does not support OpenGL"), and
//!   WebKit aborts the whole process when it gets a GPU (DMA-BUF) buffer for
//!   a view it cannot make one for. [`offscreen_has_gl`] asks GTK.
//! - **Does a page actually paint?** A small page with a known background
//!   is loaded into a web view (in an off-screen window for the Wayland
//!   backend, in a window placed off the screen for X11, exactly where the
//!   shell's pages live) and the view is drawn into memory until that colour
//!   shows up. A GPU that refuses WebKit's buffers ("Failed to create GBM
//!   buffer") leaves the page black, so the check fails; the parent also
//!   reads WebKit's own messages from the child's standard error.

use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};
use webkit2gtk::WebViewExt;

/// The page painted.
pub const RENDERS: u8 = 0;
/// The page never showed its colour in time.
pub const BLACK: u8 = 10;
/// GTK could not start on the display.
pub const NO_DISPLAY: u8 = 13;
/// An off-screen window cannot make a GL context, and WebKit might hand
/// the view GPU buffers (neither shared memory nor the classic renderer is
/// forced).
pub const NO_GL: u8 = 20;

/// The probe page's background, as the 8-bit RGB the check looks for.
pub const COLOUR: (u8, u8, u8) = (0x12, 0x8a, 0x5e);
const SIZE: i32 = 64;

/// Whether GTK can make a GL context for an off-screen window.
pub fn offscreen_has_gl() -> bool {
    let window = gtk::OffscreenWindow::new();
    window.realize();
    let ok = window.window().is_some_and(|w| w.create_gl_context().and_then(|c| c.realize()).is_ok());
    // SAFETY: the window is ours and was never shown or shared.
    unsafe { window.destroy() };
    ok
}

/// Whether the environment keeps GPU buffers away from the view.
fn gpu_buffers_off() -> bool {
    let set = |v: &str| std::env::var_os(v).is_some_and(|v| !v.is_empty() && v != "0");
    set("WEBKIT_DMABUF_RENDERER_FORCE_SHM") || set("WEBKIT_DISABLE_DMABUF_RENDERER")
}

/// Whether `pixel` (cairo ARGB32, native-endian) is the probe colour.
pub fn is_probe_colour(pixel: u32) -> bool {
    let (r, g, b) = ((pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8);
    let near = |a: u8, b: u8| a.abs_diff(b) <= 8;
    near(r, COLOUR.0) && near(g, COLOUR.1) && near(b, COLOUR.2)
}

/// Draw `widget` into memory and read its centre pixel.
fn centre_pixel(widget: &impl IsA<gtk::Widget>) -> Option<u32> {
    let mut surface = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, SIZE, SIZE).ok()?;
    {
        let cr = gtk::cairo::Context::new(&surface).ok()?;
        widget.draw(&cr);
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().ok()?;
    let at = (SIZE as usize / 2) * stride + (SIZE as usize / 2) * 4;
    Some(u32::from_ne_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// Run the check: `offscreen` for the Wayland backend's off-screen pages,
/// `gl_only` to stop after the GL question. Returns the exit code.
pub fn run(offscreen: bool, gl_only: bool) -> u8 {
    if gtk::init().is_err() {
        return NO_DISPLAY;
    }
    if offscreen && !gpu_buffers_off() && !offscreen_has_gl() {
        return NO_GL;
    }
    if gl_only {
        return RENDERS;
    }
    let view = webkit2gtk::WebView::new();
    let window: gtk::Window = if offscreen {
        gtk::OffscreenWindow::new().upcast()
    } else {
        // A real window (GL works there), placed off every screen so it is
        // never seen; a popup has no decorations or taskbar entry.
        let window = gtk::Window::new(gtk::WindowType::Popup);
        window.move_(-10_000, -10_000);
        window
    };
    window.set_default_size(SIZE, SIZE);
    window.add(&view);
    let (r, g, b) = COLOUR;
    view.load_html(&format!("<html><body style=\"margin:0;background:#{r:02x}{g:02x}{b:02x}\"></body></html>"), None);
    window.show_all();
    let result = Rc::new(Cell::new(None));
    let end = Instant::now() + Duration::from_secs(5);
    let (found, v) = (result.clone(), view.clone());
    gtk::glib::timeout_add_local(Duration::from_millis(50), move || {
        if centre_pixel(&v).is_some_and(is_probe_colour) {
            found.set(Some(RENDERS));
        } else if Instant::now() >= end {
            found.set(Some(BLACK));
        }
        if found.get().is_some() { gtk::glib::ControlFlow::Break } else { gtk::glib::ControlFlow::Continue }
    });
    let context = gtk::glib::MainContext::default();
    while result.get().is_none() {
        context.iteration(true);
    }
    result.get().unwrap_or(BLACK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_colour_is_recognised_and_black_is_not() {
        let (r, g, b) = COLOUR;
        let argb = |r: u8, g: u8, b: u8| 0xff00_0000 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b);
        assert!(is_probe_colour(argb(r, g, b)));
        assert!(is_probe_colour(argb(r + 3, g - 4, b + 2)), "colour management may shift it a little");
        assert!(!is_probe_colour(argb(0, 0, 0)), "a black page");
        assert!(!is_probe_colour(argb(0xff, 0xff, 0xff)), "a blank white page");
        assert!(!is_probe_colour(0), "nothing drawn");
    }
}
