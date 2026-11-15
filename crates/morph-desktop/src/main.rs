//! `morph`: the desktop shell on GPUI (ARCHITECTURE.md sections 2.0 and 2a).
//!
//! The window is a host for components. Phase 1 has one, the browser, in the
//! main slot; the layout model (`morph_ui::layout`) already has the sidebar
//! and bottom slots later components use.

#[cfg(target_os = "linux")]
mod address;
#[cfg(target_os = "linux")]
mod browser_view;
mod cli;
#[cfg(target_os = "linux")]
mod library;
#[cfg(target_os = "linux")]
mod shell;
#[cfg(target_os = "linux")]
mod text_input;

use std::process::ExitCode;

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    match cli::parse(std::env::args().skip(1)) {
        Ok(cli::Command::Print(text)) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("morph: the GPUI desktop shell runs on Linux only so far; macOS and Windows are planned");
            ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    let args = match cli::parse(std::env::args().skip(1)) {
        Ok(cli::Command::Run(args)) => args,
        Ok(cli::Command::Print(text)) => {
            println!("{text}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("morph: {message}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    prefer_x11();
    let paths = match morph_identity::Paths::resolve(None) {
        Ok(paths) => paths,
        Err(e) => {
            eprintln!("morph: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = morph_webview::wry_linux::init() {
        eprintln!("morph: {e}");
        return ExitCode::FAILURE;
    }
    shell::run(paths.home, args);
    ExitCode::SUCCESS
}

/// Web views are X11 child windows (wry has none on Wayland), so on a
/// Wayland session with XWayland (`DISPLAY` set) the shell runs on X11:
/// GPUI picks Wayland whenever `WAYLAND_DISPLAY` is set, and GTK follows
/// `GDK_BACKEND`. Native Wayland is an open decision (ARCHITECTURE.md).
#[cfg(target_os = "linux")]
fn prefer_x11() {
    if std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()) {
        // SAFETY: called first thing in `main`, before any other thread exists.
        unsafe {
            std::env::remove_var("WAYLAND_DISPLAY");
            std::env::set_var("GDK_BACKEND", "x11");
        }
    }
}
