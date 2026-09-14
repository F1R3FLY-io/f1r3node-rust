#!/usr/bin/env bash
set -euo pipefail

if (($# > 2)); then
    printf 'Usage: %s [source-directory] [evidence-directory]\n' "$0" >&2
    exit 2
fi
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUTPUT="${2:-$(mktemp -d)}"
IMAGE="${SOAK_DISK_TEST_IMAGE:-}"
result=0
for scenario in restart-uncounted restart-counted; do
    status=0
    SOAK_DISK_TEST_SCENARIO="$scenario" SOAK_DISK_TEST_IMAGE="$IMAGE" \
        bash "$ROOT/scripts/bench/test-soak-disk-admission.sh" \
        "${1:-$ROOT}" "$OUTPUT/$scenario" || status=$?
    if ((status != 0 && status != 1)); then
        printf 'ERROR: The restart fixture failed without a behavioral verdict.\n' >&2
        exit 2
    fi
    if ((status == 1)); then result=1; fi
    IMAGE="$(jq -r '.[0].Id' "$OUTPUT/$scenario/image-inspect.json")"
done
exit "$result"
