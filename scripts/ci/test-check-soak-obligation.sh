#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${1:?An evidence directory is required.}"
[[ ! -e "$OUT" ]] || exit 2
mkdir -p "$OUT/api" "$OUT/tools" "$OUT/cases" "$OUT/runs"
OUT="$(cd "$OUT" && pwd)"
CHECK="$ROOT/scripts/ci/check-soak-obligation.sh"
API=repos/F1R3FLY-io/f1r3node-rust
HARNESS="$(printf '1%.0s' {1..40})"
NODE="$(printf '2%.0s' {1..40})"
REQUIRED="$(printf '3%.0s' {1..40})"
MASTER="$(printf '4%.0s' {1..40})"
OTHER="$(printf '5%.0s' {1..40})"
MIXED="$(printf '6%.0s' {1..40})"
FULL="$(printf '7%.0s' {1..40})"
EVIDENCE='.seeds=[11,12] | .started_epoch=1800000000 | .finished_epoch=1800216004'
HEX="$(printf 'a%.0s' {1..64})"
export FIXTURE_API="$OUT/api"
hash() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f1
  else
    shasum -a 256 "$1" | cut -d ' ' -f1
  fi
}
size() { wc -c < "$1" | tr -d '[:space:]'; }
key() { printf '%s' "$1" | tr -c 'A-Za-z0-9' '_'; }
serve() { cp "$2" "$FIXTURE_API/$(key "$1")"; }
passed=0
expect() {
  local name="$1" expected="$2" actual
  shift 2
  set +e
  "$@" > "$OUT/cases/$name.stdout" 2> "$OUT/cases/$name.stderr"
  actual=$?
  set -e
  if [[ "$actual" != "$expected" ]]; then
    printf 'FAILED %s: expected exit %s, actual exit %s\n' "$name" "$expected" "$actual" >&2
    cat "$OUT/cases/$name.stderr" >&2
    exit 1
  fi
  passed=$((passed + 1))
}
verify() {
  local name="$1"
  shift
  if ! "$@" >/dev/null 2>&1; then
    printf 'FAILED %s: the expected content is absent\n' "$name" >&2
    exit 1
  fi
  passed=$((passed + 1))
}
make_run() {
  jq -n --argjson id "$1" --arg revision "$HARNESS" '
    {id:$id,run_attempt:1,path:".github/workflows/merge-recovery-soak.yml",
     repository:{full_name:"F1R3FLY-io/f1r3node-rust"},head_sha:$revision,
     event:"workflow_dispatch",status:"completed",conclusion:"success"}'
}
make_result() {
  jq -n --arg id "$1" --arg arch "$2" --arg node "$NODE" --arg harness "$HARNESS" --arg hex "$HEX" '
    {schema_version:1,repository:"F1R3FLY-io/f1r3node-rust",run_id:$id,run_attempt:1,
     plan:{stage:"stability",candidate_id:("dev-"+$arch),platform:("linux/"+$arch),
       phase:"post_pr216_merge",node_revision:$node,harness_revision:$harness,
       image_digest:("sha256:"+$hex),image_config_digest:("sha256:"+$hex),
       node_binary_digest:("sha256:"+$hex),workload:{path:"docs/campaign/workload.json",sha256:$hex},
       duration_seconds:216000},
     result:"passed",admission:"admitted",evidence_kind:"node_observation",cloud_launch_count:1,
     node_launch_count:4,cleanup:{complete:true},host_protection:{status:"passed"},
     integration_preflight:"passed",soak_verdict:"passed",workload_elapsed_seconds:216004,
     measurement_completeness:"complete",product_failures:[],infrastructure_failures:[]}'
}
make_facts() {
  jq -n --arg result "$(hash "$1")" --arg archive "$2" --arg node "$NODE" --arg required "$REQUIRED" --arg master "$MASTER" '
    {schema_version:1,required_commit:$required,required_commit_source:"pull_request",
     master_revision:$master,
     revision_in_master:{base:$node,head:$master,status:"ahead",behind_by:0},
     required_in_revision:{base:$required,head:$node,status:"ahead",behind_by:0},
     result_archive_sha256:$archive,result_sha256:$result}'
}
make_artifact() {
  jq -n --argjson id "$1" --argjson artifact "$2" --arg digest "sha256:$3" --argjson size "$4" --arg revision "$HARNESS" '
    {id:$artifact,name:("casper-campaign-result-"+($id|tostring)+"-1"),expired:false,
     size_in_bytes:$size,digest:$digest,workflow_run:{id:$id,head_sha:$revision}}'
}
build() {
  local name="$1" arch="$2" run_filter="$3" result_filter="$4" facts_filter="$5" artifact_filter="$6" dir="$OUT/cases/$1"
  mkdir "$dir"
  make_run 101 | jq "$run_filter" > "$dir/run.json"
  make_result 101 "$arch" | jq "$result_filter" > "$dir/result.json"
  make_facts "$dir/result.json" "$HEX" | jq "$facts_filter" > "$dir/facts.json"
  make_artifact 101 301 "$HEX" 512 | jq "$artifact_filter" > "$dir/artifact.json"
}
evaluate() {
  local name="$1" expected="$2"
  expect "$name" "$expected" bash "$CHECK" evaluate "$OUT/cases/$name/run.json" "$OUT/cases/$name/artifact.json" \
    "$OUT/cases/$name/result.json" "$OUT/cases/$name/facts.json" "$OUT/cases/$name/evaluation"
}
unsatisfied() {
  local name="$1" criterion="$2"
  evaluate "$name" 3
  verify "$name-criterion" jq -e --argjson id "$criterion" '
    .verdict=="not_satisfied" and ([.criteria[]|select(.satisfied==false)|.id]==[$id])
  ' "$OUT/cases/$name/evaluation/verdict.json"
}

