#!/bin/sh
# Shirabe Meter Suite installer for Linux.
#
# Installs the VST3 and CLAP plug-ins, the standalone application and a desktop
# entry either for the current user or, with --system, under a prefix.
set -eu

PRODUCT="Shirabe Meter Suite"

SELF_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Usage: ./install.sh [--system [--prefix DIR]]

Installs the VST3 and CLAP plug-ins, the standalone application and a desktop
entry from this directory.

  (no option)    Install for the current user:
                   ~/.vst3/Shirabe Meter Suite.vst3
                   ~/.clap/Shirabe Meter Suite.clap
                   ~/.local/bin/shirabe
                   ${XDG_DATA_HOME:-~/.local/share}/applications/shirabe.desktop
                   ${XDG_DATA_HOME:-~/.local/share}/icons/hicolor/scalable/apps/shirabe.svg
                   ${XDG_DATA_HOME:-~/.local/share}/doc/shirabe/
  --system       Install for all users under PREFIX (default /usr/local,
                 needs root):
                   PREFIX/lib/vst3, PREFIX/lib/clap, PREFIX/bin,
                   PREFIX/share/applications, PREFIX/share/icons,
                   PREFIX/share/doc/shirabe
  --prefix DIR   Use DIR instead of /usr/local for --system. A prefix you can
                 write to does not need root.

Re-running the script replaces the installed copy. Settings in
${XDG_CONFIG_HOME:-~/.config}/shirabe are never touched.
EOF
}

mode=user
prefix=/usr/local
prefix_set=no

while [ "$#" -gt 0 ]; do
    case "$1" in
        --system)
            mode=system
            ;;
        --prefix)
            [ "$#" -ge 2 ] || die "--prefix needs a directory"
            prefix=$2
            prefix_set=yes
            shift
            ;;
        --prefix=*)
            prefix=${1#--prefix=}
            prefix_set=yes
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "unknown option: $1 (use --help)"
            ;;
    esac
    shift
done

if [ "$prefix_set" = yes ] && [ "$mode" != system ]; then
    die "--prefix is only used together with --system"
fi

if [ "$mode" = system ]; then
    case "$prefix" in
        /*) ;;
        *) die "--prefix must be an absolute path" ;;
    esac
    if [ "$prefix" = /usr/local ] && [ "$(id -u)" -ne 0 ]; then
        die "--system installs to /usr/local and needs root: run with sudo, or pass --prefix"
    fi
    vst3_dir=$prefix/lib/vst3
    clap_dir=$prefix/lib/clap
    bin_dir=$prefix/bin
    apps_dir=$prefix/share/applications
    icon_dir=$prefix/share/icons/hicolor/scalable/apps
    hicolor_dir=$prefix/share/icons/hicolor
    doc_dir=$prefix/share/doc/shirabe
else
    [ -n "${HOME:-}" ] || die "HOME is not set"
    case "${XDG_DATA_HOME:-}" in
        /*) data_home=$XDG_DATA_HOME ;;
        *) data_home=$HOME/.local/share ;;
    esac
    vst3_dir=$HOME/.vst3
    clap_dir=$HOME/.clap
    bin_dir=$HOME/.local/bin
    apps_dir=$data_home/applications
    icon_dir=$data_home/icons/hicolor/scalable/apps
    hicolor_dir=$data_home/icons/hicolor
    doc_dir=$data_home/doc/shirabe
fi

vst3_src=$SELF_DIR/$PRODUCT.vst3
clap_src=$SELF_DIR/$PRODUCT.clap
app_src=$SELF_DIR/shirabe

for required in "$vst3_src" "$clap_src" "$app_src" "$SELF_DIR/shirabe.desktop" \
    "$SELF_DIR/shirabe.svg" "$SELF_DIR/README.md" "$SELF_DIR/CHANGELOG.md" \
    "$SELF_DIR/LICENSES/GPL-3.0-or-later.txt"; do
    [ -e "$required" ] || die "missing $required; run install.sh from the extracted tarball"
done

mkdir -p "$vst3_dir" "$clap_dir" "$bin_dir" "$apps_dir" "$icon_dir" "$doc_dir"

# Replace only this product's files; other plug-ins in the same directories are
# left alone.
rm -rf "$vst3_dir/$PRODUCT.vst3"
cp -R "$vst3_src" "$vst3_dir/$PRODUCT.vst3"

install -m 0755 "$clap_src" "$clap_dir/$PRODUCT.clap"
install -m 0755 "$app_src" "$bin_dir/shirabe"
install -m 0644 "$SELF_DIR/shirabe.desktop" "$apps_dir/shirabe.desktop"
install -m 0644 "$SELF_DIR/shirabe.svg" "$icon_dir/shirabe.svg"

rm -rf "$doc_dir/LICENSES"
cp -R "$SELF_DIR/LICENSES" "$doc_dir/LICENSES"
install -m 0644 "$SELF_DIR/README.md" "$doc_dir/README.md"
install -m 0644 "$SELF_DIR/CHANGELOG.md" "$doc_dir/CHANGELOG.md"

# Refresh the desktop and icon caches when the tools exist; a failure here does
# not affect the installed files.
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$apps_dir" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t "$hicolor_dir" >/dev/null 2>&1 || true
fi

printf 'Installed %s:\n' "$PRODUCT"
printf '  VST3  %s\n' "$vst3_dir/$PRODUCT.vst3"
printf '  CLAP  %s\n' "$clap_dir/$PRODUCT.clap"
printf '  App   %s\n' "$bin_dir/shirabe"
printf '  Menu  %s\n' "$apps_dir/shirabe.desktop"
printf '  Docs  %s\n' "$doc_dir"
printf '\n'
printf 'Restart or rescan plug-ins in your host application.\n'

case ":$PATH:" in
    *":$bin_dir:"*)
        printf 'Run "shirabe" or start "Shirabe Meter Suite" from the application menu.\n'
        ;;
    *)
        printf 'Note: %s is not in PATH. Run %s directly, or add the directory to PATH.\n' \
            "$bin_dir" "$bin_dir/shirabe"
        ;;
esac
printf 'Uninstall with %s/uninstall.sh' "$SELF_DIR"
if [ "$mode" = system ]; then
    printf ' --system'
    if [ "$prefix" != /usr/local ]; then
        printf ' --prefix %s' "$prefix"
    fi
fi
printf '.\n'
