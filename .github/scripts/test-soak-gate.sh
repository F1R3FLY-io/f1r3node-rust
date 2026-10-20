#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORKFLOW="$ROOT/.github/workflows/merge-recovery-soak.yml"
REPORT="$ROOT/.github/scripts/report-soak-gate.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

SHA=1111111111111111111111111111111111111111
OTHER=2222222222222222222222222222222222222222

ruby -ryaml -e '
  doc = YAML.load_file(ARGV[0])
  step = doc.dig("jobs", "schedule_gate", "steps").find { |item| item["id"] == "resolve" }
  abort "schedule gate not found" unless step && step["run"].is_a?(String)
  File.write(ARGV[1], step["run"])
  jobs = doc["jobs"]
  wiring = {
    "launch_group" => jobs.dig("launch_runner", "concurrency", "group"),
    "soak_group" => jobs.dig("soak", "concurrency", "group"),
    "launch_label" => jobs.dig("launch_runner", "env", "SOAK_LABEL"),
    "soak_runs_on" => jobs.dig("soak", "runs-on").join(","),
    "perf_if" => jobs.dig("perf_report", "if"),
    "pending_if" => jobs.dig("soak_gate_pending", "if"),
    "finalize_if" => jobs.dig("soak_gate_finalize", "if"),
    "finalize_needs" => jobs.dig("soak_gate_finalize", "needs").join(","),
    "final_deadline" => jobs.dig("soak", "steps").find { |s| s["name"] == "Soak final segment" }.dig("with", "deadline_epoch"),
    "verdict_output" => jobs.dig("soak", "outputs", "verdict"),
    "retry_run" => jobs.dig("retry_within_window", "steps", 0, "run"),
    "retry_script" => jobs.dig("retry_within_window", "steps", 0, "run"),
    "run_name" => doc["run-name"],
  }
  wiring.each { |k, v| abort "#{k} not found" if v.nil? || v.to_s.empty?; File.write(File.join(ARGV[2], k), v.to_s) }
' "$WORKFLOW" "$TMP/gate.sh" "$TMP"

contains() {
	grep -Fq -- "$2" "$TMP/$1" || fail "$1 does not contain $2: $(cat "$TMP/$1")"
}
for group in launch_group soak_group; do
	contains "$group" "gate == 'ca-item' && format('ca-item-{0}', needs.schedule_gate.outputs.gate_pr)"
	contains "$group" "gate == 'ca-release' && 'ca-release'"
	contains "$group" "|| needs.schedule_gate.outputs.kind }}"
done
contains launch_label "gate != '' && format('f1r3fly-rust-soak-run-{0}', github.run_id) || 'f1r3fly-rust-soak'"
contains soak_runs_on "needs.launch_runner.outputs.runner_label"
contains perf_if "needs.schedule_gate.outputs.gate == ''"
contains pending_if "needs.schedule_gate.outputs.gate != ''"
contains finalize_if "always()"
contains finalize_if "needs.schedule_gate.outputs.gate != ''"
contains finalize_needs "soak"
contains final_deadline "needs.schedule_gate.outputs.gate == ''"
contains verdict_output "steps.verdict.outputs.verdict"
contains retry_run '-f gate="$GATE"'
contains run_name "format('Merge Recovery Soak [ca-item:#{0}]', inputs.gate_pr)"

mkdir -p "$TMP/bin"
# The clock puts one Pacific 13:00 checkpoint one hour after the start, so a
# run without a gate has a checkpoint and a gate run must clear it.
cat >"$TMP/bin/date" <<'SH'
#!/usr/bin/env bash
case "$*" in
  "-u +%s") printf '1785810600\n' ;;
  *" +%F") printf '2026-01-01\n' ;;
  *"2026-01-01 13:00 +%s") printf '1785814200\n' ;;
  *" +%s") printf '0\n' ;;
  *) printf '2026-01-01T00:00:00Z\n' ;;
esac
SH
cat >"$TMP/bin/gh" <<SH
#!/usr/bin/env bash
case "\$*" in
  *"/commits/"*) printf '%s\n' "$SHA" ;;
  *) printf 'unexpected gh invocation: %s\n' "\$*" >&2; exit 1 ;;