build satisfied_amd64 amd64 . . . .
evaluate satisfied_amd64 0
verify satisfied_amd64_record jq -e --arg node "$NODE" '
  .verdict=="satisfied" and .architecture=="amd64" and .node_revision==$node and
  .run_id=="101" and all(.criteria[];.satisfied) and (.criteria|length)==4
' "$OUT/cases/satisfied_amd64/evaluation/verdict.json"
build satisfied_arm64 arm64 '.id=102' '.run_id="102"' . '.name="casper-campaign-result-102-1" | .workflow_run.id=102'
evaluate satisfied_arm64 0
build identical_revision amd64 . . '.revision_in_master.status="identical" | .required_in_revision.status="identical"' .
evaluate identical_revision 0
verify evidence_absent jq -e '
  .unavailable_evidence==["seeds","workload_window_start","workload_window_end"] and
  .seeds==null and .workload_window_start==null and .workload_window_end==null
' "$OUT/cases/satisfied_amd64/evaluation/verdict.json"
build evidence_amd64 amd64 '.id=111' ".run_id=\"111\" | $EVIDENCE" . '.name="casper-campaign-result-111-1" | .workflow_run.id=111'
evaluate evidence_amd64 0
build evidence_arm64 arm64 '.id=112' ".run_id=\"112\" | $EVIDENCE" . '.name="casper-campaign-result-112-1" | .workflow_run.id=112'
evaluate evidence_arm64 0
verify evidence_present jq -e '
  .unavailable_evidence==[] and .seeds==[11,12] and
  .workload_window_start==1800000000 and .workload_window_end==1800216004
' "$OUT/cases/evidence_amd64/evaluation/verdict.json"
build evidence_seeds_only amd64 . '.seeds=[7]' . .
evaluate evidence_seeds_only 0
verify evidence_seeds_only_record jq -e '.unavailable_evidence==["workload_window_start","workload_window_end"]' \
  "$OUT/cases/evidence_seeds_only/evaluation/verdict.json"

while IFS=';' read -r name criterion run_filter result_filter facts_filter; do
  build "$name" amd64 "$run_filter" "$result_filter" "$facts_filter" .
  unsatisfied "$name" "$criterion"
done <<'CASES'
required_diverged;1;.;.;.required_in_revision.status="diverged" | .required_in_revision.behind_by=2
required_behind;1;.;.;.required_in_revision.status="behind" | .required_in_revision.behind_by=1
required_ahead_with_missing_commits;1;.;.;.required_in_revision.behind_by=1
revision_not_on_master;1;.;.;.revision_in_master.status="diverged" | .revision_in_master.behind_by=4
source_not_merged;1;.;.;.required_commit=null | .required_in_revision=null | .required_commit_source="pull_request_not_merged"
scheduled_event;2;.event="schedule";.;.
shortened_window;3;.;.plan.duration_seconds=86400 | .workload_elapsed_seconds=86400;.
elapsed_below_window;3;.;.workload_elapsed_seconds=215999;.
elapsed_fraction;3;.;.workload_elapsed_seconds=216000.5;.
elapsed_missing;3;.;.workload_elapsed_seconds=null;.
measurement_partial;3;.;.measurement_completeness="partial";.
run_failed;4;.conclusion="failure";.;.
run_cancelled;4;.conclusion="cancelled";.;.
run_second_attempt;4;.run_attempt=2;.;.
result_not_passed;4;.;.result="non_passing" | .soak_verdict="non_passing";.
verdict_not_passed;4;.;.soak_verdict="non_passing";.
admission_blocked;4;.;.admission="blocked";.
evidence_not_observed;4;.;.evidence_kind="synthetic";.
no_cloud_launch;4;.;.cloud_launch_count=0;.
no_node_launch;4;.;.node_launch_count=0;.
cleanup_incomplete;4;.;.cleanup.complete=false;.
host_protection_failed;4;.;.host_protection.status="failed";.
CASES

