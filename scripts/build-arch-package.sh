#!/usr/bin/env bash
# Build the Arch package from the current working tree (committed or not),
# using packaging/arch/md-editor-git/PKGBUILD with its git source swapped for
# a tarball of the tree. Used by CI and for testing packaging changes locally.
#
#   scripts/build-arch-package.sh [makepkg args...]   # e.g. --nocheck, -si
#
# The package lands in target/arch-package/.
set -euo pipefail
cd "$(dirname "$0")/.."

out=target/arch-package
rm -rf "$out"
mkdir -p "$out"

git ls-files -co --exclude-standard -z |
    tar --null -T - --transform 's,^,md-editor/,' -czf "$out/md-editor-src.tar.gz"

sed -e 's|  "$_name::git+$url.git"|  "md-editor-src.tar.gz"|' \
    -e "/^pkgver() {/,/^}/c\\pkgver() { printf 'r%s.%s' \"\$(git -C '$PWD' rev-list --count HEAD)\" \"\$(git -C '$PWD' rev-parse --short=7 HEAD)\"; }" \
    packaging/arch/md-editor-git/PKGBUILD > "$out/PKGBUILD"

cd "$out"
makepkg -f "$@"
