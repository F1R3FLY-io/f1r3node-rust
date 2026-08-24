#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/release-train.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
REPOSITORY=example/repository

sha() { printf '%s' "$1" | sha256sum | awk '{print substr($1, 1, 40)}'; }
S299="$(sha 299)"; S312="$(sha 312)"; S319="$(sha 319)"; S311="$(sha 311)"
OTHER="$(sha other)"; CONTROL="$(sha control)"; M311="$(sha merge311)"

expect_failure() {
	local label="$1"
	shift
	if "$@" >"$TMP/out" 2>"$TMP/err"; then
		printf 'expected failure: %s\n' "$label" >&2
		exit 1
	fi
}

# --- Manifests ---------------------------------------------------------------
cat >"$TMP/single.yml" <<EOF
schema_version: 1
id: cost-accounting
state: proposed
target_version: 0.5.0
pull_request: 216
head_sha: $S311
base_branch: master
required_gates:
  - id: cost-accounting-testbed
    workflow: testbed-quality-gate.yml
    job: Cost Accounting Quality Gate
    binds_image_digest: true
EOF
cat >"$TMP/stack.yml" <<EOF
schema_version: 2
id: key-contention
state: proposed
target_version: 0.4.46
pull_request: 311
head_sha: $S311
base_branch: master
publishing: false
stack:
  members:
    - pull_request: 299
    - pull_request: 312
    - pull_request: 319
    - pull_request: 311
required_gates: []
EOF
"$TOOL" validate-manifest "$TMP/single.yml" | jq -e '.publishing == true and (has("stack") | not)' >/dev/null
"$TOOL" validate-manifest "$TMP/stack.yml" >"$TMP/stack.json"
jq -e '.publishing == false and .stack.integration_branch == "dev" and (.stack.members | length) == 4' "$TMP/stack.json" >/dev/null

bad_manifest() {
	local label="$1" filter="$2"
	jq "$filter" "$TMP/stack.json" | ruby -rjson -ryaml -e 'puts JSON.parse(STDIN.read).to_yaml' >"$TMP/bad.yml"
	expect_failure "$label" "$TOOL" validate-manifest "$TMP/bad.yml"
}
bad_manifest 'stack block on schema 1' '.schema_version = 1'
bad_manifest 'uppercase train id' '.id = "Key-Contention"'
bad_manifest 'unknown state' '.state = "running"'
bad_manifest 'short head sha' '.head_sha = "abc123"'
bad_manifest 'top member differs from pull_request' '.pull_request = 319'
bad_manifest 'duplicate member' '.stack.members[1].pull_request = 299'
bad_manifest 'single-member stack' '.stack.members = [{pull_request: 311}] | .pull_request = 311'
bad_manifest 'publishing not boolean' '.publishing = "no"'
bad_manifest 'gate without job' '.required_gates = [{id: "x", workflow: "x.yml", binds_image_digest: true}]'
printf 'not: [valid\n' >"$TMP/broken.yml"
expect_failure 'broken yaml' "$TOOL" validate-manifest "$TMP/broken.yml"

# --- Stack inputs: a valid chain 299 -> 312 -> 319 -> 311 ---------------------
IN="$TMP/inputs"
mkdir -p "$IN"
pull() { # number state merged base head_ref head_sha [merge_sha]
	local base_sha
	base_sha="$(sha "base-$4")"
	jq -n --argjson n "$1" --arg state "$2" --argjson merged "$3" --arg base "$4" --arg base_sha "$base_sha" --arg ref "$5" --arg head "$6" --arg merge "${7:-}" '{
		number: $n, state: $state, merged: $merged, base: {ref: $base, sha: $base_sha}, head: {ref: $ref, sha: $head},
		merge_commit_sha: (if $merge == "" then null else $merge end)}' >"$IN/pull-$1.json"
}
compare() { jq -n --arg s "$3" '{status: $s}' >"$IN/compare-$1-$2.json"; }
reach() { jq -n --arg s "$2" '{status: $s}' >"$IN/reach-$1.json"; }
pull 299 open false dev fix/key-contention-starvation "$S299"
pull 312 open false fix/key-contention-starvation feat/key-contention-phase2 "$S312"
pull 319 open false feat/key-contention-phase2 fix/key-contention-base-bias "$S319"
pull 311 open false fix/key-contention-base-bias formal/enhance-cbc-design "$S311" "$M311"
B311="$(jq -r '.base.sha' "$IN/pull-311.json")"
MB311="$(sha synthetic-base)"
jq -n --arg sha "$M311" --arg base "$MB311" --arg head "$S311" \
	'{sha: $sha, parents: [{sha: $base}, {sha: $head}]}' >"$IN/top-merge.json"
