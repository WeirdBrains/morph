//! Which WebKit renderer settings, and on a Wayland session which backend,
//! actually paint pages here, found at start-up by trying them.
//!
//! Nothing is chosen by GPU vendor (m3talsmith, 2026-11-15). Each try is a
//! child process (`morph --probe-render`, see `morph_webview::render_probe`)
//! with the environment under test, so a driver that misbehaves, or WebKit
//! aborting, never touches the shell:
//!
//! 1. **Wayland: can off-screen pages take GPU buffers?** GTK 3's off-screen
//!    windows cannot make a GL context, and WebKit aborts when it has a GPU
//!    buffer for a view without one ("GDK is not able to create a GL
//!    context: The current backend does not support OpenGL"). When the child
//!    finds no GL there, pages use WebKit's shared-memory buffers
//!    (`WEBKIT_DMABUF_RENDERER_FORCE_SHM=1`): WebKit still renders with the
//!    GPU, and the off-screen backend copies every frame anyway. This check
//!    always runs when the machine has a DRM device, even with
//!    `MORPH_DMABUF_PROBE=0`, because the alternative is an abort.
//! 2. **Does a page paint?** The child loads a page with a known colour
//!    where the shell's pages live and waits for that colour, while the
//!    parent watches WebKit's messages for a failed buffer ("Failed to
//!    create GBM buffer of size 1x1: Invalid argument" left pages black on
//!    an NVIDIA GPU with nvidia-open 615.78). The settings tried, in order:
//!    as they are; GPU buffers exported from GL textures
//!    (`WEBKIT_DMABUF_RENDERER_DISABLE_GBM=1`); shared memory
//!    (`WEBKIT_DMABUF_RENDERER_FORCE_SHM=1`); the classic renderer
//!    (`WEBKIT_DISABLE_DMABUF_RENDERER=1`). The first that paints is kept.
//!    On Wayland, when none paints and X11 is there, the same ladder runs
//!    for XWayland, and the shell uses it if a setting paints there.
//!
//! The page check runs only where WebKit could use GPU buffers (a DRM
//! device exists) or when `MORPH_DMABUF_PROBE=1`; `MORPH_DMABUF_PROBE=0`, or
//! any WebKit renderer variable set by the user, skips it.

use crate::backend::Backend;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The argument that makes `morph` run the child side.
pub const CHILD_FLAG: &str = "--probe-render";

/// The variables WebKit reads that mean the user chose a renderer already.
pub const USER_CHOICES: [&str; 5] = [
    "WEBKIT_DISABLE_DMABUF_RENDERER",
    "WEBKIT_DMABUF_RENDERER_DISABLE_GBM",
    "WEBKIT_DMABUF_RENDERER_FORCE_SHM",
    "WEBKIT_FORCE_DMABUF_RENDERER",
    "WEBKIT_DISABLE_COMPOSITING_MODE",
];

/// Variables to set, as a whole.
pub type Env = &'static [(&'static str, &'static str)];
pub const AS_IS: Env = &[];
pub const GL_EXPORT: Env = &[("WEBKIT_DMABUF_RENDERER_DISABLE_GBM", "1")];
pub const SHARED_MEMORY: Env = &[("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1")];
pub const CLASSIC: Env = &[("WEBKIT_DISABLE_DMABUF_RENDERER", "1")];

/// The settings to try, best first. Off-screen pages without GL start at
/// shared memory (anything before it would abort).
pub fn ladder(no_gl: bool) -> &'static [Env] {
    if no_gl { &[SHARED_MEMORY, CLASSIC] } else { &[AS_IS, GL_EXPORT, SHARED_MEMORY, CLASSIC] }
}

/// What one try found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The test page painted (or, for the GL question, GL is there).
    Renders,
    /// The page stayed black, WebKit reported a failed buffer, or the child
    /// crashed.
    Fails,
    /// An off-screen window cannot make a GL context.
    NoGl,
    /// The child could not run or hung: no conclusion.
    Unknown,
}

/// Which checks to make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checks {
    /// Ask whether off-screen windows can make GL contexts (Wayland).
    pub gl: bool,
    /// Try the settings ladder with a test page.
    pub pages: bool,
}

/// The backend and environment to start with, and a line for the log when
/// anything changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub backend: Backend,
    pub env: Env,
    pub note: Option<String>,
}

