# Morph

Morph is a distributed web browser with local AI, built on Flutter, a Rust 2024
core, and Weirdnet.

This repository is the start of milestone M1 (the shell): a Flutter app, a thin
`flutter_rust_bridge` API, and Weirdnet 0.1.0 identity types. The Linux WebKit
texture backend, Android `flutter_inappwebview` path, and page-bridge
round-trip on a live view come next in M1.

Planning documents: `PLAN.md`, `PRIORITIES.md`, and `ARCHITECTURE.md`.
Do not edit `PLAN.md`. `PRIORITIES.md` is the adopted priority list; update it
to reflect progress, but do not restructure it.

## Build

Rust 1.99+ and Flutter 3.50+ (Dart 3.14) are required. On alien-blue, point cargo at scratch
storage so `target/` never lands in `~/Projects`:

```sh
export CARGO_TARGET_DIR=/mnt/ext1/cargo-target/morph
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
- `crates/morph-bridge` — thin flutter_rust_bridge surface
- `app/` — Flutter shell

Crate versions are `0.1.0` (BSD-3-Clause, edition 2024). They are not published
in this pass.
