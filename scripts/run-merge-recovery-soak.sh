#!/usr/bin/env bash
set -uo pipefail

DURATION_SECONDS="${SOAK_DURATION_SECONDS:?SOAK_DURATION_SECONDS is required}"
SYSTEM_INTEGRATION_DIR="${SYSTEM_INTEGRATION_DIR:?SYSTEM_INTEGRATION_DIR is required}"
OUTPUT_DIR="${SOAK_OUTPUT_DIR:-/tmp/merge-recovery-soak}"
TARGET_REF="${SOAK_TARGET_REF:-unknown}"
TARGET_SHA="${SOAK_TARGET_SHA:-unknown}"
TRIGGER_SOURCE="${SOAK_TRIGGER_SOURCE:-manual}"
SLOT_DELAY_SECONDS="${SOAK_SLOT_DELAY_SECONDS:-0}"
if ! [[ "$SLOT_DELAY_SECONDS" =~ ^[0-9]+$ ]]; then
	SLOT_DELAY_SECONDS=0
fi
if ! [[ "$DURATION_SECONDS" =~ ^[1-9][0-9]*$ ]]; then
	printf 'SOAK_DURATION_SECONDS must be a positive integer\n' >&2
	exit 2
fi
PROVIDERS=(docker subprocess)

# A soak is run as one or more segments so results can be published part-way
# through: a single 22h invocation cannot be interrupted to publish, but three
# consecutive invocations sharing one output directory can be. Every segment
# after the first resumes from this state file, so counters, the original start
# time and iteration numbering continue rather than restarting — restarting
# them would make each segment overwrite the previous one's iteration
# directories and silently discard its metrics.
mkdir -p "$OUTPUT_DIR"
STATE_FILE="$OUTPUT_DIR/.soak-state"
if [ -f "$STATE_FILE" ]; then
	# shellcheck source=/dev/null
	. "$STATE_FILE"
	SEGMENT="$((SEGMENT + 1))"
	printf 'resuming soak: segment %s, %s iterations so far, started %s\n' \
		"$SEGMENT" "$ITERATIONS" "$(date -d "@$STARTED_AT" '+%F %T %Z' 2>/dev/null || date -r "$STARTED_AT")"
else
	STARTED_AT="$(date +%s)"
	ITERATIONS=0
	FAILURES=0
	SEGMENT=1
fi

# An absolute deadline lets the caller end a segment on a wall-clock boundary
# (a Pacific checkpoint) rather than after a fixed span. Without one, the
# deadline is the full requested duration measured from the original start, so
# a final segment needs no deadline of its own.
# Whether this segment ends on a checkpoint boundary. The caller publishes a
# checkpoint only when it passed a deadline, so this also decides whether an
# on-demand checkpoint is possible here at all — see the signal handling below.
HAS_CHECKPOINT_BOUNDARY=0
if [ -n "${SOAK_DEADLINE_EPOCH:-}" ]; then
	if ! [[ "$SOAK_DEADLINE_EPOCH" =~ ^[1-9][0-9]*$ ]]; then
		printf 'SOAK_DEADLINE_EPOCH must be a positive integer\n' >&2
		exit 2
	fi
	HAS_CHECKPOINT_BOUNDARY=1
	DEADLINE="$SOAK_DEADLINE_EPOCH"
	# Never run past the overall budget, whatever the caller asked for.
	if [ "$DEADLINE" -gt "$((STARTED_AT + DURATION_SECONDS))" ]; then
		DEADLINE="$((STARTED_AT + DURATION_SECONDS))"
	fi
else
	DEADLINE="$((STARTED_AT + DURATION_SECONDS))"
fi

# An earlier segment may have ended the soak deliberately — the branch under
# test advanced, so the pinned image is now testing history. Later segments
# must not restart it: without this each remaining segment would soak on to its
# own deadline, undoing the early exit and burning the runner time it was meant
# to save. The rollup below still runs, so the final segment produces a summary
# to publish.
if [ -f "$OUTPUT_DIR/early-exit.txt" ]; then
	printf 'soak already ended (%s); this segment does no work\n' \
		"$(head -1 "$OUTPUT_DIR/early-exit.txt" 2>/dev/null || echo 'reason unrecorded')"
	DEADLINE=0
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Harness telemetry roots. Subprocess sessions write monitor artifacts and
# node logs under data/; docker sessions write them under log-archive/ (the
# provider's host-visible per-session dir — DockerProvider.monitor_output_dir
# in system-integration). Every search for monitor output, node logs, breach
# markers or SOAK_METRIC lines must cover both, or docker iterations lose
# their telemetry: rss_peak_mb and finalization_latency were null on every
# docker iteration (runs 30880995655, 30906818259) because only data/ was
# searched. Created up front so multi-root find never ENOENTs.
HARNESS_TELEMETRY_DIRS=(
	"$SYSTEM_INTEGRATION_DIR/integration-tests/data"
	"$SYSTEM_INTEGRATION_DIR/integration-tests/log-archive"
)
mkdir -p "${HARNESS_TELEMETRY_DIRS[@]}"
RUN_BENCHMARKS="${SOAK_RUN_BENCHMARKS:-false}"
BENCH_EVERY="${SOAK_BENCH_EVERY:-4}"
BENCH_DURATION="${SOAK_BENCH_DURATION:-300}"
BENCH_RATE="${SOAK_BENCH_RATE:-2}"
NODE_REPO_DIR="${SOAK_NODE_REPO_DIR:-}"
# Carried over by the state file when resuming a later segment.
BENCH_SEGMENTS="${BENCH_SEGMENTS:-0}"
BENCH_FAILURES="${BENCH_FAILURES:-0}"
# Minimum seconds before a move of the branch under test may end the soak.
# 0 disables merge-triggered exit entirely (the weekend soak sets this).
MERGE_EXIT_MIN_SECONDS="${SOAK_MERGE_EXIT_MIN_SECONDS:-0}"
EARLY_EXIT_REASON=""

# Total-node-RSS watchdog ceiling passed to the harness (--rss-ceiling-mb).
# The harness default is 5000MB — sized for laptops, not the 32GB soak VM:
# the 6-node shard legitimately peaks ~10GB under test_load, and the default
# killed every iteration of run 30516534214 ~130s in. Size to the host
# instead, so the watchdog stays on to catch a real leak before swap-thrash
# freezes the VM without ever firing on normal load.
#
# The reserve is 12GB, not the 8GB first tried. 8GB permits a 24GB node
# working set on the 32GB VM, which puts the *kernel* OOM killer ahead of this
# watchdog: at that point the harness has not breached, so the kernel picks a
# victim by oom_score and Runner.Worker is a plausible one — a runner that
# vanishes mid-step with no log and no failed step, which is exactly what
# happened to run 30590630059. Reserving 12GB caps nodes at 20GB, still ~2x
# the observed 10.8GB peak, and keeps the attributable failure ahead of the
# unattributable one. SOAK_RSS_CEILING_MB overrides; 0 disables the watchdog.
HOST_RESERVE_MB="${SOAK_HOST_RESERVE_MB:-12288}"
if ! [[ "$HOST_RESERVE_MB" =~ ^[0-9]+$ ]]; then
	printf 'SOAK_HOST_RESERVE_MB must be a non-negative integer\n' >&2
	exit 2
