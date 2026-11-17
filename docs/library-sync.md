# Library sync

Status: design, 2026-11-17; generic protocol moves into `weirdnet-sync` (not yet published). Product rules are in
`docs/identity-recovery.md` section 5. This file is the Morph implementation
plan for that section: bookmarks, folders, tags and history first, then
settings, between a user's own linked devices, end-to-end encrypted, over
`weirdnet-net` 0.5 and the same relays device linking already uses.

Nothing here is built yet. Do not treat this as a wire-format freeze until
milestone 2 ships a versioned `weirdnet-sync/v1` codec with tests.

## Goal

Two (or more) Morph profiles that share one Weirdnet user identity keep the
same library. A bookmark added on the phone appears on the desktop; a delete
or a folder move on either side wins according to the rules below. Sync is
peer to peer. The official relay forwards ciphertext only and does not store
library pages.

Local storage stays `profiles/<id>/library.db` (`morph-store`, opened through
`morph-core::library_db` with the profile's `LibraryKey`). Sync does not
replace that file; it applies remote changes to the same records the shell
already reads and writes.

## Already in the tree

- **Records:** `morph_store::{Bookmark, Folder, Tombstone, HistoryEntry, BookmarkSet}`.
  Bookmarks and folders already have a stable UUIDv7 id (`morph_core::bookmarks::new_id`),
  `created`/`updated` Unix-ms, a parent, and a fractional base-62 `order` key
  among siblings (chosen so a move rewrites only the moved row). Deletes of
  bookmarks and folders already insert a `tombstones` row ("so sync can tell
  delete from absent"). Tags live in `bookmark_tags`.
- **Keys:** `LibraryKey` is HKDF-SHA256 of the identity seed with info
  `morph/library-db/v1` (`morph_identity::profile_identity`). After a QR link,
  both devices derive the same key and can open a library encrypted with it.
  That key is for SQLCipher at rest, not for the wire.
- **Nodes:** `weirdnet-device-link` already issues a `NodeCert` (user key
  signs the new node's peer id and name) and writes `identity/node.key` plus
  `identity/node.cert.json` on the new device. `Transfer` already has a
  `sync: Vec<String>` field; `AddDevice::send` currently passes `settings = "{}"`
  and `make_transfer` leaves `sync` empty. `has_linked_node` is true only on
  a device that *received* a link: a profile that created or restored its
  identity has no node key yet.
- **Transport:** `morph_identity::device_link::net_config` / `PUBLIC_RELAYS`
  (and `MORPH_LINK_RELAYS`) already dial TCP and QUIC together and fall back
  to a relay circuit. `crates/weirdnet-device-link/examples/local-relay.rs`
  and `scripts/xvfb-check.py --local-relay` are the CI pattern to reuse.
- **Not in the tree yet:** per-record HLCs or node ids, a change log, history
  tombstones, history row ids, a device roster or revocation, a self-issued
  `NodeCert` on the first device, or any sync loop.

## Data model

Not a tree CRDT. Each device keeps an **op log** of its own changes, and
every bookmark, folder and settings record carries a **hybrid logical clock**
plus the node id that last wrote it. Peers pull log pages and merge
record-by-record. This matches section 5 and the record shapes that already
exist.

### Clocks and the log

An HLC is `(physical_ms, counter, node_id)`. `physical_ms` is the same
caller-supplied Unix-ms the library already uses in tests (`now_ms`). On
every local write the clock is `max(wall, last_hlc.physical)`, with `counter`
incremented when the physical time does not advance. The log is a table in
`library.db` (same SQLCipher file, so it is already encrypted at rest):

```text
sync_log (node_id TEXT, seq INTEGER, hlc TEXT, typ TEXT, record_id TEXT, op BLOB, PRIMARY KEY (node_id, seq))
sync_cursor (node_id TEXT PRIMARY KEY, last_seq INTEGER)   -- last seq applied from each peer
sync_meta (key TEXT PRIMARY KEY, value BLOB)               -- roster, epoch, opt-ins, our clock
```

Each local mutation (add/edit/move/tag/delete, a history visit, a history
delete, a settings write) appends one log entry and updates the row. A device
never rewrites another node's log entries; it only appends to its own.

### Bookmarks, folders, tags

One record per id, as today. Schema additions (append a `morph-store`
migration; do not edit the shipped one):

- `hlc TEXT NOT NULL` and `node_id TEXT NOT NULL` on `folders` and `bookmarks`
- `deleted` tombstones gain `hlc` and `node_id`; keep `deleted_ms` as the
  physical part for the existing UI

**Merge (last writer wins by HLC, with these specials):**

- Title, URL, `folder_id` / `parent_id`, and `order` each LWW independently,
  so a rename on one device and a move on the other both survive.
- Tags are an OR-set: a concurrent add on A and add on B keeps both; a
  remove is a per-tag tombstone and LWW against adds of that tag.
- Concurrent edit vs delete: the edit wins. A delete whose HLC is strictly
  later than the edit wins (the tombstone stands). Losing versions are kept
  for 30 days (`sync_conflict`) and the UI shows "1 conflict" for bookmarks
  (and later settings), as in section 5.
- Tombstones are garbage-collected after 90 days, but only after every known
  peer cursor is past that delete (otherwise a late catch-up would resurrect
  the row).
- If a merge would cycle a folder, move the later-HLC folder to the root.
  If a bookmark's parent is missing, show it at the root until the folder
  arrives or is tombstoned.

### History

Visits stay append-only and immutable, so two visits never conflict. The
current table cannot sync deletions as-is: `delete_visits` and `clear_history`
remove rows and leave no tombstone, and rows have no stable id.

Migration: give each visit a stable id (UUIDv7 for new rows; a deterministic
id from `(url, visited_ms)` for existing ones, same idea as
`bookmarks::legacy_id`). Add `history_tombstones (id, url, hlc, node_id)`:

- Delete-by-URL (today's `delete_history_url`) writes one tombstone that
  matches `same_page` for that URL; later visits to the same URL, with an
  HLC after the tombstone, are kept.
- Clear-all writes a single `history_cleared` tombstone; visits with an
  HLC before it stay gone; visits after it survive.

`MAX_HISTORY` (5000) remains a local cap applied *after* merge, by
`visited_ms`, newest first. Trims are not synced as deletes: a visit older
than the 5000th is ignored if it arrives later. Reloads that only refresh
the newest entry stay a local `replace_newest_visit` and one log op, as
today.

### Settings (later milestone)

`profiles/<id>/settings.json` stays the device file. Sync only the fields
that section 5 would call cross-device: `search_engine`, `home_page`,
`theme`. Each is its own LWW record in `library.db`. Leave `tab_position`,
`sidebar_collapsed`, `sidebar_width` and `downloads_dir` local (layout and
paths). Default-browser state is already OS-specific and stays out.

Open tabs (`profiles/<id>/tabs.json` is reserved) are out of the first
milestones. When they come, they are shown as "tabs on your other devices"
and are not merged into the local tab strip, as section 5 already says.

Never synced: the sealed seed, node keys, keystore entries, web-view cookies
and site data, recovery words, backups, paper shards.

## Keys

Two layers, both from the profile seed, never logged:

| Key | Derivation | Use |
|---|---|---|
| `LibraryKey` | HKDF-SHA256(seed, info = `morph/library-db/v1`) | SQLCipher for `library.db` (already shipped) |
| `SyncKey(type)` | HKDF-SHA256(seed, salt = user id, info = `weirdnet-sync/v1/<type>` or, after a revocation, `…/<type>/e<n>`) | Seal change pages on the wire |

`<type>` is `bookmarks`, `history`, or `settings`. The OS keystore still
holds only `LibraryKey`, as today.

Per-device authentication is the node's Ed25519 key and its `NodeCert`. A
sync session is accepted only if the peer presents a cert for the same
`user_id` that `WEIRDNET.verify_cert` accepts and that is not revoked. The
Noise (or `weirdnet-net`) channel authenticates the peer id; the cert binds
that peer id to the user.

Revoking a device (Settings > Devices, once built) signs a revocation with
the user key, gossips it in the log, and bumps the key epoch so the revoked
node cannot decrypt later pages even if it still has an old `SyncKey`.

## Transport and protocol

Reuse the link node's listen addresses and relay list
(`net_config` / `PUBLIC_RELAYS` / `MORPH_LINK_RELAYS`). Prefer a direct
connection; if both sides are on different networks, reserve a circuit on
the same official relay and talk through it. Do not send library pages as
`weirdnet-<app>://` app calls; those are for opening a peer's app in a tab.

Wire format `weirdnet-sync/v1`, length-prefixed CBOR, one request/reply at a
time (same style as `/weirdnet/link/1`). After the authenticated channel is
up, every payload that carries records is sealed with `SyncKey(type)`.

Messages:

- `Hello { cert, user_id, epoch, types, our_cursors, our_hlc }` — cursors
  and certs ride inside the sealed body.
- `HelloOk { cert, epoch, cursors, revocations }` or `Refuse { reason }`
  (`wrong-user`, `revoked`, `epoch-behind`, `type-off`).
- `Pull { node_id, after_seq, limit }` — pages of that node's log.
- `Page { node_id, from_seq, to_seq, ops }` — sealed ops; size stays within
  the link caps already used by `weirdnet_device_link::reach` (256 KiB
  requests, 16 KiB is too small here: raise the *reply* cap for sync only,
  e.g. 256 KiB pages, still well under weirdnet-net's 8 MiB default).
- `Ack { node_id, last_seq }`.
- `Roster { certs, revocations }` — on first session and when the set changes.

**Catch-up:** on unlock (keystore or password) and then periodically, the
device hello's every node in its roster, directly or via the relay, and
pulls each node from `sync_cursor`. If none are reachable, it waits; there
is no server copy. A desktop left running is the practical "always-on" node
for a phone. An optional self-run store of *sealed* pages is a later
addition and must not be enabled on the official WeirdBrains relay.

Applying a page is transactional: merge into `library.db`, advance the
cursor, then ack. A crash mid-page replays from the last cursor. Unknown
`typ` values are stored in the log and skipped, so a newer device can talk
to an older one.

## How linking enrolls a device

1. The first device to hold an identity (create, import, or restore) must
   become a node: generate `identity/node.key`, self-certify a `NodeCert`
   with the user key, and put itself in the roster. Today only a *new*
   linked device gets a cert (`NewDevice::finish`). Sync cannot start until
   this gap is closed.
2. QR / invite linking stays as it is (SAS, sealed seed, cert for the new
   node). Fill `Transfer.sync` with the existing device's opt-ins
   (default: `bookmarks`, `history`; `settings` once that milestone ships)
   and put a snapshot of the roster (certs only, no secrets) in
   `Transfer.settings` JSON, which is already defined as "app settings
   worth carrying over".
3. `finish` on the new device stores the seed, derives `LibraryKey`, adopts
   the library (`morph_services::library::adopt_new_identity`), writes the
   node key and cert, and saves the roster. Any plaintext bookmarks and
   history already in that profile are imported into `library.db` as today,
   then merged with what is pulled from the roster (merge, not replace,
   unless the user picks Replace in a restore flow).
4. First sync starts automatically once the library is unlocked: hello every
   roster member, pull their logs, then offer our own (including the
   imported local rows). Pages are sized so a 5000-entry history still
   catch-up in several pulls.
5. Two profiles on one install stay two identities and two sync sets.

A device linked before sync exists already has a cert and can be added to
the first device's roster the next time they both run a hello, once the
first device has self-certified. No re-link is required if both are online;
if the first device has no cert and no roster, the user re-opens Add device
(or a one-shot "enable sync" that self-certifies and waits for a hello).

## Privacy

| Who | What they see |
|---|---|
| Official relay / ingress peer | Peer ids, that a circuit was reserved, encrypted blob sizes and timing. Not URLs, titles, tags, settings, user ids, or certs in the clear (those sit in the sealed channel). |
| Another Weirdnet user | Nothing. Sessions whose cert is not the same `user_id`, or is revoked, are refused before any page is sent. |
| A revoked device that kept an old `SyncKey` | Pages sealed under earlier epochs only. |
| Logs and `morph-event` lines | Actions, record ids, node ids, seq numbers. Never seeds, words, passwords, `LibraryKey`, `SyncKey`, or page URLs and titles. |

At rest each library remains a SQLCipher file mode 0600, opened only while
the identity is unlocked. Sync runs only then.

## Migration

Append one `morph-store` schema migration. Existing bookmark and folder rows
get `hlc` from `updated_ms` (or `added_ms` / `created_ms` if `updated_ms` is
0) with counter 0 and `node_id` equal to this device's node once it exists;
legacy ids from `legacy_id` stay. Existing history rows get deterministic
ids; no tombstones are invented for past deletes (those are already gone).
`settings.json` is left in place; on the settings milestone, the three
syncable fields are copied into `library.db` once.

Profiles still on plaintext JSON (`LibraryState::Plain`) do not sync. Sync
starts after the identity exists and the library is unlocked, which already
imports JSON and deletes those files.

The SQLCipher `LibraryKey` info string is not bumped; linked devices already
share it. Wire sealing uses the new `weirdnet-sync/v1/…` info strings only.

## Testing

Prefer Rust tests that do not need a GUI.

- **Merge unit tests** for the primitives live in `weirdnet-sync`. Morph
  tests record-level apply in `morph-store` / `morph-core`: LWW title vs
  move, OR-set tags, concurrent edit vs delete, folder cycle break, history
  delete-by-URL and clear-all vs a later visit, cap-5000 trim not
  resurrecting, two `MORPH_HOME`s in one process with a fake transport.
- **Link then sync** in `morph-services`: two temp homes, `local-relay`
  example, existing `device_link` helpers to enroll B from A, add a
  bookmark and a visit on A, run the sync loop, assert B's `library.db`
  matches; delete on B, catch A up; concurrent edits; a third home that
  was offline pulls both logs. Use `morph-keystore`'s `mock` feature and
  never `~/.morph`.
- **CI:** a `cargo test` job (and the existing `desktop-linux` check) runs
  the two-home + local-relay test. An Xvfb path is optional later (create a
  bookmark in the shell, assert it on the other home) and should follow
  `scripts/xvfb-check.py --local-relay`.

## Crate split

Generic protocol and merge primitives live in **`weirdnet-sync`**
(`WeirdBrains/weirdnet-sync`, formats `weirdnet-sync/v1`), not in Morph, so
Muninn can share them, as `docs/identity-recovery.md` already sketched. That
crate owns: HLC, the op log, LWW and OR-set merge, tombstone GC, page sealing
(`SyncKey` derivation), roster / self-cert / revocation, cursors, and the
pull/hello protocol over `weirdnet-net`. It does *not* own bookmark, folder,
history or settings types.

Morph keeps only Morph-specific record types, the `morph-store` schema
migration, and wiring (when to start, opt-ins, enrollment) plus a small
GPUI / Flutter panel (conflicts, devices, last synced).

`weirdnet-storage` already has a `sync` module: a SQL row change log with a
three-way take/keep/conflict decision. That is not this protocol. Wrap it
later if it grows to cover the HLC log, rather than forking it.

`weirdnet-sync` is not on crates.io yet, and the GitHub repo still has to be
created. Morph releases cannot take a git or path dependency (they pin
published crates.io versions). Until 0.1.0 is published, Morph can land the
schema migration and this document only; do not wire `weirdnet-sync` into
Morph's `Cargo.toml`.

## Milestones

1. **`weirdnet-sync` crate, Morph schema only.** HLC, op log, merge
   primitives, sealing, roster, protocol and tests in the Weirdnet crate.
   In Morph: history row ids and tombstones, and `hlc` / `node_id` columns
   on bookmarks, folders and tombstones. Self-certify a `NodeCert` on
   create / import / restore as well as on link, and persist a roster of
   one, once that helper exists in `weirdnet-device-link` or can be called
   without a new unpublished dependency. No user-visible sync yet, and no
   Morph git/path dep on `weirdnet-sync`.
2. **Wire + two homes** (after `weirdnet-sync` is published). Morph wires
   the crates.io crate, pull/hello over `weirdnet-net` 0.5,
   sealed pages, local-relay integration test. Fill `Transfer.sync` and the
   roster snapshot at link time.
3. **Bookmarks, folders, tags** in the GPUI and mobile shells, default on
   after link, conflicts surfaced in the library panel.
4. **History**, including delete-by-URL and clear-all.
5. **Settings** (`search_engine`, `home_page`, `theme` only).
6. **Devices UI and revocation** (Settings > Devices, key epoch). Until
   this ships, losing a device means the profile password and a later
   epoch bump still need a manual story — do not advertise revocation as
   done.
7. Later, out of scope here: open tabs on other devices, an optional
   always-on sealed page store, saved passwords (separate opt-in and key).

## Open decisions

1. **Edit vs delete:** this plan follows section 5 (concurrent edit wins;
   a strictly later delete wins). Confirm before milestone 1 tests freeze
   it.
2. **Default opt-ins:** this plan turns `bookmarks` and `history` on at
   link (and at "enable sync" on a first device) without a pre-flight
   prompt, with a Settings toggle to turn a type off. A prompt-first
   policy is the other option.
3. **Sync reply cap:** 256 KiB pages, requested above the 16 KiB link
   answer cap. Exact number can move; it must stay under weirdnet-net's
   default 8 MiB.
4. **Publish `weirdnet-sync`:** a new `WeirdBrains/weirdnet-sync` repo and
   a crates.io 0.1.0 release are required before Morph can wire the
   protocol. Wire names are already `weirdnet-sync/v1`.
5. **Official relay storage:** no. Only a user-run node may keep sealed
   pages for catch-up.
6. **Re-link vs automatic roster:** this plan tries hello-based discovery
   after the first device self-certifies, and only asks for a re-link if
   that never happens. Worth a trial in milestone 2 before adding UI.
