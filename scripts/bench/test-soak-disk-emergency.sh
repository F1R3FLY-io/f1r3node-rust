#!/usr/bin/env bash
set -euo pipefail

if (($# > 2)); then
    printf 'Usage: %s [source-directory] [evidence-directory]\n' "$0" >&2
    exit 2
fi
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUTPUT="${2:-$(mktemp -d)}"
IMAGE="${SOAK_DISK_TEST_IMAGE:-}"
for test in active-probe record guardian-death probe-timeout diagnostic-deadline restart; do
    script="test-soak-disk-$test"
    [[ "$test" != guardian-death ]] || script="test-soak-guardian-death"
    SOAK_DISK_TEST_IMAGE="$IMAGE" bash "$ROOT/scripts/bench/$script.sh" \
        "${1:-$ROOT}" "$OUTPUT/$test"
    if [[ -z "$IMAGE" ]]; then
        IMAGE="$(jq -r '.[0].Id' "$OUTPUT/$test/image-inspect.json")"
    fi
done
printf 'PASS: The isolated disk emergency regressions completed.\n'
