#!/usr/bin/env bash
set -euo pipefail

if (($# > 2)); then
    printf 'Usage: %s [source-directory] [evidence-directory]\n' "$0" >&2
    exit 2
fi
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUTPUT="${2:-$(mktemp -d)}"
IMAGE="${SOAK_DISK_TEST_IMAGE:-}"
for test in active-probe record guardian-death probe-timeout diagnostic-deadline restart stop-deadline guardian-admission benchmark-restart benchmark-disk-admission benchmark-disk-monitor benchmark-admission-cases benchmark-cancellation benchmark-guardian-admission guardian-progress hygiene-deadline settings cleanup-ownership cleanup-outcomes; do
    script="test-soak-disk-$test"
    [[ "$test" != guardian-death && "$test" != guardian-admission && "$test" != benchmark-restart && "$test" != benchmark-disk-admission && "$test" != benchmark-disk-monitor && "$test" != benchmark-admission-cases && "$test" != benchmark-cancellation && "$test" != benchmark-guardian-admission && "$test" != guardian-progress && "$test" != cleanup-ownership && "$test" != cleanup-outcomes ]] || script="test-soak-$test"
    SOAK_DISK_TEST_IMAGE="$IMAGE" bash "$ROOT/scripts/bench/$script.sh" \
        "${1:-$ROOT}" "$OUTPUT/$test"
    if [[ -z "$IMAGE" ]]; then
        IMAGE="$(jq -r '.[0].Id' "$OUTPUT/$test/image-inspect.json")"
    fi
done
SOAK_DISK_TEST_IMAGE="$IMAGE" bash "$ROOT/scripts/bench/test-soak-host-stop-ownership.sh" \
    "${1:-$ROOT}" "$OUTPUT/host-stop-ownership"
SOAK_HOST_STOP_SCENARIO=memory SOAK_DISK_TEST_IMAGE="$IMAGE" bash "$ROOT/scripts/bench/test-soak-host-stop-ownership.sh" \
    "${1:-$ROOT}" "$OUTPUT/memory-stop-ownership"
SOAK_HOST_STOP_SCENARIO=oom SOAK_DISK_TEST_IMAGE="$IMAGE" bash "$ROOT/scripts/bench/test-soak-host-stop-ownership.sh" \
    "${1:-$ROOT}" "$OUTPUT/host-oom-ownership"
SOAK_DISK_TEST_IMAGE="$IMAGE" bash "$ROOT/scripts/bench/test-soak-crash-monitor-death.sh" \
    "${1:-$ROOT}" "$OUTPUT/crash-monitor-death"
SOAK_DISK_TEST_IMAGE="$IMAGE" bash "$ROOT/scripts/bench/test-soak-benchmark-monitor-death.sh" \
    "${1:-$ROOT}" "$OUTPUT/benchmark-monitor-death"
for mode in benchmark iteration; do
    SOAK_MONITOR_ADMISSION_MODE="$mode" SOAK_DISK_TEST_IMAGE="$IMAGE" bash "$ROOT/scripts/bench/test-soak-monitor-admission.sh" \
        "${1:-$ROOT}" "$OUTPUT/monitor-admission-$mode"
done
SOAK_DISK_TEST_IMAGE="$IMAGE" bash "$ROOT/scripts/bench/test-soak-monitor-inherited-pipe.sh" \
    "${1:-$ROOT}" "$OUTPUT/monitor-inherited-pipe"
printf 'PASS: The isolated disk emergency regressions completed.\n'
