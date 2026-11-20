//! `weirdnet-morph://` links: Morph registers the scheme with every OS
//! (Linux desktop file, macOS Info.plist, Windows registry) so a link
//! clicked in another app reaches the running Morph through the
//! single-instance handoff (Apple Events on macOS).
//!
//! What most such links *mean* is v0.3's (peer-to-peer content, PRIORITIES.md
//! roadmap). For now Morph parses and routes them: [`parse`] checks the
//! form and [`Link::describe`] gives a log-safe form (no query, no
//! fragment, control characters dropped, length capped), so a link carrying
//! tokens never lands in a log whole.
//!
//! One kind is handled today: a new device's device-link code,
//! `weirdnet-morph://link/…`, or `weirdnet-ymir://link/…` from a Ymir node
//! (`ymirctl pool link`), routes to [`Route::DeviceLink`]: Add a device with
//! the code filled in. The profile password and the six-digit check still
//! come first; the code is only checked for shape here, and
//! weirdnet-device-link verifies its signature when it pairs.
//!
//! The other direction too: an existing device's invite,
//! `weirdnet-morph://invite/…` (the code its Add a device shows), routes to
//! [`Route::Invite`]: this device waits for the identity with the invite
//! filled in. Ymir shows no invites (a Ymir node only joins), so
//! `weirdnet-ymir://invite/…` stays unknown.

/// The scheme, without `://`.
pub const SCHEME: &str = "weirdnet-morph";

/// Ymir's scheme (github.com/WeirdBrains/ymir). Morph opens its
/// device-link codes so a Ymir node joins the profile's devices.
pub const YMIR_SCHEME: &str = "weirdnet-ymir";

/// Every scheme Morph registers with the OS and routes.
pub const SCHEMES: &[&str] = &[SCHEME, YMIR_SCHEME];

/// The longest device-link code accepted (weirdnet-device-link's limit).
const CODE_MAX: usize = 4096;

/// The longest description [`Link::describe`] gives.
const DESCRIBE_MAX: usize = 120;

/// A parsed `weirdnet-morph://host/path?query#fragment` link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// Which of [`SCHEMES`] it uses (lowercase).
    pub scheme: &'static str,
    /// Lowercased host part (a peer or name, once v0.3 says which).
    pub host: String,
    /// Path segments, empty ones dropped.
    pub path: Vec<String>,
    /// Whether the link had a query or fragment (kept out of logs).
    pub has_query: bool,
}

/// What Morph does with a link today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// A new device's device-link code: open Add a device with it.
    DeviceLink(DeviceLink),
    /// An existing device's invite: receive its identity here.
    Invite(Invite),
    /// No handler knows this link yet; the shell logs [`Link::describe`]
    /// and tells the user.
    Unknown(Link),
}

/// A new device's code, `<scheme>://link/<base64url>`, shape-checked only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceLink {
    /// The code as weirdnet-device-link reads it (scheme in lowercase).
    pub code: String,
    /// Which app showed it.
    pub app: LinkApp,
}

/// An existing device's invite, `weirdnet-morph://invite/<base64url>`,
/// shape-checked only (weirdnet-device-link decodes it before it is used).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    /// The invite as weirdnet-device-link reads it (scheme in lowercase).
    pub code: String,
}

/// The app a device-link code came from, by its scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkApp {
    Morph,
    Ymir,
}

impl LinkApp {
    pub fn name(self) -> &'static str {
        match self {
            LinkApp::Morph => "Morph",
            LinkApp::Ymir => "Ymir",
        }
    }
}

/// Which of [`SCHEMES`] `text` starts with (`scheme:`, any case).
fn scheme_of(text: &str) -> Option<&'static str> {
    SCHEMES.iter().copied().find(|s| {
        text.len() > s.len() && text.is_char_boundary(s.len()) && text[..s.len()].eq_ignore_ascii_case(s) && text[s.len()..].starts_with(':')
    })
}

/// Whether `text` is a `weirdnet-morph:` or `weirdnet-ymir:` link (any case).
pub fn is_link(text: &str) -> bool {
    scheme_of(text.trim()).is_some()
}

