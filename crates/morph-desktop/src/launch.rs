//! A launch that opens a link (`morph URL`, which is what `xdg-open`,
//! `$BROWSER`, `x-www-browser` and the desktop file's `%u` run) returns as
//! soon as the link is on its way, the way it already did when a Morph was
//! running (see [`crate::single`]).
//!
//! Programs that open a page and then wait for the answer, such as
//! `gh auth login` / `gh auth refresh` (device code) or a CLI with a
//! loopback OAuth callback, run the browser command and wait for it to
//! exit before they go on. When no Morph was running, the launch became
//! the browser itself and only exited when its window closed, so the
//! terminal never heard back. Now that launch starts the browser as a
//! process of its own (a new session, with no hold on the caller's
//! terminal or pipes), waits until it listens for links, and exits 0.
//!
//! `MORPH_FOREGROUND=1` keeps Morph in the launching process (test
//! harnesses that read its output use it); a launch with no URL always
//! stays in the foreground.

use std::path::Path;
use std::time::Duration;

/// Keep the browser in the launching process.
pub const FOREGROUND_ENV: &str = "MORPH_FOREGROUND";

/// How long a launch waits for the new Morph to listen for links before it
/// exits anyway (the new Morph still opens the URL).
const READY_WITHIN: Duration = Duration::from_secs(20);

/// Whether a launch that opens `open` and found no running Morph starts
/// the browser in the background. `foreground` is [`FOREGROUND_ENV`].
pub fn in_background(open: Option<&str>, foreground: Option<&std::ffi::OsStr>) -> bool {
    cfg!(unix) && open.is_some() && !foreground.is_some_and(|v| !v.is_empty() && v != "0")
}

/// The Morph log a background start writes to, in the Morph home.
pub fn log_path(home: &Path) -> std::path::PathBuf {
    home.join("morph.log")
}

/// What became of a background start.
#[derive(Debug, PartialEq, Eq)]
pub enum Started {
    /// The new Morph listens for links (it opens the URL itself).
    Ready,
    /// It exited before it listened, with this code.
    Exited(i32),
    /// It still runs but did not listen within the wait.
    Slow,
}

/// Start `exe url` as a Morph of its own and wait until `socket` takes
/// connections. Its output goes to [`log_path`].
#[cfg(unix)]
pub fn start(exe: &Path, home: &Path, socket: &Path, url: &str) -> std::io::Result<Started> {
    start_with(exe, home, socket, &[url], READY_WITHIN)
}

#[cfg(unix)]
fn start_with(exe: &Path, home: &Path, socket: &Path, args: &[&str], within: Duration) -> std::io::Result<Started> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    std::fs::create_dir_all(home)?;
    let log = std::fs::File::create(log_path(home))?;
    let mut command = Command::new(exe);
    command.args(args).env(FOREGROUND_ENV, "1").stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log);
    // SAFETY: setsid is async-signal-safe and touches no memory; a new
    // session means no SIGHUP or Ctrl+C from the caller's terminal.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    let deadline = std::time::Instant::now() + within;
    loop {
        if crate::single::is_running(socket) {
            return Ok(Started::Ready);
        }
        if let Some(status) = child.try_wait()? {
            return Ok(Started::Exited(status.code().unwrap_or(1)));
        }
        if std::time::Instant::now() >= deadline {
            return Ok(Started::Slow);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn only_link_launches_go_to_the_background() {
        assert!(in_background(Some("http://127.0.0.1:8000/callback?code=x"), None));
        assert!(in_background(Some("https://github.com/login/device"), Some(OsStr::new("0"))));
        assert!(in_background(Some("example.com"), Some(OsStr::new(""))));
        assert!(!in_background(Some("example.com"), Some(OsStr::new("1"))));
        assert!(!in_background(None, None));
    }

    /// A stand-in for Morph: a shell script that listens on the socket
    /// (python3's Unix socket server) and runs on after it is ready.
    fn fake_morph(dir: &Path, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let exe = dir.join("fake-morph");
        std::fs::write(&exe, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        exe
    }

    #[test]
    fn the_launch_returns_once_the_new_morph_listens_and_leaves_it_running() {
        let tmp = tempfile::tempdir().unwrap();
        let socket = tmp.path().join("m.sock");
        let pid = tmp.path().join("pid");
        // Listens after a moment, records that it saw the URL and its own
        // environment, and keeps running.
        let exe = fake_morph(
            tmp.path(),
            &format!(
                "echo \"$1 $MORPH_FOREGROUND\" > {seen}; echo $$ > {pid}; sleep 0.3\nexec python3 -c 'import socket,sys,time; s=socket.socket(socket.AF_UNIX); s.bind(sys.argv[1]); s.listen(); time.sleep(30)' {socket}",
                seen = tmp.path().join("seen").display(),
                pid = pid.display(),
                socket = socket.display()
            ),
        );
        let home = tmp.path().join("home");
        let url = "http://127.0.0.1:8123/callback?code=a&state=b";
        let start = std::time::Instant::now();
        assert_eq!(start_with(&exe, &home, &socket, &[url], Duration::from_secs(10)).unwrap(), Started::Ready);
        assert!(start.elapsed() < Duration::from_secs(5));
        assert_eq!(std::fs::read_to_string(tmp.path().join("seen")).unwrap().trim(), format!("{url} 1"));
        assert!(log_path(&home).exists());
        let pid: i32 = std::fs::read_to_string(&pid).unwrap().trim().parse().unwrap();
        // SAFETY: plain syscalls on a pid we started.
        unsafe {
            assert_eq!(libc::getsid(pid), pid, "the new Morph leads a session of its own");
            assert_eq!(libc::kill(pid, 0), 0, "and still runs");
            libc::kill(pid, libc::SIGTERM);
        }
    }

    #[test]
    fn a_morph_that_fails_to_start_reports_its_exit_code() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = fake_morph(tmp.path(), "echo 'morph: no display' >&2; exit 3");
        let home = tmp.path().join("home");
        let got = start_with(&exe, &home, &tmp.path().join("m.sock"), &["example.com"], Duration::from_secs(10)).unwrap();
        assert_eq!(got, Started::Exited(3));
        assert!(std::fs::read_to_string(log_path(&home)).unwrap().contains("no display"));
    }

    #[test]
    fn a_slow_morph_is_left_to_open_the_url() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = fake_morph(tmp.path(), "exec sleep 2");
        let got = start_with(&exe, &tmp.path().join("home"), &tmp.path().join("m.sock"), &["example.com"], Duration::from_millis(300)).unwrap();
        assert_eq!(got, Started::Slow);
    }
}
