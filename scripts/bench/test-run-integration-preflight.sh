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
selectors=()
for arg in "$@"; do
	case "$arg" in
	--junitxml=*) xml=${arg#*=} ;;
	--collect-only) collect=true ;;
	integration-tests/test/tests/*/) selectors+=("$arg") ;;
	esac
done
if [ "$collect" = true ]; then
	case "${FAKE_MODE:-pass}" in
		collection-fail) exit 1 ;;
		collection-empty) exit 0 ;;
		*)
			printf '%s\n' \
				'integration-tests/test/tests/shared/test_beta.py::test_one' \
				'integration-tests/test/tests/custom/test_alpha.py::test_two' \
				'integration-tests/test/tests/standalone/test_gamma.py::test_three'
			exit 0
			;;
	esac
fi
[ -n "$xml" ]
[ "${#selectors[@]}" -eq 1 ]
suite=${selectors[0]%/}
suite=${suite##*/}
printf '%s\n' "$suite" >>"${FAKE_CALLS:?}"
body="<testcase name=\"$suite\"/>"
status=0
case "${FAKE_MODE:-pass}:$suite" in
	skip:custom) body='<testcase name="custom"><skipped/></testcase>' ;;
	short:standalone) body= ;;
	fail:custom) body='<testcase name="custom"><failure/></testcase>'; status=1 ;;
	no-report:custom) exit 0 ;;
	timeout:shared) sleep 5 ;;
	pass:* | skip:* | short:* | fail:* | no-report:* | timeout:*) ;;
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
	PREFLIGHT_TIMEOUT_SECONDS="${4:-30}" \
	FAKE_MODE="${3:-pass}" \
	FAKE_CALLS="$2/calls" \
		"$RUNNER"
}

run_preflight "$TMP/profile" "$TMP/pass"
grep -Fq 'tests=3 collected=3 failures=0 errors=0 skipped=0' "$TMP/pass/report.txt"
test "$(cat "$TMP/pass/result")" = passed
test "$(cat "$TMP/pass/selection.txt")" = "$(cat "$TMP/profile")"
test "$(cat "$TMP/pass/calls")" = $'shared\ncustom\nstandalone'
test -f "$TMP/pass/junit-shared.xml"
test -f "$TMP/pass/junit-custom.xml"
test -f "$TMP/pass/junit-standalone.xml"
test "$(grep -o '<testcase ' "$TMP/pass/junit.xml" | wc -l)" -eq 3

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

if run_preflight "$TMP/profile" "$TMP/timeout" timeout 1; then
	printf 'preflight accepted a global timeout\n' >&2
	exit 1
fi
test "$(cat "$TMP/timeout/result")" = failed
test "$(cat "$TMP/timeout/calls")" = shared
grep -Fq 'suite_statuses=shared:124 custom:124 standalone:124' "$TMP/timeout/report.txt"

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