fn describe(env: Env) -> String {
    if env.is_empty() { "WebKit's defaults".into() } else { env.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ") }
}

/// The first setting in the ladder that paints; `Err` when a try had no
/// conclusion.
fn first_painting(run: &mut impl FnMut(Backend, Env, bool) -> Outcome, backend: Backend, no_gl: bool) -> Result<Option<Env>, ()> {
    for &env in ladder(no_gl) {
        match run(backend, env, false) {
            Outcome::Renders => return Ok(Some(env)),
            Outcome::Unknown => return Err(()),
            Outcome::Fails | Outcome::NoGl => {}
        }
    }
    Ok(None)
}

/// Decide, given `run(backend, env, gl_only)` for one try. Pure apart from
/// `run`, so the choices are tested with made-up outcomes.
pub fn decide(backend: Backend, x11_fallback: bool, checks: Checks, mut run: impl FnMut(Backend, Env, bool) -> Outcome) -> Decision {
    match backend {
        Backend::X11 => {
            if !checks.pages {
                return Decision { backend, env: AS_IS, note: None };
            }
            match first_painting(&mut run, Backend::X11, false) {
                Ok(Some(env)) if env.is_empty() => Decision { backend, env, note: None },
                Ok(Some(env)) => {
                    Decision { backend, env, note: Some(format!("a test page stayed black with WebKit's defaults; using {}", describe(env))) }
                }
                Ok(None) => {
                    Decision { backend, env: AS_IS, note: Some("no WebKit renderer setting painted a test page; leaving WebKit's defaults".into()) }
                }
                Err(()) => Decision { backend, env: AS_IS, note: None },
            }
        }
        Backend::Wayland => {
            let no_gl = checks.gl && run(Backend::Wayland, AS_IS, true) == Outcome::NoGl;
            let base = if no_gl { SHARED_MEMORY } else { AS_IS };
            let base_note = no_gl.then(|| {
                "off-screen pages cannot use GL here, so WebKit hands them shared-memory buffers (WEBKIT_DMABUF_RENDERER_FORCE_SHM=1)".to_string()
            });
            if !checks.pages {
                return Decision { backend, env: base, note: base_note };
            }
            match first_painting(&mut run, Backend::Wayland, no_gl) {
                Ok(Some(env)) if env == base => Decision { backend, env, note: base_note },
                Ok(Some(env)) => Decision {
                    backend,
                    env,
                    note: Some(format!("a test page stayed black on Wayland with {}; using {}", describe(base), describe(env))),
                },
                Err(()) => Decision { backend, env: base, note: base_note },
                Ok(None) => {
                    if x11_fallback && let Ok(Some(env)) = first_painting(&mut run, Backend::X11, false) {
                        return Decision {
                            backend: Backend::X11,
                            env,
                            note: Some(format!("no WebKit setting painted a test page on Wayland; using X11 (XWayland) with {}", describe(env))),
                        };
                    }
                    Decision { backend, env: base, note: Some(format!("no WebKit renderer setting painted a test page; using {}", describe(base))) }
                }
            }
        }
    }
}

/// WebKit's messages that mean the buffers it renders into failed.
pub fn is_buffer_failure(line: &str) -> bool {
    [
        "Failed to create GBM buffer",
        "Failed to create EGL image for DMABufs",
        "Failed to extract DMA-buf attributes",
        "GDK is not able to create a GL context",
        "GDK failed to realize the GL context",
    ]
    .iter()
    .any(|m| line.contains(m))
}

/// What a child's exit code means.
pub fn outcome(code: Option<i32>) -> Outcome {
    use morph_webview::render_probe::{BLACK, NO_GL, RENDERS};
    match code {
        Some(c) if c == i32::from(RENDERS) => Outcome::Renders,
        Some(c) if c == i32::from(BLACK) => Outcome::Fails,
        Some(c) if c == i32::from(NO_GL) => Outcome::NoGl,
        // Killed by a signal: WebKit aborted, or the driver crashed.
        None => Outcome::Fails,
        Some(_) => Outcome::Unknown,
    }
}

/// One try in a child process.
pub fn run_child(backend: Backend, env: Env, gl_only: bool) -> Outcome {
    let Ok(exe) = std::env::current_exe() else {
        return Outcome::Unknown;
    };
    let mut command = Command::new(exe);
    command
        .arg(CHILD_FLAG)
        .arg(if backend == Backend::Wayland { "offscreen" } else { "window" })
        .arg(if gl_only { "gl" } else { "page" })
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        // Its own process group, so its web processes go with it.
        .process_group(0);
    match backend {
        Backend::Wayland => command.env("GDK_BACKEND", "wayland"),
        Backend::X11 => command.env("GDK_BACKEND", "x11").env_remove("WAYLAND_DISPLAY"),
    };
    // SAFETY: setrlimit is async-signal-safe. No core file when WebKit
    // aborts in the child (they run to gigabytes for a web process).
    unsafe {
        command.pre_exec(|| {
            let none = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            libc::setrlimit(libc::RLIMIT_CORE, &none);
            Ok(())
        });
    }
    let Ok(mut child) = command.spawn() else {
        return Outcome::Unknown;
    };
    let (tx, rx) = mpsc::channel();
    if let Some(stderr) = child.stderr.take() {
        // Only reads the pipe (never the environment); it ends when the
        // last web process lets go of it.
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    let end = Instant::now() + Duration::from_secs(8);
    let result = loop {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(20))
            && is_buffer_failure(&line)
        {
            break Outcome::Fails;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                // WebKit's message can trail the exit by a moment.
                std::thread::sleep(Duration::from_millis(50));
                break if rx.try_iter().any(|l| is_buffer_failure(&l)) { Outcome::Fails } else { outcome(status.code()) };
            }
            Ok(None) if Instant::now() < end => {}
            _ => break Outcome::Unknown,
        }
    };
    // SAFETY: plain kill(2) of the child's own process group.
    unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
    let _ = child.wait();
    result
}

