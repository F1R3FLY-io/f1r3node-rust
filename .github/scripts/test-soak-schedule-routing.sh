#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

ruby -ryaml -e '
  doc = YAML.load_file(ARGV[0])
  step = doc.dig("jobs", "schedule_gate", "steps").find { |item| item["id"] == "resolve" }
  abort "schedule gate not found" unless step && step["run"].is_a?(String)
  pending = doc.dig("jobs", "preflight_pending", "steps", 0, "run")
  finalizer = doc.dig("jobs", "preflight_finalize", "steps", 0, "run")
  status = YAML.load_file(ARGV[4]).dig("jobs", "report", "steps", 0, "run")
  abort "status script not found" unless pending && finalizer && status
  File.write(ARGV[1], step["run"])
  File.write(ARGV[2], pending)
  File.write(ARGV[3], finalizer)
  File.write(ARGV[5], status)
' "$ROOT/.github/workflows/merge-recovery-soak.yml" "$TMP/gate.sh" \
  "$TMP/pending.sh" "$TMP/finalizer.sh" \
  "$ROOT/.github/workflows/soak-preflight-status.yml" "$TMP/status.sh"

mkdir -p "$TMP/bin"
cat >"$TMP/bin/date" <<'SH'
#!/usr/bin/env bash
args="$*"
case "$args" in
  "-u +%s") printf '%s\n' "$FAKE_NOW_EPOCH" ;;
  *" +%H%M") printf '%s\n' "${FAKE_PACIFIC_HM:-1930}" ;;
  *" +%u") printf '%s\n' "$FAKE_PACIFIC_WEEKDAY" ;;
  *" +%FT%TZ") printf '%s\n' '2026-01-01T00:00:00Z' ;;
  *" +%Y-%m-%dT%H:%M:%SZ") printf '%s\n' '2026-01-01T00:00:00Z' ;;
  *" +%F") printf '%s\n' '2026-01-01' ;;
  *" +%s")
    case "$args" in
      *":30:00"*) printf '%s\n' "$FAKE_SLOT_EPOCH" ;;
      *) printf '0\n' ;;
    esac
    ;;
  *) printf 'unsupported fake date invocation: %s\n' "$args" >&2; exit 1 ;;
esac
SH
cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
case "$*" in
  *"/commits/"*"/status"*) printf '%b\n' "${FAKE_STATUS_CURRENT:-}" ;;
  *"statuses/"*) printf '%s\n' "$*" >>"${FAKE_STATUS_POSTS:?}" ;;
  *"/commits?"*) printf '1\n' ;;
  *"/commits/"*) printf 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n' ;;
  *"runs?event=workflow_dispatch"*)
    if [ -n "${FAKE_CLAIM_QUERY_FAIL:-}" ]; then
      exit 1
    fi
    if [ -n "${FAKE_CLAIMED_SLOT:-}" ]; then
      printf '{"workflow_runs":[{"id":123,"display_title":"Merge Recovery Soak [scheduled:%s]","status":"%s","conclusion":%s}]}\n' \
        "$FAKE_CLAIMED_SLOT" "${FAKE_CLAIMED_STATUS:-in_progress}" "${FAKE_CLAIMED_CONCLUSION:-null}"
    else
      printf '%s\n' '{"workflow_runs":[]}'
    fi
    ;;
  *)
    printf 'unexpected gh invocation: %s\n' "$*" >&2
    exit 1
    ;;
esac
SH
chmod +x "$TMP/bin/date" "$TMP/bin/gh"

run_gate() {
	local mode="$1" weekday="$2" slot="$3" tag="${4:-}" output
	output="$TMP/output-$mode-$weekday${tag:+-$tag}"
	: >"$output"
	PATH="$TMP/bin:$PATH" \
		FAKE_NOW_EPOCH="$((slot + 60))" \
		FAKE_SLOT_EPOCH="$slot" \
		FAKE_CLAIMED_SLOT="${FAKE_CLAIMED_SLOT:-}" \
		FAKE_CLAIMED_STATUS="${FAKE_CLAIMED_STATUS:-}" \
		FAKE_CLAIMED_CONCLUSION="${FAKE_CLAIMED_CONCLUSION:-}" \
		FAKE_CLAIM_QUERY_FAIL="${FAKE_CLAIM_QUERY_FAIL:-}" \
		FAKE_PACIFIC_WEEKDAY="$weekday" \
		EVENT_NAME="$([ "$mode" = oci ] && printf workflow_dispatch || printf schedule)" \
		EVENT_SCHEDULE="30 2 * * *" \
		INPUT_SCHEDULED_SLOT="$([ "$mode" = oci ] && printf '%s' "$slot")" \
		INPUT_DURATION=daily-24h \
		INPUT_TARGET_REF=dev \
		INPUT_WINDOW_END="" \
		INPUT_SERIES="" \
		INPUT_RETRY_ATTEMPT=0 \
		INPUT_CANARY=false \
		INPUT_PREFLIGHT_ONLY=false \
		INPUT_INJECT_PROTECTION_BREACH=false \
		GH_TOKEN=test \
		GITHUB_REPOSITORY=F1R3FLY-io/f1r3node-rust \
		GITHUB_RUN_ID=999 \
		GITHUB_OUTPUT="$output" \
		GITHUB_STEP_SUMMARY="$TMP/summary" \
		bash -euo pipefail "$TMP/gate.sh" >/dev/null
	printf '%s\n' "$output"
}

