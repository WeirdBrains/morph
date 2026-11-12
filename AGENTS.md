# Morph

This file adds Morph-specific rules on top of the collective
`WeirdBrains/weirdnet-agents` `AGENTS.md`. That collective file is mandatory.
If the two conflict, the collective file wins unless this file explicitly
overrides a rule and gives a reason.

## Planning documents

- Never modify `PLAN.md`. That is Michael's.
- `PRIORITIES.md` is the adopted priority list. You may update it to reflect
  progress, but do not restructure it.
- Architecture lives in `ARCHITECTURE.md`.

## Build and test

- On alien-blue, set `CARGO_TARGET_DIR` to a scratch directory under `/mnt/ext1`
  (fallback `/mnt/ext5t`). Never run cargo so that `target/` lands inside
  `~/Projects`. After every build or test run, clean cargo targets and Flutter
  `build/` / `.dart_tool/` outputs.
- Tests and manual runs use a temp directory or `MORPH_HOME`. Never touch a
  user's real `~/.morph` or other home and config directories.
- Secrets (keys, recovery words, passwords, tokens) are never logged, printed
  in verbose output, or committed.
- Before every commit: `cargo fmt --check`, `cargo build`, `cargo clippy --all-targets -- -D warnings`,
  and `cargo test` all pass. New behaviour ships with tests.
- Flutter iOS and macOS builds use Swift Package Manager, not CocoaPods. There
  should be no Podfile.
- Websites use Jaspr. Do not invent a different site stack.

## Identity and data

- A person has one Weirdnet user key across apps. Morph uses `Profile::WEIRDNET`
  and `weirdnet-keys` / `weirdnet-net` 0.1.0 so an install can later link to an
  existing Muninn identity. Morph never creates a second identity for someone
  who already has one.
- Full first-run keyring, store unlock and device linking is milestone M3.
  This tree may generate keys in memory or under `MORPH_HOME` for tests only.

## Flutter Rust bridge

- `morph-bridge` is the thin `flutter_rust_bridge` surface. Domain logic lives
  in `morph-core` and the other `morph-*` crates, which must be testable without
  Flutter.
- After changing `crates/morph-bridge/src/api`, run
  `cd app && flutter_rust_bridge_codegen generate`.