jq -n '{status: "ahead"}' >"$IN/top-base.json"
jq -n '{status: "ahead"}' >"$IN/integration-base.json"
compare "$S299" "$S312" ahead
compare "$S312" "$S319" ahead
compare "$S319" "$S311" identical
"$TOOL" validate-stack "$TMP/stack.json" "$IN" "$TMP/train-record.json" 2>/dev/null
IBASE="$(jq -r '.base.sha' "$IN/pull-299.json")"
jq -e --arg top "$S311" --arg s299 "$S299" --arg merge "$M311" --arg base "$B311" --arg merge_base "$MB311" --arg integration_base "$IBASE" '
	.schema_version == 1 and .train_id == "key-contention" and .publishing == false
	and .head_sha == $top and .head_pull_request == 311 and .integration_branch == "dev"
	and .merge_sha == $merge and .base_sha == $base and .merge_base_sha == $merge_base
	and .integration_base_sha == $integration_base
	and (.members | length) == 4
	and .members[0] == {pull_request: 299, head_sha: $s299, base: "dev", merged: false}
	and .members[3].base == "fix/key-contention-base-bias"' "$TMP/train-record.json" >/dev/null
"$TOOL" summarize "$TMP/train-record.json" | grep -q 'rehearsal, non-publishing'

# --- Section 15 rejections ------------------------------------------------------
stack_case() { # label, then a shell snippet that mutates a copy of inputs
	local label="$1" snippet="$2" dir
	dir="$(mktemp -d "$TMP/case-XXXXXX")"
	rmdir "$dir"
	cp -R "$IN" "$dir"
	( IN="$dir"; eval "$snippet" )
	expect_failure "$label" "$TOOL" validate-stack "$TMP/stack.json" "$dir" "$dir/record.json"
}
stack_case 'member base points at the following member (inverted)' \
	'pull 312 open false fix/key-contention-base-bias feat/key-contention-phase2 "$S312"'
stack_case 'bottom member does not target the integration branch' \
	'pull 299 open false master fix/key-contention-starvation "$S299"'
stack_case 'member head is not an ancestor of the following head' \
	'compare "$S312" "$S319" diverged'
stack_case 'member closed without merge' \
	'pull 319 closed false feat/key-contention-phase2 fix/key-contention-base-bias "$S319"'
stack_case 'manifest head_sha differs from the top member head' \
	'pull 311 open false fix/key-contention-base-bias formal/enhance-cbc-design "$OTHER"; compare "$S319" "$OTHER" ahead'
stack_case 'merged member without a merge commit' \
	'pull 299 closed true dev fix/key-contention-starvation "$S299"'
stack_case 'merged member squashed (single parent)' \
	'pull 299 closed true dev fix/key-contention-starvation "$S299" "$OTHER"; jq -n --arg sha "$OTHER" --arg p "$(sha parent)" "{sha: \$sha, parents: [{sha: \$p}]}" >"$IN/merge-299.json"'
stack_case 'merged member merge commit second parent is not the recorded head' \
	'pull 299 closed true dev fix/key-contention-starvation "$S299" "$OTHER"; jq -n --arg sha "$OTHER" --arg p "$(sha parent)" --arg q "$(sha q)" "{sha: \$sha, parents: [{sha: \$p}, {sha: \$q}]}" >"$IN/merge-299.json"; reach 299 identical'
