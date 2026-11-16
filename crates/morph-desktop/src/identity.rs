//! The identity panel's model, with no GPUI: a profile's Weirdnet identity
//! onboarding (create, restore from words or a backup file, link from
//! another device), its recovery words and backups, and adding a device.
//! The work is `morph-services`; this holds the steps, checks what the user
//! typed and keeps link sessions moving.

use std::path::{Path, PathBuf};

use morph_identity::profile_identity::{self, ProfileIdentity, SecretString, Source, Zeroizing};
use morph_services::device_link::{InviteView, LinkState, Links, Offer, is_invite};
use morph_services::relays::{RelayReport, RelayStatus};
use std::sync::mpsc;

/// How often a link screen reads the relay state again.
const RELAY_POLL: std::time::Duration = std::time::Duration::from_secs(1);

/// Shortest profile or backup password accepted.
pub const MIN_PASSWORD: usize = 8;

/// Paper shards: any `SHARD_THRESHOLD` of `SHARD_COUNT` rebuild the identity.
pub const SHARD_THRESHOLD: usize = 3;
pub const SHARD_COUNT: usize = 5;

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
    /// Paper shards after the password.
    Shards,
    /// The shards, shown until the user leaves the step.
    ShardList(Vec<String>),
    /// Send this profile's identity to a new device: this device's QR (the
    /// invite) for the new device to scan, or the new device's link pasted
    /// here; then the password.
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
    /// The button beside the field.
    pub helper: Helper,
}

/// What fills a field besides typing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Helper {
    None,
    /// Paste from the clipboard.
    Paste,
    /// Choose files to open (a native picker; several allowed).
    Open,
    /// Choose where to save (a native Save dialog).
    Save,
}

const fn field(label: &'static str, masked: bool) -> Field {
    Field { label, masked, helper: Helper::None }
}

const fn with(label: &'static str, helper: Helper) -> Field {
    Field { label, masked: false, helper }
}

/// A secret the panel shows that can be copied or saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Secret {
    Words,
    /// Shard number `n` (counting from 0).
    Shard(usize),
}

/// The link screens' relay line for `report`: the summary and the least
/// ready relay's state (no relay at all reads as no node).
pub fn relay_line(report: &RelayReport) -> (String, RelayStatus) {
    let order = |s: RelayStatus| match s {
        RelayStatus::NoNode => 0,
        RelayStatus::NotConnected => 1,
        RelayStatus::Connected => 2,
        RelayStatus::Reserved => 3,
    };
    let worst = report.relays.iter().map(|r| report.status(r)).min_by_key(|s| order(*s)).unwrap_or(RelayStatus::NoNode);
    (report.summary(), worst)
}

/// What a submit did, for the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing outside the panel changed.
    Stay,
    /// The profile has a new identity (and an encrypted library).
    IdentityChanged,
}

/// A link node starting on its own thread: building it, listening, and
/// waiting a few seconds for a relay circuit (DNS for the relay, a dial)
/// would freeze the window and the pages in it.
enum Starting {
    Invite(mpsc::Receiver<Result<InviteView, String>>),
    Offer(mpsc::Receiver<Result<Offer, String>>),
}

