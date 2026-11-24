# Handoff

The running log of finished work on Morph, newest first. Each finished part adds a dated entry in the same push: what changed, the commits, the checks run and what wasn't tested, what's left, and what's next. This file follows the public-content rules (no host names, IPs or home paths; m3talsmith only; links are full GitHub URLs).

Morph's agent work has its own handoff doc: [docs/handoff.md](docs/handoff.md). It is kept separately and not duplicated here.

Repo: https://github.com/WeirdBrains/morph. A [Weird Brains](https://weirdbrains.com) project.

## 2026-11-24

### Profile lock follow-ups 1: locked profiles marked in the profiles panel

- **What changed:** the profiles panel (desktop) marks a profile as "Locked" when opening it would ask for its password: it has an identity, the password is on, and it is not open in this run. A profile whose password is off opens from the keystore and is not marked. Switching to a locked one shows the lock screen as before. Commit [`f0e1a07`](https://github.com/WeirdBrains/morph/commit/f0e1a0786efcf1bf1f43bc00614f9275de2d696b) (`crates/morph-desktop/src/profiles.rs` `shows_locked`, `profiles_view.rs`); [docs/profile-lock.md](https://github.com/WeirdBrains/morph/blob/main/docs/profile-lock.md) updated.
- **Checks:** `cargo fmt --check`, `cargo clippy --locked -p morph-desktop --all-targets -- -D warnings`, and the new `profiles::tests::locked_profiles_show_as_locked` (no identity, just onboarded, locked, unlocked again, password off). CI and iOS for this push are in the Actions tab. Not tested: the panel by eye (no screenshot taken), the phone (it has no lock gate yet).
- **Left:** the phone's profile list, once the phone has the lock (item 4).
- **Next:** auto-lock after idle, then OS-authentication unlock, then the phone lock with biometrics.

## 2026-11-23

### v0.3.0-preview.2 (pre-release)

- **What changed:** the second 0.3 preview, cut from main at [`bc41a7f`](https://github.com/WeirdBrains/morph/commit/bc41a7f031ca) after CI and iOS passed there. Tag `v0.3.0-preview.2` points at [`c887255`](https://github.com/WeirdBrains/morph/commit/c8872558a565). Release: https://github.com/WeirdBrains/morph/releases/tag/v0.3.0-preview.2 (pre-release, not Latest). Lead items: links from a terminal hand control back (`gh auth refresh` no longer hangs when Morph was not running), per-type sync toggles, the password saver on for profiles with an identity, and `weirdnet-device-link` 0.4.2. Phone build number 10410. Assets are CI's builds of the release commit (Linux, macOS, Windows, Android debug APK); the signed `android-arm64` APK is attached by the Android release workflow. This commit moves the Arch recipes (`morph-bin`, `morph`, `.SRCINFO`) and `packaging/nix/release.json` to the preview.
- **Checks:** every platform asset and checksum (`scripts/weirdnet_release.py assets 0.3.0-preview.2`), `taur-check` for `morph-bin`. Not tested: installing the TAUR package on Arch, the APK on real phones.
- **Next:** history sync, then the profile lock follow-ups (auto-lock, OS-authentication unlock, mobile biometrics).

## 2026-11-22

### Links from a terminal hand control back (gh auth refresh)

- **What changed:** with Morph as the default browser and no Morph running, `gh auth refresh` (and anything else that opens the browser and waits for that command: `xdg-open`, `$BROWSER`, `morph URL`) stayed stuck until the Morph window closed. The launch became the browser itself, and gh runs the browser command and waits for it to exit before it polls GitHub for the device code. Now a first launch with a URL starts Morph as a process of its own (a new session, output to `morph.log` in the Morph home), waits until it listens for links, and exits 0 (or with Morph's exit code and the log's tail if it failed to start). A later launch already handed its URL to the running Morph and exited. A launch without a URL stays in the foreground, and `MORPH_FOREGROUND=1` keeps the old behavior. Linux and macOS; Windows is unchanged. Loopback OAuth callbacks themselves were fine: http://127.0.0.1 and http://localhost addresses open as typed (no search fallback, no https upgrade, no blocking), and a redirect or form POST to them loads. Commits [`c29db8c`](https://github.com/WeirdBrains/morph/commit/c29db8c344cd22d2c5169da60951fe58b34c8e76) (the fix, `crates/morph-desktop/src/launch.rs`), [`defeede`](https://github.com/WeirdBrains/morph/commit/defeedeb683336f5db7ebf6f9d19e590c4571f99) (Xvfb check; harnesses that read the first Morph's output set `MORPH_FOREGROUND=1`) and [`779028a`](https://github.com/WeirdBrains/morph/commit/779028ac80715b9024206d87c5c1c10c9a6237b6) (the Arch start check in CI does too).
- **Checks:** unit tests in `launch.rs` (returns once the new Morph listens and leaves it running in its own session, reports an early exit code, gives up waiting on a slow start). New Xvfb group "terminal link" (`scripts/xvfb-check.py --only terminal-link`, also in the full suite): a first launch with a loopback authorize URL exits 0 with its pipes closed, the browser runs on in its own session, and the page follows a 302 from 127.0.0.1 to localhost and a form POST back to 127.0.0.1 with the query string intact. It passes on X11 and headless Wayland locally; with `MORPH_FOREGROUND=1` (the old behavior) the launch never returns. `xdg-open` in the generic desktop mode returns in under a second for the first link and the ones after it. `cargo fmt --check` and `cargo clippy -p morph-desktop --all-targets -- -D warnings` are clean. Not tested: a real `gh auth refresh` (no real account was signed in), and macOS outside CI.
- **Left:** nothing for this bug. Michael needs a build with the fix (the next release) for `gh auth refresh` to return on its own.
- **Next:** history sync, then the profile lock follow-ups (auto-lock, OS-authentication unlock, mobile biometrics).

### Password saver on by default for profiles with an identity

- **What changed:** the password saver is on as soon as a profile gets a Weirdnet identity (onboarding, restore or a device link) and stays off without one. Commit [`aa1a87b`](https://github.com/WeirdBrains/morph/commit/aa1a87bf30fc93f263453d7923813a7dc01a4e54). The `MORPH_PASSWORD_SAVER` gate is gone: the desktop always injects the login script, shows the save/fill bar, Settings > Privacy > Passwords, and the address-menu Passwords row. The phone has Settings > Passwords (on/off, ask again, saved logins with show behind the profile password, copy and delete) and the address-sheet Passwords row opens it. Users can still turn it off per profile. Credentials stay in that profile's encrypted store; nothing is saved or filled while the profile is locked. Password values are never logged and companions still never read them back.
- **Checks:** `cargo fmt --check`; `cargo clippy --locked --all-targets -p morph-core -p morph-services -p morph-bridge -p morph-desktop -- -D warnings`; `cargo test --locked` for the password-saver tests in morph-core, morph-services, morph-bridge and morph-desktop; Flutter `passwords_page_test` and `settings_page_test`. `scripts/xvfb-check.py --only password-saver` (a temp `MORPH_HOME`) is in the script and in the main Xvfb suite; it will run in CI. Not tested locally on this push: the Xvfb check (the desktop binary was not rebuilt here), Windows Hello and macOS prompts on real hardware beyond CI; capture and fill in the phone's page; mobile biometrics.
- **Left:** capture and fill in the phone's page (Settings and the address-sheet row are in); mobile biometrics when the profile password is off; password sync (still waiting on weirdnet-sync).
- **Next:** history sync, then the profile lock follow-ups (auto-lock, OS-authentication unlock, mobile biometrics).

### weirdnet-device-link 0.4.2

- **What changed:** Morph now pins `weirdnet-device-link` =0.4.2 ([crates.io](https://crates.io/crates/weirdnet-device-link/0.4.2), tag [v0.4.2](https://github.com/WeirdBrains/weirdnet-device-link/releases/tag/v0.4.2)), the 0.4 line release of the generic helpers already used from 0.3.3. Commit [`d4a49d6`](https://github.com/WeirdBrains/morph/commit/d4a49d61c262b319384b30e5d80168a19ac62731) updates the workspace pin and lockfile (only this package), CI's `local-relay` install, and the current-version mentions in the README, changelog and [docs/library-sync.md](https://github.com/WeirdBrains/morph/blob/main/docs/library-sync.md). No further Morph device-link code was removed: the crate already provided those helpers in 0.3.3, and Morph's wrappers (`OFFER_SCHEMES`, `OfferApp`, `device_label`, `link_error`, `LinkState`, `NewDevice`, `AddDevice`, `Invite`, `has_linked_node`, the `Links` registry, `drop_later` and `check_invite`) stay here. The LinkSettings roster snapshot also stays in Morph; it is not in 0.4.2 and belongs in `weirdnet-sync`.
- **Checks:** `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` and `cargo test --locked` (whole workspace, headless) pass locally; `cargo metadata --locked` accepts the lockfile. CI and iOS results for this push are in the Actions tab. Not tested: linking two real devices through the public relay, and the Flutter apps beyond CI.
- **Left:** the LinkSettings roster snapshot belongs in `weirdnet-sync` and needs its own release.
- **Next:** history sync, then the profile lock follow-ups (auto-lock, OS-authentication unlock, mobile biometrics). The password saver (on once a profile has an identity) and per-type sync toggles are also approved and follow this pin.

### Settings: a sync toggle for each type

- **What changed:** Settings > Sync on the desktop and the phone has a switch for each type this device sends and receives for the profile (bookmarks, history, settings, companions, companion permissions, chats, MCP servers, MCP secrets, journal). The choices are per profile and stay on this device; they are not a synced setting. Turning a type off stops sending and receiving it and does not delete data already here; turning it back on backfills local rows and pulls what the other devices have. New links still start with every type on, even if the device that sent the invite had turned some off. Locked profiles cannot change the list. Docs: [docs/library-sync.md](https://github.com/WeirdBrains/morph/blob/main/docs/library-sync.md).
- **Checks:** `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` and `cargo test --locked` (whole workspace, including a two-home local-relay test that turns history off and back on), plus the Flutter widget tests for the Sync section. Xvfb checks on two linked homes turn History off on one device and confirm the other keeps it on. CI and iOS results for this push are in the Actions tab once they finish. Not tested: toggling every companion type between two real devices, or a type that is not in this build yet (open tabs, saved passwords).
- **Left:** open tabs and saved passwords have no switch until those types exist. The Settings search "sync" also still matches Linked devices (it has "sync" as a keyword).
- **Next:** history sync polish if anything comes back from the preview, then the profile lock follow-ups (auto-lock, OS-authentication unlock, mobile biometrics).

### Weirdnet pins: clean patch releases and weirdnet-device-link 0.3.3

- **What changed:** Morph now pins `weirdnet-net` =0.8.2, `weirdnet-sync` =0.7.1, `weirdnet-text` =0.1.1, `weirdnet-device-link` =0.3.3, `weirdnet-keys` =0.1.2, `weirdnet-schemes` =0.1.1 and `weirdnet-storage` =0.1.1, replacing versions withdrawn from crates.io; `Cargo.lock` changes only those seven packages ([`228c70b`](https://github.com/WeirdBrains/morph/commit/228c70b335863b0f191234b450dcbb48dee06a67)). `weirdnet-storage` 0.1.1 moved vendored OpenSSL behind a default `vendored-openssl` feature, so `morph-store` turns it on to keep SQLCipher and OpenSSL compiled from source. CI's local relay is installed from device-link 0.3.3. Morph's generic device-link code now comes from the crate ([`59bcf9d`](https://github.com/WeirdBrains/morph/commit/59bcf9dd99d62702ca4ca7b033caf26aed92f5bf)): `PUBLIC_RELAYS` is gone (use `known_relays()`), `offer_scheme`/`is_offer` wrap the crate's `offer_scheme` over `OFFER_SCHEMES`, `net_config` is `link_node_config("morph-link", link_relays())`, and `link_relays` is `relays_from_env("MORPH_LINK_RELAYS")`. The scheme constants, `OfferApp`, `device_label`, `link_error`, `LinkState`, `NewDevice`, `AddDevice`, `Invite`, `has_linked_node`, the `Links` registry, `drop_later`, `check_invite` and the LinkSettings snapshot stay in Morph. One behavior difference: a `MORPH_LINK_RELAYS` entry without a `/p2p/<relay>` peer id is now dropped (it could never hold a relay reservation).
- **Checks:** `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` and `cargo test --locked` (whole workspace, headless) pass locally; `cargo metadata --locked` accepts the lockfile. CI and iOS results for this push are in the next entry or the Actions tab. Not tested: linking two real devices through the public relay, and the Flutter apps beyond CI.
- **Left:** device-link main has the same additions unreleased (0.4.2 needs Michael's OK); the LinkSettings roster snapshot belongs in `weirdnet-sync` and needs its own release.
- **Next:** history sync, then the profile lock follow-ups (auto-lock, OS-authentication unlock, mobile biometrics).

## 2026-11-21

### Docs: library sync status brought up to date

- **What changed:** [docs/library-sync.md](https://github.com/WeirdBrains/morph/blob/main/docs/library-sync.md) now reads settings sync and Settings > Linked devices as done (milestones 1–6), lists the current pins (`weirdnet-sync` 0.7.0, `weirdnet-net` 0.8.1, `weirdnet-device-link` 0.3.1), and drops the stale "no cert on the first device" and "settings once that milestone ships" wording. The default opt-ins decision now says what the code does: every supported type is on at link, and no Settings toggle for per-type sync exists in the desktop or phone UI (only `morph_core::sync::set_types` in code); Linked devices lists and revokes only. Docs only.
- **Checks:** claims checked against `crates/morph-core/src/sync.rs`, `crates/morph-services/src/sync.rs` and the Settings view. No code changed, so no new tests.
- **Left:** a per-type sync toggle or a prompt-first policy is still an open decision; open tabs on other devices are not built.
- **Next:** upstream Morph's `weirdnet-device-link` additions into the Weirdnet crate (needs a crate release, which needs Michael's OK).

## 2026-11-20

### Site: signed arm64 APK added to the preview

- **What changed:** the preview section on the [download page](https://morph.weirdbrains.com/download#preview) now lists the signed `morph-0.3.0-preview.1-android-arm64.apk` (23.4 MB) ahead of the debug APK, and the "signed APK is on its way" sentence is gone. Commit [`0e592fd`](https://github.com/WeirdBrains/morph/commit/0e592fd91f89f7b554bb815436515aaba724feb9), site files only.
- **Checks:** `jaspr build` passes; the APK is attached to the [pre-release](https://github.com/WeirdBrains/morph/releases/tag/v0.3.0-preview.1). Live link checks run after the Pages deploy. Not tested: installing the APK.
- **Left:** nothing open on the site for this preview.
- **Next:** move the preview to the stable section when v0.3.0 ships.

### Site: v0.3.0-preview.1 listed as a preview

- **What changed:** the [Morph site](https://morph.weirdbrains.com) has a labeled Preview section on the [download page](https://morph.weirdbrains.com/download#preview) with the files that are on the [pre-release](https://github.com/WeirdBrains/morph/releases/tag/v0.3.0-preview.1), a note on the TAUR `morph-bin` preview (0.3.0preview1-1), and a line that previews may change and linking devices needs v0.3 on both. The home page links "Preview: 0.3.0-preview.1" and the v0.3 roadmap note mentions it; v0.3 items stay In progress. 0.2.3 is still the stable download. Commit [`85dc564`](https://github.com/WeirdBrains/morph/commit/85dc564d275eae4ce56f4a4b552d0db18159af8d), site files only.
- **Checks:** `jaspr build` passes. Live page and link checks run after the Pages deploy. Not tested: installing the preview packages.
- **Left:** the signed arm64 APK was not attached yet, so the site links only the debug APK and says the signed one is on its way.
- **Next:** add the signed arm64 APK to the preview list once it is attached.

### v0.3.0-preview.1 (pre-release)

- **What changed:** the first 0.3 preview, cut from main after the Android address-sheet fix ([`4013307`](https://github.com/WeirdBrains/morph/commit/40133076c0805a9019c810e89d5aa07b5eab1a97)). Tag `v0.3.0-preview.1` points at [`b20fd31`](https://github.com/WeirdBrains/morph/commit/b20fd315dde3fa8089e2596df537d70d574ccf1e). Release: https://github.com/WeirdBrains/morph/releases/tag/v0.3.0-preview.1 (pre-release, not Latest). Packaging moved to the preview in [`dba4b03`](https://github.com/WeirdBrains/morph/commit/dba4b03ab5393946a9b83cc3615355199126cb09) (PKGBUILDs, `.SRCINFO`, `packaging/nix/release.json`) because the tagged tree still held the 0.2.3 recipe. TAUR `morph-bin` is at `0.3.0preview1-1`; the PKGBUILD maintainer line is Weird Brains LLC. The signed `android-arm64` APK is attached by the Android release workflow (run https://github.com/WeirdBrains/morph/actions/runs/44861530556); with it the release has 13 assets.
- **Checks:** CI https://github.com/WeirdBrains/morph/actions/runs/44856203143 and iOS https://github.com/WeirdBrains/morph/actions/runs/44856203047 passed on [`b20fd31`](https://github.com/WeirdBrains/morph/commit/b20fd315dde3fa8089e2596df537d70d574ccf1e). The release checks every platform asset and its checksum (`scripts/weirdnet_release.py assets 0.3.0-preview.1`). The TAUR recipe check and dry run passed before publishing. Not tested: installing the TAUR package on Arch, and the APK on real phones beyond the address-sheet fix.
- **Left:** the site's latest-version and download pages; pinging Morph intern and the Weirdnet lead with the release link. `SHA256SUMS` lists the debug APK but not the arm64 APK, same as 0.2.3.
- **Next:** feedback from the preview; see the open items in the entries below.

### Password saver (hidden)

- **What changed:** system authentication (polkit, LocalAuthentication, Windows Hello or CredUI), saving and filling logins only on the exact site they came from, and Settings > Privacy > Passwords with the saved-logins list. Commits [`d800089`](https://github.com/WeirdBrains/morph/commit/d8000899e6ccf8a2e408a35ba8de0f0ca40493c1) through [`3ae31cb`](https://github.com/WeirdBrains/morph/commit/3ae31cb98b573928582f64f5a819a8b5e8a665d5). It is hidden unless Morph starts with `MORPH_PASSWORD_SAVER=1`; the address-menu Passwords row follows the same gate ([`0950055`](https://github.com/WeirdBrains/morph/commit/0950055ffadec61150ddaa979a0ec58a76ce9d15)).
- **Checks:** CI passed on all platforms. Not tested: Windows Hello and macOS prompts on real hardware beyond CI smoke runs.
- **Left:** the gate decision (when to turn the saver on by default), and mobile biometrics.
- **Next:** decide the gate, then mobile biometrics.

### Weirdnet upgrade

- **What changed:** weirdnet-net 0.8.1 and weirdnet-device-link 0.3.1 ([`62bd4d3`](https://github.com/WeirdBrains/morph/commit/62bd4d307c53b5ea9b09bca3dfb92fcb9642a145)), weirdnet-text 0.1.0 ([`8fe633e`](https://github.com/WeirdBrains/morph/commit/8fe633e9d07fad1f542d2c277a5a39a0a714457c)), weirdnet-sync 0.7.0 ([`7154b0d`](https://github.com/WeirdBrains/morph/commit/7154b0df2c8a7e4d84e9814159efb5b4dcb5ac17)), changelog in [`aebb1a9`](https://github.com/WeirdBrains/morph/commit/aebb1a9a1b3e7f426ce550f9ea461ed2e3698daf). Agent, model and mcp pins unchanged (Morph Dev owns them).
- **Checks:** CI passed on every platform, iOS included. Not tested: linking service nodes across devices on mixed sync versions.
- **Left:** profile lock leftovers (locked profiles in the picker, auto-lock after idle, unlock with OS authentication, mobile lock screen with biometrics).
- **Next:** relays, AnchoredPopup, then the v0.3 backend (shares, DDNS, credentials, trash, autofill). Upgrade every device before linking service nodes.

### v0.2.3

- **What changed:** patch release from the 0.2.x branch so packages show Weird Brains LLC as maintainer: [`54e4aed`](https://github.com/WeirdBrains/morph/commit/54e4aede361a8aef2323bc4b103c9b545a82d2ed) (release), [`8d6bb21`](https://github.com/WeirdBrains/morph/commit/8d6bb2180b417e763cbb7ad3b13b05acd6bd795c) (packaging), [`ca62c48`](https://github.com/WeirdBrains/morph/commit/ca62c48c0ea8f9f7da6c95c7da31b511a40736d1) (changelog). https://github.com/WeirdBrains/morph/releases/tag/v0.2.3, TAUR `morph-bin` 0.2.3-1.
- **Checks:** CI green, 13 assets verified. Not tested: upgrade from older packages.
- **Left:** nothing open.
- **Next:** none planned for 0.2.x.

### Windows evidence screenshot

- **What changed:** retaken on a fresh demo profile with no names, paths or host details: [`af6309a`](https://github.com/WeirdBrains/morph/commit/af6309aa7231c0c53f55c5cf9f2f21072f83df3b) (`docs/evidence/windows-webview.png`). The older shots were removed.
- **Checks:** reviewed by eye before commit. Not tested: n/a.
- **Left:** a macOS evidence screenshot, left out on purpose for now.
- **Next:** retake it in a clean virtual session when needed.

### Menu and settings "Coming soon" items hidden

- **What changed:** removed placeholder rows so the preview shows nothing unfinished: [`63123f3`](https://github.com/WeirdBrains/morph/commit/63123f35790dadc4f5abcc099a760c17809ea385) (Settings > Agents MCP row), [`0950055`](https://github.com/WeirdBrains/morph/commit/0950055ffadec61150ddaa979a0ec58a76ce9d15) (address menu Extensions and More; Passwords gated), [`544bd3a`](https://github.com/WeirdBrains/morph/commit/544bd3a057c15be93d50a930825442b99f5e496d) (Privacy "site permissions" note).
- **Checks:** fmt, clippy and desktop tests passed; CI green.
- **Left:** restore these rows when the features land.
- **Next:** see the Extensions and v0.3 backend work above.
