#!/usr/bin/env bash
# Print the GitHub Release notes for the archives in DIST_DIR: for each target
# in PLENTY_RELEASE_TARGETS (default x86_64-unknown-linux-gnu), its checksums
# followed by the archive's own README, so the release page and the download
# give the same requirements and installation steps.
#
# Usage: release-notes.sh DIST_DIR
set -euo pipefail

dist="$1"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
for target in ${PLENTY_RELEASE_TARGETS:-x86_64-unknown-linux-gnu}; do
    PLENTY_RELEASE_TARGET="$target"
    # shellcheck source=target.sh
    . "$here/target.sh"
    paths=("$dist"/plenty-[0-9]*-"$target.$release_archive_ext")
    if [ "${#paths[@]}" != 1 ] || [ ! -f "${paths[0]}" ]; then
        echo "release-notes.sh: expected one $target compiler archive in $dist" >&2
        exit 1
    fi
    archive="${paths[0]##*/}"
    name="${archive%".$release_archive_ext"}"
    sums=""
    for component in $release_components; do
        sums+="$(cat "$dist/$component-${archive#plenty-}.sha256")"$'\n'
    done
    case "$release_os" in
        linux)
            summary="GNU/Linux with glibc 2.28 or later, plus a C compiler driver (\`cc\`) for linking.
Select the archive matching your CPU architecture. Each optional shared library
uses the same release and architecture as the compiler. For \`std.dataframe\`, also
install \`plenty-dataframe-${archive#plenty-}\`; for \`std.linalg\`, install \`plenty-linalg-${archive#plenty-}\`." ;;
        macos)
            summary="**Experimental:** macOS ${release_macos_min%.0} or later on Apple silicon, plus the Xcode
Command Line Tools for linking. Not notarized; the optional engines are not
available for macOS." ;;
        windows)
            summary="**Experimental:** 64-bit Windows 10 or later on x86-64, plus Visual Studio
Build Tools with the C++ workload for linking. Not signed; the optional engines
are not available for Windows." ;;
    esac
    # shellcheck disable=SC2016 # Markdown backticks, not expansions
    printf 'Download `%s`. SHA-256:\n\n```text\n%s```\n\n%s\n\n' "$archive" "$sums" "$summary"
    if [ "$release_archive_ext" = zip ]; then
        unzip -p "$dist/$archive" "$name/README.md"
    else
        tar -xzOf "$dist/$archive" "$name/README.md"
    fi | sed '1s/^# /## /; 2,$s/^## /### /'
    echo
done
