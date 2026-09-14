#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
if [ -n "${SOAK_REPORT_TEST_DIR:-}" ]; then
	TMP="$SOAK_REPORT_TEST_DIR"
	mkdir "$TMP"
else
	TMP="$(mktemp -d)"
	trap 'rm -rf "$TMP"' EXIT
fi

mkdir -p "$TMP/failed" "$TMP/failed-report"
cat >"$TMP/failed/summary.json" <<'JSON'
{
  "target_ref": "dev",
  "target_sha": "0123456789abcdef",
  "version": "0.4.43",
  "started_at": 1000,
  "finished_at": 1100,
  "elapsed_seconds": 100,
  "iterations": 1,
  "failures": 1,
  "failure_rate": 1,
  "iterations_per_hour": 36,
  "rss_peak_mb": 17100,
  "cpu_peak_pct": 100,
  "cpu_peak_core_grid_pct": {"validator1": {"all": 55.5}, "bootstrap": {"all": 12}},
  "finalization_p50_ms": null,
  "finalization_p95_ms": null,
  "finalization_p99_ms": null,
  "too_far_ahead_errors": 0,
  "providers": {
    "docker": {"iterations": 1, "failures": 1, "avg_duration_s": 100},
    "subprocess": {"iterations": 0, "failures": 0, "avg_duration_s": null}
  },
  "tracked_metrics": {}
}
JSON
SOAK_DIR="$TMP/failed" OUT_DIR="$TMP/failed-report" RUN_ID=1 RUN_ATTEMPT=1 \
	SOAK_KIND=daily DURATION_SECONDS=1800 WINDOW_SECONDS=79200 RETRY_ATTEMPT=1 \
	"$ROOT/scripts/bench/aggregate-perf-report.sh"
jq -e '
  .verdict == "regress"
  and .bootstrap == true
  and (.failures | any(test("1 passive soak iteration\\(s\\) failed")))
  and .run.shard_up_seconds == null
' "$TMP/failed-report/verdict.json" >/dev/null

# A checkpoint keeps the in_progress verdict but surfaces mid-run-valid
# failures (completed iteration failures), and the badge turns orange.
mkdir -p "$TMP/checkpoint-report"
SOAK_DIR="$TMP/failed" OUT_DIR="$TMP/checkpoint-report" RUN_ID=1 RUN_ATTEMPT=1 \
	SOAK_KIND=daily DURATION_SECONDS=1800 WINDOW_SECONDS=79200 RETRY_ATTEMPT=1 \
	SOAK_STATUS=in_progress \
	"$ROOT/scripts/bench/aggregate-perf-report.sh"
jq -e '
  .verdict == "in_progress"
  and (.failures | any(test("1 passive soak iteration\\(s\\) failed")))
  and .warnings == []
' "$TMP/checkpoint-report/verdict.json" >/dev/null
jq -e '.color == "orange" and (.message | endswith("· failing"))' \
	"$TMP/checkpoint-report/badge.json" >/dev/null

mkdir -p "$TMP/recovered" "$TMP/recovered-report"
cat >"$TMP/recovered/.soak-checkpoint-state.json" <<'JSON'
{
  "target_ref": "dev",
  "target_sha": "fedcba9876543210",
  "trigger_source": "manual",
  "slot_delay_seconds": 0,
  "version": "0.4.43",
  "started_at": 1000,
  "requested_seconds": 1800,
  "iterations": 1,
  "failures": 1,
  "bench_segments": 0,
  "bench_failures": 0
}
JSON
SOAK_DIR="$TMP/recovered" OUT_DIR="$TMP/recovered-report" RUN_ID=9 RUN_ATTEMPT=2 \
	SOAK_KIND=daily DURATION_SECONDS=1800 WINDOW_SECONDS=79200 RETRY_ATTEMPT=0 \
	SOAK_STATUS=in_progress \
	"$ROOT/scripts/bench/aggregate-perf-report.sh"
jq -e '
  .target_ref == "dev"
  and .target_sha == "fedcba9876543210"
  and .started_at == 1000
  and (.elapsed_seconds | type) == "number"
  and .elapsed_seconds >= 0
  and .iterations == 1
  and .failures == 1
