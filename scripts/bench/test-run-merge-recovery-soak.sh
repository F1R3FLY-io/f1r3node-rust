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
# Docker sessions write monitor artifacts under log-archive/ (the provider's
# host-visible per-session dir); subprocess sessions write under data/. The
# monitor CSV goes to the archive root and the metrics CSV to the data root
# so a driver that searches only one of them fails this test.
mkdir -p "$FAKE_ARCHIVE_DIR/session" "$FAKE_DATA_DIR/session"
cat >"$FAKE_ARCHIVE_DIR/session/resource-timeseries.csv" <<'CSV'
elapsed_s,node,memory_mb,cpu_percent,memory_limit_mb
1.0,rnode.test.validator1,256.0,10.0,0
1.0,rnode.test.validator2,512.0,20.0,0
CSV
# Per-core telemetry for validator1 only: validator2 must keep its "all"
# fallback row in the summary grid (mixed real/fallback rendering). The
# __system__ row is host state and must not become a grid node, and the
# non-numeric core id is a malformed row that must be rejected — the grid
# assertion below proves both filters.
# CRLF endings on purpose: the harness writes this CSV with Python
# csv.writer, whose default line terminator is \r\n — cpu_percent is the
# LAST column, so without CR-stripping every row fails the numeric check
# (smoke run 31547587950 published an all-fallback grid exactly this way).
cat <<'CSV' | sed 's/$/\r/' >"$FAKE_ARCHIVE_DIR/session/resource-percore-timeseries.csv"
elapsed_s,node,core,cpu_percent
1.0,rnode.test.validator1,0,7.5
1.0,rnode.test.validator1,1,42.0
1.0,__system__,0,93.0
1.0,rnode.test.validator1,not-a-core,88.0
CSV
cat >"$FAKE_DATA_DIR/session/node-metrics-timeseries.csv" <<'CSV'
elapsed_s,node,metric,value
1.0,rnode.test.validator1,replay_cache_entries,12
1.0,rnode.test.validator1,replay_cache_retained_bytes,1048576
CSV
# The identical node log in BOTH roots (teardown archives a copy of a log
# that also lives under data/): counting metrics must see it once.
cat >"$FAKE_DATA_DIR/session/validator1.log" <<'LOG'
proposal rejected: too far ahead of the last finalized block
LOG
cp "$FAKE_DATA_DIR/session/validator1.log" "$FAKE_ARCHIVE_DIR/session/validator1.log"
printf 'fake pytest started\n'
while :; do sleep 1; done
SH
cat >"$TMP/bin/docker" <<'SH'
#!/usr/bin/env bash
case "$1" in
	ps)
		if printf '%s\n' "$*" | grep -q -- '--format' && [ -s "$FAKE_POETRY_PID_FILE" ]; then
			printf 'rnode.test.validator1\n'
		fi
		;;
	inspect) cat "$FAKE_POETRY_PID_FILE" ;;
	port) printf '0.0.0.0:40413\n' ;;
	rm|kill) exit 0 ;;
esac
SH
cat >"$TMP/bin/curl" <<'SH'
#!/usr/bin/env bash
cat <<'METRICS'
replay_cache_entries{source="casper"} 12
replay_cache_retained_bytes{source="casper"} 1048576
block_processing_active{source="block-processor"} 2
block_processing_parallel_limit{source="block-processor"} 2
block_processing_queue_pending{source="block-processor"} 7
METRICS
SH
chmod +x "$TMP/bin/poetry" "$TMP/bin/docker" "$TMP/bin/curl"

PATH="$TMP/bin:$PATH" \
	FAKE_POETRY_PID_FILE="$TMP/fake-poetry.pid" \
	FAKE_DATA_DIR="$TMP/system-integration/integration-tests/data" \
	FAKE_ARCHIVE_DIR="$TMP/system-integration/integration-tests/log-archive" \
	SOAK_DURATION_SECONDS=120 \
	SYSTEM_INTEGRATION_DIR="$TMP/system-integration" \
	SOAK_OUTPUT_DIR="$TMP/output" \
	SOAK_RSS_CEILING_MB=0 \
	SOAK_HOST_FREE_FLOOR_MB=0 \
	SOAK_GUARDIAN_POLL_SECONDS=1 \
	SOAK_MONITOR_SNAPSHOT_SECONDS=0.1 \
	"$ROOT/scripts/run-merge-recovery-soak.sh" >"$TMP/driver.log" 2>&1 &
DRIVER_PID=$!

for _ in $(seq 1 20); do
	[ -e "$TMP/output/iteration-00001-docker/.started" ] && break
	sleep 0.25
done
test -e "$TMP/output/iteration-00001-docker/.started"
for _ in $(seq 1 20); do
	[ -s "$TMP/output/iteration-00001-docker/node-metrics-timeseries.csv" ] &&
		grep -q 'test.validator1.*12.*1048576.*2.*2.*7' \
			"$TMP/output/iteration-00001-docker/node-memory-timeseries.tsv" 2>/dev/null && break
	sleep 0.25
done
for _ in $(seq 1 20); do
	[ -s "$TMP/output/iteration-00001-docker/resource-percore-timeseries.csv" ] && break
	sleep 0.25
done
test -s "$TMP/output/iteration-00001-docker/resource-timeseries.csv"
test -s "$TMP/output/iteration-00001-docker/resource-percore-timeseries.csv"
test -s "$TMP/output/iteration-00001-docker/node-metrics-timeseries.csv"
grep -q 'test.validator1.*12.*1048576.*2.*2.*7' \
	"$TMP/output/iteration-00001-docker/node-memory-timeseries.tsv"
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
jq -e '
  .iterations == 1
  and .failures == 1
  and .rss_peak_mb == 768
  and .cpu_peak_pct == 30
  and .cpu_peak_core_grid_pct == {"validator1": {"0": 7.5, "1": 42}, "validator2": {"all": 20}}
  and .iteration_metrics[0].exit_code == 1
  and .iteration_metrics[0].rss_peak_mb == 768
  and .iteration_metrics[0].cpu_peak_per_node_pct == {"validator1": 10, "validator2": 20}
  and .iteration_metrics[0].cpu_peak_per_node_core_pct == {"validator1": {"0": 7.5, "1": 42}}
  and .iteration_metrics[0].too_far_ahead_errors == 1
' "$TMP/output/summary.json" >/dev/null
grep -q 'replay_cache_retained_bytes,1048576' \
	"$TMP/output/iteration-00001-docker/node-metrics-timeseries.csv"
test ! -e "$TMP/output/iteration-00001-docker/.pytest-output.fifo"
test -s "$TMP/fake-poetry.pid"
! kill -0 "$(cat "$TMP/fake-poetry.pid")" 2>/dev/null

printf 'soak fail-closed driver tests passed\n'
