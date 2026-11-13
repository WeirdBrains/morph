//! Login over a QR code (device linking). Sessions live in Rust and are named
//! by an id; the seed, keys and passwords never cross to Dart, only the link
//! text, the six-digit code and public identity facts.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use morph_identity::device_link::{AddDevice, LinkState, NewDevice, net_config};
use morph_identity::profile_identity::SecretString;

use super::identity::{ProfileIdentityView, now_ms, profile_dir};

/// A new device's offer: show [`link`](Self::link) as a QR code and as text.
pub struct LinkOfferView {
    pub session: u64,
    pub link: String,
    pub expires_ms: i64,
}

/// Where a new device's link stands. `state` is one of `waiting`, `paired`,
/// `received`, `done`, `cancelled`, `expired`, `failed`; `sas` is set while
/// paired or received; `reason` when failed.
pub struct LinkStatusView {
    pub state: String,
    pub sas: Option<String>,
    pub reason: Option<String>,
}

/// An existing device paired with a scanned link: compare `sas`, then send.
pub struct LinkPairingView {
    pub session: u64,
    pub sas: String,
    pub device: String,
}

static NEXT: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn hosts() -> MutexGuard<'static, HashMap<u64, NewDevice>> {
    static HOSTS: OnceLock<Mutex<HashMap<u64, NewDevice>>> = OnceLock::new();
    HOSTS.get_or_init(Default::default).lock().unwrap_or_else(|p| p.into_inner())
}

fn pairings() -> MutexGuard<'static, HashMap<u64, AddDevice>> {
    static PAIRINGS: OnceLock<Mutex<HashMap<u64, AddDevice>>> = OnceLock::new();
    PAIRINGS.get_or_init(Default::default).lock().unwrap_or_else(|p| p.into_inner())
}

const GONE: &str = "this link has ended; start again";

/// New device: start listening and make a fresh offer. `device_name` is shown
/// on the other device.
pub fn link_new_device_start(device_name: String) -> Result<LinkOfferView, String> {
    let device = NewDevice::start(&device_name, net_config()).map_err(|e| e.to_string())?;
    let view = LinkOfferView { session: next_id(), link: device.link().to_string(), expires_ms: device.expires_ms() };
    hosts().insert(view.session, device);
    Ok(view)
}

pub fn link_new_device_status(session: u64) -> Result<LinkStatusView, String> {
    let hosts = hosts();
    let device = hosts.get(&session).ok_or(GONE)?;
    let (state, sas, reason) = match device.state() {
        LinkState::Waiting => ("waiting", None, None),
        LinkState::Paired { sas } => ("paired", Some(sas), None),
        LinkState::Received { sas } => ("received", Some(sas), None),
        LinkState::Done => ("done", None, None),
        LinkState::Cancelled => ("cancelled", None, None),
        LinkState::Expired => ("expired", None, None),
        LinkState::Failed(why) => ("failed", None, Some(why)),
    };
    Ok(LinkStatusView { state: state.into(), sas, reason })
}

/// New device: the user tapped "Codes match".
pub fn link_new_device_confirm(session: u64) -> Result<(), String> {
    hosts().get(&session).ok_or(GONE)?.confirm();
    Ok(())
}

/// New device: store the identity that arrived in `profile_id`, sealed under
/// `password`. Ends the session on success.
pub fn link_new_device_finish(home: Option<String>, profile_id: String, session: u64, password: String) -> Result<ProfileIdentityView, String> {
    let dir = profile_dir(home.clone(), &profile_id)?;
    let password = SecretString::from(password);
    let mut hosts = hosts();
    let identity = hosts.get(&session).ok_or(GONE)?.finish(&dir, &password, now_ms()).map_err(|e| e.to_string())?;
    hosts.remove(&session);
    drop(hosts);
    match morph_identity::profile_identity::library_key(&dir, &password) {
        Ok(key) => crate::api::library::adopt_new_identity(&crate::api::identity::resolve_home(home)?, &profile_id, &dir, &identity.user_id, &key),
        Err(e) => eprintln!("Morph: library key for linked profile {profile_id}: {e}"),
    }
    Ok(ProfileIdentityView::from(identity))
}

