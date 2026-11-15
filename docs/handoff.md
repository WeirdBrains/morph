# Handoff

Updated 2026-11-14 on the `gpui` branch. This file is the resume point for moving
Morph's desktop shell to GPUI. Michael's usage was under 1% of the weekly limit.
**Stage 1 (plan and docs) is done and pushed. Nothing else is started: Michael asked
to stop here and wait for his go-ahead before any Flutter code is removed or any
GPUI code is written.**

## Where things stand

- Branch `gpui` was created from `origin/main` at `e9da772` (`Update dependencies`).
  Michael's local `gpui` branch pointed at that same commit, so nothing of his was
  left out. `main` is untouched.
- This branch so far changes only documents, plus a throwaway spike kept as
  evidence in `docs/evidence/gpui-spike/` (not a workspace member, not built by
  `cargo build` at the root).
- The popup typing and repaint bug from the previous handoff (Flutter Linux web
  view) is not fixed on `main`. On this branch it goes away with the Flutter Linux
  web view itself; the spike shows the GPUI design does not have it (below).

## Decisions so far (Michael, 2026-11-14)

- **Desktop moves to GPUI; Flutter stays for mobile.** Linux, macOS and Windows get
  a Rust GPUI shell. Android, iOS and F-Droid keep the Flutter app,
  `morph_webview(_inapp)`, `morph-bridge` and `flutter_rust_bridge`.
- **Linux first.** The GPUI shell has to be fully usable on Linux before any macOS
  or Windows GPUI work. Those two are planned in the docs only.
- **The desktop shell is a component host.** Browsing is one component behind its
  own crates (`morph-browser` over `morph-webview`); chat, notes, projects and AI
  come as components the same way. `PRIORITIES.md` 1 item 2 is the backbone.
- **Dart is not an extension language.** It stays only as the mobile UI language.
  No Dart extension SDK, no Dart to JS/WasmGC, no "Rust first; Dart later".
- **Each phase ends with a Linux build Michael can run, including Arch
  packages**: `morph` built from the checkout (`makepkg -si` or `pacman -U`) and
  a `morph-bin` recipe for his TAUR server, uploaded only after he approves the
  phase.
- `PLAN.md` is not touched. `PRIORITIES.md` got one dated progress note only.

All of it is written up in `ARCHITECTURE.md`: sections 1 (shape), 2.0 (desktop web
view), 2a (components), 3 (crates), and the three 2026-11-14 entries at the end
(the switch, Dart, and the open decisions).

## What was checked (verified, not guessed)

On this box (Debian 13, Xvfb, `WAYLAND_DISPLAY` unset, webkit2gtk 2.54.0, Mesa
lavapipe Vulkan, Rust 1.100.0), with the spike in `docs/evidence/gpui-spike/`:

- crates.io `gpui` 0.2.2 (2025-10-22, the newest published) does not build with
  current `libc` (0.2.190+): `xattr` 0.2.3 under its `zed-async-tar` uses
  `libc::ENOATTR`, which Linux no longer has. With `xattr` patched it builds, but
  asking its X11 window for a raw handle panics (`unimplemented!()` in
  `src/platform/linux/x11/window.rs`), so wry cannot attach a child web view.
- GPUI from Zed main (`703b5ac3f4ffe1e87a073123ae56e22f72f095c4`, 2026-11-14,
  crates `gpui` + `gpui_platform`) builds (wgpu 30 renderer; Zed pins Rust 1.98.1)
  and implements the X11 and Wayland handles. wry 0.57.2 accepts only an Xlib
  handle, so the spike passes the same X11 window id as an Xlib handle.
- A GPUI window drew its own toolbar with a webkit2gtk child below it showing a
  local page. `xdotool` clicks and typing reached the page input (one title change
  per keystroke).
- `window.open` reached Morph's handler, which built a GTK popup with a web view
  related to the opener (`NewWindowResponse::Create`). Text typed in the popup's
  field showed with the caret in the field; a button that changes the heading
  repainted it; every keystroke also reached the opener by `postMessage`. That is
  the Flutter popup bug's reproduction (local fixture, no real account), passing.
- wry's default popup handling (`NewWindowResponse::Allow`) unwraps the top level
  as a `gtk::ApplicationWindow` and would panic for a child web view (read in
  `wry-0.57.2/src/webkitgtk/mod.rs`), so Morph must make its own popups.

Not checked: a real Wayland session (the plan uses XWayland there), NVIDIA, HiDPI,
IME, window resizing, real sites, macOS, Windows, mobile builds, and the CPU cost
of pumping GTK from a timer.

## The plan: phases, each ending in a Linux build to test

Every phase passes the quality bar before each commit (`cargo fmt --check`,
`cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo test`), ships
tests for new behaviour, runs GUI checks only under Xvfb (or a headless
compositor), and is pushed to `origin/gpui` only. Domain logic goes in `morph-*`
crates that test without a display; the GPUI crates stay thin.

**How Michael tests a phase on alien-blue** (Rust 1.99; Flutter 3.50 only matters
for the mobile app):