/// Run `work` on a thread; its answer comes back on the receiver.
fn in_background<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> mpsc::Receiver<T> {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("morph-link-start".into()).spawn(move || {
        let _ = tx.send(work());
    });
    if let Err(e) = spawned {
        eprintln!("morph: cannot start a link thread: {e}");
    }
    rx
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
    /// Add a device: the QR this device shows, while the step lasts.
    invite: Option<InviteView>,
    /// The link a new device answered the invite with.
    answered: Option<String>,
    /// Something changed without a new step (the invite was answered).
    dirty: bool,
    /// The relay status line on the link screens, and when it was read.
    relay_line: Option<(String, RelayStatus)>,
    relays_read: Option<std::time::Instant>,
    /// A link node still starting (see [`Starting`]).
    starting: Option<Starting>,
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
            invite: None,
            answered: None,
            dirty: false,
            relay_line: None,
            relays_read: None,
            starting: None,
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
        const IMPORT: &[Field] = &[with("The 24 recovery words, or enough shards", Helper::Paste), NEW];
        const RESTORE: &[Field] = &[with("Backup, words or shard files", Helper::Open), field("The backup's password (empty if none)", true), NEW];
        const RECEIVED: &[Field] = &[NEW];
        const REVEAL: &[Field] = &[PASSWORD];
        const EXPORT: &[Field] = &[PASSWORD, field("A password for the backup", true), with("Save the backup as", Helper::Save)];
        const ADD: &[Field] = &[PASSWORD, with("Or paste the link the new device shows", Helper::Paste)];
        const OFFER: &[Field] = &[with("Or paste the link your other device shows", Helper::Paste)];
        match self.step {
            Step::Create => CREATE,
            Step::Import => IMPORT,
            Step::Restore => RESTORE,
            Step::Received { .. } => RECEIVED,
            Step::Reveal | Step::Shards => REVEAL,
            Step::Export => EXPORT,
            Step::AddDevice => ADD,
            Step::Offer { .. } => OFFER,
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
        if self.step == Step::AddDevice {
            self.start_invite();
        }
    }

    /// Add a device: listen for a new device that scans this device's QR.
    fn start_invite(&mut self) {
        let links = self.links;
        self.starting = Some(Starting::Invite(in_background(move || links.invite_start().map_err(|e| e.to_string()))));
    }

    /// Whether a link node is still starting (the code is not ready yet).
    pub fn is_starting(&self) -> bool {
        self.starting.is_some()
    }

    /// Pick up a link node that finished starting. Returns whether the
    /// panel changed. A node that is no longer wanted (the user left the
    /// step) never arrives: [`Self::end_sessions`] drops the receiver and
    /// the finished node is cancelled here instead.
    fn poll_starting(&mut self) -> bool {
        let log = std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1");
        match self.starting.take() {
            None => false,
            Some(Starting::Invite(rx)) => match rx.try_recv() {
                Err(mpsc::TryRecvError::Empty) => {
                    self.starting = Some(Starting::Invite(rx));
                    false
                }
                Ok(Ok(invite)) if self.step == Step::AddDevice => {
                    if log {
                        println!("morph-event link invite={}", invite.link);
                    }
                    self.invite = Some(invite);
                    true
                }
                Ok(Ok(invite)) => {
                    self.links.invite_cancel(invite.session);
                    false
                }
                Ok(Err(e)) => {
                    self.notice = Some(format!("Could not show a code here ({e}); paste the new device's link instead."));
                    true
                }
                Err(mpsc::TryRecvError::Disconnected) => true,
            },
            Some(Starting::Offer(rx)) => match rx.try_recv() {
                Err(mpsc::TryRecvError::Empty) => {
                    self.starting = Some(Starting::Offer(rx));
                    false
                }
                Ok(Ok(offer)) if matches!(self.step, Step::Offer { session: 0, .. }) => {
                    if log {
                        println!("morph-event link offer={}", offer.link);
                    }
                    self.step = Step::Offer { session: offer.session, link: offer.link };
                    true
                }
                Ok(Ok(offer)) => {
                    self.links.new_device_cancel(offer.session);
                    false
                }
                Ok(Err(e)) => {
                    self.error = Some(format!("Could not start linking: {e}"));
                    self.step = Step::Choose;
                    true
                }
                Err(mpsc::TryRecvError::Disconnected) => true,
            },
        }
    }

    /// Tests: wait for a starting link node.
    #[cfg(test)]
    pub fn wait_started(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while self.starting.is_some() && std::time::Instant::now() < deadline {
            self.poll_starting();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// The QR text Add a device shows, while it listens.
    pub fn invite_link(&self) -> Option<&str> {
        self.invite.as_ref().map(|i| i.link.as_str())
    }

    /// Whether a new device answered this device's QR.
    pub fn answered(&self) -> bool {
        self.answered.is_some()
    }

    /// Whether something on the step changed since the last call.
    /// Whether the step runs a link node (and so shows the relay line).
    pub fn links_running(&self) -> bool {
        matches!(self.step, Step::Offer { .. } | Step::Paired { .. } | Step::Adding { .. } | Step::AddDevice)
    }

    /// The relay status line for the link screens: its text and the worst
    /// relay state, read from the running link nodes.
    pub fn relay_line(&self) -> Option<&(String, RelayStatus)> {
        self.relay_line.as_ref().filter(|_| self.links_running())
    }

    /// Read the relay state again (at most every [`RELAY_POLL`]) while a link
    /// screen shows. Marks the panel dirty when the line changed.
    pub fn poll_relays(&mut self) {
        if !self.links_running() {
            self.relays_read = None;
            return;
        }
        if self.relays_read.is_some_and(|t| t.elapsed() < RELAY_POLL) {
            return;
        }
        self.relays_read = Some(std::time::Instant::now());
        let line = relay_line(&self.links.relays());
        if self.relay_line.as_ref() != Some(&line) {
            if std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1") {
                println!("morph-event link relays state={}", line.1.as_str());
            }
            self.relay_line = Some(line);
            self.dirty = true;
        }
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    fn end_invite(&mut self) {
        if let Some(invite) = self.invite.take() {
            self.links.invite_cancel(invite.session);
        }
        self.answered = None;
    }

    /// Become a new device: listen and show the link.
    /// The step is `Offer` with session 0 and no link until the node is up.
    pub fn start_offer(&mut self) {
        self.start(Step::Choose);
        let (links, name) = (self.links, device_name());
        self.starting = Some(Starting::Offer(in_background(move || links.new_device_start(&name).map_err(|e| e.to_string()))));
        self.step = Step::Offer { session: 0, link: String::new() };
    }

    /// Back to the first step, ending any link session.
    pub fn cancel(&mut self) {
        self.start(if self.identity.is_some() { Step::Overview } else { Step::Choose });
    }

    fn end_sessions(&mut self) {
        // A node still starting: let it finish on its thread and drop it there.
        if let Some(starting) = self.starting.take() {
            let links = self.links;
            std::thread::spawn(move || match starting {
                Starting::Invite(rx) => {
                    if let Ok(Ok(i)) = rx.recv() {
                        links.invite_cancel(i.session);
                    }
                }
                Starting::Offer(rx) => {
                    if let Ok(Ok(o)) = rx.recv() {
                        links.new_device_cancel(o.session);
                    }
                }
            });
        }
        self.end_invite();
        match self.step {
            Step::Offer { session, .. } | Step::Paired { session, .. } | Step::Received { session } => self.links.new_device_cancel(session),
            Step::Adding { session, .. } => self.links.add_device_cancel(session),
            _ => {}
        }
    }

    /// Follow the new device's link session. Returns whether the step changed.
    pub fn poll(&mut self) -> bool {
        let before = std::mem::discriminant(&self.step);
        if self.poll_starting() {
            // A failed start goes back a step; a code arriving only redraws
            // (the fields keep what was typed).
            if std::mem::discriminant(&self.step) != before {
                return true;
            }
            self.dirty = true;
        }
        if self.step == Step::AddDevice {
            self.poll_invite();
            return false;
        }
        let session = match self.step {
            Step::Offer { session: 0, .. } => return false,
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

    /// Add a device: pick up the new device's answer; an expired code is
    /// replaced with a fresh one.
    fn poll_invite(&mut self) {
        let Some(session) = self.invite.as_ref().map(|i| i.session) else { return };
        if self.answered.is_some() {
            return;
        }
        match self.links.invite_take(session) {
            Ok(Some(link)) => {
                self.answered = Some(link);
                if std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1") {
                    println!("morph-event link answered");
                }
                self.notice = Some("The new device answered. Type this profile's password and press Pair to compare codes.".into());
                self.dirty = true;
            }
            Ok(None) => {}
            Err(_) => {
                self.invite = None;
                self.start_invite();
                self.dirty = true;
            }
        }
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
                let paths = split_paths(v(0));
                if paths.is_empty() {
                    return Err("Choose the backup file.".into());
                }
                let mut files = Vec::new();
                for path in &paths {
                    files.push(Zeroizing::new(std::fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?));
                }
                let shards: Vec<String> = files.iter().flat_map(|f| find_shards(&String::from_utf8_lossy(f))).collect();
                if !shards.is_empty() {
                    let words = words_from_shards(&shards)?;
                    let pw = new_password(v(2), None)?;
                    let identity = morph_services::identity::import(&home, &profile, &words, &pw)
                        .map_err(|e| format!("Those shards did not restore an identity: {e}"))?;
                    return self.done(identity, "Identity restored from its shards.");
                }
                if paths.len() > 1 {
                    return Err("Choose one backup file (several files only for shards).".into());
                }
                let contents = &files[0];
                let backup_pw = (!v(1).is_empty()).then(|| SecretString::from(v(1).to_string()));
                let pw = new_password(v(2), None)?;
                let identity = morph_services::identity::restore_backup(&home, &profile, contents, backup_pw.as_ref(), &pw)
                    .map_err(|e| format!("Could not restore the backup: {e}"))?;
                self.done(identity, "Identity restored from the backup.")
            }
            Step::Offer { session: 0, .. } => Err("Still getting this device's code ready; a moment.".into()),
            Step::Offer { session, .. } => {
                let invite = v(0).trim();
                if invite.is_empty() {
                    return Err("Scan this code on your other device, or paste the link it shows here.".into());
                }
                if !is_invite(invite) {
                    return Err("That is a new device's link: paste it under Add a device on the device that has the identity.".into());
                }
                self.links.new_device_answer(session, invite).map_err(|e| format!("Could not reach the other device: {e}"))?;
                self.notice = Some("Sent. Waiting for the other device to pair…".into());
                Ok(Outcome::Stay)
            }
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
            Step::RevealWords(_) | Step::ShardList(_) => {
                self.step = Step::Overview;
                Ok(Outcome::Stay)
            }
            Step::Shards => {
                let shards = morph_services::identity::shards(&home, &profile, &password(v(0)), SHARD_THRESHOLD, SHARD_COUNT)
                    .map_err(|_| "Wrong password.".to_string())?;
                self.step = Step::ShardList(shards.iter().map(|s| s.to_string()).collect());
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
                write_private(&path, armored.as_bytes(), false).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
                self.step = Step::Overview;
                self.notice = Some(format!("Backup saved to {}.", path.display()));
                Ok(Outcome::Stay)
            }
            Step::AddDevice => {
                let pasted = v(1).trim().to_string();
                let link = if pasted.is_empty() { self.answered.clone().unwrap_or_default() } else { pasted };
                let link = link.as_str();
                if link.is_empty() {
                    return Err("Scan this code on the new device, or paste the link it shows.".into());
                }
                if is_invite(link) {
                    return Err("That is another existing device's code: open it on the new device instead.".into());
                }
                let pairing = self
                    .links
                    .add_device_start(&home, &profile, &self.profile_name, &password(v(0)), link)
                    .map_err(|e| format!("Could not pair: {e}"))?;
                self.end_invite();
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

/// The recovery words, normalised: 24 words separated by single spaces, or
/// the words that the shards in `text` rebuild.
fn recovery_words(text: &str) -> Result<Zeroizing<String>, String> {
    let shards = find_shards(text);
    if !shards.is_empty() {
        return words_from_shards(&shards);
    }
    let words: Vec<String> = text.split_whitespace().map(str::to_lowercase).collect();
    if words.len() != 24 {
        return Err(format!("A recovery phrase has 24 words; that has {}.", words.len()));
    }
    Ok(Zeroizing::new(words.join(" ")))
}

/// The shards in `text`: every word that reads as one.
pub fn find_shards(text: &str) -> Vec<String> {
    text.split_whitespace().filter(|w| profile_identity::shard_meta(w).is_some()).map(str::to_string).collect()
}

fn words_from_shards(shards: &[String]) -> Result<Zeroizing<String>, String> {
    profile_identity::words_from_shards(shards).map_err(|e| format!("Those shards did not rebuild the identity: {e}"))
}

/// The paths in a file field: one per line or separated by `;` (a picker
/// that chose several fills it that way), `~/` expanded.
pub fn split_paths(text: &str) -> Vec<PathBuf> {
    text.split(['\n', ';']).map(str::trim).filter(|p| !p.is_empty()).map(expand_home).collect()
}

/// A file field's text for the paths a picker chose.
pub fn join_paths(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("; ")
}

/// The profile's name as a file name part (`My Stuff!` to `my-stuff`).
fn slug(profile_name: &str) -> String {
    let slug: String = profile_name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() { "profile".into() } else { slug.into() }
}

impl Identity {
    /// `secret` as clipboard text (the bare words or shard).
    pub fn secret_text(&self, secret: Secret) -> Option<Zeroizing<String>> {
        match (&self.step, secret) {
            (Step::Words(w) | Step::RevealWords(w), Secret::Words) => Some(Zeroizing::new(w.clone())),
            (Step::ShardList(shards), Secret::Shard(n)) => shards.get(n).map(|s| Zeroizing::new(s.clone())),
            _ => None,
        }
    }

    /// The file name to suggest when saving `secret`.
    pub fn file_name(&self, secret: Secret) -> String {
        let slug = slug(&self.profile_name);
        match secret {
            Secret::Words => format!("morph-recovery-words-{slug}.txt"),
            Secret::Shard(n) => format!("morph-shard-{slug}-{}-of-{}.txt", n + 1, self.shard_count()),
        }
    }

    fn shard_count(&self) -> usize {
        match &self.step {
            Step::ShardList(s) => s.len(),
            _ => 0,
        }
    }

    /// What a saved file of `secret` holds: a short note, then the secret on
    /// its own line (restore finds it there).
    pub fn file_text(&self, secret: Secret) -> Option<Zeroizing<String>> {
        let text = self.secret_text(secret)?;
        let name = &self.profile_name;
        Some(Zeroizing::new(match secret {
            Secret::Words => format!(
                "Morph recovery words for the profile \"{name}\".\nAnyone with these 24 words has the identity. Keep them offline: print this, then delete the file.\n\n{}\n",
                text.as_str()
            ),
            Secret::Shard(n) => {
                let k = profile_identity::shard_meta(&text).map(|m| m.1).unwrap_or(SHARD_THRESHOLD);
                format!(
                    "Morph identity shard {} of {} for the profile \"{name}\". Any {k} of the {} shards rebuild the identity; one alone tells nothing.\nGive each shard to a different person or place, and keep it offline.\n\n{}\n",
                    n + 1,
                    self.shard_count(),
                    self.shard_count(),
                    text.as_str()
                )
            }
        }))
    }

    /// Save `secret` to `path` (the user chose it in a Save dialog, which
    /// already asked about replacing a file).
    pub fn save_secret(&mut self, secret: Secret, path: &Path) {
        let Some(text) = self.file_text(secret) else {
            return;
        };
        self.error = None;
        match write_private(path, text.as_bytes(), true) {
            Ok(()) => self.notice = Some(format!("Saved to {}. Keep it offline; anyone with it has your identity.", path.display())),
            Err(e) => self.error = Some(format!("Could not write {}: {e}", path.display())),
        }
    }

    /// Save every shard into `dir`, one file each; refuses to replace files.
    pub fn save_all_shards(&mut self, dir: &Path) {
        self.error = None;
        let count = self.shard_count();
        if let Some(taken) = (0..count).map(|n| dir.join(self.file_name(Secret::Shard(n)))).find(|p| p.exists()) {
            self.error = Some(format!("{} already exists; choose another folder.", taken.display()));
            return;
        }
        for n in 0..count {
            let path = dir.join(self.file_name(Secret::Shard(n)));
            let Some(text) = self.file_text(Secret::Shard(n)) else {
                return;
            };
            if let Err(e) = write_private(&path, text.as_bytes(), false) {
                self.error = Some(format!("Could not write {}: {e}", path.display()));
                return;
            }
        }
        self.notice = Some(format!("Saved {count} shard files in {}. Give each to a different person or place.", dir.display()));
    }
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
    format!("~/morph-{}-identity-backup.txt", slug(profile_name))
}

/// Write a file only the user can read: a new one, or (`replace`) over an
/// old one.
fn write_private(path: &Path, bytes: &[u8], replace: bool) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    if replace {
        options.write(true).create(true).truncate(true);
    } else {
        options.write(true).create_new(true);
    }
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

    #[test]
    fn the_relay_line_shows_the_least_ready_relay() {
        use morph_identity::device_link::{Multiaddr, NetStatus, PUBLIC_RELAYS};
        let one: Vec<Multiaddr> = PUBLIC_RELAYS.iter().map(|a| a.parse().unwrap()).collect();
        let (text, state) = relay_line(&RelayReport::build(&one, &[], false, |_, _| None));
        assert_eq!(state, RelayStatus::NoNode);
        assert!(text.starts_with("Not connected"), "{text}");
        let peer = "12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB".to_string();
        let up = NetStatus { connected: vec![peer.clone()], reservations: vec![peer], ..Default::default() };
        assert_eq!(relay_line(&RelayReport::build(&one, std::slice::from_ref(&up), false, |_, _| None)).1, RelayStatus::Reserved);
        let mut two = one.clone();
        two.push("/ip4/192.0.2.7/tcp/4001/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp".parse().unwrap());
        assert_eq!(relay_line(&RelayReport::build(&two, &[up], false, |_, _| None)).1, RelayStatus::NotConnected);
        assert_eq!(relay_line(&RelayReport::build(&[], &[NetStatus::default()], false, |_, _| None)).1, RelayStatus::NoNode);
    }

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
        new.wait_started();
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

    /// The other direction: the existing device's Add a device shows a QR,
    /// the new device pastes (scans) it, and the same code check follows.
    #[test]
    fn a_new_device_can_answer_the_existing_devices_code() {
        let (_a, old_home) = home();
        let (_b, new_home) = home();
        let shared = links();
        let mut old = Identity::new(&old_home, "p1", "Work", shared);
        old.start(Step::Create);
        old.submit(&s(&["old password", "old password"]));
        old.submit(&[]);
        old.start(Step::AddDevice);
        assert!(old.is_starting(), "the node starts off the caller's thread");
        old.wait_started();
        let invite = old.invite_link().expect("Add a device shows a code").to_string();
        assert!(invite.starts_with("weirdnet-morph://invite/"));
        old.submit(&s(&["old password", ""]));
        assert!(old.error.as_deref().unwrap().contains("Scan this code"), "{:?}", old.error);

        let mut new = Identity::new(&new_home, "p1", "Work", shared);
        new.start_offer();
        new.wait_started();
        let Step::Offer { link, .. } = new.step.clone() else { panic!("{:?} {:?}", new.step, new.error) };
        new.submit(&s(&[&link]));
        assert!(new.error.as_deref().unwrap().contains("new device's link"), "{:?}", new.error);
        new.submit(&s(&[&invite]));
        assert_eq!(new.error, None);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !old.answered() && std::time::Instant::now() < deadline {
            old.poll();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(old.take_dirty() && old.answered());
        assert_eq!(old.step, Step::AddDevice, "the password field keeps its text");
        old.submit(&s(&["old password", ""]));
        let Step::Adding { sas, .. } = old.step.clone() else { panic!("{:?} {:?}", old.step, old.error) };
        assert_eq!(old.invite_link(), None, "the code is gone once paired");
        while !matches!(new.step, Step::Paired { .. }) && std::time::Instant::now() < deadline {
            new.poll();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let Step::Paired { sas: new_sas, .. } = new.step.clone() else { panic!("{:?}", new.step) };
        assert_eq!(new_sas, sas);
        new.submit(&[]);
        old.submit(&[]);
        while !matches!(new.step, Step::Received { .. }) && std::time::Instant::now() < deadline {
            new.poll();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(new.submit(&s(&["new password"])), Outcome::IdentityChanged, "{:?}", new.error);
        assert_eq!(new.identity.as_ref().unwrap().user_id, old.identity.as_ref().unwrap().user_id);
    }

    #[test]
    fn shards_save_one_file_each_and_any_three_restore() {
        let (t, home) = home();
        morph_core::profiles::create(&home, "Spare").unwrap();
        let mut id = Identity::new(&home, "p1", "Work", links());
        id.start(Step::Create);
        id.submit(&s(&["long enough", "long enough"]));
        let Step::Words(words) = id.step.clone() else { panic!() };
        assert_eq!(id.secret_text(Secret::Words).unwrap().as_str(), words);
        assert_eq!(id.file_name(Secret::Words), "morph-recovery-words-work.txt");
        let words_file = t.path().join("words.txt");
        id.save_secret(Secret::Words, &words_file);
        let saved = std::fs::read_to_string(&words_file).unwrap();
        assert!(saved.contains(&words) && saved.contains("offline"), "{saved}");
        id.submit(&[]);

        id.start(Step::Shards);
        id.submit(&s(&["wrong password"]));
        assert_eq!(id.error.as_deref(), Some("Wrong password."));
        id.submit(&s(&["long enough"]));
        let Step::ShardList(shards) = id.step.clone() else { panic!("{:?}", id.error) };
        assert_eq!(shards.len(), SHARD_COUNT);
        assert_eq!(id.file_name(Secret::Shard(1)), "morph-shard-work-2-of-5.txt");
        let dir = t.path().join("shards");
        std::fs::create_dir(&dir).unwrap();
        id.save_all_shards(&dir);
        assert!(id.error.is_none(), "{:?}", id.error);
        let files: Vec<PathBuf> = (1..=5).map(|n| dir.join(format!("morph-shard-work-{n}-of-5.txt"))).collect();
        let second = std::fs::read_to_string(&files[1]).unwrap();
        assert!(second.contains("shard 2 of 5") && second.contains("Any 3 of the 5") && second.contains(&shards[1]), "{second}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&files[0]).unwrap().permissions().mode() & 0o777, 0o600);
        }
        id.save_all_shards(&dir);
        assert!(id.error.as_deref().unwrap().contains("already exists"), "never replaces shard files");

        // Two shards are not enough; three files restore another profile.
        let mut spare = Identity::new(&home, "p2", "Spare", links());
        spare.start(Step::Restore);
        let two = join_paths(&files[..2]);
        assert_eq!(spare.submit(&s(&[&two, "", "spare pass"])), Outcome::Stay);
        assert!(spare.error.as_deref().unwrap().contains("did not rebuild"), "{:?}", spare.error);
        let three = join_paths(&[files[4].clone(), files[0].clone(), files[2].clone()]);
        assert_eq!(spare.submit(&s(&[&three, "", "spare pass"])), Outcome::IdentityChanged, "{:?}", spare.error);
        assert_eq!(spare.identity.as_ref().unwrap().user_id, id.identity.as_ref().unwrap().user_id);

        // Pasted shards in the words field work too.
        let mut default = Identity::new(&home, "default", "Default", links());
        default.start(Step::Import);
        let pasted = format!("{}\n{}\n{}", shards[1], shards[3], shards[2]);
        assert_eq!(default.submit(&s(&[&pasted, "other pass"])), Outcome::IdentityChanged, "{:?}", default.error);
        assert_eq!(default.identity.as_ref().unwrap().user_id, id.identity.as_ref().unwrap().user_id);
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
