//! `morph`: the desktop shell on GPUI (ARCHITECTURE.md sections 2.0 and 2a).
//!
//! The window is a host for components. Phase 1 has one, the browser, in the
//! main slot; the layout model (`morph_ui::layout`) already has the sidebar
//! and bottom slots later components use. It runs on Linux (X11 and
//! Wayland), macOS and Windows.

// A GUI app on Windows (no console window of its own); see `attach_console`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod address;
#[cfg(target_os = "linux")]
mod backend;
mod browser_view;
mod cli;
mod default_browser;
mod identity;
mod identity_view;
mod library;
mod library_view;
mod page_input;
mod profiles;
mod profiles_view;
#[cfg(target_os = "linux")]
mod render_check;
mod settings_view;
mod shell;
mod single;
mod text_input;
mod widgets;

use std::process::ExitCode;

/// Print to the console Morph was started from, if any (Windows GUI apps
/// have none of their own). Redirected output is left alone.
#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: no arguments to get wrong; failure (no parent console) is fine.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

fn main() -> ExitCode {
    #[cfg(windows)]
    attach_console();
    // The start-up render check runs in a child of Morph (see `render_check`).
    #[cfg(target_os = "linux")]
    if std::env::args().nth(1).as_deref() == Some(render_check::CHILD_FLAG) {
        let arg = |n: usize, yes: &str| std::env::args().nth(n).as_deref() == Some(yes);
        return ExitCode::from(morph_webview::render_probe::run(arg(2, "offscreen"), arg(3, "gl")));
    }
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
    #[cfg(target_os = "linux")]
    if let Err(e) = backend::start() {
        eprintln!("morph: {e}");
        return ExitCode::FAILURE;
    }
    let paths = match morph_identity::Paths::resolve(None) {
        Ok(paths) => paths,
        Err(e) => {
            eprintln!("morph: {e}");
            return ExitCode::FAILURE;
        }
    };
    // A Morph already running for this home takes the URL (a new tab) and
    // this launch ends here.
    let socket = single::socket_for(&paths.home);
    let request = match &args.open {
        Some(url) => single::Request::Open(url.clone()),
        None => single::Request::Show,
    };
    if single::hand_off(&socket, &request) {
        return ExitCode::SUCCESS;
    }
    let listener = match single::Listener::bind(&socket) {
        Ok(listener) => Some(listener),
        Err(e) => {
            eprintln!("morph: links from other apps will open new windows: {e}");
            None
        }
    };
    shell::run(paths.home, args, listener);
    ExitCode::SUCCESS
}
