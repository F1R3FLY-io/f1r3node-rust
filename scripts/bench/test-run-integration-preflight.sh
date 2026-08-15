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
	"$TMP/harness/integration-tests/test/tests/shared" \
	"$TMP/harness/integration-tests/test/tests/standalone"
printf '%s\n' test-capability >"$TMP/node/.github/system-integration-node-capabilities.txt"
touch \
	"$TMP/harness/integration-tests/test/tests/custom/test_alpha.py" \
	"$TMP/harness/integration-tests/test/tests/shared/test_beta.py" \
	"$TMP/harness/integration-tests/test/tests/standalone/test_gamma.py"
printf '%s\n' \
	integration-tests/test/tests/shared/ \
	integration-tests/test/tests/custom/ \
	integration-tests/test/tests/standalone/ >"$TMP/profile"

cat >"$TMP/bin/poetry" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
xml=
collect=false
for arg in "$@"; do
	case "$arg" in
	--junitxml=*) xml=${arg#*=} ;;
	--collect-only) collect=true ;;
	esac
done
if [ "$collect" = true ]; then
	case "${FAKE_MODE:-pass}" in
		collection-fail) exit 1 ;;
		collection-empty) exit 0 ;;
		*)
			printf '%s\n' \
				'integration-tests/test/tests/shared/test_beta.py::test_one' \
				'integration-tests/test/tests/custom/test_alpha.py::test_two'
			exit 0
			;;
	esac
fi
[ -n "$xml" ]
case "${FAKE_MODE:-pass}" in
	pass) body='<testcase name="one"/><testcase name="two"/>'; status=0 ;;
	skip) body='<testcase name="one"><skipped/></testcase><testcase name="two"/>'; status=0 ;;
	short) body='<testcase name="one"/>'; status=0 ;;
	fail) body='<testcase name="one"><failure/></testcase><testcase name="two"/>'; status=1 ;;
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
grep -Fq 'tests=2 collected=2 failures=0 errors=0 skipped=0' "$TMP/pass/report.txt"
test "$(cat "$TMP/pass/result")" = passed
test "$(cat "$TMP/pass/selection.txt")" = "$(cat "$TMP/profile")"

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

if run_preflight "$TMP/profile" "$TMP/collection-fail" collection-fail; then
	printf 'preflight accepted a collection failure\n' >&2
	exit 1
fi
test "$(cat "$TMP/collection-fail/result")" = failed

if run_preflight "$TMP/profile" "$TMP/collection-empty" collection-empty; then
	printf 'preflight accepted an empty collection\n' >&2
	exit 1
fi
test "$(cat "$TMP/collection-empty/result")" = failed

printf '%s\n' \
	integration-tests/test/tests/shared/ \
	integration-tests/test/tests/custom/ \
	integration-tests/test/tests/custom/ >"$TMP/duplicate-profile"
if run_preflight "$TMP/duplicate-profile" "$TMP/duplicate"; then
	printf 'preflight accepted a duplicate suite root\n' >&2
	exit 1
fi

printf '%s\n' \
	integration-tests/test/tests/shared/ \
	integration-tests/test/tests/custom/ >"$TMP/incomplete-profile"
if run_preflight "$TMP/incomplete-profile" "$TMP/incomplete"; then
	printf 'preflight accepted an incomplete suite profile\n' >&2
	exit 1
fi

printf '%s\n' integration-tests/test/tests/custom/test_alpha.py >"$TMP/invalid-profile"
if run_preflight "$TMP/invalid-profile" "$TMP/invalid"; then
	printf 'preflight accepted a file selector instead of the full suite\n' >&2
	exit 1
fi

printf 'integration preflight checks passed\n'