' "$TMP/recovered/summary.json" >/dev/null
jq -e '
  .run.status == "in_progress"
  and .run.run_id == "9"
  and .run.run_attempt == 2
  and .run.kind == "daily"
  and .run.started_at == 1000
  and (.run.elapsed_seconds | type) == "number"
' "$TMP/recovered-report/weekly-summary.json" >/dev/null

mkdir -p "$TMP/unrecoverable" "$TMP/unrecoverable-report"
printf '%s\n' '{"started_at":null}' >"$TMP/unrecoverable/.soak-checkpoint-state.json"
if SOAK_DIR="$TMP/unrecoverable" OUT_DIR="$TMP/unrecoverable-report" \
	RUN_ID=10 RUN_ATTEMPT=1 SOAK_KIND=daily DURATION_SECONDS=1800 \
	WINDOW_SECONDS=79200 RETRY_ATTEMPT=0 SOAK_STATUS=in_progress \
	"$ROOT/scripts/bench/aggregate-perf-report.sh" >"$TMP/unrecoverable.log" 2>&1; then
	echo "checkpoint aggregation must reject absent summary and state metadata" >&2
	exit 1
fi
grep -q 'no valid summary or recoverable persisted state' "$TMP/unrecoverable.log"

mkdir -p "$TMP/passing" "$TMP/passing-report"
jq '.failures = 0 | .failure_rate = 0 | .providers.docker.failures = 0
	| .shard_up_seconds = 90' \
	"$TMP/failed/summary.json" >"$TMP/passing/summary.json"
SOAK_DIR="$TMP/passing" OUT_DIR="$TMP/passing-report" RUN_ID=2 RUN_ATTEMPT=1 \
	SOAK_KIND=daily DURATION_SECONDS=1800 WINDOW_SECONDS=79200 RETRY_ATTEMPT=0 \
	"$ROOT/scripts/bench/aggregate-perf-report.sh"
jq -e '.verdict == "pass" and .bootstrap == true and .failures == []
	and .run.shard_up_seconds == 90' \
	"$TMP/passing-report/verdict.json" >/dev/null

# A healthy checkpoint stays clean: no failures, neutral badge.
mkdir -p "$TMP/passing-checkpoint-report"
SOAK_DIR="$TMP/passing" OUT_DIR="$TMP/passing-checkpoint-report" RUN_ID=2 RUN_ATTEMPT=1 \
	SOAK_KIND=daily DURATION_SECONDS=1800 WINDOW_SECONDS=79200 RETRY_ATTEMPT=0 \
	SOAK_STATUS=in_progress \
	"$ROOT/scripts/bench/aggregate-perf-report.sh"
jq -e '.verdict == "in_progress" and .failures == []' \
	"$TMP/passing-checkpoint-report/verdict.json" >/dev/null
jq -e '.color == "lightgrey" and (.message | contains("failing") | not)' \
	"$TMP/passing-checkpoint-report/badge.json" >/dev/null

# The "no passive summary" line is a completion-only signal: a checkpoint
# before the first summary write must not paint a healthy soak red.
mkdir -p "$TMP/nodata" "$TMP/nodata-checkpoint-report"
cat >"$TMP/nodata/.soak-checkpoint-state.json" <<'JSON'
{
  "target_ref": "dev",
  "target_sha": "0123456789abcdef",
  "trigger_source": "manual",
  "slot_delay_seconds": 0,
  "version": "0.4.43",
  "started_at": 1000,
  "requested_seconds": 1800,
  "iterations": 0,
  "failures": 0,
  "bench_segments": 0,
  "bench_failures": 0
}
JSON
SOAK_DIR="$TMP/nodata" OUT_DIR="$TMP/nodata-checkpoint-report" RUN_ID=5 RUN_ATTEMPT=1 \
	SOAK_KIND=daily DURATION_SECONDS=1800 WINDOW_SECONDS=79200 RETRY_ATTEMPT=0 \
	SOAK_STATUS=in_progress \
	"$ROOT/scripts/bench/aggregate-perf-report.sh"
jq -e '.verdict == "in_progress" and .failures == []' \
	"$TMP/nodata-checkpoint-report/verdict.json" >/dev/null

