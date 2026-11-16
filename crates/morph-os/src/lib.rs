//! Calls into the operating system that need no window, kept apart from
//! the GPUI shell so they build (and cross-check) on their own:
//!
//! * [`run`]: a command with a time limit (every desktop call Morph makes
//!   has one, so a hung `xdg-mime` or settings app never hangs Morph);
//! * [`windows`]: Morph as a browser in the Windows registry (Default apps
//!   lists it), the `weirdnet-morph://` protocol, and Windows Settings;
//! * [`macos`]: Morph as the default browser through Launch Services.

use std::time::{Duration, Instant};

pub mod macos;
pub mod windows;

/// How long a desktop call may take before Morph gives up on it.
pub const DESKTOP_CALL_LIMIT: Duration = Duration::from_secs(20);

/// Run `program` with `args`, killing it after `limit`. `Ok(stdout)` when it
/// exited successfully, else `Err(stderr or a description)`.
pub fn run(program: &str, args: &[&str], limit: Duration) -> Result<String, String> {
    use std::io::Read;
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    // Read the pipes on threads so a chatty child cannot fill them and block.
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_string(&mut s);
        }
        s
    });
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = err.as_mut() {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let end = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= end => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} did not answer within {} s", limit.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("{program}: {e}")),
        }
    };
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    if status.success() {
        Ok(stdout)
    } else {
        let stderr = stderr.trim();
        Err(if stderr.is_empty() { format!("{program} failed ({status})") } else { format!("{stderr} ({status})") })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn output_and_failures_come_back() {
        assert_eq!(run("sh", &["-c", "echo hi"], Duration::from_secs(5)).unwrap(), "hi\n");
        let err = run("sh", &["-c", "echo nope >&2; exit 3"], Duration::from_secs(5)).unwrap_err();
        assert!(err.starts_with("nope ("), "{err}");
        assert!(run("/nonexistent/morph-test", &[], Duration::from_secs(5)).is_err());
    }

    #[test]
    fn a_hung_command_is_killed_at_the_limit() {
        let started = Instant::now();
        let err = run("sleep", &["30"], Duration::from_millis(300)).unwrap_err();
        assert!(err.contains("did not answer"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
