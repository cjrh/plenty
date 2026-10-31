#!/usr/bin/env bash
# Print the GitHub Release notes for the archive in DIST_DIR: its checksum
# followed by the archive's own README, so the release page and the download
# give the same requirements and installation steps.
#
# Usage: release-notes.sh DIST_DIR
set -euo pipefail

dist="$1"
for path in "$dist"/plenty-[0-9]*.tar.gz; do
archive="${path##*/}"
name="${archive%.tar.gz}"
engine="plenty-dataframe-${archive#plenty-}"
linalg="plenty-linalg-${archive#plenty-}"
cat <<EOF
Download \`$archive\`. SHA-256:

\`\`\`text
$(cat "$dist/$archive.sha256")
$(cat "$dist/$engine.sha256")
$(cat "$dist/$linalg.sha256")
\`\`\`

GNU/Linux with glibc 2.28 or later, plus a C compiler driver (\`cc\`) for linking.
Select the archive matching your CPU architecture. Each optional shared library
uses the same release and architecture as the compiler. For \`std.dataframe\`, also
install \`$engine\`; for \`std.linalg\`, install \`$linalg\`.

EOF
tar -xzOf "$dist/$archive" "$name/README.md" | sed '1s/^# /## /; 2,$s/^## /### /'
done