/// Parse a `weirdnet-morph://` (or `weirdnet-ymir://`) link; `None` when it
/// is not one or is malformed (no `//`, control characters, an empty host,
/// too long).
pub fn parse(text: &str) -> Option<Link> {
    let text = text.trim();
    let scheme = scheme_of(text)?;
    if text.len() > CODE_MAX || text.chars().any(char::is_control) {
        return None;
    }
    let rest = text[scheme.len() + 1..].strip_prefix("//")?;
    let cut = rest.find(['?', '#']).unwrap_or(rest.len());
    let (main, tail) = rest.split_at(cut);
    let (host, path) = main.split_once('/').unwrap_or((main, ""));
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | ':')) {
        return None;
    }
    Some(Link {
        scheme,
        host: host.to_ascii_lowercase(),
        path: path.split('/').filter(|s| !s.is_empty()).map(str::to_string).collect(),
        has_query: !tail.is_empty(),
    })
}

/// Route a link: a device-link code goes to Add a device, a Morph invite
/// to receiving an identity; everything else waits for v0.3 (and
/// `weirdnet-ymir://` means nothing else to Morph).
pub fn route(link: Link) -> Route {
    if let Some(code) = device_link(&link) {
        return Route::DeviceLink(code);
    }
    match invite(&link) {
        Some(invite) => Route::Invite(invite),
        None => Route::Unknown(link),
    }
}

/// `<host>/<one non-empty base64url segment>` with nothing after it.
fn code_segment<'a>(link: &'a Link, host: &str) -> Option<&'a str> {
    let [code] = link.path.as_slice() else { return None };
    let base64url = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    (link.host == host && !link.has_query && !code.is_empty() && code.chars().all(base64url)).then_some(code.as_str())
}

/// `<scheme>://link/<code>` with a non-empty unpadded base64url code and
/// nothing after it. Not a signature check: that happens when it pairs.
fn device_link(link: &Link) -> Option<DeviceLink> {
    let code = code_segment(link, "link")?;
    let app = if link.scheme == YMIR_SCHEME { LinkApp::Ymir } else { LinkApp::Morph };
    Some(DeviceLink { code: format!("{}://link/{code}", link.scheme), app })
}

/// `weirdnet-morph://invite/<code>` (Morph's scheme only).
fn invite(link: &Link) -> Option<Invite> {
    let code = code_segment(link, "invite").filter(|_| link.scheme == SCHEME)?;
    Some(Invite { code: format!("{SCHEME}://invite/{code}") })
}

/// An existing device's invite in `text`, opened or pasted.
pub fn invite_code(text: &str) -> Option<Invite> {
    parse(text).as_ref().and_then(invite)
}

/// Whether `text` is a device-link code or an invite: links that start
/// pairing, which a locked profile refuses and the address bar hands to
/// the Identity panel instead of calling an app.
pub fn is_pairing_link(text: &str) -> bool {
    device_link_code(text).is_some() || invite_code(text).is_some()
}

/// A device-link code in `text`: a `weirdnet-morph://link/…` or
/// `weirdnet-ymir://link/…` link, opened or pasted.
pub fn device_link_code(text: &str) -> Option<DeviceLink> {
    parse(text).as_ref().and_then(device_link)
}