esac
SH
chmod +x "$TMP/bin/date" "$TMP/bin/gh"

# run_gate GATE GATE_PR DURATION TARGET [NAME=VALUE...]: sets status and out.
run_gate() {
	local gate="$1" gate_pr="$2" duration="$3" target="$4"
	shift 4
	: >"$TMP/output"
	set +e
	out="$(env PATH="$TMP/bin:$PATH" EVENT_NAME=workflow_dispatch EVENT_SCHEDULE="" INPUT_SCHEDULED_SLOT="" \
		INPUT_DURATION="$duration" INPUT_TARGET_REF="$target" INPUT_WINDOW_END="" INPUT_SERIES="" \
		INPUT_RETRY_ATTEMPT=0 INPUT_CANARY=false INPUT_PREFLIGHT_ONLY=false INPUT_SKIP_PREFLIGHT=false \
		INPUT_INJECT_PROTECTION_BREACH=false INPUT_CANDIDATE_TAG="" INPUT_GATE="$gate" INPUT_GATE_PR="$gate_pr" \
		GH_TOKEN=test GITHUB_REPOSITORY=example/repo GITHUB_RUN_ID=999 GITHUB_OUTPUT="$TMP/output" \
		GITHUB_STEP_SUMMARY="$TMP/summary" "$@" bash -euo pipefail "$TMP/gate.sh" 2>&1)"
	status=$?
	set -e
}

run_gate ca-item 12 daily-24h "$SHA"
[ "$status" -eq 0 ] || fail "a ca-item gate run was refused: $out"
for line in gate=ca-item gate_pr=12 kind=daily duration_seconds=79200 "target_sha=$SHA" checkpoint_1= should_run=true; do
	grep -qx "$line" "$TMP/output" || fail "the ca-item gate run has no $line: $(cat "$TMP/output")"
done

run_gate ca-release "" weekend-60h "$SHA"
[ "$status" -eq 0 ] || fail "a ca-release gate run was refused: $out"
for line in gate=ca-release gate_pr= kind=weekend duration_seconds=216000 checkpoint_1=; do
	grep -qx "$line" "$TMP/output" || fail "the ca-release gate run has no $line: $(cat "$TMP/output")"
done

run_gate ca-item 12 daily-24h "$SHA" INPUT_RETRY_ATTEMPT=1
[ "$status" -eq 0 ] || fail "the restart of a ca-item gate run was refused: $out"
grep -qx retry_attempt=1 "$TMP/output" || fail "the gate restart lost its retry attempt: $(cat "$TMP/output")"

run_gate "" "" daily-24h dev
[ "$status" -eq 0 ] || fail "a manual run without a gate was refused: $out"
grep -qx gate= "$TMP/output" || fail "a run without a gate reported a gate: $(cat "$TMP/output")"
grep -qx checkpoint_1=1785814200 "$TMP/output" || fail "the fake clock gave a run without a gate no checkpoint: $(cat "$TMP/output")"

