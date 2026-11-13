# Handoff

Updated 2026-11-13, after `005d01c`. Michael's usage was at 88.1% of the weekly limit and still climbing. This file is the resume point. The Linux `Isolate.create` fix is on main. The next task is the popup typing and repaint bug. It is not started.

## Where things stand

`main` on origin is `005d01c` (`Linux: build on Flutter 3.50 with nativeapi 0.6`). The working tree is clean aside from this handoff edit. Earlier Windows commits `d30a211` and `e6c0c5e` are still on main.

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

### Linux `flutter build linux` / `Isolate.create` (done, on main)

Commit `005d01c4ad7b2f415dd592c299dd1473c6e126e0` (`Linux: build on Flutter 3.50 with nativeapi 0.6`). `app/pubspec.yaml` requires Dart `^3.14.0` and Flutter `>=3.50.0`, and `window_manager` `>=0.7.0 <0.8.0`. The lockfile has `nativeapi` 0.6.0. CI `flutter-version` is `3.50.0`. Package lockfiles picked up `test_api` 0.7.14 and `listen` 1.0.1 from that SDK.

What Michael should run, already on Flutter 3.50 / Dart 3.14:

```sh
cd morph
git pull
cd app
flutter pub get
flutter build linux
```

Do not downgrade Flutter. `flutter clean` does not fix this.

### Linux `flutter build linux` / `Isolate.create` (investigation, kept for the evidence)

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

Those edits are what `005d01c` committed. `flutter build linux`, with `WAYLAND_DISPLAY` unset, exited 0 twice and wrote `app/build/linux/x64/release/bundle/morph`. Both runs also printed `ERROR: File modified during build. Build must be rerun.` and then `Built`.

That line is from `hooks_runner` 1.6.4 `build_runner.dart`: if a hook dependency's mtime is after `DateTime.now()` taken at the start of the hook, it logs that string at severe and still returns `Success`. It is not the `Isolate.create` failure, and `flutter clean` cannot fix the missing member. After the successful build, `app/.dart_tool/hooks_runner/morph/87f123bf01/dependencies.dependencies_hash_file.json` records `crates/morph-webview-control/proto` with hash `1`, which is `hooks_runner`'s sentinel for "modified after the cutoff". The directory's on-disk mtime is 2026-11-12. The message repeated on the second build. The bundle was still produced both times.

Verified on this machine (Flutter 3.50.0, Dart 3.14.0), `WAYLAND_DISPLAY` unset, stdin from `/dev/null`: `flutter build linux` exited 0 twice and wrote `app/build/linux/x64/release/bundle/morph`. `flutter test` in `app/` passed (186). Package tests for `morph_webview`, `morph_webview_inapp`, and `morph_webview_linux` passed. `cargo fmt --check`, `cargo build`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` passed. Those build directories were removed afterward. CI for `005d01c` had not been checked when this note was updated.

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
- Linux fix committed as `005d01c`: `window_manager` 0.7 / `nativeapi` 0.6, `flutter build linux` succeeded twice, app and package tests passed, Rust quality bar passed.

## Next

The Linux fix is `005d01c4ad7b2f415dd592c299dd1473c6e126e0` on `main`. Next, and not started: the popup bug below. Find why a popup's text field accepts a caret but not visible text, and why a later DOM change updates only the heading. Fix typing and page updates. Make passkey/WebAuthn work or fall back cleanly. Check WebView2 as well as the Linux web view. Test on Xvfb with `WAYLAND_DISPLAY` unset. Never sign into a real account.

## How to resume

```sh
cd /mnt/ext1/Projects/WeirdBrains/morph
git pull --ff-only origin main
# Linux fix is 005d01c. Working tree should be clean.
# Next work is the popup bug described above.
```

Windows, on a Windows host:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build-windows.ps1
```

Do not touch `~/.morph`. Do not print secrets. GUI only under Xvfb with `WAYLAND_DISPLAY` unset.
