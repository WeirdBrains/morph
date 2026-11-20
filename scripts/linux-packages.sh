#!/usr/bin/env bash
# Pack the release tarball from scripts/linux-dist.sh into the other Linux
# formats, each with the desktop entry and icons (desktop integration) and
# standalone (only bin/morph):
#   dist/morph-<version>-<arch>-unknown-linux-gnu.zip
#   dist/morph_<debver>-1_<debarch>.deb   dist/morph-standalone_<debver>-1_<debarch>.deb
#   dist/morph-<rpmver>-1.<arch>.rpm      dist/morph-standalone-<rpmver>-1.<arch>.rpm
# and adds them to dist/SHA256SUMS. The deb needs dpkg-deb; the rpm needs
# rpmbuild (skipped with a notice when missing, or MORPH_SKIP_RPM=1;
# MORPH_SKIP_DEB=1 skips the deb).
# Versions: 0.2.0-alpha.5 becomes 0.2.0~alpha.5 in deb and rpm, which sorts
# before 0.2.0 there.
#
#   scripts/linux-packages.sh [dist/morph-<version>-<arch>-unknown-linux-gnu.tar.gz]
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
tarball="${1:-$(ls -t dist/morph-*-unknown-linux-gnu.tar.gz | head -1)}"
name="$(basename "$tarball" .tar.gz)"
version="$(sed -E 's/^morph-(.*)-(x86_64|aarch64)-unknown-linux-gnu$/\1/' <<<"$name")"
arch="$(sed -E 's/^.*-(x86_64|aarch64)-unknown-linux-gnu$/\1/' <<<"$name")"
pkgver="${version/-/\~}"
case "$arch" in
  x86_64) debarch=amd64 ;;
  aarch64) debarch=arm64 ;;
esac

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
tar -C "$work" -xzf "$tarball"
bundle="$work/$name"
test -x "$bundle/bin/morph"

# zip: the same tree as the tarball.
python3 - "$work" "$name" "dist/$name.zip" <<'PY'
import os, sys, zipfile
work, name, out = sys.argv[1:]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for dirpath, dirs, files in sorted(os.walk(os.path.join(work, name))):
        dirs.sort()
        for f in sorted(files):
            path = os.path.join(dirpath, f)
            info = zipfile.ZipInfo(os.path.relpath(path, work), (1980, 1, 1, 0, 0, 0))
            info.external_attr = (os.stat(path).st_mode & 0o777 | 0o100000) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            with open(path, "rb") as fh:
                z.writestr(info, fh.read())
PY
built=("$name.zip")

# The file trees of the two forms under /usr.
tree() { # tree DIR FORM
  install -Dm755 "$bundle/bin/morph" "$1/usr/bin/morph"
  install -Dm644 "$bundle/LICENSE" "$1/usr/share/licenses/morph/LICENSE"
  if [ "$2" = desktop ]; then
    cp -a "$bundle/share/applications" "$bundle/share/icons" "$1/usr/share/"
  fi
}

deb_depends="libwebkit2gtk-4.1-0, libgtk-3-0, libglib2.0-0, libxkbcommon-x11-0, libxcb1, libvulkan1, libfontconfig1, libfreetype6, libstdc++6, libssl3 | libssl3t64"
for form in desktop standalone; do
  [ "${MORPH_SKIP_DEB:-0}" = 1 ] && break
  pkg=morph; [ "$form" = standalone ] && pkg=morph-standalone
  d="$work/deb-$pkg"
  tree "$d" "$form"
  install -d "$d/DEBIAN"
  {
    echo "Package: $pkg"
    echo "Version: $pkgver-1"
    echo "Architecture: $debarch"
    echo "Maintainer: Weird Brains LLC <m3talsmith@gmail.com>"
    echo "Depends: $deb_depends"
    echo "Recommends: mesa-vulkan-drivers | vulkan-icd"
    [ "$form" = standalone ] && echo "Conflicts: morph" && echo "Provides: morph"
    [ "$form" = desktop ] && echo "Conflicts: morph-standalone"
    echo "Section: web"
    echo "Priority: optional"
    echo "Homepage: https://github.com/WeirdBrains/morph"
    echo "Installed-Size: $(du -sk "$d/usr" | cut -f1)"
    if [ "$form" = desktop ]; then
      echo "Description: Distributed web browser with local AI (GPUI desktop shell)"
      echo " Morph with its desktop entry and icons; it can be the default browser."
    else
      echo "Description: Distributed web browser with local AI (standalone binary)"
      echo " Only the morph binary, without a desktop entry or icons."
    fi
  } > "$d/DEBIAN/control"
  file="${pkg}_${pkgver}-1_${debarch}.deb"
  dpkg-deb --root-owner-group -Zxz --build "$d" "dist/$file" >/dev/null
  built+=("$file")
done

if [ "${MORPH_SKIP_RPM:-0}" != 1 ] && command -v rpmbuild >/dev/null; then
  for form in desktop standalone; do
    pkg=morph; [ "$form" = standalone ] && pkg=morph-standalone
    r="$work/rpm-$pkg"
    mkdir -p "$r/root"
    tree "$r/root" "$form"
    (cd "$r/root" && find . -type f -o -type l | sed 's|^\.||' | sort) > "$r/files"
    rpmbuild -bb --quiet \
      --define "_topdir $r/top" --define "_rpmdir $r/out" --define "_build_id_links none" \
      --define "pkg $pkg" --define "pkgver $pkgver" --define "form $form" \
      --define "payload $r/root" --define "filelist $r/files" \
      --target "$arch" packaging/rpm/morph.spec >/dev/null
    file="$pkg-$pkgver-1.$arch.rpm"
    cp "$r/out/$arch/$file" "dist/$file"
    built+=("$file")
  done
else
  echo "rpmbuild not found (or MORPH_SKIP_RPM=1): no rpm" >&2
fi

(cd dist && for f in "${built[@]}"; do grep -v "  $f\$" SHA256SUMS > SHA256SUMS.new || true; mv SHA256SUMS.new SHA256SUMS; sha256sum "$f" >> SHA256SUMS; done)
printf 'dist/%s\n' "${built[@]}"
