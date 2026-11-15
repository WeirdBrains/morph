//! The identity panel's model, with no GPUI: a profile's Weirdnet identity
//! onboarding (create, restore from words or a backup file, link from
//! another device), its recovery words and backups, and adding a device.
//! The work is `morph-services`; this holds the steps, checks what the user
//! typed and keeps link sessions moving.

use std::path::{Path, PathBuf};

use morph_identity::profile_identity::{ProfileIdentity, SecretString, Source, Zeroizing};
use morph_services::device_link::{LinkState, Links};

/// Shortest profile or backup password accepted.
pub const MIN_PASSWORD: usize = 8;

/// Where the panel is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// No identity yet: pick how to get one.
    Choose,
    Create,
    /// The new identity's recovery words, shown once.
    Words(String),
    Import,
    Restore,
    /// This device waits for an identity from another device.
    Offer {
        session: u64,
        link: String,
    },
    /// Both devices show `sas`; the user compares them.
    Paired {
        session: u64,
        sas: String,
    },
    /// The identity arrived; it needs this device's password.
    Received {
        session: u64,
    },
    /// The profile has an identity.
    Overview,
    /// Recovery words after the password.
    Reveal,
    RevealWords(String),
    Export,
    /// Send this profile's identity to a new device: password and its link.
    AddDevice,
    /// Paired with the new device; compare `sas` and send.
    Adding {
        session: u64,
        sas: String,
        device: String,
    },
}

/// One text field a step asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub label: &'static str,
    pub masked: bool,
}

const fn field(label: &'static str, masked: bool) -> Field {
    Field { label, masked }
}

/// What a submit did, for the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing outside the panel changed.
    Stay,
    /// The profile has a new identity (and an encrypted library).
    IdentityChanged,
}

pub struct Identity {
    home: PathBuf,
    profile: String,
    profile_name: String,
    pub step: Step,
    pub identity: Option<ProfileIdentity>,
    pub error: Option<String>,
    pub notice: Option<String>,
    links: &'static Links,
}

impl Identity {
    pub fn new(home: &Path, profile: &str, profile_name: &str, links: &'static Links) -> Self {
        let mut this = Self {
            home: home.to_path_buf(),
            profile: profile.to_string(),
            profile_name: profile_name.to_string(),
            step: Step::Choose,
            identity: None,
            error: None,
            notice: None,
            links,
        };
        this.reload();
        this
    }

    /// Read the profile's identity again and go to its first step.
    pub fn reload(&mut self) {
        self.identity = morph_services::identity::load(&self.home, &self.profile).ok().flatten();
        self.step = if self.identity.is_some() { Step::Overview } else { Step::Choose };
    }

    pub fn set_profile_name(&mut self, name: &str) {
        self.profile_name = name.to_string();
    }

