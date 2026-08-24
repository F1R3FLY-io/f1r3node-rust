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
cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
case "$*" in
  *"pulls/311"*) cat "$FAKE_PULL" ;;
  *"commits/"*) cat "$FAKE_COMMIT" ;;
  *"compare/"*) printf '%s\n' "${FAKE_RELATIONSHIP:-ahead}" ;;
  *"pulls"*) printf '%s\n' "$FAKE_CHILDREN" ;;
  *) printf 'unexpected gh invocation: %s\n' "$*" >&2; exit 1 ;;
esac
SH
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

run_case standalone pull_request refs/pull/1/merge dev feature/one example/repository '[]' '' '' true
run_case lower-stack pull_request refs/pull/1/merge dev feature/one example/repository '[{}]' '' '' false
run_case upper-stack pull_request refs/pull/2/merge feature/one feature/two example/repository '[]' '' '' false
run_case fork pull_request refs/pull/3/merge dev feature/three fork/repository '[]' '' '' false
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

printf 'CI stack gate tests passed\n'
