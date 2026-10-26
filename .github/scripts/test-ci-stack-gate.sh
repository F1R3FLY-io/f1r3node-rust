#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

ruby -ryaml -e '
  doc = YAML.load_file(ARGV[0])
  step = doc.dig("jobs", "build_base", "steps").find { |item| item["id"] == "target" }
  abort "source target step not found" unless step && step["run"].is_a?(String)
  File.write(ARGV[1], step["run"])
' "$ROOT/.github/workflows/ci.yml" "$TMP/gate.sh"

mkdir -p "$TMP/bin"
cat >"$TMP/bin/gh" <<'FAKE'
#!/usr/bin/env bash
filter=""
args=("$@")
for ((i = 0; i < ${#args[@]}; i++)); do
  [ "${args[i]}" = --jq ] && filter="${args[i + 1]}"
done
emit() {
  if [ -n "$filter" ]; then jq -r "$filter" <<<"$1"; else printf '%s\n' "$1"; fi
}
case "$*" in
  *"${FAKE_FAIL_ON:-no-failure}"*) printf 'simulated API failure: %s\n' "$*" >&2; exit 1 ;;
  *"actions/workflows/ci.yml/runs"*) emit "$FAKE_WORKFLOW_RUNS" ;;
  *"actions/runs/${FAKE_NEWEST_RUN:-none}/jobs"*) emit "$FAKE_NEWEST_JOBS" ;;
  *"actions/runs/"*"/jobs"*) emit "$FAKE_OTHER_JOBS" ;;
  *"git/commits/${FAKE_GROUP_HEAD:-none}"*) emit "{\"tree\": {\"sha\": \"$FAKE_GROUP_TREE\"}}" ;;
  *"git/commits/"*) emit "{\"tree\": {\"sha\": \"$FAKE_HEAD_TREE\"}}" ;;
  *"pulls/648"*) emit "{\"head\": {\"sha\": \"$FAKE_PULL_HEAD\"}}" ;;
  *"pulls/311"*) cat "$FAKE_PULL" ;;
  *"commits/"*) cat "$FAKE_COMMIT" ;;
  *"compare/"*) printf '%s\n' "${FAKE_RELATIONSHIP:-ahead}" ;;
  *"pulls"*) printf '%s\n' "$FAKE_CHILDREN" ;;
  *) printf 'unexpected gh invocation: %s\n' "$*" >&2; exit 1 ;;
esac
FAKE
chmod +x "$TMP/bin/gh"

SHA=1111111111111111111111111111111111111111
MERGE=2222222222222222222222222222222222222222
BASE=3333333333333333333333333333333333333333
HEAD=4444444444444444444444444444444444444444
MERGE_BASE=5555555555555555555555555555555555555555
jq -n --arg merge "$MERGE" --arg base "$BASE" --arg head "$HEAD" '{
  state: "open", merge_commit_sha: $merge,
  head: {sha: $head, repo: {full_name: "example/repository"}}, base: {sha: $base}
}' >"$TMP/pull.json"
jq -n --arg merge "$MERGE" --arg base "$MERGE_BASE" --arg head "$HEAD" \
  '{sha: $merge, parents: [{sha: $base}, {sha: $head}]}' >"$TMP/commit.json"

