# Morph

Morph is a distributed web browser with local AI, built on a Rust 2024 core and
Weirdnet.

On `main` the shell is Flutter on every platform. On the `gpui` branch the
desktop shell (Linux first, then macOS and Windows) is moving to
[GPUI](https://gpui.rs), Zed's Rust UI framework, while Flutter stays the mobile
shell (Android, iOS, F-Droid). Both shells sit on the same Rust crates. So far
the branch holds the plan only: see `ARCHITECTURE.md` (sections 1, 2.0 and 2a,
and the 2026-11-14 decisions) and `docs/handoff.md` for the phases. Until phase
1 lands, the build steps below are the existing Flutter ones.

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

### Desktop shell on GPUI (`gpui` branch, from phase 1; not built yet)

Phase 1 adds the `morph-desktop` crate (binary `morph`). It is planned to build
with the same cargo commands, plus the system libraries wry and GPUI need on
Linux. On Arch (my-desktop):

```sh
sudo pacman -S --needed webkit2gtk-4.1 gtk3 libxkbcommon-x11 vulkan-icd-loader
export CARGO_TARGET_DIR=<scratch-dir>/cargo-target/morph
cargo run -p morph-desktop --release
```

On Debian or Ubuntu: `libwebkit2gtk-4.1-dev libgtk-3-dev libxkbcommon-x11-dev
libvulkan-dev` plus a Vulkan driver (`mesa-vulkan-drivers` for Mesa). On a
Wayland session the shell runs through XWayland for now (`ARCHITECTURE.md`
section 2.0). Each phase also ships Arch packages (`cd packaging/arch/morph && makepkg -si`,
or `pacman -U` on the package CI attaches; `morph-bin` on the WeirdBrains TAUR
server once a phase is approved),
and CI will attach a prebuilt Linux x86_64 tarball to each run on the branch. The exact list is confirmed when phase 1 lands; `docs/handoff.md` has
the current state.

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

- `crates/morph-core` — version, event ticks, page-bridge message contract
- `crates/morph-identity` — paths, Weirdnet user and node identity helpers
- `crates/morph-store`, `crates/morph-keystore` — encrypted library database and its key
- `crates/morph-webview-control` — in-process gRPC control layer for web views
- `crates/weirdnet-device-link` — device linking, moving to its own Weirdnet repo
- `crates/morph-bridge` — thin flutter_rust_bridge surface (mobile shell)
- `app/` — Flutter shell (all platforms on `main`; mobile only once phase 2 lands)
- Planned on `gpui`: `morph-ui`, `morph-webview`, `morph-browser`, `morph-desktop`

Crate versions are `0.1.0` (BSD-3-Clause, edition 2024). They are not published
in this pass.