mkdir -p "$TMP/segments/bench-segment-00001/bench" "$TMP/segments-report"
cp "$TMP/passing/summary.json" "$TMP/segments/summary.json"
cat >"$TMP/segments/bench-segment-00001/metrics.json" <<'JSON'
{
  "segment_index": 1,
  "offset_seconds": 60,
  "ok": true,
  "latency": {"p50_ms": 10, "p95_ms": 20},
  "observed_throughput": 2,
  "finalization_rate": 1,
  "rss_peak_mb": 100
}
JSON
cat >"$TMP/segments/bench-segment-00001/bench/metrics.json" <<'JSON'
{"segment_index": null, "offset_seconds": null, "ok": true}
JSON
SOAK_DIR="$TMP/segments" OUT_DIR="$TMP/segments-report" RUN_ID=4 RUN_ATTEMPT=1 \
	SOAK_KIND=daily DURATION_SECONDS=1800 WINDOW_SECONDS=79200 RETRY_ATTEMPT=0 \
	"$ROOT/scripts/bench/aggregate-perf-report.sh"
jq -e '
  .active.segments_total == 1
  and .active.segments_ok == 1
  and (.active.segments | length) == 1
  and .active.segments[0].offset_seconds == 60
  and .passive.cpu_peak_core_grid_pct == {"validator1": {"all": 55.5}, "bootstrap": {"all": 12}}
' "$TMP/segments-report/weekly-summary.json" >/dev/null

mkdir -p "$TMP/breach" "$TMP/breach-report"
cp "$TMP/passing/summary.json" "$TMP/breach/summary.json"
printf '%s\n' 'host_protection_breach: injected guardian marker' \
	>"$TMP/breach/early-exit.txt"
SOAK_DIR="$TMP/breach" OUT_DIR="$TMP/breach-report" RUN_ID=3 RUN_ATTEMPT=1 \
	SOAK_KIND=daily DURATION_SECONDS=1800 WINDOW_SECONDS=79200 RETRY_ATTEMPT=0 \
	"$ROOT/scripts/bench/aggregate-perf-report.sh"
jq -e '
  .verdict == "regress"
  and .run.protection_breach == true
  and (.failures | any(. == "host protection breach aborted the soak"))
' "$TMP/breach-report/verdict.json" >/dev/null

# A breach is a completed fact mid-run too: a checkpoint after the guardian
# fired reports it instead of a neutral "running".
mkdir -p "$TMP/breach-checkpoint-report"
SOAK_DIR="$TMP/breach" OUT_DIR="$TMP/breach-checkpoint-report" RUN_ID=3 RUN_ATTEMPT=1 \
	SOAK_KIND=daily DURATION_SECONDS=1800 WINDOW_SECONDS=79200 RETRY_ATTEMPT=0 \
	SOAK_STATUS=in_progress \
	"$ROOT/scripts/bench/aggregate-perf-report.sh"
jq -e '
  .verdict == "in_progress"
  and (.failures | any(. == "host protection breach aborted the soak"))
' "$TMP/breach-checkpoint-report/verdict.json" >/dev/null