run_case() {
  local label="$1" event="$2" ref="$3" base_ref="$4" head_ref="$5" head_repo="$6" children="$7" target="$8" top="$9" expected="${10}"
  local fake_pull="${11:-$TMP/pull.json}" relationship="${12:-ahead}" output="$TMP/output-${label}"
  : >"$output"
  PATH="$TMP/bin:$PATH" \
    GH_TOKEN=test \
    GITHUB_REPOSITORY=example/repository \
    GITHUB_SHA="$SHA" \
    GITHUB_OUTPUT="$output" \
    RUNNER_TEMP="$TMP" \
    REQUESTED_TARGET_SHA="$target" \
    REQUESTED_TOP_PULL_REQUEST="$top" \
    EVENT_NAME="$event" \
    EVENT_REF="$ref" \
    PR_BASE_REF="$base_ref" \
    PR_HEAD_REF="$head_ref" \
    PR_HEAD_REPOSITORY="$head_repo" \
    PR_HAS_HEAVY_LABEL="${PR_HAS_HEAVY_LABEL:-false}" \
    MERGE_GROUP_HEAD_REF="${MERGE_GROUP_HEAD_REF:-}" \
    MERGE_GROUP_HEAD_SHA="${MERGE_GROUP_HEAD_SHA:-}" \
    MERGE_GROUP_BASE_SHA="${MERGE_GROUP_BASE_SHA:-}" \
    FAKE_FAIL_ON="${FAKE_FAIL_ON:-no-failure}" \
    FAKE_WORKFLOW_RUNS="${FAKE_WORKFLOW_RUNS:-}" \
    FAKE_NEWEST_RUN="${FAKE_NEWEST_RUN:-none}" \
    FAKE_NEWEST_JOBS="${FAKE_NEWEST_JOBS:-}" \
    FAKE_OTHER_JOBS="${FAKE_OTHER_JOBS:-}" \
    FAKE_GROUP_HEAD="${MERGE_GROUP_HEAD_SHA:-none}" \
    FAKE_GROUP_TREE="${FAKE_GROUP_TREE:-}" \
    FAKE_HEAD_TREE="${FAKE_HEAD_TREE:-}" \
    FAKE_PULL_HEAD="${FAKE_PULL_HEAD:-}" \
    FAKE_CHILDREN="$children" \
    FAKE_PULL="$fake_pull" \
    FAKE_COMMIT="$TMP/commit.json" \
    FAKE_RELATIONSHIP="$relationship" \
    bash "$TMP/gate.sh"
  actual="$(grep '^run_heavy=' "$output" | cut -d= -f2)"
  [ "$actual" = "$expected" ] || {
    printf '%s: expected run_heavy=%s, got %s\n' "$label" "$expected" "$actual" >&2
    return 1
  }
  if [ -n "$target" ]; then
    [ "$(grep '^merge_base_sha=' "$output" | cut -d= -f2)" = "$MERGE_BASE" ] || {
      printf '%s: exact target did not record the synthetic base\n' "$label" >&2
      return 1
    }
  fi
}

run_case standalone pull_request refs/pull/1/merge dev feature/one example/repository '[]' '' '' false
run_case bottom-stack-with-child pull_request refs/pull/1/merge dev feature/one example/repository '[{}]' '' '' false
run_case upper-stack pull_request refs/pull/2/merge feature/one feature/two example/repository '[]' '' '' false
run_case fork pull_request refs/pull/3/merge dev feature/three fork/repository '[]' '' '' false
PR_HAS_HEAVY_LABEL=true \
  run_case labelled-pr pull_request refs/pull/1/merge dev feature/one example/repository '[]' '' '' true
PR_HAS_HEAVY_LABEL=true \
  run_case labelled-fork pull_request refs/pull/3/merge dev feature/three fork/repository '[]' '' '' false
PR_HAS_HEAVY_LABEL=true \
  run_case labelled-upper-stack pull_request refs/pull/2/merge feature/one feature/two example/repository '[]' '' '' false
run_case merge-group merge_group refs/heads/gh-readonly-queue/dev/pr-1-abc '' '' '' '[]' '' '' true
run_case dev-push push refs/heads/dev '' '' '' '[]' '' '' true
run_case version-tag push refs/tags/v0.4.46 '' '' '' '[]' '' '' true
run_case staging-push push refs/heads/staging '' '' '' '[]' '' '' false
run_case exact-merge workflow_dispatch refs/heads/master '' '' '' '[]' "$MERGE" 311 true

jq --arg other "$SHA" '.merge_commit_sha = $other' "$TMP/pull.json" >"$TMP/stale-pull.json"
if run_case stale-merge workflow_dispatch refs/heads/master '' '' '' '[]' "$MERGE" 311 true "$TMP/stale-pull.json" >"$TMP/stale.out" 2>"$TMP/stale.err"; then
  echo 'stale exact merge passed' >&2
  exit 1
fi
if run_case divergent-base workflow_dispatch refs/heads/master '' '' '' '[]' "$MERGE" 311 true "$TMP/pull.json" diverged >"$TMP/diverged.out" 2>"$TMP/diverged.err"; then
  echo 'divergent synthetic base passed' >&2
  exit 1
fi
if run_case missing-top workflow_dispatch refs/heads/master '' '' '' '[]' "$MERGE" '' true >"$TMP/missing-top.out" 2>"$TMP/missing-top.err"; then
  echo 'exact target without a top pull request passed' >&2
  exit 1
fi
if run_case missing-target workflow_dispatch refs/heads/master '' '' '' '[]' '' 311 true >"$TMP/missing-target.out" 2>"$TMP/missing-target.err"; then
  echo 'top pull request without an exact target passed' >&2
  exit 1
fi

