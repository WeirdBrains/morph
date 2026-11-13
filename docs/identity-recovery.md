# Identity backup, recovery, device linking and sync

Status: design, 2026-11-12. Section 4 (login over QR) is built; see
`qr-login.md`. The rest is not built yet. Decisions are recorded
in ARCHITECTURE.md (section 5, "Identity recovery and sync").

Every Morph profile has its own Weirdnet identity: a 32-byte seed, shown as
24 words, from which the Ed25519 user key is derived. It is sealed in
`profiles/<id>/identity/user.key` with the profile password (Argon2id,
XChaCha20-Poly1305). This document adds five things on top, reusing Muninn's
designs and the same Weirdnet keys, so a Morph profile and a Muninn install
can hold the same identity:

1. Backups of a profile.
2. Restore during onboarding.
3. Dark shards (the seed split among trusted peers).
4. Login over a QR code (move the identity to another device).
5. Sync of profile data between your own devices.

## Where the code goes

Per AGENTS.md, logic that Muninn already has or will need moves into Weirdnet
crates; Morph keeps only wiring, UI and its own data types.

| Crate | Contents | Source |
|---|---|---|
| `weirdnet-keys` (exists) | seed, words, Shamir, `password_seal`, HKDF, X25519 | already used |
| `weirdnet-backup` (new) | archive format, manifest, BLAKE3 checks, age/password encryption, restore planner | Muninn `backup` |
| `weirdnet-shards` (new) | dark shard sealing, claim ids, entrust/restore protocol messages, holder store | Muninn `net shard` |
| `weirdnet-device-link` (new) | QR offer, ephemeral key agreement, SAS, sealed transfer of the identity, node certs for the new device | Muninn `user link` + `net exchange` |
| `weirdnet-sync` (new, if `weirdnet-storage::sync` is not enough) | change-log pull protocol, per-record clocks, conflict store, sync keys | Muninn linked-node sync on `weirdnet-storage` |
| `weirdnet-net` (exists) | transport, relays, sealed messages, short codes | already used |

Morph adds `morph-identity` glue (per-profile paths, which data types a
profile syncs) and Flutter screens. Muninn moves to the same crates later, so
the formats below are shared and versioned (`weirdnet-…/v1`), not Morph-only.

## 1. Backups

A backup is one profile, never the whole Morph install.

`morph-<profile>-backup-<UTC timestamp>.wnbk` is a zstd tar (the
`weirdnet-backup` format) with:

| Path | Contents |
|---|---|
| `manifest.json` | format `weirdnet-backup/v1`, app `morph/<version>`, profile id and name, user id, created_at, BLAKE3 of every file |
| `identity/user.key` | the sealed seed, as on disk (still password-sealed) |
| `identity/identity.json` | public user id and fingerprint |
| `data/library.db` | bookmarks, history, folders, tags (SQLite online-backup copy) |
| `data/tabs.json`, `data/settings.json` | open tabs and profile settings |
| `trust/` | trusted peers and pinned node ids |

- The whole archive is always encrypted (age scrypt recipient, or
  `password_seal` of the archive key) with a **backup password** the user
  types when exporting, at least 8 characters, eye toggle, typed twice.
  Unlike Muninn, unencrypted backups are not offered: browsing history is
  sensitive even without keys.
- Web view storage (cookies, site data) is not included in v1. It is
  engine-specific and often large; sign-ins are re-done after a restore.
  Revisit once sync of passwords exists.
- Restore verifies every hash before writing anything; a mismatch writes
  nothing.
- Desktop: save dialog. Android: SAF (as for the recovery phrase). Automatic
  scheduled backups to a folder are a later option.

## 2. Restore in onboarding

The first-use identity screen already offers "Restore from recovery words".
It gets three ways in:

1. **24 words** (exists): rebuilds the seed; the user sets a new profile
   password.
2. **Backup file**: pick a `.wnbk`, enter the backup password, preview what
   it holds (profile name, user id fingerprint, counts of bookmarks, history
   and tabs), then restore into this profile. The seed inside is still sealed
   with the old profile password; the user enters it once, then sets a new
   one (or keeps it).