for mode in oci cron; do
	friday="$(run_gate "$mode" 5 1785551400)"
	grep -qx 'target_ref=master' "$friday"
	grep -qx 'duration_seconds=216000' "$friday"
	grep -qx 'kind=weekend' "$friday"
	grep -qx 'run_benchmarks=true' "$friday"
	grep -qx 'target_sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' "$friday"

	monday="$(run_gate "$mode" 1 1785810600)"
	grep -qx 'target_ref=dev' "$monday"
	grep -qx 'duration_seconds=79200' "$monday"
	grep -qx 'kind=daily' "$monday"
	grep -qx 'run_benchmarks=false' "$monday"
done

# A cron delivery whose slot was claimed by a live OCI-dispatched run must
# yield: the soak concurrency group cancels in-progress, so a late cron
# would otherwise kill a soak hours into its run.
claimed="$(FAKE_CLAIMED_SLOT=1785810600 run_gate cron 1 1785810600 live-claim)"
grep -qx 'should_run=false' "$claimed"
# A dispatch that failed or was cancelled left no soak running, so the cron
# fallback must still soak the night rather than yield to a dead claim.
dead_claim="$(FAKE_CLAIMED_SLOT=1785810600 FAKE_CLAIMED_STATUS=completed \
	FAKE_CLAIMED_CONCLUSION='"failure"' run_gate cron 1 1785810600 dead-claim)"
grep -qx 'should_run=true' "$dead_claim"
# A completed successful dispatch keeps the claim (nothing left to soak).
done_claim="$(FAKE_CLAIMED_SLOT=1785810600 FAKE_CLAIMED_STATUS=completed \
	FAKE_CLAIMED_CONCLUSION='"success"' run_gate cron 1 1785810600 done-claim)"
grep -qx 'should_run=false' "$done_claim"
# The unclaimed cron delivery from the loop above must still have run.
grep -qx 'should_run=true' "$TMP/output-cron-1"
# A failed claim query must fail OPEN: soak anyway rather than silently skip.
open_fail="$(FAKE_CLAIM_QUERY_FAIL=1 run_gate cron 1 1785810600 query-fail)"
grep -qx 'should_run=true' "$open_fail"

if PATH="$TMP/bin:$PATH" \
	FAKE_NOW_EPOCH=1785551460 \
	FAKE_SLOT_EPOCH=1785551400 \
	FAKE_PACIFIC_WEEKDAY=5 \
	EVENT_NAME=workflow_dispatch \
	EVENT_SCHEDULE="" \
	INPUT_SCHEDULED_SLOT=1785551400 \
	INPUT_DURATION=daily-24h \
	INPUT_TARGET_REF=dev \
	INPUT_WINDOW_END="" \
	INPUT_SERIES="" \
	INPUT_RETRY_ATTEMPT=0 \
	INPUT_CANARY=true \
	INPUT_PREFLIGHT_ONLY=false \
	INPUT_INJECT_PROTECTION_BREACH=false \
	GH_TOKEN=test \
	GITHUB_REPOSITORY=F1R3FLY-io/f1r3node-rust \
	GITHUB_RUN_ID=999 \
	GITHUB_OUTPUT="$TMP/conflict-output" \
	GITHUB_STEP_SUMMARY="$TMP/summary" \
	bash -euo pipefail "$TMP/gate.sh" >/dev/null 2>&1; then
	echo 'scheduled slot accepted conflicting canary controls' >&2
	exit 1
fi

preflight_output="$TMP/preflight-output"
: >"$preflight_output"
PATH="$TMP/bin:$PATH" \
	FAKE_NOW_EPOCH=1785810660 \
	FAKE_SLOT_EPOCH=1785810600 \
	FAKE_PACIFIC_WEEKDAY=1 \
	EVENT_NAME=workflow_dispatch \
	EVENT_SCHEDULE="" \
	INPUT_SCHEDULED_SLOT="" \
	INPUT_DURATION=daily-24h \
	INPUT_TARGET_REF=dev \
	INPUT_WINDOW_END="" \
	INPUT_SERIES="" \
	INPUT_RETRY_ATTEMPT=0 \
	INPUT_CANARY=false \
	INPUT_PREFLIGHT_ONLY=true \
	INPUT_INJECT_PROTECTION_BREACH=false \
	GH_TOKEN=test \
	GITHUB_REPOSITORY=F1R3FLY-io/f1r3node-rust \
	GITHUB_RUN_ID=999 \
	GITHUB_OUTPUT="$preflight_output" \
	GITHUB_STEP_SUMMARY="$TMP/summary" \
	bash -euo pipefail "$TMP/gate.sh" >/dev/null
