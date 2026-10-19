#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$SCRIPT_DIR/summarize-issue24-phases.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fail() {
	echo "FAIL: $*" >&2
	exit 1
}

metrics() {
	local created="$1" empty="$2" age="$3"
	printf 'ISSUE24_METRICS {"counters": {"block_creator_empty_block_built": {"delta": %s, "unit": "events"}, "heartbeat_proposals": {"delta": %s, "unit": "events"}}, "histograms": {"deploy_selection_age_time": {"mean": %s, "samples": 50.0, "unit": "seconds"}, "proposer_create_time": {"mean": 0.7, "samples": %s, "unit": "seconds"}}}\n' \
		"$empty" "$created" "$age" "$created"
}

phase() {
	local name="$1" p95="$2" unfinalized="$3"
	printf '  Phase %s: inclusion p50=4.0s p95=%ss, finalization p50=20.0s p95=30.0s, LFB #10->#20 (12.5 blk/min), unfinalized=%s\n' \
		"$name" "$p95" "$unfinalized"
}

{
	echo "DockerProvider: starting session aaaa"
	phase low 6.0 0
	echo "  Node internals (validator1):"
	metrics 5.0 1.0 1.0
	phase high 11.5 0
	echo "  Node internals (validator1):"
	metrics 6.0 4.0 3.0
	echo "  Node internals (validator2):"
	metrics 6.0 2.0 3.5
	echo "PASSED                                                                   [100%]"
	echo "INFO     root:test_load.py:518   Phase high: inclusion p50=4.0s p95=1.0s, finalization p50=1.0s p95=1.0s, LFB #1->#2 (1.0 blk/min), unfinalized=0"
	echo "INFO     root:test_load.py:531   Node internals (validator1):"
	metrics 99.0 99.0 99.0
	echo "Subprocess genesis written to /data/genesis"
	phase high 6.5 0
	echo "  Node internals (validator1):"
	metrics 8.0 2.0 1.5
	echo "  Node internals (validator2):"
	echo "ISSUE24_METRICS {not json"
	phase low 6.0 0
	echo "Subprocess genesis written to /data/genesis"
	phase high 12.0 7
	echo "  Node internals (validator1):"
	metrics 7.0 5.0 3.2
	echo "FAILED                                                                   [100%]"
} | sed 's/^/2026-10-18T17:29:48.9670367Z /' >"$tmp/job.log"

"$tool" records "$tmp/job.log" >"$tmp/records.jsonl"

[ "$(wc -l <"$tmp/records.jsonl" | tr -d ' ')" = 5 ] || fail "expected 5 records without the captured-log copy and the malformed line"
[ "$(jq -r 'select(.phase == "high") | "\(.session):\(.provider):\(.validator)"' "$tmp/records.jsonl" | paste -sd, -)" = \
	"1:docker:validator1,1:docker:validator2,2:subprocess:validator1,3:subprocess:validator1" ] ||
	fail "sessions, providers, or validators are wrong"
jq -e 'select(.session == 3 and .phase == "high") | .unfinalized == 7 and .inclusion_p95 == 12 and .lfb_rate == 12.5' "$tmp/records.jsonl" >/dev/null ||
	fail "the phase line fields are not parsed"
jq -e 'select(.session == 1 and .phase == "high" and .validator == "validator1") | .counters.block_creator_empty_block_built == 4 and .samples.proposer_create_time == 6 and .histograms.deploy_selection_age_time == 3' "$tmp/records.jsonl" >/dev/null ||
	fail "the metric values are not flattened"
[ "$(jq -r .time "$tmp/records.jsonl" | sort -u)" = 2026-10-18T17:29:48.9670367Z ] || fail "the timestamp is not kept"

"$tool" records --provider docker "$tmp/job.log" >"$tmp/forced.jsonl"
[ "$(jq -r '.provider' "$tmp/forced.jsonl" | sort -u)" = docker ] || fail "--provider does not override the provider"

"$tool" records --session 41 "$tmp/job.log" >"$tmp/offset.jsonl"
[ "$(jq -r .session "$tmp/offset.jsonl" | sort -un | paste -sd, -)" = 41,42,43 ] || fail "--session does not set the first session"
if "$tool" records --session x "$tmp/job.log" >/dev/null 2>&1; then
	fail "a session that is not a number must be rejected"
fi

"$tool" compare "$tmp/records.jsonl" >"$tmp/compare.md"
grep -qxF '| docker | 1 | 0 | 1 | 0 |' "$tmp/compare.md" || fail "the docker slow share is wrong"
grep -qxF '| subprocess | 2 | 1 | 0 | 1 |' "$tmp/compare.md" || fail "the subprocess slow share is wrong"
grep -qxF '| Metric | fast (1) | slow (2) | failing (1) |' "$tmp/compare.md" || fail "the group sizes are wrong"
grep -qxF '| Empty blocks built | 2 | 3 | 5 |' "$tmp/compare.md" || fail "the empty-block means are wrong"
grep -qxF '| Deploy selection age (s) | 1.5 | 3.25 | 3.2 |' "$tmp/compare.md" || fail "the selection-age means are wrong"
grep -qxF '| Finalizer timeouts | - | - | - |' "$tmp/compare.md" || fail "a missing metric is not shown as -"

"$tool" compare --phase low --slow-inclusion-p95 5 "$tmp/records.jsonl" >"$tmp/low.md"
grep -qxF '| docker | 1 | 0 | 1 | 0 |' "$tmp/low.md" || fail "--phase or --slow-inclusion-p95 is not applied"

if "$tool" compare --phase burst "$tmp/records.jsonl" >/dev/null 2>"$tmp/err"; then
	fail "a phase without records must fail"
fi
grep -q 'No burst phases' "$tmp/err" || fail "the missing-phase error is not reported"

if "$tool" records --provider other "$tmp/job.log" >/dev/null 2>&1; then
	fail "an unknown provider must be rejected"
fi

phase high 5.0 0 | sed 's/$/ "quoted"/' >"$tmp/a=b.log"
printf '  Node internals (validator1):\n' >>"$tmp/a=b.log"
metrics 4.0 1.0 1.0 >>"$tmp/a=b.log"
"$tool" records "$tmp/a=b.log" >"$tmp/quoted.jsonl"
[ "$(wc -l <"$tmp/quoted.jsonl" | tr -d ' ')" = 1 ] || fail "a quote in the phase line or an = in the file name drops the record"

sed 's/LFB #10->#20/LFB #30->#40/' "$tmp/a=b.log" >"$tmp/second.log"
{ "$tool" records "$tmp/a=b.log"; "$tool" records "$tmp/second.log"; } >"$tmp/two.jsonl"
grep -qxF '| docker | 2 | 2 | 0 | 0 |' <("$tool" compare "$tmp/two.jsonl") || fail "two logs with the same session number merge into one phase"

if "$tool" compare --slow-inclusion-p95 abc "$tmp/records.jsonl" >/dev/null 2>&1; then
	fail "a threshold that is not a number must be rejected"
fi
if "$tool" records "$tmp/missing.log" >/dev/null 2>&1; then
	fail "a missing log must be rejected"
fi

echo "summarize-issue24-phases: all checks passed"