for role in passive baseline thresholds; do
	for value in missing empty null whitespace truncated trailing multiple; do
		input="$TMP/json-$role-$value"
		output="$TMP/json-$role-$value-report"
		mkdir -p "$input" "$output"
		cp "$TMP/passing/summary.json" "$input/summary.json"
		cp "$TMP/passing-report/weekly-summary.json" "$input/baseline.json"
		cp "$ROOT/scripts/bench/soak-gate-thresholds.json" "$input/thresholds.json"
		case "$role" in
		passive) changed="$input/summary.json" ;;
		baseline) changed="$input/baseline.json" ;;
		thresholds) changed="$input/thresholds.json" ;;
		esac
		chmod u+w "$changed"
		case "$value" in
		missing) rm "$changed" ;;
		empty) : >"$changed" ;;
		null) printf 'null\n' >"$changed" ;;
		whitespace) printf ' \n\t\n' >"$changed" ;;
		truncated) printf '{"incomplete":' >"$changed" ;;
		trailing) printf '\ngarbage\n' >>"$changed" ;;
		multiple) printf '\n{}\n' >>"$changed" ;;
		esac
		expected_exit=0
		case "$value" in whitespace | truncated | trailing | multiple) expected_exit=1 ;; esac
		case "$role-$value" in thresholds-missing | thresholds-empty | thresholds-null) expected_exit=1 ;; esac
		actual_exit=0
		SOAK_DIR="$input" OUT_DIR="$output" BASELINE_JSON="$input/baseline.json" \
			THRESHOLDS_JSON="$input/thresholds.json" SOAK_STATUS=complete \
			RUN_ID=12 RUN_ATTEMPT=1 SOAK_KIND=daily DURATION_SECONDS=1800 \
			WINDOW_SECONDS=79200 RETRY_ATTEMPT=0 \
			"$ROOT/scripts/bench/aggregate-perf-report.sh" >"$output/stdout.txt" 2>"$output/stderr.txt" || actual_exit=$?
		if [ "$expected_exit" -eq 1 ]; then
			if [ "$actual_exit" -eq 0 ]; then
				echo "The report accepted the $role/$value input." >&2
				exit 1
			fi
		else
			[ "$actual_exit" -eq 0 ]
			expected_verdict=pass
			[ "$role" != passive ] || expected_verdict=regress
			expected_bootstrap=false
			[ "$role" != baseline ] || expected_bootstrap=true
			jq -e --arg verdict "$expected_verdict" --argjson bootstrap "$expected_bootstrap" '
				.verdict == $verdict and .bootstrap == $bootstrap
				and .failures == (if $verdict == "regress" then
					["no passive soak summary was produced (no data)"] else [] end)
			' "$output/verdict.json" >/dev/null
		fi
		printf 'JSON input control passed: %s/%s\n' "$role" "$value"
	done
done

mkdir -p "$TMP/ignored-baseline-report"
SOAK_DIR="$TMP/passing" OUT_DIR="$TMP/ignored-baseline-report" \
	BASELINE_JSON="$TMP/json-baseline-truncated/baseline.json" SOAK_STATUS=in_progress \
	RUN_ID=13 RUN_ATTEMPT=1 SOAK_KIND=daily DURATION_SECONDS=1800 \
	WINDOW_SECONDS=79200 RETRY_ATTEMPT=0 \
	"$ROOT/scripts/bench/aggregate-perf-report.sh"
jq -e '.verdict == "in_progress" and .bootstrap == false
	and .baseline_run == null and .failures == [] and .warnings == []' \
	"$TMP/ignored-baseline-report/verdict.json" >/dev/null

transport_failures=0
for alias_input in passive baseline thresholds; do
	input="$TMP/alias-$alias_input"
	output="$TMP/alias-$alias_input-report"
	mkdir -p "$input" "$output"
	cp "$TMP/passing/summary.json" "$input/summary.json"
	baseline="$input/baseline.json"
	thresholds="$input/thresholds.json"
	case "$alias_input" in
	passive)
		mv "$input/summary.json" "$output/weekly-summary.json"
		ln -s "$output/weekly-summary.json" "$input/summary.json"
		;;
	baseline) baseline="$output/weekly-summary.json" ;;
	thresholds) thresholds="$output/verdict.json" ;;
	esac
	jq '.passive.rss_peak_mb = 10000' "$TMP/passing-report/weekly-summary.json" >"$baseline"
	cp "$ROOT/scripts/bench/soak-gate-thresholds.json" "$thresholds"
	chmod u+w "$thresholds"
	if ! SOAK_DIR="$input" OUT_DIR="$output" BASELINE_JSON="$baseline" \
		THRESHOLDS_JSON="$thresholds" SOAK_STATUS=complete \
		RUN_ID=14 RUN_ATTEMPT=1 SOAK_KIND=daily DURATION_SECONDS=1800 \
		WINDOW_SECONDS=79200 RETRY_ATTEMPT=0 \
		"$ROOT/scripts/bench/aggregate-perf-report.sh" >"$output/stdout.txt" 2>"$output/stderr.txt"; then
		echo "Report generation failed for the $alias_input output alias." >&2
		transport_failures=$((transport_failures + 1))
		continue
	fi
	if ! jq -e '.verdict == "regress" and .bootstrap == false
		and .failures == ["peak RSS 17100MB > baseline 10000MB +20%"]
		and .warnings == []' "$output/verdict.json" >/dev/null; then
		echo "The report lost the $alias_input input through an output alias." >&2
		transport_failures=$((transport_failures + 1))
		continue
	fi
	grep -q '^| peak RSS (MB) | 17100 | 10000 |$' "$output/perf-report.md"
	printf 'Output alias control passed: %s\n' "$alias_input"