ruby -ryaml -e '
  doc = YAML.load_file(ARGV[0])
  names = doc["jobs"].values.map { |job| job["name"] }
  gate = doc.dig("jobs", "build_base", "steps").find { |item| item["id"] == "target" }["run"]
  ["Integration Tests (amd64)", "Integration Tests (arm64)"].each do |name|
    abort "ci.yml has no job named #{name}" unless names.count(name) == 1
    abort "the heavy reuse gate does not read #{name}" unless gate.include?(name)
  end
' "$ROOT/.github/workflows/ci.yml"

GROUP_HEAD=6666666666666666666666666666666666666666
GROUP_BASE=7777777777777777777777777777777777777777
PULL_HEAD=8888888888888888888888888888888888888888
TREE=9999999999999999999999999999999999999999
OTHER_TREE=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
WORKFLOW_RUNS='{"workflow_runs": [
  {"id": 10, "path": ".github/workflows/ci.yml"},
  {"id": 20, "path": ".github/workflows/ci.yml"},
  {"id": 40, "path": ".github/workflows/other.yml"}
]}'
jobs_json() {
  jq -n --arg amd64 "$1" --arg arm64 "$2" '{jobs: [
    {name: "Heavy Pipeline / Integration Tests (amd64-docker)", conclusion: "success"},
    {name: "Integration Tests (amd64)", conclusion: (if $amd64 == "" then null else $amd64 end)},
    {name: "Integration Tests (arm64)", conclusion: (if $arm64 == "" then null else $arm64 end)}
  ]}'
}
merge_group_case() {
  local label="$1" expected="$2"
  MERGE_GROUP_HEAD_REF="${REF:-refs/heads/gh-readonly-queue/dev/pr-648-$GROUP_BASE}" \
    MERGE_GROUP_HEAD_SHA="$GROUP_HEAD" \
    MERGE_GROUP_BASE_SHA="$GROUP_BASE" \
    FAKE_PULL_HEAD="$PULL_HEAD" \
    FAKE_GROUP_TREE="$TREE" \
    FAKE_HEAD_TREE="${HEAD_TREE:-$TREE}" \
    FAKE_WORKFLOW_RUNS="${RUNS:-$WORKFLOW_RUNS}" \
    FAKE_NEWEST_RUN=20 \
    FAKE_NEWEST_JOBS="${NEWEST:-$(jobs_json success success)}" \
    FAKE_OTHER_JOBS="${OTHER:-$(jobs_json failure failure)}" \
    FAKE_FAIL_ON="${FAIL_ON:-no-failure}" \
    run_case "$label" merge_group refs/heads/gh-readonly-queue/dev/pr-648 '' '' '' '[]' '' '' "$expected" \
    "$TMP/pull.json" "${RELATIONSHIP:-ahead}"
}

merge_group_case reuse-newest-ci-run false
RELATIONSHIP=identical merge_group_case reuse-identical-base false
HEAD_TREE="$OTHER_TREE" merge_group_case tree-differs true
RELATIONSHIP=behind merge_group_case base-not-contained true
RELATIONSHIP=diverged merge_group_case base-diverged true
NEWEST="$(jobs_json skipped skipped)" merge_group_case heavy-skipped true
NEWEST="$(jobs_json skipped skipped)" OTHER="$(jobs_json success success)" merge_group_case older-run-passed-newer-skipped true
NEWEST="$(jobs_json success failure)" merge_group_case arm64-failed true
NEWEST="$(jobs_json success "")" merge_group_case arm64-pending true
NEWEST='{"jobs": [{"name": "Integration Tests (amd64)", "conclusion": "success"}, {"name": "Integration Tests (amd64)", "conclusion": "success"}, {"name": "Integration Tests (arm64)", "conclusion": "success"}]}' \
  merge_group_case duplicate-job true
NEWEST='{"jobs": []}' merge_group_case no-jobs true
RUNS='{"workflow_runs": []}' merge_group_case no-ci-run true
RUNS='{"workflow_runs": [{"id": 20, "path": ".github/workflows/other.yml"}]}' merge_group_case other-workflow-only true
REF=refs/heads/gh-readonly-queue/dev/pr-648-abc merge_group_case malformed-ref true
REF=refs/heads/feature/pr-648 merge_group_case foreign-ref true
FAIL_ON=pulls/648 merge_group_case pull-api-error true
FAIL_ON=compare/ merge_group_case compare-api-error true
FAIL_ON=git/commits merge_group_case tree-api-error true
FAIL_ON=workflows/ci.yml/runs merge_group_case runs-api-error true
FAIL_ON=/jobs merge_group_case jobs-api-error true

printf 'CI stack gate tests passed\n'
