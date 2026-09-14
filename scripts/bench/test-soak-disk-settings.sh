#!/usr/bin/env bash
set -euo pipefail
if (($# > 2)); then
    printf 'Usage: %s [source-directory] [evidence-directory]\n' "$0" >&2
    exit 2
fi
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUTPUT="${2:-$(mktemp -d)}"
IMAGE="${SOAK_DISK_TEST_IMAGE:-}"
for scenario in disk-floor-range disk-band-range disk-sum-range disk-max-floor disk-max-band; do
    SOAK_DISK_TEST_IMAGE="$IMAGE" SOAK_DISK_TEST_SCENARIO="$scenario" \
        bash "$ROOT/scripts/bench/test-soak-disk-admission.sh" "${1:-$ROOT}" "$OUTPUT/$scenario"
    if [[ -z "$IMAGE" ]]; then
        IMAGE="$(jq -r '.[0].Id' "$OUTPUT/$scenario/image-inspect.json")"
    fi
done
printf 'PASS: All five disk setting cases passed.\n'