```sh
cd ~/Projects/WeirdBrains/morph
git fetch origin && git switch gpui && git pull --ff-only
sudo pacman -S --needed webkit2gtk-4.1 gtk3 libxkbcommon-x11 vulkan-icd-loader
export CARGO_TARGET_DIR=/mnt/ext1/cargo-target/morph
MORPH_HOME=$(mktemp -d) cargo run -p morph-desktop --release
```

**Every phase also ships Arch packages**, in the shape of Michael's existing
TAUR packages (`mouser-bin`, `muninn-bin`, `muninn-app-bin` under org
WeirdBrains, read from https://pkg.taur.weirdbrains.com on 2026-11-14):

- `packaging/arch/morph/PKGBUILD` builds from the checkout with cargo
  (`--locked --release`) for `makepkg -si`. `depends`: `webkit2gtk-4.1`, `gtk3`,
  `glib2`, `libxkbcommon-x11`, `vulkan-icd-loader`, `hicolor-icon-theme`,
  `glibc`, `gcc-libs`; `makedepends`: `cargo`, `git`, `clang`, `pkgconf`.
- `packaging/arch/morph-bin/PKGBUILD` installs the prebuilt app from a GitHub
  Release, like `muninn-app-bin`: `source_x86_64` (and later `source_aarch64`)
  `$url/releases/download/v$pkgver/morph-$pkgver-$CARCH-unknown-linux-gnu.tar.gz`,
  `sha256sums_*` filled from the release's `SHA256SUMS`, `provides=('morph')`,
  `conflicts=('morph')`, `options=('!strip' '!debug')`, the maintainer line
  and a header comment saying where the tarball comes from.
- Both install `/usr/bin/morph`, `/usr/share/applications/morph.desktop`
  (`Exec=morph %u`, http/https handlers), the icon from `docs/brand` in the
  hicolor theme, `LICENSE` and `README.md`, and each has a `.SRCINFO` made with
  `makepkg --printsrcinfo` next to it. CI fails if
  `makepkg --printsrcinfo | diff - .SRCINFO` shows a difference.
- Checked in an `archlinux:latest` container before a phase is called done:
  `makepkg` as a non-root user, `pacman -U`, `morph --version`, a start under
  Xvfb. CI builds `morph` the same way and attaches the `.pkg.tar.zst`.

```sh
cd packaging/arch/morph && makepkg -si     # from the checkout
sudo pacman -U morph-*-x86_64.pkg.tar.zst  # or the package CI attached
```

**Publishing each phase to TAUR (after Michael approves the phase).** The agent
does not run `taurctl` and does not create GitHub Releases. Once Michael
approves, the release tarball and `SHA256SUMS` go on a GitHub Release, the
`morph-bin` recipe gets that version and those sums, and a recipe archive
(`morph-bin/PKGBUILD` + `morph-bin/.SRCINFO`, as `morph-bin-<version>.tar.gz`)
is uploaded from Michael's machine with
`taurctl pkg upload morph-bin-<version>.tar.gz --org WeirdBrains`, dry-run
first, per the collective release rules. How phase builds are versioned and
tagged off `main` is open decision 10.

After each push to `origin/gpui`, Michael's checkout at
`~/Projects/WeirdBrains/morph` on alien-blue (branch `gpui`) is fast-forwarded
(`git pull --ff-only origin gpui`).

Or download the `morph-linux-x86_64` tarball that CI attaches to the branch run
(phase 1 adds the job and turns on CI for pushes to `gpui`), unpack and run
`./morph`. The package list is from Arch package names and is not yet checked on
alien-blue. On Hyprland the window runs through XWayland (`MORPH_ALLOW_WAYLAND=1`
will skip that once a native path exists).

### Phase 1 — GPUI shell on Linux with one working browser component

Flutter is untouched in this phase; both shells live side by side.

- `morph-ui`: component kinds, `Layout` (slots, sizes, visibility, saved per
  profile as JSON), named actions and a keymap with main's shortcuts
  (Ctrl+T, Ctrl+W, Ctrl+L, Ctrl+R, Ctrl+F, Ctrl+Tab, Ctrl+1..9), theme tokens from
  main's `morph_theme.dart`. Unit tests.
- `morph-core`: the address-bar rule ("phrases are searches", main's
  `omnibox.dart` and `searchUrl`) ported to Rust with main's `omnibox_test.dart`
  cases. The Flutter app keeps its Dart copy until phase 2 moves it onto the
  Rust one.
- `morph-webview`: `WebView` trait, events, the wry backend (X11 shim, GTK pump,
  Morph-made popups, a `WebContext` per profile with
  `morph_core::profiles::webkit_data_dir`), and an in-memory fake.
- `morph-browser`: tabs on `morph_core::tabs::TabSet`, address bar, back,
  forward, reload, stop; history recorded through `morph_core::library`. Tests
  drive it with the fake web view.
- `morph-desktop`: binary `morph`, `MORPH_HOME` honoured, GPUI on X11 when
  `DISPLAY` is set, the component host with the browser in the main slot.
- CI: a `desktop-linux` job (build, clippy, test, release tarball
  `morph-<version>-x86_64-unknown-linux-gnu.tar.gz` with `SHA256SUMS`) and an
  `arch-package` job (`archlinux:latest` container, `.SRCINFO` check,
  `makepkg`, `pacman -U`, package artifact); CI runs on pushes to `gpui`. The
  Flutter jobs stay as they are for now.
- `packaging/arch/morph` and `packaging/arch/morph-bin` (PKGBUILD + `.SRCINFO`
  each), the `.desktop` file and the icon, checked in an Arch container.
- Xvfb checks: open a page, type into it, open a popup and type into it, switch
  and close tabs, type a phrase and get a search.
- **Michael tests:** installs the Arch package (`makepkg -si` or `pacman -U`),
  then browse, tabs, address bar, sign-in popups (no real account needed for the
  check, but he may try his own).

### Phase 2 — Flutter desktop removed, shared logic moved down

- Remove `app/linux`, `app/macos`, `app/windows`, `packages/morph_webview_linux`,
  desktop-only Dart (window chrome, `window_manager`, desktop chrome widgets), the
  Flutter `linux` and `windows` CI jobs and `scripts/build-windows.ps1`.
- Move the logic in `morph-bridge` that both shells need (library open and unlock,
  identity, device linking, profile deletion side effects) into a toolkit-neutral
  `morph-services`, with its tests. `morph-bridge` keeps its function signatures,
  so the generated Dart bindings do not change.
- The mobile app moves onto the Rust address-bar rule through the bridge.
- `flutter analyze`, `flutter test`, the `morph_webview` and
  `morph_webview_inapp` package tests and the Android CI job stay green.
- **Michael tests:** the updated Arch package, plus the Android debug APK from CI.

### Phase 3 — Everyday browser parts on Linux

In `PRIORITIES.md` order as far as they apply: library component (bookmarks with
folders and tags, history, search), profiles (switch, create, rename, delete;
library unlock from the keystore, password prompt otherwise), settings component
(tab position, sidebar), find in page, crash and hang panel per tab, Morph error
pages, frameless window with custom controls (close-only on tiling window
managers), downloads to a chosen folder.
- **Michael tests:** the updated Arch package; daily browsing with bookmarks,
  history and two profiles.

### Phase 4 — Identity and links on Linux

Identity onboarding (create, restore from words or a backup file), recovery
phrase and backups, device linking (QR offer shown, SAS check), "Make default
browser" (`xdg-settings`), links from the command line into the running instance
(single instance), page bridge round trip.
- **Michael tests:** the updated Arch package; onboarding a fresh profile,
  linking with the phone app, opening links from other apps.

### Phase 5 — Linux packaging and the Wayland question

The remaining packages per the collective rules (deb, rpm, Nix flake, and every
format with and without the `.desktop` entry), the shared release script
(`scripts/weirdnet_release.py`, `release.toml`, its `taur` step for `morph-bin`), and, depending on Michael's call
on open decision 1, the native Wayland web view work.
- **Michael tests:** a release-style Arch package and the other formats.

### Later — macOS, Windows, more components

macOS (WKWebView child) and Windows (WebView2 child) through the same
`morph-webview` trait; chat, notes, projects and AI components as their domain
crates land.

## Open decisions for Michael

Listed in full in `ARCHITECTURE.md` (2026-11-14 — Open decisions from the GPUI
switch):

1. Native Wayland: accept XWayland for phase 1, or make native Wayland a gate?
2. GPUI pin: Zed repository by commit (recommended), or a WeirdBrains fork on
   crates.io?
3. Keep or drop the Linux texture/DMA-BUF/hidden-text-layer work (only useful
   later for native Wayland)?
4. `flutter_inappwebview` "other platforms" now means Android and iOS only.
5. App updates: RFW and Dart-patch items now concern mobile only.
6. Flutter engine fork: keep the BSD part, or plan BSD on GPUI?
7. GTK 4 / native X11 item in Priorities 2, given wry is GTK 3.
8. GPUI cannot draw over the page: popovers beside it, OK?
9. Apply the frameless-window rule to the GPUI window?
10. Phase builds and releases: the shared release script releases only from
    `main`, and `morph-bin` needs a GitHub Release per phase. Tag phase builds
    from `gpui` as pre-releases (for example `v0.2.0-gpui.1`, with `pkgver`
    `0.2.0.gpui.1`, since `pkgver` cannot contain `-`), or release only from
    `main` after a merge?

## How to resume

```sh
cd ~/Projects/WeirdBrains/morph
git fetch origin
git switch gpui            # or: git switch -c gpui --track origin/gpui
git pull --ff-only origin gpui
```

Start phase 1 only after Michael's go-ahead. Do not push to `main`. Do not touch
`~/.morph`. Do not print secrets. GUI only under Xvfb (or a headless compositor)
with `WAYLAND_DISPLAY` unset.

## History before the GPUI branch

The 2026-11-13 handoff (Windows release build `d30a211`/`e6c0c5e`, Linux
`Isolate.create` fix `005d01c`, the popup bug report) is in git:
`git show d73e240:docs/handoff.md`. The stash `stash@{0}` and the branches
`gpu-fix` and `linux-persistent-webview-session` mentioned there were left alone.
