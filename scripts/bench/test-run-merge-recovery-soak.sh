#!/usr/bin/env bash
set -Eeuo pipefail
# Many assertions are plain test and grep commands. Name the one that fails.
trap '[[ $- == *e* ]] && printf "FAIL: line %s: %s\n" "$LINENO" "$BASH_COMMAND" >&2' ERR

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
DRIVER_PID=""
cleanup() {
	local pid_file
	[ -z "$DRIVER_PID" ] || kill "$DRIVER_PID" 2>/dev/null || true
	for pid_file in "$TMP"/fake-writer-*.pid "$TMP/unrelated.pid"; do
		[ ! -s "$pid_file" ] || kill -KILL "$(cat "$pid_file")" 2>/dev/null || true
	done
	rm -rf "$TMP"
}
trap cleanup EXIT
# live_pid PID: the process runs and is not a zombie.
live_pid() {
	local state
	state="$(awk '{print $3}' "/proc/$1/stat" 2>/dev/null)" || return 1
	[ -n "$state" ] && [ "$state" != Z ]
}
# The harness wrapper fails host-control stop-owned when FAKE_STOP_OWNED_FAIL=1.
REAL_SOAK_HARNESS_BIN="${SOAK_HARNESS_BIN:-$ROOT/target/debug/casper-soak}"
mkdir -p "$TMP/harness"
cat >"$TMP/harness/casper-soak" <<SH
#!/usr/bin/env bash
if [ "\${FAKE_STOP_OWNED_FAIL:-0}" = 1 ] && [ "\${1:-} \${2:-}" = 'host-control stop-owned' ]; then
	echo 'simulated stop-owned failure' >&2
	exit 1
fi
exec "$REAL_SOAK_HARNESS_BIN" "\$@"
SH
chmod +x "$TMP/harness/casper-soak"
mkdir -p "$TMP/bin" "$TMP/system-integration"
cat >"$TMP/bin/poetry" <<'SH'
#!/usr/bin/env bash
trap 'exit 143' TERM INT
printf '%s\n' "$$" >"$FAKE_POETRY_PID_FILE"
# A subprocess node runs in its own session, so the timeout(1) signal to the
# pytest process group does not reach it. Only the owner marker that it
# inherits identifies it.
if [ -n "${FAKE_WRITER_PID_FILE:-}" ]; then
	setsid bash -c 'printf "%s\n" "$$" >"$1"; while :; do sleep 1; done' fake-node "$FAKE_WRITER_PID_FILE" \
		</dev/null >/dev/null 2>&1 &
fi
# Docker sessions write monitor artifacts under log-archive/. Subprocess
# sessions write them under .subprocess-data/. The first scenario splits
# telemetry across the archive and legacy data roots.
mkdir -p "$FAKE_ARCHIVE_DIR/session" "$FAKE_DATA_DIR/session"
# Every harness CSV below is written CRLF (sed 's/$/\r/'): the real harness
# writes them with Python csv.writer, whose default line terminator is \r\n,
# so a fixture with bare \n would pass extractors that break in production.
cat <<'CSV' | sed 's/$/\r/' >"$FAKE_ARCHIVE_DIR/session/resource-timeseries.csv"
elapsed_s,node,memory_mb,cpu_percent,memory_limit_mb
1.0,rnode.test.validator1,256.0,10.0,0
1.0,rnode.test.validator2,512.0,20.0,0
CSV
# Per-core telemetry for validator1 only: validator2 must keep its "all"
# fallback row in the summary grid (mixed real/fallback rendering). The
# __system__ row is host state and must not become a grid node, and the
# non-numeric core id is a malformed row that must be rejected — the grid
# assertion below proves both filters.
# validator1 appears under BOTH container namings — the legacy
# "rnode.<network>." prefix and the bare per-iteration shard hash
# ("f6f7eb46.") that once leaked 42 columns into the published grid. The
# node id is the name's final dot-segment, so both rows must fold into one
# "validator1" key with the per-cell max (7.5 beats 3.0).
# CRLF matters most here: cpu_percent is the LAST column, so without
# CR-stripping every row fails the numeric check (smoke run 31547587950
# published an all-fallback grid exactly this way).
cat <<'CSV' | sed 's/$/\r/' >"$FAKE_ARCHIVE_DIR/session/resource-percore-timeseries.csv"
elapsed_s,node,core,cpu_percent
1.0,f6f7eb46.validator1,0,3.0
2.0,rnode.test.validator1,0,7.5
1.0,rnode.test.validator1,1,42.0
1.0,__system__,0,93.0
1.0,rnode.test.validator1,not-a-core,88.0
CSV
cat <<'CSV' | sed 's/$/\r/' >"$FAKE_DATA_DIR/session/node-metrics-timeseries.csv"
elapsed_s,node,metric,value
1.0,rnode.test.validator1,replay_cache_entries,12
1.0,rnode.test.validator1,replay_cache_retained_bytes,1048576
CSV
# The identical node log in BOTH roots (teardown archives a copy of a log
# that also lives under data/): counting metrics must see it once.
cat >"$FAKE_DATA_DIR/session/validator1.log" <<'LOG'
proposal rejected: too far ahead of the last finalized block
Propose timing: total_ms=3525
LOG
cp "$FAKE_DATA_DIR/session/validator1.log" "$FAKE_ARCHIVE_DIR/session/validator1.log"
printf 'fake pytest started\n'
printf 'low      |      30 |    1.0 |    1.9    5.9    5.9 |   90.0  100.0  101.0 |   45.4\r\n'
printf 'sustained |    1200 |    4.0 |   18.3   87.6  106.6 |   85.1  120.6  133.3 |  -20.2\r\n'
printf 'All-node LFBs at drain: {validator1: 12, validator2: 9} (spread 3 blocks)\n'
printf '  Phase high: inclusion p50=6.5s p95=11.6s, finalization p50=27.7s p95=35.6s, LFB #19->#27 (7.9 blk/min), unfinalized=13\n'
printf '  Node internals (validator1):\n'
printf 'ISSUE24_METRICS {"counters": {"block_creator_empty_block_built": {"delta": 4.0, "unit": "events"}}, "histograms": {}}\n'
if [ "${FAKE_EMIT_SOAK_METRIC:-false}" = "true" ]; then
	printf 'SOAK_METRIC name=lfb_spread value=4 phase=drain\n'
