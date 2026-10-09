#!/usr/bin/env bash
# Checks the fixture failure report and the scenario timing of
# scripts/bench/test-soak-disk-admission.sh. Each check runs that script as a
# process and reads only its exit code, its output, and its evidence directory.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DISK_TEST="$ROOT/scripts/bench/test-soak-disk-admission.sh"
WORK="$(mktemp -d)"
STRESS_BURNER=""
trap '[[ -z "$STRESS_BURNER" ]] || docker rm -f "$STRESS_BURNER" >/dev/null 2>&1
    rm -rf "$WORK"' EXIT

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

assert_log_scenario_within_a_third_of_its_timeout() {
    local scenario="$1" out="$WORK/timing-$1" status=0 seconds
    if [[ "${SOAK_DISK_TEST_SKIP_TIMING:-0}" == 1 ]]; then
        printf 'SKIP: SOAK_DISK_TEST_SKIP_TIMING=1 skips the %s timing check.\n' "$scenario"
        return 0
    fi
    mkdir -p "$out"
    bash "$DISK_TEST" --scenario "$scenario" "$ROOT" "$out/evidence" >"$out/stdout" 2>"$out/stderr" || status=$?
    [[ "$status" == 0 ]] || fail "$scenario gave exit $status, not a pass."
    seconds="$(jq -r '.[0].State | [.StartedAt, .FinishedAt] | map(sub("\\.[0-9]+"; "") | fromdateiso8601) | .[1] - .[0]' \
        "$out/evidence/container-finished.json")"
    [[ "$seconds" =~ ^[0-9]+$ ]] || fail "The $scenario container time is not readable."
    ((seconds * 3 < 50)) || fail "$scenario needed ${seconds} s, not less than one third of its 50 s driver timeout."
    printf 'PASS: %s reached its verdict in %s s, less than one third of its driver timeout.\n' "$scenario" "$seconds"
}

test_log_probe_vanished_reaches_its_verdict_within_a_third_of_its_timeout() {
    assert_log_scenario_within_a_third_of_its_timeout log-probe-vanished
}

test_log_within_budget_and_sudo_fallback_finish_within_a_third_of_their_timeout() {
    assert_log_scenario_within_a_third_of_its_timeout log-within-budget
    assert_log_scenario_within_a_third_of_its_timeout log-sudo-fallback
}

test_full_iteration_scenarios_survive_cpu_saturation() {
    local rounds="${SOAK_DISK_TEST_STRESS_ROUNDS:-0}" out="$WORK/stress" image harness cpus burner_cpus round scenario
    local -a scenarios=(cleanup-sufficient log-within-budget log-sudo-fallback log-budget-disabled log-probe-vanished benchmark-disabled)
    local -a broken=()
    if [[ "$rounds" == 0 ]]; then
        printf 'SKIP: Set SOAK_DISK_TEST_STRESS_ROUNDS to run the CPU saturation rounds.\n'
        return 0
    fi
    [[ "$rounds" =~ ^[1-9][0-9]*$ ]] || fail "SOAK_DISK_TEST_STRESS_ROUNDS must be a positive integer."
    mkdir -p "$out"
    bash "$DISK_TEST" --scenario cleanup-sufficient "$ROOT" "$out/prime" >"$out/prime.log" 2>&1 ||
        fail "The priming scenario did not pass."
    image="$(<"$out/prime/image-id.txt")"
    harness="$(awk '{print $2}' "$out/prime/harness.sha256")"
    cpus="$(docker info --format '{{.NCPU}}')"
    burner_cpus=1
    ((cpus <= 2)) || burner_cpus=$((cpus - 2))
    STRESS_BURNER="$(docker run -d --rm --network none --cpus "$burner_cpus" --entrypoint bash "$image" \
        -c "for _ in \$(seq 1 $((burner_cpus * 2))); do while :; do :; done & done; wait")"
    for round in $(seq 1 "$rounds"); do
        for scenario in "${scenarios[@]}"; do
            (
                status=0
                SOAK_DISK_TEST_IMAGE="$image" SOAK_DISK_TEST_HARNESS_BIN="$harness" \
                    bash "$DISK_TEST" --scenario "$scenario" "$ROOT" "$out/$round-$scenario" \
                    >"$out/$round-$scenario.log" 2>&1 || status=$?
                printf '%s\n' "$status" >"$out/$round-$scenario.status"
            ) &
        done
        wait
    done
    docker rm -f "$STRESS_BURNER" >/dev/null 2>&1 || true
    STRESS_BURNER=""
    for round in $(seq 1 "$rounds"); do
        for scenario in "${scenarios[@]}"; do
            [[ "$(<"$out/$round-$scenario.status")" == 0 ]] || broken+=("$round-$scenario")
        done
    done
    ((${#broken[@]} == 0)) || fail "Under CPU saturation these runs gave no pass: ${broken[*]}."
    printf 'PASS: %s rounds of the %s full-iteration scenarios passed under CPU saturation.\n' "$rounds" "${#scenarios[@]}"
}

test_bounded_command_returns_when_its_watchdog_starts_late() {
    local image body elapsed
    run_forced_timeout
    image="$(<"$FORCED/evidence/image-id.txt")"
    body="$(awk '/^session_bounded\(\) \{$/,/^}$/' "$ROOT/scripts/run-merge-recovery-soak.sh")"
    [[ -n "$body" ]] || fail "session_bounded is missing from the soak driver."
    elapsed="$({
        printf '%s\n' "$body"
        cat <<'EOF'
mkdir -p /tmp/late-setsid
cat >/tmp/late-setsid/setsid <<'SH'
#!/usr/bin/env bash
if [[ "${1:-}" == bash ]]; then
    : >>/tmp/late-setsid/delayed
    sleep 0.5
fi
exec /usr/bin/setsid "$@"
SH
chmod +x /tmp/late-setsid/setsid
PATH="/tmp/late-setsid:$PATH"
start="$(date +%s%N)"
session_bounded 8 true
elapsed="$((($(date +%s%N) - start) / 1000000))"
[[ -e /tmp/late-setsid/delayed ]] || elapsed=no-delay
printf '%s\n' "$elapsed"
EOF
    } | docker run --rm -i --network none --cap-drop ALL --security-opt no-new-privileges --user 65534:65534 \
        --entrypoint bash "$image" -s)"
    [[ "$elapsed" != no-delay ]] ||
        fail "The setsid stand-in did not delay the watchdog, so the check did not force the race. session_bounded no longer starts its watchdog with setsid bash."
    [[ "$elapsed" =~ ^[0-9]+$ ]] || fail "The late-watchdog check printed no elapsed time."
    ((elapsed < 3000)) ||
        fail "session_bounded waited ${elapsed} ms for a command that had ended, because its watchdog started late."
    printf 'PASS: session_bounded returned in %s ms after the command ended, with a late watchdog.\n' "$elapsed"
}

test_fixture_failure_reports_the_driver_state
test_fixture_failure_report_shows_no_host_path_or_key_value
test_bounded_command_returns_when_its_watchdog_starts_late
test_log_probe_vanished_reaches_its_verdict_within_a_third_of_its_timeout
test_log_within_budget_and_sudo_fallback_finish_within_a_third_of_their_timeout
test_full_iteration_scenarios_survive_cpu_saturation
