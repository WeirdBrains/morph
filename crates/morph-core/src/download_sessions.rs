//! The session saved with a download, so a paused download can resume behind
//! a login: the cookies the engine would send, the Referer and User-Agent the
//! first request went with, the address the bytes really came from (after
//! redirects), and the validators (`ETag`, `Last-Modified`) that let a resume
//! ask "only if the file is still the same" (`If-Range`).
//!
//! It is kept per download in the profile's encrypted library database and
//! nowhere else on disk: while the library is still plaintext (never unlocked)
//! or locked, nothing is saved. It goes when the download completes, is
//! cancelled or is cleared. Its `Debug` never shows a cookie value.
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::library_db;
use crate::profiles::ProfileError;

/// One cookie the engine would send with the download's request.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCookie {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub secure: bool,
}

impl fmt::Debug for SessionCookie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}=<{} bytes> ({}{})", self.name, self.value.len(), self.domain, self.path)
    }
}

/// The request context a download started with.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadContext {
    /// Where the bytes came from, after redirects (empty: the download's own URL).
    #[serde(default)]
    pub final_url: String,
    #[serde(default)]
    pub referer: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub cookies: Vec<SessionCookie>,
    /// Other request headers that carry auth (`Authorization`, ...).
    #[serde(default)]
    pub auth_headers: Vec<(String, String)>,
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub last_modified: Option<String>,
}

impl fmt::Debug for DownloadContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DownloadContext")
            .field("final_url", &self.final_url)
            .field("referer", &self.referer.is_some())
            .field("user_agent", &self.user_agent.is_some())
            .field("cookies", &self.cookies)
            .field("auth_headers", &self.auth_headers.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>())
            .field("etag", &self.etag)
            .field("last_modified", &self.last_modified)
            .finish()
    }
}

/// Request headers that must not be copied from the first request onto a resume.
const NOT_AUTH: &[&str] = &["range", "if-range", "cookie", "referer", "user-agent", "accept-encoding", "host", "content-length", "connection"];

/// Whether a request header carries auth and should go with a resume.
pub fn is_auth_header(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    !NOT_AUTH.contains(&n.as_str())
        && (n == "authorization"
            || n == "proxy-authorization"
            || n.starts_with("x-") && (n.contains("auth") || n.contains("token") || n.contains("csrf")))
}

impl DownloadContext {
    /// The `Cookie` header these cookies make, if any.
    pub fn cookie_header(&self) -> Option<String> {
        (!self.cookies.is_empty()).then(|| self.cookies.iter().map(|c| format!("{}={}", c.name, c.value)).collect::<Vec<_>>().join("; "))
    }

    /// The `If-Range` validator: a strong `ETag`, else `Last-Modified`
    /// (a weak `ETag` cannot validate a range).
    pub fn if_range(&self) -> Option<&str> {
        match &self.etag {
            Some(e) if !e.trim_start().starts_with("W/") && !e.trim().is_empty() => Some(e.trim()),
            _ => self.last_modified.as_deref().map(str::trim).filter(|v| !v.is_empty()),
        }
    }

    /// The headers a resume sends (besides `Range`), with `cookies` instead of
    /// the saved ones when the engine has fresh ones.
    pub fn resume_headers(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if let Some(c) = self.cookie_header() {
            out.push(("Cookie".into(), c));
        }
        if let Some(r) = &self.referer {
            out.push(("Referer".into(), r.clone()));
        }
        if let Some(ua) = &self.user_agent {
            out.push(("User-Agent".into(), ua.clone()));
        }
        out.extend(self.auth_headers.iter().filter(|(n, _)| is_auth_header(n)).cloned());
        if let Some(v) = self.if_range() {
            out.push(("If-Range".into(), v.to_string()));
        }
        out
    }
}

/// Where download sessions are kept. Calls never fail loudly: a session that
/// can't be saved only means a resume goes without it.
pub trait SessionStore: Send + Sync {
    fn save(&self, key: &str, context: &DownloadContext);
    fn load(&self, key: &str) -> Option<DownloadContext>;
    fn delete(&self, key: &str);
    /// Forget every download's session (at start: downloads don't outlive the process).
    fn clear(&self);
}

impl fmt::Debug for dyn SessionStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionStore")
    }
}

/// A profile's encrypted library database as a [`SessionStore`].
#[derive(Debug, Clone)]
pub struct LibrarySessions {
    home: PathBuf,
    profile: String,
}

impl LibrarySessions {
    pub fn new(home: impl Into<PathBuf>, profile: impl Into<String>) -> Self {
        Self { home: home.into(), profile: profile.into() }
    }

    /// Save `context` for download `key`: `Ok(false)` if the library isn't
    /// encrypted yet, so nothing was written.
    pub fn try_save(&self, key: &str, context: &DownloadContext, now_ms: i64) -> Result<bool, ProfileError> {
        let json = serde_json::to_string(context)?;
        Ok(library_db::with(&self.home, &self.profile, |lib| lib.save_download_session(key, &json, now_ms))?.is_some())
    }

