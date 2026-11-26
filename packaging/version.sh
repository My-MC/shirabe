#!/bin/sh
# Prints the version from [workspace.package] in the workspace Cargo.toml.
# POSIX sh so the Linux, macOS and CI steps read the same single source.
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

version=$(
    awk '
        /^\[workspace\.package\]/ { in_section = 1; next }
        /^\[/ { in_section = 0 }
        in_section && $1 == "version" { gsub(/"/, "", $3); print $3; exit }
    ' "$repo_root/Cargo.toml"
)

if [ -z "$version" ]; then
    echo "error: cannot read [workspace.package] version from $repo_root/Cargo.toml" >&2
    exit 1
fi

printf '%s\n' "$version"
