//! Which display backend the shell runs on (`MORPH_BACKEND=x11|wayland`).
//!
//! On a Wayland session the shell runs natively and pages are rendered off
//! screen and drawn by GPUI (`morph_webview::offscreen`). On X11, and under
//! XWayland when asked for or when GTK cannot use Wayland, web views are
//! X11 child windows of the shell's window. GPUI picks Wayland whenever
//! `WAYLAND_DISPLAY` is set, and GTK follows `GDK_BACKEND`, so the choice
//! is made in the environment before either starts.
//!
//! With NVIDIA's proprietary driver the automatic choice stays on X11 when
//! XWayland is there: WebKitGTK's accelerated rendering under GDK's Wayland
//! backend has a history of failing on that driver (ARCHITECTURE.md 2.1,
//! NVIDIA caveat). `MORPH_BACKEND=wayland` still runs natively there.

use std::ffi::OsStr;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    X11,
    Wayland,
}

/// The backend for `MORPH_BACKEND`, the session's displays and whether
/// NVIDIA's driver is loaded, and whether X11 is there to fall back on.
/// `None` for `MORPH_BACKEND` is automatic.
pub fn choose(wanted: Option<&OsStr>, wayland: Option<&OsStr>, x11: Option<&OsStr>, nvidia: bool) -> Result<(Backend, bool), String> {
    let set = |v: Option<&OsStr>| v.is_some_and(|v| !v.is_empty());
    let (wayland, x11) = (set(wayland), set(x11));
    match wanted.map(|w| w.to_string_lossy().trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("auto") => match (wayland, x11) {
            (true, true) if nvidia => Ok((Backend::X11, false)),
            (true, x11) => Ok((Backend::Wayland, x11)),
            (false, true) => Ok((Backend::X11, false)),
            (false, false) => Err("no display: neither WAYLAND_DISPLAY nor DISPLAY is set".into()),
        },
        Some("wayland") if wayland => Ok((Backend::Wayland, false)),
        Some("wayland") => Err("MORPH_BACKEND=wayland but WAYLAND_DISPLAY is not set".into()),
        Some("x11") if x11 => Ok((Backend::X11, false)),
        Some("x11") => Err("MORPH_BACKEND=x11 but DISPLAY is not set (no X server or XWayland)".into()),
        Some(other) => Err(format!("MORPH_BACKEND must be x11 or wayland, not {other:?}")),
    }
}

/// Whether NVIDIA's driver (proprietary or open kernel modules) is loaded,
/// looked up under `root` (`/` normally; a fake tree in tests).
pub fn nvidia_driver(root: &Path) -> bool {
    root.join("proc/driver/nvidia/version").exists() || root.join("sys/module/nvidia_drm").exists()
}

/// Whether to set `WEBKIT_DISABLE_DMABUF_RENDERER=1`: on NVIDIA's driver
/// WebKitGTK's DMA-BUF renderer fails ("Failed to create GBM buffer") and
/// pages stay black, so it is turned off there unless the user already set
/// the variable or asked to keep the renderer (`MORPH_DMABUF=1`).
pub fn disable_dmabuf(nvidia: bool, already_set: bool, keep: Option<&OsStr>) -> bool {
    nvidia && !already_set && keep.is_none_or(|v| v != "1")
}

