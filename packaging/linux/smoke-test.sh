#!/usr/bin/env bash
# Installs and uninstalls the Linux tarball into a temporary HOME and a
# temporary --system prefix, and checks the files. Safe to run anywhere: it
# never writes outside its own temporary directory.
#
# Usage: packaging/linux/smoke-test.sh dist/installers/ShirabeMeterSuite-*-linux-x64.tar.gz
set -euo pipefail

PRODUCT="Shirabe Meter Suite"

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

[ "$#" -ge 1 ] || die "usage: $0 <tarball>"
tarball=$1
[ -f "$tarball" ] || die "no such file: $tarball"
tarball=$(CDPATH= cd -- "$(dirname -- "$tarball")" && pwd)/$(basename -- "$tarball")

work=$(mktemp -d "${TMPDIR:-/tmp}/shirabe-smoke.XXXXXX")
trap 'rm -rf "$work"' EXIT

tar -xzf "$tarball" -C "$work"
root=$(find "$work" -mindepth 1 -maxdepth 1 -type d -print -quit)
[ -n "$root" ] || die "the tarball has no top-level directory"

assert_file() {
    [ -f "$1" ] || die "expected file: $1"
}

assert_missing() {
    [ ! -e "$1" ] || die "still present: $1"
}

assert_exec() {
    [ -x "$1" ] || die "not executable: $1"
}

echo "== user install (temporary HOME) =="
home=$work/home
mkdir -p "$home"
# A plug-in that must survive the uninstall.
mkdir -p "$home/.vst3/Other Plugin.vst3/Contents/x86_64-linux"
printf 'sentinel\n' > "$home/.vst3/Other Plugin.vst3/Contents/x86_64-linux/other.so"
mkdir -p "$home/.config/shirabe"
printf '{}\n' > "$home/.config/shirabe/settings.json"

env -u XDG_DATA_HOME HOME="$home" "$root/install.sh" > "$work/install.log"
assert_file "$home/.vst3/$PRODUCT.vst3/Contents/x86_64-linux/$PRODUCT.so"
assert_file "$home/.clap/$PRODUCT.clap"
assert_exec "$home/.local/bin/shirabe"
assert_file "$home/.local/share/applications/shirabe.desktop"
assert_file "$home/.local/share/icons/hicolor/scalable/apps/shirabe.svg"
assert_file "$home/.local/share/doc/shirabe/LICENSES/GPL-3.0-or-later.txt"

grep -qx 'Exec=shirabe' "$home/.local/share/applications/shirabe.desktop" ||
    die "the desktop entry does not launch shirabe"

# A second run must succeed and replace the copy.
env -u XDG_DATA_HOME HOME="$home" "$root/install.sh" > "$work/install2.log"

echo "== user uninstall =="
env -u XDG_DATA_HOME HOME="$home" "$root/uninstall.sh" > "$work/uninstall.log"
assert_missing "$home/.vst3/$PRODUCT.vst3"
assert_missing "$home/.clap/$PRODUCT.clap"
assert_missing "$home/.local/bin/shirabe"
assert_missing "$home/.local/share/applications/shirabe.desktop"
assert_missing "$home/.local/share/icons/hicolor/scalable/apps/shirabe.svg"
assert_missing "$home/.local/share/doc/shirabe"
# Unrelated files and the settings survive.
assert_file "$home/.vst3/Other Plugin.vst3/Contents/x86_64-linux/other.so"
assert_file "$home/.config/shirabe/settings.json"

echo "== system prefix install =="
prefix=$work/prefix
mkdir -p "$prefix"
env -u XDG_DATA_HOME "$root/install.sh" --system --prefix "$prefix" > "$work/system-install.log"
assert_file "$prefix/lib/vst3/$PRODUCT.vst3/Contents/x86_64-linux/$PRODUCT.so"
assert_file "$prefix/lib/clap/$PRODUCT.clap"
assert_exec "$prefix/bin/shirabe"
assert_file "$prefix/share/applications/shirabe.desktop"
assert_file "$prefix/share/doc/shirabe/README.md"

echo "== system prefix uninstall =="
env -u XDG_DATA_HOME "$root/uninstall.sh" --system --prefix "$prefix" > "$work/system-uninstall.log"
assert_missing "$prefix/lib/vst3/$PRODUCT.vst3"
assert_missing "$prefix/lib/clap/$PRODUCT.clap"
assert_missing "$prefix/bin/shirabe"
assert_missing "$prefix/share/applications/shirabe.desktop"
assert_missing "$prefix/share/doc/shirabe"

echo "PASS: install and uninstall for the user and a system prefix"