3. **From another device** (QR, section 4), or **from peers** (dark shards,
   section 3) when the words are lost.

If the profile already has data, restore asks "Merge into this profile" or
"Replace". Merging uses the sync merge rules (section 5).

## 3. Dark shards

Same as Muninn's: the seed is split with Shamir (`weirdnet-keys`), one shard
per chosen holder, any *threshold* rebuild it (default a majority, at least
2). Each shard is sealed with a **shard password** (Argon2id +
XChaCha20-Poly1305, AAD binds set id, shard index, threshold and user id)
before it leaves, then travels inside the exchange's sealed channel. Holders
store a blob they cannot open.

- **Who can hold one:** only nodes of trusted connections (an exchange whose
  short code was verified, or a user you marked trusted) and your own other
  devices. Holders must be different users or nodes; Morph warns when two
  shards would sit with the same person.
- **Claim ids:** each shard is also filed under
  `claim = HKDF(shard password, salt = set id, info = "weirdnet-shards/v1/claim")`,
  so a fresh install with a new node key can still ask for it.
- **Restore:** "Restore from peers" asks your connections (by user id,
  through relays as needed) for shards under your user id or claim, opens
  them with the shard password and rebuilds the seed. It is checked against
  the profile's user id before use.
- **Lifecycle:** a set has an id; re-issuing makes a new set and asks holders
  to drop the old one. Holders can drop shards they hold. History records
  which set went to which holder, never shard contents.
- **Per profile:** a profile's shards are for that profile's identity only;
  other profiles' contacts are not offered as holders.

## 4. Login over QR (device linking)

Moves a profile's identity to another device of the same person, phone to
desktop or desktop to phone, without typing 24 words.

Roles: the **new device** (no identity for this profile yet) shows the QR;
the **existing device** (has the identity, unlocked) scans it. This direction
means the secret only flows to the side that showed a fresh key, and it works
the same both ways: phones scan with the camera; a desktop without a camera
shows the QR and the phone scans it, or the desktop can paste the
`weirdnet-morph://link/…` text instead.

Protocol (`weirdnet-device-link/v1`):

```text
new device:   e_N = fresh X25519 keypair, node key N (Ed25519), nonce n
              QR = weirdnet-morph://link/<base64url CBOR {
                     v:1, node: N.pub, eph: e_N.pub, nonce: n,
                     addrs / relay, expires: now + 120 s, profile hint }>
              signed by N
existing:     checks signature and expiry, shows "Link <device name>?"
              e_E = fresh X25519 keypair
              connects over Weirdnet (direct, mDNS or relay) to N
              transcript = SHA-256("weirdnet-device-link/v1" ‖ offer ‖ e_E.pub)
              k = HKDF(X25519(e_E, e_N), salt = transcript, info = ".../key")
              SAS = HKDF(..., info = ".../sas") as 6 digits
both screens: show the SAS; the user confirms they match on the existing
              device (and taps "Matches" on the new one)
existing:     sends seal_k({seed, profile name, settings, node cert for N
              signed by the user key, sync opt-ins}), single use
new device:   unseals, checks user id = derived from seed, asks for a new
              profile password, seals the seed locally, acks
both:         erase e_N, e_E, k; the QR is now dead
```

- **Expiry and single use:** the offer lives 120 seconds and is accepted
  once; the new device stops listening after one transfer or on timeout.
- **SAS:** protects against a QR photographed or swapped by someone nearby;
  the transfer only happens after the user confirms the code on the device
  that holds the key. Three wrong attempts cancel.
- **No secret in the QR:** only public keys, addresses and a signature, so a
  leaked screenshot is useless after two minutes and without the SAS step.
- **Transport:** Weirdnet sealed request/response (`/weirdnet/link/1`) over
  libp2p Noise; relays only see ciphertext. Same LAN works with mDNS and no
  internet.
- **Result:** both devices are nodes of the same user; the new node holds a
  `NodeCert` signed by the user key and appears in the device list
  (section 5).
- **Platforms:** Android uses the camera (CameraX through a small plugin or
  `mobile_scanner`, decided at build time by APK size); desktop renders the
  QR with a pure-Dart encoder and also accepts the text link.

