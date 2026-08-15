#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
SYSTEM_INTEGRATION_DIR=${SYSTEM_INTEGRATION_DIR:?SYSTEM_INTEGRATION_DIR is required}
NODE_REPO_DIR=${NODE_REPO_DIR:?NODE_REPO_DIR is required}
PROFILE_FILE=${PREFLIGHT_PROFILE_FILE:-$SYSTEM_INTEGRATION_DIR/integration-tests/test/full-suite.txt}
OUTPUT_DIR=${PREFLIGHT_OUTPUT_DIR:-/tmp/merge-recovery-soak/preflight}
PROVIDER=${PREFLIGHT_PROVIDER:-docker}
TIMEOUT_SECONDS=${PREFLIGHT_TIMEOUT_SECONDS:-10800}
RSS_CEILING_MB=${PREFLIGHT_RSS_CEILING_MB:-45056}
HOST_FREE_FLOOR_MB=${PREFLIGHT_HOST_FREE_FLOOR_MB:-8192}
CAPABILITY_READER=$ROOT/.github/scripts/read-system-integration-node-capabilities.sh
CAPABILITIES_FILE=$NODE_REPO_DIR/.github/system-integration-node-capabilities.txt

case "$PROVIDER" in
	docker | subprocess) ;;
	*) printf 'unsupported preflight provider: %s\n' "$PROVIDER" >&2; exit 2 ;;
esac
for value_name in TIMEOUT_SECONDS RSS_CEILING_MB HOST_FREE_FLOOR_MB; do
	value=${!value_name}
	if ! [[ "$value" =~ ^[0-9]+$ ]] || { [ "$value_name" = TIMEOUT_SECONDS ] && [ "$value" -eq 0 ]; }; then
		printf '%s must be a valid integer\n' "$value_name" >&2
		exit 2
	fi
done
[ -f "$PROFILE_FILE" ] || { printf 'preflight profile not found: %s\n' "$PROFILE_FILE" >&2; exit 2; }
[ -d "$SYSTEM_INTEGRATION_DIR" ] || { printf 'system-integration directory not found: %s\n' "$SYSTEM_INTEGRATION_DIR" >&2; exit 2; }
[ -x "$CAPABILITY_READER" ] || { printf 'capability reader is not executable: %s\n' "$CAPABILITY_READER" >&2; exit 2; }

mkdir -p "$OUTPUT_DIR"
EXPECTED_TESTS=(
	integration-tests/test/tests/shared/
	integration-tests/test/tests/custom/
	integration-tests/test/tests/standalone/
)
TESTS=()
seen='|'
while IFS= read -r selector || [ -n "$selector" ]; do
	selector=${selector%$'\r'}
	[ -n "$selector" ] || continue
	if [[ ! "$selector" =~ ^integration-tests/test/tests/(shared|custom|standalone)/$ ]]; then
		printf 'invalid full-suite selector: %s\n' "$selector" >&2
		exit 2
	fi
	case "$seen" in
	*"|$selector|"*) printf 'duplicate full-suite selector: %s\n' "$selector" >&2; exit 2 ;;
	esac
	[ -d "$SYSTEM_INTEGRATION_DIR/$selector" ] || { printf 'full-suite target not found: %s\n' "$selector" >&2; exit 2; }
	seen="${seen}${selector}|"
	TESTS+=("$selector")
done <"$PROFILE_FILE"
if [ "${TESTS[*]}" != "${EXPECTED_TESTS[*]}" ]; then
	printf 'preflight profile must select the complete shared, custom, and standalone suites\n' >&2
	exit 2
fi
printf '%s\n' "${TESTS[@]}" >"$OUTPUT_DIR/selection.txt"

NODE_CAPABILITIES=$("$CAPABILITY_READER" "$CAPABILITIES_FILE")
NODE_CAPABILITY_ARGS=()
while IFS= read -r capability; do
	[ -z "$capability" ] || NODE_CAPABILITY_ARGS+=("--node-capability=$capability")
done <<<"$NODE_CAPABILITIES"