fi
MEM_TOTAL_MB="$(awk '/^MemTotal:/ {print int($2 / 1024)}' /proc/meminfo 2>/dev/null || echo 0)"
RSS_CEILING_MB="${SOAK_RSS_CEILING_MB:-}"
if [ -n "$RSS_CEILING_MB" ]; then
	if ! [[ "$RSS_CEILING_MB" =~ ^[0-9]+$ ]]; then
		printf 'SOAK_RSS_CEILING_MB must be a non-negative integer\n' >&2
		exit 2
	fi
else
	RSS_CEILING_MB="$((MEM_TOTAL_MB - HOST_RESERVE_MB))"
	if [ "$RSS_CEILING_MB" -lt 5000 ]; then
		RSS_CEILING_MB=5000
	fi
fi

# Host-available-RAM floor passed to the harness (--host-free-floor-mb). The
# harness default of 2000MB only fires once the machine is already at the
# edge, by which point the kernel may have picked a victim first; 6000MB on the
# 32GB soak VM makes the harness kill the nodes and report an attributable
# breach while there is still room. This is a backstop to the RSS ceiling
# above, catching pressure the ceiling is blind to (docker overhead, the runner
# agent itself). Enforced on subprocess iterations only — docker iterations
# rely on the ceiling.
#
# Scaled to MemTotal/4 and capped at 6000, because a flat 6000 is incoherent on
# a small host: on an 8GB laptop it demands 6GB free while the ceiling still
# permits a 5GB shard, so the floor would breach on contact. Quartering keeps
# the same intent (fire well before the kernel does) at every size, and lands
# on 2048 at 8GB — effectively the harness's own 2000 default.
# SOAK_HOST_FREE_FLOOR_MB overrides; 0 disables the floor.
HOST_FREE_FLOOR_MB="${SOAK_HOST_FREE_FLOOR_MB:-}"
if [ -n "$HOST_FREE_FLOOR_MB" ]; then
	if ! [[ "$HOST_FREE_FLOOR_MB" =~ ^[0-9]+$ ]]; then
		printf 'SOAK_HOST_FREE_FLOOR_MB must be a non-negative integer\n' >&2
		exit 2
	fi
else
	HOST_FREE_FLOOR_MB="$((MEM_TOTAL_MB / 4))"
	if [ "$HOST_FREE_FLOOR_MB" -gt 6000 ]; then
		HOST_FREE_FLOOR_MB=6000
	fi
fi

printf 'Soak host protection: node RSS ceiling=%sMB; host free floor=%sMB\n' \
	"$RSS_CEILING_MB" "$HOST_FREE_FLOOR_MB"

if [ "$RUN_BENCHMARKS" = "true" ] && [ -z "$NODE_REPO_DIR" ]; then
	printf 'SOAK_NODE_REPO_DIR is required when SOAK_RUN_BENCHMARKS=true\n' >&2
	exit 2
fi

# Declared version of the code under test. The workflow passes the ref and the
# sha but not the version, and "which version was soaked" is what a reader of
# the dashboard actually wants — a sha answers "which commit", not "which
# release". This is the same value the Docker LABEL and the published image tag
# carry, so a dashboard row can be matched against a pulled image.
#
# Takes the first `version = "..."` line, which is the same line release.yml
# bumps (`0,/^version = ".*"/`), so the two cannot disagree about which one is
# the package version. Degrades to "unknown" and never fails: a missing version
# string must not abort a soak, and every consumer downstream treats it as
# optional.
#
# Deliberately NOT scripts/version.sh, which resolves the newest `v*` tag in the
# current repo. That answers "what is the latest release", not "what is this
# commit" — a dev soak would report a tag that postdates or has nothing to do
# with the code under test — and it needs tags present, which a shallow
# node-under-test checkout does not guarantee. Do not consolidate the two.
VERSION="unknown"
if [ -n "$NODE_REPO_DIR" ] && [ -f "$NODE_REPO_DIR/node/Cargo.toml" ]; then
	parsed_version="$(awk -F'"' '/^version = "/ { print $2; exit }' \
		"$NODE_REPO_DIR/node/Cargo.toml" 2>/dev/null || true)"
	[ -n "$parsed_version" ] && VERSION="$parsed_version"
fi

persist_soak_state() {
	local state_tmp="${STATE_FILE}.tmp"
	local checkpoint_state="$OUTPUT_DIR/.soak-checkpoint-state.json"
	local checkpoint_tmp="${checkpoint_state}.tmp"
	mkdir -p "$OUTPUT_DIR"
	{
		printf 'STARTED_AT=%s\n' "$STARTED_AT"
		printf 'ITERATIONS=%s\n' "$ITERATIONS"
		printf 'FAILURES=%s\n' "$FAILURES"
		printf 'BENCH_SEGMENTS=%s\n' "$BENCH_SEGMENTS"
		printf 'BENCH_FAILURES=%s\n' "$BENCH_FAILURES"
		printf 'SEGMENT=%s\n' "$SEGMENT"
	} >"$state_tmp" && mv "$state_tmp" "$STATE_FILE"
	jq -n \
		--arg target_ref "$TARGET_REF" \
		--arg target_sha "$TARGET_SHA" \
		--arg trigger_source "$TRIGGER_SOURCE" \
		--argjson slot_delay "$SLOT_DELAY_SECONDS" \
		--arg version "$VERSION" \
		--argjson started_at "$STARTED_AT" \
		--argjson requested_seconds "$DURATION_SECONDS" \
		--argjson iterations "$ITERATIONS" \
		--argjson failures "$FAILURES" \
		--argjson bench_segments "$BENCH_SEGMENTS" \
		--argjson bench_failures "$BENCH_FAILURES" \
		'{target_ref: $target_ref, target_sha: $target_sha,
      trigger_source: $trigger_source, slot_delay_seconds: $slot_delay,
      version: $version, started_at: $started_at,
      requested_seconds: $requested_seconds, iterations: $iterations,
      failures: $failures, bench_segments: $bench_segments,
      bench_failures: $bench_failures}' >"$checkpoint_tmp" &&
		mv "$checkpoint_tmp" "$checkpoint_state"
}

persist_soak_state

# Peak total node RSS for this iteration, from the newest harness
# resource-timeseries.csv written after the iteration's start marker
# (columns: elapsed_s,node,memory_mb,cpu_percent,memory_limit_mb; the
# __system__ row is host state, not node RSS). Empty output when absent.
iteration_rss_peak_mb() {
	local iteration_dir="$1" ts_csv="$1/resource-timeseries.csv"
	if [ ! -s "$ts_csv" ]; then
		ts_csv="$(find "${HARNESS_TELEMETRY_DIRS[@]}" \
			-name resource-timeseries.csv -newer "$iteration_dir/.started" -print0 2>/dev/null |
			xargs -0 -r ls -t 2>/dev/null | head -1 || true)"
		[ -n "$ts_csv" ] || return 0
		cp "$ts_csv" "$iteration_dir/resource-timeseries.csv" 2>/dev/null || true
	fi
	awk -F, 'NR > 1 && $2 != "__system__" { sum[$1] += $3 }
           END { max = 0; for (t in sum) if (sum[t] > max) max = sum[t]
                 if (max > 0) printf "%.0f\n", max }' "$ts_csv" 2>/dev/null || true
}

