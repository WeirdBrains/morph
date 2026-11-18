//! Resume behind a JavaScript login: sites that sign in with a token the
//! page keeps in `localStorage`/`sessionStorage` and sends as
//! `Authorization: Bearer <jwt>`. When a resume is refused (401/403), the
//! shell reads that site's storage from a page of the same profile and
//! origin ([`STORAGE_SCRIPT`]), picks a usable JWT ([`pick_jwt`]) and tries
//! again with it. Nothing here logs or keeps a value; callers keep the
//! chosen token only in memory or in the encrypted per-download session.
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;

/// Run in a page: its origin and every `localStorage` and `sessionStorage`
/// entry, as JSON (`{"origin":…,"items":[[key,value],…]}`). Never throws.
pub const STORAGE_SCRIPT: &str = r#"(function(){var o={origin:"",items:[]};try{o.origin=String(location.origin);}catch(e){}
[function(){return window.localStorage;},function(){return window.sessionStorage;}].forEach(function(get){try{var s=get();
for(var i=0;i<s.length;i++){var k=s.key(i);o.items.push([k,String(s.getItem(k))]);}}catch(e){}});return JSON.stringify(o);})()"#;

/// What [`STORAGE_SCRIPT`] read.
#[derive(Clone, Default, PartialEq, Eq, Deserialize)]
pub struct StorageRead {
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub items: Vec<(String, String)>,
}

impl std::fmt::Debug for StorageRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StorageRead({}, {} entries)", self.origin, self.items.len())
    }
}

/// Parse a script result: the JSON itself, or (as some engines hand
/// results back) the JSON encoded once more as a JSON string.
pub fn parse_storage(raw: &str) -> Option<StorageRead> {
    if let Ok(read) = serde_json::from_str::<StorageRead>(raw) {
        return Some(read);
    }
    let inner: String = serde_json::from_str(raw).ok()?;
    serde_json::from_str(&inner).ok()
}

/// `scheme://host[:port]` of `url`, lowercased; `None` for addresses
/// without a host (`data:`, `about:`).
pub fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?;
    if host.is_empty() {
        return None;
    }
    let scheme = scheme.to_ascii_lowercase();
    let host = host.to_ascii_lowercase();
    let host = match (scheme.as_str(), host.rsplit_once(':')) {
        ("http", Some((h, "80"))) | ("https", Some((h, "443"))) => h.to_string(),
        _ => host,
    };
    Some(format!("{scheme}://{host}"))
}

/// The token in an `Authorization: Bearer <token>` value.
pub fn bearer_of(authorization: &str) -> Option<&str> {
    let (scheme, token) = authorization.trim().split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim()).filter(|t| !t.is_empty())
}

fn decode_json(part: &str) -> Option<serde_json::Value> {
    let bytes = URL_SAFE_NO_PAD.decode(part.trim_end_matches('=')).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Whether `token` is JWT-shaped: three base64url parts whose header is a
/// JSON object naming an `alg` (unsigned `none` tokens don't count).
pub fn is_jwt(token: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
        return false;
    }
    let Some(header) = decode_json(parts[0]) else {
        return false;
    };
    match header.get("alg").and_then(|a| a.as_str()) {
        Some(alg) => !alg.eq_ignore_ascii_case("none") && decode_json(parts[1]).is_some_and(|p| p.is_object()),
        None => false,
    }
}

/// A JWT's `exp` (seconds since 1970), if it has one.
pub fn jwt_exp(token: &str) -> Option<i64> {
    let payload = decode_json(token.split('.').nth(1)?)?;
    payload.get("exp").and_then(|e| e.as_i64().or_else(|| e.as_f64().map(|f| f as i64)))
}

/// Every JWT in `value`: the value itself, or tokens inside it (a JSON
/// blob such as `{"access_token":"…"}`, or `Bearer …`).
pub fn jwts_in(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    for run in value.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')) {
        let run = run.trim_matches('.');
        if run.len() >= 20 && is_jwt(run) && !out.iter().any(|t| t == run) {
            out.push(run.to_string());
        }
    }
    out
}

/// How long before its `exp` a token counts as expired already (the
/// request takes a moment, and clocks differ).
const EXP_MARGIN_S: i64 = 30;

/// The token to resume with, from a site's storage entries: JWTs only,
/// never an expired one; the one the download first went with (`captured`)
/// when it's still there and valid, else the one that lasts longest.
pub fn pick_jwt<'a>(items: impl IntoIterator<Item = &'a (String, String)>, captured: Option<&str>, now_s: i64) -> Option<String> {
    let mut tokens: Vec<String> = Vec::new();
    for (_, value) in items {
        for t in jwts_in(value) {
            if !tokens.contains(&t) {
                tokens.push(t);
            }
        }
    }
    tokens.retain(|t| jwt_exp(t).is_none_or(|exp| exp > now_s + EXP_MARGIN_S));
    if let Some(c) = captured
        && let Some(t) = tokens.iter().find(|t| *t == c)
    {
        return Some(t.clone());
    }
    // Longest-lasting first; tokens without `exp` after those with one.
    tokens.into_iter().max_by_key(|t| jwt_exp(t).map_or((0, 0), |e| (1, e)))
}

