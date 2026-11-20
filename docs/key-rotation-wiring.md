# Key rotation: Morph wiring plan

Status (2026-11-18): done. Morph pins weirdnet-sync `=0.3.1` (0.3.0 plus two
rotation fixes: two first roots made at once merge into one chain, and revoking the
chain's creator keeps the chain); steps 1–8 are wired, step 6 on desktop and phone
(`morph-bridge` `api::devices`, `app/lib/src/widgets/linked_devices.dart`). m3talsmith's answers: rotate automatically after the first link
and on upgrade (wired), leave revoke-by-a-revoked-device for later, and warn before a
revoke that leaves devices without a key out (wired). Current behavior is described in
`docs/library-sync.md`.

## What the crate does

- `SyncState::revoke(user, peer, now)` revokes and rotates: a random epoch key, sealed
  to each remaining device's X25519 device key (derived from its node key). The
  sealed copies travel in `Roster::chain`. A revoked device holds the seed but not the key.
- Pulls are served only under the head key. Older and seed keys only get the roster back,
  so a device that missed a rotation catches up in the same `pull_from`.
- A device restored from the recovery words (new node key) gets
  `Refused("needs-admission")` and `key_status() == NeedsAdmission` until a member
  admits it. `SyncState::approve(cert)` admits it as soon as its first hello brings its
  device key, and the reply to that hello already carries the admission.
- Epoch keys (`SyncState::keys`, `EpochKeys`) are secret; the chain is public and
  rides in the roster JSON Morph already stores.

## Steps

1. **State**: `build_state` sets `device: Device::from_node_key(&node_key)` and
   `keys: load_epoch_keys(lib)`, then `publish_device_key` and `refresh_keys`.
   `tick` pulls into `SyncState::snapshot` and folds it back with weirdnet-sync's
   `SyncState::merge_back` (0.5.1), which merges the roster, then the keys (and
   `refresh_keys`). Without that, a rotation made in the pull copy would leave the
   live state without its own key.
2. **Storage**: `META_EPOCH_KEYS` in `library.db` (encrypted): `EpochKeys::export` /
   `import`. Save it next to the roster after every tick, revoke and approve.
3. **refresh_live**: merge the stored roster with the live one, and merge the stored keys,
   rather than replacing them. Otherwise an admission the live handler made during a
   hello is lost.
4. **Revoke** (Settings → Linked devices): `revoke_device` builds the state offline
   (seed, node key, roster, keys) and calls `SyncState::revoke`, which rotates. It then saves
   the roster and keys, and calls `refresh_live`.
5. **Linking = admission**: `adopt_peer_cert` calls `SyncState::approve(cert)`, so a
   device linked (or re-linked after a restore) after a rotation gets the key on its
   first sync. No change to the link protocol is needed.
6. **UI**: `DeviceView.needs_admission`. On this device's row it means "Needs
   re-admission from another device: open Settings → Identity → Link a device on one of
   your other devices". On another device's row it means "Waiting for its first sync".
   The same state goes in the mobile Linked devices list (bridge + Flutter) and the
   identity-restore success screen. Optionally show `DeviceKey::fingerprint()` on both
   devices while linking.
7. **Docs**: update `docs/library-sync.md` (threat table: "A revoked device that kept an
   old SyncKey" now reads nothing written after the rotation) and the changelog.
8. **Tests**: the two-home relay test checks that A holds an epoch key and B none after
   the revoke. Add a three-home test: revoke C, B catches up from A, and a restored C' is
   re-linked and catches up.

## Open questions for m3talsmith

- Rotate once right after the first link (and on upgrade, when every device is still
  trusted)? Until the first rotation, every seed holder is equal. A revoked seed holder could
  offer a rival first rotation to a device that has not synced the real one yet.
- Every device can sign with the user key, so a revoked device can still revoke other
  devices (denial of service). It still can't read anything. Fixing that needs a user-key
  rotation or a separate admin key.
- A device that has never synced since upgrading (so it has no published device key) is
  left out of the first rotation, and then needs re-admission. Should we warn about this
  before revoking?