    pub fn try_load(&self, key: &str) -> Result<Option<DownloadContext>, ProfileError> {
        let json = library_db::with(&self.home, &self.profile, |lib| lib.download_session(key))?.flatten();
        Ok(match json {
            Some(j) => Some(serde_json::from_str(&j)?),
            None => None,
        })
    }

    pub fn try_delete(&self, key: &str) -> Result<(), ProfileError> {
        library_db::with(&self.home, &self.profile, |lib| lib.delete_download_session(key))?;
        Ok(())
    }

    pub fn try_clear(&self) -> Result<(), ProfileError> {
        library_db::with(&self.home, &self.profile, |lib| lib.clear_download_sessions())?;
        Ok(())
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

impl SessionStore for LibrarySessions {
    fn save(&self, key: &str, context: &DownloadContext) {
        if let Err(e) = self.try_save(key, context, now_ms()) {
            eprintln!("morph: download session not saved: {e}");
        }
    }

    fn load(&self, key: &str) -> Option<DownloadContext> {
        self.try_load(key).unwrap_or_else(|e| {
            eprintln!("morph: download session not read: {e}");
            None
        })
    }

    fn delete(&self, key: &str) {
        if let Err(e) = self.try_delete(key) {
            eprintln!("morph: download session not deleted: {e}");
        }
    }

    fn clear(&self) {
        let _ = self.try_clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles;
    use morph_identity::profile_identity::LibraryKey;

    fn context() -> DownloadContext {
        DownloadContext {
            final_url: "https://files.example/get/42".into(),
            referer: Some("https://app.example/files".into()),
            user_agent: Some("Morph-Test".into()),
            cookies: vec![
                SessionCookie {
                    name: "sid".into(),
                    value: "very-secret-cookie".into(),
                    domain: "files.example".into(),
                    path: "/".into(),
                    secure: true,
                },
                SessionCookie { name: "pref".into(), value: "dark".into(), ..Default::default() },
            ],
            auth_headers: vec![("Authorization".into(), "Bearer very-secret-token".into()), ("Accept".into(), "*/*".into())],
            etag: Some("\"v1\"".into()),
            last_modified: Some("Wed, 18 Nov 2026 10:00:00 GMT".into()),
        }
    }

    /// Every file under `dir`, read whole.
    fn all_bytes(dir: &std::path::Path) -> Vec<u8> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(all_bytes(&p));
            } else if let Ok(b) = std::fs::read(&p) {
                out.extend(b);
            }
        }
        out
    }

    fn contains(hay: &[u8], needle: &str) -> bool {
        hay.windows(needle.len()).any(|w| w == needle.as_bytes())
    }

    #[test]
    fn resume_headers_carry_the_session_and_validator() {
        let c = context();
        let h = c.resume_headers();
        let get = |n: &str| h.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str());
        assert_eq!(get("Cookie"), Some("sid=very-secret-cookie; pref=dark"));
        assert_eq!(get("Referer"), Some("https://app.example/files"));
        assert_eq!(get("User-Agent"), Some("Morph-Test"));
        assert_eq!(get("Authorization"), Some("Bearer very-secret-token"));
        assert_eq!(get("Accept"), None, "only auth headers are copied");
        assert_eq!(get("If-Range"), Some("\"v1\""));
        let weak = DownloadContext { etag: Some("W/\"v1\"".into()), ..context() };
        assert_eq!(weak.if_range(), Some("Wed, 18 Nov 2026 10:00:00 GMT"), "a weak ETag falls back to Last-Modified");
        assert_eq!(DownloadContext::default().resume_headers(), vec![]);
    }

    #[test]
    fn debug_never_shows_a_secret() {
        let shown = format!("{:?}", context());
        assert!(!shown.contains("very-secret"), "{shown}");
        assert!(shown.contains("sid"), "{shown}");
    }

    #[test]
    fn sessions_live_encrypted_in_the_library_and_go_when_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        profiles::load_catalog(&home).unwrap();
        let id = profiles::create(&home, "Work").unwrap().id;
        let store = LibrarySessions::new(&home, &id);

        // Never unlocked: the library is plaintext, so nothing is saved.
        assert!(!store.try_save("d1", &context(), 1).unwrap());
        assert_eq!(store.load("d1"), None);
        assert!(!contains(&all_bytes(&home), "very-secret"), "nothing is written while the library is plaintext");

        library_db::unlock(&home, &id, &LibraryKey::from_bytes([7; 32])).unwrap();
        assert!(store.try_save("d1", &context(), 2).unwrap());
        store.save("d2", &context());
        assert_eq!(store.load("d1"), Some(context()));
        let raw = all_bytes(&home);
        for secret in ["very-secret-cookie", "very-secret-token", "files.example/get"] {
            assert!(!contains(&raw, secret), "{secret} is readable on disk");
        }

        store.delete("d1");
        assert_eq!(store.load("d1"), None, "a completed download's session is gone");
        assert!(store.load("d2").is_some());
        store.clear();
        assert_eq!(store.load("d2"), None);

        // Locked: nothing to read or write, and no plaintext fallback.
        library_db::lock(&home, &id);
        assert!(store.try_save("d3", &context(), 3).is_err());
        assert_eq!(store.load("d3"), None);
        assert!(!contains(&all_bytes(&home), "very-secret"));
    }
}