refuse() {
	local reason="$1"
	shift
	run_gate "$@"
	[ "$status" -ne 0 ] || fail "$reason was accepted"
	[[ ! -s "$TMP/output" ]] || fail "$reason still wrote outputs: $(cat "$TMP/output")"
}
refuse "a gate_pr without a gate" "" 12 daily-24h "$SHA"
refuse "an unknown gate" ca-other 12 daily-24h "$SHA"
refuse "a gate with a branch name as target" ca-item 12 daily-24h ca/item
refuse "a gate with a short SHA" ca-item 12 daily-24h 111111111
refuse "a ca-item gate without gate_pr" ca-item "" daily-24h "$SHA"
refuse "a ca-item gate with a bad gate_pr" ca-item 12a daily-24h "$SHA"
refuse "a ca-item gate for 60 hours" ca-item 12 weekend-60h "$SHA"
refuse "a ca-release gate for 22 hours" ca-release "" daily-24h "$SHA"
refuse "a ca-release gate with gate_pr" ca-release 12 weekend-60h "$SHA"
refuse "a gate canary" ca-item 12 daily-24h "$SHA" INPUT_CANARY=true
refuse "a gate preflight-only run" ca-item 12 daily-24h "$SHA" INPUT_PREFLIGHT_ONLY=true
refuse "a gate without the preflight" ca-item 12 daily-24h "$SHA" INPUT_SKIP_PREFLIGHT=true
refuse "a gate window restart" ca-item 12 daily-24h "$SHA" INPUT_WINDOW_END=1785900000 INPUT_SERIES=daily
refuse "a gate candidate" ca-release "" weekend-60h "$SHA" INPUT_CANDIDATE_TAG=v1.0.0-canary.1
refuse "a scheduled gate" ca-item 12 daily-24h "$SHA" INPUT_SCHEDULED_SLOT=1785810600
refuse "a gate on a cron event" ca-item 12 daily-24h "$SHA" EVENT_NAME=schedule

# The restart of a gate run dispatches a full new window: no window_end_epoch,
# no series, and the gate inputs of the failed run.
retry() {
	: >"$TMP/retry.log"
	env PATH="$TMP/bin:$PATH" GH_LOG="$TMP/retry.log" END_EPOCH=1785900000 KIND="$1" TARGET_REF="$SHA" \
		SKIP_PREFLIGHT=false CANDIDATE_TAG="" GATE="$2" GATE_PR="$3" WORKFLOW_REF=master GITHUB_RUN_ID=999 \
		bash -euo pipefail "$TMP/retry_script" >/dev/null
}
cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$GH_LOG"
SH
cat >"$TMP/bin/date" <<'SH'
#!/usr/bin/env bash
case "$*" in
  "-u +%s") printf '1785810600\n' ;;
  *) printf '2026-01-01T00:00:00Z\n' ;;
esac
SH
chmod +x "$TMP/bin/gh" "$TMP/bin/date"
retry daily ca-item 12
grep -Fxq "workflow run merge-recovery-soak.yml --ref master -f target_ref=$SHA -f duration=daily-24h -f gate=ca-item -f gate_pr=12 -f retry_attempt=1" "$TMP/retry.log" ||
	fail "the ca-item restart is not a full new window: $(cat "$TMP/retry.log")"
retry weekend ca-release ""
grep -Fxq "workflow run merge-recovery-soak.yml --ref master -f target_ref=$SHA -f duration=weekend-60h -f gate=ca-release -f gate_pr= -f retry_attempt=1" "$TMP/retry.log" ||
	fail "the ca-release restart is not a full new window: $(cat "$TMP/retry.log")"
retry daily "" ""
grep -q -- "-f window_end_epoch=1785900000 -f series=daily" "$TMP/retry.log" ||
	fail "the restart of a run without a gate no longer soaks the rest of its window: $(cat "$TMP/retry.log")"

cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$GH_LOG"
case "$*" in
  *"/commits/"*"/status"*) printf '%s\n' "${FAKE_CURRENT_URL:-}" ;;
  *"/pulls/"*) printf '%s\n' "${FAKE_PR:-}" ;;
  *"/statuses/"* | *"/labels"*) ;;
  *) printf 'unexpected gh invocation: %s\n' "$*" >&2; exit 1 ;;
esac
SH
chmod +x "$TMP/bin/gh"

RUN_URL=https://github.com/example/repo/actions/runs/500/attempts/1

# report MODE [NAME=VALUE...]: sets status, out, and gh.log.
report() {
	local mode="$1"
	shift
	: >"$TMP/gh.log"
	set +e
	out="$(env PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" GH_REPO=example/repo GATE=ca-item GATE_PR=12 \
		TARGET_SHA="$SHA" RUN_URL="$RUN_URL" GITHUB_SERVER_URL=https://github.com \
		FAKE_PR="$(printf 'open\t%s\tca-canary' "$SHA")" "$@" bash "$REPORT" "$mode" 2>&1)"
	status=$?
	set -e
}
posted() {
	grep -q "^api --method POST repos/example/repo/statuses/$SHA -f state=$1 -f context=$2 " "$TMP/gh.log" ||
		fail "no $1 status for $2: $(cat "$TMP/gh.log")"
}
labelled() { grep -q 'issues/12/labels -f labels\[\]=ca-soaked' "$TMP/gh.log"; }
PASS=(SOAK_RESULT=success COMPLETED=true PREFLIGHT=passed VERDICT=pass LAUNCH_RESULT=success)