fi
while :; do sleep 1; done
SH
cat >"$TMP/bin/docker" <<'SH'
#!/usr/bin/env bash
[ -z "${FAKE_DOCKER_CALLS:-}" ] || printf '%s\n' "$*" >>"$FAKE_DOCKER_CALLS"
# FAKE_OWNED_CONTAINER is a node container of the soak owner. It is listed only
# after pytest ends, so the log probe of the running session does not read it.
owned_listed() {
	[ -n "${FAKE_OWNED_CONTAINER:-}" ] && [ -s "$FAKE_POETRY_PID_FILE" ] &&
		! kill -0 "$(cat "$FAKE_POETRY_PID_FILE")" 2>/dev/null &&
		printf '%s\n' "$*" | grep -q 'label=io.f1r3fly.soak.owner='
}
if [ "$1" = ps ] && owned_listed "$@"; then
	printf '%s\n' "$FAKE_OWNED_CONTAINER"
	exit 0
fi
if [ "$1" = inspect ] && [ "${!#}" = "${FAKE_OWNED_CONTAINER:-}" ]; then
	printf '%s\n' "$SOAK_WRITER_OWNER"
	exit 0
fi
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

test ! -e "$TMP/output"
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
jq -e '
  .target_ref == "unknown"
  and .target_sha == "unknown"
  and (.started_at | type) == "number"
  and .requested_seconds == 120
  and .iterations == 1
  and .failures == 0
' "$TMP/output/.soak-checkpoint-state.json" >/dev/null
for _ in $(seq 1 20); do
	[ -s "$TMP/output/iteration-00001-docker/resource-timeseries.csv" ] &&
		[ -s "$TMP/output/iteration-00001-docker/resource-percore-timeseries.csv" ] &&
		[ -s "$TMP/output/iteration-00001-docker/node-metrics-timeseries.csv" ] &&
		grep -q 'test.validator1.*12.*1048576.*2.*2.*7' \
			"$TMP/output/iteration-00001-docker/node-memory-timeseries.tsv" 2>/dev/null && break
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
test -s "$TMP/system-integration/integration-tests/log-archive/session/validator1.log"