    /// The fields the current step asks for, in order.
    pub fn fields(&self) -> &'static [Field] {
        const PASSWORD: Field = field("Password for this profile", true);
        const NEW: Field = field("New password for this profile", true);
        const CREATE: &[Field] = &[field("New password (8 characters or more)", true), field("The same password again", true)];
        const IMPORT: &[Field] = &[field("The 24 recovery words", false), NEW];
        const RESTORE: &[Field] = &[field("Backup file (path)", false), field("The backup's password (empty if none)", true), NEW];
        const RECEIVED: &[Field] = &[NEW];
        const REVEAL: &[Field] = &[PASSWORD];
        const EXPORT: &[Field] = &[PASSWORD, field("A password for the backup", true), field("Save the backup as (path)", false)];
        const ADD: &[Field] = &[PASSWORD, field("The link the new device shows", false)];
        match self.step {
            Step::Create => CREATE,
            Step::Import => IMPORT,
            Step::Restore => RESTORE,
            Step::Received { .. } => RECEIVED,
            Step::Reveal => REVEAL,
            Step::Export => EXPORT,
            Step::AddDevice => ADD,
            _ => &[],
        }
    }

    /// Text to put in the fields when a step starts (a suggested path).
    pub fn defaults(&self) -> Vec<String> {
        match self.step {
            Step::Export => vec![String::new(), String::new(), suggested_backup_path(&self.profile_name)],
            _ => Vec::new(),
        }
    }

    /// Go to `step` from a button or key. Starting an offer opens a link
    /// session.
    pub fn start(&mut self, step: Step) {
        self.error = None;
        self.notice = None;
        self.end_sessions();
        self.step = step;
    }

    /// Become a new device: listen and show the link.
    pub fn start_offer(&mut self) {
        self.start(Step::Choose);
        match self.links.new_device_start(&device_name()) {
            Ok(offer) => {
                if std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1") {
                    println!("morph-event link offer={}", offer.link);
                }
                self.step = Step::Offer { session: offer.session, link: offer.link };
            }
            Err(e) => self.error = Some(format!("Could not start linking: {e}")),
        }
    }

    /// Back to the first step, ending any link session.
    pub fn cancel(&mut self) {
        self.start(if self.identity.is_some() { Step::Overview } else { Step::Choose });
    }

    fn end_sessions(&mut self) {
        match self.step {
            Step::Offer { session, .. } | Step::Paired { session, .. } | Step::Received { session } => self.links.new_device_cancel(session),
            Step::Adding { session, .. } => self.links.add_device_cancel(session),
            _ => {}
        }
    }

    /// Follow the new device's link session. Returns whether the step changed.
    pub fn poll(&mut self) -> bool {
        let session = match self.step {
            Step::Offer { session, .. } | Step::Paired { session, .. } => session,
            _ => return false,
        };
        let next = match self.links.new_device_status(session) {
            Ok(LinkState::Waiting) => return false,
            Ok(LinkState::Paired { sas }) => match &self.step {
                Step::Paired { .. } => return false,
                _ => Step::Paired { session, sas },
            },
            Ok(LinkState::Received { .. }) => Step::Received { session },
            Ok(other) => {
                self.error = Some(link_failure(&other));
                if self.identity.is_some() { Step::Overview } else { Step::Choose }
            }
            Err(e) => {
                self.error = Some(format!("The link ended: {e}"));
                Step::Choose
            }
        };
        self.step = next;
        true
    }

    /// The user pressed Enter or the step's main button with `values` in
    /// its fields.
    pub fn submit(&mut self, values: &[String]) -> Outcome {
        self.error = None;
        match self.try_submit(values) {
            Ok(outcome) => outcome,
            Err(e) => {
                self.error = Some(e);
                Outcome::Stay
            }
        }
    }

    fn try_submit(&mut self, values: &[String]) -> Result<Outcome, String> {
        let v = |i: usize| values.get(i).map(String::as_str).unwrap_or("");
        let (home, profile) = (self.home.clone(), self.profile.clone());
        match self.step.clone() {
            Step::Create => {
                let pw = new_password(v(0), Some(v(1)))?;
                let made = morph_services::identity::create(&home, &profile, &pw).map_err(|e| e.to_string())?;
                self.identity = Some(made.identity);
                self.step = Step::Words(made.words.to_string());
                Ok(Outcome::IdentityChanged)
            }
            Step::Words(_) => {
                self.step = Step::Overview;
                self.notice = Some("Keep the recovery words somewhere safe; they are the only way back without this device.".into());
                Ok(Outcome::Stay)
            }
            Step::Import => {
                let words = recovery_words(v(0))?;
                let pw = new_password(v(1), None)?;
                let identity = morph_services::identity::import(&home, &profile, &words, &pw)
                    .map_err(|e| format!("Those words did not restore an identity: {e}"))?;
                self.done(identity, "Identity restored from its recovery words.")
            }
            Step::Restore => {
                let path = expand_home(v(0).trim());
                let contents = std::fs::read(&path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
                let backup_pw = (!v(1).is_empty()).then(|| SecretString::from(v(1).to_string()));
                let pw = new_password(v(2), None)?;
                let identity = morph_services::identity::restore_backup(&home, &profile, &contents, backup_pw.as_ref(), &pw)
                    .map_err(|e| format!("Could not restore the backup: {e}"))?;
                self.done(identity, "Identity restored from the backup.")
            }
            Step::Offer { .. } => Ok(Outcome::Stay),
            Step::Paired { session, .. } => {
                self.links.new_device_confirm(session).map_err(|e| e.to_string())?;
                self.notice = Some("Codes confirmed; waiting for the other device to send.".into());
                Ok(Outcome::Stay)
            }
            Step::Received { session } => {
                let pw = new_password(v(0), None)?;
                // Seeing the step means the codes were compared on both screens.
                let _ = self.links.new_device_confirm(session);
                let identity = self.links.new_device_finish(&home, &profile, session, &pw).map_err(|e| e.to_string())?;
                self.done(identity, "This profile now has the identity from your other device.")
            }
            Step::Overview => Ok(Outcome::Stay),
            Step::Reveal => {
                let words = morph_services::identity::recovery_words(&home, &profile, &password(v(0))).map_err(|_| "Wrong password.".to_string())?;
                self.step = Step::RevealWords(words.to_string());
                Ok(Outcome::Stay)
            }
            Step::RevealWords(_) => {
                self.step = Step::Overview;
                Ok(Outcome::Stay)
            }
            Step::Export => {
                let backup_pw = new_password(v(1), None)?;
                let path = expand_home(v(2).trim());
                if v(2).trim().is_empty() {
                    return Err("Say where to save the backup.".into());
                }
                if path.exists() {
                    return Err(format!("{} already exists; choose another name.", path.display()));
                }
                let armored = morph_services::identity::export_backup(&home, &profile, &password(v(0)), &backup_pw)
                    .map_err(|_| "Wrong password.".to_string())?;
                write_private(&path, armored.as_bytes()).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
                self.step = Step::Overview;
                self.notice = Some(format!("Backup saved to {}.", path.display()));
                Ok(Outcome::Stay)
            }
            Step::AddDevice => {
                let link = v(1).trim();
                if link.is_empty() {
                    return Err("Paste the link the new device shows.".into());
                }
                let pairing = self
                    .links
                    .add_device_start(&home, &profile, &self.profile_name, &password(v(0)), link)
                    .map_err(|e| format!("Could not pair: {e}"))?;
                self.step = Step::Adding { session: pairing.session, sas: pairing.sas, device: pairing.device };
                Ok(Outcome::Stay)
            }
            Step::Adding { session, device, .. } => {
                self.links.add_device_send(session).map_err(|e| format!("Could not send: {e}"))?;
                self.step = Step::Overview;
                self.notice = Some(format!("Sent this profile's identity to {device}."));
                Ok(Outcome::Stay)
            }
            Step::Choose => Ok(Outcome::Stay),
        }
    }

    fn done(&mut self, identity: ProfileIdentity, notice: &str) -> Result<Outcome, String> {
        self.identity = Some(identity);
        self.step = Step::Overview;
        self.notice = Some(notice.into());
        Ok(Outcome::IdentityChanged)
    }
}

