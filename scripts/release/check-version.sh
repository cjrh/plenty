#!/usr/bin/env bash
# Release version checks.
#
#   check-version.sh --print        print the `plenty` package version
#   check-version.sh --tag TAG      require TAG == v<package version>; print
#                                   version=... and prerelease=true|false
#                                   (also appended to $GITHUB_OUTPUT if set)
#
# Release tags are vMAJOR.MINOR.PATCH, optionally with a SemVer pre-release
# suffix such as -rc.1. A pre-release tag publishes a GitHub pre-release.
# The runtime crate keeps its own version and is never tagged.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

package_version() {
    # The first `version` key of the [package] table in the root manifest.
    awk '
        /^\[/ { in_package = ($0 == "[package]") }
        in_package && /^version[[:space:]]*=/ {
            sub(/^version[[:space:]]*=[[:space:]]*"/, ""); sub(/".*/, ""); print; exit
        }
    ' "$repo/Cargo.toml"
}

version="$(package_version)"
if [ -z "$version" ]; then
    echo "check-version.sh: no [package] version in $repo/Cargo.toml" >&2
    exit 1
fi

case "${1:-}" in
    --print)
        echo "$version"
        ;;
    --tag)
        tag="${2:?--tag requires a tag name}"
        semver='^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'
        if ! [[ "$tag" =~ $semver ]]; then
            echo "check-version.sh: '$tag' is not a release tag (vX.Y.Z or vX.Y.Z-PRE)" >&2
            exit 1
        fi
        if [ "$tag" != "v$version" ]; then
            echo "check-version.sh: tag $tag does not match Cargo.toml version $version" >&2
            exit 1
        fi
        prerelease=false
        case "$version" in *-*) prerelease=true ;; esac
        printf 'version=%s\nprerelease=%s\n' "$version" "$prerelease" | tee -a "${GITHUB_OUTPUT:-/dev/null}"
        ;;
    *)
        echo "usage: check-version.sh --print | --tag TAG" >&2
        exit 2
        ;;
esac
