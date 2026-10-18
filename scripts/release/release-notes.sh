#!/usr/bin/env bash
# Print the GitHub Release notes for the archive in DIST_DIR: its checksum
# followed by the archive's own README, so the release page and the download
# give the same requirements and installation steps.
#
# Usage: release-notes.sh DIST_DIR
set -euo pipefail

dist="$1"
archive="$(cd "$dist" && ls plenty-*.tar.gz)"
name="${archive%.tar.gz}"
cat <<EOF
Download \`$archive\`. SHA-256:

\`\`\`text
$(cat "$dist/$archive.sha256")
\`\`\`

Linux x86-64 with glibc 2.28 or later, plus a C compiler driver (\`cc\`) for
linking. This is not a self-contained toolchain; see Requirements below.

EOF
# The README with its title demoted under the release title.
tar -xzOf "$dist/$archive" "$name/README.md" | sed '1s/^# /## /; 2,$s/^## /### /'