impl Drop for Identity {
    fn drop(&mut self) {
        self.end_sessions();
    }
}

/// How an identity came to this profile, for the overview.
pub fn source_text(source: &Source) -> &'static str {
    match source {
        Source::Created => "created on this device",
        Source::Imported => "restored from recovery words or a backup",
        Source::Linked => "linked from another device",
    }
}

fn password(text: &str) -> SecretString {
    SecretString::from(text.to_string())
}

/// A new password, long enough and (when `again` is given) typed twice the same.
fn new_password(text: &str, again: Option<&str>) -> Result<SecretString, String> {
    if text.chars().count() < MIN_PASSWORD {
        return Err(format!("Use at least {MIN_PASSWORD} characters."));
    }
    if again.is_some_and(|a| a != text) {
        return Err("The passwords differ.".into());
    }
    Ok(password(text))
}

/// The recovery words, normalised: 24 words separated by single spaces.
fn recovery_words(text: &str) -> Result<Zeroizing<String>, String> {
    let words: Vec<String> = text.split_whitespace().map(str::to_lowercase).collect();
    if words.len() != 24 {
        return Err(format!("A recovery phrase has 24 words; that has {}.", words.len()));
    }
    Ok(Zeroizing::new(words.join(" ")))
}

/// `~/x` to the home directory's `x`.
fn expand_home(path: &str) -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    match (path.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

/// `~/morph-<profile>-identity-backup.txt`, the export's suggested name.
fn suggested_backup_path(profile_name: &str) -> String {
    let slug: String = profile_name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let slug = slug.trim_matches('-');
    format!("~/morph-{}-identity-backup.txt", if slug.is_empty() { "profile" } else { slug })
}

/// Write a new file only the user can read.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    // Windows: a new file in the user's profile inherits the profile's ACL
    // (the user, SYSTEM and administrators).
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// This computer's name, shown on the other device.
fn device_name() -> String {
    let host = std::fs::read_to_string("/etc/hostname").ok().or_else(|| std::env::var("COMPUTERNAME").ok()).unwrap_or_default();
    let host = host.trim();
    if host.is_empty() { format!("Morph on {}", os_name()) } else { format!("Morph on {host}") }
}

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        _ => "Linux",
    }
}