stack_case 'merged member merge commit is not reachable from the integration branch' \
	'pull 299 closed true dev fix/key-contention-starvation "$S299" "$OTHER"; jq -n --arg sha "$OTHER" --arg p "$(sha parent)" --arg q "$S299" "{sha: \$sha, parents: [{sha: \$p}, {sha: \$q}]}" >"$IN/merge-299.json"; reach 299 diverged'
stack_case 'merged member without a reachability document' \
	'pull 299 closed true dev fix/key-contention-starvation "$S299" "$OTHER"; jq -n --arg sha "$OTHER" --arg p "$(sha parent)" --arg q "$S299" "{sha: \$sha, parents: [{sha: \$p}, {sha: \$q}]}" >"$IN/merge-299.json"'
stack_case 'member retargeted to the integration branch while its predecessor is still open' \
	'pull 312 open false dev feat/key-contention-phase2 "$S312"'
stack_case 'empty pull request document' \
	': >"$IN/pull-312.json"'
stack_case 'top synthetic merge changed after observation' \
	'pull 311 open false fix/key-contention-base-bias formal/enhance-cbc-design "$S311" "$OTHER"'
stack_case 'top synthetic merge base does not contain the logical base' \
	'jq -n --arg sha "$M311" --arg base "$OTHER" --arg head "$S311" "{sha: \$sha, parents: [{sha: \$base}, {sha: \$head}]}" >"$IN/top-merge.json"; jq -n "{status: \"diverged\"}" >"$IN/top-base.json"'
stack_case 'top synthetic merge job input is missing' \
	'rm "$IN/top-merge.json"'
stack_case 'top synthetic base ancestry input is missing' \
	'rm "$IN/top-base.json"'
stack_case 'integration base is stale' \
	'jq -n "{status: \"behind\"}" >"$IN/integration-base.json"'
stack_case 'integration base ancestry input is missing' \
	'rm "$IN/integration-base.json"'

# A correctly merged bottom member passes.
MERGED="$TMP/merged"
cp -R "$IN" "$MERGED"
M299="$(sha merge299)"
( IN="$MERGED"; pull 299 closed true dev fix/key-contention-starvation "$S299" "$M299" )
jq -n --arg sha "$M299" --arg p "$(sha devtip)" --arg q "$S299" '{sha: $sha, parents: [{sha: $p}, {sha: $q}]}' >"$MERGED/merge-299.json"
( IN="$MERGED"; reach 299 ahead )
"$TOOL" validate-stack "$TMP/stack.json" "$MERGED" "$MERGED/record.json" 2>/dev/null
jq -e '.members[0].merged == true and .members[1].base == "fix/key-contention-starvation"' "$MERGED/record.json" >/dev/null
# After the merged member's branch is deleted, GitHub retargets the next
# member to the integration branch (Section 13.3). That base is accepted and
# recorded as observed.
RETARGET="$TMP/retarget"
cp -R "$MERGED" "$RETARGET"
( IN="$RETARGET"; pull 312 open false dev feat/key-contention-phase2 "$S312" )
"$TOOL" validate-stack "$TMP/stack.json" "$RETARGET" "$RETARGET/record.json" 2>/dev/null
jq -e '.members[0].merged == true and .members[1].base == "dev" and .members[2].base == "feat/key-contention-phase2"' "$RETARGET/record.json" >/dev/null

