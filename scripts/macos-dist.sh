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

for t in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build --locked --release -p morph-desktop --target "$t"
done

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
echo "packing dist/$name.zip"
ditto -c -k --sequesterRsrc --keepParent "$app" "dist/$name.zip"
dmg="$stage/dmg"
mkdir -p "$dmg"
cp -R "$app" "$dmg/"
ln -s /Applications "$dmg/Applications"
# hdiutil on GitHub's macOS runners sometimes fails with "Resource busy"
# while something (XProtect, Spotlight) still holds the staged files, and
# with -quiet it said nothing, so the step only exited 1. Show its output and
# retry a few times, with a pause, before giving up.
# hdiutil sizes the image from -srcfolder and can come up short ("No space
# left on device" while copying Morph.app/Contents/MacOS/morph, on GitHub's
# runners and on the Mac alike), so give it the staged size plus a fifth and
# 20 MB.
dmg_kb=$(du -sk "$dmg" | cut -f1)
dmg_size="$((dmg_kb + dmg_kb / 5 + 20480))k"
dmg_ok=0
for attempt in 1 2 3 4 5; do
  if hdiutil create -volname "Morph $version" -srcfolder "$dmg" -size "$dmg_size" -ov -format UDZO "dist/$name.dmg"; then
    dmg_ok=1
    break
  fi
  echo "hdiutil create failed (attempt $attempt of 5); retrying" >&2
  rm -f "dist/$name.dmg"
  sleep $((attempt * 5))
done
if [ "$dmg_ok" != 1 ]; then
  echo "hdiutil create failed 5 times" >&2
  exit 1
fi
(cd dist && shasum -a 256 "$name.zip" "$name.dmg" > SHA256SUMS-macos)
echo "dist/$name.zip"
echo "dist/$name.dmg"
cat dist/SHA256SUMS-macos