while IFS=';' read -r name run_filter result_filter facts_filter artifact_filter; do
  build "$name" amd64 "$run_filter" "$result_filter" "$facts_filter" "$artifact_filter"
  evaluate "$name" 2
  verify "$name-no-record" test ! -e "$OUT/cases/$name/evaluation"
done <<'CASES'
run_identifier_differs;.id=102;.;.;.
repository_differs;.repository.full_name="other/repository";.;.;.
workflow_differs;.path=".github/workflows/other.yml";.;.;.
result_repository_differs;.;.repository="other/repository";.;.
result_run_differs;.;.run_id="102";.;.
result_schema_differs;.;.schema_version=2;.;.
plan_missing;.;del(.plan);.;.
artifact_name_differs;.;.;.;.name="casper-campaign-result-102-1"
artifact_run_differs;.;.;.;.workflow_run.id=102
artifact_expired;.;.;.;.expired=true
artifact_digest_differs;.;.;.;.digest="sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
result_digest_differs;.;.;.result_sha256="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";.
revision_malformed;.;.plan.node_revision="HEAD";.;.
revision_short;.;.plan.node_revision="2222222";.;.
platform_unknown;.;.plan.platform="linux/riscv64";.;.
candidate_with_text;.;.plan.candidate_id="dev-amd64 @maintainer";.;.
candidate_with_markup;.;.plan.candidate_id="dev-amd64|x";.;.
phase_with_text;.;.plan.phase="post merge";.;.
image_digest_malformed;.;.plan.image_digest="latest";.;.
workload_path_with_markup;.;.plan.workload.path="docs/[x](y)";.;.
duration_text;.;.plan.duration_seconds="216000";.;.
facts_for_other_revision;.;.;.revision_in_master.base="5555555555555555555555555555555555555555";.
facts_for_other_master;.;.;.revision_in_master.head="5555555555555555555555555555555555555555";.
required_for_other_revision;.;.;.required_in_revision.head="5555555555555555555555555555555555555555";.
required_without_comparison;.;.;.required_in_revision=null;.
required_source_unknown;.;.;.required_commit_source="manual";.
comparison_status_unknown;.;.;.revision_in_master.status="unknown";.
comparison_count_negative;.;.;.revision_in_master.behind_by=-1;.
seeds_empty;.;.seeds=[];.;.
seeds_text;.;.seeds=["11"];.;.
seeds_not_a_list;.;.seeds=11;.;.
seeds_negative;.;.seeds=[-1];.;.
window_start_only;.;.started_epoch=1800000000;.;.
window_end_only;.;.finished_epoch=1800216004;.;.
window_reversed;.;.started_epoch=1800216004 | .finished_epoch=1800000000;.;.
window_text;.;.started_epoch="2027-01-15" | .finished_epoch="2027-01-18";.;.
CASES

build stage_baseline amd64 . '.plan.stage="baseline" | .plan.duration_seconds=86400' . .
evaluate stage_baseline 4
build stage_preflight amd64 . '.plan.stage="preflight" | .plan.duration_seconds=0' . .
evaluate stage_preflight 4
mkdir "$OUT/cases/two_objects"
cp "$OUT/cases/satisfied_amd64/run.json" "$OUT/cases/satisfied_amd64/artifact.json" "$OUT/cases/two_objects/"
cat "$OUT/cases/satisfied_amd64/result.json" "$OUT/cases/satisfied_amd64/result.json" > "$OUT/cases/two_objects/result.json"
make_facts "$OUT/cases/two_objects/result.json" "$HEX" > "$OUT/cases/two_objects/facts.json"
evaluate two_objects 2
expect existing_output 2 bash "$CHECK" evaluate "$OUT/cases/satisfied_amd64/run.json" "$OUT/cases/satisfied_amd64/artifact.json" \
  "$OUT/cases/satisfied_amd64/result.json" "$OUT/cases/satisfied_amd64/facts.json" "$OUT/cases/satisfied_amd64/evaluation"
expect unknown_command 2 bash "$CHECK" unknown