report pending
[ "$status" -eq 0 ] || fail "the pending report failed: $out"
posted pending 'Soak Gate (22h)'
! labelled || fail "a pending report added ca-soaked"

report final "${PASS[@]}"
[ "$status" -eq 0 ] || fail "the passing report failed: $out"
posted success 'Soak Gate (22h)'
labelled || fail "a passed soak did not add ca-soaked: $(cat "$TMP/gh.log")"

report final "${PASS[@]}" GATE=ca-release GATE_PR=
posted success 'Soak Gate (60h)'
! labelled || fail "a ca-release gate added a label"
if grep -q '/pulls/' "$TMP/gh.log"; then fail "a ca-release gate read a pull request"; fi

for broken in SOAK_RESULT=failure COMPLETED= PREFLIGHT=failed VERDICT=regress VERDICT= SOAK_RESULT=skipped; do
	report final "${PASS[@]}" "$broken"
	[ "$status" -eq 0 ] || fail "the report with $broken failed: $out"
	posted failure 'Soak Gate (22h)'
	! labelled || fail "a soak with $broken added ca-soaked"
done

report final "${PASS[@]}" SOAK_RESULT=cancelled
posted error 'Soak Gate (22h)'
report final "${PASS[@]}" SOAK_RESULT=skipped LAUNCH_RESULT=cancelled
posted error 'Soak Gate (22h)'

report final "${PASS[@]}" FAKE_PR="$(printf 'open\t%s\tca-canary' "$OTHER")"
posted success 'Soak Gate (22h)'
! labelled || fail "ca-soaked was added after the head moved"
[[ "$out" == *"ca-soaked is not added"* ]] || fail "a moved head was not reported: $out"
report final "${PASS[@]}" FAKE_PR="$(printf 'closed\t%s\tca-canary' "$SHA")"
! labelled || fail "ca-soaked was added to a closed pull request"
report final "${PASS[@]}" FAKE_PR="$(printf 'open\t%s\tdev' "$SHA")"
! labelled || fail "ca-soaked was added to a pull request into dev"

report final "${PASS[@]}" FAKE_CURRENT_URL=https://github.com/example/repo/actions/runs/501/attempts/1
[ "$status" -eq 0 ] || fail "the report under a newer run failed: $out"
if grep -q "repos/example/repo/statuses/" "$TMP/gh.log" || labelled; then fail "a run replaced the status of a newer run: $(cat "$TMP/gh.log")"; fi
report final "${PASS[@]}" FAKE_CURRENT_URL=https://github.com/example/repo/actions/runs/499/attempts/2
posted success 'Soak Gate (22h)'

for bad in GATE= GATE=ca-other TARGET_SHA=main GATE_PR=x RUN_URL=https://github.com/other/repo/actions/runs/1; do
	report final "${PASS[@]}" "$bad"
	[ "$status" -eq 2 ] || fail "the report accepted $bad: $out"
	[[ ! -s "$TMP/gh.log" ]] || fail "the report with $bad still called gh: $(cat "$TMP/gh.log")"
done

printf 'PASS: a gate run needs a manual dispatch, a commit SHA, and the 22h ca-item or 60h ca-release duration, and refuses canary, preflight-only, skipped preflight, window restart, candidate, and scheduled inputs. Gate runs have their own concurrency group and runner label, publish no checkpoint or series, and restart with a full window. Soak Gate is success only for a completed soak with a passed preflight and a pass verdict, error for a cancelled soak, and failure otherwise. ca-soaked is added only to an open ca-canary pull request whose head is the soaked commit, and a run never replaces the status of a newer run.\n'
