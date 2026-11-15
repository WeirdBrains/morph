# Morph

Morph is a distributed web browser with local AI, built on a Rust 2024 core and
Weirdnet.

The desktop shell (Linux first, then macOS and Windows) is
[GPUI](https://gpui.rs), Zed's Rust UI framework; Flutter stays the mobile shell
(Android, iOS, F-Droid). Both shells sit on the same Rust crates. Linux has a
working GPUI shell since phase 1 (`v0.2.0-alpha.1`); the Flutter desktop app is
still in the tree until phase 2 removes it. See `ARCHITECTURE.md` (sections 1,
2.0 and 2a, and the 2026-11-14 decisions) and `docs/handoff.md` for the phases.

Planning documents: `PLAN.md`, `PRIORITIES.md`, and `ARCHITECTURE.md`.
Do not edit `PLAN.md`. `PRIORITIES.md` is the adopted priority list; update it
to reflect progress, but do not restructure it.

## Build

Rust 1.99+ and Flutter 3.50+ (Dart 3.14) are required. On my-desktop, point cargo at scratch
storage so `target/` never lands in `~/src`:

```sh
export CARGO_TARGET_DIR=<scratch-dir>/cargo-target/morph
cargo fmt --check
cargo build
cargo clippy --all-targets -- -D warnings
cargo test
```

The Flutter app lives in `app/`:

```sh
cd app
flutter pub get
flutter_rust_bridge_codegen generate   # after changing crates/morph-bridge/src/api
flutter analyze
flutter test
```

Data for tests and manual runs goes in a temp directory or `MORPH_HOME`, never
in a user's real `~/.morph`.

### Desktop shell on GPUI (Linux)

The `morph-desktop` crate builds the binary `morph`. Besides Rust it needs the
system libraries wry (webkit2gtk) and GPUI use. On Arch (my-desktop):

```sh
sudo pacman -S --needed webkit2gtk-4.1 gtk3 libxkbcommon-x11 libxcb vulkan-icd-loader \
  fontconfig freetype2 clang cmake pkgconf
export CARGO_TARGET_DIR=<scratch-dir>/cargo-target/morph
MORPH_HOME=$(mktemp -d) cargo run -p morph-desktop --release -- example.com
```

On Debian or Ubuntu the build packages are listed in `DESKTOP_DEPS` in
`.github/workflows/ci.yml`, plus a Vulkan driver (`mesa-vulkan-drivers` for
Mesa). On a Wayland session the shell runs through XWayland for now
(`ARCHITECTURE.md` section 2.0).

Packages and tests:

- `cd packaging/arch/morph && makepkg -si` builds and installs the package from
  the checkout; `packaging/arch/morph-bin` installs the prebuilt release
  (on the WeirdBrains TAUR server as `morph-bin`).
- `scripts/linux-dist.sh` builds the release tarball and `SHA256SUMS` in `dist/`.
- `scripts/xvfb-check.py --bin <path to morph>` drives the shell in Xvfb with
  xdotool (needs `Xvfb`, `xdotool`, and `openbox` for the F11 check): keys in
  pages and popups, tabs, the address bar, popups.

Windows (on a Windows machine with Visual Studio 2022's C++ workload,
Strawberry Perl, and `nuget.exe` on `PATH`):

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build-windows.ps1
```

That builds `app/build/windows/x64/runner/Release/morph.exe` and zips it to
`dist/morph-<version>-x86_64-pc-windows-msvc.zip`. The script points `TEMP`
at `C:\t\morph` for the build. SQLCipher compiles OpenSSL with `nmake`, and
the default user temp directory is long enough that `cl` cannot write the
object files (`C1083`). Running the app needs the WebView2 Runtime installed;
the build only needs the WebView2 SDK, which `nuget.exe` downloads.

## Layout

- `crates/morph-core` — version, event ticks, page-bridge message contract, shell rules (tabs, address bar, profiles, library)
- `crates/morph-identity` — paths, Weirdnet user and node identity helpers
- `crates/morph-store`, `crates/morph-keystore` — encrypted library database and its key
- `crates/morph-webview-control` — in-process gRPC control layer for web views
- `crates/weirdnet-device-link` — device linking, moving to its own Weirdnet repo
- `crates/morph-bridge` — thin flutter_rust_bridge surface (mobile shell)
- `crates/morph-ui` — UI component model without a toolkit: kinds, layout slots, actions, keymap, text field, theme
- `crates/morph-webview` — web view trait and events, the wry/webkit2gtk backend, a fake for tests
- `crates/morph-browser` — the browser component: tabs, address bar, navigation, history
- `crates/morph-desktop` — the GPUI desktop shell (binary `morph`): window and component host
- `app/` — Flutter shell (mobile; its desktop targets go in phase 2)
- `packaging/` — Linux desktop file and the Arch recipes (`morph`, `morph-bin`)
- `third_party/` — patched copies of published crates (see its README)
- `tests/xvfb/` — the page `scripts/xvfb-check.py` drives

Crate versions are `0.1.0` (BSD-3-Clause, edition 2024), except `morph-desktop`,
which carries the desktop build's version (`0.2.0-alpha.1`). They are not published
in this pass.