done

for transport in small passive active baseline thresholds; do
	input="$TMP/transport-$transport"
	output="$TMP/transport-$transport-report"
	mkdir -p "$input/bench-segment-00001" "$output"
	cp "$TMP/failed/summary.json" "$input/summary.json"
	cp "$TMP/segments/bench-segment-00001/metrics.json" "$input/bench-segment-00001/metrics.json"
	cp "$TMP/passing-report/weekly-summary.json" "$input/baseline.json"
	cp "$ROOT/scripts/bench/soak-gate-thresholds.json" "$input/thresholds.json"
	case "$transport" in
	passive)
		large_input="$input/summary.json"
		padding_path='["tracked_metrics", "transport_probe"]'
		;;
	active)
		large_input="$input/bench-segment-00001/metrics.json"
		padding_path='["transport_probe"]'
		;;
	baseline)
		large_input="$input/baseline.json"
		padding_path='["run", "transport_probe"]'
		;;
	thresholds)
		large_input="$input/thresholds.json"
		padding_path='["comment"]'
		;;
	esac
	if [ "$transport" != small ]; then
		jq --argjson path "$padding_path" 'setpath($path; "x" * 1048576)' \
			"$large_input" >"$input/large.json"
		mv "$input/large.json" "$large_input"
		[ "$(wc -c <"$large_input")" -gt 1048576 ]
	fi
	if ! SOAK_DIR="$input" OUT_DIR="$output" BASELINE_JSON="$input/baseline.json" \
		THRESHOLDS_JSON="$input/thresholds.json" SOAK_STATUS=complete \
		RUN_ID=11 RUN_ATTEMPT=1 SOAK_KIND=daily DURATION_SECONDS=1800 \
		WINDOW_SECONDS=79200 RETRY_ATTEMPT=0 \
		"$ROOT/scripts/bench/aggregate-perf-report.sh" >"$output/stdout.txt" 2>"$output/stderr.txt"; then
		echo "Report generation failed for the $transport JSON transport case." >&2
		cat "$output/stderr.txt" >&2
		transport_failures=$((transport_failures + 1))
		continue
	fi
	jq -e --slurpfile passive "$input/summary.json" \
		--slurpfile segment "$input/bench-segment-00001/metrics.json" '
		.passive.tracked_metrics == $passive[0].tracked_metrics
		and .passive.providers == $passive[0].providers
		and .passive.iterations == 1 and .passive.failures == 1
		and .active.segments == $segment
		and .active.segments_total == 1 and .active.segments_ok == 1
		and .active.p50_ms == 10 and .active.p95_ms == 20
		and .active.throughput == 2 and .active.finalization_rate == 1
	' "$output/weekly-summary.json" >/dev/null
	jq -e --slurpfile baseline "$input/baseline.json" \
		--slurpfile thresholds "$input/thresholds.json" '
		.verdict == "regress" and .bootstrap == false
		and .failures == ["1 passive soak iteration(s) failed",
			"failure rate 1 exceeds baseline 0 by more than 5pts"]
		and .warnings == [] and .baseline_run == $baseline[0].run
		and .thresholds == $thresholds[0]
	' "$output/verdict.json" >/dev/null
	jq -e '.message == "regress" and .color == "red"' "$output/badge.json" >/dev/null
	jq -e '.message == "0% · 1 iters" and .color == "orange"' "$output/badge-stability.json" >/dev/null
	jq -e '.message == "36.0/h" and .color == "blue"' "$output/badge-perf.json" >/dev/null
	grep -q '^## Verdict: REGRESS$' "$output/perf-report.md"
	grep -q '^- FAIL: 1 passive soak iteration(s) failed$' "$output/perf-report.md"
	grep -q '^| iterations | 1 | 1 |$' "$output/perf-report.md"
	grep -q '^| 1 | 0 | 10 | 20 | 2 | 0/0 | 100 | true |$' "$output/perf-report.md"
	printf 'JSON transport case passed: %s\n' "$transport"
done
[ "$transport_failures" -eq 0 ]

printf 'soak report verdict tests passed\n'