# --- Version and reservation ---------------------------------------------------
SRC="$TMP/source"
mkdir -p "$SRC/node" "$SRC/.github" "$TMP/manifests"
printf '%s\n' '[package]' 'name = "node"' 'version = "0.4.46"' >"$SRC/node/Cargo.toml"
printf '%s\n' 'FROM scratch' 'LABEL version="0.4.46"' >"$SRC/node/Dockerfile"
printf '%s\n' 'version = 4' '[[package]]' 'name = "node"' 'version = "0.4.46"' >"$SRC/Cargo.lock"
printf 'SYSTEM_INTEGRATION_REF=0123456789abcdef0123456789abcdef01234567\n' >"$SRC/.github/oci-validation.env"
git -C "$SRC" init -q
git -C "$SRC" config user.name t
git -C "$SRC" config user.email t@example.com
git -C "$SRC" add .
git -C "$SRC" commit -qm fixture
git -C "$SRC" tag v0.4.46
# Non-publishing rehearsal: the version only has to match, even when a
# stable tag already exists for it.
"$TOOL" validate-version "$TMP/stack.json" "$SRC" "$TMP/manifests" 2>/dev/null
jq '.target_version = "0.4.47"' "$TMP/stack.json" >"$TMP/stack-mismatch.json"
expect_failure 'source version differs from target_version' "$TOOL" validate-version "$TMP/stack-mismatch.json" "$SRC" "$TMP/manifests"
# Publishing train: must be release-eligible and unreserved.
jq '.publishing = true' "$TMP/stack.json" >"$TMP/stack-publishing.json"
expect_failure 'publishing train on an already-stable version' "$TOOL" validate-version "$TMP/stack-publishing.json" "$SRC" "$TMP/manifests"
git -C "$SRC" tag -d v0.4.46 >/dev/null
git -C "$SRC" tag v0.4.45
"$TOOL" validate-version "$TMP/stack-publishing.json" "$SRC" "$TMP/manifests" 2>/dev/null
cat >"$TMP/manifests/other.yml" <<EOF
schema_version: 1
id: other-train
state: active
target_version: 0.4.46
pull_request: 1
head_sha: $OTHER
base_branch: master
required_gates: []
EOF
expect_failure 'version reserved by another active train' "$TOOL" validate-version "$TMP/stack-publishing.json" "$SRC" "$TMP/manifests"
sed -i.bak 's/^state: active$/state: promoted/' "$TMP/manifests/other.yml"
"$TOOL" validate-version "$TMP/stack-publishing.json" "$SRC" "$TMP/manifests" 2>/dev/null

# --- CI planning ----------------------------------------------------------------
TITLE="CI [exact merge $M311]"
jq -n --arg title "$TITLE" --arg control "$CONTROL" --arg repo "$REPOSITORY" '{workflow_runs: [
	{id: 1, run_number: 10, head_sha: $control, head_branch: "master", display_title: $title, event: "workflow_dispatch", path: ".github/workflows/ci.yml", status: "completed", conclusion: "success", repository: {full_name: $repo}, head_repository: {full_name: $repo}},
	{id: 2, run_number: 11, head_sha: $control, head_branch: "master", display_title: $title, event: "workflow_dispatch", path: ".github/workflows/ci.yml", status: "completed", conclusion: "failure", repository: {full_name: $repo}, head_repository: {full_name: $repo}},
	{id: 3, run_number: 12, head_sha: $control, head_branch: "master", display_title: $title, event: "workflow_dispatch", path: ".github/workflows/ci.yml", status: "completed", conclusion: "success", repository: {full_name: $repo}, head_repository: {full_name: $repo}}]}' >"$TMP/runs.json"
