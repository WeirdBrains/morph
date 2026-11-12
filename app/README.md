# Morph Flutter app

The Morph shell. Native code lives in `../crates/morph-bridge` and is built
through the Native Assets hook (`hook/build.dart`).

```sh
export CARGO_TARGET_DIR=/mnt/ext1/cargo-target/morph
flutter pub get
flutter_rust_bridge_codegen generate   # after changing crates/morph-bridge/src/api
flutter analyze
flutter test
```

Use `MORPH_HOME` or a temp directory in tests. Do not write to a user's
`~/.morph`.

On desktop, Morph draws custom window buttons (minimize, maximize, close) on
Windows and Linux. Tiling compositors such as Hyprland, Sway, i3, niri, and
river show only close. Override with `MORPH_WINDOW_BUTTONS=close` or
`MORPH_WINDOW_BUTTONS=all`. On macOS the native traffic lights stay visible
on the left of a hidden title bar; the tab strip pads to clear them.

Tab position (top, bottom, left, or right) is stored as `tab_position` in
`settings.json` under the Morph home. Collapsing the left or right tab list
is stored as `sidebar_collapsed` in the same file (default: expanded). The
address and navigation row always stays at the top; only the tab strip or
list moves. Bottom tabs flip their decorations downward so they attach to
the content above.
