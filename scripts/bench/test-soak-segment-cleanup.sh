#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DRIVER="${SOAK_CLEANUP_DRIVER:-$ROOT/scripts/run-merge-recovery-soak.sh}"
HARNESS="${SOAK_HARNESS_BIN:-$ROOT/target/debug/casper-soak}"
TMP="$(mktemp -d)"
cleanup() {
    local file
    for file in "$TMP"/*.pid; do
        [ ! -s "$file" ] || kill -KILL "$(cat "$file")" 2>/dev/null || true
    done
    rm -rf "$TMP"
}
trap cleanup EXIT
fail() {
    printf 'FAIL: %s\n' "$1" >&2
    cat "$TMP"/driver-*.log >&2 2>/dev/null || true
    exit 1
}
live() {
    local state
    state="$(awk '{print $3}' "/proc/$1/stat" 2>/dev/null)" || return 1
    [ "$state" != Z ]
}
mkdir -p "$TMP/bin" "$TMP/si"
cat >"$TMP/bin/poetry" <<'POETRY'
#!/usr/bin/env bash
trap 'exit 143' TERM INT
printf '%s\n' "$SOAK_PROCESS_OWNER" >"$CASE_ROOT/owner-$CASE_SEGMENT"
mkdir -p "$SYSTEM_INTEGRATION_DIR/integration-tests/.subprocess-data/$CASE_SEGMENT"
setsid bash -c 'printf "%s\n" "$$" >"$1"; while :; do sleep 0.1; done' \
    owned "$CASE_ROOT/owned-$CASE_SEGMENT.pid" </dev/null >/dev/null 2>&1 &
env -u SOAK_PROCESS_OWNER setsid bash -c 'printf "%s\n" "$$" >"$1"; while :; do printf x >>"$2/writer.log"; sleep 0.1; done' \
    residual "$CASE_ROOT/residual-$CASE_SEGMENT.pid" \
    "$SYSTEM_INTEGRATION_DIR/integration-tests/.subprocess-data/$CASE_SEGMENT" </dev/null >/dev/null 2>&1 &
printf 'fixture started\n'
while :; do sleep 0.1; done
POETRY
cat >"$TMP/bin/docker" <<'DOCKER'
#!/usr/bin/env bash
exit 0
DOCKER
cat >"$TMP/bin/harness" <<WRAPPER
#!/usr/bin/env bash
if [ "\${CASE_STOP_FAIL:-0}" = 1 ] && [ "\${1:-} \${2:-}" = 'host-control stop-owned' ]; then exit 2; fi
exec "$HARNESS" "\$@"
WRAPPER
chmod +x "$TMP/bin/poetry" "$TMP/bin/docker" "$TMP/bin/harness"
setsid bash -c 'printf "%s\n" "$$" >"$1"; while :; do sleep 0.1; done' unrelated "$TMP/unrelated.pid" \
    </dev/null >/dev/null 2>&1 &
run_segment() {
    local label="$1" output="$2" stop_fail="$3"
    status=0
    timeout --signal=TERM --kill-after=2 35 env PATH="$TMP/bin:$PATH" \
        SOAK_HARNESS_BIN="$TMP/bin/harness" CASE_ROOT="$TMP" CASE_SEGMENT="$label" CASE_STOP_FAIL="$stop_fail" \
        SOAK_DURATION_SECONDS=60 SOAK_DEADLINE_EPOCH="$(($(date +%s) + 3))" SOAK_OUTPUT_DIR="$output" SYSTEM_INTEGRATION_DIR="$TMP/si" \
        SOAK_RSS_CEILING_MB=0 SOAK_HOST_FREE_FLOOR_MB=0 SOAK_DISK_FREE_FLOOR_MB=0 \
        SOAK_CONTAINER_LOG_BUDGET_MB=0 SOAK_NODE_LOG_BUDGET_MB=0 \
        SOAK_GUARDIAN_POLL_SECONDS=0.1 SOAK_MONITOR_SNAPSHOT_SECONDS=0.1 \
        "$DRIVER" >"$TMP/driver-$label.log" 2>&1 || status=$?
    [ "$status" -ne 124 ] || fail 'segment exceeded its deadline'
}
for segment in first second; do
    run_segment "$segment" "$TMP/healthy" 0
    [ "$status" -eq 0 ] || fail 'healthy segment failed'
    [ -s "$TMP/owned-$segment.pid" ] || fail 'owned fixture never started'
    [ -s "$TMP/residual-$segment.pid" ] || fail 'residual fixture never started'
    if live "$(cat "$TMP/owned-$segment.pid")"; then fail 'owned writer survived'; fi
    if live "$(cat "$TMP/residual-$segment.pid")"; then fail 'residual writer survived'; fi
    live "$(cat "$TMP/unrelated.pid")" || fail 'unrelated process stopped'
done
[ "$(cat "$TMP/owner-first")" != "$(cat "$TMP/owner-second")" ] || fail 'segments reused an owner'
grep -q '^segments=2$' "$TMP/healthy/summary.txt" || fail 'segment history lost'
grep -q '^failures=0$' "$TMP/healthy/summary.txt" || fail 'healthy history failed'
run_segment failed "$TMP/failed" 1
[ "$status" -ne 0 ] || fail 'unconfirmed stop passed'
test -s "$TMP/failed/writer-stop-failure.txt" || fail 'stop failure marker missing'
grep -q '^writer_stop_failed:' "$TMP/failed/early-exit.txt" || fail 'stop reason missing'
grep -q '^FAILURES=1$' "$TMP/failed/.soak-state" || fail 'stop failure count missing'
run_segment restart "$TMP/failed" 1
[ "$status" -ne 0 ] || fail 'failed restart passed'
[ ! -e "$TMP/owner-restart" ] || fail 'failed restart launched work'
grep -q '^early_exit_reason=writer_stop_failed$' "$TMP/failed/summary.txt" || fail 'earlier reason overwritten'
grep -q '^FAILURES=1$' "$TMP/failed/.soak-state" || fail 'restart changed failure count'
printf 'Two-segment cleanup and failed restart passed.\n'
