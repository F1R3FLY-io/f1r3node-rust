#!/usr/bin/env bash
set -euo pipefail

OUTPUT_DIR="${SOAK_OUTPUT_DIR:?SOAK_OUTPUT_DIR is required}"
REGISTRY="${SOAK_METRICS_REGISTRY:?SOAK_METRICS_REGISTRY is required}"
TARGET_REF="${SOAK_TARGET_REF:-unknown}"
TARGET_SHA="${SOAK_TARGET_SHA:-unknown}"
TRIGGER_SOURCE="${SOAK_TRIGGER_SOURCE:-manual}"
SLOT_DELAY_SECONDS="${SOAK_SLOT_DELAY_SECONDS:-0}"
if ! [[ "$SLOT_DELAY_SECONDS" =~ ^[0-9]+$ ]]; then
	SLOT_DELAY_SECONDS=0
fi
VERSION="${SOAK_VERSION:-unknown}"
STARTED_AT="${SOAK_STARTED_AT:?SOAK_STARTED_AT is required}"
FINISHED_AT="${SOAK_FINISHED_AT:?SOAK_FINISHED_AT is required}"
DURATION_SECONDS="${SOAK_DURATION_SECONDS:?SOAK_DURATION_SECONDS is required}"
ITERATIONS="${SOAK_ITERATIONS:?SOAK_ITERATIONS is required}"
FAILURES="${SOAK_FAILURES:?SOAK_FAILURES is required}"
BENCH_SEGMENTS="${SOAK_BENCH_SEGMENTS:?SOAK_BENCH_SEGMENTS is required}"
BENCH_FAILURES="${SOAK_BENCH_FAILURES:?SOAK_BENCH_FAILURES is required}"

ITERATIONS_JSON="$OUTPUT_DIR/iterations.json"
while IFS= read -r -d '' metrics; do
	jq -e 'select(type == "object")' "$metrics" 2>/dev/null || true
done < <(find "$OUTPUT_DIR" -path '*iteration-*/metrics.json' -print0 | sort -z) |
	jq -s 'sort_by(.iteration // 0)' >"$ITERATIONS_JSON"

jq -n \
	--slurpfile iters "$ITERATIONS_JSON" \
	--slurpfile registry "$REGISTRY" \
	--arg target_ref "$TARGET_REF" \
	--arg target_sha "$TARGET_SHA" \
	--arg trigger_source "$TRIGGER_SOURCE" \
	--argjson slot_delay "${SLOT_DELAY_SECONDS:-0}" \
	--arg version "$VERSION" \
	--argjson started "$STARTED_AT" \
	--argjson finished "$FINISHED_AT" \
	--argjson requested "$DURATION_SECONDS" \
	--argjson iterations "$ITERATIONS" \
	--argjson failures "$FAILURES" \
	--argjson bench_segments "$BENCH_SEGMENTS" \
	--argjson bench_failures "$BENCH_FAILURES" \
	'
    def median: sort | if length == 0 then null else .[(length - 1) / 2 | floor] end;
    def max_or_null: if length == 0 then null else max end;
    ($iters[0] | map(select(type == "object"))) as $all
    | def numeric_values(path): [$all[] | path | select(type == "number")];
      def provider_split(p):
        ($all | map(select(.provider? == p))) as $provider_iters
        | {iterations: ($provider_iters | length),
           failures: ($provider_iters | map(select((.ok? // false) | not)) | length),
           avg_duration_s: (([$provider_iters[] | .duration_s? | select(type == "number")]) as $durations
             | if ($durations | length) > 0
               then (($durations | add) / ($durations | length) | floor)
               else null end)};
      def rollup_tracked_metrics:
        ($registry[0].metrics // []) as $defs
        | [ $defs[] | . as $mdef
            | ([$all[] | .metrics?[$mdef.key]? | select(type == "object")]) as $samples
            | select(($samples | length) > 0)
            | {
                key: $mdef.key,
                value: (
                  reduce ($mdef.aggregate // ["p50", "p95", "max"])[] as $agg
                    ({};
                     ($samples | map(.[$agg]? | select(type == "number"))) as $values
                     | . + {($agg): (if ($values | length) == 0 then null
                                      elif $agg == "max" then ($values | max)
                                      elif $agg == "min" then ($values | min)
                                      else ($values | median) end)})
                  | . + {samples: ([$samples[] | .samples? | select(type == "number")] | add // 0)}
                )
              }
          ]
        | from_entries;
      ($finished - $started) as $elapsed
      | {
          target_ref: $target_ref,
          target_sha: $target_sha,
          trigger_source: $trigger_source,
          slot_delay_seconds: $slot_delay,
          version: $version,
          started_at: $started,
          finished_at: $finished,
          requested_seconds: $requested,
          elapsed_seconds: $elapsed,
          # Shard uptime at iteration granularity: the summed wall-clock of
          # iterations that completed their full bring-up -> load -> finalize
          # cycle. A failed or watchdog-killed iteration contributes nothing —
          # the shard was not reliably up for any of it — and inter-iteration
          # gaps count as downtime. Feeds the dashboard uptime bar
          # (run.shard_up_seconds vs duration/elapsed).
          shard_up_seconds: ([$all[] | select(.ok? == true) | .duration_s? | select(type == "number")] | add // 0),
          iterations: $iterations,
          failures: $failures,
          failure_rate: (if $iterations > 0 then ($failures / $iterations) else 0 end),
          iterations_per_hour: (if $elapsed > 0 then ($iterations * 3600 / $elapsed * 100 | floor / 100) else 0 end),
          rss_peak_mb: (numeric_values(.rss_peak_mb?) | max_or_null),
          cpu_peak_pct: (numeric_values(.cpu_peak_pct?) | max_or_null),
          # The dashboard CPU grid: per-node peaks across the whole run
          # (cell-wise max over iterations), published under the "all" core
          # row because the harness monitor samples per container, not per
          # core — real core rows take over when it does. Null (absent) when
          # no iteration recorded a per-node map, so pre-emission history
          # entries and this run stay shaped alike.
          cpu_peak_core_grid_pct: (
            [$all[] | .cpu_peak_per_node_pct? | select(type == "object") | to_entries[]
             | select(.value | type == "number")] as $cells
            | if ($cells | length) == 0 then null
              else ($cells | group_by(.key)
                    | map({key: .[0].key, value: {all: (map(.value) | max)}})
                    | from_entries)
              end),
          finalization_p50_ms: (numeric_values(.finalization_latency?.p50_ms?) | median),
          finalization_p95_ms: (numeric_values(.finalization_latency?.p95_ms?) | median),
          finalization_p99_ms: (numeric_values(.finalization_latency?.p99_ms?) | median),
          too_far_ahead_errors: (numeric_values(.too_far_ahead_errors?) | add // 0),
          providers: {docker: provider_split("docker"), subprocess: provider_split("subprocess")},
          bench_segments: $bench_segments,
          bench_failures: $bench_failures,
          tracked_metrics: rollup_tracked_metrics,
          iteration_metrics: $all
        }
  ' >"$OUTPUT_DIR/summary.json"
