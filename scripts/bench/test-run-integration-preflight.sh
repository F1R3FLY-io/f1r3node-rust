#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
RUNNER=$ROOT/scripts/run-integration-preflight.sh
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p \
	"$TMP/bin" \
	"$TMP/node/.github" \
	"$TMP/harness/integration-tests/test/tests/custom" \
	"$TMP/harness/integration-tests/test/tests/shared"
printf '%s\n' test-capability >"$TMP/node/.github/system-integration-node-capabilities.txt"
touch \
	"$TMP/harness/integration-tests/test/tests/custom/test_alpha.py" \
	"$TMP/harness/integration-tests/test/tests/shared/test_beta.py"
printf '%s\n' \
	integration-tests/test/tests/custom/test_alpha.py \
	integration-tests/test/tests/shared/test_beta.py::test_one >"$TMP/profile"

cat >"$TMP/bin/poetry" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
xml=
for arg in "$@"; do
	case "$arg" in --junitxml=*) xml=${arg#*=} ;; esac
done
[ -n "$xml" ]
case "${FAKE_MODE:-pass}" in
	pass) body='<testcase name="one"/><testcase name="two"/>'; status=0 ;;
	skip) body='<testcase name="one"><skipped/></testcase>'; status=0 ;;
	short) body='<testcase name="one"/>'; status=0 ;;
	fail) body='<testcase name="one"><failure/></testcase>'; status=1 ;;
	no-report) exit 0 ;;
	*) exit 2 ;;
esac
printf '<testsuites><testsuite>%s</testsuite></testsuites>\n' "$body" >"$xml"
exit "$status"
EOF
chmod +x "$TMP/bin/poetry"

run_preflight() {
	PATH="$TMP/bin:$PATH" \
	SYSTEM_INTEGRATION_DIR="$TMP/harness" \
	NODE_REPO_DIR="$TMP/node" \
	PREFLIGHT_PROFILE_FILE="$1" \
	PREFLIGHT_OUTPUT_DIR="$2" \
	PREFLIGHT_TIMEOUT_SECONDS=30 \
	FAKE_MODE="${3:-pass}" \
		"$RUNNER"
}

run_preflight "$TMP/profile" "$TMP/pass"
grep -Fq 'tests=2 failures=0 errors=0 skipped=0' "$TMP/pass/report.txt"
test "$(cat "$TMP/pass/result")" = passed

if run_preflight "$TMP/profile" "$TMP/skip" skip; then
	printf 'preflight accepted a skipped test\n' >&2
	exit 1
fi
test "$(cat "$TMP/skip/result")" = failed

if run_preflight "$TMP/profile" "$TMP/short" short; then
	printf 'preflight accepted fewer tests than selectors\n' >&2
	exit 1
fi
test "$(cat "$TMP/short/result")" = failed

if run_preflight "$TMP/profile" "$TMP/fail" fail; then
	printf 'preflight accepted a pytest failure\n' >&2
	exit 1
fi
test "$(cat "$TMP/fail/result")" = failed

if run_preflight "$TMP/profile" "$TMP/no-report" no-report; then
	printf 'preflight accepted a missing JUnit report\n' >&2
	exit 1
fi
test "$(cat "$TMP/no-report/result")" = failed

printf '%s\n' \
	integration-tests/test/tests/custom/test_alpha.py \
	integration-tests/test/tests/custom/test_alpha.py >"$TMP/duplicate-profile"
if run_preflight "$TMP/duplicate-profile" "$TMP/duplicate"; then
	printf 'preflight accepted a duplicate selector\n' >&2
	exit 1
fi

printf '%s\n' integration-tests/test/tests/custom/test_missing.py >"$TMP/missing-profile"
if run_preflight "$TMP/missing-profile" "$TMP/missing"; then
	printf 'preflight accepted a missing target\n' >&2
	exit 1
fi

printf '%s\n' ../test_alpha.py >"$TMP/invalid-profile"
if run_preflight "$TMP/invalid-profile" "$TMP/invalid"; then
	printf 'preflight accepted an invalid selector\n' >&2
	exit 1
fi

printf 'integration preflight checks passed\n'
