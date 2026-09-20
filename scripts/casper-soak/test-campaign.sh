#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${1:?An evidence directory is required.}"
[[ ! -e "$OUT" ]] || exit 2
mkdir -p "$OUT/root/docs/campaign"
OUT="$(cd "$OUT" && pwd)"
FIXTURE="$OUT/root"
HELPER="$ROOT/scripts/casper-soak/campaign.sh"
REV="$(printf '1%.0s' {1..40})"
NODE="$(printf '2%.0s' {1..40})"
SUITE="$(printf '3%.0s' {1..40})"
DIGEST="$(printf 'a%.0s' {1..64})"
export CAMPAIGN_CONTROL_REVISION="$REV"
export GITHUB_RUN_ATTEMPT=1
export GITHUB_EVENT_NAME=workflow_dispatch
hash() { sha256sum "$1" | cut -d ' ' -f1; }
for path in scripts/casper-soak/campaign.sh .github/workflows/merge-recovery-soak.yml .github/actions/soak-segment/action.yml scripts/run-merge-recovery-soak.sh scripts/run-integration-preflight.sh; do
  mkdir -p "$FIXTURE/$(dirname "$path")"
  printf 'Synthetic source fixture.\n' > "$FIXTURE/$path"
done
jq -n --arg suite "$SUITE" --arg digest "$DIGEST" '{schema_version:1,profile_id:"current-dev-load",external_harness_revision:$suite,required_capabilities:["load","query","metrics"],test_path:"integration-tests/test/tests/custom/test_load.py",test_sha256:$digest,preflight_profile_path:"integration-tests/test/full-suite.txt",preflight_profile_sha256:$digest,providers:["docker","subprocess"],policy_variant:"current-dev-load"}' > "$FIXTURE/docs/campaign/workload.json"
for arch in amd64 arm64; do
  jq -n --arg candidate "dev-$arch" --arg node "$NODE" --arg digest "$DIGEST" --arg workload "$(hash "$FIXTURE/docs/campaign/workload.json")" '{schema_version:1,candidate_id:$candidate,node_revision:$node,node_binary_digest:("sha256:"+$digest),image_digest:("sha256:"+$digest),workload_sha256:$workload,evidence_kind:"node_observation",status:"qualified",capabilities:{load:"qualified",query:"qualified",metrics:"qualified"}}' > "$FIXTURE/docs/campaign/$arch-qualification.json"
done
sources='{}'
for path in scripts/casper-soak/campaign.sh .github/workflows/merge-recovery-soak.yml .github/actions/soak-segment/action.yml scripts/run-merge-recovery-soak.sh scripts/run-integration-preflight.sh; do
  sources="$(jq -cn --argjson old "$sources" --arg path "$path" --arg digest "$(hash "$FIXTURE/$path")" '$old + {($path):$digest}')"
done
candidates='{}'
for arch in amd64 arm64; do
  candidates="$(jq -cn --argjson old "$candidates" --arg id "dev-$arch" --arg platform "linux/$arch" --arg node "$NODE" --arg digest "$DIGEST" --arg workload "$(hash "$FIXTURE/docs/campaign/workload.json")" --arg qualification "$(hash "$FIXTURE/docs/campaign/$arch-qualification.json")" --arg qualification_path "docs/campaign/$arch-qualification.json" '$old + {($id):{platform:$platform,node_revision:$node,image_digest:("sha256:"+$digest),image_config_digest:("sha256:"+$digest),node_binary_digest:("sha256:"+$digest),workload:{path:"docs/campaign/workload.json",sha256:$workload},qualification:{path:$qualification_path,sha256:$qualification}}}')"
