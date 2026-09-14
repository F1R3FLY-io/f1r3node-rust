#!/usr/bin/env bash
set -euo pipefail
if (($# > 2)); then
    printf 'Usage: %s [source-directory] [evidence-directory]\n' "$0" >&2
    exit 2
fi
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUTPUT="${2:-$(mktemp -d)}"
IMAGE="${SOAK_DISK_TEST_IMAGE:-}"
for action in list remove network image builder partial sufficient; do
    scenario="cleanup-error-$action"
    if [[ "$action" == partial || "$action" == sufficient ]]; then scenario="cleanup-$action"; fi
    SOAK_DISK_TEST_IMAGE="$IMAGE" SOAK_DISK_TEST_SCENARIO="$scenario" \
        bash "$ROOT/scripts/bench/test-soak-disk-admission.sh" "${1:-$ROOT}" "$OUTPUT/$action"
    if [[ -z "$IMAGE" ]]; then
        IMAGE="$(jq -r '.[0].Id' "$OUTPUT/$action/image-inspect.json")"
    fi
done
printf 'PASS: All five cleanup error cases and two reclamation cases passed.\n'