/// Whether WebKit could get GPU buffers here at all (it needs a DRM device).
pub fn has_drm_device() -> bool {
    std::fs::read_dir("/dev/dri").is_ok_and(|mut d| d.next().is_some())
}

/// The checks to make for `MORPH_DMABUF_PROBE`, whether the user chose a
/// renderer, and whether there is a DRM device.
pub fn checks(morph_probe: Option<&str>, user_chose: bool, drm: bool) -> Checks {
    let forced = morph_probe == Some("1");
    Checks { gl: drm || forced, pages: !user_chose && morph_probe != Some("0") && (drm || forced) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const BOTH: Checks = Checks { gl: true, pages: true };

    /// A fake machine: `paints(backend, env)` and whether off-screen GL works.
    type Tries = std::rc::Rc<RefCell<Vec<String>>>;

    fn machine(gl: bool, paints: impl Fn(Backend, Env) -> bool) -> (impl FnMut(Backend, Env, bool) -> Outcome, Tries) {
        let log = std::rc::Rc::new(RefCell::new(Vec::new()));
        let l = log.clone();
        let run = move |backend: Backend, env: Env, gl_only: bool| {
            l.borrow_mut().push(format!("{backend:?} {} {}", describe(env), if gl_only { "gl" } else { "page" }));
            if gl_only {
                return if gl { Outcome::Renders } else { Outcome::NoGl };
            }
            if paints(backend, env) { Outcome::Renders } else { Outcome::Fails }
        };
        (run, log)
    }

    #[test]
    fn offscreen_pages_without_gl_get_shared_memory_and_never_gpu_buffers() {
        // m3talsmith's report: native Wayland aborted with "GDK is not able to
        // create a GL context". Shared memory paints, so it stays native.
        let (run, log) = machine(false, |_, env| env != AS_IS && env != GL_EXPORT);
        let d = decide(Backend::Wayland, true, BOTH, run);
        assert_eq!((d.backend, d.env), (Backend::Wayland, SHARED_MEMORY));
        assert!(d.note.unwrap().contains("FORCE_SHM"));
        assert!(log.borrow().iter().all(|t| !t.contains("defaults page") && !t.contains("DISABLE_GBM")), "{log:?}");
    }

    #[test]
    fn the_gl_check_runs_even_when_the_page_check_is_off() {
        let (run, _) = machine(false, |_, _| unreachable!("no page check"));
        let d = decide(Backend::Wayland, true, Checks { gl: true, pages: false }, run);
        assert_eq!((d.backend, d.env), (Backend::Wayland, SHARED_MEMORY));
        let (run, _) = machine(true, |_, _| unreachable!("no page check"));
        assert_eq!(decide(Backend::Wayland, true, Checks { gl: true, pages: false }, run).env, AS_IS);
    }

    #[test]
    fn a_gpu_that_refuses_webkits_buffers_gets_the_first_setting_that_paints() {
        // m3talsmith's report on X11: "Failed to create GBM buffer of size 1x1".
        let (run, log) = machine(true, |_, env| env != AS_IS);
        let d = decide(Backend::X11, false, BOTH, run);
        assert_eq!((d.backend, d.env), (Backend::X11, GL_EXPORT));
        assert!(d.note.is_some());
        assert_eq!(log.borrow().len(), 2);
        let (run, _) = machine(true, |_, env| env == CLASSIC);
        assert_eq!(decide(Backend::X11, false, BOTH, run).env, CLASSIC);
    }

    #[test]
    fn a_working_gpu_changes_nothing() {
        let (run, log) = machine(true, |_, _| true);
        assert_eq!(decide(Backend::X11, false, BOTH, run), Decision { backend: Backend::X11, env: AS_IS, note: None });
        assert_eq!(log.borrow().len(), 1);
        let (run, _) = machine(true, |_, _| true);
        assert_eq!(decide(Backend::Wayland, true, BOTH, run), Decision { backend: Backend::Wayland, env: AS_IS, note: None });
    }

    #[test]
    fn wayland_falls_back_to_xwayland_only_when_nothing_paints_natively() {
        let (run, _) = machine(false, |backend, env| backend == Backend::X11 && env == GL_EXPORT);
        let d = decide(Backend::Wayland, true, BOTH, run);
        assert_eq!((d.backend, d.env), (Backend::X11, GL_EXPORT));
        assert!(d.note.unwrap().contains("XWayland"));
        // Without X11 to go to it stays native, on shared memory (no abort).
        let (run, _) = machine(false, |_, _| false);
        let d = decide(Backend::Wayland, false, BOTH, run);
        assert_eq!((d.backend, d.env), (Backend::Wayland, SHARED_MEMORY));
    }

    #[test]
    fn no_conclusion_keeps_the_safe_default() {
        let run = |_: Backend, _: Env, gl_only: bool| if gl_only { Outcome::NoGl } else { Outcome::Unknown };
        assert_eq!(decide(Backend::Wayland, true, BOTH, run).env, SHARED_MEMORY);
        let run = |_: Backend, _: Env, _: bool| Outcome::Unknown;
        assert_eq!(decide(Backend::X11, false, BOTH, run), Decision { backend: Backend::X11, env: AS_IS, note: None });
    }

    #[test]
    fn webkits_buffer_failures_are_recognised() {
        assert!(is_buffer_failure("Failed to create GBM buffer of size 1x1: Invalid argument"));
        assert!(is_buffer_failure("Failed to create GBM buffer of size 2x2: no valid format found"));
        assert!(is_buffer_failure(
            "** (morph): ERROR **: 09:58:01.123: GDK is not able to create a GL context: The current backend does not support OpenGL."
        ));
        assert!(!is_buffer_failure("(morph:123): Gdk-CRITICAL **: _gdk_frame_clock_freeze: assertion 'GDK_IS_FRAME_CLOCK (clock)' failed"));
    }

    #[test]
    fn exit_codes_and_signals() {
        assert_eq!(outcome(Some(0)), Outcome::Renders);
        assert_eq!(outcome(Some(10)), Outcome::Fails);
        assert_eq!(outcome(Some(20)), Outcome::NoGl);
        assert_eq!(outcome(Some(13)), Outcome::Unknown, "no display");
        assert_eq!(outcome(Some(101)), Outcome::Unknown, "a panic");
        assert_eq!(outcome(None), Outcome::Fails, "WebKit aborted");
    }

    #[test]
    fn which_checks_run() {
        assert_eq!(checks(None, false, true), BOTH);
        assert_eq!(checks(None, false, false), Checks { gl: false, pages: false }, "no DRM device: WebKit never gets GPU buffers");
        assert_eq!(checks(Some("0"), false, true), Checks { gl: true, pages: false }, "MORPH_DMABUF_PROBE=0 keeps the GL check");
        assert_eq!(checks(None, true, true), Checks { gl: true, pages: false }, "the user chose a renderer");
        assert_eq!(checks(Some("1"), false, false), BOTH);
    }
}