AMD="$OUT/cases/satisfied_amd64/evaluation/verdict.json"
ARM="$OUT/cases/satisfied_arm64/evaluation/verdict.json"
expect comment_satisfied 0 bash "$CHECK" comment "$AMD"
verify comment_marker grep -Fx "<!-- soak-obligation O1 run=101 architecture=amd64 revision=$NODE verdict=satisfied -->" "$OUT/cases/comment_satisfied.stdout"
verify comment_unavailable grep -F 'seeds, workload_window_start, workload_window_end' "$OUT/cases/comment_satisfied.stdout"
expect comment_evidence 0 bash "$CHECK" comment "$OUT/cases/evidence_amd64/evaluation/verdict.json"
verify comment_seeds grep -Fx '| Seeds | 11, 12 |' "$OUT/cases/comment_evidence.stdout"
verify comment_window_start grep -Fx '| Start of the workload window | 2027-01-15T08:00:00Z |' "$OUT/cases/comment_evidence.stdout"
verify comment_window_end grep -Fx '| End of the workload window | 2027-01-17T20:00:04Z |' "$OUT/cases/comment_evidence.stdout"
verify comment_no_absent_items sh -c '! grep -F "does not contain" "$1"' sh "$OUT/cases/comment_evidence.stdout"
expect comment_failed 0 bash "$CHECK" comment "$OUT/cases/run_failed/evaluation/verdict.json"
verify comment_failed_marker grep -F 'verdict=not_satisfied -->' "$OUT/cases/comment_failed.stdout"
verify comment_failed_text grep -F 'The obligation stays open.' "$OUT/cases/comment_failed.stdout"
verify comment_failed_row grep -F '| 4 | The terminal verdict is a pass. | no |' "$OUT/cases/comment_failed.stdout"
verify comment_no_mention sh -c '! grep -E "@[A-Za-z]" "$1"' sh "$OUT/cases/comment_satisfied.stdout"

expect pair_complete 0 bash "$CHECK" pair "$AMD" "$ARM" "$OUT/cases/pair_complete-out"
verify pair_record jq -e --arg node "$NODE" '
  .status=="complete" and .node_revision==$node and ([.runs[].architecture]==["amd64","arm64"]) and
  ([.runs[].run_id]==["101","102"])
' "$OUT/cases/pair_complete-out/obligation.json"
verify pair_evidence_absent jq -e '
  .evidence_complete==false and .unavailable_evidence==["seeds","workload_window_end","workload_window_start"]
' "$OUT/cases/pair_complete-out/obligation.json"
expect pair_reversed 0 bash "$CHECK" pair "$ARM" "$AMD" "$OUT/cases/pair_reversed-out"
expect pair_evidence 0 bash "$CHECK" pair "$OUT/cases/evidence_amd64/evaluation/verdict.json" \
  "$OUT/cases/evidence_arm64/evaluation/verdict.json" "$OUT/cases/pair_evidence-out"
verify pair_evidence_complete jq -e '.evidence_complete==true and .unavailable_evidence==[]' "$OUT/cases/pair_evidence-out/obligation.json"
jq '.run_id="113"' "$OUT/cases/evidence_seeds_only/evaluation/verdict.json" > "$OUT/cases/amd64-seeds-only.json"
expect pair_evidence_partial 0 bash "$CHECK" pair "$OUT/cases/amd64-seeds-only.json" \
  "$OUT/cases/evidence_arm64/evaluation/verdict.json" "$OUT/cases/pair_partial-out"
verify pair_evidence_partial_record jq -e '
  .evidence_complete==false and .unavailable_evidence==["workload_window_end","workload_window_start"]
' "$OUT/cases/pair_partial-out/obligation.json"
expect pair_evidence_partial_second 0 bash "$CHECK" pair "$OUT/cases/evidence_arm64/evaluation/verdict.json" \
  "$OUT/cases/amd64-seeds-only.json" "$OUT/cases/pair_partial_second-out"
verify pair_evidence_partial_second_record jq -e '
  .evidence_complete==false and .unavailable_evidence==["workload_window_end","workload_window_start"]