test "$(find "$TMP/output" -maxdepth 1 -type d -name 'iteration-*' | wc -l | tr -d ' ')" = 1
# Diagnostic for a hosted failure of the failure count below: print the run
# records and every process that still carries an owner marker.
if ! jq -e '.failures == 1' "$TMP/output/summary.json" >/dev/null; then
	{
		printf '== summary.json failures: %s\n' "$(jq -c .failures "$TMP/output/summary.json")"
		for record in summary.txt early-exit.txt protection-breach.txt writer-stop-failure.txt .soak-state; do
			[ -e "$TMP/output/$record" ] || continue
			printf '== %s\n' "$record"
			cat "$TMP/output/$record"
		done
		printf '== processes with an owner marker\n'
		for environ in /proc/[0-9]*/environ; do
			pid="${environ#/proc/}"
			pid="${pid%/environ}"
			marker="$(tr '\0' '\n' <"$environ" 2>/dev/null | grep '^SOAK_PROCESS_OWNER=' || true)"
			[ -n "$marker" ] || continue
			printf '%s %s state=%s ppid=%s cmd=%s\n' "$pid" "$marker" \
				"$(awk '{print $3}' "/proc/$pid/stat" 2>/dev/null)" \
				"$(awk '{print $4}' "/proc/$pid/stat" 2>/dev/null)" \
				"$(tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null)"
		done
		# A zombie has an empty environ, so list the zombies separately.
		printf '== zombie processes\n'
		for stat in /proc/[0-9]*/stat; do
			awk '$3 == "Z" {print "pid=" $1, "comm=" $2, "ppid=" $4}' "$stat" 2>/dev/null || true
		done
		printf '== driver.log\n'
		cat "$TMP/driver.log"
	} >&2
fi
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
  and .iteration_metrics[0].finalization_latency == {p50_ms: 85100, p95_ms: 120600, p99_ms: 133300, samples: 2}
  and .finalization_p50_ms == 85100
  and .finalization_p95_ms == 120600
  and .finalization_p99_ms == 133300
  and .iteration_metrics[0].cpu_peak_per_node_pct == {"validator1": 10, "validator2": 20}
  and .iteration_metrics[0].cpu_peak_per_node_core_pct == {"validator1": {"0": 7.5, "1": 42}}
  and .iteration_metrics[0].too_far_ahead_errors == 1
  and .iteration_metrics[0].metrics.lfb_spread == {p50: 3, p95: 3, max: 3, min: 3, samples: 1}
  and .tracked_metrics.lfb_spread == {p50: 3, p95: 3, max: 3, samples: 1}
' "$TMP/output/summary.json" >/dev/null
grep -q 'replay_cache_retained_bytes,1048576' \
	"$TMP/output/iteration-00001-docker/node-metrics-timeseries.csv"
jq -e -s 'length == 1 and .[0].session == 1 and .[0].provider == "docker" and .[0].phase == "high" and .[0].unfinalized == 13 and .[0].counters.block_creator_empty_block_built == 4' \
	"$TMP/output/iteration-00001-docker/issue24-phases.jsonl" >/dev/null
test ! -e "$TMP/output/iteration-00001-docker/.pytest-output.fifo"
test -s "$TMP/fake-poetry.pid"
! kill -0 "$(cat "$TMP/fake-poetry.pid")" 2>/dev/null

# Scenario 2: the soak window ends mid-iteration. timeout(1) kills pytest and
# reports 124; the driver must write the deadline marker, NOT count the
# iteration as a failure (exit 0, no exit-code.txt, no early-exit), and still
# publish telemetry from the subprocess monitor root before the kill.
# The fake session also starts a detached node with the owner marker. The test
# starts a process with another owner marker. The end of the segment must stop
# only the node of its own owner.
# shellcheck disable=SC2016 # The inner bash expands $$ and $1.
SOAK_PROCESS_OWNER=00000000-0000-4000-8000-000000000000 \
	setsid bash -c 'printf "%s\n" "$$" >"$1"; while :; do sleep 1; done' unrelated "$TMP/unrelated.pid" \
	</dev/null >/dev/null 2>&1 &
disown
for _ in $(seq 1 20); do
	[ -s "$TMP/unrelated.pid" ] && break
	sleep 0.25
done
live_pid "$(cat "$TMP/unrelated.pid")"
mkdir -p "$TMP/si2"
PATH="$TMP/bin:$PATH" \
	FAKE_POETRY_PID_FILE="$TMP/fake-poetry-2.pid" \
	FAKE_WRITER_PID_FILE="$TMP/fake-writer-2.pid" \
	FAKE_DOCKER_CALLS="$TMP/docker-calls-2.txt" \
	FAKE_OWNED_CONTAINER=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef \
	FAKE_DATA_DIR="$TMP/si2/integration-tests/data" \
	FAKE_ARCHIVE_DIR="$TMP/si2/integration-tests/.subprocess-data" \
	FAKE_EMIT_SOAK_METRIC=true \
	SOAK_DURATION_SECONDS=6 \
	SYSTEM_INTEGRATION_DIR="$TMP/si2" \
	SOAK_OUTPUT_DIR="$TMP/output2" \
	SOAK_RSS_CEILING_MB=0 \
	SOAK_HOST_FREE_FLOOR_MB=0 \
	SOAK_GUARDIAN_POLL_SECONDS=1 \
	SOAK_MONITOR_SNAPSHOT_SECONDS=0.1 \
	"$ROOT/scripts/run-merge-recovery-soak.sh" >"$TMP/driver2.log" 2>&1 &