/// New device: stop listening (screen closed, or the codes differ).
pub fn link_new_device_cancel(session: u64) {
    if let Some(device) = hosts().remove(&session) {
        device.cancel();
    }
}

/// Existing device: check `profile_id`'s password, read `link` and pair with
/// the new device.
pub fn link_add_device_start(
    home: Option<String>,
    profile_id: String,
    profile_name: String,
    password: String,
    link: String,
) -> Result<LinkPairingView, String> {
    let dir = profile_dir(home, &profile_id)?;
    let password = SecretString::from(password);
    let pairing = AddDevice::start(&dir, &profile_name, &password, &link, net_config()).map_err(|e| e.to_string())?;
    let view = LinkPairingView { session: next_id(), sas: pairing.sas().to_string(), device: pairing.device().to_string() };
    pairings().insert(view.session, pairing);
    Ok(view)
}

/// Existing device: the codes match; send the identity and end the session.
pub fn link_add_device_send(session: u64) -> Result<(), String> {
    let mut pairing = pairings().remove(&session).ok_or(GONE)?;
    pairing.send().map_err(|e| e.to_string())
}

/// Existing device: the codes differ, or the user gave up.
pub fn link_add_device_cancel(session: u64) {
    if let Some(pairing) = pairings().remove(&session) {
        let _ = pairing.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn home_with_profiles() -> (tempfile::TempDir, String, String) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().display().to_string();
        let work = super::super::profiles::create_profile(Some(home.clone()), "Work".into()).unwrap().current;
        (tmp, home, work)
    }

    fn wait(session: u64, state: &str) -> LinkStatusView {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let s = link_new_device_status(session).unwrap();
            if s.state == state || Instant::now() > deadline {
                return s;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Both sides in one process, over real TCP: the new device's "Work"
    /// profile ends up with the old device's identity.
    #[test]
    fn link_moves_an_identity_between_homes() {
        let (_a, old_home, old_work) = home_with_profiles();
        let (_b, new_home, new_work) = home_with_profiles();
        let made = super::super::identity::create_profile_identity(Some(old_home.clone()), old_work.clone(), "old password".into()).unwrap();

        let offer = link_new_device_start("Morph test".into()).unwrap();
        assert!(offer.link.starts_with("weirdnet-morph://link/"));
        assert_eq!(link_new_device_status(offer.session).unwrap().state, "waiting");
        assert!(link_add_device_start(Some(old_home.clone()), old_work.clone(), "Work".into(), "wrong password".into(), offer.link.clone()).is_err());

        let pairing = link_add_device_start(Some(old_home), old_work, "Work".into(), "old password".into(), offer.link.clone()).unwrap();
        assert_eq!(pairing.device, "Morph test");
        let status = wait(offer.session, "paired");
        assert_eq!(status.sas.as_deref(), Some(pairing.sas.as_str()));

        link_new_device_confirm(offer.session).unwrap();
        link_add_device_send(pairing.session).unwrap();
        assert!(link_add_device_send(pairing.session).is_err(), "single use");
        assert_eq!(wait(offer.session, "received").state, "received");
        let linked = link_new_device_finish(Some(new_home.clone()), new_work.clone(), offer.session, "new password".into()).unwrap();
        assert!(linked.linked && !linked.imported);
        assert_eq!(linked.user_id, made.identity.user_id);
        assert!(link_new_device_status(offer.session).is_err(), "session ended");
        let words = super::super::identity::profile_recovery_words(Some(new_home), new_work, "new password".into()).unwrap();
        assert_eq!(words, made.words);
    }

    #[test]
    fn cancelling_ends_a_new_device_session() {
        let offer = link_new_device_start("Morph test".into()).unwrap();
        link_new_device_cancel(offer.session);
        assert!(link_new_device_status(offer.session).is_err());
        link_add_device_cancel(12345);
    }
}