' "$OUT/cases/pair_partial_second-out/obligation.json"
expect pair_same_architecture 3 bash "$CHECK" pair "$AMD" "$AMD" "$OUT/cases/pair_same-out"
jq '.run_id="103"' "$AMD" > "$OUT/cases/amd64-second.json"
expect pair_two_amd64 3 bash "$CHECK" pair "$AMD" "$OUT/cases/amd64-second.json" "$OUT/cases/pair_two-out"
jq --arg other "$OTHER" '.node_revision=$other' "$ARM" > "$OUT/cases/arm64-other-revision.json"
expect pair_other_revision 3 bash "$CHECK" pair "$AMD" "$OUT/cases/arm64-other-revision.json" "$OUT/cases/pair_revision-out"
jq --arg other "$OTHER" '.required_commit=$other' "$ARM" > "$OUT/cases/arm64-other-required.json"
expect pair_other_required 3 bash "$CHECK" pair "$AMD" "$OUT/cases/arm64-other-required.json" "$OUT/cases/pair_required-out"
jq '.verdict="not_satisfied"' "$ARM" > "$OUT/cases/arm64-not-satisfied.json"
expect pair_not_satisfied 3 bash "$CHECK" pair "$AMD" "$OUT/cases/arm64-not-satisfied.json" "$OUT/cases/pair_unsatisfied-out"
jq '.run_id="101"' "$ARM" > "$OUT/cases/arm64-same-run.json"
expect pair_same_run 3 bash "$CHECK" pair "$AMD" "$OUT/cases/arm64-same-run.json" "$OUT/cases/pair_run-out"
expect completion_text 0 bash "$CHECK" completion "$OUT/cases/pair_complete-out/obligation.json"
verify completion_marker grep -Fx "<!-- soak-obligation O1 complete revision=$NODE -->" "$OUT/cases/completion_text.stdout"
verify completion_no_mark grep -F 'The check does not mark obligation O1.' "$OUT/cases/completion_text.stdout"
expect completion_evidence 0 bash "$CHECK" completion "$OUT/cases/pair_evidence-out/obligation.json"
verify completion_mark grep -F 'The check marks obligation O1 in the issue text.' "$OUT/cases/completion_evidence.stdout"

cat > "$OUT/cases/issue-body.md" <<'BODY'
## Obligations

- [ ] **O1. 60-hour stability soak.** Tracker task: TASK-018-7.
- [ ] **O2. Merge gate and post-merge baseline.** Tracker task: TASK-018-1.
- [ ] **O7. Formal verification gate as a required check on `dev`.**

### O1: preconditions

- [ ] #436 is merged, and its merge commit is an ancestor of `master`.
BODY
expect mark_open 0 bash "$CHECK" mark "$OUT/cases/issue-body.md"
verify mark_line grep -Fx -- '- [x] **O1. 60-hour stability soak.** Tracker task: TASK-018-7.' "$OUT/cases/mark_open.stdout"
verify mark_one_change test "$(diff "$OUT/cases/issue-body.md" "$OUT/cases/mark_open.stdout" | grep -c '^[<>]')" = 2
verify mark_others test "$(grep -c -- '^- \[ \] ' "$OUT/cases/mark_open.stdout")" = 3
expect mark_again 0 bash "$CHECK" mark "$OUT/cases/mark_open.stdout"
verify mark_idempotent cmp "$OUT/cases/mark_open.stdout" "$OUT/cases/mark_again.stdout"
{ cat "$OUT/cases/issue-body.md"; printf -- '- [ ] **O1. A second line.**\n'; } > "$OUT/cases/issue-duplicate.md"
expect mark_duplicate 2 bash "$CHECK" mark "$OUT/cases/issue-duplicate.md"
{ cat "$OUT/cases/mark_open.stdout"; printf -- '- [ ] **O1. A second line.**\n'; } > "$OUT/cases/issue-mixed.md"
expect mark_mixed 2 bash "$CHECK" mark "$OUT/cases/issue-mixed.md"
grep -v 'O1\.' "$OUT/cases/issue-body.md" > "$OUT/cases/issue-absent.md"
expect mark_absent 2 bash "$CHECK" mark "$OUT/cases/issue-absent.md"
expect mark_missing_file 2 bash "$CHECK" mark "$OUT/cases/no-such-file.md"

cat > "$OUT/tools/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == api && "$2" == --hostname && "$3" == github.com && "$4" == --method ]] || exit 98
method="$5"
shift 5
printf '%s %s\n' "$method" "$*" >> "$FIXTURE_API/calls.txt"
key() { printf '%s' "$1" | tr -c 'A-Za-z0-9' '_'; }
issue=repos/F1R3FLY-io/f1r3node-rust/issues/473
case "$method" in
  GET)
    [[ "$1" != --paginate ]] || shift
    [[ -f "$FIXTURE_API/$(key "$1")" ]] || exit 97
    cat "$FIXTURE_API/$(key "$1")" ;;
  POST)
    [[ "$1" == "$issue/comments" && "$2" == --input ]] || exit 96
    store="$FIXTURE_API/$(key "$issue/comments?per_page=100")"
    jq --slurpfile p "$3" '. + [{user:{login:"github-actions[bot]"},body:$p[0].body}]' "$store" > "$store.new"
    mv "$store.new" "$store"
    printf '{}\n' ;;
  PATCH)
    [[ "$1" == "$issue" && "$2" == --input ]] || exit 96
    store="$FIXTURE_API/$(key "$issue")"
    jq --slurpfile p "$3" '.body=$p[0].body' "$store" > "$store.new"
    mv "$store.new" "$store"
    printf '{}\n' ;;
  *) exit 95 ;;