# Peak total node CPU (%) for this iteration, same resource-timeseries.csv as
# iteration_rss_peak_mb — reuses the copy that function already left in
# iteration_dir rather than re-finding and re-copying it. Must run after
# iteration_rss_peak_mb. Empty output when absent.
#
# Deliberately NOT the max of the per-node peaks below: this sums cpu_percent
# ACROSS nodes at each timestamp and peaks that total — "how hot did the whole
# shard run at once" — so its value can exceed every individual node's peak
# (two nodes at 10% and 20% in the same sample yield 30). The per-node
# extractor answers the different question "how hot did each node get".
#
# LC_ALL=C on every awk that prints "%.1f": a comma-decimal LC_NUMERIC locale
# would emit "30,0", which downstream jq rejects — and because these values
# ride --argjson into metrics.json, that would silently cost the iteration its
# entire metrics file, not just this field.
iteration_cpu_peak_percent() {
	local iteration_dir="$1" ts_csv="$iteration_dir/resource-timeseries.csv"
	[ -s "$ts_csv" ] || return 0
	LC_ALL=C awk -F, 'NR > 1 && $2 != "__system__" { sum[$1] += $4 }
           END { max = 0; for (t in sum) if (sum[t] > max) max = sum[t]
                 if (max > 0) printf "%.1f\n", max }' "$ts_csv" 2>/dev/null
}

# The same CSV split back out per node: each node's own peak cpu_percent over
# the iteration, as a JSON object {node: pct}. This is the dashboard CPU
# grid's AGGREGATE fallback — one "all cores combined" value per node — used
# by the summary rollup for nodes the per-core extractor below has no data
# for (pre-emission harness, provider without the per-core hook). The harness
# prefixes container names with "rnode.<network>." — stripped here so grid
# columns read "validator1", not a Docker network id. Node names are then
# sanitized to a safe character class ([-A-Za-z0-9._], anything else becomes
# "_") so a hostile or malformed name can neither break the hand-built JSON
# nor silently collide with another name the way character DELETION could.
# '{}' when absent, and the caller validates the output is JSON before
# trusting it (fail-soft like every metric here).
iteration_cpu_peak_per_node_percent() {
	local iteration_dir="$1" ts_csv="$iteration_dir/resource-timeseries.csv"
	[ -s "$ts_csv" ] || {
		printf '{}'
		return 0
	}
	LC_ALL=C awk -F, 'NR > 1 { sub(/\r$/, "") }
	         NR > 1 && $2 != "__system__" && $4 ~ /^[0-9]+([.][0-9]+)?$/ {
	           n = $2; sub(/^rnode\.[^.]*\./, "", n)
	           if (!(n in peak) || $4 + 0 > peak[n]) peak[n] = $4 + 0 }
	         END { printf "{"; sep = ""
	               for (n in peak) {
	                 k = n; gsub(/[^A-Za-z0-9._-]/, "_", k)
	                 printf "%s\"%s\":%.1f", sep, k, peak[n]; sep = "," }
	               printf "}" }' "$ts_csv" 2>/dev/null || printf '{}'
}

# Real core rows for the same grid: the harness monitor's per-core telemetry
# (resource-percore-timeseries.csv, columns elapsed_s,node,core,cpu_percent —
# a separate file from resource-timeseries.csv precisely so the aggregate
# extractors above cannot double-count), reduced to each (node, core) cell's
# peak over the iteration as nested JSON {node: {core: pct}}. '{}' when the
# harness predates per-core emission or the provider has no per-core hook;
# the summary rollup then keeps that node's "all" fallback row. Same name
# prefix stripping and JSON validation contract as the per-node extractor.
iteration_cpu_peak_per_node_core_percent() {
	local iteration_dir="$1" pc_csv="$iteration_dir/resource-percore-timeseries.csv"
	[ -s "$pc_csv" ] || {
		printf '{}'
		return 0
	}
	# Same row discipline as the per-node extractor: __system__ is host
	# state, not a node, and must never become a grid column (a node with
	# per-core rows drops its "all" fallback, so a phantom node here would
	# distort the grid, not just add noise). Core ids are bare CPU indices
	# per the telemetry contract — anything non-numeric is a malformed row,
	# rejected rather than sanitized into a phantom core.
	#
	# The leading sub() strips a CR before the fields are tested: the harness
	# writes this CSV with Python csv.writer, whose default line terminator
	# is \r\n, so cpu_percent — the LAST column — arrives as "1.5\r" and
	# would fail the numeric check on every row (smoke run 31547587950
	# published an all-fallback grid exactly this way). The per-node
	# extractor above gets the same guard for symmetry, though its CSV
	# carries a 5th column that happened to absorb the CR.
	LC_ALL=C awk -F, 'NR > 1 { sub(/\r$/, "") }
	         NR > 1 && $2 != "__system__" && $3 ~ /^[0-9]+$/ &&
	           $4 ~ /^[0-9]+([.][0-9]+)?$/ {
	           n = $2; sub(/^rnode\.[^.]*\./, "", n)
	           cell = n SUBSEP $3
	           if (!(cell in peak) || $4 + 0 > peak[cell]) peak[cell] = $4 + 0 }
	         END { printf "{"; nsep = ""
	               for (cell in peak) { split(cell, parts, SUBSEP); nodes[parts[1]] = 1 }
	               for (n in nodes) {
	                 k = n; gsub(/[^A-Za-z0-9._-]/, "_", k)
	                 printf "%s\"%s\":{", nsep, k; nsep = ","
	                 csep = ""
	                 for (cell in peak) {
	                   split(cell, parts, SUBSEP)
	                   if (parts[1] != n) continue
	                   printf "%s\"%s\":%.1f", csep, parts[2], peak[cell]; csep = ","
	                 }
	                 printf "}"
	               }
	               printf "}" }' "$pc_csv" 2>/dev/null || printf '{}'
}

snapshot_iteration_monitor_outputs() {
	local iteration_dir="$1" started_epoch filename source tmp
	started_epoch="$(date +%s)"
	while :; do
		for filename in resource-timeseries.csv resource-percore-timeseries.csv node-metrics-timeseries.csv resource-summary.txt host-protection-breach.txt; do
			source="$(find "${HARNESS_TELEMETRY_DIRS[@]}" \
				-name "$filename" -newer "$iteration_dir/.started" -print0 2>/dev/null |
				xargs -0 -r ls -t 2>/dev/null | head -1 || true)"
			[ -n "$source" ] || continue
			tmp="$iteration_dir/.$filename.tmp"
			if cp "$source" "$tmp" 2>/dev/null; then
				mv "$tmp" "$iteration_dir/$filename"
			else
				rm -f "$tmp"
			fi
		done
		scrape_node_memory_timeseries "$iteration_dir" "$started_epoch"
		sleep "${SOAK_MONITOR_SNAPSHOT_SECONDS:-2}"
	done
}

