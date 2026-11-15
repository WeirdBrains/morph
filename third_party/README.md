# third_party

Crates Morph carries a patched copy of, through `[patch.crates-io]` (and
`[patch."https://github.com/zed-industries/zed"]`) in the workspace
`Cargo.toml`. Each one is the published crate with the smallest
change, and goes away when upstream no longer needs it.

## libp2p-swarm 0.48.0

Published crate, unchanged except two things. One line in `Cargo.toml`: its wasm-only
dependency `wasm-bindgen-futures = "=0.4.58"` (which pins `js-sys` 0.3.85) is
relaxed to `0.4.58`. GPUI's wgpu (pinned with GPUI from Zed, see
ARCHITECTURE.md section 2.0) needs `js-sys` 0.3.104 or later. Cargo resolves
one lockfile for every target, so the wasm-only pin blocked the desktop shell
even though neither crate builds for wasm here. The `tests/` and `benches/`
directories and their `[[test]]`/`[[bench]]` entries are left out. And
`#![allow(warnings)]` at the top of `src/lib.rs`, because a path dependency's
warnings are not capped the way a registry crate's are, and this is upstream
code.

Drop this when a libp2p-swarm release no longer pins `wasm-bindgen-futures`
exactly (or `weirdnet-net` moves past libp2p 0.57).

## gpui_linux (Zed 703b5ac)

GPUI's Linux platform from the Zed commit GPUI is pinned to (ARCHITECTURE.md
section 2.0), Apache-2.0 (`LICENSE-APACHE`). `Cargo.toml` spells out the
versions Zed's workspace gave it (same versions, so `Cargo.lock` does not
move), and `#![allow(warnings, clippy::all)]` tops `src/gpui_linux.rs`, as
for libp2p-swarm. The one change: `src/linux/raw_key.rs` and the four
`raw_key::dispatching` calls in the Wayland keyboard handler
(`src/linux/wayland/client.rs`) tell an embedder the hardware keycode, keysym
and modifier state of the key event GPUI is dispatching. `gpui::Keystroke`
names keys by what they type, so the keypad's 1 and Enter look like the top
row's; Morph's off-screen web views on Wayland (`morph-webview`'s
`offscreen` backend) hand pages the real key, so `KeyboardEvent.code` says
`Numpad1` as it does on X11.

Drop this when GPUI's key events carry the keycode (or Morph moves to a GPUI
that does), or when native child web views on Wayland make the off-screen
backend unnecessary.
