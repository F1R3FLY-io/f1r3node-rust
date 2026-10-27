#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/record-ci-heavy-waiver.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

mkdir -p "$TMP/bin"
cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$GH_LOG"
calls=$(($(cat "$CALLS") + 1))
printf '%s\n' "$calls" >"$CALLS"
for at in ${READD_AT:-0} ${READD_AGAIN_AT:-0}; do
	if [ "$at" -gt 0 ] && [ "$calls" -eq "$at" ]; then
		grep -qx ci-heavy "$LABELS" || printf 'ci-heavy\n' >>"$LABELS"
	fi
done
if [ "${FAIL_READ_AT:-0}" -eq "$calls" ]; then
	exit 1
fi
case "$*" in
	"api --method POST "*"/labels -f labels[]=ci-heavy-waived")
		printf 'ci-heavy-waived\n' >>"$LABELS"
		;;
	"api --method DELETE "*"/labels/ci-heavy")
		grep -vx ci-heavy "$LABELS" >"$LABELS.new" || true
		mv "$LABELS.new" "$LABELS"
		;;
	"api "*"/labels --jq .[].name")
		cat "$LABELS"
		;;
	*)
		exit 1
		;;
esac
SH
chmod +x "$TMP/bin/gh"

# run INITIAL_LABELS READD_AT: sets status, out, and the final labels.
run() {
	printf '%s' "$1" | tr ',' '\n' | sed '/^$/d' >"$TMP/labels"
	printf '0\n' >"$TMP/calls"
	: >"$TMP/gh.log"
	set +e
	out="$(PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" LABELS="$TMP/labels" CALLS="$TMP/calls" \
		READD_AT="$2" READD_AGAIN_AT="${READD_AGAIN_AT:-0}" FAIL_READ_AT="${FAIL_READ_AT:-0}" \
		REPOSITORY=example/repo PR_NUMBER=42 WAIVER_POLL_SECONDS=0 WAIVER_POLL_ATTEMPTS=4 \
		bash "$TOOL" 2>&1)"
	status=$?
	set -e
	final="$(sort "$TMP/labels" | paste -sd, -)"
}

run "" 0
[ "$status" -eq 0 ] || fail "a removal without a re-add failed: $out"
[ "$final" = ci-heavy-waived ] || fail "a removal without a re-add left labels: $final"

run "" 3
[ "$status" -eq 0 ] || fail "a removal with a re-add failed: $out"
[ "$final" = ci-heavy-waived ] || fail "ci-heavy stayed after Mergify added it again: $final"
[ "$(grep -c 'DELETE' "$TMP/gh.log")" -eq 1 ] || fail "ci-heavy was not removed exactly once: $(cat "$TMP/gh.log")"

run "ci-heavy-waived" 0
[ "$status" -eq 0 ] || fail "an existing waiver failed: $out"
if grep -q 'POST' "$TMP/gh.log"; then fail "an existing waiver was added again"; fi

READD_AGAIN_AT=5 run "" 3
[ "$status" -eq 0 ] || fail "a second re-add failed the step: $out"
[[ "$out" == *"::warning::"*"has ci-heavy again"* ]] || fail "a second re-add gave no warning: $out"
[ "$final" = ci-heavy,ci-heavy-waived ] || fail "a second re-add was hidden: $final"

FAIL_READ_AT=3 run "" 0
[ "$status" -eq 1 ] || fail "a failed label read did not fail the step: $out"
[[ "$out" == *"::error::"*"could not be read"* ]] || fail "a failed label read gave no error annotation: $out"

printf '0\n' >"$TMP/calls"
set +e
out="$(PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" LABELS="$TMP/labels" CALLS="$TMP/calls" REPOSITORY=example/repo PR_NUMBER=abc bash "$TOOL" 2>&1)"
status=$?
set -e
[ "$status" -eq 2 ] || fail "an invalid pull request number was accepted: $out"

echo "CI heavy waiver record: 6 cases passed"
