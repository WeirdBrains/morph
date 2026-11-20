#!/usr/bin/env bash
# Build the desktop shell (GPUI) for Linux and pack the release tarball:
#   dist/morph-<version>-<arch>-unknown-linux-gnu.tar.gz and dist/SHA256SUMS
# The tarball holds bin/morph, the desktop file, icons, LICENSE and README.md,
# the layout packaging/arch/morph-bin/PKGBUILD installs.
#
#   scripts/linux-dist.sh            # uses $CARGO_TARGET_DIR or ./target
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
target_dir="${CARGO_TARGET_DIR:-$root/target}"
version="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "morph-desktop"))')"
arch="$(uname -m)"
name="morph-$version-$arch-unknown-linux-gnu"

# llama.cpp (local models) for an explicit x86-64-v3 baseline rather than the
# build machine's CPU: AVX, AVX2, FMA, F16C and BMI2 (Haswell / Zen 1, 2013 on),
# no AVX-512. weirdnet-model's CPU guard refuses local models cleanly on older
# CPUs. The deb, rpm, Nix and morph-bin packages repack this tarball. ARM keeps
# llama.cpp's defaults.
if [ "$arch" = x86_64 ]; then
  export GGML_NATIVE=OFF GGML_AVX=ON GGML_AVX2=ON GGML_FMA=ON GGML_F16C=ON GGML_BMI2=ON GGML_AVX512=OFF
fi

cargo build --locked --release -p morph-desktop

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
dir="$stage/$name"
install -Dm755 "$target_dir/release/morph" "$dir/bin/morph"
install -Dm644 packaging/linux/morph.desktop "$dir/share/applications/morph.desktop"
# The password saver's polkit action (crates/morph-os/src/os_auth.rs).
install -Dm644 packaging/linux/com.weirdbrains.morph.policy "$dir/share/polkit-1/actions/com.weirdbrains.morph.policy"
for size in 16 24 32 48 64 128 256 512; do
  install -Dm644 "docs/brand/png/morph-icon-$size.png" "$dir/share/icons/hicolor/${size}x${size}/apps/morph.png"
done
install -Dm644 docs/brand/svg/morph-icon.svg "$dir/share/icons/hicolor/scalable/apps/morph.svg"
install -Dm644 LICENSE "$dir/LICENSE"
install -Dm644 README.md "$dir/README.md"

mkdir -p dist
tar -C "$stage" --owner=0 --group=0 --sort=name -czf "dist/$name.tar.gz" "$name"
(cd dist && sha256sum "$name.tar.gz" > SHA256SUMS)
echo "dist/$name.tar.gz"
cat dist/SHA256SUMS