# Per-node memory attribution for the RSS-runaway defect: joins each running
# rnode container's RSS (from the harness resource-timeseries.csv snapshot)
# with the replay-cache and block-processing gauges scraped from the node's
# own /metrics endpoint. This is what turns "total RSS breached the ceiling"
# into "validator3's replay cache retained N bytes at breach time". Appends to
# node-memory-timeseries.tsv in the iteration dir; every lookup is fail-soft
# so a node mid-restart or a missing gauge yields '-' columns, never a dead
# snapshot loop.
NODE_METRICS_HTTP_PORT="${SOAK_NODE_METRICS_HTTP_PORT:-40403}"
scrape_node_memory_timeseries() {
	local iteration_dir="$1" started_epoch="$2" tsv="$1/node-memory-timeseries.tsv" \
		node port_line host_port metrics_body elapsed rss
	command -v docker >/dev/null 2>&1 || return 0
	command -v curl >/dev/null 2>&1 || return 0
	while IFS= read -r node; do
		[ -n "$node" ] || continue
		port_line="$(docker port "$node" "$NODE_METRICS_HTTP_PORT" 2>/dev/null | head -1 || true)"
		host_port="${port_line##*:}"
		[[ "$host_port" =~ ^[0-9]+$ ]] || continue
		metrics_body="$(curl -fsS --max-time 5 "http://127.0.0.1:${host_port}/metrics" 2>/dev/null || true)"
		[ -n "$metrics_body" ] || continue
		elapsed="$(($(date +%s) - started_epoch))"
		rss="$(awk -F, -v node="$node" \
			'NR > 1 && $2 == node { v = $3 } END { if (v == "") v = "-"; print v }' \
			"$iteration_dir/resource-timeseries.csv" 2>/dev/null || printf '%s\n' '-')"
		if [ ! -s "$tsv" ]; then
			printf 'elapsed_s\tnode\tmemory_mb\treplay_cache_entries\treplay_cache_retained_bytes\tblock_processing_active\tblock_processing_parallel_limit\tblock_processing_queue_pending\n' >"$tsv"
		fi
		printf '%s\n' "$metrics_body" | awk -v elapsed="$elapsed" -v node="$node" -v rss="$rss" '
			/^replay_cache_entries/ { entries = $NF }
			/^replay_cache_retained_bytes/ { retained = $NF }
			/^block_processing_active/ { active = $NF }
			/^block_processing_parallel_limit/ { limit = $NF }
			/^block_processing_queue_pending/ { queued = $NF }
			END {
				if (entries == "") entries = "-"
				if (retained == "") retained = "-"
				if (active == "") active = "-"
				if (limit == "") limit = "-"
				if (queued == "") queued = "-"
				printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n", elapsed, node, rss, entries, retained, active, limit, queued
			}' >>"$tsv"
	done < <(docker ps --filter 'name=rnode.' --format '{{.Names}}' 2>/dev/null || true)
}

# Drop content-duplicate files from a newline-separated list on stdin (first
# occurrence wins). The same log can appear under both telemetry roots when
# teardown archives a copy of a file that also lives under data/, and the
# counting metrics below (too-far-ahead, sample counts) would double without
# this. Hash content rather than compare paths: the roots' layouts differ,
# so the same file carries unrelated relative paths. Fail-soft — an
# unhashable file passes through rather than being dropped.
dedup_files_by_content() {
	local f digest
	declare -A seen_digests=()
	while IFS= read -r f; do
		[ -f "$f" ] || continue
		if command -v md5sum >/dev/null 2>&1; then
			digest="$(md5sum "$f" 2>/dev/null | awk '{print $1}')"
		else
			digest="$(md5 -q "$f" 2>/dev/null)"
		fi
		if [ -n "$digest" ]; then
			[ -n "${seen_digests[$digest]:-}" ] && continue
			seen_digests[$digest]=1
		fi
		printf '%s\n' "$f"
	done
}

# Propose-timing latency samples (total_ms) from node JSON logs written after
# the iteration's start marker — the f1r3fly.propose.timing parse target from
# profile-casper-latency.sh. Emits "p50 p95 p99 count" or nothing.
iteration_finalization_latency() {
	local iteration_dir="$1"
	# `|| true` is load-bearing under pipefail: a failed iteration can leave
	# zero matching samples, making grep exit 1 and the pipeline nonzero.
	find "${HARNESS_TELEMETRY_DIRS[@]}" \
		-name '*.log' -newer "$iteration_dir/.started" 2>/dev/null |
		dedup_files_by_content |
		xargs -r grep -h -o 'Propose timing:[^"]*' 2>/dev/null |
		grep -oE 'total_ms=[0-9]+' | grep -oE '[0-9]+' |
		sort -n |
		awk '{ a[NR] = $1 }
           END { if (NR == 0) exit
                 p50 = a[int((NR + 1) * 0.5)]; p95 = a[int((NR + 1) * 0.95)]; p99 = a[int((NR + 1) * 0.99)]
                 if (p50 == "") p50 = a[NR]
                 if (p95 == "") p95 = a[NR]
                 if (p99 == "") p99 = a[NR]
                 print p50, p95, p99, NR }' || true
}

# Count of proposal rejections logged as too far ahead of the last finalized
# block since the iteration started — the exact string casper emits at
# casper/src/rust/blocks/proposer/propose_result.rs:185. A rising count means
# the proposer is outrunning finalization badly enough to be rejected outright,
# distinct from the passive finalization_p95_ms latency this soak already
# tracks (that measures how slow finalization is, not how often it is refused).
iteration_too_far_ahead_errors() {
	local iteration_dir="$1"
	find "${HARNESS_TELEMETRY_DIRS[@]}" \
		-type f \( -name '*.log' -o -name '*.txt' \) -newer "$iteration_dir/.started" 2>/dev/null |
		dedup_files_by_content |
		xargs -r grep -h -o 'too far ahead of the last finalized block' 2>/dev/null |
		wc -l | tr -d ' '
}