/// Run in the page a download came from: start `url` again as the page
/// would (a link to it, clicked), so the engine fetches it with the page's
/// own session.
pub fn retrigger_script(url: &str) -> String {
    let quoted = serde_json::to_string(url).unwrap_or_else(|_| "\"\"".into());
    format!(
        "(function(){{try{{var a=document.createElement('a');a.href={quoted};a.rel='noopener';a.style.display='none';\
         (document.body||document.documentElement).appendChild(a);a.click();a.remove();return 'ok';}}catch(e){{return 'error';}}}})()"
    )
}

/// Test helper (and for other crates' tests): a signed-looking JWT with
/// `claims`, so `alg` and `exp` parse.
pub fn test_jwt(claims: &str) -> String {
    format!(
        "{}.{}.{}",
        URL_SAFE_NO_PAD.encode(r#"{"alg":"HS256","typ":"JWT"}"#),
        URL_SAFE_NO_PAD.encode(claims),
        URL_SAFE_NO_PAD.encode("signature-bytes")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn items(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn jwts_are_recognised_by_shape_and_header() {
        let good = test_jwt(r#"{"sub":"me"}"#);
        assert!(is_jwt(&good));
        assert!(!is_jwt("a.b.c"));
        assert!(!is_jwt("not-a-token"));
        let none = format!("{}.{}.sig", URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#), URL_SAFE_NO_PAD.encode("{}"));
        assert!(!is_jwt(&none), "unsigned tokens don't count");
        let no_alg = format!("{}.{}.sig", URL_SAFE_NO_PAD.encode(r#"{"typ":"JWT"}"#), URL_SAFE_NO_PAD.encode("{}"));
        assert!(!is_jwt(&no_alg));
        assert_eq!(jwts_in(&format!(r#"{{"auth":{{"access_token":"{good}","x":1}}}}"#)), std::slice::from_ref(&good));
        assert_eq!(jwts_in(&format!("Bearer {good}")), [good]);
    }

    #[test]
    fn expired_tokens_are_skipped_and_the_captured_one_wins() {
        let expired = test_jwt(&format!(r#"{{"exp":{}}}"#, NOW - 10));
        let soon = test_jwt(&format!(r#"{{"exp":{}}}"#, NOW + 10));
        let later = test_jwt(&format!(r#"{{"exp":{}}}"#, NOW + 3600));
        let latest = test_jwt(&format!(r#"{{"exp":{},"n":2}}"#, NOW + 7200));
        assert_eq!(pick_jwt(&items(&[("t", &expired)]), None, NOW), None);
        assert_eq!(pick_jwt(&items(&[("t", &soon)]), None, NOW), None, "about to expire counts as expired");
        let all = items(&[("a", &expired), ("b", &later), ("c", "plain"), ("d", &latest)]);
        assert_eq!(pick_jwt(&all, None, NOW).as_deref(), Some(latest.as_str()), "the longest-lasting");
        assert_eq!(pick_jwt(&all, Some(&later), NOW).as_deref(), Some(later.as_str()), "the one it went with");
        assert_eq!(pick_jwt(&all, Some(&expired), NOW).as_deref(), Some(latest.as_str()), "never an expired one");
    }

    #[test]
    fn storage_results_parse_plain_or_encoded_twice() {
        let json = r#"{"origin":"http://a.test","items":[["k","v"]]}"#;
        let read = parse_storage(json).unwrap();
        assert_eq!((read.origin.as_str(), read.items.len()), ("http://a.test", 1));
        assert_eq!(parse_storage(&serde_json::to_string(json).unwrap()), Some(read.clone()));
        assert_eq!(parse_storage("null"), None);
        assert!(!format!("{read:?}").contains("\"v\""), "Debug shows no values");
    }

    #[test]
    fn origins_and_bearer_values() {
        assert_eq!(origin_of("HTTPS://Files.Example:443/a?b").as_deref(), Some("https://files.example"));
        assert_eq!(origin_of("http://127.0.0.1:8080/x").as_deref(), Some("http://127.0.0.1:8080"));
        assert_eq!(origin_of("http://user:pw@h.test/").as_deref(), Some("http://h.test"));
        assert_eq!(origin_of("about:blank"), None);
        assert_eq!(bearer_of("Bearer abc"), Some("abc"));
        assert_eq!(bearer_of("Basic abc"), None);
        assert!(retrigger_script("http://a.test/x\"y").contains(r#""http://a.test/x\"y""#));
    }
}