done
jq -n --arg revision "$REV" --arg suite "$SUITE" --argjson sources "$sources" --argjson candidates "$candidates" '{schema_version:1,status:"approved",campaign_id:"task-017-12-fixture",phase:"pre_pr216_merge",harness_revision:$revision,external_harness_revision:$suite,source_digests:$sources,candidates:$candidates,preflight_candidate_id:"dev-amd64",stages:{preflight:{approved:true,duration_seconds:0,runner_max_seconds:14400,memory_gb:64,max_launches:1},baseline:{approved:true,duration_seconds:86400,runner_max_seconds:93600,memory_gb:64,max_launches:2},stability:{approved:true,duration_seconds:216000,runner_max_seconds:230400,memory_gb:64,max_launches:2}}}' > "$FIXTURE/docs/campaign/approval.json"
make_request() {
  jq -n --arg digest "$(hash "$FIXTURE/docs/campaign/approval.json")" --arg stage "$1" --arg candidate "$2" '{schema_version:1,campaign_id:"task-017-12-fixture",stage:$stage,candidate_id:$candidate,approval:{path:"docs/campaign/approval.json",sha256:$digest},preflight_run_id:(if $stage=="preflight" then null else "101" end),baseline_run_ids:(if $stage=="stability" then {"dev-amd64":"102","dev-arm64":"103"} else {} end)}'
}
passed=0
expect() {
  local name="$1" expected="$2" actual
  shift 2
  set +e
  "$@" > "$OUT/$name.stdout" 2> "$OUT/$name.stderr"
  actual=$?
  set -e
  printf '%s\n' "$actual" > "$OUT/$name.exit"
  if [[ "$actual" != "$expected" ]]; then
    printf 'FAIL %s: expected exit %s, received %s\n' "$name" "$expected" "$actual" >&2
    exit 1
  fi
  passed=$((passed + 1))
}
make_request baseline dev-amd64 > "$OUT/request.json"
expect baseline_amd64 0 bash "$HELPER" plan "$FIXTURE" "$OUT/request.json"
jq -e '.stage=="baseline" and .duration_seconds==86400 and .runner_max_seconds==93600 and .runner_arch=="x64" and .max_launches==1 and .node_launch_count==0 and .soak_verdict=="pending"' "$OUT/baseline_amd64.stdout" >/dev/null
make_request baseline dev-arm64 > "$OUT/request.json"
expect baseline_arm64 0 bash "$HELPER" plan "$FIXTURE" "$OUT/request.json"
jq -e '.runner_arch=="arm64" and .platform=="linux/arm64"' "$OUT/baseline_arm64.stdout" >/dev/null
make_request preflight dev-amd64 > "$OUT/request.json"
expect preflight 0 bash "$HELPER" plan "$FIXTURE" "$OUT/request.json"
make_request stability dev-arm64 > "$OUT/request.json"
expect stability 0 bash "$HELPER" plan "$FIXTURE" "$OUT/request.json"
jq -e '.duration_seconds==216000 and (.required_prior_runs|length)==3' "$OUT/stability.stdout" >/dev/null
make_request preflight dev-arm64 > "$OUT/request.json"
expect wrong_preflight_candidate 2 bash "$HELPER" plan "$FIXTURE" "$OUT/request.json"
make_request baseline dev-amd64 > "$OUT/request.json"
expect rerun 2 env GITHUB_RUN_ATTEMPT=2 bash "$HELPER" plan "$FIXTURE" "$OUT/request.json"
expect scheduled 2 env GITHUB_EVENT_NAME=schedule bash "$HELPER" plan "$FIXTURE" "$OUT/request.json"
expect different_control 2 env CAMPAIGN_CONTROL_REVISION="$NODE" bash "$HELPER" plan "$FIXTURE" "$OUT/request.json"
jq '.preflight_run_id=null' "$OUT/request.json" > "$OUT/missing-preflight.json"
expect missing_preflight 2 bash "$HELPER" plan "$FIXTURE" "$OUT/missing-preflight.json"
jq '.approval.path="../../outside.json"' "$OUT/request.json" > "$OUT/traversal.json"
expect path_traversal 2 bash "$HELPER" plan "$FIXTURE" "$OUT/traversal.json"
printf 'Changed source.\n' >> "$FIXTURE/scripts/run-merge-recovery-soak.sh"
expect source_drift 2 bash "$HELPER" plan "$FIXTURE" "$OUT/request.json"
printf 'Synthetic source fixture.\n' > "$FIXTURE/scripts/run-merge-recovery-soak.sh"
cp "$FIXTURE/docs/campaign/approval.json" "$OUT/approval-original.json"
printf '\n' >> "$FIXTURE/docs/campaign/approval.json"
expect approval_digest_drift 2 bash "$HELPER" plan "$FIXTURE" "$OUT/request.json"
cp "$OUT/approval-original.json" "$FIXTURE/docs/campaign/approval.json"
ln -s "$OUT/approval-original.json" "$FIXTURE/docs/campaign/outside-link.json"
jq '.approval.path="docs/campaign/outside-link.json"' "$OUT/request.json" > "$OUT/symlink.json"
expect symlink_escape 2 bash "$HELPER" plan "$FIXTURE" "$OUT/symlink.json"
mkdir "$OUT/tools"
printf '#!/usr/bin/env bash\nfor arg in "$@"; do\n  if [[ "$arg" == '\''.source_digests|to_entries[]|[.key,.value]|@tsv'\'' ]]; then exit 7; fi\ndone\nexec %q "$@"\n' "$(command -v jq)" > "$OUT/tools/jq"
chmod +x "$OUT/tools/jq"
expect source_inventory_error 2 env PATH="$OUT/tools:$PATH" bash "$HELPER" plan "$FIXTURE" "$OUT/request.json"
for entry in 'short_baseline|.stages.baseline.duration_seconds=79200' 'extra_launches|.stages.baseline.max_launches=3' 'unapproved|.stages.baseline.approved=false' 'wrong_memory|.stages.baseline.memory_gb=48' 'wrong_platform|.candidates["dev-amd64"].platform="linux/arm64"' 'missing_source|del(.source_digests["scripts/run-merge-recovery-soak.sh"])' 'unapproved_campaign|.status="pending"'; do
  name="${entry%%|*}"; filter="${entry#*|}"
  jq "$filter" "$OUT/approval-original.json" > "$FIXTURE/docs/campaign/approval.json"
  make_request baseline dev-amd64 > "$OUT/request.json"
  expect "$name" 2 bash "$HELPER" plan "$FIXTURE" "$OUT/request.json"