DRIVER_PID=$!

writer_live_before_deadline=0
for _ in $(seq 1 60); do
	kill -0 "$DRIVER_PID" 2>/dev/null || break
	if [ "$writer_live_before_deadline" -eq 0 ] && [ -s "$TMP/fake-writer-2.pid" ] &&
		[ ! -e "$TMP/output2/iteration-00001-docker/deadline.txt" ] &&
		live_pid "$(cat "$TMP/fake-writer-2.pid")"; then
		writer_live_before_deadline=1
	fi
	sleep 0.5
done
if kill -0 "$DRIVER_PID" 2>/dev/null; then
	cat "$TMP/driver2.log" >&2
	echo 'soak driver did not stop at its deadline' >&2
	exit 1
fi
set +e
wait "$DRIVER_PID"
status=$?
set -e
DRIVER_PID=""
if [ "$status" -ne 0 ]; then
	cat "$TMP/driver2.log" >&2
	echo "deadline-killed iteration must not fail the soak (driver exited $status)" >&2
	exit 1
fi

test -e "$TMP/output2/iteration-00001-docker/deadline.txt"
test ! -e "$TMP/output2/iteration-00001-docker/exit-code.txt"
test ! -e "$TMP/output2/early-exit.txt"
test ! -e "$TMP/output2/writer-stop-failure.txt"
grep -q '^early_exit_reason=none$' "$TMP/output2/summary.txt"
# A deadline stop skips the failure-evidence copy. The evidence of the stopped
# session is its log, its monitor snapshots, and its metrics. That evidence must
# remain after the writer stop. The stop kills owned containers and does not
# remove them, so their logs also remain on the host.
grep -q '^fake pytest started' "$TMP/output2/iteration-00001-docker/pytest.log"
test -s "$TMP/output2/iteration-00001-docker/resource-timeseries.csv"
grep -q '^kill 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef$' "$TMP/docker-calls-2.txt" || {
	echo 'the writer stop did not kill the owned container' >&2
	exit 1
}
if grep -q '^rm ' "$TMP/docker-calls-2.txt"; then
	echo 'the writer stop removed a container' >&2
	exit 1
fi
# The node-metrics copy is gzipped once its iteration ends (scenario 1 keeps
# it plain because the guardian breach runs under the emergency deadline).
test ! -e "$TMP/output2/iteration-00001-docker/node-metrics-timeseries.csv"
gzip -dc "$TMP/output2/iteration-00001-docker/node-metrics-timeseries.csv.gz" |
	grep -q 'replay_cache_retained_bytes,1048576'
jq -e '
  .iterations == 1
  and .failures == 0
  and .providers.docker == {iterations: 1, failures: 0, avg_duration_s: .providers.docker.avg_duration_s}
  and .rss_peak_mb == 768
  and .cpu_peak_pct == 30
  and .cpu_peak_core_grid_pct == {"validator1": {"0": 7.5, "1": 42}, "validator2": {"all": 20}}
  and .iteration_metrics[0].exit_code == 124
  and .iteration_metrics[0].deadline_truncated == true
  and .iteration_metrics[0].rss_peak_mb == 768
  and .iteration_metrics[0].finalization_latency == {p50_ms: 85100, p95_ms: 120600, p99_ms: 133300, samples: 2}
  and .finalization_p50_ms == 85100
  and .finalization_p95_ms == 120600
  and .finalization_p99_ms == 133300
  and .iteration_metrics[0].cpu_peak_per_node_pct == {"validator1": 10, "validator2": 20}
  and .iteration_metrics[0].cpu_peak_per_node_core_pct == {"validator1": {"0": 7.5, "1": 42}}
  and .iteration_metrics[0].metrics.lfb_spread == {p50: 4, p95: 4, max: 4, min: 4, samples: 1}
  and .tracked_metrics.lfb_spread == {p50: 4, p95: 4, max: 4, samples: 1}
' "$TMP/output2/summary.json" >/dev/null
test -s "$TMP/fake-poetry-2.pid"
! kill -0 "$(cat "$TMP/fake-poetry-2.pid")" 2>/dev/null
# The segment owns the node, and the next segment gets a new owner marker,
# so no later segment stops a node that survives this one (most likely the
# disk growth of runs 39194089405 and 39369559337).
[ "$writer_live_before_deadline" -eq 1 ] || {
	echo 'the detached node did not run before the deadline' >&2
	exit 1
}
if live_pid "$(cat "$TMP/fake-writer-2.pid")"; then
	echo 'a node of the deadline-stopped session survived the segment' >&2
	exit 1
