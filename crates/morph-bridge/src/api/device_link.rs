//! Login over a QR code (device linking). Sessions live in Rust and are named
//! by an id; the seed, keys and passwords never cross to Dart, only the link
//! text, the six-digit code and public identity facts.
use morph_identity::profile_identity::SecretString;
use morph_services::device_link::{LinkState, Links};

use super::identity::ProfileIdentityView;
use crate::home::resolve as resolve_home;

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

fn links() -> &'static Links {
    Links::global()
}

/// New device: start listening and make a fresh offer. `device_name` is shown
/// on the other device.
pub fn link_new_device_start(device_name: String) -> Result<LinkOfferView, String> {
    let offer = links().new_device_start(&device_name).map_err(|e| e.to_string())?;
    Ok(LinkOfferView { session: offer.session, link: offer.link, expires_ms: offer.expires_ms })
}

pub fn link_new_device_status(session: u64) -> Result<LinkStatusView, String> {
    let (state, sas, reason) = match links().new_device_status(session).map_err(|e| e.to_string())? {
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
    links().new_device_confirm(session).map_err(|e| e.to_string())
}

/// New device: store the identity that arrived in `profile_id`, sealed under
/// `password`. Ends the session on success.
pub fn link_new_device_finish(home: Option<String>, profile_id: String, session: u64, password: String) -> Result<ProfileIdentityView, String> {
    let home = resolve_home(home)?;
    links().new_device_finish(&home, &profile_id, session, &SecretString::from(password)).map(ProfileIdentityView::from).map_err(|e| e.to_string())
}

/// New device: stop listening (screen closed, or the codes differ).
pub fn link_new_device_cancel(session: u64) {
    links().new_device_cancel(session);
}

/// New device: answer the existing device's invite (`weirdnet-morph://invite/…`,
/// scanned or pasted) with this session's offer. The existing device then
/// pairs as if it had scanned this device's code; poll
/// [`link_new_device_status`] as usual.
pub fn link_new_device_answer(session: u64, invite: String) -> Result<(), String> {
    links().new_device_answer(session, &invite).map_err(|e| e.to_string())
}

/// Whether `text` is an existing device's invite rather than a new device's
/// link.
#[flutter_rust_bridge::frb(sync)]
pub fn link_is_invite(text: String) -> bool {
    morph_services::device_link::is_invite(&text)
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
    let home = resolve_home(home)?;
    let pairing = links().add_device_start(&home, &profile_id, &profile_name, &SecretString::from(password), &link).map_err(|e| e.to_string())?;
    Ok(LinkPairingView { session: pairing.session, sas: pairing.sas, device: pairing.device })
}

/// Existing device: the codes match; send the identity and end the session.
pub fn link_add_device_send(session: u64) -> Result<(), String> {
    links().add_device_send(session).map_err(|e| e.to_string())
}

/// Existing device: the codes differ, or the user gave up.
pub fn link_add_device_cancel(session: u64) {
    links().add_device_cancel(session);
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

    /// A phone (new device) answers a desktop's invite: the desktop gets
    /// the phone's offer link back and pairs with it.
    #[test]
    fn a_new_device_answers_an_invite() {
        let invite = links().invite_start().unwrap();
        assert!(link_is_invite(invite.link.clone()));
        let offer = link_new_device_start("Morph test".into()).unwrap();
        assert!(!link_is_invite(offer.link.clone()));
        assert!(link_new_device_answer(offer.session, "weirdnet-morph://link/nope".into()).is_err());
        link_new_device_answer(offer.session, invite.link.clone()).unwrap();
        assert_eq!(links().invite_take(invite.session).unwrap().as_deref(), Some(offer.link.as_str()));
        links().invite_cancel(invite.session);
        link_new_device_cancel(offer.session);
    }

    #[test]
    fn cancelling_ends_a_new_device_session() {
        let offer = link_new_device_start("Morph test".into()).unwrap();
        link_new_device_cancel(offer.session);
        assert!(link_new_device_status(offer.session).is_err());
        link_add_device_cancel(12345);
    }
}