done
cp "$OUT/approval-original.json" "$FIXTURE/docs/campaign/approval.json"
cp "$FIXTURE/docs/campaign/amd64-qualification.json" "$OUT/qualification-original.json"
for entry in 'unknown_capability|.capabilities.query="unknown"' 'synthetic_qualification|.evidence_kind="synthetic_fixture"' 'wrong_node|.node_revision="0000000000000000000000000000000000000000"' 'wrong_binary|.node_binary_digest="sha256:0000000000000000000000000000000000000000000000000000000000000000"'; do
  name="${entry%%|*}"; filter="${entry#*|}"
  jq "$filter" "$OUT/qualification-original.json" > "$FIXTURE/docs/campaign/amd64-qualification.json"
  jq --arg digest "$(hash "$FIXTURE/docs/campaign/amd64-qualification.json")" '.candidates["dev-amd64"].qualification.sha256=$digest' "$OUT/approval-original.json" > "$FIXTURE/docs/campaign/approval.json"
  make_request baseline dev-amd64 > "$OUT/request.json"
  expect "$name" 2 bash "$HELPER" plan "$FIXTURE" "$OUT/request.json"
done
expect full_baseline_window 0 bash "$HELPER" window "$OUT/baseline_amd64.stdout" 2000000000 2000000600
jq -e '.duration_seconds==86400 and .workload_deadline_epoch==2000087000 and .runner_deadline_epoch==2000093600' "$OUT/full_baseline_window.stdout" >/dev/null
expect insufficient_baseline_time 2 bash "$HELPER" window "$OUT/baseline_amd64.stdout" 2000000000 2000006601
expect exact_baseline_limit 0 bash "$HELPER" window "$OUT/baseline_amd64.stdout" 2000000000 2000006600
expect clock_reversal 2 bash "$HELPER" window "$OUT/baseline_amd64.stdout" 2000000000 1999999999
expect full_stability_window 0 bash "$HELPER" window "$OUT/stability.stdout" 2000000000 2000000600
: > "$OUT/empty.json"
expect empty_window_input 2 bash "$HELPER" window "$OUT/empty.json" 2000000000 2000000600
jq -s '.[]' "$OUT/baseline_amd64.stdout" "$OUT/stability.stdout" > "$OUT/multiple-windows.json"
expect multiple_window_documents 2 bash "$HELPER" window "$OUT/multiple-windows.json" 2000000000 2000000600
jq '.duration_seconds=1' "$OUT/baseline_amd64.stdout" > "$OUT/invalid-window.json"
jq -s '.[]' "$OUT/invalid-window.json" "$OUT/baseline_amd64.stdout" > "$OUT/invalid-first-window.json"
expect invalid_first_window_document 2 bash "$HELPER" window "$OUT/invalid-first-window.json" 2000000000 2000000600
jq -s '.' "$OUT/baseline_amd64.stdout" > "$OUT/window-array.json"
expect array_window_input 2 bash "$HELPER" window "$OUT/window-array.json" 2000000000 2000000600
jq -n --argjson count "$passed" '{schema_version:1,status:"passed",checks:$count,evidence_kind:"synthetic_fixture",node_launch_count:0,cloud_launch_count:0,claim_discharge:"pending"}' > "$OUT/summary.json"
printf 'PASS: %s campaign admission and duration checks. No node or cloud runner launched.\n' "$passed"
