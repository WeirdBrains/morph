# Handoff

Updated 2026-11-13, after `aa02ea6`. Michael's usage was at 88.1% of the weekly limit and still climbing. This file is the resume point. The task in progress is finishing the Linux `Isolate.create` fix (tests, Rust quality bar, commit). The popup bug is not started.

## Where things stand

`main` on origin is `e6c0c5e` (`Windows: silence MSVC 14.51's experimental coroutine error`). The working tree on alien-blue also has uncommitted Linux-build edits (not pushed yet): `app/pubspec.yaml`, `app/pubspec.lock`, `.github/workflows/ci.yml`, `README.md`, `CHANGELOG.md`. A `flutter test` of the app was still running when this note was written. Do not throw those edits away.

### Windows build (done, on main)

The Windows app was not built before this work. CI only ran Rust on `windows-latest`, plus `flutter build` for Linux and Android. `ARCHITECTURE.md` said the Windows app still needed `nuget.exe`.

Two commits are on main:

- `d30a211590c4eb92ad1aaf69a08032029d4bcb77` — `scripts/build-windows.ps1` builds the release app and zips it. CI runs that script. `TEMP` and `TMP` are set to `C:\t\morph` because `flutter_rust_bridge_hooks` puts the Cargo target under `TEMP`, and the default user temp path makes OpenSSL's `cl` object files exceed `MAX_PATH` (`C1083`). The first attempt on the Windows VM failed that way (object path 265 characters). With the short temp, the same path was 236 characters and the object file was written.
- `e6c0c5ead5711b19c9e4df1960a210ff654f8fc2` — MSVC 14.51 (Visual Studio 18 on `windows-latest`) rejects `<experimental/coroutine>`, which `flutter_inappwebview_windows` still includes. `app/windows/CMakeLists.txt` defines `_SILENCE_EXPERIMENTAL_COROUTINE_DEPRECATION_WARNINGS`. The CI run for `d30a211` failed on that error. The run for `e6c0c5e` passed, including the `windows` job: https://github.com/WeirdBrains/morph/actions/runs/43503875554

VM build (WIN-R0LRC6FBTOM, Flutter 3.47.8, rustc 1.99.1, VS 2022 Build Tools 17.14 / MSVC 14.44):

- Zip: `/mnt/ext1/scratch/morph-windows/morph-0.1.0-x86_64-pc-windows-msvc.zip` (20,733,310 bytes, SHA-256 `6d235b6c321c2d5dd60faee5d40c0e7995394635aa7c507c8fbf474e04160b6e`).
- `morph.exe` is an x64 Windows GUI binary. `LoadLibrary` succeeded for `morph.exe`, `morph_bridge.dll`, `flutter_inappwebview_windows_plugin.dll`, and `WebView2Loader.dll`. No Morph window was started.
- The zip does not include the VC++ 2015–2022 runtime (`VCRUNTIME140.dll`, `MSVCP140.dll`) or the WebView2 Runtime. The loader DLL is in the zip. The VM already had WebView2 Runtime 157.0.4341.46.
- How to rebuild, on a Windows machine with Flutter, Rust, Visual Studio C++, Strawberry Perl, and `nuget.exe` on `PATH`: `powershell -ExecutionPolicy Bypass -File scripts/build-windows.ps1`.

Not verified: launching the GUI, loading a page, the frameless window, or WebView2 itself. Wine is not installed. This was not a Linux cross-compile. There is no WiX installer.

A stash is still present and was not applied: `stash@{0}` on `302020b`, two lines adding `morph_webview_inapp` to `app/macos/Flutter/GeneratedPluginRegistrant.swift`. Current main already registers that plugin. Branches `gpu-fix` and `linux-persistent-webview-session` were left alone.

### Linux `flutter build linux` / `Isolate.create` (fix written locally, not committed)

Michael, on Flutter stable with Dart 3.14.0, ran `flutter build linux` from `morph/app`. `flutter clean` did not help. The failure is:

```
nativeapi-0.5.10/lib/src/run_native_app.dart:45:25: Error: Member not found: 'Isolate.create'.
    final app = Isolate.create(debugName: 'nativeapi');
```

Verified, not guessed:

- Dart 3.13.7 `sdk/lib/isolate/isolate.dart` (tag `3.13.7`) has `external static Isolate create({String? debugName})`, `runSync`, and `runEventLoopSync`.
- Dart 3.14.0's `isolate.dart` has none of those. This machine's SDK is Flutter 3.50.0 / Dart 3.14.0, and a search of that SDK found no `Isolate.create`.
- `nativeapi` 0.5.10 `lib/nativeapi.dart` exports `src/run_native_app.dart`, so every app that imports `package:nativeapi/nativeapi.dart` compiles the removed call. Its pubspec still says `sdk: ^3.13.0`, so a lockfile from Flutter 3.47 stays on 0.5.10 under Dart 3.14. `window_manager` 0.6.0 depends on `nativeapi: ^0.5.5`, which does not allow 0.6.0 (`^0.5.5` is `<0.6.0`).
- `nativeapi` 0.5.11 and 0.6.0 require Dart `^3.14.0`. Their changelogs say Dart 3.14 removed those VM APIs, and they moved `runNativeApp()` to `package:nativeapi/standalone.dart` so a normal import does not compile it. Checked: 0.6.0 `lib/nativeapi.dart` exports only `src/generated.dart`.
- `window_manager` 0.7.0 requires Flutter `>=3.50.0`, Dart `^3.14.0`, and `nativeapi` `^0.6.0`. Morph's window code uses `Window`, `WindowManager`, `TitleBarStyle`, `setFrameless`, and `setContentUnderTitleBar`. Those members are still on `nativeapi` 0.6.0's `Window`.

