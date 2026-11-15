//! `morph`: the desktop shell on GPUI (ARCHITECTURE.md sections 2.0 and 2a).
//!
//! The window is a host for components. Phase 1 has one, the browser, in the
//! main slot; the layout model (`morph_ui::layout`) already has the sidebar
//! and bottom slots later components use.

#[cfg(target_os = "linux")]
mod address;
#[cfg(target_os = "linux")]
mod backend;
#[cfg(target_os = "linux")]
mod browser_view;
mod cli;
#[cfg(target_os = "linux")]
mod default_browser;
#[cfg(target_os = "linux")]
mod dmabuf;
#[cfg(target_os = "linux")]
mod identity;
#[cfg(target_os = "linux")]
mod identity_view;
#[cfg(target_os = "linux")]
mod library;
#[cfg(target_os = "linux")]
mod library_view;
#[cfg(target_os = "linux")]
mod page_input;
#[cfg(target_os = "linux")]
mod profiles;
#[cfg(target_os = "linux")]
mod profiles_view;
#[cfg(target_os = "linux")]
mod settings_view;
#[cfg(target_os = "linux")]
mod shell;
#[cfg(target_os = "linux")]
mod single;
#[cfg(target_os = "linux")]
mod text_input;
#[cfg(target_os = "linux")]
mod widgets;

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
    // The DMA-BUF probe runs in a child of Morph (see `dmabuf`).
    if std::env::args().nth(1).as_deref() == Some("--probe-dmabuf") {
        return ExitCode::from(dmabuf::probe().code() as u8);
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