[ "$("$TOOL" plan-ci "$TMP/runs.json" "$M311" "$REPOSITORY" master "$CONTROL")" = 3 ]
[ "$("$TOOL" plan-ci "$TMP/runs.json" "$OTHER" "$REPOSITORY" master "$CONTROL")" = dispatch ]
jq '.workflow_runs |= map(.event = "pull_request")' "$TMP/runs.json" >"$TMP/runs-lightweight.json"
[ "$("$TOOL" plan-ci "$TMP/runs-lightweight.json" "$M311" "$REPOSITORY" master "$CONTROL")" = dispatch ]
jq --arg other "$OTHER" '.workflow_runs |= map(.head_sha = $other)' "$TMP/runs.json" >"$TMP/runs-untrusted-control.json"
[ "$("$TOOL" plan-ci "$TMP/runs-untrusted-control.json" "$M311" "$REPOSITORY" master "$CONTROL")" = dispatch ]
jq '.workflow_runs |= map(.head_repository.full_name = "fork/repo")' "$TMP/runs.json" >"$TMP/runs-fork.json"
[ "$("$TOOL" plan-ci "$TMP/runs-fork.json" "$M311" "$REPOSITORY" master "$CONTROL")" = dispatch ]
jq '.workflow_runs |= map(del(.head_repository))' "$TMP/runs.json" >"$TMP/runs-nohead.json"
[ "$("$TOOL" plan-ci "$TMP/runs-nohead.json" "$M311" "$REPOSITORY" master "$CONTROL")" = dispatch ]
jq '.workflow_runs |= map(.status = "in_progress" | .conclusion = null) | .workflow_runs[2].id = 5 | .workflow_runs[2].run_number = 14' "$TMP/runs.json" >"$TMP/runs-active.json"
[ "$("$TOOL" plan-ci "$TMP/runs-active.json" "$M311" "$REPOSITORY" master "$CONTROL")" = wait:5 ]

jq -n --arg repo "$REPOSITORY" --arg merge "$M311" --arg head "$S311" --arg base "$B311" --arg merge_base "$MB311" '{
	schema_version: 1, repository: $repo, workflow: ".github/workflows/ci.yml", event: "workflow_dispatch",
	target_sha: $merge, top_pull_request: 311, head_sha: $head, base_sha: $base, merge_base_sha: $merge_base, run_id: 3, run_attempt: 1
}' >"$TMP/ci-target.json"
jq -n '{jobs: [
	{name: "Integration Tests (amd64)", status: "completed", conclusion: "success"},
	{name: "Integration Tests (arm64)", status: "completed", conclusion: "success"}
]}' >"$TMP/ci-jobs.json"
"$TOOL" validate-ci-evidence "$TMP/ci-target.json" "$TMP/ci-jobs.json" "$M311" 311 "$S311" "$B311" "$MB311" 3 1 "$REPOSITORY"
jq '.jobs[1].conclusion = "skipped"' "$TMP/ci-jobs.json" >"$TMP/ci-jobs-skipped.json"
expect_failure 'skipped Heavy Pipeline aggregator' "$TOOL" validate-ci-evidence "$TMP/ci-target.json" "$TMP/ci-jobs-skipped.json" "$M311" 311 "$S311" "$B311" "$MB311" 3 1 "$REPOSITORY"
jq '.jobs = [.jobs[0]]' "$TMP/ci-jobs.json" >"$TMP/ci-jobs-missing.json"
expect_failure 'missing Heavy Pipeline aggregator' "$TOOL" validate-ci-evidence "$TMP/ci-target.json" "$TMP/ci-jobs-missing.json" "$M311" 311 "$S311" "$B311" "$MB311" 3 1 "$REPOSITORY"
jq '.jobs += [.jobs[0]]' "$TMP/ci-jobs.json" >"$TMP/ci-jobs-duplicate.json"
expect_failure 'duplicate Heavy Pipeline aggregator' "$TOOL" validate-ci-evidence "$TMP/ci-target.json" "$TMP/ci-jobs-duplicate.json" "$M311" 311 "$S311" "$B311" "$MB311" 3 1 "$REPOSITORY"
expect_failure 'CI target binds a stale merge' "$TOOL" validate-ci-evidence "$TMP/ci-target.json" "$TMP/ci-jobs.json" "$OTHER" 311 "$S311" "$B311" "$MB311" 3 1 "$REPOSITORY"
expect_failure 'CI target binds a stale synthetic base' "$TOOL" validate-ci-evidence "$TMP/ci-target.json" "$TMP/ci-jobs.json" "$M311" 311 "$S311" "$B311" "$OTHER" 3 1 "$REPOSITORY"

printf 'release train tests passed\n'
