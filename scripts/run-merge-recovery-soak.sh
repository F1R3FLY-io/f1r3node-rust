#!/usr/bin/env bash
set -uo pipefail

DURATION_SECONDS="${SOAK_DURATION_SECONDS:?SOAK_DURATION_SECONDS is required}"
SYSTEM_INTEGRATION_DIR="${SYSTEM_INTEGRATION_DIR:?SYSTEM_INTEGRATION_DIR is required}"
OUTPUT_DIR="${SOAK_OUTPUT_DIR:-/tmp/merge-recovery-soak}"
TARGET_REF="${SOAK_TARGET_REF:-unknown}"
TARGET_SHA="${SOAK_TARGET_SHA:-unknown}"
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
if [ -n "${SOAK_DEADLINE_EPOCH:-}" ]; then
  if ! [[ "$SOAK_DEADLINE_EPOCH" =~ ^[1-9][0-9]*$ ]]; then
    printf 'SOAK_DEADLINE_EPOCH must be a positive integer\n' >&2
    exit 2
  fi
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

if [ "$RUN_BENCHMARKS" = "true" ] && [ -z "$NODE_REPO_DIR" ]; then
  printf 'SOAK_NODE_REPO_DIR is required when SOAK_RUN_BENCHMARKS=true\n' >&2
  exit 2
fi

# Peak total node RSS for this iteration, from the newest harness
# resource-timeseries.csv written after the iteration's start marker
# (columns: elapsed_s,node,memory_mb,cpu_percent,memory_limit_mb; the
# __system__ row is host state, not node RSS). Empty output when absent.
iteration_rss_peak_mb() {
  local iteration_dir="$1" ts_csv
  ts_csv="$(find "$SYSTEM_INTEGRATION_DIR/integration-tests/data" \
    -name resource-timeseries.csv -newer "$iteration_dir/.started" 2>/dev/null \
    | xargs -r ls -t 2>/dev/null | head -1)"
  [ -n "$ts_csv" ] || return 0
  cp "$ts_csv" "$iteration_dir/resource-timeseries.csv" 2>/dev/null || true
  awk -F, 'NR > 1 && $2 != "__system__" { sum[$1] += $3 }
           END { max = 0; for (t in sum) if (sum[t] > max) max = sum[t]
                 if (max > 0) printf "%.0f\n", max }' "$ts_csv" 2>/dev/null
}

# Propose-timing latency samples (total_ms) from node JSON logs written after
# the iteration's start marker — the f1r3fly.propose.timing parse target from
# profile-casper-latency.sh. Emits "p50 p95 count" or nothing.
iteration_finalization_latency() {
  local iteration_dir="$1"
  find "$SYSTEM_INTEGRATION_DIR/integration-tests/data" \
    -name '*.log' -newer "$iteration_dir/.started" 2>/dev/null \
    | xargs -r grep -h -o 'Propose timing:[^"]*' 2>/dev/null \
    | grep -oE 'total_ms=[0-9]+' | grep -oE '[0-9]+' \
    | sort -n \
    | awk '{ a[NR] = $1 }
           END { if (NR == 0) exit
                 p50 = a[int((NR + 1) * 0.5)]; p95 = a[int((NR + 1) * 0.95)]
                 print p50, p95, NR }'
}

