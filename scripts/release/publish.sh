#!/usr/bin/env bash
# Create or resume the GitHub Release for TAG and publish it.
#
# Usage: publish.sh TAG PRERELEASE DIST_DIR
#
# Used by the release workflow after every build and smoke job has passed.
# Needs `gh` with GH_TOKEN (contents: write) and GITHUB_REPOSITORY.
#
# - No release for TAG: create a draft, upload both archives and checksums,
#   then publish it. Readers never see a release with missing assets.
# - A draft from an interrupted run: replace its assets and notes, publish.
# - Already published: succeed without changes when its assets have the same
#   checksums (a rerun of a finished release), else fail. A published
#   release is never modified; fix problems with a new version.
#
# PRERELEASE=true publishes a GitHub pre-release that is not marked latest.
set -euo pipefail

tag="$1"
prerelease="$2"
dist="$3"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY must name the repository}"

assets=()
checksums=()
for target in ${PLENTY_RELEASE_TARGETS:-x86_64-unknown-linux-gnu}; do
    case "$target" in x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu) ;; *) echo "publish.sh: unsupported target $target" >&2; exit 1 ;; esac
    for component in plenty plenty-dataframe plenty-linalg; do
        archive="$component-${tag#v}-$target.tar.gz"
        assets+=("$dist/$archive" "$dist/$archive.sha256")
        checksums+=("$archive.sha256")
    done
done
mapfile -t assets < <(printf '%s\n' "${assets[@]}" | sort)
for asset in "${assets[@]}"; do
    [ -f "$asset" ] || { echo "publish.sh: missing release asset $asset" >&2; exit 1; }
done
actual=("$dist"/*.tar.gz "$dist"/*.sha256)
[ "${#actual[@]}" = "${#assets[@]}" ] || { echo "publish.sh: unexpected archive/checksum set in $dist" >&2; exit 1; }
(cd "$dist" && sha256sum -c ./*.sha256)

notes="$(mktemp)"
published=""
trap 'rm -f "$notes" "$notes.err"; if [ -n "$published" ]; then rm -rf "$published"; fi' EXIT
"$here/release-notes.sh" "$dist" > "$notes"

flags=()
latest=true
if [ "$prerelease" = true ]; then
    flags+=(--prerelease)
    latest=false
fi

# Only "not found" means no release; any other failure must not create a
# second draft for the same tag.
if ! state="$(gh release view "$tag" --repo "$repo" --json isDraft --jq .isDraft 2>"$notes.err")"; then
    if grep -qi 'not found' "$notes.err"; then
        state=missing
    else
        cat "$notes.err" >&2
        exit 1
    fi
fi
rm -f "$notes.err"
case "$state" in
    false)
        published="$(mktemp -d)"
        gh release download "$tag" --repo "$repo" --pattern '*.sha256' --dir "$published"
        matches=1
        for checksum in "${checksums[@]}"; do
            cmp -s "$published/$checksum" "$dist/$checksum" || matches=0
        done
        count="$(find "$published" -maxdepth 1 -name '*.sha256' | wc -l)"
        if [ "$matches" = 1 ] && [ "$count" = "${#checksums[@]}" ]; then
            echo "publish.sh: $tag is already published with these assets; nothing to do"
            exit 0
        fi
        echo "publish.sh: $tag is already published with different assets; publish a new version instead" >&2
        exit 1
        ;;
    true)
        echo "publish.sh: resuming draft release $tag"
        gh release upload "$tag" "${assets[@]}" --repo "$repo" --clobber
        gh release edit "$tag" --repo "$repo" --notes-file "$notes" "${flags[@]}"
        ;;
    missing)
        gh release create "$tag" "${assets[@]}" --repo "$repo" --draft --verify-tag \
            --title "Plenty ${tag#v}" --notes-file "$notes" "${flags[@]}"
        ;;
    *)
        echo "publish.sh: unexpected release state '$state'" >&2
        exit 1
        ;;
esac
gh release edit "$tag" --repo "$repo" --draft=false --latest="$latest" "${flags[@]}"
echo "publish.sh: published $tag"
