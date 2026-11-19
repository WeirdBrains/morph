# Profile lock

A profile with a Weirdnet identity asks for its password before it opens.
This is on by default and can be turned off per profile in Settings. A
profile without an identity has no key, so it is **not encrypted and not
protected by a password**. Anyone who can use the computer can open it.

Code: `morph-core/src/profile_lock.rs` (the setting, policy and backoff),
`morph-core/src/profile_files.rs` (sealed profile files),
`morph-services/src/profile_lock.rs` (the gate and the password operations),
`morph-services/src/library.rs` (open and unlock),
`morph-identity/src/profile_identity.rs` (`change_password`),
`morph-desktop/src/lock_view.rs` (the lock screen) and `shell.rs` (where windows build).

## Status

Progress note for whoever picks this up next (update it with each push).

- Done, in five commits on main: identity `change_password` (crash-safe),
  sealed profile files, `morph_services::profile_lock` (default on, policy,
  backoff), gates on launch, switch, new window, CLI/URL hand-off, MCP host
  (stops servers beside the `applied_revision` watch), sync (`Error::Locked`)
  and `DeviceCaller` (`DeviceCallError::Locked`), the desktop lock screen,
  Settings > Privacy (off with the current password and a warning, change),
  docs and changelog.
- Tests: workspace tests and clippy `-D warnings` pass;
  `sync::tests::three_homes_rotate_on_revoke_and_re_admit_a_restored_device`
  can time out under a full parallel run and passes alone, on main as well.
- `scripts/xvfb-check.py --only profile-lock` is written but not run yet: the
  dev machine has no `xdotool` (or `sway` for `--wayland`). Run it in CI or a
  box that has them.
- Open: seal `companions/`, `ai/policy.json` and the MCP host's
  `mcp-*.local.json` under the file key (with Morph Dev); show locked
  profiles in `profiles_view`; auto-lock after idle; OS authentication
  unlock; the Flutter gate with biometrics; OS authentication backends (see "Password saver" below).

## Keys

```text
profile password ──Argon2id (19 MiB, t=2, p=1, random salt per seal)──▶ KEK
KEK ──XChaCha20-Poly1305──▶ identity/user.key   (the 32-byte identity seed, random)
seed ──HKDF-SHA256 "morph/library-db/v1"──▶ library key  (SQLCipher key of library.db)
library key ──HKDF-SHA256 "morph/profile-files/v1"──▶ file key (settings.json, tabs.json)
```

The seed is the random profile data key. It is wrapped under the password
(`weirdnet_keys::password_seal`) and is the same seed that the 24 recovery
words encode. Changing the password only reseals `user.key` and `node.key`,
so the library and files never need re-encrypting. The change is
crash-safe: both files are staged as `*.next`, then a `password-change.commit`
marker is written, then they are renamed. An interrupted change is rolled
forward if the marker exists and dropped if it doesn't.

Keys are zeroized on drop (`Key32`, `LibraryKey`, `Zeroizing`). They live
only in the open SQLCipher connection and in the file-key map, and `lock`
drops both.

## What is encrypted at rest

| Data | Where | How |
|---|---|---|
| Bookmarks, history, sync log, sync seed, epoch keys and node key copy, companion sync records | `library.db` | SQLCipher under the library key |
| Settings, open windows and tabs | `settings.json.sealed`, `tabs.json.sealed` | XChaCha20-Poly1305 under the file key, bound to the file name |
| Identity seed, node key | `identity/user.key`, `identity/node.key` | sealed under the password |
| Saved passwords | `library.db` (`saved_logins`) | SQLCipher under the library key, and each password sealed again (XChaCha20-Poly1305) under the saver key |

**Companion and MCP files** are sealed too (`morph-services::profile_io`, through
`morph_core::profile_files`): everything under `companions/` (roster, `policy.json`, settings,
chats, journals, `mcp.json`, `exports/`, `tool-log.jsonl`) and every `mcp-*.local.json` beside it
(`mcp-secrets.local.json`, `mcp-approved.local.json`). `morph-ai` reaches them only through its
`FileIo` seam (`FileStore`, `FileSecrets`, `McpApprovals`), so it holds no key. While the profile is
locked they read as `StoreError::Locked` (secrets and approvals read as empty and drop what they
had in memory), nothing is written over them, and the next unlock seals any plaintext an older build
left (`profile_io::seal_existing`, run by `library::{open, unlock, adopt_new_identity}`).

