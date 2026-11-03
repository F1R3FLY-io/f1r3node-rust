#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
SYSTEM_INTEGRATION_DIR=${SYSTEM_INTEGRATION_DIR:?SYSTEM_INTEGRATION_DIR is required}
NODE_REPO_DIR=${NODE_REPO_DIR:?NODE_REPO_DIR is required}
OUTPUT_DIR=${HEARTBEAT_TIMING_OUTPUT_DIR:?HEARTBEAT_TIMING_OUTPUT_DIR is required}
: "${F1R3FLY_NODE_IMAGE:?F1R3FLY_NODE_IMAGE is required}"
repetitions=${HEARTBEAT_TIMING_REPS:-20}
selector=integration-tests/test/tests/custom/test_heartbeat_timing.py

if ! [[ "$repetitions" =~ ^([1-9]|1[0-9]|20)$ ]]; then
	printf 'HEARTBEAT_TIMING_REPS must be an integer from 1 to 20.\n' >&2
	exit 2
fi
[ -f "$SYSTEM_INTEGRATION_DIR/$selector" ] || { printf 'The pinned harness has no heartbeat timing test.\n' >&2; exit 2; }
mkdir -p "$OUTPUT_DIR"
OUTPUT_DIR=$(cd "$OUTPUT_DIR" && pwd -P)
[ ! -e "$OUTPUT_DIR/junit.xml" ] || { printf 'Use a new benchmark output directory.\n' >&2; exit 2; }
node_capabilities=$(bash "$ROOT/.github/scripts/read-system-integration-node-capabilities.sh" "$NODE_REPO_DIR/.github/system-integration-node-capabilities.txt")
capabilities=()
while IFS= read -r capability; do
	[ -z "$capability" ] || capabilities+=("--node-capability=$capability")
done <<<"$node_capabilities"

status=0
(
	cd "$SYSTEM_INTEGRATION_DIR"
	HEARTBEAT_TIMING=1 HEARTBEAT_TIMING_REPS="$repetitions" \
		timeout --signal=TERM --kill-after=30 2730 \
		poetry run pytest "$selector" --provider=docker "${capabilities[@]}" \
		--monitor --rss-ceiling-mb 45056 --host-free-floor-mb 8192 \
		-v --tb=short --instafail -o log_cli=true -o log_cli_level=INFO -o 'log_cli_format=%(message)s' \
		-n 0 --timeout=2700 --junitxml="$OUTPUT_DIR/junit.xml"
) >"$OUTPUT_DIR/pytest.log" 2>&1 || status=$?
cat "$OUTPUT_DIR/pytest.log"
if [ "$status" -eq 0 ]; then
	if [ ! -s "$OUTPUT_DIR/junit.xml" ]; then
		status=1
	elif grep -Eq '<(skipped|failure|error)([[:space:]/>])' "$OUTPUT_DIR/junit.xml"; then
		status=1
	elif [ "$(grep -o '<testcase[[:space:]>]' "$OUTPUT_DIR/junit.xml" | wc -l | tr -d ' ')" -ne 1 ]; then
		status=1
	fi
fi
jq -n --argjson repetitions "$repetitions" --argjson status "$status" \
	--arg image "${F1R3FLY_NODE_IMAGE:?F1R3FLY_NODE_IMAGE is required}" \
	'{schema:"heartbeat-timing/v1",repetitions:$repetitions,warmup:1,provider:"docker",
	  timeout_seconds:2700,image:$image,exit_code:$status,release_gate:false}' >"$OUTPUT_DIR/report.json"
printf 'Heartbeat timing benchmark exit status: %s\n' "$status"
exit "$status"