fi
live_pid "$(cat "$TMP/unrelated.pid")" || {
	echo 'the segment stopped a process of another owner' >&2
	exit 1
}

mkdir -p "$TMP/bin3" "$TMP/si3" "$TMP/tmp3/test-stale" "$TMP/tmp3/test-live" "$TMP/runner3/_diag"
perl -e 'utime $^T - 7200, $^T - 7200, @ARGV' "$TMP/tmp3/test-stale"
printf '7000\n' >"$TMP/fake-df-avail"
cat >"$TMP/bin3/df" <<'SH'
#!/usr/bin/env bash
printf 'Filesystem 1M-blocks Used Available Capacity Mounted on\n'
printf '/dev/fake 47000 40000 %s 85%% /\n' "$(cat "$FAKE_DF_AVAIL_FILE")"
SH
cat >"$TMP/bin3/docker" <<'SH'
#!/usr/bin/env bash
exit 0
SH
chmod +x "$TMP/bin3/df" "$TMP/bin3/docker"
PATH="$TMP/bin3:$TMP/bin:$PATH" \
	FAKE_DF_AVAIL_FILE="$TMP/fake-df-avail" \
	FAKE_POETRY_PID_FILE="$TMP/fake-poetry-3.pid" \
	FAKE_DATA_DIR="$TMP/si3/integration-tests/data" \
	FAKE_ARCHIVE_DIR="$TMP/si3/integration-tests/log-archive" \
	SOAK_DURATION_SECONDS=120 \
	SYSTEM_INTEGRATION_DIR="$TMP/si3" \
	SOAK_OUTPUT_DIR="$TMP/output3" \
	SOAK_RSS_CEILING_MB=0 \
	SOAK_HOST_FREE_FLOOR_MB=0 \
	SOAK_TMP_ROOT="$TMP/tmp3" \
	SOAK_RUNNER_ROOT="$TMP/runner3" \
	SOAK_GUARDIAN_POLL_SECONDS=1 \
	SOAK_MONITOR_SNAPSHOT_SECONDS=0.1 \
	"$ROOT/scripts/run-merge-recovery-soak.sh" >"$TMP/driver3.log" 2>&1 &
DRIVER_PID=$!

for _ in $(seq 1 60); do
	kill -0 "$DRIVER_PID" 2>/dev/null || break
	sleep 0.5
done
if kill -0 "$DRIVER_PID" 2>/dev/null; then
	cat "$TMP/driver3.log" >&2
	echo 'soak driver did not stop when hygiene left free space inside the band' >&2
	exit 1
fi
set +e
wait "$DRIVER_PID"
status=$?
set -e
DRIVER_PID=""
if [ "$status" -ne 1 ]; then
	cat "$TMP/driver3.log" >&2
	echo "a no-op hygiene pass inside the band must fail the soak closed (driver exited $status)" >&2
	exit 1
fi

test -d "$TMP/tmp3/test-stale"
test -d "$TMP/tmp3/test-live"
grep -q 'free disk 7000MB inside hygiene band (floor 4096MB + band 4096MB); reclaiming' "$TMP/driver3.log"
grep -q '^disk hygiene: 7000MB free -> 7000MB free$' "$TMP/driver3.log"
! grep -q '^disk usage: ' "$TMP/driver3.log"
grep -q '^host_protection_breach: disk floor: free 7000MB still inside hygiene band (floor 4096MB + band 4096MB) after hygiene$' \
	"$TMP/output3/early-exit.txt"
grep -q 'still inside hygiene band' "$TMP/output3/protection-breach.txt"
grep -q '^early_exit_reason=host_protection_breach$' "$TMP/output3/summary.txt"
grep -q "$TMP/output3" "$TMP/output3/disk-floor-breach.txt"
grep -q "$TMP/runner3/_diag" "$TMP/output3/disk-floor-breach.txt"
grep -q "$TMP/tmp3/test-live" "$TMP/output3/disk-floor-breach.txt"
test "$(find "$TMP/output3" -maxdepth 1 -type d -name 'iteration-*' | wc -l | tr -d ' ')" = 0
test ! -e "$TMP/fake-poetry-3.pid"

