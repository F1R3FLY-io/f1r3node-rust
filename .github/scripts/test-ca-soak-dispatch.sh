#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/ca-soak-dispatch.sh"
WORKFLOW="$ROOT/.github/workflows/ca-soak-dispatch.yml"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

SHA=1111111111111111111111111111111111111111

mkdir -p "$TMP/bin"
cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$GH_LOG"
case "$*" in
  "workflow run "*) ;;
  *"--method DELETE "*"/labels/"*) [ "${FAKE_LABEL_MISSING:-0}" = 1 ] && exit 1 ;;
  *"/runs?event=workflow_dispatch"*)
    [ "${FAKE_RUNS_FAIL:-0}" = 1 ] && exit 1
    case "$*" in *"status=queued&"*) printf '%s\n' "${FAKE_QUEUED:-}" ;; esac
    case "$*" in *"status=in_progress&"*) printf '%s\n' "${FAKE_RUNS:-}" ;; esac
    # gh joins the pages only with --paginate.
    case "$*" in *"--paginate "*"status=in_progress&"*) printf '%s\n' "${FAKE_RUNS_PAGE2:-}" ;; esac
    ;;
  *"/cancel") ;;
  *) printf 'unexpected gh invocation: %s\n' "$*" >&2; exit 1 ;;
esac
SH
chmod +x "$TMP/bin/gh"

# run [NAME=VALUE...]: sets status, out, and gh.log.
run() {
	: >"$TMP/gh.log"
	set +e
	out="$(env PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" GH_REPO=example/repo PR_NUMBER=12 HEAD_SHA="$SHA" \
		HEAD_REF=ca/feature HEAD_REPO=example/repo BASE_REF=ca-canary SENDER=octo-cat "$@" bash "$TOOL" 2>&1)"
	status=$?
	set -e
}
logged() { grep -Fq -- "$1" "$TMP/gh.log"; }
dispatched() { grep -q '^workflow run merge-recovery-soak.yml' "$TMP/gh.log"; }

run EVENT=labeled LABEL=ca-soak
[ "$status" -eq 0 ] || fail "the ca-soak label failed: $out"
logged "api --method DELETE repos/example/repo/issues/12/labels/ca-soaked" || fail "ca-soak did not remove ca-soaked first: $(cat "$TMP/gh.log")"
grep -Fxq "workflow run merge-recovery-soak.yml --repo example/repo --ref master -f target_ref=$SHA -f duration=daily-24h -f gate=ca-item -f gate_pr=12" "$TMP/gh.log" ||
	fail "ca-soak did not start the 22-hour item soak of the head from master: $(cat "$TMP/gh.log")"
[ "$(grep -n 'labels/ca-soaked' "$TMP/gh.log" | cut -d: -f1)" -lt "$(grep -n '^workflow run' "$TMP/gh.log" | cut -d: -f1)" ] ||
	fail "ca-soaked was removed after the soak started: $(cat "$TMP/gh.log")"

run EVENT=labeled LABEL=ca-soak FAKE_LABEL_MISSING=1
[ "$status" -eq 0 ] || fail "a missing ca-soaked label stopped the soak: $out"
dispatched || fail "a missing ca-soaked label stopped the soak"

run EVENT=labeled LABEL=ci-heavy
[ "$status" -eq 0 ] && [[ ! -s "$TMP/gh.log" ]] || fail "another label started a soak: $(cat "$TMP/gh.log")"

for other in BASE_REF=dev HEAD_REF=feature/x HEAD_REPO=fork/repo; do
	run EVENT=labeled LABEL=ca-soak "$other"
	[ "$status" -eq 0 ] || fail "a pull request with $other failed: $out"
	[[ ! -s "$TMP/gh.log" ]] || fail "a pull request with $other started a soak: $(cat "$TMP/gh.log")"
done
for bad in PR_NUMBER=x HEAD_SHA=main GH_REPO=bad; do
	run EVENT=labeled LABEL=ca-soak "$bad"
	[ "$status" -eq 2 ] || fail "the dispatcher accepted $bad: $out"
	[[ ! -s "$TMP/gh.log" ]] || fail "the dispatcher with $bad still called gh: $(cat "$TMP/gh.log")"
done

run EVENT=synchronize SENDER='mergify[bot]'
[ "$status" -eq 0 ] && [[ ! -s "$TMP/gh.log" ]] || fail "a Mergify base merge changed the soak: $(cat "$TMP/gh.log")"