# Parse the pytest terminal summary line ("== 1 failed, 64 passed, ... ==")
# and emit a per-iteration metrics.json with resource + latency samples.
# Metrics are additive: missing jq or an unparseable log must never fail
# the soak.
emit_iteration_metrics() {
	local iteration_dir="$1" iteration="$2" provider="$3" \
		iter_started="$4" iter_finished="$5" exit_code="$6"
	command -v jq >/dev/null || return 0
	local summary_line passed failed skipped errors rss_peak cpu_peak latency lat_p50 lat_p95 lat_p99 lat_n too_far_ahead
	summary_line="$(grep -E '^=+ .* in [0-9.]+s( \([^)]*\))? =+$' "$iteration_dir/pytest.log" 2>/dev/null | tail -1 || true)"
	passed="$(printf '%s' "$summary_line" | grep -oE '[0-9]+ passed' | grep -oE '[0-9]+' || echo 0)"
	failed="$(printf '%s' "$summary_line" | grep -oE '[0-9]+ failed' | grep -oE '[0-9]+' || echo 0)"
	skipped="$(printf '%s' "$summary_line" | grep -oE '[0-9]+ skipped' | grep -oE '[0-9]+' || echo 0)"
	errors="$(printf '%s' "$summary_line" | grep -oE '[0-9]+ error' | grep -oE '[0-9]+' || echo 0)"
	rss_peak="$(iteration_rss_peak_mb "$iteration_dir")"
	cpu_peak="$(iteration_cpu_peak_percent "$iteration_dir")"
	local cpu_per_node cpu_per_core
	cpu_per_node="$(iteration_cpu_peak_per_node_percent "$iteration_dir")"
	jq -e 'type == "object"' >/dev/null 2>&1 <<<"$cpu_per_node" || cpu_per_node='{}'
	cpu_per_core="$(iteration_cpu_peak_per_node_core_percent "$iteration_dir")"
	jq -e 'type == "object"' >/dev/null 2>&1 <<<"$cpu_per_core" || cpu_per_core='{}'
	latency="$(iteration_finalization_latency "$iteration_dir")"
	lat_p50="$(printf '%s' "$latency" | awk '{print $1}')"
	lat_p95="$(printf '%s' "$latency" | awk '{print $2}')"
	lat_p99="$(printf '%s' "$latency" | awk '{print $3}')"
	lat_n="$(printf '%s' "$latency" | awk '{print $4}')"
	too_far_ahead="$(iteration_too_far_ahead_errors "$iteration_dir")"
	# Registry-driven metrics (see scripts/bench/soak-metrics.json). Unlike the
	# bespoke extractors above, adding a metric here needs no code change: the
	# harness emits a SOAK_METRIC line and the registry declares it. Fail-soft
	# by the same contract — the collector yields {} rather than erroring.
	local registry_metrics
	registry_metrics="$("$SCRIPT_DIR/bench/collect-soak-metrics.sh" \
		"$iteration_dir/.started" \
		"$(
			IFS=:
			printf '%s' "${HARNESS_TELEMETRY_DIRS[*]}"
		)" 2>/dev/null || printf '{}')"
	jq -e . >/dev/null 2>&1 <<<"$registry_metrics" || registry_metrics='{}'
	jq -n \
		--argjson metrics "$registry_metrics" \
		--argjson iteration "$iteration" \
		--arg provider "$provider" \
		--argjson started "$iter_started" \
		--argjson finished "$iter_finished" \
		--argjson exit_code "$exit_code" \
		--argjson passed "$passed" \
		--argjson failed "$failed" \
		--argjson skipped "$skipped" \
		--argjson errors "$errors" \
		--argjson rss_peak "${rss_peak:-null}" \
		--argjson cpu_peak "${cpu_peak:-null}" \
		--argjson cpu_per_node "$cpu_per_node" \
		--argjson cpu_per_core "$cpu_per_core" \
		--argjson lat_p50 "${lat_p50:-null}" \
		--argjson lat_p95 "${lat_p95:-null}" \
		--argjson lat_p99 "${lat_p99:-null}" \
		--argjson lat_n "${lat_n:-null}" \
		--argjson too_far_ahead "${too_far_ahead:-0}" \
		'{iteration: $iteration, provider: $provider,
      started_at: $started, finished_at: $finished,
      duration_s: ($finished - $started), exit_code: $exit_code,
      pytest: {passed: $passed, failed: $failed, skipped: $skipped, errors: $errors},
      rss_peak_mb: $rss_peak,
      cpu_peak_pct: $cpu_peak,
      cpu_peak_per_node_pct: (if ($cpu_per_node | length) > 0 then $cpu_per_node else null end),
      cpu_peak_per_node_core_pct: (if ($cpu_per_core | length) > 0 then $cpu_per_core else null end),
      finalization_latency: {p50_ms: $lat_p50, p95_ms: $lat_p95, p99_ms: $lat_p99, samples: ($lat_n // 0)},
      too_far_ahead_errors: $too_far_ahead,
      metrics: $metrics,
      ok: ($exit_code == 0)}' >"$iteration_dir/metrics.json" 2>/dev/null || true
}

# True once the branch under test has advanced past the SHA this soak pinned
# at checkout. The soak builds one image and tests it for the whole run, so
# after a merge the results describe code that is no longer the branch head,
# and the runner is better spent starting fresh against the new tip.
#
# Gated behind MERGE_EXIT_MIN_SECONDS so a merge landing shortly after launch
# cannot reduce a night to a token soak. Fail-soft in every direction: an
# unreachable remote, an unreadable ref or a missing SHA all mean "keep
# soaking", never "stop" — a network hiccup must not end a 22-hour run.
target_ref_moved() {
	[ "$MERGE_EXIT_MIN_SECONDS" -gt 0 ] || return 1
	[ -n "$NODE_REPO_DIR" ] && [ -d "$NODE_REPO_DIR" ] || return 1
	[ "$TARGET_SHA" != "unknown" ] && [ "$TARGET_REF" != "unknown" ] || return 1
	[ "$(($(date +%s) - STARTED_AT))" -ge "$MERGE_EXIT_MIN_SECONDS" ] || return 1
	local remote_sha
	remote_sha="$(git -C "$NODE_REPO_DIR" ls-remote origin "$TARGET_REF" 2>/dev/null |
		awk 'NR == 1 {print $1}')"
	[ -n "$remote_sha" ] || return 1
	[ "$remote_sha" != "$TARGET_SHA" ]
}

run_bench_segment() {
	# Benchmark segments are fail-soft for the soak itself: a broken segment is
	# recorded and counted, and the perf-report job decides pass/fail from the
	# collected metrics.
	local remaining="$((DEADLINE - $(date +%s)))"
	if [ "$remaining" -le "$((BENCH_DURATION + 600))" ]; then
		return 0
	fi
	BENCH_SEGMENTS="$((BENCH_SEGMENTS + 1))"
	local segment_dir
	segment_dir="$OUTPUT_DIR/bench-segment-$(printf '%05d' "$BENCH_SEGMENTS")"
	mkdir -p "$segment_dir"
	NODE_REPO_DIR="$NODE_REPO_DIR" \
		OUT_DIR="$segment_dir" \
		BENCH_DURATION="$BENCH_DURATION" \
		BENCH_RATE="$BENCH_RATE" \
		SEGMENT_INDEX="$BENCH_SEGMENTS" \
		SOAK_STARTED_AT="$STARTED_AT" \
		"$SCRIPT_DIR/bench/run-bench-segment.sh" >"$segment_dir/segment.log" 2>&1
	local status=$?
	if [ "$status" -ne 0 ]; then
		BENCH_FAILURES="$((BENCH_FAILURES + 1))"
		printf '%s\n' "$status" >"$segment_dir/exit-code.txt"
		# The segment's output goes to files that die with the VM when the run is
		# lost — 30713818751's bench failed 100% silently for exactly that reason
		# (missing DEPLOYER_KEY, visible only in a bench.log nobody ever saw).
		printf 'bench segment %s failed (status %s); segment.log tail:\n' \
			"$BENCH_SEGMENTS" "$status" >&2
		tail -20 "$segment_dir/segment.log" >&2 || true
		if [ -s "$segment_dir/bench.log" ]; then
			printf 'bench.log tail:\n' >&2
			tail -20 "$segment_dir/bench.log" >&2 || true
		fi
	fi
}