# A later segment of the ended soak does no work. Its summary must keep the
# reason of the earlier stop (run 39369559337 published early_exit_reason=none).
# The disk floor writes no guardian marker. A guardian marker restores the
# reason by another path, so this restart must not have one.
test ! -e "$TMP/output3/host-guardian-breach.txt"
PATH="$TMP/bin3:$TMP/bin:$PATH" \
	FAKE_DF_AVAIL_FILE="$TMP/fake-df-avail" \
	FAKE_POETRY_PID_FILE="$TMP/fake-poetry-3.pid" \
	FAKE_DATA_DIR="$TMP/si3/integration-tests/data" \
	FAKE_ARCHIVE_DIR="$TMP/si3/integration-tests/log-archive" \
	SOAK_DURATION_SECONDS=120 \
	SYSTEM_INTEGRATION_DIR="$TMP/si3" \
	SOAK_OUTPUT_DIR="$TMP/output3" \
	SOAK_RSS_CEILING_MB=0 \
	SOAK_HOST_FREE_FLOOR_MB=0 \
	SOAK_TMP_ROOT="$TMP/tmp3" \
	SOAK_RUNNER_ROOT="$TMP/runner3" \
	SOAK_GUARDIAN_POLL_SECONDS=1 \
	SOAK_MONITOR_SNAPSHOT_SECONDS=0.1 \
	timeout --signal=TERM --kill-after=2 60 "$ROOT/scripts/run-merge-recovery-soak.sh" >"$TMP/driver3b.log" 2>&1 || true
grep -q '^soak already ended (host_protection_breach: disk floor' "$TMP/driver3b.log"
grep -q '^segments=2$' "$TMP/output3/summary.txt"
grep -q '^early_exit_reason=host_protection_breach$' "$TMP/output3/summary.txt" || {
	cat "$TMP/output3/summary.txt" >&2
	echo 'a segment after the stop lost the early exit reason' >&2
	exit 1
}
test ! -e "$TMP/fake-poetry-3.pid"

# The writer stop at the end of a segment fails. The soak must fail closed: a
# failure marker, one more failure in the saved state, and a nonzero exit.
# writer_stop_failure_case NAME DURATION FLOOR_MB: runs one segment with a
# failing stop-owned and sets status.
writer_stop_failure_case() {
	local name="$1" duration="$2" floor="$3"
	mkdir -p "$TMP/si-$name"
	status=0
	timeout --signal=TERM --kill-after=2 60 env PATH="$TMP/bin3:$TMP/bin:$PATH" \
		SOAK_HARNESS_BIN="$TMP/harness/casper-soak" \
		FAKE_STOP_OWNED_FAIL=1 \
		FAKE_DF_AVAIL_FILE="$TMP/fake-df-avail" \
		FAKE_POETRY_PID_FILE="$TMP/fake-poetry-$name.pid" \
		FAKE_WRITER_PID_FILE="$TMP/fake-writer-$name.pid" \
		FAKE_DATA_DIR="$TMP/si-$name/integration-tests/data" \
		FAKE_ARCHIVE_DIR="$TMP/si-$name/integration-tests/.subprocess-data" \
		SOAK_DURATION_SECONDS="$duration" \
		SYSTEM_INTEGRATION_DIR="$TMP/si-$name" \
		SOAK_OUTPUT_DIR="$TMP/output-$name" \
		SOAK_RSS_CEILING_MB=0 \
		SOAK_HOST_FREE_FLOOR_MB=0 \
		SOAK_DISK_FREE_FLOOR_MB="$floor" \
		SOAK_TMP_ROOT="$TMP/tmp3" \
		SOAK_RUNNER_ROOT="$TMP/runner3" \
		SOAK_GUARDIAN_POLL_SECONDS=1 \
		SOAK_MONITOR_SNAPSHOT_SECONDS=0.1 \
		"$ROOT/scripts/run-merge-recovery-soak.sh" >"$TMP/driver-$name.log" 2>&1 || status=$?
	[ "$status" -ne 124 ] || { cat "$TMP/driver-$name.log" >&2; echo "the $name scenario did not finish" >&2; exit 1; }
}

# Without an earlier stop reason, the failed stop becomes the reason.
writer_stop_failure_case stop-fail 6 0
[ "$status" -ne 0 ] || { cat "$TMP/driver-stop-fail.log" >&2; echo 'a failed writer stop passed the soak' >&2; exit 1; }
test -s "$TMP/output-stop-fail/writer-stop-failure.txt"
grep -q '^writer_stop_failed:' "$TMP/output-stop-fail/early-exit.txt"
grep -q '^FAILURES=1$' "$TMP/output-stop-fail/.soak-state"
grep -q '^failures=1$' "$TMP/output-stop-fail/summary.txt"
grep -q '^early_exit_reason=writer_stop_failed$' "$TMP/output-stop-fail/summary.txt"