**Web view data** (cookies, local storage, IndexedDB, cache under
`profiles/<id>/webkit/`) is **not encrypted by Morph**. It sits in the
profile's own directory, protected only by OS file permissions and by
full-disk encryption if you use it. A locked profile builds no web view, so
Morph never reads or shows that data while the profile is locked. But
someone with your user account, or the disk, can read it directly. Cheap
improvements per platform, not done yet:

- **WebKitGTK:** an ephemeral `WebsiteDataManager` per profile (an opt-in
  "Don't keep site data" switch). No cookie encryption option exists.
- **WebView2:** cookies are already DPAPI-encrypted per Windows user, which
  isn't the same as per profile.
- **WKWebView:** a per-profile `WKWebsiteDataStore(forIdentifier:)`. On iOS,
  Data Protection (`NSFileProtectionComplete`) covers the files while the
  device is locked.

## The setting

`<profile>/lock.json` holds `require_password` (default `true`) and the
failed-attempt count. It is plaintext because it is read before the unlock.
Editing it gains an attacker nothing: the library key is in the OS keystore
only after the user turned the password off with the current password.

- **On (default):** the library key is never kept in the keystore. Every
  launch, switch to the profile, new window or link into it needs the password.
- **Off:** Settings > Privacy and site data asks for the current password and shows a warning
  before turning it off. The key is then kept in the system keystore (Secret
  Service, Keychain, Credential Manager, Android Keystore), so the profile
  opens without a prompt. The library and files **stay encrypted**, with the
  key in the keystore, instead of being decrypted. This is deliberately
  stronger than writing plaintext. Turning it back on removes the key from
  the keystore.
- **Admin policy:** set `"require_profile_password": true` in any network
  policy file (`MORPH_NETWORK_POLICY`, `<home>/network.policy.json`,
  `/etc/morph/network.policy.json`). The password then stays on, and turning
  it off is refused. Only `/etc/morph/` is out of the user's reach.

## Entry points

Every path goes through `morph_services::library::open` or
`morph_services::profile_lock::is_locked` / `require_unlocked`:

- **App launch:** a locked current profile opens one window showing the lock
  screen. Nothing else of the profile is built. After the unlock, its saved
  windows come back.
- **Profile switch** (menu or picker): the window saves the tabs of the
  profile it leaves, then shows the lock screen if the target is locked. That
  profile's other saved windows open after the unlock. The profile it left
  is locked once no window shows it.
- **New window:** the window shows the lock screen if its profile is locked.
- **CLI, single-instance hand-off and OS URL handler:** links handed to a
  locked window wait (64 at most) until it unlocks. They are never opened in
  another profile.
- **MCP host:** runs no server for a locked profile. Running servers stop
  within about 250 ms of a lock (`mcp_host.rs` `Hooks::locked`, and
  `mcp_may_start` fails closed).
- **Sync:** the node runs only while the library is open. It stops on lock
  and starts again on unlock; `sync::start` on a locked profile returns
  `Error::Locked` ("profile locked").
- **DeviceCaller** (linked-device calls, `NodeRouter` streams): `for_profile`
  on a locked profile returns `DeviceCallError::Locked` ("profile locked").
  A caller made before the lock refuses calls, streams and served requests
  from then on, as well as losing its halted node.
- **Lock profile** (main menu, or `shell.lock_profile`): saves the tabs, then
  every window of the profile drops its pages and shows the lock screen.

While locked, settings and tabs read as locked (`ProfileError::Locked`).
Writes and removals are refused, so a locked profile's saved windows are
never lost or overwritten.

## Wrong passwords

The first 3 wrong passwords cost nothing. After that each attempt waits 1 s,
then 2 s, doubling up to 5 minutes. An attempt during the wait is refused
without being tried. The count is kept in `lock.json`, survives restarts and
is cleared by a right password. Offline, the real cost of each guess is
Argon2id. The count only slows guessing through Morph.

## Forgotten password

A forgotten password can't be recovered on this device. You can:

- delete the profile from another profile, or
- link it again from one of your other devices, or restore it from the
  recovery words or a backup, into a new profile.