mkdir -p "$OUTPUT_DIR"

# Only on the first segment: this is the run's opening baseline measurement,
# and repeating it at every resume would add segments the cadence never asked
# for and skew the run's active-benchmark averages.
if [ "$RUN_BENCHMARKS" = "true" ] && [ "$SEGMENT" -eq 1 ]; then
	run_bench_segment
	persist_soak_state
fi

# Operator signal, polled between iterations.
#
# A soak publishes only at a segment boundary, and those boundaries are fixed
# Pacific instants (07:30 / 13:00). A run starting off that grid — a short
# restart window, an afternoon dispatch — can run for hours with nothing on the
# dashboard, and a 60h weekend run is dark for its first twelve. This makes a
# boundary reachable on demand. The signal file is written by the poller in the
# soak-segment action, which reads it from this instance's own OCI freeform
# tags; see .github/workflows/soak-signal.yml for the sender.
#
# Checked here rather than in the poller because only this loop knows where an
# iteration boundary is. Stopping mid-iteration would leave a half-finished
# iteration that the aggregator counts as a real one, skewing the very numbers
# the checkpoint exists to report.
#
#   checkpoint  End THIS segment now, so the caller's aggregate/upload/publish
#               steps run exactly as at a scheduled checkpoint. The next
#               segment resumes from the state file with counters, start time
#               and iteration numbering intact, and the run continues.
#
#               Only possible where a segment boundary exists. The caller gates
#               those publish steps on having passed a deadline, so in the
#               final segment the request is refused rather than silently
#               ending the run and publishing nothing -- which is what it would
#               do if this simply broke the loop. Zero-checkpoint runs are all
#               final segment, so that is the common case, not a corner.
#
#   finalize    End the run. The marker survives into the remaining segments,
#               which exit immediately, so the run drains to the normal
#               end-of-run publish and produces a real verdict and history
#               entry rather than a latest-* refresh.
SIGNAL_FILE="${SOAK_SIGNAL_FILE:-$OUTPUT_DIR/signal}"
FINALIZE_MARKER="$OUTPUT_DIR/finalize-requested"
if [ -e "$FINALIZE_MARKER" ]; then
	printf 'finalize requested in an earlier segment; segment %s does no work\n' "$SEGMENT"
	DEADLINE=0
fi

# Orchestrator-level host guardian, independent of the whole test stack.
#
# The harness's protections live inside or under pytest: the in-process
# ceiling dies with the pytest process, and the docker provider sets no
# per-container memory limit. On run 30713818751 the kernel OOM killer shot
# pytest itself (oom_score_adj 500) while the docker-provider node containers
# — not pytest children — ran on uncapped and unwatched until the VM froze
# and the runner was lost. This loop is a separate process of this script,
# not of pytest, so it survives exactly that: on a sustained free-RAM floor
# breach it SIGKILLs every node process and container, writes a breach
# marker, and the iteration loop below fails the soak closed.
HOST_GUARDIAN_BREACH="$OUTPUT_DIR/host-guardian-breach.txt"
rm -f "$HOST_GUARDIAN_BREACH"
HOST_GUARDIAN_PID=""
ITERATION_PID=""
ITERATION_TEE_PID=""
ITERATION_SNAPSHOT_PID=""
ITERATION_FIFO=""
cleanup_soak_processes() {
	[ -z "$HOST_GUARDIAN_PID" ] || kill "$HOST_GUARDIAN_PID" 2>/dev/null || true
	if [ -n "$ITERATION_PID" ]; then
		kill "$ITERATION_PID" 2>/dev/null || true
		pkill -KILL -f 'integration-tests/test/tests/custom/test_load.py' 2>/dev/null || true
	fi
	[ -z "$ITERATION_TEE_PID" ] || kill "$ITERATION_TEE_PID" 2>/dev/null || true
	[ -z "$ITERATION_SNAPSHOT_PID" ] || kill "$ITERATION_SNAPSHOT_PID" 2>/dev/null || true
	[ -z "$ITERATION_FIFO" ] || rm -f "$ITERATION_FIFO"
}
trap cleanup_soak_processes EXIT
if [ "$HOST_FREE_FLOOR_MB" -gt 0 ] && [ -r /proc/meminfo ]; then
	(
		over=0
		while :; do
			sleep 5
			free_mb="$(awk '/^MemAvailable:/ {print int($2 / 1024)}' /proc/meminfo 2>/dev/null)"
			[ -n "$free_mb" ] || continue
			if [ "$free_mb" -ge "$HOST_FREE_FLOOR_MB" ]; then
				over=0
				continue
			fi
			over=$((over + 1))
			[ "$over" -lt 3 ] && continue
			# Kill first, marker second: the marker asserts the host was defended,
			# so it must not exist before the kills have run. `|| true` on each —
			# pkill exits 1 with no matching process (normal when only containers
			# are up), and neither miss may stop the other mitigation.
			pkill -9 -f '/tmp/rnode' 2>/dev/null || true
			docker ps -q --filter 'name=rnode.' 2>/dev/null | xargs -r docker kill 2>/dev/null || true
			printf 'orchestrator host guardian: host available RAM %sMB < floor %sMB for 3 consecutive samples (5s each); killed all node processes and containers to protect the host\n' \
				"$free_mb" "$HOST_FREE_FLOOR_MB" >"$HOST_GUARDIAN_BREACH"
			exit 0
		done
	) &
	HOST_GUARDIAN_PID=$!
	printf 'orchestrator host guardian watching MemAvailable floor %sMB (pid %s)\n' \
		"$HOST_FREE_FLOOR_MB" "$HOST_GUARDIAN_PID"
fi