esac
EOF
chmod +x "$OUT/tools/gh"
export PATH="$OUT/tools:$PATH"
COMMENTS="$FIXTURE_API/$(key "$API/issues/473/comments?per_page=100")"
ISSUE="$FIXTURE_API/$(key "$API/issues/473")"
printf '[]\n' > "$COMMENTS"
jq -n --rawfile body "$OUT/cases/issue-body.md" '{state:"open",body:$body}' > "$ISSUE"
jq -n --arg revision "$REQUIRED" '{merged:true,merge_commit_sha:$revision}' > "$OUT/runs/pull-merged.json"
jq -n '{merged:false,merge_commit_sha:null}' > "$OUT/runs/pull-open.json"
serve "$API/pulls/436" "$OUT/runs/pull-merged.json"
jq -n --arg revision "$MASTER" '{ref:"refs/heads/master",object:{sha:$revision,type:"commit"}}' > "$OUT/runs/master.json"
serve "$API/git/ref/heads/master" "$OUT/runs/master.json"
jq -n '{status:"ahead",ahead_by:3,behind_by:0,commits:[],files:[]}' > "$OUT/runs/compare.json"
serve "$API/compare/$NODE...$MASTER?per_page=1" "$OUT/runs/compare.json"
serve "$API/compare/$REQUIRED...$NODE?per_page=1" "$OUT/runs/compare.json"
for revision in "$MIXED" "$FULL"; do
  serve "$API/compare/$revision...$MASTER?per_page=1" "$OUT/runs/compare.json"
  serve "$API/compare/$REQUIRED...$revision?per_page=1" "$OUT/runs/compare.json"
done
publish_run() {
  local id="$1" artifact="$2" arch="$3" run_filter="$4" result_filter="$5" dir="$OUT/runs/$1"
  mkdir "$dir"
  make_run "$id" | jq "$run_filter" > "$dir/run.json"
  make_result "$id" "$arch" | jq "$result_filter" > "$dir/campaign-result.json"
  (cd "$dir" && zip -q -j result.zip campaign-result.json)
  make_artifact "$id" "$artifact" "$(hash "$dir/result.zip")" "$(size "$dir/result.zip")" > "$dir/artifact.json"
  jq -n --slurpfile a "$dir/artifact.json" '{total_count:1,artifacts:$a}' > "$dir/artifacts.json"
  serve "$API/actions/runs/$id" "$dir/run.json"
  serve "$API/actions/runs/$id/artifacts?per_page=100" "$dir/artifacts.json"
  serve "$API/actions/artifacts/$artifact/zip" "$dir/result.zip"
}
count() { jq --arg text "$1" '[.[]|select(.body|contains($text))]|length' "$COMMENTS"; }
writes() { grep -c -E "^$1 " "$FIXTURE_API/calls.txt" || true; }

make_run 501 > "$OUT/runs/run-501.json"
serve "$API/actions/runs/501" "$OUT/runs/run-501.json"
jq -n '{total_count:0,artifacts:[]}' > "$OUT/runs/artifacts-501.json"
serve "$API/actions/runs/501/artifacts?per_page=100" "$OUT/runs/artifacts-501.json"
expect nightly_run 0 bash "$CHECK" execute 501 "$OUT/runs/execute-501"
verify nightly_no_write test "$(writes POST)" = 0
make_run 502 | jq '.path=".github/workflows/ci.yml"' > "$OUT/runs/run-502.json"
serve "$API/actions/runs/502" "$OUT/runs/run-502.json"
expect other_workflow 0 bash "$CHECK" execute 502 "$OUT/runs/execute-502"
verify other_workflow_no_write test "$(writes POST)" = 0
publish_run 503 703 amd64 . '.plan.stage="baseline" | .plan.duration_seconds=86400'
expect baseline_run 0 bash "$CHECK" execute 503 "$OUT/runs/execute-503"
verify baseline_no_write test "$(writes POST)" = 0
make_run 504 | jq '.status="in_progress" | .conclusion=null' > "$OUT/runs/run-504.json"
serve "$API/actions/runs/504" "$OUT/runs/run-504.json"
expect run_in_progress 2 bash "$CHECK" execute 504 "$OUT/runs/execute-504"
expect run_unknown 2 bash "$CHECK" execute 599 "$OUT/runs/execute-599"
expect run_identifier_text 2 bash "$CHECK" execute 'abc' "$OUT/runs/execute-text"
expect required_commit_text 2 bash "$CHECK" execute 501 "$OUT/runs/execute-required-text" HEAD

