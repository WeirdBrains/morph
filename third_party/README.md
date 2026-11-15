# third_party

Crates Morph carries a patched copy of, through `[patch.crates-io]` in the
workspace `Cargo.toml`. Each one is the published crate with the smallest
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
