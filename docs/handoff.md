# Handoff

Updated 2026-11-14. This file is the resume point for moving Morph's desktop shell to
GPUI. **Phase 1 is done and pushed to `main`**, with pre-release `v0.2.0-alpha.1`
(https://github.com/WeirdBrains/morph/releases/tag/v0.2.0-alpha.1) and the `morph-bin`
recipe for TAUR. Michael said to keep going through the phases without waiting for
him; phase 2 is next.

## Where things stand

- The `gpui` branch was merged into `main` (fast-forward, `fbb5af9`) on 2026-11-14.
  **All work is committed and pushed straight to `origin/main`, no pull requests.**
  Another worker pushes the website (`site/`) to `main`, so always
  `git pull --rebase origin main` before pushing; never force-push. The remote
  `gpui` branch is left as it was.
- Phase 1 (below) is in `main`: crates `morph-ui`, `morph-webview`,
  `morph-browser`, `morph-desktop` (binary `morph`), the Arch recipes, the Xvfb key
  checks and the CI jobs. Flutter is untouched; both shells live side by side
  until phase 2.
- The throwaway spike from planning stays in `docs/evidence/gpui-spike/` (not a
  workspace member).
- Builds are tagged `v0.2.0-alpha.N` from `main` (chosen over `v0.2.0-gpui.N`
  since the work is on `main` now), `pkgver` `0.2.0alphaN`. Only `morph-desktop`
  carries that version.
- CI on `main`: the `flutter` and `android` jobs were already failing on
  `fbb5af9` before phase 1 (a password-field widget test, and the Android Gradle
  Plugin 9.1.0 being below Flutter's 9.1.1 minimum). Phase 1 leaves Flutter alone,
  so they still fail; phase 2 touches that code.

## Phase 1 result: what works and how it was checked

On this box (Debian 13, Xvfb, `WAYLAND_DISPLAY` unset, openbox, webkit2gtk 2.54,
Mesa lavapipe Vulkan, Rust 1.100), release build of `v0.2.0-alpha.1`:

- **Window and browser component.** A 1200x800 GPUI window (`app_id` `morph`)
  draws a tab strip, a toolbar (back, forward, reload/stop, address bar) and the
  page below as a webkit2gtk child window. Tabs open, close and switch (buttons
  and keys); the address bar edits text with the mouse and keys, phrases become
  searches (main's rule and test cases, `morph_core::omnibox`); back, forward,
  reload and stop follow the page; history goes into the profile's library; a
  crashed page shows a reload panel in its tab.
- **Popups** (`window.open`) are GTK windows Morph makes, related to the opener
  (same session, `postMessage` works); their title follows the page, F5 and
  Ctrl+R reload them, Ctrl+W closes them, and every other key goes to their page.
- **Keys.** `scripts/xvfb-check.py` starts the binary in its own Xvfb with
  openbox and a temporary `MORPH_HOME`, loads `tests/xvfb/keys.html` (a page that
  puts every keydown, the focused field, the field values, the caret and the
  scroll position in its title), and drives it with `xdotool`. 72 of 72 pass:

| Where | Check | Result |
|---|---|---|
| shell | window opens | pass |
| shell | page loads | pass |
| page | click focuses the input | pass |
| page | typing | pass |
| page | typing with the pointer over the shell | pass |
| page | Backspace | pass |
| page | Home in a field | pass |
| page | Delete | pass |
| page | End in a field | pass |
| page | Left arrow | pass |
| page | Right arrow | pass |
| page | Ctrl+A, Tab moves focus | pass |
| page | Ctrl+C / Ctrl+V | pass |
| page | Shift+Tab moves focus back | pass |
| page | Ctrl+X | pass |
| page | Ctrl+Z | pass |
| page | Enter reaches the page | pass |
| page | Escape reaches the page | pass |
| page | Ctrl+F reaches the page (no find bar yet) | pass |
| page | keypad key, Num Lock off | pass |
| page | keypad digits and +, Num Lock on | pass |
| page | keypad Enter | pass |
| page | Page Down scrolls | pass |
| page | Page Up scrolls back | pass |
| page | Space scrolls down | pass |
| page | Shift+Space scrolls up | pass |
| page | Down arrow scrolls | pass |
| page | Up arrow scrolls | pass |
| page | End scrolls to the bottom | pass |
| page | Home scrolls to the top | pass |
| page | F5 reloads | pass |
| page | Ctrl+R reloads | pass |
| shell | Ctrl+T opens a tab from the page | pass |
| shell | the new tab's address bar takes typing | pass |
| shell | Ctrl+Tab switches tabs | pass |
| page | page never saw Ctrl+T / Ctrl+Tab | pass |
| shell | Ctrl+L, then a phrase searches | pass |
| shell | Ctrl+W closes the tab (tab 2 shows again) | pass |
| shell | F11 toggles full screen | pass |
| shell | F11 again leaves full screen | pass |
| popup | window.open makes a popup window | pass |
| popup | click focuses the input | pass |
| popup | typing | pass |
| popup | Backspace | pass |
| popup | Home in a field | pass |
| popup | Delete | pass |
| popup | End in a field | pass |
| popup | Left arrow | pass |
| popup | Right arrow | pass |
| popup | Ctrl+A, Tab moves focus | pass |
| popup | Ctrl+C / Ctrl+V | pass |
| popup | Shift+Tab moves focus back | pass |
| popup | Ctrl+X | pass |
| popup | Ctrl+Z | pass |
| popup | Enter reaches the page | pass |
| popup | Escape reaches the page | pass |
| popup | Ctrl+F reaches the page (no find bar yet) | pass |
| popup | keypad key, Num Lock off | pass |
| popup | keypad digits and +, Num Lock on | pass |
| popup | keypad Enter | pass |
| popup | Page Down scrolls | pass |
| popup | Page Up scrolls back | pass |
| popup | Space scrolls down | pass |
| popup | Shift+Space scrolls up | pass |
| popup | Down arrow scrolls | pass |
| popup | Up arrow scrolls | pass |
| popup | End scrolls to the bottom | pass |
| popup | Home scrolls to the top | pass |
| popup | F5 reloads | pass |
| popup | Ctrl+R reloads | pass |
| popup | popup state reached the shell | pass |
| popup | Ctrl+W closes the popup | pass |

  The page rows run in the main window, the popup rows in a popup opened from the
  page. "page never saw Ctrl+T / Ctrl+Tab" reads the page's key log: shell keys
  are taken before the page. Ctrl+F goes to the page until find in page exists
  (phase 3). CI runs the same script on Ubuntu 24.04 (`desktop-linux` job) and
  attaches the report as `xvfb-checks.md`.
- **Arch package.** In an `archlinux:latest` container (podman): `makepkg
  --printsrcinfo` matches both `.SRCINFO` files; `makepkg -s` for
  `packaging/arch/morph` built it from the checkout (`cargo build --locked
  --release`), `pacman -U` installed it, `morph --version` printed `morph
  0.2.0-alpha.1`, `pacman -Ql` listed the binary, desktop file, icons, licence and
  readme, and the installed `morph` opened its window under Xvfb. `morph-bin`
  was checked against the published release: the download passed its sha256,
  `makepkg` and `pacman -U` worked, `morph --version` printed `0.2.0-alpha.1`, and
  the window opened under Xvfb.
- **Quality bar** before each commit: `cargo fmt --check`, `cargo build`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
  New tests: `morph-ui` (layout, keymap, text field), `morph-core::omnibox`,
  `morph-browser` (17, against the fake web view), `morph-desktop` (key routing,
  action mapping, UTF-16 offsets).

**Not checked:** a real Wayland session through XWayland (Hyprland on
alien-blue), NVIDIA, HiDPI scaling, IME input, window resizing by hand, real
sites and real sign-in popups, a long session's CPU cost of pumping GTK every
8 ms, and Michael's Arch box itself (packages were checked in a container only).

### How the keys are routed (X11)

- The page is a child X window of the shell's window. X11 gives a key to the
  window under the pointer when the focused window is its ancestor, so:
  - **Shell has focus** (address bar, after Ctrl+T or Ctrl+L, a click on the tab
    strip or toolbar): the shell's window has X focus, and the page's GTK window
    hands any key it gets on to the shell's window (`XSendEvent`, `HostKeys` in
    `morph-webview`).
  - **Page has focus** (a click into the page, Escape in the address bar, a
    submitted address): the web view's own X window gets X focus, so keys reach
    the page wherever the pointer is.
- While a page has focus, its web view reports the keys the shell uses (Ctrl+T,
  Ctrl+W, Ctrl+L, Ctrl+R, F5, Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+1..9, Alt+Left/Right,
  F11, Ctrl+Q) and does not pass them to the page; everything else goes to the page.
- WebKitGTK does not bind Ctrl+Z / Ctrl+Shift+Z itself, so a small injected
  script runs undo/redo when the page did not handle the key.

## How Michael builds and installs on alien-blue

Arch, Rust 1.99 (the workspace's `rust-version` is 1.99, so that is the minimum).

```sh
cd ~/Projects/WeirdBrains/morph
git switch main && git pull --ff-only origin main
sudo pacman -S --needed base-devel webkit2gtk-4.1 gtk3 libxkbcommon-x11 libxcb \
  vulkan-icd-loader fontconfig freetype2 openssl clang cmake pkgconf

# 1. Run from the checkout without installing (a throwaway profile):
export CARGO_TARGET_DIR=/mnt/ext1/cargo-target/morph
MORPH_HOME=$(mktemp -d) cargo run -p morph-desktop --release

# 2. Or build and install the Arch package from the checkout:
cd packaging/arch/morph && makepkg -si      # morph 0.2.0alpha1-1
morph                                        # or "Morph" in the launcher

# 3. Or the prebuilt release from TAUR (once uploaded):
#    install morph-bin from the WeirdBrains TAUR repo; it conflicts with morph.
```

`makepkg` builds into `packaging/arch/morph/src/target` (its own target dir, not
`/mnt/ext1`), so the first build takes a while. Without `MORPH_HOME` the
installed `morph` uses the normal profile directory. On Hyprland it runs
through XWayland (needs `xorg-xwayland`, which Hyprland normally has).

## Release and TAUR (phase 1)

- Pre-release `v0.2.0-alpha.1` on GitHub, from `main`, with
  `morph-0.2.0-alpha.1-x86_64-unknown-linux-gnu.tar.gz` and `SHA256SUMS`. The
  tarball holds `bin/morph`, `share/applications/morph.desktop`, the hicolor
  icons, `LICENSE` and `README.md` (`scripts/linux-dist.sh`).
- `packaging/arch/morph-bin` points at that tarball with its sha256 filled in.
  The recipe archive (`morph-bin/PKGBUILD` + `morph-bin/.SRCINFO`) is
  `morph-bin-0.2.0alpha1.tar.gz`, uploaded from Michael's side with
  `taurctl pkg upload morph-bin-0.2.0alpha1.tar.gz --org WeirdBrains` (dry run
  first). The agent does not run `taurctl`.
- CI on `main`: `check` (fmt, clippy, tests, on Ubuntu 24.04 with the desktop
  libraries), `desktop-linux` (tarball, Xvfb key checks, artifacts) and
  `arch-package` (`.SRCINFO` check, `makepkg`, `pacman -U`, `--version`, start
  under Xvfb, the `.pkg.tar.zst` as an artifact), next to the existing jobs.

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

## What the planning spike checked (before phase 1)

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
compositor), and is pushed straight to `origin/main` after
`git pull --rebase origin main` (no pull requests, no force-push). Domain logic goes in `morph-*`
crates that test without a display; the GPUI crates stay thin.

**How Michael tests a phase on alien-blue:** see "How Michael builds and
installs on alien-blue" above.

**Every phase also ships Arch packages**, in the shape of Michael's existing
TAUR packages (`mouser-bin`, `muninn-bin`, `muninn-app-bin` under org
WeirdBrains, read from https://pkg.taur.weirdbrains.com on 2026-11-14):

- `packaging/arch/morph/PKGBUILD` builds from the checkout with cargo
  (`--locked --release`) for `makepkg -si`. `depends`: `webkit2gtk-4.1`, `gtk3`,
  `glib2`, `libxkbcommon-x11`, `vulkan-icd-loader`, `hicolor-icon-theme`,
  `glibc`, `gcc-libs`; `makedepends`: `cargo`, `git`, `clang`, `pkgconf`.
- `packaging/arch/morph-bin/PKGBUILD` installs the prebuilt app from a GitHub
  Release, like `muninn-app-bin`: `source_x86_64` (and later `source_aarch64`)
  `$url/releases/download/v$_ver/morph-$_ver-$CARCH-unknown-linux-gnu.tar.gz`
  (`_ver` is the tag's version, `0.2.0-alpha.1`; `pkgver` is `0.2.0alpha1`),
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

**Publishing each phase to TAUR.** The agent tags the phase build as a GitHub
pre-release from `main` (`v0.2.0-alpha.N`) with the tarball and `SHA256SUMS`, fills
the `morph-bin` recipe's version and sums, and leaves the recipe archive
(`morph-bin/PKGBUILD` + `morph-bin/.SRCINFO`, as `morph-bin-<pkgver>.tar.gz`) for
upload from Michael's side with `taurctl pkg upload ... --org WeirdBrains`, dry
run first. The agent does not run `taurctl`. After each push, Michael's checkout
at `~/Projects/WeirdBrains/morph` on alien-blue (branch `main`) is fast-forwarded
(`git pull --ff-only origin main`). CI also attaches the tarball and the Arch
package to each `main` run.

### Phase 1 — GPUI shell on Linux with one working browser component (done)

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
  `makepkg`, `pacman -U`, package artifact); CI runs on pushes to `main` and
  on pull requests. The Flutter jobs stay as they are for now.
- `packaging/arch/morph` and `packaging/arch/morph-bin` (PKGBUILD + `.SRCINFO`
  each), the `.desktop` file and the icon, checked in an Arch container.
- Xvfb checks: open a page, type into it, open a popup and type into it, switch
  and close tabs, type a phrase and get a search; every key Michael listed, in
  pages and popups (results above).
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
switch). Decided for phase 1 (`ARCHITECTURE.md`, 2026-11-14 — Phase 1 decisions):

1. Native Wayland: **XWayland for now**; native Wayland stays future work.
2. GPUI pin: **Zed repository by commit** (`703b5ac`).
8. GPUI cannot draw over the page: **popovers beside it**.
10. Phase builds: **pre-releases from `main`**, `v0.2.0-alpha.N`, `pkgver`
    `0.2.0alphaN` (sorts correctly before a final 0.2.0).

Still open:

3. Keep or drop the Linux texture/DMA-BUF/hidden-text-layer work (only useful
   later for native Wayland)?
4. `flutter_inappwebview` "other platforms" now means Android and iOS only.
5. App updates: RFW and Dart-patch items now concern mobile only.
6. Flutter engine fork: keep the BSD part, or plan BSD on GPUI?
7. GTK 4 / native X11 item in Priorities 2, given wry is GTK 3.
9. Apply the frameless-window rule to the GPUI window? (Phase 3 plans it.)

## How to resume

```sh
cd /workspace/morph            # or ~/Projects/WeirdBrains/morph
git switch main && git pull --rebase origin main
```

Next is phase 2. Commit as Michael, run the quality bar before each commit,
`git pull --rebase origin main` before each push, push to `origin/main`; no pull
requests, no force-push, leave the remote `gpui` branch alone. Do not modify
`PLAN.md`; `PRIORITIES.md` gets dated progress notes only. Do not touch
`~/.morph`. Do not print secrets. GUI only under Xvfb with `WAYLAND_DISPLAY`
unset (`scripts/xvfb-check.py` starts its own). Do not run `taurctl`.

## History before the GPUI branch

The 2026-11-13 handoff (Windows release build `d30a211`/`e6c0c5e`, Linux
`Isolate.create` fix `005d01c`, the popup bug report) is in git:
`git show d73e240:docs/handoff.md`. The stash `stash@{0}` and the branches
`gpu-fix` and `linux-persistent-webview-session` mentioned there were left alone.