while [ "$(date +%s)" -lt "$DEADLINE" ]; do
	# The orchestrator guardian can fire outside a failing iteration — during a
	# bench segment, or after an iteration that still exited 0. Never start new
	# work once the host has been defended.
	if [ -s "$HOST_GUARDIAN_BREACH" ]; then
		EARLY_EXIT_REASON="host_protection_breach"
		printf 'orchestrator host guardian fired; ending soak (fail-closed)\n'
		head -1 "$HOST_GUARDIAN_BREACH" | tee "$OUTPUT_DIR/protection-breach.txt"
		printf 'host_protection_breach: %s\n' "$(head -1 "$HOST_GUARDIAN_BREACH")" \
			>"$OUTPUT_DIR/early-exit.txt"
		FAILURES="$((FAILURES + 1))"
		break
	fi
	if [ -e "$SIGNAL_FILE" ]; then
		SIGNAL="$(tr -d '[:space:]' <"$SIGNAL_FILE" 2>/dev/null || true)"
		# Consumed either way: an unread signal re-fires on every later iteration
		# and every later segment.
		rm -f "$SIGNAL_FILE"
		case "$SIGNAL" in
		checkpoint)
			if [ "$HAS_CHECKPOINT_BOUNDARY" -eq 1 ]; then
				printf 'checkpoint signalled after iteration %s; ending segment %s\n' "$ITERATIONS" "$SEGMENT"
				printf '%s\n' "checkpoint signalled after iteration $ITERATIONS" \
					>"$OUTPUT_DIR/signalled-checkpoint.txt"
				break
			fi
			printf 'checkpoint signalled, but segment %s is the final segment and publishes no checkpoint; ignoring. Use finalize to end the run and publish a full verdict.\n' \
				"$SEGMENT" >&2
			;;
		finalize)
			printf 'finalize signalled after iteration %s; ending the run\n' "$ITERATIONS"
			printf '%s\n' "finalize signalled after iteration $ITERATIONS" >"$FINALIZE_MARKER"
			break
			;;
		'')
			;;
		*)
			printf 'ignoring unrecognised soak signal: %s\n' "$SIGNAL" >&2
			;;
		esac
	fi
	PROVIDER="${PROVIDERS[$((ITERATIONS % ${#PROVIDERS[@]}))]}"
	ITERATIONS="$((ITERATIONS + 1))"
	persist_soak_state
	ITERATION_DIR="$OUTPUT_DIR/iteration-$(printf '%05d' "$ITERATIONS")-$PROVIDER"
	mkdir -p "$ITERATION_DIR"
	REMAINING="$((DEADLINE - $(date +%s)))"
	if [ "$REMAINING" -le 0 ]; then
		break
	fi

	ITER_STARTED="$(date +%s)"
	touch "$ITERATION_DIR/.started"
	ITERATION_FIFO="$ITERATION_DIR/.pytest-output.fifo"
	rm -f "$ITERATION_FIFO"
	mkfifo "$ITERATION_FIFO"
	tee "$ITERATION_DIR/pytest.log" <"$ITERATION_FIFO" &
	ITERATION_TEE_PID=$!
	(
		cd "$SYSTEM_INTEGRATION_DIR"
		exec timeout --signal=TERM --kill-after=30 "${REMAINING}s" \
			poetry run pytest \
			integration-tests/test/tests/custom/test_load.py \
			--provider="$PROVIDER" \
			--monitor \
			--rss-ceiling-mb "$RSS_CEILING_MB" \
			--host-free-floor-mb "$HOST_FREE_FLOOR_MB" \
			-v --tb=short --instafail --maxfail=20 \
			--timeout=1200
	) >"$ITERATION_FIFO" 2>&1 &
	ITERATION_PID=$!
	snapshot_iteration_monitor_outputs "$ITERATION_DIR" &
	ITERATION_SNAPSHOT_PID=$!
	GUARDIAN_INTERRUPTED=0
	while kill -0 "$ITERATION_PID" 2>/dev/null; do
		if [ -s "$HOST_GUARDIAN_BREACH" ]; then
			GUARDIAN_INTERRUPTED=1
			kill -TERM "$ITERATION_PID" 2>/dev/null || true
			for _ in $(seq 1 15); do
				kill -0 "$ITERATION_PID" 2>/dev/null || break
				sleep 1
			done
			kill -KILL "$ITERATION_PID" 2>/dev/null || true
			break
		fi
		sleep "${SOAK_GUARDIAN_POLL_SECONDS:-2}"
	done
	wait "$ITERATION_PID" 2>/dev/null
	STATUS=$?
	wait "$ITERATION_TEE_PID" 2>/dev/null || true
	kill "$ITERATION_SNAPSHOT_PID" 2>/dev/null || true
	wait "$ITERATION_SNAPSHOT_PID" 2>/dev/null || true
	rm -f "$ITERATION_FIFO"
	ITERATION_PID=""
	ITERATION_TEE_PID=""
	ITERATION_SNAPSHOT_PID=""
	ITERATION_FIFO=""
	if [ "$GUARDIAN_INTERRUPTED" -eq 1 ]; then
		STATUS=1
		pkill -9 -f '/tmp/rnode' 2>/dev/null || true
		docker ps -aq --filter 'name=rnode.' 2>/dev/null | xargs -r docker rm -f >/dev/null 2>&1 || true
	fi
	# No `set -e` restore: this script never enables errexit (line 2 is
	# `set -uo pipefail`), and turning it on here made the first failed
	# iteration fatal — the metric pipelines return nonzero when a failed
	# iteration leaves nothing to sample, which killed every segment mid-loop
	# before the state file or rollup could be written (run 30516534214).
	ITER_FINISHED="$(date +%s)"
	emit_iteration_metrics "$ITERATION_DIR" "$ITERATIONS" "$PROVIDER" \
		"$ITER_STARTED" "$ITER_FINISHED" "$STATUS" || true

	if [ "$STATUS" -eq 124 ] && [ "$(date +%s)" -ge "$DEADLINE" ]; then
		printf '%s\n' "deadline reached during iteration $ITERATIONS" >"$ITERATION_DIR/deadline.txt"
		break
	fi
	if [ "$STATUS" -ne 0 ]; then
		FAILURES="$((FAILURES + 1))"
		persist_soak_state
		printf '%s\n' "$STATUS" >"$ITERATION_DIR/exit-code.txt"
		for evidence_root in "${HARNESS_TELEMETRY_DIRS[@]}"; do
			[ -d "$evidence_root" ] || continue
			# Evidence only — logs, CSVs, configs. A full `cp -a` drags the nodes'
			# LMDB data along, which ran to a silent half-hour stall between
			# iterations on run 30713818751 (19:16→19:45 with zero output) and
			# would bloat the results artifact past uploadable size.
			COPY_STARTED="$(date +%s)"
			evidence_name="$(basename "$evidence_root")"
			printf 'preserving failure evidence from integration-tests/%s (iteration %s)\n' \
				"$evidence_name" "$ITERATIONS"
			mkdir -p "$ITERATION_DIR/$evidence_name"
			(cd "$evidence_root" &&
				find . -type f \( -name '*.log' -o -name '*.csv' -o -name '*.txt' \
					-o -name '*.json' -o -name '*.conf' -o -name '*.toml' \) -print0 |
				tar --null -T - -cf -) |
				tar -xf - -C "$ITERATION_DIR/$evidence_name" ||
				printf 'failure-evidence copy incomplete (non-fatal)\n' >&2
			printf 'failure evidence preserved in %ss\n' "$(($(date +%s) - COPY_STARTED))"
		done
		# Fail closed on a host-protection breach. The guardian killing the nodes
		# means the load does not fit under the configured ceiling/floor on this
		# host; every further iteration reproduces the breach (30713818751 breached
		# on all three iterations across both providers), and each one is another
		# spin of a workload known to endanger the host. Preserve evidence above,
		# then end the whole soak: the early-exit marker makes every remaining
		# segment a no-op, and the job still reaches its completion marker so
		# retry_within_window does not relaunch the same doomed run on a fresh VM.
		#
		# Three channels, most authoritative first: the orchestrator guardian's
		# own marker, the harness monitor's marker file, and — for harness
		# versions predating the marker — the breach message in the pytest log.
		BREACH_LINE=""
		if [ -s "$HOST_GUARDIAN_BREACH" ]; then
			BREACH_LINE="$(head -1 "$HOST_GUARDIAN_BREACH")"
		else
			HARNESS_MARKER="$(find "${HARNESS_TELEMETRY_DIRS[@]}" \
				-name host-protection-breach.txt -newer "$ITERATION_DIR/.started" 2>/dev/null |
				head -1 || true)"
			if [ -n "$HARNESS_MARKER" ]; then
				BREACH_LINE="$(head -1 "$HARNESS_MARKER")"
			else
				BREACH_LINE="$(grep -m1 -E \
					'Host-protection guardian breach|Resource ceiling breached|host-protection watchdog killed' \
					"$ITERATION_DIR/pytest.log" 2>/dev/null || true)"
			fi
		fi
		if [ -n "$BREACH_LINE" ]; then
			EARLY_EXIT_REASON="host_protection_breach"
			printf 'host-protection breach in iteration %s; ending soak (fail-closed)\n' "$ITERATIONS"
			printf '%s\n' "$BREACH_LINE" | tee "$OUTPUT_DIR/protection-breach.txt"
			printf 'host_protection_breach: iteration %s: %s\n' "$ITERATIONS" "$BREACH_LINE" \
				>"$OUTPUT_DIR/early-exit.txt"
			break
		fi
		sleep 30
	fi

	if target_ref_moved; then
		EARLY_EXIT_REASON="target_advanced"
		printf '%s advanced past %s; ending soak after iteration %s\n' \
			"$TARGET_REF" "$TARGET_SHA" "$ITERATIONS" |
			tee "$OUTPUT_DIR/early-exit.txt"
		break
	fi

	if [ "$RUN_BENCHMARKS" = "true" ] && [ "$((ITERATIONS % BENCH_EVERY))" -eq 0 ]; then
		run_bench_segment
		persist_soak_state
	fi