runs="$(printf '%s\n' 601 602)"
run EVENT=synchronize FAKE_RUNS="$runs"
[ "$status" -eq 0 ] || fail "an author push failed: $out"
grep -Fxq "api --method DELETE repos/example/repo/issues/12/labels/ca-soaked" "$TMP/gh.log" || fail "an author push kept ca-soaked"
grep -Fxq "api --method DELETE repos/example/repo/issues/12/labels/ca-soak" "$TMP/gh.log" || fail "an author push kept ca-soak"
logged 'select(.display_title == "Merge Recovery Soak [ca-item:#12]" and .status != "completed")' ||
	fail "an author push did not look for the runs of its pull request: $(cat "$TMP/gh.log")"
logged "api --method POST repos/example/repo/actions/runs/601/cancel" && logged "api --method POST repos/example/repo/actions/runs/602/cancel" ||
	fail "an author push did not cancel the running soaks: $(cat "$TMP/gh.log")"
! dispatched || fail "an author push started a soak"

run EVENT=synchronize FAKE_RUNS=601 FAKE_RUNS_PAGE2=603 FAKE_QUEUED="$(printf '%s\n' 601 604)"
[ "$status" -eq 0 ] || fail "an author push with runs on later pages failed: $out"
for id in 601 603 604; do
	[ "$(grep -cFx "api --method POST repos/example/repo/actions/runs/$id/cancel" "$TMP/gh.log")" -eq 1 ] ||
		fail "an author push did not cancel run $id exactly once: $(cat "$TMP/gh.log")"
done
for run_status in queued in_progress waiting requested pending; do
	logged "--paginate repos/example/repo/actions/workflows/merge-recovery-soak.yml/runs?event=workflow_dispatch&status=$run_status&" ||
		fail "an author push did not read every page of the $run_status runs: $(cat "$TMP/gh.log")"
done

run EVENT=synchronize FAKE_RUNS_FAIL=1
[ "$status" -ne 0 ] || fail "an author push ignored a failed run listing: $out"

run EVENT=push PUSH_REF=refs/heads/ca-release
[ "$status" -eq 0 ] || fail "a ca-release push failed: $out"
grep -Fxq "workflow run merge-recovery-soak.yml --repo example/repo --ref master -f target_ref=$SHA -f duration=weekend-60h -f gate=ca-release" "$TMP/gh.log" ||
	fail "a ca-release push did not start the 60-hour soak: $(cat "$TMP/gh.log")"
run EVENT=push PUSH_REF=refs/heads/ca-canary
[ "$status" -eq 2 ] && [[ ! -s "$TMP/gh.log" ]] || fail "a ca-canary push started the 60-hour soak: $out"
run EVENT=opened
[ "$status" -eq 2 ] && [[ ! -s "$TMP/gh.log" ]] || fail "an unknown event was accepted: $out"

ruby -ryaml -e '
  doc = YAML.load_file(ARGV[0])
  on = doc[true] || doc["on"]
  abort "the pull request trigger is not labeled and synchronize into ca-canary" unless
    on.dig("pull_request_target", "types").sort == %w[labeled synchronize] &&
    on.dig("pull_request_target", "branches") == ["ca-canary"]
  abort "the push trigger is not ca-release only" unless on.dig("push", "branches") == ["ca-release"]
  abort "the workflow grants permissions at the top level" unless doc["permissions"] == {}
  doc["jobs"].each do |name, job|
    job["steps"].each do |step|
      abort "#{name} checks out a ref" if step["uses"].to_s.start_with?("actions/checkout") && step.dig("with", "ref")
      abort "#{name} interpolates an expression into run" if step["run"].to_s.include?("${{")
    end
  end
  pr = doc.dig("jobs", "pull-request", "if")
  abort "the pull request job does not require a ca/ head of this repository" unless
    pr.include?("head.repo.full_name == github.repository") && pr.include?("startsWith(github.head_ref, '"'"'ca/'"'"')")
' "$WORKFLOW" || fail "ca-soak-dispatch.yml wiring"

printf 'PASS: the ca-soak label on a ca/ pull request of this repository into ca-canary removes ca-soaked and starts the 22-hour item soak of the head from master. Other labels, bases, heads, and forks start nothing. A Mergify base merge changes nothing. An author push removes ca-soak and ca-soaked and cancels the queued and running soaks of the pull request on every page without starting one. A failed run listing fails the push handler. A ca-release push starts the 60-hour release soak. The workflow checks out no pull request ref and passes event values only through the environment.\n'
