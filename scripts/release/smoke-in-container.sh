#!/usr/bin/env bash
# Run smoke-test.sh against ARCHIVE in a clean container of IMAGE.
#
# Usage: smoke-in-container.sh IMAGE ARCHIVE [smoke-test options]
#
# Starts IMAGE, installs only the prerequisites the archive README documents
# for that distribution, disconnects the container from the network, copies
# in the archive, its checksum, and the smoke test (no source checkout or
# Rust toolchain), and runs the test with --clean-environment. The container
# is labelled org.plenty.release-smoke and removed afterwards.
set -euo pipefail

image="$1"
archive="$2"
shift 2
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Keep in step with the commands in README.dist.md and the installation page.
case "${image%%[:@]*}" in
    debian|ubuntu) prerequisites='apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y gcc libc6-dev' ;;
    fedora|almalinux) prerequisites='dnf install -y gcc tar gzip' ;;
    archlinux) prerequisites='pacman -Syu --needed --noconfirm gcc' ;;
    *) echo "smoke-in-container: no documented prerequisites for $image" >&2; exit 2 ;;
esac

container="$(docker run -d --label org.plenty.release-smoke=1 "$image" sleep infinity)"
trap 'docker rm -f "$container" >/dev/null' EXIT
echo "smoke-in-container: $image ($container)"
docker exec "$container" sh -c "$prerequisites" > /dev/null
docker exec "$container" sh -c '. /etc/os-release && echo "$PRETTY_NAME" && ldd --version | head -n1'
for network in $(docker inspect -f '{{range $name, $_ := .NetworkSettings.Networks}}{{$name}} {{end}}' "$container"); do
    docker network disconnect "$network" "$container"
done
docker exec "$container" mkdir -p /opt/check
docker cp "$archive" "$container:/opt/check/"
docker cp "$archive.sha256" "$container:/opt/check/"
docker cp "$here/smoke-test.sh" "$container:/opt/check/"
docker exec -w / "$container" bash /opt/check/smoke-test.sh \
    "/opt/check/$(basename "$archive")" --clean-environment "$@"