done

# A breach during the final iteration (or one that still exited 0) ends the
# loop by deadline without passing the top-of-loop check, which would let the
# run finish green with the host guardian's marker never surfaced.
if [ -z "$EARLY_EXIT_REASON" ] && [ -s "$HOST_GUARDIAN_BREACH" ]; then
	EARLY_EXIT_REASON="host_protection_breach"
	printf 'orchestrator host guardian fired during the final iteration; recording fail-closed exit\n'
	head -1 "$HOST_GUARDIAN_BREACH" | tee "$OUTPUT_DIR/protection-breach.txt"
	printf 'host_protection_breach: %s\n' "$(head -1 "$HOST_GUARDIAN_BREACH")" \
		>"$OUTPUT_DIR/early-exit.txt"
	FAILURES="$((FAILURES + 1))"
fi

FINISHED_AT="$(date +%s)"

# Written before the rollup so a later segment resumes from accurate counters
# even if the rollup below fails.
persist_soak_state

{
	printf 'started_at=%s\n' "$STARTED_AT"
	printf 'segments=%s\n' "$SEGMENT"
	printf 'finished_at=%s\n' "$FINISHED_AT"
	printf 'target_ref=%s\n' "$TARGET_REF"
	printf 'target_sha=%s\n' "$TARGET_SHA"
	printf 'requested_seconds=%s\n' "$DURATION_SECONDS"
	printf 'elapsed_seconds=%s\n' "$((FINISHED_AT - STARTED_AT))"
	printf 'iterations=%s\n' "$ITERATIONS"
	printf 'failures=%s\n' "$FAILURES"
	printf 'bench_segments=%s\n' "$BENCH_SEGMENTS"
	printf 'bench_failures=%s\n' "$BENCH_FAILURES"
	printf 'early_exit_reason=%s\n' "${EARLY_EXIT_REASON:-none}"
} | tee "$OUTPUT_DIR/summary.txt"

if command -v jq >/dev/null; then
	SOAK_OUTPUT_DIR="$OUTPUT_DIR" \
		SOAK_METRICS_REGISTRY="$SCRIPT_DIR/bench/soak-metrics.json" \
		SOAK_TARGET_REF="$TARGET_REF" \
		SOAK_TARGET_SHA="$TARGET_SHA" \
		SOAK_TRIGGER_SOURCE="$TRIGGER_SOURCE" \
		SOAK_SLOT_DELAY_SECONDS="$SLOT_DELAY_SECONDS" \
		SOAK_VERSION="$VERSION" \
		SOAK_STARTED_AT="$STARTED_AT" \
		SOAK_FINISHED_AT="$FINISHED_AT" \
		SOAK_DURATION_SECONDS="$DURATION_SECONDS" \
		SOAK_ITERATIONS="$ITERATIONS" \
		SOAK_FAILURES="$FAILURES" \
		SOAK_BENCH_SEGMENTS="$BENCH_SEGMENTS" \
		SOAK_BENCH_FAILURES="$BENCH_FAILURES" \
		"$SCRIPT_DIR/bench/write-soak-summary.sh" ||
		{
			# The full rollup failing must not leave the run without passive data:
			# aggregate-perf-report.sh then emits started_at/elapsed_seconds as
			# null, and Soak Checkpoint Publish rejects that checkpoint outright
			# ("metadata does not match"). Fall back to the counters this script
			# already holds — nulls for the sampled metrics, real values for the
			# run identity and timing the publish contract validates.
			printf 'summary.json emission failed; writing minimal fallback summary\n' >&2
			jq -n \
				--arg target_ref "$TARGET_REF" \
				--arg target_sha "$TARGET_SHA" \
				--arg trigger_source "$TRIGGER_SOURCE" \
				--argjson slot_delay "$SLOT_DELAY_SECONDS" \
				--arg version "$VERSION" \
				--argjson started "$STARTED_AT" \
				--argjson finished "$FINISHED_AT" \
				--argjson requested "$DURATION_SECONDS" \
				--argjson iterations "$ITERATIONS" \
				--argjson failures "$FAILURES" \
				--argjson bench_segments "$BENCH_SEGMENTS" \
				--argjson bench_failures "$BENCH_FAILURES" \
				'{target_ref: $target_ref, target_sha: $target_sha, version: $version,
          trigger_source: $trigger_source, slot_delay_seconds: $slot_delay,
          started_at: $started, finished_at: $finished,
          requested_seconds: $requested,
          elapsed_seconds: ($finished - $started),
          iterations: $iterations, failures: $failures,
          failure_rate: (if $iterations > 0 then ($failures / $iterations) else 0 end),
          bench_segments: $bench_segments, bench_failures: $bench_failures,
          degraded: "full summary emission failed; sampled metrics missing"}' \
				>"$OUTPUT_DIR/summary.json" ||
				printf 'fallback summary emission failed too (non-fatal)\n' >&2
		}
fi

if [ "$FAILURES" -ne 0 ]; then
	exit 1
fi