## 5. Sync across Weirdnet

Profile data syncs end-to-end encrypted between devices that share the
profile's identity, peer to peer over Weirdnet.

**What syncs (opt-in per data type, per profile):** bookmarks (with folders
and tags), history, open tabs (shown as "tabs on your other devices", not
merged into local tabs), settings that make sense across devices, and later
saved passwords (separate opt-in, separate key). Never synced: the sealed
seed (moved only by section 4), node keys, web view cookies and site data,
device-only settings (layout, default browser).

**Keys:** `sync_key = HKDF(seed, salt = user id, info = "weirdnet-sync/v1/<type>")`.
Each change page is sealed with the data type's key, on top of the
exchange's channel, so a relay or ingress peer only forwards ciphertext and
another user's node cannot read it even if misrouted.

**Model and conflicts:**
- Every record has a stable id (UUIDv7), `created_at`, `updated_at`, the
  node id that last changed it, and a per-record version (hybrid logical
  clock). Deletes are tombstones kept for 90 days.
- Each node keeps a change log (`weirdnet-storage` change log). Peers pull
  changes after the last sequence they have from each node, a page at a time.
- Merge per record: last writer wins by HLC; a field-level merge for
  bookmarks (title, url, folder, tags merged separately; tags are an OR-set
  so concurrent adds both survive). Edit-vs-delete keeps the edit. Like
  Muninn, a losing version is kept as a conflict for 30 days rather than
  dropped; the UI shows "1 conflict" only for bookmarks and settings.
- Folder moves use the record's `parent_id` plus a fractional `order` key, so
  concurrent reorders do not collide.
- History is append-only (visits are immutable), so it never conflicts.

**Devices and revocation:** Settings > Devices lists the profile's nodes
(name, platform, last seen, last synced). Removing one signs a node
revocation with the user key; other devices stop answering it at once, and
the next key epoch (`info = …/<type>/e<n>`) is used for new pages so the
revoked node cannot read later data even via a relay copy.

**Offline catch-up:** a device that was offline pulls from whichever of its
own nodes is reachable, directly or via its home relay. If none is online,
nothing happens until one is; there is no server copy. An optional
"always-on" node (your desktop, or a self-run relay storing sealed pages
only) can hold pages for phones.

**Per profile:** sync runs per profile identity; two profiles on one device
are two unrelated users to the network.

## Threat model

| Threat | Mitigation |
|---|---|
| Stolen backup file | Always encrypted with the backup password; seed inside is sealed again with the profile password. Offline brute force is slowed by Argon2id/scrypt. |
| Shard holder (or colluding holders below threshold) | Shamir below threshold reveals nothing; each shard is also password-sealed, so even threshold holders together need the shard password. |
| All holders collude and know the shard password | Out of scope; choose holders and password accordingly. Re-issue the set to rotate. |
| QR photographed or shown on a shared screen | No secret in the QR, 120 s expiry, single use, SAS confirmation on the key-holding device. |
| Network attacker / relay | Noise channel plus sealed payloads; relays and ingress peers see ciphertext and peer ids only. |
| Lost or stolen device | Revoke from another device; key epoch moves forward; the profile password protects the sealed seed at rest. |
| Malicious sync peer of another user | Answers only nodes with a valid, unrevoked node cert from the same user id; pages are sealed with keys derived from the seed. |
| Corrupt or tampered backup | Manifest hashes checked before anything is written. |
| Secrets in logs | Seeds, words, shards, passwords and sync keys are never logged; history records only actions and ids. |

Not covered: a compromised device that is unlocked (it has the keys), and
an attacker who knows the 24 words.

## Order of work

1. `weirdnet-backup` + profile backup/restore (desktop and Android), onboarding
   restore from file.
2. `weirdnet-device-link` + QR login (desktop shows, phone scans first).
3. Devices list and revocation.
4. `weirdnet-sync` with bookmarks, then history, tabs, settings.
5. `weirdnet-shards` + dark shards UI.

Each step ships with tests against the shared crates and Morph's UI, and with
cross-checks against Muninn where the formats are shared.
