//! `morph`: the desktop shell on GPUI (ARCHITECTURE.md sections 2.0 and 2a).
//!
//! The window is a host for components. Phase 1 has one, the browser, in the
//! main slot; the layout model (`morph_ui::layout`) already has the sidebar
//! and bottom slots later components use. It runs on Linux (X11 and
//! Wayland), macOS and Windows.

// A GUI app on Windows (no console window of its own); see `attach_console`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod about;
mod address;
#[cfg(target_os = "linux")]
mod backend;
mod brand;
mod bridge_selftest;
mod browser_view;
mod cli;
mod companion_devices;
mod companion_host;
mod companion_markdown;
mod companion_research;
mod companion_view;
mod companion_web;
mod default_browser;
mod dialogs;
mod identity;
mod identity_view;
#[cfg(unix)]
mod launch;
#[cfg(target_os = "linux")]
mod legacy_desktop;
mod library;
mod library_view;
mod lock_view;
mod mcp_host;
mod menu_view;
mod notices;
mod page_input;
mod password_saver;
mod perf;
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
    morph_services::sync::enable_autostart();
    let paths = match morph_identity::Paths::resolve(None) {
        Ok(paths) => paths,
        Err(e) => {
            eprintln!("morph: {e}");
            return ExitCode::FAILURE;
        }
    };
    // A Morph already running for this home takes the URL (a new tab) and
    // this launch ends here, before any start-up work (render check,
    // media probe, desktop files) that could leave children holding the
    // caller's output open.
    let socket = single::socket_for(&paths.home);
    let request = match &args.open {
        Some(url) => single::Request::Open(url.clone()),
        None => single::Request::Show,
    };
    if single::hand_off(&socket, &request) {
        return ExitCode::SUCCESS;
    }
    // No Morph yet: a launch that opens a link (`xdg-open`, `$BROWSER`)
    // starts the browser on its own and exits once it listens, so callers
    // that wait for the browser command (`gh auth refresh`) go on.
    #[cfg(unix)]
    if let Some(url) = args.open.as_deref().filter(|u| launch::in_background(Some(u), std::env::var_os(launch::FOREGROUND_ENV).as_deref())) {
        match std::env::current_exe().and_then(|exe| launch::start(&exe, &paths.home, &socket, url)) {
            Ok(launch::Started::Ready) => return ExitCode::SUCCESS,
            Ok(launch::Started::Slow) => {
                eprintln!("morph: still starting; the page opens when the window does (log: {})", launch::log_path(&paths.home).display());
                return ExitCode::SUCCESS;
            }
            // Handed to a Morph that started at the same moment.
            Ok(launch::Started::Exited(0)) => return ExitCode::SUCCESS,
            Ok(launch::Started::Exited(code)) => {
                let log = std::fs::read_to_string(launch::log_path(&paths.home)).unwrap_or_default();
                let lines: Vec<&str> = log.lines().collect();
                for line in &lines[lines.len().saturating_sub(20)..] {
                    eprintln!("{line}");
                }
                eprintln!("morph: the browser exited with code {code} as it started");
                return ExitCode::from(u8::try_from(code).unwrap_or(1));
            }
            // Could not start a process of its own: be the browser here.
            Err(e) => eprintln!("morph: running in the foreground: {e}"),
        }
    }
    #[cfg(target_os = "linux")]
    if let Err(e) = backend::start() {
        eprintln!("morph: {e}");
        return ExitCode::FAILURE;
    }
    // The old Flutter build's broken desktop entry and browser associations.
    #[cfg(target_os = "linux")]
    legacy_desktop::spawn();
    // GStreamer and VA-API for the About page, found in the background.
    #[cfg(target_os = "linux")]
    about::probe_media(backend::startup().is_some_and(|s| s.va_ranked));
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