# Parse the pytest terminal summary line ("== 1 failed, 64 passed, ... ==")
# and emit a per-iteration metrics.json with resource + latency samples.
# Metrics are additive: missing jq or an unparseable log must never fail
# the soak.
emit_iteration_metrics() {
  local iteration_dir="$1" iteration="$2" provider="$3" \
        iter_started="$4" iter_finished="$5" exit_code="$6"
  command -v jq >/dev/null || return 0
  local summary_line passed failed skipped errors rss_peak latency lat_p50 lat_p95 lat_n
  summary_line="$(grep -E '^=+ .* in [0-9.]+s( \([^)]*\))? =+$' "$iteration_dir/pytest.log" 2>/dev/null | tail -1)"
  passed="$(printf '%s' "$summary_line" | grep -oE '[0-9]+ passed' | grep -oE '[0-9]+' || echo 0)"
  failed="$(printf '%s' "$summary_line" | grep -oE '[0-9]+ failed' | grep -oE '[0-9]+' || echo 0)"
  skipped="$(printf '%s' "$summary_line" | grep -oE '[0-9]+ skipped' | grep -oE '[0-9]+' || echo 0)"
  errors="$(printf '%s' "$summary_line" | grep -oE '[0-9]+ error' | grep -oE '[0-9]+' || echo 0)"
  rss_peak="$(iteration_rss_peak_mb "$iteration_dir")"
  latency="$(iteration_finalization_latency "$iteration_dir")"
  lat_p50="$(printf '%s' "$latency" | awk '{print $1}')"
  lat_p95="$(printf '%s' "$latency" | awk '{print $2}')"
  lat_n="$(printf '%s' "$latency" | awk '{print $3}')"
  jq -n \
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
    --argjson lat_p50 "${lat_p50:-null}" \
    --argjson lat_p95 "${lat_p95:-null}" \
    --argjson lat_n "${lat_n:-null}" \
    '{iteration: $iteration, provider: $provider,
      started_at: $started, finished_at: $finished,
      duration_s: ($finished - $started), exit_code: $exit_code,
      pytest: {passed: $passed, failed: $failed, skipped: $skipped, errors: $errors},
      rss_peak_mb: $rss_peak,
      finalization_latency: {p50_ms: $lat_p50, p95_ms: $lat_p95, samples: ($lat_n // 0)},
      ok: ($exit_code == 0)}' > "$iteration_dir/metrics.json" 2>/dev/null || true
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
  [ "$(( $(date +%s) - STARTED_AT ))" -ge "$MERGE_EXIT_MIN_SECONDS" ] || return 1
  local remote_sha
  remote_sha="$(git -C "$NODE_REPO_DIR" ls-remote origin "$TARGET_REF" 2>/dev/null \
    | awk 'NR == 1 {print $1}')"
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
  set +e
  NODE_REPO_DIR="$NODE_REPO_DIR" \
  OUT_DIR="$segment_dir" \
  BENCH_DURATION="$BENCH_DURATION" \
  BENCH_RATE="$BENCH_RATE" \
  SEGMENT_INDEX="$BENCH_SEGMENTS" \
  SOAK_STARTED_AT="$STARTED_AT" \
    "$SCRIPT_DIR/bench/run-bench-segment.sh" > "$segment_dir/segment.log" 2>&1
  local status=$?
  set -e
  if [ "$status" -ne 0 ]; then
    BENCH_FAILURES="$((BENCH_FAILURES + 1))"
    printf '%s\n' "$status" > "$segment_dir/exit-code.txt"
  fi
}

mkdir -p "$OUTPUT_DIR"

# Only on the first segment: this is the run's opening baseline measurement,
# and repeating it at every resume would add segments the cadence never asked
# for and skew the run's active-benchmark averages.
if [ "$RUN_BENCHMARKS" = "true" ] && [ "$SEGMENT" -eq 1 ]; then
  run_bench_segment
fi

while [ "$(date +%s)" -lt "$DEADLINE" ]; do
  PROVIDER="${PROVIDERS[$((ITERATIONS % ${#PROVIDERS[@]}))]}"
  ITERATIONS="$((ITERATIONS + 1))"
  ITERATION_DIR="$OUTPUT_DIR/iteration-$(printf '%05d' "$ITERATIONS")-$PROVIDER"
  mkdir -p "$ITERATION_DIR"
  REMAINING="$((DEADLINE - $(date +%s)))"
  if [ "$REMAINING" -le 0 ]; then
    break
  fi

  ITER_STARTED="$(date +%s)"
  touch "$ITERATION_DIR/.started"
  set +e
  (
    cd "$SYSTEM_INTEGRATION_DIR"
    timeout --signal=TERM --kill-after=120 "${REMAINING}s" \
      poetry run pytest \
      integration-tests/test/tests/custom/test_load.py \
      --provider="$PROVIDER" \
      --monitor \
      -v --tb=short --instafail --maxfail=20 \
      --timeout=1200
  ) 2>&1 | tee "$ITERATION_DIR/pytest.log"
  STATUS="${PIPESTATUS[0]}"
  set -e
  ITER_FINISHED="$(date +%s)"
  emit_iteration_metrics "$ITERATION_DIR" "$ITERATIONS" "$PROVIDER" \
    "$ITER_STARTED" "$ITER_FINISHED" "$STATUS"

  if [ "$STATUS" -eq 124 ] && [ "$(date +%s)" -ge "$DEADLINE" ]; then
    printf '%s\n' "deadline reached during iteration $ITERATIONS" > "$ITERATION_DIR/deadline.txt"
    break
  fi
  if [ "$STATUS" -ne 0 ]; then
    FAILURES="$((FAILURES + 1))"
    printf '%s\n' "$STATUS" > "$ITERATION_DIR/exit-code.txt"
    if [ -d "$SYSTEM_INTEGRATION_DIR/integration-tests/data" ]; then
      cp -a "$SYSTEM_INTEGRATION_DIR/integration-tests/data" "$ITERATION_DIR/data"
    fi
    sleep 30
  fi

  if target_ref_moved; then
    EARLY_EXIT_REASON="target_advanced"
    printf '%s advanced past %s; ending soak after iteration %s\n' \
      "$TARGET_REF" "$TARGET_SHA" "$ITERATIONS" \
      | tee "$OUTPUT_DIR/early-exit.txt"
    break
  fi

  if [ "$RUN_BENCHMARKS" = "true" ] && [ "$((ITERATIONS % BENCH_EVERY))" -eq 0 ]; then
    run_bench_segment
  fi

done

FINISHED_AT="$(date +%s)"

# Written before the rollup so a later segment resumes from accurate counters
# even if the rollup below fails.
{
  printf 'STARTED_AT=%s\n' "$STARTED_AT"
  printf 'ITERATIONS=%s\n' "$ITERATIONS"
  printf 'FAILURES=%s\n' "$FAILURES"
  printf 'BENCH_SEGMENTS=%s\n' "$BENCH_SEGMENTS"
  printf 'BENCH_FAILURES=%s\n' "$BENCH_FAILURES"
  printf 'SEGMENT=%s\n' "$SEGMENT"
} > "$STATE_FILE"

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
  find "$OUTPUT_DIR" -path '*iteration-*/metrics.json' -print0 \
    | sort -z \
    | xargs -0 --no-run-if-empty cat \
    | jq -s 'sort_by(.iteration)' > "$OUTPUT_DIR/iterations.json"
  [ -s "$OUTPUT_DIR/iterations.json" ] || echo '[]' > "$OUTPUT_DIR/iterations.json"
  jq -n \
    --slurpfile iters "$OUTPUT_DIR/iterations.json" \
    --arg target_ref "$TARGET_REF" \
    --arg target_sha "$TARGET_SHA" \
    --argjson started "$STARTED_AT" \
    --argjson finished "$FINISHED_AT" \
    --argjson requested "$DURATION_SECONDS" \
    --argjson iterations "$ITERATIONS" \
    --argjson failures "$FAILURES" \
    --argjson bench_segments "$BENCH_SEGMENTS" \
    --argjson bench_failures "$BENCH_FAILURES" \
    '
    def median: sort | if length == 0 then null else .[(length - 1) / 2 | floor] end;
    def provider_split(p):
      ($iters[0] | map(select(.provider == p))) as $p_iters
      | {iterations: ($p_iters | length),
         failures: ($p_iters | map(select(.ok | not)) | length),
         avg_duration_s: (if ($p_iters | length) > 0
                          then (($p_iters | map(.duration_s) | add) / ($p_iters | length) | floor)
                          else null end)};
    ($finished - $started) as $elapsed
    | {
        target_ref: $target_ref,
        target_sha: $target_sha,
        started_at: $started,
        finished_at: $finished,
        requested_seconds: $requested,
        elapsed_seconds: $elapsed,
        iterations: $iterations,
        failures: $failures,
        failure_rate: (if $iterations > 0 then ($failures / $iterations) else 0 end),
        iterations_per_hour: (if $elapsed > 0 then ($iterations * 3600 / $elapsed * 100 | floor / 100) else 0 end),
        rss_peak_mb: ($iters[0] | map(.rss_peak_mb | select(. != null)) | max),
        finalization_p95_ms: ($iters[0] | map(.finalization_latency.p95_ms | select(. != null)) | median),
        providers: {docker: provider_split("docker"), subprocess: provider_split("subprocess")},
        bench_segments: $bench_segments,
        bench_failures: $bench_failures,
        iteration_metrics: $iters[0]
      }' > "$OUTPUT_DIR/summary.json" 2>/dev/null \
    || printf 'summary.json emission failed (non-fatal)\n' >&2
fi

if [ "$FAILURES" -ne 0 ]; then
  exit 1
fi
