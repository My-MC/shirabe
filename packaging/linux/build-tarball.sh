#!/usr/bin/env bash
# Builds the reproducible Linux x64 tar.gz of the bundles, the standalone
# application, the install/uninstall scripts and the desktop entry.
#
# The file contents come from the DIST directory (the CI "Collect artifacts"
# step or a local release build). Timestamps, ownership, order and gzip header
# are normalised, so two runs with the same DIST and SOURCE_DATE_EPOCH produce
# a byte-identical archive.
set -euo pipefail

PRODUCT="Shirabe Meter Suite"
repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)

dist=$repo_root/dist/shirabe-linux-x64
out_dir=$repo_root/dist/installers
version=""
epoch=${SOURCE_DATE_EPOCH:-}

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Usage: packaging/linux/build-tarball.sh [--dist DIR] [--out DIR] [--version V]
                                        [--source-date-epoch SECONDS]

  --dist DIR                Directory with the VST3 bundle, CLAP and shirabe
                            (default dist/shirabe-linux-x64)
  --out DIR                 Directory for the tar.gz (default dist/installers)
  --version V               Override the version from Cargo.toml
  --source-date-epoch SEC   Timestamp stored in the archive (default: the last
                            commit time, or 0 outside a Git checkout)

Environment: SOURCE_DATE_EPOCH is used when --source-date-epoch is omitted.
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --dist)
            [ "$#" -ge 2 ] || die "--dist needs a directory"
            dist=$2
            shift
            ;;
        --out)
            [ "$#" -ge 2 ] || die "--out needs a directory"
            out_dir=$2
            shift
            ;;
        --version)
            [ "$#" -ge 2 ] || die "--version needs a value"
            version=$2
            shift
            ;;
        --source-date-epoch)
            [ "$#" -ge 2 ] || die "--source-date-epoch needs a value"
            epoch=$2
            shift
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

if [ -z "$version" ]; then
    version=$("$repo_root/packaging/version.sh")
fi

printf '%s' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$' ||
    die "unexpected version: $version"

if [ -z "$epoch" ]; then
    epoch=$(git -C "$repo_root" log -1 --format=%ct 2>/dev/null || true)
    epoch=${epoch:-0}
fi

case "$epoch" in
    ''|*[!0-9]*) die "SOURCE_DATE_EPOCH must be a whole number of seconds" ;;
esac

vst3_src=$dist/$PRODUCT.vst3
clap_src=$dist/$PRODUCT.clap
app_src=$dist/shirabe

[ -d "$vst3_src" ] || die "missing $vst3_src (build with cargo xtask bundle shirabe-plugin --release)"
[ -f "$clap_src" ] || die "missing $clap_src"
[ -f "$app_src" ] || die "missing $app_src (build with cargo build --release -p shirabe-app --bin shirabe)"

work=$(mktemp -d "${TMPDIR:-/tmp}/shirabe-tarball.XXXXXX")
trap 'rm -rf "$work"' EXIT

root_name="ShirabeMeterSuite-$version-linux-x64"
root=$work/$root_name
mkdir -p "$root"

cp -R "$vst3_src" "$root/"
cp "$clap_src" "$root/"
cp "$app_src" "$root/"
cp "$repo_root/README.md" "$repo_root/CHANGELOG.md" "$root/"
cp -R "$repo_root/LICENSES" "$root/"
cp "$repo_root/packaging/linux/install.sh" "$root/"
cp "$repo_root/packaging/linux/uninstall.sh" "$root/"
cp "$repo_root/packaging/linux/shirabe.desktop" "$root/"
cp "$repo_root/packaging/linux/shirabe.svg" "$root/"

# Normalise modes so the archive does not depend on the build umask.
find "$root" -type d -exec chmod 0755 {} +
find "$root" -type f -exec chmod 0644 {} +
chmod 0755 "$root/shirabe" "$root/install.sh" "$root/uninstall.sh"
find "$root" -type f \( -name '*.clap' -o -name '*.so' \) -exec chmod 0755 {} +

mkdir -p "$out_dir"
out=$out_dir/$root_name.tar.gz
tmp_out=$out.tmp

tar -C "$work" \
    --sort=name \
    --mtime="@$epoch" \
    --owner=0 --group=0 --numeric-owner \
    --format=gnu \
    -cf - "$root_name" | gzip -9n > "$tmp_out"
mv "$tmp_out" "$out"

if command -v sha256sum >/dev/null 2>&1; then
    digest=$(sha256sum "$out" | awk '{print $1}')
else
    digest=$(shasum -a 256 "$out" | awk '{print $1}')
fi

printf 'Built %s\n' "$out"
printf 'Version %s, SOURCE_DATE_EPOCH %s\n' "$version" "$epoch"
printf 'Size %s bytes, sha256 %s\n' "$(wc -c < "$out" | tr -d ' ')" "$digest"
