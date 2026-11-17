//! Moving a profile identity between devices over a QR link. Sessions live
//! here and are named by an id, so a shell only holds the id, the link text,
//! the six-digit code and public identity facts; seeds, keys and passwords
//! stay in Rust.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Instant;

pub use morph_identity::device_link::LinkState;
pub use morph_identity::device_link::is_invite;
use morph_identity::device_link::{AddDevice, Invite, NetStatus, NewDevice, RelayProbe};
use morph_identity::profile_identity::{self, ProfileIdentity, SecretString};

use crate::home::{now_ms, profile_dir};
use crate::{Error, Result};

/// A new device's offer: show `link` as a QR code and as text.
#[derive(Debug, Clone)]
pub struct Offer {
    pub session: u64,
    pub link: String,
    pub expires_ms: i64,
}

/// An existing device paired with a scanned link: compare `sas`, then send.
#[derive(Debug, Clone)]
pub struct Pairing {
    pub session: u64,
    pub sas: String,
    pub device: String,
}

/// Open link sessions, both sides. One per process is enough
/// ([`Links::global`]); tests may make their own.
#[derive(Default)]
pub struct Links {
    next: AtomicU64,
    hosts: Mutex<HashMap<u64, NewDevice>>,
    pairings: Mutex<HashMap<u64, AddDevice>>,
    pairing_ctx: Mutex<HashMap<u64, (std::path::PathBuf, String)>>,
    invites: Mutex<HashMap<u64, Invite>>,
    /// A running relay check and when it stops ([`crate::relays`]).
    probe: Mutex<Option<(RelayProbe, Instant)>>,
}

/// An existing device's invite: show `link` as a QR code and as text.
#[derive(Debug, Clone)]
pub struct InviteView {
    pub session: u64,
    pub link: String,
    pub expires_ms: i64,
}

/// Drop a link node on its own thread: stopping one joins its network
/// thread, which a caller on the UI thread should not wait for.
fn drop_later<T: Send + 'static>(x: T) {
    if std::thread::Builder::new().name("morph-link-stop".into()).spawn(move || drop(x)).is_err() {
        // could not spawn: dropped here instead
    }
}

