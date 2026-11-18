//! Morph's own transfer for a download that was paused: the engine's
//! download stops on pause, and resume fetches the rest over HTTP with a
//! `Range` request from where the kept bytes end. A server that ignores the
//! range (200 instead of 206) sends the whole file again: the transfer
//! starts over and says so.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// [`Job::stop`] values.
pub(crate) const RUN: u8 = 0;
/// Stop and keep the bytes (pause).
pub(crate) const PAUSE: u8 = 1;
/// Stop and delete the bytes (cancel).
pub(crate) const CANCEL: u8 = 2;

/// How often a transfer reports its progress at most.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// One transfer: download `id`'s `url` into `path`, from byte `offset`.
pub(crate) struct Job {
    pub id: u64,
    /// Which resume of the download this is; older ones' news is dropped.
    pub generation: u64,
    pub url: String,
    pub path: PathBuf,
    pub offset: u64,
    /// The session's headers ([`morph_core::download_sessions::DownloadContext::resume_headers`]);
    /// `If-Range` goes only with a `Range`.
    pub headers: Vec<(String, String)>,
    pub stop: Arc<AtomicU8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Change {
    /// The server can't resume: the bytes start over from zero.
    Restarted,
    /// The response's validators, for the next resume's `If-Range`.
    Validators {
        etag: Option<String>,
        last_modified: Option<String>,
    },
    Progress {
        received: u64,
        total: Option<u64>,
    },
    /// 401 or 403: the session didn't do; `why` says so if nothing else works.
    Refused {
        status: u16,
        why: String,
    },
    Finished,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Msg {
    pub id: u64,
    pub generation: u64,
    pub change: Change,
}

/// Run `job` on a thread of its own, reporting to `tx`.
pub(crate) fn spawn(job: Job, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let send = |change| {
            let _ = tx.send(Msg { id: job.id, generation: job.generation, change });
        };
        match transfer(&job, &send) {
            Ok(true) => send(Change::Finished),
            Ok(false) => {
                if job.stop.load(Ordering::SeqCst) == CANCEL {
                    let _ = std::fs::remove_file(&job.path);
                }
            }
            Err(why) => send(Change::Failed(why)),
        }
    });
}

/// The first byte and the total size in a `Content-Range` (`bytes 10-99/100`).
fn content_range(value: &str) -> Option<(u64, Option<u64>)> {
    let rest = value.trim().strip_prefix("bytes")?.trim();
    let (range, total) = rest.split_once('/')?;
    let start = range.split_once('-')?.0.trim().parse().ok()?;
    Some((start, total.trim().parse().ok()))
}

/// Ok(true): all bytes are in; Ok(false): stopped.
fn transfer(job: &Job, send: &dyn Fn(Change)) -> Result<bool, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_connect(Some(Duration::from_secs(30))).http_status_as_error(false).build().into();
    let mut request = agent.get(&job.url);
    for (name, value) in &job.headers {
        if job.offset == 0 && name.eq_ignore_ascii_case("if-range") {
            continue;
        }
        request = request.header(name.as_str(), value.as_str());
    }
    if job.offset > 0 {
        request = request.header("Range", &format!("bytes={}-", job.offset));
    }
    let response = request.call().map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let header = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    if matches!(status, 200 | 206) {
        send(Change::Validators { etag: header("etag"), last_modified: header("last-modified") });
    }
    let length: Option<u64> = header("content-length").and_then(|v| v.trim().parse().ok());
    let mut file = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(&job.path).map_err(|e| e.to_string())?;
    let (mut received, total) = match status {
        206 => {
            let (start, total) = header("content-range").and_then(|v| content_range(&v)).unwrap_or((job.offset, None));
            if start > job.offset {
                return Err(format!("the server resumed at byte {start}, past the {} kept", job.offset));
            }
            file.set_len(start).map_err(|e| e.to_string())?;
            (start, total.or(length.map(|l| l + start)))
        }
        200 => {
            if job.offset > 0 {
                send(Change::Restarted);
            }
            file.set_len(0).map_err(|e| e.to_string())?;
            (0, length)
        }
        // The range starts at the end: every byte is already here.
        416 if job.offset > 0 => return Ok(true),
        401 | 403 => {
            let session = if job.headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("cookie") || n.eq_ignore_ascii_case("authorization")) {
                "the sign-in it was saved with no longer works"
            } else {
                "no sign-in was saved with it"
            };
            // The shell may find a sign-in and try again before this counts as a failure.
            send(Change::Refused { status, why: format!("the server refused to resume (HTTP {status}): {session}; sign in again and retry") });
            return Ok(false);
        }
        _ => return Err(format!("the server answered HTTP {status}")),
    };
    file.seek(SeekFrom::Start(received)).map_err(|e| e.to_string())?;
    send(Change::Progress { received, total });
    let mut body = response.into_body().into_reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut last = Instant::now();
    loop {
        if job.stop.load(Ordering::SeqCst) != RUN {
            let _ = file.flush();
            return Ok(false);
        }
        let n = body.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        received += n as u64;
        if last.elapsed() >= PROGRESS_EVERY {
            last = Instant::now();
            send(Change::Progress { received, total });
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    if let Some(total) = total
        && received < total
    {
        return Err(format!("the connection closed after {received} of {total} bytes"));
    }
    send(Change::Progress { received, total: total.or(Some(received)) });
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_ranges_parse() {
        assert_eq!(content_range("bytes 100-999/1000"), Some((100, Some(1000))));
        assert_eq!(content_range("bytes 5-9/*"), Some((5, None)));
        assert_eq!(content_range("items 1-2/3"), None);
    }
}
