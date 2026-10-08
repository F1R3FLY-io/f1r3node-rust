#!/usr/bin/env bash
# Checks the fixture failure report and the scenario timing of
# scripts/bench/test-soak-disk-admission.sh. Each check runs that script as a
# process and reads only its exit code, its output, and its evidence directory.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DISK_TEST="$ROOT/scripts/bench/test-soak-disk-admission.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

fail() {
    printf 'FAIL: %s\n' "$*" >&2
    exit 1
}

test_fixture_failure_reports_the_driver_state() {
    local out="$WORK/forced-timeout" status=0
    mkdir -p "$out"
    SOAK_DISK_TEST_DRIVER_TIMEOUT=1 bash "$DISK_TEST" --scenario log-within-budget "$ROOT" "$out/evidence" \
        >"$out/stdout" 2>"$out/stderr" || status=$?
    [[ "$status" == 2 ]] || fail "A forced driver timeout gave exit $status, not the fixture failure exit 2."
    grep -Fxq 'Fixture failure: log-within-budget' "$out/stderr" ||
        fail "The fixture failure report does not name the scenario."
    grep -Fxq 'Driver exit: 124' "$out/stderr" ||
        fail "The fixture failure report does not show the driver exit code."
    grep -Eq '^Summary: (missing|degraded: .+|complete)$' "$out/stderr" ||
        fail "The fixture failure report does not show the summary state."
    grep -Fxq 'Driver log (last 20 lines):' "$out/stderr" ||
        fail "The fixture failure report does not show the driver log."
    grep -Fq 'Soak host protection:' "$out/stderr" ||
        fail "The fixture failure report does not include the driver log lines."
    printf 'PASS: A fixture failure reports the scenario, the driver exit code, the summary state, and the driver log.\n'
}

test_fixture_failure_reports_the_driver_state
