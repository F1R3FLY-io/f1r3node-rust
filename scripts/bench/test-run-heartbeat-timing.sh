#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/bin" "$TMP/node/.github" "$TMP/harness/integration-tests/test/tests/custom"
printf '%s\n' test-capability >"$TMP/node/.github/system-integration-node-capabilities.txt"
touch "$TMP/harness/integration-tests/test/tests/custom/test_heartbeat_timing.py"
cat >"$TMP/bin/poetry" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
[ "$HEARTBEAT_TIMING" = 1 ]
[ "$HEARTBEAT_TIMING_REPS" = 3 ]
printf '%s\n' "$@" >"$FAKE_ARGS"
for arg in "$@"; do
	case "$arg" in --junitxml=*) xml=${arg#*=} ;; esac
done
printf 'timing fixture output\n'
case "$FAKE_MODE" in
	pass) printf '<testsuites><testsuite><testcase name="timing"/></testsuite></testsuites>\n' >"$xml" ;;
	skip) printf '<testsuites><testsuite><testcase name="timing"><skipped/></testcase></testsuite></testsuites>\n' >"$xml" ;;
	empty) printf '<testsuites><testsuite/></testsuites>\n' >"$xml" ;;
	missing) ;;
	fail) exit 1 ;;
	*) exit 2 ;;
esac
SH
cat >"$TMP/bin/timeout" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
[ "$1" = --signal=TERM ]
[ "$2" = --kill-after=30 ]
[ "$3" = 2730 ]
shift 3
exec "$@"
SH
chmod +x "$TMP/bin/poetry" "$TMP/bin/timeout"
run_case() {
	PATH="$TMP/bin:$PATH" SYSTEM_INTEGRATION_DIR="$TMP/harness" NODE_REPO_DIR="$TMP/node" \
		HEARTBEAT_TIMING=0 HEARTBEAT_TIMING_REPS="${2:-3}" HEARTBEAT_TIMING_OUTPUT_DIR="$TMP/$1" \
		F1R3FLY_NODE_IMAGE=fixture-image FAKE_ARGS="$TMP/args" FAKE_MODE="$1" \
		bash "$ROOT/scripts/bench/run-heartbeat-timing.sh"
}
run_case pass
jq -e '.repetitions==3 and .timeout_seconds==2700 and .exit_code==0 and .release_gate==false' "$TMP/pass/report.json" >/dev/null
grep -Fxq integration-tests/test/tests/custom/test_heartbeat_timing.py "$TMP/args"
grep -Fxq -- --node-capability=test-capability "$TMP/args"
grep -Fxq -- --timeout=2700 "$TMP/args"
grep -A1 -Fx -- -n "$TMP/args" | tail -1 | grep -Fxq 0
for mode in skip empty missing fail; do
	if run_case "$mode"; then
		printf 'The benchmark accepted %s.\n' "$mode" >&2
		exit 1
	fi
	jq -e '.exit_code!=0' "$TMP/$mode/report.json" >/dev/null
	test -s "$TMP/$mode/pytest.log"
done
for repetitions in 0 21 invalid 01; do
	if run_case invalid "$repetitions"; then
		printf 'The benchmark accepted invalid repetitions.\n' >&2
		exit 1
	fi
done
awk '
    /^      - name: Resolve benchmark inputs$/ { step=1; next }
    step && /^        run: \|$/ { body=1; next }
    body && /^  [a-z_]+:$/ { exit }
    body { sub(/^          /, ""); print }
' "$ROOT/.github/workflows/heartbeat-timing.yml" >"$TMP/resolve.sh"
mkdir -p "$TMP/control/.github"
printf 'SYSTEM_INTEGRATION_REF=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n' >"$TMP/control/.github/oci-validation.env"
cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" >"$FAKE_GH_ARGS"
printf '%s\n' "$FAKE_TARGET_SHA"
SH
chmod +x "$TMP/bin/gh"
resolve_case() {
	(cd "$TMP/control" && PATH="$TMP/bin:$PATH" GITHUB_REPOSITORY=example/node \
		TARGET_REF=feature/heartbeat REPETITIONS="$1" GITHUB_OUTPUT="$TMP/resolved" \
		FAKE_GH_ARGS="$TMP/gh-args" FAKE_TARGET_SHA="${2:-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}" \
		bash -euo pipefail "$TMP/resolve.sh")
}
resolve_case 20
grep -Fxq 'target_sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' "$TMP/resolved"
grep -Fxq 'harness_sha=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' "$TMP/resolved"
grep -Fxq 'repetitions=20' "$TMP/resolved"
grep -Fxq 'repos/example/node/commits/feature%2Fheartbeat' "$TMP/gh-args"
if resolve_case 21; then
	printf 'The workflow accepted invalid repetitions.\n' >&2
	exit 1
fi
if resolve_case 20 invalid-revision; then
	printf 'The workflow accepted an invalid node revision.\n' >&2
	exit 1
fi
bash "$ROOT/.github/scripts/test-ephemeral-run-tagging.sh" "$ROOT/.github/workflows/heartbeat-timing.yml"
printf 'Heartbeat timing driver checks passed.\n'