publish_run 601 801 amd64 . .
expect first_architecture 0 bash "$CHECK" execute 601 "$OUT/runs/execute-601"
verify first_comment test "$(count 'run=601 architecture=amd64')" = 1
verify first_satisfied test "$(count 'verdict=satisfied')" = 1
verify first_no_completion test "$(count 'O1 complete')" = 0
verify first_no_mark test "$(writes PATCH)" = 0
expect first_architecture_again 0 bash "$CHECK" execute 601 "$OUT/runs/execute-601-again"
verify first_comment_once test "$(count 'run=601 architecture=amd64')" = 1

publish_run 602 802 arm64 '.conclusion="failure"' '.result="non_passing" | .soak_verdict="non_passing" | .workload_elapsed_seconds=3600'
expect failed_architecture 0 bash "$CHECK" execute 602 "$OUT/runs/execute-602"
verify failed_comment test "$(count 'run=602 architecture=arm64')" = 1
verify failed_verdict test "$(count "run=602 architecture=arm64 revision=$NODE verdict=not_satisfied")" = 1
verify failed_no_completion test "$(count 'O1 complete')" = 0
verify failed_no_mark test "$(writes PATCH)" = 0

publish_run 603 803 arm64 . .
jq --arg body "<!-- soak-obligation O1 run=603 architecture=arm64 revision=$NODE verdict=satisfied -->" \
  '. + [{user:{login:"other-account"},body:$body}]' "$COMMENTS" > "$COMMENTS.new"
mv "$COMMENTS.new" "$COMMENTS"
expect foreign_marker 0 bash "$CHECK" execute 601 "$OUT/runs/execute-601-foreign"
verify foreign_no_completion test "$(count 'O1 complete')" = 0
verify foreign_no_mark test "$(writes PATCH)" = 0
jq --arg body "<!-- soak-obligation O1 run=602 architecture=arm64 revision=$NODE verdict=satisfied -->" \
  '. + [{user:{login:"github-actions[bot]"},body:$body}]' "$COMMENTS" > "$COMMENTS.new"
mv "$COMMENTS.new" "$COMMENTS"
expect marker_without_evidence 0 bash "$CHECK" execute 601 "$OUT/runs/execute-601-marker"
verify marker_no_completion test "$(count 'O1 complete')" = 0
verify marker_no_mark test "$(writes PATCH)" = 0

expect second_architecture 0 bash "$CHECK" execute 603 "$OUT/runs/execute-603"
verify second_comment test "$(count 'run=603 architecture=arm64')" = 2
verify second_completion test "$(count "O1 complete revision=$NODE")" = 1
verify second_completion_text test "$(count 'The check does not mark obligation O1.')" = 1
verify second_no_mark test "$(writes PATCH)" = 0
verify second_issue_unchanged test "$(jq -r .body "$ISSUE" | grep -c -- '^- \[ \] ')" = 4
verify second_pair jq -e '[.runs[].run_id]==["601","603"] and .evidence_complete==false' "$OUT/runs/execute-603/pair/obligation.json"
expect second_architecture_again 0 bash "$CHECK" execute 603 "$OUT/runs/execute-603-again"
verify second_completion_once test "$(count "O1 complete revision=$NODE")" = 1
verify second_still_no_mark test "$(writes PATCH)" = 0

publish_run 621 821 amd64 . ".plan.node_revision=\"$MIXED\" | $EVIDENCE"
publish_run 623 823 arm64 . ".plan.node_revision=\"$MIXED\""
expect mixed_first 0 bash "$CHECK" execute 621 "$OUT/runs/execute-621"
expect mixed_second 0 bash "$CHECK" execute 623 "$OUT/runs/execute-623"
verify mixed_completion test "$(count "O1 complete revision=$MIXED")" = 1
verify mixed_no_mark test "$(writes PATCH)" = 0
verify mixed_issue_unchanged test "$(jq -r .body "$ISSUE" | grep -c -- '^- \[ \] ')" = 4

publish_run 611 811 amd64 . ".plan.node_revision=\"$FULL\" | $EVIDENCE"
publish_run 613 813 arm64 . ".plan.node_revision=\"$FULL\" | $EVIDENCE"
expect full_first 0 bash "$CHECK" execute 611 "$OUT/runs/execute-611"
verify full_first_no_completion test "$(count "O1 complete revision=$FULL")" = 0
verify full_first_no_mark test "$(writes PATCH)" = 0
expect full_second 0 bash "$CHECK" execute 613 "$OUT/runs/execute-613"
verify full_completion test "$(count "O1 complete revision=$FULL")" = 1
verify full_completion_text test "$(count 'The check marks obligation O1 in the issue text.')" = 1
verify full_mark test "$(writes PATCH)" = 1
verify full_issue_line sh -c 'jq -r .body "$1" | grep -Fxq -- "- [x] **O1. 60-hour stability soak.** Tracker task: TASK-018-7."' sh "$ISSUE"
verify full_issue_others test "$(jq -r .body "$ISSUE" | grep -c -- '^- \[ \] ')" = 3
verify full_issue_open jq -e '.state=="open"' "$ISSUE"
verify full_pair jq -e '[.runs[].run_id]==["611","613"] and .evidence_complete==true' "$OUT/runs/execute-613/pair/obligation.json"
verify full_update_has_text_only jq -e 'keys==["body"]' "$OUT/runs/execute-613/issue-payload.json"
expect full_second_again 0 bash "$CHECK" execute 613 "$OUT/runs/execute-613-again"
verify full_completion_once test "$(count "O1 complete revision=$FULL")" = 1
verify full_mark_once test "$(writes PATCH)" = 1