# After a disk floor stop, the failed stop adds a failure and keeps the reason.
writer_stop_failure_case breach-stop-fail 120 4096
[ "$status" -ne 0 ] || { cat "$TMP/driver-breach-stop-fail.log" >&2; echo 'a failed writer stop passed the soak' >&2; exit 1; }
test -s "$TMP/output-breach-stop-fail/writer-stop-failure.txt"
grep -q '^host_protection_breach: disk floor' "$TMP/output-breach-stop-fail/early-exit.txt"
grep -q '^FAILURES=2$' "$TMP/output-breach-stop-fail/.soak-state"
grep -q '^early_exit_reason=host_protection_breach$' "$TMP/output-breach-stop-fail/summary.txt"

# A later segment of that soak does no work, and its writer stop also fails.
# The failure counts, but the reason of the earlier stop stays.
rm -f "$TMP/output-breach-stop-fail/writer-stop-failure.txt"
writer_stop_failure_case breach-stop-fail 120 4096
[ "$status" -ne 0 ] || { cat "$TMP/driver-breach-stop-fail.log" >&2; echo 'a failed writer stop passed the soak' >&2; exit 1; }
grep -q '^soak already ended (host_protection_breach: disk floor' "$TMP/driver-breach-stop-fail.log"
test -s "$TMP/output-breach-stop-fail/writer-stop-failure.txt"
grep -q '^host_protection_breach: disk floor' "$TMP/output-breach-stop-fail/early-exit.txt"
grep -q '^FAILURES=3$' "$TMP/output-breach-stop-fail/.soak-state"
grep -q '^segments=2$' "$TMP/output-breach-stop-fail/summary.txt"
grep -q '^early_exit_reason=host_protection_breach$' "$TMP/output-breach-stop-fail/summary.txt"

# Scenario 4: two failed iterations. Each fake session writes its own archive
# directory and exits 1. The second iteration's failure evidence must hold only
# its own session, and the harness archive must not keep the first session:
# run 37153082817 copied every earlier session into each failed iteration and
# reached the disk floor after 15 iterations.
mkdir -p "$TMP/bin4" "$TMP/si4"
cat >"$TMP/bin4/poetry" <<'SH'
#!/usr/bin/env bash
count="$(($(cat "$FAKE_SESSION_COUNTER" 2>/dev/null || printf 0) + 1))"
printf '%s\n' "$count" >"$FAKE_SESSION_COUNTER"
mkdir -p "$FAKE_ARCHIVE_DIR/session-$count"
printf 'session %s log\n' "$count" >"$FAKE_ARCHIVE_DIR/session-$count/validator1.log"
printf 'fake pytest session %s failed\n' "$count"
exit 1
SH
chmod +x "$TMP/bin4/poetry"
status=0
timeout --signal=TERM --kill-after=5 150 env PATH="$TMP/bin4:$TMP/bin:$PATH" \
	FAKE_POETRY_PID_FILE="$TMP/fake-poetry-4.pid" \
	FAKE_SESSION_COUNTER="$TMP/fake-session-counter" \
	FAKE_ARCHIVE_DIR="$TMP/si4/integration-tests/log-archive" \
	SOAK_DURATION_SECONDS=40 \
	SYSTEM_INTEGRATION_DIR="$TMP/si4" \
	SOAK_OUTPUT_DIR="$TMP/output4" \
	SOAK_RSS_CEILING_MB=0 \
	SOAK_HOST_FREE_FLOOR_MB=0 \
	SOAK_DISK_FREE_FLOOR_MB=0 \
	SOAK_GUARDIAN_POLL_SECONDS=1 \
	SOAK_MONITOR_SNAPSHOT_SECONDS=0.1 \
	"$ROOT/scripts/run-merge-recovery-soak.sh" >"$TMP/driver4.log" 2>&1 || status=$?
[ "$status" -ne 124 ] || { cat "$TMP/driver4.log" >&2; echo 'soak driver did not finish the two-failure scenario' >&2; exit 1; }
second="$(find "$TMP/output4" -maxdepth 1 -type d -name 'iteration-00002-*' | head -1)"
[ -n "$second" ] || { cat "$TMP/driver4.log" >&2; echo 'the two-failure scenario ran fewer than two iterations' >&2; exit 1; }
test -s "$second/log-archive/session-2/validator1.log"
test ! -e "$second/log-archive/session-1"
test ! -e "$TMP/si4/integration-tests/log-archive/session-1"
test -z "$(ls -A "$TMP/si4/integration-tests/log-archive")"
jq -e '.failures >= 2' "$TMP/output4/summary.json" >/dev/null