fn guard<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Links {
    /// The process-wide sessions.
    pub fn global() -> &'static Links {
        static LINKS: OnceLock<Links> = OnceLock::new();
        LINKS.get_or_init(Links::default)
    }

    pub(crate) fn probe_slot(&self) -> MutexGuard<'_, Option<(RelayProbe, Instant)>> {
        guard(&self.probe)
    }

    /// The network status of every running link node, both sides.
    pub(crate) fn net_statuses(&self) -> Vec<NetStatus> {
        let mut v: Vec<NetStatus> = guard(&self.hosts).values().map(NewDevice::net_status).collect();
        v.extend(guard(&self.pairings).values().map(AddDevice::net_status));
        v.extend(guard(&self.invites).values().map(Invite::net_status));
        v
    }

    fn next_id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// New device: start listening and make a fresh offer. `device_name` is
    /// shown on the other device.
    pub fn new_device_start(&self, device_name: &str) -> Result<Offer> {
        let device = NewDevice::start(device_name, crate::relays::link_config()).map_err(Error::failed)?;
        let offer = Offer { session: self.next_id(), link: device.link().to_string(), expires_ms: device.expires_ms() };
        guard(&self.hosts).insert(offer.session, device);
        Ok(offer)
    }

    /// New device: where session `session` stands.
    pub fn new_device_status(&self, session: u64) -> Result<LinkState> {
        guard(&self.hosts).get(&session).map(NewDevice::state).ok_or(Error::LinkGone)
    }

    /// New device: the user confirmed that the codes match.
    pub fn new_device_confirm(&self, session: u64) -> Result<()> {
        guard(&self.hosts).get(&session).ok_or(Error::LinkGone)?.confirm();
        Ok(())
    }

    /// New device: store the identity that arrived in `profile_id`, sealed
    /// under `password`, and encrypt the profile's library. Ends the session
    /// on success.
    pub fn new_device_finish(&self, home: &Path, profile_id: &str, session: u64, password: &SecretString) -> Result<ProfileIdentity> {
        let dir = profile_dir(home, profile_id)?;
        let linked = {
            let mut hosts = guard(&self.hosts);
            let linked = hosts.get(&session).ok_or(Error::LinkGone)?.finish(&dir, password, now_ms()).map_err(Error::failed)?;
            hosts.remove(&session);
            linked
        };
        let identity = linked.identity;
        match profile_identity::library_key(&dir, password) {
            Ok(key) => crate::library::adopt_new_identity(home, profile_id, &identity.user_id, &key),
            Err(e) => eprintln!("Morph: library key for linked profile {profile_id}: {e}"),
        }
        let _ = crate::sync::on_password(home, profile_id, password);
        let _ = crate::sync::apply_link_snapshot(home, profile_id, &linked.sync, &linked.settings);
        Ok(identity)
    }

    /// New device: stop listening (screen closed, or the codes differ).
    pub fn new_device_cancel(&self, session: u64) {
        let device = guard(&self.hosts).remove(&session);
        if let Some(device) = device {
            device.cancel();
            drop_later(device);
        }
    }

    /// Existing device: check `profile_id`'s password, read `link` and pair
    /// with the new device.
    pub fn add_device_start(&self, home: &Path, profile_id: &str, profile_name: &str, password: &SecretString, link: &str) -> Result<Pairing> {
        let dir = profile_dir(home, profile_id)?;
        let pairing = AddDevice::start(&dir, profile_name, password, link, crate::relays::link_config()).map_err(Error::failed)?;
        let view = Pairing { session: self.next_id(), sas: pairing.sas().to_string(), device: pairing.device().to_string() };
        guard(&self.pairings).insert(view.session, pairing);
        guard(&self.pairing_ctx).insert(view.session, (home.to_path_buf(), profile_id.to_string()));
        Ok(view)
    }

    /// Existing device: the codes match; send the identity and end the session.
    pub fn add_device_send(&self, session: u64) -> Result<()> {
        let mut pairing = guard(&self.pairings).remove(&session).ok_or(Error::LinkGone)?;
        let ctx = guard(&self.pairing_ctx).remove(&session);
        if let Some((home, profile_id)) = ctx {
            let (settings, sync) = crate::sync::link_payload(&home, &profile_id).unwrap_or_else(|_| ("{}".into(), vec!["bookmarks".into()]));
            let cert = pairing.send_with(&settings, &sync).map_err(Error::failed)?;
            let _ = crate::sync::adopt_peer_cert(&home, &profile_id, cert);
            Ok(())
        } else {
            pairing.send().map_err(Error::failed)
        }
    }

    /// Existing device: the codes differ, or the user gave up.
    pub fn add_device_cancel(&self, session: u64) {
        guard(&self.pairing_ctx).remove(&session);
        let pairing = guard(&self.pairings).remove(&session);
        if let Some(pairing) = pairing {
            drop_later(move || {
                let _ = pairing.cancel();
            });
        }
    }

    /// Existing device: listen for a new device that scans this device's QR.
    pub fn invite_start(&self) -> Result<InviteView> {
        let invite = Invite::start(crate::relays::link_config()).map_err(Error::failed)?;
        let view = InviteView { session: self.next_id(), link: invite.link().to_string(), expires_ms: invite.expires_ms() };
        guard(&self.invites).insert(view.session, invite);
        Ok(view)
    }

    /// Existing device: the offer link a new device answered the invite
    /// with, once; `Ok(None)` while it waits. An expired invite is gone.
    pub fn invite_take(&self, session: u64) -> Result<Option<String>> {
        let mut invites = guard(&self.invites);
        let invite = invites.get(&session).ok_or(Error::LinkGone)?;
        if let Some(link) = invite.take_offer() {
            return Ok(Some(link));
        }
        if invite.expired() {
            if let Some(old) = invites.remove(&session) {
                drop_later(old);
            }
            return Err(Error::LinkGone);
        }
        Ok(None)
    }

    /// Existing device: stop listening for answers.
    pub fn invite_cancel(&self, session: u64) {
        let invite = guard(&self.invites).remove(&session);
        if let Some(invite) = invite {
            drop_later(invite);
        }
    }

    /// New device: answer the existing device's `invite` with this
    /// session's offer; pairing then goes on as if it had been scanned.
    pub fn new_device_answer(&self, session: u64, invite: &str) -> Result<()> {
        let link = guard(&self.hosts).get(&session).ok_or(Error::LinkGone)?.link().to_string();
        morph_identity::device_link::answer_invite(invite, &link, crate::relays::link_config()).map_err(Error::failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use morph_core::library_db::LibraryState;
    use std::time::{Duration, Instant};

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    fn home_with_work() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::create(&home, "Work").unwrap();
        (tmp, home)
    }

    fn wait(links: &Links, session: u64, done: impl Fn(&LinkState) -> bool) -> LinkState {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let s = links.new_device_status(session).unwrap();
            if done(&s) || Instant::now() > deadline {
                return s;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Both sides in one process over real TCP: the new home's "Work"
    /// profile ends up with the old home's identity and an encrypted library.
    #[test]
    fn a_link_moves_an_identity_and_encrypts_the_library() {
        let links = Links::default();
        let (_a, old) = home_with_work();
        let (_b, new) = home_with_work();
        let made = crate::identity::create(&old, "p1", &pw("old password")).unwrap();

        let offer = links.new_device_start("Morph test").unwrap();
        assert!(offer.link.starts_with("weirdnet-morph://link/"));
        assert_eq!(links.new_device_status(offer.session).unwrap(), LinkState::Waiting);
        assert!(links.add_device_start(&old, "p1", "Work", &pw("wrong password"), &offer.link).is_err());
        let pairing = links.add_device_start(&old, "p1", "Work", &pw("old password"), &offer.link).unwrap();
        assert_eq!(pairing.device, "Morph test");
        match wait(&links, offer.session, |s| matches!(s, LinkState::Paired { .. })) {
            LinkState::Paired { sas } => assert_eq!(sas, pairing.sas),
            other => panic!("{other:?}"),
        }
        links.new_device_confirm(offer.session).unwrap();
        links.add_device_send(pairing.session).unwrap();
        assert!(matches!(links.add_device_send(pairing.session), Err(Error::LinkGone)), "single use");
        assert!(matches!(wait(&links, offer.session, |s| matches!(s, LinkState::Received { .. })), LinkState::Received { .. }));

        let linked = links.new_device_finish(&new, "p1", offer.session, &pw("new password")).unwrap();
        assert_eq!(linked.user_id, made.identity.user_id);
        assert!(matches!(links.new_device_status(offer.session), Err(Error::LinkGone)), "session ended");
        assert_eq!(crate::library::state(&new, "p1").unwrap(), LibraryState::Unlocked);
        assert_eq!(*crate::identity::recovery_words(&new, "p1", &pw("new password")).unwrap(), *made.words);
    }

    /// The other direction over real TCP: the existing device shows an
    /// invite, the new device answers it, and the existing device pairs with
    /// the offer it got; the identity moves after the same code check.
    #[test]
    fn a_new_device_that_answers_an_invite_gets_the_identity() {
        let links = Links::default();
        let (_a, old) = home_with_work();
        let (_b, new) = home_with_work();
        let made = crate::identity::create(&old, "p1", &pw("old password")).unwrap();

        let invite = links.invite_start().unwrap();
        assert!(invite.link.starts_with("weirdnet-morph://invite/"));
        assert!(is_invite(&invite.link));
        assert_eq!(links.invite_take(invite.session).unwrap(), None);
        let offer = links.new_device_start("Morph test").unwrap();
        links.new_device_answer(offer.session, &invite.link).unwrap();
        let answered = links.invite_take(invite.session).unwrap().expect("the new device's offer");
        assert_eq!(answered, offer.link);
        assert_eq!(links.invite_take(invite.session).unwrap(), None, "taken once");
        let pairing = links.add_device_start(&old, "p1", "Work", &pw("old password"), &answered).unwrap();
        match wait(&links, offer.session, |s| matches!(s, LinkState::Paired { .. })) {
            LinkState::Paired { sas } => assert_eq!(sas, pairing.sas),
            other => panic!("{other:?}"),
        }
        links.new_device_confirm(offer.session).unwrap();
        links.add_device_send(pairing.session).unwrap();
        wait(&links, offer.session, |s| matches!(s, LinkState::Received { .. }));
        let linked = links.new_device_finish(&new, "p1", offer.session, &pw("new password")).unwrap();
        assert_eq!(linked.user_id, made.identity.user_id);
        links.invite_cancel(invite.session);
        assert!(matches!(links.invite_take(invite.session), Err(Error::LinkGone)));
    }

    #[test]
    fn cancelling_ends_a_session_and_unknown_ids_are_gone() {
        let links = Links::default();
        let offer = links.new_device_start("Morph test").unwrap();
        links.new_device_cancel(offer.session);
        assert!(matches!(links.new_device_status(offer.session), Err(Error::LinkGone)));
        assert!(matches!(links.new_device_confirm(999), Err(Error::LinkGone)));
        links.add_device_cancel(12345);
        assert!(std::ptr::eq(Links::global(), Links::global()));
    }
}
