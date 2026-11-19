#!/usr/bin/env bash
# Build the desktop shell (GPUI, WKWebView) for macOS and pack Morph.app,
# universal (arm64 + x86_64), as a zip and a dmg:
#   dist/morph-<version>-universal-apple-darwin.zip
#   dist/morph-<version>-universal-apple-darwin.dmg
# Unsigned apart from an ad-hoc signature (docs/install-macos-windows.md has
# the Gatekeeper steps). Needs Xcode (the Metal shader compiler) and both
# Rust targets:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
#   scripts/macos-dist.sh            # uses $CARGO_TARGET_DIR or ./target
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
target_dir="${CARGO_TARGET_DIR:-$root/target}"
version="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "morph-desktop"))')"
short="${version%%-*}"
name="morph-$version-universal-apple-darwin"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"

# llama.cpp's Metal backend (ggml-metal-device.m) uses @available, which calls
# ___isPlatformVersionAtLeast from clang's runtime; rustc's cc link step does
# not pull that archive in, so name it (it is a fat archive, both arches).
rt="$(clang -print-file-name=libclang_rt.osx.a)"
if [ -f "$rt" ]; then
  export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=$rt"
fi

# The Intel slice gets llama.cpp (local models) for an explicit x86-64-v3
# baseline (AVX, AVX2, FMA, F16C, BMI2; no AVX-512), as on Linux and Windows;
# weirdnet-model refuses local models cleanly on older CPUs. arm64 keeps the
# defaults.
x86_64_v3=(GGML_NATIVE=OFF GGML_AVX=ON GGML_AVX2=ON GGML_FMA=ON GGML_F16C=ON GGML_BMI2=ON GGML_AVX512=OFF)
cargo build --locked --release -p morph-desktop --target aarch64-apple-darwin
env "${x86_64_v3[@]}" cargo build --locked --release -p morph-desktop --target x86_64-apple-darwin

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
app="$stage/Morph.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/morph" \
  "$target_dir/aarch64-apple-darwin/release/morph" "$target_dir/x86_64-apple-darwin/release/morph"
sed -e "s/@VERSION@/$version/" -e "s/@SHORT_VERSION@/$short/" packaging/macos/Info.plist > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"
cp docs/brand/Morph.icns "$app/Contents/Resources/Morph.icns"
cp LICENSE README.md "$app/Contents/Resources/"
# Apple silicon refuses unsigned code; an ad-hoc signature is enough to run
# (Gatekeeper still asks once, see the install notes).
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"
lipo -info "$app/Contents/MacOS/morph"

mkdir -p dist
rm -f "dist/$name.zip" "dist/$name.dmg"
ditto -c -k --sequesterRsrc --keepParent "$app" "dist/$name.zip"
dmg="$stage/dmg"
mkdir -p "$dmg"
cp -R "$app" "$dmg/"
ln -s /Applications "$dmg/Applications"
hdiutil create -quiet -volname "Morph $version" -srcfolder "$dmg" -ov -format UDZO "dist/$name.dmg"
(cd dist && shasum -a 256 "$name.zip" "$name.dmg" > SHA256SUMS-macos)
echo "dist/$name.zip"
echo "dist/$name.dmg"
cat dist/SHA256SUMS-macos