grep -qx 'duration_seconds=14400' "$preflight_output"
grep -qx 'preflight_only=true' "$preflight_output"
grep -qx 'run_benchmarks=false' "$preflight_output"
grep -qx 'checkpoint_1=' "$preflight_output"
grep -qx 'target_sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' "$preflight_output"

if PATH="$TMP/bin:$PATH" \
	FAKE_NOW_EPOCH=1785810660 \
	FAKE_SLOT_EPOCH=1785810600 \
	FAKE_PACIFIC_WEEKDAY=1 \
	EVENT_NAME=workflow_dispatch \
	EVENT_SCHEDULE="" \
	INPUT_SCHEDULED_SLOT="" \
	INPUT_DURATION=daily-24h \
	INPUT_TARGET_REF=dev \
	INPUT_WINDOW_END="" \
	INPUT_SERIES="" \
	INPUT_RETRY_ATTEMPT=0 \
	INPUT_CANARY=false \
	INPUT_PREFLIGHT_ONLY=invalid \
	INPUT_INJECT_PROTECTION_BREACH=false \
	GH_TOKEN=test \
	GITHUB_REPOSITORY=F1R3FLY-io/f1r3node-rust \
	GITHUB_RUN_ID=999 \
	GITHUB_OUTPUT="$TMP/invalid-preflight-output" \
	GITHUB_STEP_SUMMARY="$TMP/summary" \
	bash -euo pipefail "$TMP/gate.sh" >/dev/null 2>&1; then
	echo 'schedule gate accepted invalid preflight_only input' >&2
	exit 1
fi

run_status_script() {
  local script="$1" current="$2" run_id="$3" run_attempt="$4"
  : >"$TMP/status-posts"
  PATH="$TMP/bin:$PATH" \
    FAKE_STATUS_CURRENT="$current" \
    FAKE_STATUS_POSTS="$TMP/status-posts" \
    GH_TOKEN=test \
    GITHUB_ACTOR='github-actions[bot]' \
    GITHUB_REPOSITORY=F1R3FLY-io/f1r3node-rust \
    GITHUB_SERVER_URL=https://github.com \
    TARGET_SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
    RUN_URL="https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/${run_id}/attempts/${run_attempt}" \
    SOURCE_RUN_URL="https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/${run_id}/attempts/${run_attempt}" \
    PENDING_RESULT=success \
    LAUNCH_RESULT=success \
    SOAK_RESULT=success \
    PREFLIGHT_RESULT=passed \
    bash -euo pipefail "$script" >/dev/null
}

run_status_script "$TMP/pending.sh" \
  $'failure\thttps://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/100' 200 1
grep -q -- '-f state=pending' "$TMP/status-posts"
run_status_script "$TMP/pending.sh" \
  $'pending\thttps://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/300/attempts/1' 200 1
test ! -s "$TMP/status-posts"
run_status_script "$TMP/pending.sh" \
  $'success\thttps://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/200/attempts/1' 200 1
test ! -s "$TMP/status-posts"
run_status_script "$TMP/pending.sh" \
  $'pending\thttps://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/200/attempts/2' 200 1
test ! -s "$TMP/status-posts"
run_status_script "$TMP/pending.sh" \
  $'failure\thttps://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/200/attempts/1' 200 2
grep -q -- '-f state=pending' "$TMP/status-posts"
run_status_script "$TMP/finalizer.sh" \
  $'failure\thttps://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/300/attempts/1' 200 1
test ! -s "$TMP/status-posts"
run_status_script "$TMP/finalizer.sh" \
  $'pending\thttps://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/200/attempts/1' 200 1
grep -q -- '-f state=success' "$TMP/status-posts"
run_status_script "$TMP/finalizer.sh" \
  $'pending\thttps://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/200/attempts/2' 200 1
test ! -s "$TMP/status-posts"
if run_status_script "$TMP/finalizer.sh" \
  $'pending\thttps://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/100/attempts/1' 200 1; then
  echo 'finalizer replaced a status that belongs to another run' >&2
  exit 1
fi
test ! -s "$TMP/status-posts"
run_status_script "$TMP/status.sh" \
  $'pending\thttps://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/300/attempts/1' 200 1
test ! -s "$TMP/status-posts"
run_status_script "$TMP/status.sh" \
  $'pending\thttps://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/200/attempts/1' 200 1
grep -q -- '-f state=success' "$TMP/status-posts"
run_status_script "$TMP/status.sh" \
  $'failure\thttps://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/200/attempts/1' 200 1
test ! -s "$TMP/status-posts"

printf 'soak schedule routing and status ownership tests passed\n'