/// Pick the backend and start GTK on it. Call first thing in `main`,
/// before any other thread exists (it changes the environment).
pub fn start() -> Result<Backend, String> {
    let nvidia = nvidia_driver(Path::new("/"));
    if disable_dmabuf(nvidia, std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some(), std::env::var_os("MORPH_DMABUF").as_deref()) {
        eprintln!(
            "morph: NVIDIA driver found; setting WEBKIT_DISABLE_DMABUF_RENDERER=1 (black pages otherwise; MORPH_DMABUF=1 keeps the DMA-BUF renderer)"
        );
        // SAFETY: no other thread exists yet (see above). The web processes
        // WebKit starts inherit it.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }
    let (backend, fallback) = choose(
        std::env::var_os("MORPH_BACKEND").as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").as_deref(),
        std::env::var_os("DISPLAY").as_deref(),
        nvidia,
    )?;
    if backend == Backend::Wayland {
        // SAFETY: no other thread exists yet (see above).
        unsafe {
            std::env::set_var("GDK_BACKEND", "wayland");
            // Overlay scrollbars in an off-screen page keep animating,
            // redrawing the whole page about 12 times a second while idle.
            if std::env::var_os("GTK_OVERLAY_SCROLLING").is_none() {
                std::env::set_var("GTK_OVERLAY_SCROLLING", "0");
            }
        }
        match morph_webview::wry_linux::init() {
            Ok(()) => return Ok(Backend::Wayland),
            Err(e) if fallback => eprintln!("morph: {e}; using X11 (XWayland) instead"),
            Err(e) => return Err(e.to_string()),
        }
    }
    // SAFETY: as above.
    unsafe {
        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::set_var("GDK_BACKEND", "x11");
    }
    morph_webview::wry_linux::init().map_err(|e| e.to_string())?;
    Ok(Backend::X11)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(wanted: Option<&str>, wayland: Option<&str>, x11: Option<&str>) -> Result<(Backend, bool), String> {
        choose(wanted.map(OsStr::new), wayland.map(OsStr::new), x11.map(OsStr::new), false)
    }

    #[test]
    fn a_wayland_session_runs_natively_with_x11_to_fall_back_on() {
        assert_eq!(pick(None, Some("wayland-0"), Some(":0")), Ok((Backend::Wayland, true)));
        assert_eq!(pick(Some("auto"), Some("wayland-0"), None), Ok((Backend::Wayland, false)));
        assert_eq!(pick(None, Some(""), Some(":0")), Ok((Backend::X11, false)));
        assert!(pick(None, None, None).is_err());
    }

    #[test]
    fn nvidia_is_found_by_its_proc_file_or_drm_module() {
        let root = tempfile::tempdir().unwrap();
        assert!(!nvidia_driver(root.path()));
        std::fs::create_dir_all(root.path().join("proc/driver/nvidia")).unwrap();
        assert!(!nvidia_driver(root.path()), "the directory alone is not the driver");
        std::fs::write(root.path().join("proc/driver/nvidia/version"), "NVRM version: 615.78\n").unwrap();
        assert!(nvidia_driver(root.path()));
        let open = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(open.path().join("sys/module/nvidia_drm")).unwrap();
        assert!(nvidia_driver(open.path()));
    }

    #[test]
    fn the_dmabuf_renderer_goes_off_on_nvidia_unless_kept() {
        assert!(disable_dmabuf(true, false, None));
        assert!(disable_dmabuf(true, false, Some(OsStr::new("0"))));
        assert!(!disable_dmabuf(true, false, Some(OsStr::new("1"))));
        assert!(!disable_dmabuf(true, true, None), "the user's own setting wins");
        assert!(!disable_dmabuf(false, false, None));
    }

    #[test]
    fn nvidia_stays_on_xwayland_unless_asked() {
        let nvidia =
            |wanted: Option<&str>, x11: Option<&str>| choose(wanted.map(OsStr::new), Some(OsStr::new("wayland-1")), x11.map(OsStr::new), true);
        assert_eq!(nvidia(None, Some(":0")), Ok((Backend::X11, false)));
        assert_eq!(nvidia(None, None), Ok((Backend::Wayland, false)));
        assert_eq!(nvidia(Some("wayland"), Some(":0")), Ok((Backend::Wayland, false)));
    }

    #[test]
    fn morph_backend_overrides_and_never_falls_back() {
        assert_eq!(pick(Some("x11"), Some("wayland-0"), Some(":0")), Ok((Backend::X11, false)));
        assert_eq!(pick(Some("Wayland"), Some("wayland-0"), Some(":0")), Ok((Backend::Wayland, false)));
        assert!(pick(Some("x11"), Some("wayland-0"), None).unwrap_err().contains("DISPLAY"));
        assert!(pick(Some("wayland"), None, Some(":0")).unwrap_err().contains("WAYLAND_DISPLAY"));
        assert!(pick(Some("gtk"), Some("wayland-0"), Some(":0")).unwrap_err().contains("x11 or wayland"));
    }
}
