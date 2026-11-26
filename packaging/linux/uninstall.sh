#!/bin/sh
# Removes the files installed by install.sh.
#
# Settings in ${XDG_CONFIG_HOME:-~/.config}/shirabe are not touched.
set -eu

PRODUCT="Shirabe Meter Suite"

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Usage: ./uninstall.sh [--system [--prefix DIR]]

Removes the Shirabe Meter Suite files installed by install.sh, using the same
destination rules:

  (no option)    ~/.vst3, ~/.clap, ~/.local/bin and
                 ${XDG_DATA_HOME:-~/.local/share}
  --system       PREFIX/lib/vst3, PREFIX/lib/clap, PREFIX/bin and PREFIX/share
                 (default prefix /usr/local, needs root)
  --prefix DIR   Prefix used with --system when it was installed.

Only this product's files are removed. Other plug-ins in the same directories
and the settings directory are left alone.
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
        die "--system removes files under /usr/local and needs root: run with sudo, or pass --prefix"
    fi
    vst3_dir=$prefix/lib/vst3
    clap_dir=$prefix/lib/clap
    bin_dir=$prefix/bin
    apps_dir=$prefix/share/applications
    icon_dir=$prefix/share/icons/hicolor/scalable/apps
    doc_dir=$prefix/share/doc/shirabe
    config_dir=""
else
    [ -n "${HOME:-}" ] || die "HOME is not set"
    case "${XDG_DATA_HOME:-}" in
        /*) data_home=$XDG_DATA_HOME ;;
        *) data_home=$HOME/.local/share ;;
    esac
    case "${XDG_CONFIG_HOME:-}" in
        /*) config_dir=$XDG_CONFIG_HOME/shirabe ;;
        *) config_dir=$HOME/.config/shirabe ;;
    esac
    vst3_dir=$HOME/.vst3
    clap_dir=$HOME/.clap
    bin_dir=$HOME/.local/bin
    apps_dir=$data_home/applications
    icon_dir=$data_home/icons/hicolor/scalable/apps
    doc_dir=$data_home/doc/shirabe
fi

removed=0
remove_path() {
    if [ -e "$1" ]; then
        rm -rf "$1"
        printf 'Removed %s\n' "$1"
        removed=$((removed + 1))
    fi
}

remove_path "$vst3_dir/$PRODUCT.vst3"
remove_path "$clap_dir/$PRODUCT.clap"
remove_path "$bin_dir/shirabe"
remove_path "$apps_dir/shirabe.desktop"
remove_path "$icon_dir/shirabe.svg"
remove_path "$doc_dir"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$apps_dir" >/dev/null 2>&1 || true
fi

if [ "$removed" -eq 0 ]; then
    printf '%s was not installed in these locations; nothing to remove.\n' "$PRODUCT"
else
    printf '\nRestart or rescan plug-ins in your host application.\n'
    if [ -n "$config_dir" ]; then
        printf 'Settings were kept in %s.\n' "$config_dir"
    else
        printf "Settings in each user's config directory were kept.\n"
    fi
fi