# Scenario 5: the archive reset cannot delete. The driver must log the failed
# reset with its root and continue, and the session stays in place.
mkdir -p "$TMP/bin5" "$TMP/si5"
cat >"$TMP/bin5/find" <<SH
#!/usr/bin/env bash
for argument in "\$@"; do
	[ "\$argument" != -delete ] || { echo 'find: cannot delete: Permission denied' >&2; exit 1; }
done
exec $(command -v find) "\$@"
SH
chmod +x "$TMP/bin5/find"
status=0
timeout --signal=TERM --kill-after=5 120 env PATH="$TMP/bin5:$TMP/bin4:$TMP/bin:$PATH" \
	FAKE_POETRY_PID_FILE="$TMP/fake-poetry-5.pid" \
	FAKE_SESSION_COUNTER="$TMP/fake-session-counter-5" \
	FAKE_ARCHIVE_DIR="$TMP/si5/integration-tests/log-archive" \
	SOAK_DURATION_SECONDS=5 \
	SYSTEM_INTEGRATION_DIR="$TMP/si5" \
	SOAK_OUTPUT_DIR="$TMP/output5" \
	SOAK_RSS_CEILING_MB=0 \
	SOAK_HOST_FREE_FLOOR_MB=0 \
	SOAK_DISK_FREE_FLOOR_MB=0 \
	SOAK_GUARDIAN_POLL_SECONDS=1 \
	SOAK_MONITOR_SNAPSHOT_SECONDS=0.1 \
	"$ROOT/scripts/run-merge-recovery-soak.sh" >"$TMP/driver5.log" 2>&1 || status=$?
[ "$status" -ne 124 ] || { cat "$TMP/driver5.log" >&2; echo 'soak driver did not finish the failed-reset scenario' >&2; exit 1; }
grep -q "harness archive reset incomplete for $TMP/si5/integration-tests/log-archive .*Permission denied" "$TMP/driver5.log" ||
	{ cat "$TMP/driver5.log" >&2; echo 'the failed archive reset was not logged' >&2; exit 1; }
test -s "$TMP/si5/integration-tests/log-archive/session-1/validator1.log"

for state_case in valid executable duplicate unknown missing overflow; do
	state_output="$TMP/state-$state_case"
	mkdir -p "$state_output"
	printf 'STARTED_AT=1\nITERATIONS=0\nFAILURES=0\nSEGMENT=1\n' >"$state_output/.soak-state"
	case "$state_case" in
		executable) printf 'printf executed >"$OUTPUT_DIR/executed"\nexit 2\n' >>"$state_output/.soak-state" ;;
		duplicate) printf 'ITERATIONS=0\n' >>"$state_output/.soak-state" ;;
		unknown) printf 'UNRECOGNIZED=0\n' >>"$state_output/.soak-state" ;;
		missing) printf 'STARTED_AT=1\nITERATIONS=0\nFAILURES=0\n' >"$state_output/.soak-state" ;;
		overflow) printf 'STARTED_AT=1\nITERATIONS=0\nFAILURES=0\nSEGMENT=99999999999999999999\n' >"$state_output/.soak-state" ;;
	esac
	cp "$state_output/.soak-state" "$state_output/before.txt"
	status=0
	timeout --signal=TERM --kill-after=2 15 env PATH="$TMP/bin:$PATH" \
		FAKE_POETRY_PID_FILE="$TMP/state-child.pid" \
		SOAK_DURATION_SECONDS=1 SYSTEM_INTEGRATION_DIR="$TMP/system-integration" \
		SOAK_OUTPUT_DIR="$state_output" SOAK_RSS_CEILING_MB=0 \
		SOAK_HOST_FREE_FLOOR_MB=0 SOAK_DISK_FREE_FLOOR_MB=0 \
		"$ROOT/scripts/run-merge-recovery-soak.sh" >"$TMP/state-$state_case.log" 2>&1 || status=$?
	if [ "$state_case" = valid ]; then
		[ "$status" -eq 0 ] || { cat "$TMP/state-$state_case.log" >&2; exit 1; }
		grep -q '^SEGMENT=2$' "$state_output/.soak-state"
	else
		[ "$status" -eq 2 ] || { printf 'Invalid state accepted: %s (exit %s)\n' "$state_case" "$status" >&2; exit 1; }
		[ ! -e "$state_output/executed" ] || { printf 'Saved state executed shell input.\n' >&2; exit 1; }
		cmp "$state_output/before.txt" "$state_output/.soak-state"
	fi
	[ ! -e "$TMP/state-child.pid" ]
done

printf 'legacy soak driver tests passed (fail-closed + deadline + segment writer stop + disk-band + restart reason + writer stop failure + evidence scope + failed reset + restart state)\n'
