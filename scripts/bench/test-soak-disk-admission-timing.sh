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

FORCED="$WORK/forced-timeout"

run_forced_timeout() {
    [[ ! -e "$FORCED/status" ]] || return 0
    local status=0
    mkdir -p "$FORCED"
    SOAK_DISK_TEST_DRIVER_TIMEOUT=1 bash "$DISK_TEST" --scenario log-within-budget "$ROOT" "$FORCED/evidence" \
        >"$FORCED/stdout" 2>"$FORCED/stderr" || status=$?
    printf '%s\n' "$status" >"$FORCED/status"
}

test_fixture_failure_reports_the_driver_state() {
    local out="$FORCED" status
    run_forced_timeout
    status="$(<"$out/status")"
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

test_fixture_failure_report_shows_no_host_path_or_key_value() {
    local report
    run_forced_timeout
    report="$(sed -n '/^Fixture failure: /,$p' "$FORCED/stderr")"
    [[ -n "$report" ]] || fail "The forced driver timeout produced no fixture failure report."
    [[ "$report" != *"$ROOT"* ]] || fail "The fixture failure report shows the host source directory."
    [[ "$report" != *"$HOME"* ]] || fail "The fixture failure report shows the host home directory."
    [[ "$report" != *"$FORCED"* ]] || fail "The fixture failure report shows a host evidence path."
    [[ "$report" != *fixture-not-used* && "$report" != *DEPLOYER_KEY* && "$report" != *PRIVATE_KEY* ]] ||
        fail "The fixture failure report shows a key value."
    printf 'PASS: The fixture failure report shows no host path and no key value.\n'
}

test_log_probe_vanished_reaches_its_verdict_within_a_third_of_its_timeout() {
    local out="$WORK/vanished" status=0 seconds
    mkdir -p "$out"
    bash "$DISK_TEST" --scenario log-probe-vanished "$ROOT" "$out/evidence" >"$out/stdout" 2>"$out/stderr" || status=$?
    [[ "$status" == 0 ]] || fail "log-probe-vanished gave exit $status, not a pass."
    seconds="$(jq -r '.[0].State | [.StartedAt, .FinishedAt] | map(sub("\\.[0-9]+"; "") | fromdateiso8601) | .[1] - .[0]' \
        "$out/evidence/container-finished.json")"
    [[ "$seconds" =~ ^[0-9]+$ ]] || fail "The log-probe-vanished container time is not readable."
    ((seconds * 3 < 50)) || fail "log-probe-vanished needed ${seconds} s, not less than one third of its 50 s driver timeout."
    printf 'PASS: log-probe-vanished reached its verdict in %s s, less than one third of its driver timeout.\n' "$seconds"
}

test_fixture_failure_reports_the_driver_state
test_fixture_failure_report_shows_no_host_path_or_key_value
test_log_probe_vanished_reaches_its_verdict_within_a_third_of_its_timeout
