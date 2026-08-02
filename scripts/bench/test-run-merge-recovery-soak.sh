#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
DRIVER_PID=""
cleanup() {
	[ -z "$DRIVER_PID" ] || kill "$DRIVER_PID" 2>/dev/null || true
	rm -rf "$TMP"
}
trap cleanup EXIT
mkdir -p "$TMP/bin" "$TMP/system-integration"
cat >"$TMP/bin/poetry" <<'SH'
#!/usr/bin/env bash
trap 'exit 143' TERM INT
printf '%s\n' "$$" >"$FAKE_POETRY_PID_FILE"
printf 'fake pytest started\n'
while :; do sleep 1; done
SH
chmod +x "$TMP/bin/poetry"

PATH="$TMP/bin:$PATH" \
FAKE_POETRY_PID_FILE="$TMP/fake-poetry.pid" \
SOAK_DURATION_SECONDS=120 \
	SYSTEM_INTEGRATION_DIR="$TMP/system-integration" \
	SOAK_OUTPUT_DIR="$TMP/output" \
	SOAK_RSS_CEILING_MB=0 \
	SOAK_HOST_FREE_FLOOR_MB=0 \
	SOAK_GUARDIAN_POLL_SECONDS=1 \
	"$ROOT/scripts/run-merge-recovery-soak.sh" >"$TMP/driver.log" 2>&1 &
DRIVER_PID=$!

for _ in $(seq 1 20); do
	[ -e "$TMP/output/iteration-00001-docker/.started" ] && break
	sleep 0.25
done
test -e "$TMP/output/iteration-00001-docker/.started"
printf '%s\n' 'injected orchestrator host guardian breach' \
	>"$TMP/output/host-guardian-breach.txt"

for _ in $(seq 1 20); do
	kill -0 "$DRIVER_PID" 2>/dev/null || break
	sleep 0.5
done
if kill -0 "$DRIVER_PID" 2>/dev/null; then
	cat "$TMP/driver.log" >&2
	echo 'soak driver did not stop promptly after guardian breach' >&2
	exit 1
fi
set +e
wait "$DRIVER_PID"
status=$?
set -e
DRIVER_PID=""
[ "$status" -eq 1 ]

test "$(find "$TMP/output" -maxdepth 1 -type d -name 'iteration-*' | wc -l | tr -d ' ')" = 1
grep -q '^host_protection_breach:' "$TMP/output/early-exit.txt"
grep -q '^early_exit_reason=host_protection_breach$' "$TMP/output/summary.txt"
jq -e '.iterations == 1 and .failures == 1 and .iteration_metrics[0].exit_code == 1' \
	"$TMP/output/summary.json" >/dev/null
test ! -e "$TMP/output/iteration-00001-docker/.pytest-output.fifo"
test -s "$TMP/fake-poetry.pid"
! kill -0 "$(cat "$TMP/fake-poetry.pid")" 2>/dev/null

printf 'soak fail-closed driver tests passed\n'