Local edits, not committed:

- `app/pubspec.yaml`: SDK `^3.14.0`, Flutter `>=3.50.0`, `window_manager` `>=0.7.0 <0.8.0`.
- `flutter pub get` resolved `window_manager` 0.7.0, `nativeapi` 0.6.0, `nativeapi_flutter` 0.6.0, `cnativeapi` 0.6.0. Lockfile updated.
- CI `flutter-version` set to `3.50.0` in `.github/workflows/ci.yml` (four jobs). README and CHANGELOG updated to match.
- `flutter build linux` on this tree, with `WAYLAND_DISPLAY` unset, exited 0 twice and wrote `app/build/linux/x64/release/bundle/morph`. Both runs also printed `ERROR: File modified during build. Build must be rerun.` and then `Built`.

That line is from `hooks_runner` 1.6.4 `build_runner.dart`: if a hook dependency's mtime is after `DateTime.now()` taken at the start of the hook, it logs that string at severe and still returns `Success`. It is not the `Isolate.create` failure, and `flutter clean` cannot fix the missing member. After the successful build, `app/.dart_tool/hooks_runner/morph/87f123bf01/dependencies.dependencies_hash_file.json` records `crates/morph-webview-control/proto` with hash `1`, which is `hooks_runner`'s sentinel for "modified after the cutoff". The directory's on-disk mtime is 2026-11-12. The message repeated on the second build. The bundle was still produced both times.

`flutter test` in `app/` was started with stdin from `/dev/null` and `WAYLAND_DISPLAY` unset, and had not finished when this note was written. Cargo fmt/clippy/test for the Linux commit had not been run yet. Build caches (`app/build`, `app/.dart_tool`, `/mnt/ext1/cargo-target/morph-linux-314`) were still present.

### Popup / Google sign-in (not started)

From Michael's screen recording, after the Linux fix:

1. Open linkedin.com (it showed the mobile layout). Click "Sign in with Google", then Continue. A popup opens on accounts.google.com.
2. Click "Email or phone" and type. No characters appear, the placeholder stays, and a caret blinks well below the field, over the "Before using this app..." text.
3. Click Next. The heading becomes "Use your passkey to confirm it's really you", but the email field, Forgot email, Create account, and Next stay as they were. No passkey UI.
4. The popup's X closes it.

Not investigated yet. Do not sign into a real account. A local page that opens a popup with an input and then changes the DOM is enough. Public sites are read-only checks. Test on Xvfb with `WAYLAND_DISPLAY` unset. Check whether Windows WebView2 has the same gap. Passkey/WebAuthn may be separate from the paint/input offset.

## Done

- Windows release zip built on the VM and inspected (`LoadLibrary`, PE headers, DLL dependencies). CI on `e6c0c5e` is green, including the Windows job.
- Cause of the Linux `Isolate.create` error identified against the Dart 3.13.7 and 3.14.0 SDKs and the nativeapi 0.5.10 / 0.6.0 sources.
- Local dependency bump to `window_manager` 0.7 / `nativeapi` 0.6, and two successful `flutter build linux` runs on Flutter 3.50.0.

## Next

The handoff itself is `aa02ea65f971f7b29a640d98cb58a4cec8d85640` on `main`. Resume at the Linux fix:

1. Let the in-progress `flutter test` finish. If it failed, fix the test or the window API call.
2. Run the repo quality bar (`cargo fmt --check`, `cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo test`) with `CARGO_TARGET_DIR` under `/mnt/ext1` and stdin from `/dev/null`. Clean that target, `app/build`, and `app/.dart_tool` afterward.
3. Commit and push the Linux fix to `main` (no PR, no force-push, no rebase, no tag).
4. Update this file, then investigate and fix popup typing, repaint, and passkey/WebAuthn fallback. Check WebView2 as well as Linux.

## How to resume

```sh
cd /mnt/ext1/Projects/WeirdBrains/morph
git status
git pull --ff-only origin main
# uncommitted Linux fix should still be in the tree if it was not committed
cd app
unset WAYLAND_DISPLAY WAYLAND_SOCKET
flutter pub get
flutter build linux
```

Windows, on a Windows host:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build-windows.ps1
```

Do not touch `~/.morph`. Do not print secrets. GUI only under Xvfb with `WAYLAND_DISPLAY` unset.
