//! Settings → Linked devices on the phone: the profile's devices, their key
//! state after a rotation, the warning before a revoke, and revoking one.
//! The logic is `morph_services::sync`, shared with the desktop.
use morph_services::sync::{self, DeviceView};

use crate::home::{now_ms, resolve};

/// One linked device as the Linked devices list shows it.
pub struct LinkedDeviceView {
    pub peer_id: String,
    pub label: String,
    /// When it was linked (ms since the epoch).
    pub issued_ms: i64,
    pub revoked_ms: Option<i64>,
    pub this_device: bool,
    /// "This device", "Linked" or "Revoked".
    pub state: String,
    /// "Needs re-admission from another device" (this device, restored from
    /// the recovery words after keys rotated) or "Waiting for its first sync"
    /// (another device, linked here after keys rotated); else none.
    pub note: Option<String>,
    /// It has published its device key (it has synced since updating Morph).
    pub has_key: bool,
}

impl From<&DeviceView> for LinkedDeviceView {
    fn from(d: &DeviceView) -> Self {
        LinkedDeviceView {
            peer_id: d.peer_id.clone(),
            label: d.label.clone(),
            issued_ms: d.issued_ms,
            revoked_ms: d.revoked_ms,
            this_device: d.this_device,
            state: d.state_label().into(),
            note: d.key_note().map(Into::into),
            has_key: d.has_key,
        }
    }
}

/// The profile's linked devices, this one first. Empty when the profile does
/// not sync yet (no identity, or the library is locked).
pub fn linked_devices(home: Option<String>, profile_id: String) -> Result<Vec<LinkedDeviceView>, String> {
    let home = resolve(home)?;
    let list = sync::devices(&home, &profile_id).map_err(|e| e.to_string())?;
    Ok(list.iter().map(LinkedDeviceView::from).collect())
}

/// What to say before revoking `peer_id`: the other devices that have not
/// synced since updating Morph would be left out of the new key. None when
/// there are none.
pub fn linked_device_revoke_warning(home: Option<String>, profile_id: String, peer_id: String) -> Result<Option<String>, String> {
    let home = resolve(home)?;
    let list = sync::devices(&home, &profile_id).map_err(|e| e.to_string())?;
    Ok(sync::revoke_warning(&list, &peer_id))
}

/// Revoke `peer_id`: it stops syncing with this profile's devices and the
/// sync key rotates, so it cannot read anything synced afterwards. Returns
/// the list after the revoke.
pub fn revoke_linked_device(home: Option<String>, profile_id: String, peer_id: String) -> Result<Vec<LinkedDeviceView>, String> {
    let h = resolve(home.clone())?;
    sync::revoke_device(&h, &profile_id, &peer_id, now_ms()).map_err(|e| e.to_string())?;
    linked_devices(home, profile_id)
}

/// One record type in Settings → Sync.
pub struct SyncTypeView {
    pub id: String,
    pub label: String,
    pub help: String,
    pub on: bool,
}

impl From<&sync::SyncTypeView> for SyncTypeView {
    fn from(t: &sync::SyncTypeView) -> Self {
        SyncTypeView { id: t.id.clone(), label: t.label.clone(), help: t.help.clone(), on: t.on }
    }
}

/// What this device sends and receives for the profile. Empty when the
/// library is not open (no identity). A locked profile returns an error.
pub fn sync_types(home: Option<String>, profile_id: String) -> Result<Vec<SyncTypeView>, String> {
    let home = resolve(home)?;
    let list = sync::sync_types(&home, &profile_id).map_err(|e| e.to_string())?;
    Ok(list.iter().map(SyncTypeView::from).collect())
}

/// Turn one type on or off for this profile on this device. Does not delete
/// data already here. Turning a type on catches this device up.
pub fn set_sync_type(home: Option<String>, profile_id: String, typ: String, on: bool) -> Result<Vec<SyncTypeView>, String> {
    let h = resolve(home.clone())?;
    let list = sync::set_sync_type(&h, &profile_id, &typ, on).map_err(|e| e.to_string())?;
    Ok(list.iter().map(SyncTypeView::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(this: bool, revoked: Option<i64>, needs: bool, waiting: bool) -> DeviceView {
        DeviceView {
            peer_id: "p".into(),
            label: "Phone".into(),
            issued_ms: 1,
            revoked_ms: revoked,
            this_device: this,
            needs_admission: needs,
            awaiting_first_sync: waiting,
            has_key: true,
        }
    }

    #[test]
    fn views_carry_state_and_key_notes() {
        let v = LinkedDeviceView::from(&d(true, None, true, false));
        assert_eq!((v.state.as_str(), v.note.as_deref()), ("This device", Some("Needs re-admission from another device")));
        let v = LinkedDeviceView::from(&d(false, None, false, true));
        assert_eq!((v.state.as_str(), v.note.as_deref()), ("Linked", Some("Waiting for its first sync")));
        let v = LinkedDeviceView::from(&d(false, Some(5), false, true));
        assert_eq!((v.state.as_str(), v.note.as_deref()), ("Revoked", None));
    }

    #[test]
    fn a_profile_without_sync_lists_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let home = Some(dir.path().to_string_lossy().into_owned());
        assert!(linked_devices(home.clone(), "default".into()).map(|l| l.is_empty()).unwrap_or(true));
        assert!(revoke_linked_device(home.clone(), "default".into(), "nobody".into()).is_err());
        assert!(sync_types(home.clone(), "default".into()).map(|l| l.is_empty()).unwrap_or(true));
        assert!(set_sync_type(home, "default".into(), "history".into(), false).is_err());
    }
}