The lock screen says this.

## New identities and upgrades

- **Creating, importing, restoring or linking an identity** always asks for
  a password; there is no skip. The library and files are encrypted at once,
  and the lock is on.
- **Upgrading from a build that kept the key in the keystore:** the next
  open asks for the password and removes the stored key. Plaintext
  `settings.json` and `tabs.json` are sealed on that unlock. The sealed copy
  is written atomically first and the plaintext is removed after, so a crash
  at any point loses nothing.
- **Profiles without an identity** are unchanged and open freely. Tests and
  headless runs use them, so no test-only bypass exists.

## Threat model

Protects against:

- someone using your unlocked computer opening or switching to the profile,
  or reaching it through links, MCP or sync
- someone copying the profile directory while it's at rest (bookmarks,
  history, settings, tabs, sync keys)

Does not protect against:

- malware running as your user while the profile is unlocked
- reading the web view data directory (see above)
- the companion and MCP files not yet sealed
- a profile whose password is turned off: anyone who can unlock your OS
  keystore can open it

## Password saver

Approved by m3talsmith on Nov 19, 2026: **no separate master password**.

Code: `morph-store/src/saver_store.rs` (table `saved_logins`, migration 6),
`morph-core/src/password_saver.rs` (keys, sealing, requirement, `OsAuth`),
`morph-services/src/password_saver.rs` (the fill gate for front ends).

- Saved logins live in `library.db`, so the profile password already gates
  them at rest. Each password is also sealed under a **saver key**:
  `seed ─HKDF "morph/library-db/v1"▶ library key ─HKDF "morph/password-saver/v1"▶ saver key`.
  The seed is not held after unlock, so the saver key is derived through the
  library key; both follow the seed, so another device with the same
  identity derives the same key and sealed values can sync as they are. A
  device without the identity cannot open them. The saver key is kept beside
  the file key and dropped on lock.
- Each sealed value is bound (AAD) to its row's id, origin and username, so a
  row cannot be repointed at another site or user.
- Only `http` and `https` origins; the origin is normalized (default ports
  dropped). One login per origin and username (saving again updates it).
- **No identity:** no key, so the saver is not offered.
- **Password on (default):** the unlock was the proof; filling asks for nothing
  more.
- **Password off:** filling needs OS authentication (`OsAuth`). Where the OS
  cannot authenticate, which is every platform today, the saver is not
  offered (`Unavailable::NoOsAuth`).
- **"Ask again before filling"** (sealed profile file `password-saver.json`):
  the fill re-checks the profile password (same backoff as the lock screen),
  not a second password. Turning it off needs the profile password. With the
  password off it adds to the OS authentication.
- `fill` reveals a password only with a `FillPermit` built after the proof(s)
  the profile's requirement names. That is a seam against misuse, not a
  boundary against code in the same process: the key is in memory while the
  profile is open.

### Status (password saver)

- Done: storage table; sealing and `reveal`; requirement and `OsAuth` trait
  with the `NoOsAuth` stub; sealed saver settings; services layer (`offer`,
  `save`, `list`, `delete`, `fill`, `set_ask_again`); tests in all three
  crates.
- Left, nothing here is wired to a UI yet:
  - a platform `OsAuth` (polkit on Linux, Touch ID, Windows Hello);
  - capture and fill in the page (webview bridge fill exists for the
    companion; the saver needs its own prompt and origin matching rules);
  - Settings > Privacy switch for "ask again" and a saved-passwords list;
  - **password sync**: waits for weirdnet-sync. The `hlc` and `node_id`
    columns exist and are empty; deletes need a tombstone then. Values are
    already sealed, so nothing in the sync step needs the plaintext;
  - import from other managers (1Password, Bitwarden, LastPass work as
    extensions in v0.5).

## Not done yet

- **Auto-lock after idle:** Morph has no idle setting to reuse yet.
- **OS authentication as an unlock option** (polkit, Touch ID, Windows
  Hello): planned, not implemented.
- **Phone:** the Flutter app has profiles through the bridge
  (`set_current_profile`, `open_library`, `unlock_library`). The same gate
  is planned there, with system biometrics (BiometricPrompt, Face ID / Touch
  ID) releasing a keystore-held key as an unlock option. The bridge API is
  unchanged in this change.