fn link_failure(state: &LinkState) -> String {
    format!("The link did not finish ({state:?}). Start again on both devices.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn home() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::create(&home, "Work").unwrap();
        (tmp, home)
    }

    fn links() -> &'static Links {
        Box::leak(Box::default())
    }

    #[test]
    fn create_checks_the_password_then_shows_the_words_once() {
        let (_t, home) = home();
        let mut id = Identity::new(&home, "p1", "Work", links());
        assert_eq!(id.step, Step::Choose);
        id.start(Step::Create);
        assert_eq!(id.fields().len(), 2);
        assert_eq!(id.submit(&s(&["short", "short"])), Outcome::Stay);
        assert!(id.error.as_deref().unwrap().contains("8 characters"));
        assert_eq!(id.submit(&s(&["long enough", "long enougH"])), Outcome::Stay);
        assert_eq!(id.error.as_deref(), Some("The passwords differ."));
        assert_eq!(id.submit(&s(&["long enough", "long enough"])), Outcome::IdentityChanged);
        let Step::Words(words) = id.step.clone() else { panic!("{:?}", id.step) };
        assert_eq!(words.split_whitespace().count(), 24);
        id.submit(&[]);
        assert_eq!(id.step, Step::Overview);
        assert!(id.identity.is_some());
        assert_eq!(Identity::new(&home, "p1", "Work", links()).step, Step::Overview, "reloads as onboarded");

        id.start(Step::Reveal);
        id.submit(&s(&["wrong password"]));
        assert_eq!(id.error.as_deref(), Some("Wrong password."));
        id.submit(&s(&["long enough"]));
        assert_eq!(id.step, Step::RevealWords(words));
    }

    #[test]
    fn a_backup_restores_into_another_profile_and_words_into_a_third() {
        let (t, home) = home();
        morph_core::profiles::create(&home, "Spare").unwrap();
        let mut id = Identity::new(&home, "p1", "Work", links());
        id.start(Step::Create);
        id.submit(&s(&["long enough", "long enough"]));
        let Step::Words(words) = id.step.clone() else { panic!() };
        id.submit(&[]);

        let path = t.path().join("backup.txt");
        id.start(Step::Export);
        assert_eq!(id.defaults()[2], "~/morph-work-identity-backup.txt");
        id.submit(&s(&["long enough", "short", path.to_str().unwrap()]));
        assert!(id.error.is_some() && !path.exists(), "backup password too short");
        id.submit(&s(&["wrong pass", "backup pass", path.to_str().unwrap()]));
        assert_eq!(id.error.as_deref(), Some("Wrong password."));
        id.submit(&s(&["long enough", "backup pass", path.to_str().unwrap()]));
        assert_eq!(id.step, Step::Overview, "{:?}", id.error);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        id.start(Step::Export);
        id.submit(&s(&["long enough", "backup pass", path.to_str().unwrap()]));
        assert!(id.error.as_deref().unwrap().contains("already exists"));

        let mut spare = Identity::new(&home, "p2", "Spare", links());
        spare.start(Step::Restore);
        assert_eq!(spare.submit(&s(&[path.to_str().unwrap(), "nope nope", "third pass"])), Outcome::Stay);
        assert_eq!(spare.submit(&s(&[path.to_str().unwrap(), "backup pass", "third pass"])), Outcome::IdentityChanged);
        assert_eq!(spare.identity.as_ref().unwrap().user_id, id.identity.as_ref().unwrap().user_id);

        let mut default = Identity::new(&home, "default", "Default", links());
        default.start(Step::Import);
        default.submit(&s(&["just three words", "other pass"]));
        assert!(default.error.as_deref().unwrap().contains("24 words"));
        let shouted = words.to_uppercase().replace(' ', "  ");
        assert_eq!(default.submit(&s(&[&shouted, "other pass"])), Outcome::IdentityChanged, "{:?}", default.error);
        assert_eq!(default.identity.as_ref().unwrap().user_id, id.identity.as_ref().unwrap().user_id);
    }

    /// Two panels, two homes, one process: the whole QR link over real TCP.
    #[test]
    fn linking_moves_the_identity_to_the_new_device() {
        let (_a, old_home) = home();
        let (_b, new_home) = home();
        let shared = links();
        let mut old = Identity::new(&old_home, "p1", "Work", shared);
        old.start(Step::Create);
        old.submit(&s(&["old password", "old password"]));
        old.submit(&[]);

        let mut new = Identity::new(&new_home, "p1", "Work", shared);
        new.start_offer();
        let Step::Offer { link, .. } = new.step.clone() else { panic!("{:?} {:?}", new.step, new.error) };
        old.start(Step::AddDevice);
        old.submit(&s(&["old password", &link]));
        let Step::Adding { sas, .. } = old.step.clone() else { panic!("{:?} {:?}", old.step, old.error) };

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !matches!(new.step, Step::Paired { .. }) && std::time::Instant::now() < deadline {
            new.poll();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(
            new.step,
            Step::Paired {
                session: match new.step {
                    Step::Paired { session, .. } => session,
                    _ => 0,
                },
                sas: sas.clone()
            }
        );
        new.submit(&[]);
        old.submit(&[]);
        assert_eq!(old.step, Step::Overview, "{:?}", old.error);
        while !matches!(new.step, Step::Received { .. }) && std::time::Instant::now() < deadline {
            new.poll();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(matches!(new.step, Step::Received { .. }), "{:?}", new.step);
        assert_eq!(new.submit(&s(&["new password"])), Outcome::IdentityChanged, "{:?}", new.error);
        assert_eq!(new.identity.as_ref().unwrap().user_id, old.identity.as_ref().unwrap().user_id);
        assert_eq!(new.identity.as_ref().unwrap().source, Source::Linked);
    }

    #[test]
    fn small_rules() {
        assert_eq!(suggested_backup_path("My Stuff!"), "~/morph-my-stuff-identity-backup.txt");
        assert_eq!(suggested_backup_path("☺"), "~/morph-profile-identity-backup.txt");
        assert!(new_password("1234567", None).is_err());
        assert!(new_password("12345678", Some("12345678")).is_ok());
        assert_eq!(expand_home("/abs/x"), PathBuf::from("/abs/x"));
    }
}