COLLECTION_LOG=$OUTPUT_DIR/collection.log
JUNIT_XML=$OUTPUT_DIR/junit.xml
PYTEST_LOG=$OUTPUT_DIR/pytest.log
REPORT=$OUTPUT_DIR/report.txt
rm -f "$COLLECTION_LOG" "$JUNIT_XML" "$PYTEST_LOG" "$REPORT"
set +e
(
	cd "$SYSTEM_INTEGRATION_DIR"
	poetry run pytest \
		"${TESTS[@]}" \
		--provider="$PROVIDER" \
		"${NODE_CAPABILITY_ARGS[@]}" \
		--run-all-node-capability-tests \
		--collect-only -q
) 2>&1 | tee "$COLLECTION_LOG"
collection_status=${PIPESTATUS[0]}
set -e
collected_tests=$(grep -Ec '^integration-tests/test/tests/(shared|custom|standalone)/.+::' "$COLLECTION_LOG" || true)
if [ "$collection_status" -ne 0 ] || [ "$collected_tests" -eq 0 ]; then
	printf 'tests=0 collected=%s failures=0 errors=1 skipped=0\n' "$collected_tests" >"$REPORT"
	printf 'collection_status=%s\n' "$collection_status" >>"$REPORT"
	printf 'failed\n' >"$OUTPUT_DIR/result"
	printf '## Integration preflight\n\n- Result: `failed`\n- Provider: `%s`\n- Collected tests: `%s`\n- Report: `%s`\n' \
		"$PROVIDER" "$collected_tests" "$(head -1 "$REPORT")" >"$OUTPUT_DIR/summary.md"
	exit 1
fi

started=$(date +%s)
set +e
(
	cd "$SYSTEM_INTEGRATION_DIR"
	timeout --signal=TERM --kill-after=30 "${TIMEOUT_SECONDS}s" \
		poetry run pytest \
		"${TESTS[@]}" \
		--provider="$PROVIDER" \
		"${NODE_CAPABILITY_ARGS[@]}" \
		--run-all-node-capability-tests \
		--monitor \
		--rss-ceiling-mb "$RSS_CEILING_MB" \
		--host-free-floor-mb "$HOST_FREE_FLOOR_MB" \
		-v --tb=short --instafail --maxfail=20 \
		-n 1 --dist=loadgroup --timeout=1200 \
		--junitxml="$JUNIT_XML"
) 2>&1 | tee "$PYTEST_LOG"
pytest_status=${PIPESTATUS[0]}
set -e
elapsed=$(( $(date +%s) - started ))

set +e
python3 - "$JUNIT_XML" "$collected_tests" >"$REPORT" <<'PY'
import pathlib
import sys
import xml.etree.ElementTree as ET

path = pathlib.Path(sys.argv[1])
expected_tests = int(sys.argv[2])
if not path.is_file():
    print(f"tests=0 collected={expected_tests} failures=0 errors=1 skipped=0")
    raise SystemExit(1)
root = ET.parse(path).getroot()
cases = root.findall(".//testcase")
tests = len(cases)
failures = sum(case.find("failure") is not None for case in cases)
errors = sum(case.find("error") is not None for case in cases)
skipped = sum(case.find("skipped") is not None for case in cases)
print(
    f"tests={tests} collected={expected_tests} failures={failures} "
    f"errors={errors} skipped={skipped}"
)
raise SystemExit(
    0
    if tests == expected_tests and failures == 0 and errors == 0 and skipped == 0
    else 1
)
PY
report_status=$?
set -e
cat "$REPORT"
printf 'provider=%s suite_roots=%s elapsed_seconds=%s pytest_status=%s collection_status=%s\n' \
	"$PROVIDER" "${#TESTS[@]}" "$elapsed" "$pytest_status" "$collection_status" >>"$REPORT"

result=passed
if [ "$pytest_status" -ne 0 ] || [ "$report_status" -ne 0 ]; then
	result=failed
fi
printf '%s\n' "$result" >"$OUTPUT_DIR/result"
printf '## Integration preflight\n\n- Result: `%s`\n- Provider: `%s`\n- Suite roots: `%s`\n- Collected tests: `%s`\n- Elapsed seconds: `%s`\n- Report: `%s`\n' \
	"$result" "$PROVIDER" "${#TESTS[@]}" "$collected_tests" "$elapsed" "$(head -1 "$REPORT")" >"$OUTPUT_DIR/summary.md"
[ "$result" = passed ]
