#!/usr/bin/env bash
set -euo pipefail

IMAGE="${1:?Supply an image name.}"
EXPECTED="${2:?Supply the expected build identity.}"
if [[ "$EXPECTED" == unknown || "$EXPECTED" =~ [[:space:][:cntrl:]] ]]; then
	echo 'The expected build identity must be known and contain no whitespace or control characters.' >&2
	exit 1
fi

TMP="$(mktemp -d)"
CONTAINER=
cleanup() {
	if [ -n "$CONTAINER" ]; then
		docker rm "$CONTAINER" >/dev/null
	fi
	rm -rf "$TMP"
}
trap cleanup EXIT

CONTAINER="$(docker create "$IMAGE")"
docker cp "$CONTAINER:/opt/docker/bin/node" "$TMP/node"
if ! grep -aFq -- "$EXPECTED" "$TMP/node"; then
	printf 'The node binary in %s does not contain build identity %s.\n' "$IMAGE" "$EXPECTED" >&2
	exit 1
fi
printf 'PASS: The node binary in %s contains build identity %s.\n' "$IMAGE" "$EXPECTED"