publish_run 604 804 amd64 . .
jq '.digest="sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"' "$OUT/runs/604/artifact.json" > "$OUT/runs/604/artifact-changed.json"
jq -n --slurpfile a "$OUT/runs/604/artifact-changed.json" '{total_count:1,artifacts:$a}' > "$OUT/runs/604/artifacts-changed.json"
serve "$API/actions/runs/604/artifacts?per_page=100" "$OUT/runs/604/artifacts-changed.json"
before="$(writes POST)"
expect archive_digest_differs 2 bash "$CHECK" execute 604 "$OUT/runs/execute-604"
verify archive_no_write test "$(writes POST)" = "$before"
publish_run 605 805 amd64 . .
jq -n --slurpfile a "$OUT/runs/605/artifact.json" '{total_count:2,artifacts:($a+$a)}' > "$OUT/runs/605/artifacts-double.json"
serve "$API/actions/runs/605/artifacts?per_page=100" "$OUT/runs/605/artifacts-double.json"
expect artifact_ambiguous 2 bash "$CHECK" execute 605 "$OUT/runs/execute-605"
publish_run 606 806 amd64 . .
mkdir "$OUT/runs/606/two"
cp "$OUT/runs/606/campaign-result.json" "$OUT/runs/606/two/campaign-result.json"
printf 'text\n' > "$OUT/runs/606/two/extra.txt"
(cd "$OUT/runs/606/two" && zip -q -j ../two.zip campaign-result.json extra.txt)
make_artifact 606 806 "$(hash "$OUT/runs/606/two.zip")" "$(size "$OUT/runs/606/two.zip")" > "$OUT/runs/606/artifact-two.json"
jq -n --slurpfile a "$OUT/runs/606/artifact-two.json" '{total_count:1,artifacts:$a}' > "$OUT/runs/606/artifacts-two.json"
serve "$API/actions/runs/606/artifacts?per_page=100" "$OUT/runs/606/artifacts-two.json"
serve "$API/actions/artifacts/806/zip" "$OUT/runs/606/two.zip"
expect archive_two_members 2 bash "$CHECK" execute 606 "$OUT/runs/execute-606"
verify invalid_no_write test "$(writes POST)" = "$before"

serve "$API/pulls/436" "$OUT/runs/pull-open.json"
publish_run 607 807 amd64 . '.plan.node_revision="5555555555555555555555555555555555555555"'
serve "$API/compare/$OTHER...$MASTER?per_page=1" "$OUT/runs/compare.json"
expect source_open 0 bash "$CHECK" execute 607 "$OUT/runs/execute-607"
verify source_open_verdict jq -e '
  .verdict=="not_satisfied" and .required_commit==null and
  .required_commit_source=="pull_request_not_merged" and ([.criteria[]|select(.satisfied==false)|.id]==[1])
' "$OUT/runs/execute-607/evaluation/verdict.json"
verify source_open_comment test "$(count "run=607 architecture=amd64 revision=$OTHER verdict=not_satisfied")" = 1
publish_run 608 808 amd64 . '.plan.node_revision="5555555555555555555555555555555555555555"'
serve "$API/compare/$REQUIRED...$OTHER?per_page=1" "$OUT/runs/compare.json"
expect source_from_input 0 bash "$CHECK" execute 608 "$OUT/runs/execute-608" "$REQUIRED"
verify source_input_verdict jq -e --arg required "$REQUIRED" '
  .verdict=="satisfied" and .required_commit==$required and .required_commit_source=="dispatch_input"
' "$OUT/runs/execute-608/evaluation/verdict.json"

verify calls_only_expected sh -c '! grep -v -E "^(GET|POST|PATCH) " "$1"' sh "$FIXTURE_API/calls.txt"
verify no_update_without_evidence test ! -e "$OUT/runs/execute-603/issue-payload.json"
jq -n --argjson passed "$passed" '{schema_version:1,scope:"soak-obligation-check-controls",status:"passed",checks:$passed,network_requests:0,issue_updates:0}' > "$OUT/summary.json"
printf 'The soak obligation controls passed: %s checks.\n' "$passed"
