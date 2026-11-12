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