impl Link {
    /// A log-safe form: scheme, host and path only, capped in length.
    pub fn describe(&self) -> String {
        let mut out = format!("{}://{}", self.scheme, self.host);
        for segment in &self.path {
            out.push('/');
            out.push_str(segment);
        }
        if self.has_query {
            out.push_str("?…");
        }
        if out.chars().count() > DESCRIBE_MAX {
            out = out.chars().take(DESCRIBE_MAX).collect::<String>() + "…";
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_recognised_in_any_case() {
        assert!(is_link("weirdnet-morph://peer/a"));
        assert!(is_link(" WeirdNet-Morph://peer "));
        assert!(!is_link("weirdnet-morphx://peer"));
        assert!(!is_link("https://weirdnet-morph.example"));
        assert!(!is_link("weirdnet-morph"));
    }

    #[test]
    fn host_path_and_query_are_split() {
        let link = parse("weirdnet-morph://Peer.Example/site//page?token=secret#x").unwrap();
        assert_eq!(link.host, "peer.example");
        assert_eq!(link.path, ["site", "page"]);
        assert!(link.has_query);
        assert_eq!(parse("weirdnet-morph://peer").unwrap().path, Vec::<String>::new());
    }

    #[test]
    fn malformed_links_are_refused() {
        for bad in ["weirdnet-morph:peer", "weirdnet-morph:///path", "weirdnet-morph://pe er/x", "weirdnet-morph://peer/\u{7}", "https://peer"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        assert_eq!(parse(&format!("weirdnet-morph://peer/{}", "a".repeat(5000))), None);
    }

    #[test]
    fn descriptions_leave_out_the_query_and_are_capped() {
        let link = parse("weirdnet-morph://peer/site?token=secret").unwrap();
        let text = link.describe();
        assert_eq!(text, "weirdnet-morph://peer/site?…");
        assert!(!text.contains("secret"));
        let long = parse(&format!("weirdnet-morph://peer/{}", "b".repeat(400))).unwrap().describe();
        assert!(long.chars().count() <= DESCRIBE_MAX + 1);
    }

    #[test]
    fn other_links_route_to_unknown_for_now() {
        for text in [
            "weirdnet-morph://peer/x",
            "weirdnet-ymir://peer/x",
            "weirdnet-morph://link",
            "weirdnet-morph://link/",
            "weirdnet-morph://link/abc/def",
            "weirdnet-morph://link/abc?x=1",
            "weirdnet-morph://link/abc#x",
            "weirdnet-morph://link/ab+c",
            "weirdnet-morph://link/ab=",
            "weirdnet-ymir://invite/abc",
            "weirdnet-morph://invite/abc/def",
            "weirdnet-morph://invite/abc?x=1",
            "weirdnet-morph://invite/ab+c",
            "weirdnet-morph://linkx/abc",
        ] {
            let link = parse(text).unwrap();
            assert_eq!(route(link.clone()), Route::Unknown(link), "{text}");
            assert_eq!(device_link_code(text), None, "{text}");
            assert!(!is_pairing_link(text), "{text}");
        }
    }

    #[test]
    fn morph_invites_route_to_receiving_an_identity() {
        let invite = invite_code(" Weirdnet-Morph://INVITE/gqF2AQ_-x9 ").unwrap();
        assert_eq!(invite, Invite { code: "weirdnet-morph://invite/gqF2AQ_-x9".into() });
        assert_eq!(route(parse("weirdnet-morph://invite/gqF2AQ_-x9").unwrap()), Route::Invite(invite));
        assert_eq!(device_link_code("weirdnet-morph://invite/gqF2AQ_-x9"), None);
        assert_eq!(invite_code("weirdnet-ymir://invite/gqF2AQ_-x9"), None, "Ymir shows no invites");
        assert_eq!(invite_code("weirdnet-morph://link/gqF2AQ_-x9"), None);
        assert!(is_pairing_link("weirdnet-morph://invite/abc") && is_pairing_link("weirdnet-ymir://link/abc"));
        assert!(!is_pairing_link("weirdnet-morph://peer/abc") && !is_pairing_link("https://example.com/invite/abc"));
    }

    #[test]
    fn ymir_links_are_recognised_and_parsed() {
        assert!(is_link("weirdnet-ymir://link/abc"));
        assert!(is_link(" WEIRDNET-YMIR://x "));
        assert!(!is_link("weirdnet-ymirx://link/abc"));
        assert!(!is_link("weirdnet-bragi://link/abc"));
        assert!(!is_link("weirdnet://link/abc"));
        let link = parse("weirdnet-ymir://Peer/a?t=s").unwrap();
        assert_eq!(link.scheme, YMIR_SCHEME);
        assert_eq!(link.describe(), "weirdnet-ymir://peer/a?…");
    }

    #[test]
    fn device_link_codes_route_to_add_a_device() {
        let morph = device_link_code(" weirdnet-morph://link/gqF2AQ_-x9 ").unwrap();
        assert_eq!(morph, DeviceLink { code: "weirdnet-morph://link/gqF2AQ_-x9".into(), app: LinkApp::Morph });
        let ymir = device_link_code("weirdnet-ymir://link/gqF2AQ_-x9").unwrap();
        assert_eq!(ymir.app, LinkApp::Ymir);
        assert_eq!(ymir.app.name(), "Ymir");
        assert_eq!(ymir.code, "weirdnet-ymir://link/gqF2AQ_-x9");
        // The scheme and `link` are any case; the code keeps its case.
        let shouted = device_link_code("Weirdnet-Ymir://LINK/AbC").unwrap();
        assert_eq!(shouted.code, "weirdnet-ymir://link/AbC");
        assert_eq!(route(parse("weirdnet-ymir://link/AbC").unwrap()), Route::DeviceLink(shouted));
        // Too long, control characters, a bare `weirdnet://` code: not codes.
        assert_eq!(device_link_code(&format!("weirdnet-ymir://link/{}", "a".repeat(CODE_MAX))), None);
        assert_eq!(device_link_code("weirdnet-ymir://link/ab\u{7}c"), None);
        assert_eq!(device_link_code("weirdnet://link/abc"), None);
        assert_eq!(device_link_code("https://example.com/link/abc"), None);
    }
}
