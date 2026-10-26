#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/rerun-ci-for-heavy-label.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

mkdir -p "$TMP/bin"
cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$GH_LOG"
case "$*" in
	*"actions/workflows/ci.yml/runs?head_sha="*) printf '%s\n' "${FAKE_RUN_ID:-}" ;;
	*"-X POST "*"/cancel"*) printf 'completed\n' >"$FAKE_STATUS_FILE" ;;
	*"-X POST "*"/rerun"*) : ;;
	*"/jobs?"*) printf '%s\n' "${FAKE_HEAVY:-none}" ;;
	*"actions/runs/"*) cat "$FAKE_STATUS_FILE" ;;
	*) printf 'unexpected gh call: %s\n' "$*" >&2; exit 9 ;;
esac
SH
chmod +x "$TMP/bin/gh"

SHA=0123456789abcdef0123456789abcdef01234567

run_tool() {
	PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" FAKE_STATUS_FILE="$TMP/status" \
		REPOSITORY=example/repo RERUN_POLL_SECONDS=0 RERUN_POLL_ATTEMPTS="${ATTEMPTS:-5}" \
		bash "$TOOL"
}

: >"$TMP/gh.log"
printf 'completed\n' >"$TMP/status"
FAKE_RUN_ID=41 HEAD_SHA="$SHA" run_tool >/dev/null
grep -qx 'api -X POST repos/example/repo/actions/runs/41/rerun' "$TMP/gh.log" || fail "a completed run was not rerun: $(cat "$TMP/gh.log")"
! grep -q '/cancel' "$TMP/gh.log" || fail "a completed run was cancelled: $(cat "$TMP/gh.log")"

: >"$TMP/gh.log"
printf 'in_progress\n' >"$TMP/status"
FAKE_RUN_ID=42 HEAD_SHA="$SHA" run_tool >/dev/null
[ "$(grep -c 'actions/runs/42' "$TMP/gh.log")" -ge 4 ] || fail "a running run was not cancelled, awaited, and rerun: $(cat "$TMP/gh.log")"
grep -n 'actions/runs/42/cancel' "$TMP/gh.log" | head -1 | cut -d: -f1 >"$TMP/cancel.line"
grep -n 'actions/runs/42/rerun' "$TMP/gh.log" | head -1 | cut -d: -f1 >"$TMP/rerun.line"
[ -s "$TMP/cancel.line" ] && [ -s "$TMP/rerun.line" ] || fail "the running run was not cancelled and rerun: $(cat "$TMP/gh.log")"
[ "$(cat "$TMP/cancel.line")" -lt "$(cat "$TMP/rerun.line")" ] || fail "the rerun came before the cancel: $(cat "$TMP/gh.log")"

: >"$TMP/gh.log"
out="$(FAKE_RUN_ID='' HEAD_SHA="$SHA" run_tool)"
[[ "$out" == *"No CI run"* ]] || fail "a missing run was not reported: $out"
! grep -q -- '-X POST' "$TMP/gh.log" || fail "a missing run caused a write: $(cat "$TMP/gh.log")"

: >"$TMP/gh.log"
printf 'in_progress\n' >"$TMP/status"
FAKE_RUN_ID=44 FAKE_HEAVY=active WANT_HEAVY=true HEAD_SHA="$SHA" run_tool >/dev/null
! grep -q -- '-X POST' "$TMP/gh.log" || fail "a run that already runs the heavy pipeline was cancelled or rerun: $(cat "$TMP/gh.log")"

: >"$TMP/gh.log"
printf 'completed\n' >"$TMP/status"
FAKE_RUN_ID=45 FAKE_HEAVY=failed WANT_HEAVY=true HEAD_SHA="$SHA" run_tool >/dev/null
grep -qx 'api -X POST repos/example/repo/actions/runs/45/rerun' "$TMP/gh.log" || fail "a failed heavy run was not rerun for ci-heavy: $(cat "$TMP/gh.log")"

: >"$TMP/gh.log"
printf 'in_progress\n' >"$TMP/status"
FAKE_RUN_ID=46 FAKE_HEAVY=active WANT_HEAVY=false HEAD_SHA="$SHA" run_tool >/dev/null
grep -q 'actions/runs/46/cancel' "$TMP/gh.log" || fail "a waiver did not cancel the running heavy run: $(cat "$TMP/gh.log")"
grep -qx 'api -X POST repos/example/repo/actions/runs/46/rerun' "$TMP/gh.log" || fail "a waiver did not rerun CI with the current labels: $(cat "$TMP/gh.log")"

: >"$TMP/gh.log"
printf 'completed\n' >"$TMP/status"
FAKE_RUN_ID=47 FAKE_HEAVY=failed WANT_HEAVY=false HEAD_SHA="$SHA" run_tool >/dev/null
grep -qx 'api -X POST repos/example/repo/actions/runs/47/rerun' "$TMP/gh.log" || fail "a waiver did not replace failed Integration Gate results: $(cat "$TMP/gh.log")"

: >"$TMP/gh.log"
printf 'completed\n' >"$TMP/status"
FAKE_RUN_ID=48 FAKE_HEAVY=none WANT_HEAVY=false HEAD_SHA="$SHA" run_tool >/dev/null
! grep -q -- '-X POST' "$TMP/gh.log" || fail "a completed run without a heavy pipeline was rerun for a waiver: $(cat "$TMP/gh.log")"

status=0
FAKE_RUN_ID=41 WANT_HEAVY=maybe HEAD_SHA="$SHA" run_tool >/dev/null 2>&1 || status=$?
[ "$status" -eq 2 ] || fail "an invalid WANT_HEAVY was accepted (exit $status)"

: >"$TMP/gh.log"
printf 'in_progress\n' >"$TMP/status"
status=0
(
	# shellcheck disable=SC2016 # The pattern matches the literal text of the fake gh.
	sed -i.bak 's#printf .completed\\n. >"\$FAKE_STATUS_FILE"#:#' "$TMP/bin/gh"
	FAKE_RUN_ID=43 HEAD_SHA="$SHA" ATTEMPTS=3 run_tool >/dev/null 2>&1
) || status=$?
[ "$status" -ne 0 ] || fail "a run that never completes passed"
! grep -q 'actions/runs/43/rerun' "$TMP/gh.log" || fail "a run that never completed was rerun: $(cat "$TMP/gh.log")"

status=0
FAKE_RUN_ID=41 HEAD_SHA=not-a-sha run_tool >/dev/null 2>&1 || status=$?
[ "$status" -eq 2 ] || fail "an invalid head SHA was accepted (exit $status)"

printf 'PASS: the ci-heavy label reruns the newest CI run of the head, cancels a running one first, skips a missing run, and fails when the run does not complete. A run that already runs the heavy pipeline is kept. A waiver cancels a running heavy run, replaces failed gate results, and leaves a completed light run alone.\n'
