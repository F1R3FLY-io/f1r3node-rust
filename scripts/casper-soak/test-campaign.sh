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
for path in scripts/casper-soak/campaign.sh scripts/casper-soak/test-campaign.sh .github/workflows/merge-recovery-soak.yml .github/actions/soak-segment/action.yml scripts/run-merge-recovery-soak.sh scripts/run-integration-preflight.sh; do
  mkdir -p "$FIXTURE/$(dirname "$path")"
  printf 'Synthetic source fixture.\n' > "$FIXTURE/$path"
done
jq -n --arg suite "$SUITE" --arg digest "$DIGEST" '{schema_version:1,profile_id:"current-dev-load",external_harness_revision:$suite,required_capabilities:["load","query","metrics"],test_path:"integration-tests/test/tests/custom/test_load.py",test_sha256:$digest,preflight_profile_path:"integration-tests/test/full-suite.txt",preflight_profile_sha256:$digest,providers:["docker","subprocess"],policy_variant:"current-dev-load"}' > "$FIXTURE/docs/campaign/workload.json"
for arch in amd64 arm64; do
  jq -n --arg candidate "dev-$arch" --arg node "$NODE" --arg digest "$DIGEST" --arg workload "$(hash "$FIXTURE/docs/campaign/workload.json")" '{schema_version:1,candidate_id:$candidate,node_revision:$node,node_binary_digest:("sha256:"+$digest),image_digest:("sha256:"+$digest),workload_sha256:$workload,evidence_kind:"node_observation",status:"qualified",capabilities:{load:"qualified",query:"qualified",metrics:"qualified"}}' > "$FIXTURE/docs/campaign/$arch-qualification.json"
done
sources='{}'
for path in scripts/casper-soak/campaign.sh scripts/casper-soak/test-campaign.sh .github/workflows/merge-recovery-soak.yml .github/actions/soak-segment/action.yml scripts/run-merge-recovery-soak.sh scripts/run-integration-preflight.sh; do
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
for entry in 'short_baseline|.stages.baseline.duration_seconds=79200' 'extra_launches|.stages.baseline.max_launches=3' 'unapproved|.stages.baseline.approved=false' 'wrong_memory|.stages.baseline.memory_gb=48' 'wrong_platform|.candidates["dev-amd64"].platform="linux/arm64"' 'missing_source|del(.source_digests["scripts/run-merge-recovery-soak.sh"])' 'missing_test_source|del(.source_digests["scripts/casper-soak/test-campaign.sh"])' 'unapproved_campaign|.status="pending"'; do
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
cp "$OUT/qualification-original.json" "$FIXTURE/docs/campaign/amd64-qualification.json"
cp "$HELPER" "$FIXTURE/scripts/casper-soak/campaign.sh"
jq --arg digest "$(hash "$HELPER")" '.source_digests["scripts/casper-soak/campaign.sh"]=$digest' "$OUT/approval-original.json" > "$FIXTURE/docs/campaign/approval.json"
export GITHUB_REPOSITORY=F1R3FLY-io/f1r3node-rust
export GITHUB_RUN_ID=200
export INPUT_TARGET_REF=dev
export INPUT_PREFLIGHT_ONLY=false INPUT_SKIP_PREFLIGHT=false INPUT_CANARY=false INPUT_INJECT_PROTECTION_BREACH=false
export INPUT_SCHEDULED_SLOT='' INPUT_WINDOW_END='' INPUT_SERIES='' INPUT_RETRY_ATTEMPT=0 INPUT_RESTART_OF_RUN_ID='' INPUT_CANDIDATE_TAG=''
mkdir "$OUT/api" "$OUT/api-tools"
printf '#!/usr/bin/env bash\nexit 99\n' > "$OUT/api-tools/gh"
chmod +x "$OUT/api-tools/gh"
export PATH="$OUT/api-tools:$PATH"
make_request preflight dev-amd64 > "$OUT/dispatch-request.json"
expect dispatch_preflight_blocked 3 bash "$HELPER" dispatch "$FIXTURE" "$OUT/dispatch-request.json" campaign-preflight "$OUT/dispatch-preflight"
jq -e '.admission=="blocked" and .exit_code==3 and .planning=="passed" and .prior_runs=="not-required" and .cloud_launch_count==0 and .node_launch_count==0 and .soak_verdict=="non_passing"' "$OUT/dispatch-preflight/report.json" >/dev/null
cmp "$OUT/dispatch-request.json" "$OUT/dispatch-preflight/request.json"
expect dispatch_directory_reuse 2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/dispatch-request.json" campaign-preflight "$OUT/dispatch-preflight"
expect dispatch_stage_mismatch 2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/dispatch-request.json" campaign-baseline-24h "$OUT/dispatch-mismatch"
expect dispatch_legacy_duration 2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/dispatch-request.json" daily-24h "$OUT/dispatch-legacy"
expect dispatch_rerun 2 env GITHUB_RUN_ATTEMPT=2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/dispatch-request.json" campaign-preflight "$OUT/dispatch-rerun"
expect dispatch_schedule 2 env GITHUB_EVENT_NAME=schedule bash "$HELPER" dispatch "$FIXTURE" "$OUT/dispatch-request.json" campaign-preflight "$OUT/dispatch-schedule"
for pair in INPUT_PREFLIGHT_ONLY=true INPUT_SKIP_PREFLIGHT=true INPUT_CANARY=true INPUT_INJECT_PROTECTION_BREACH=true INPUT_SCHEDULED_SLOT=2000000000 INPUT_WINDOW_END=2000000000 INPUT_SERIES=weekend INPUT_RETRY_ATTEMPT=1 INPUT_RESTART_OF_RUN_ID=101 INPUT_CANDIDATE_TAG=v1.0.0 INPUT_TARGET_REF=master; do
  key="${pair%%=*}"
  expect "dispatch_$key" 2 env "$pair" bash "$HELPER" dispatch "$FIXTURE" "$OUT/dispatch-request.json" campaign-preflight "$OUT/dispatch-$key"
done
expect dispatch_empty_input 2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/empty.json" campaign-preflight "$OUT/dispatch-empty"
jq -e '.exit_code==2 and .admission=="invalid_input" and .cloud_launch_count==0' "$OUT/dispatch-empty/report.json" >/dev/null
make_request baseline dev-amd64 > "$OUT/dispatch-request.json"
expect dispatch_api_failure 2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/dispatch-request.json" campaign-baseline-24h "$OUT/dispatch-api-failure"
jq -e '.planning=="passed" and .prior_runs=="failed" and .exit_code==2' "$OUT/dispatch-api-failure/report.json" >/dev/null
expect dispatch_self_reference 2 env GITHUB_RUN_ID=101 bash "$HELPER" dispatch "$FIXTURE" "$OUT/dispatch-request.json" campaign-baseline-24h "$OUT/dispatch-self"
export FIXTURE_API="$OUT/api"
cat > "$OUT/api-tools/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == api && "$2" == --hostname && "$3" == github.com && "$4" == --method && "$5" == GET ]] || exit 98
printf '%s\n' "$6" >> "$FIXTURE_API/calls.txt"
case "$6" in
  repos/F1R3FLY-io/f1r3node-rust/actions/runs/101) cp "$FIXTURE_API/run.json" /dev/stdout ;;
  repos/F1R3FLY-io/f1r3node-rust/actions/runs/101/artifacts\?per_page=100) cp "$FIXTURE_API/artifacts.json" /dev/stdout ;;
  repos/F1R3FLY-io/f1r3node-rust/actions/artifacts/301/zip) cp "$FIXTURE_API/result.zip" /dev/stdout ;;
  repos/F1R3FLY-io/f1r3node-rust/actions/runs/10[23]) cp "$FIXTURE_API/run-${6##*/}.json" /dev/stdout ;;
  repos/F1R3FLY-io/f1r3node-rust/actions/runs/10[23]/artifacts\?per_page=100)
    id="${6#repos/F1R3FLY-io/f1r3node-rust/actions/runs/}"
    cp "$FIXTURE_API/artifacts-${id%%/*}.json" /dev/stdout ;;
  repos/F1R3FLY-io/f1r3node-rust/actions/artifacts/30[23]/zip)
    id="${6%/zip}"
    cp "$FIXTURE_API/result-${id##*/}.zip" /dev/stdout ;;
  *) exit 97 ;;
esac
EOF
chmod +x "$OUT/api-tools/gh"
bash "$HELPER" plan "$FIXTURE" "$OUT/dispatch-request.json" > "$OUT/dispatch-plan.json"
jq -n --arg revision "$REV" '{id:101,run_attempt:1,path:".github/workflows/merge-recovery-soak.yml",repository:{full_name:"F1R3FLY-io/f1r3node-rust"},head_sha:$revision,event:"workflow_dispatch",status:"completed",conclusion:"success"}' > "$OUT/api/run.json"
jq -n --slurpfile p "$OUT/dispatch-plan.json" '{schema_version:1,run_id:"101",run_attempt:1,repository:"F1R3FLY-io/f1r3node-rust",plan:($p[0]+{stage:"preflight",duration_seconds:0}),admission:"admitted",result:"passed",evidence_kind:"node_observation",cloud_launch_count:1,node_launch_count:1,cleanup:{complete:true},host_protection:{status:"passed"},integration_preflight:"passed"}' > "$OUT/api/campaign-result.json"
pack_result() {
  local zipfile
  zipfile="$(mktemp "$OUT/api/archive-XXXXXX.zip")"
  zip -q -j "$zipfile.new" "$OUT/api/campaign-result.json"
  mv "$zipfile.new" "$OUT/api/result.zip"
  jq -n --arg revision "$REV" --arg digest "sha256:$(hash "$OUT/api/result.zip")" --argjson size "$(stat -c %s "$OUT/api/result.zip")" '{total_count:1,artifacts:[{id:301,name:"casper-campaign-result-101-1",expired:false,size_in_bytes:$size,digest:$digest,workflow_run:{id:101,head_sha:$revision}}]}' > "$OUT/api/artifacts.json"
}
pack_result
expect dispatch_prior_consistent 3 bash "$HELPER" dispatch "$FIXTURE" "$OUT/dispatch-request.json" campaign-baseline-24h "$OUT/dispatch-prior-consistent"
jq -e '.planning=="passed" and .prior_runs=="verified-records-only" and .admission=="blocked" and .cloud_launch_count==0' "$OUT/dispatch-prior-consistent/report.json" >/dev/null
cp "$OUT/api/run.json" "$OUT/api/run-original.json"
cp "$OUT/api/artifacts.json" "$OUT/api/artifacts-original.json"
cp "$OUT/api/campaign-result.json" "$OUT/api/result-original.json"
for entry in 'failed|.conclusion="failure"' 'cancelled|.conclusion="cancelled"' 'running|.status="in_progress"' 'rerun|.run_attempt=2' 'wrong_workflow|.path=".github/workflows/other.yml"' 'wrong_repository|.repository.full_name="other/repo"' 'wrong_revision|.head_sha="0000000000000000000000000000000000000000"' 'wrong_event|.event="push"' 'wrong_run|.id=102'; do
  name="${entry%%|*}"; filter="${entry#*|}"
  jq "$filter" "$OUT/api/run-original.json" > "$OUT/api/run.json"
  expect "prior_$name" 2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/dispatch-request.json" campaign-baseline-24h "$OUT/prior-$name"
done
cp "$OUT/api/run-original.json" "$OUT/api/run.json"
for entry in 'expired|.artifacts[0].expired=true' 'duplicate|.artifacts += .artifacts | .total_count=2' 'digest|.artifacts[0].digest="sha256:0000000000000000000000000000000000000000000000000000000000000000"' 'artifact_run|.artifacts[0].workflow_run.id=102' 'pagination|.total_count=101' 'missing|.artifacts=[]|.total_count=0'; do
  name="${entry%%|*}"; filter="${entry#*|}"
  jq "$filter" "$OUT/api/artifacts-original.json" > "$OUT/api/artifacts.json"
  expect "prior_$name" 2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/dispatch-request.json" campaign-baseline-24h "$OUT/prior-$name"
done
cp "$OUT/api/artifacts-original.json" "$OUT/api/artifacts.json"
for entry in 'fixture|.evidence_kind="synthetic_fixture"' 'not_passed|.result="non_passing"' 'cleanup|.cleanup.complete=false' 'protection|.host_protection.status="breached"' 'identity|.plan.identity_digest="bad"' 'candidate|.plan.candidate_id="dev-arm64"' 'binary|.plan.node_binary_digest="bad"' 'preflight|.integration_preflight="failed"' 'extra_launch|.cloud_launch_count=2'; do
  name="${entry%%|*}"; filter="${entry#*|}"
  jq "$filter" "$OUT/api/result-original.json" > "$OUT/api/campaign-result.json"
  pack_result
  expect "prior_$name" 2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/dispatch-request.json" campaign-baseline-24h "$OUT/prior-$name"
done
cp "$OUT/api/result-original.json" "$OUT/api/campaign-result.json"
pack_result
jq '.plan.stage="baseline" | .plan.duration_seconds=86400 | .workload_elapsed_seconds=86400 | .soak_verdict="passed" | .measurement_completeness="complete" | .profile_verdicts={authority_finality:"passed",publication:"passed"}' "$OUT/api/result-original.json" > "$OUT/baseline-result.json"
expect baseline_record 0 bash "$HELPER" prior "$OUT/dispatch-plan.json" "$OUT/api/run.json" "$OUT/baseline-result.json" baseline dev-amd64
for entry in 'short|.workload_elapsed_seconds=86399' 'preflight_subtracted|.plan.duration_seconds=79200' 'missing_time|del(.workload_elapsed_seconds)' 'missing_measurements|.measurement_completeness="incomplete"' 'product_failure|.profile_verdicts.publication="product_failure"' 'missing_profile|del(.profile_verdicts.authority_finality)' 'no_verdict|del(.soak_verdict)'; do
  name="${entry%%|*}"; filter="${entry#*|}"
  jq "$filter" "$OUT/baseline-result.json" > "$OUT/short-result.json"
  expect "baseline_$name" 2 bash "$HELPER" prior "$OUT/dispatch-plan.json" "$OUT/api/run.json" "$OUT/short-result.json" baseline dev-amd64
done
jq -s '.[]' "$OUT/api/result-original.json" "$OUT/api/result-original.json" > "$OUT/api/campaign-result.json"
pack_result
expect prior_json_stream 2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/dispatch-request.json" campaign-baseline-24h "$OUT/prior-json-stream"
printf ' ' > "$OUT/api/extra.txt"
cp "$OUT/api/result-original.json" "$OUT/api/campaign-result.json"
pack_result
zip -q -j "$OUT/api/result.zip" "$OUT/api/extra.txt"
jq --arg digest "sha256:$(hash "$OUT/api/result.zip")" --argjson size "$(stat -c %s "$OUT/api/result.zip")" '.artifacts[0].digest=$digest | .artifacts[0].size_in_bytes=$size' "$OUT/api/artifacts-original.json" > "$OUT/api/artifacts.json"
expect prior_extra_archive_member 2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/dispatch-request.json" campaign-baseline-24h "$OUT/prior-extra-member"
awk 'BEGIN {for(i=0;i<1048577;i++) printf " "; print "{}"}' > "$OUT/api/campaign-result.json"
pack_result
expect prior_expanded_size_limit 2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/dispatch-request.json" campaign-baseline-24h "$OUT/prior-expanded-size"
WORKFLOW="$ROOT/.github/workflows/merge-recovery-soak.yml"
awk 'copy && /^  [a-z_]+:/ {exit} /^  campaign_admission:/ {copy=1} copy' "$WORKFLOW" > "$OUT/campaign-job.yml"
expect workflow_hosted_only 0 grep -Fq 'runs-on: ubuntu-latest' "$OUT/campaign-job.yml"
expect workflow_no_cloud_credentials 1 grep -Eq 'secrets\.|environment:|self-hosted|actions: write|contents: write|continue-on-error:|needs:' "$OUT/campaign-job.yml"
expect workflow_always_retain 0 grep -Fq 'if: always()' "$OUT/campaign-job.yml"
expect workflow_pinned_checkout 0 grep -Fq 'ref: ${{ github.workflow_sha }}' "$OUT/campaign-job.yml"
expect workflow_explicit_baseline 0 grep -Fxq '          - campaign-baseline-24h' "$WORKFLOW"
awk '/^  schedule_gate:/ {copy=1} /^    name:/ && copy {exit} copy' "$WORKFLOW" > "$OUT/schedule-condition.yml"
expect workflow_excludes_campaign_request 0 grep -Fq "inputs.campaign_request == '' &&" "$OUT/schedule-condition.yml"
expect workflow_excludes_campaign_duration 0 grep -Fq "!startsWith(inputs.duration, 'campaign-')" "$OUT/schedule-condition.yml"
awk '/if \[\[ -n "\$\{INPUT_CAMPAIGN_REQUEST:-\}"/ {copy=1} copy {sub(/^          /, ""); print} copy && /^fi$/ {exit}' "$WORKFLOW" > "$OUT/schedule-guard.sh"
expect workflow_legacy_guard 0 env INPUT_CAMPAIGN_REQUEST='' INPUT_DURATION=daily-24h bash "$OUT/schedule-guard.sh"
expect workflow_campaign_request_guard 2 env INPUT_CAMPAIGN_REQUEST='{}' INPUT_DURATION=daily-24h bash "$OUT/schedule-guard.sh"
for selection in campaign-preflight campaign-baseline-24h campaign-stability-60h campaign-invalid; do
  expect "workflow_$selection" 2 env INPUT_CAMPAIGN_REQUEST='' INPUT_DURATION="$selection" bash "$OUT/schedule-guard.sh"
done
cp "$OUT/api/result-original.json" "$OUT/api/campaign-result.json"
pack_result
for arch in amd64 arm64; do
  make_request baseline "dev-$arch" > "$OUT/request-baseline-$arch.json"
  expect "dispatch_baseline_$arch" 3 bash "$HELPER" dispatch "$FIXTURE" "$OUT/request-baseline-$arch.json" campaign-baseline-24h "$OUT/dispatch-baseline-$arch"
  jq -e '.prior_runs=="verified-records-only" and .execution_enabled==false' "$OUT/dispatch-baseline-$arch/report.json" >/dev/null
  jq -e --arg arch "$arch" '.platform==("linux/"+$arch) and .duration_seconds==86400 and .runner_max_seconds==93600' "$OUT/dispatch-baseline-$arch/plan.json" >/dev/null
done
pin_baseline_result() {
  local run="$1" artifact="$(( $1 + 200 ))"
  zip -q -j "$OUT/api/result-$artifact.zip" "$OUT/api/baseline-$run/campaign-result.json"
  jq -n --arg run "$run" --argjson artifact "$artifact" --arg revision "$REV" --arg digest "sha256:$(hash "$OUT/api/result-$artifact.zip")" --argjson size "$(stat -c %s "$OUT/api/result-$artifact.zip")" '{total_count:1,artifacts:[{id:$artifact,name:("casper-campaign-result-"+$run+"-1"),expired:false,size_in_bytes:$size,digest:$digest,workflow_run:{id:($run|tonumber),head_sha:$revision}}]}' > "$OUT/api/artifacts-$run.json"
}
for pair in 102:dev-amd64 103:dev-arm64; do
  run="${pair%%:*}"; candidate="${pair#*:}"
  mkdir "$OUT/api/baseline-$run"
  jq --argjson id "$run" '.id=$id' "$OUT/api/run-original.json" > "$OUT/api/run-$run.json"
  jq --arg id "$run" --arg candidate "$candidate" '.run_id=$id | .plan=(.plan + .plan.campaign_candidates[$candidate] + {candidate_id:$candidate,stage:"baseline",duration_seconds:86400})' "$OUT/baseline-result.json" > "$OUT/api/baseline-$run/campaign-result.json"
  pin_baseline_result "$run"
done
for arch in amd64 arm64; do
  make_request stability "dev-$arch" > "$OUT/request-stability-$arch.json"
  expect "dispatch_stability_$arch" 3 bash "$HELPER" dispatch "$FIXTURE" "$OUT/request-stability-$arch.json" campaign-stability-60h "$OUT/dispatch-stability-$arch"
  jq -e '.prior_runs=="verified-records-only" and .admission=="blocked" and .execution_enabled==false' "$OUT/dispatch-stability-$arch/report.json" >/dev/null
  for run in 101 102 103; do test -s "$OUT/dispatch-stability-$arch/prior-$run/result.json"; done
  jq -e '.duration_seconds==216000 and .runner_max_seconds==230400' "$OUT/dispatch-stability-$arch/plan.json" >/dev/null
done
jq '.baseline_run_ids["dev-arm64"]="102"' "$OUT/request-stability-arm64.json" > "$OUT/duplicate-baselines.json"
expect stability_duplicate_baselines 2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/duplicate-baselines.json" campaign-stability-60h "$OUT/stability-duplicate-baselines"
jq '.conclusion="failure"' "$OUT/api/run-103.json" > "$OUT/api/failed-run.json"
cp "$OUT/api/run-103.json" "$OUT/api/run-103-original.json"
cp "$OUT/api/failed-run.json" "$OUT/api/run-103.json"
expect stability_failed_arm64_baseline 2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/request-stability-arm64.json" campaign-stability-60h "$OUT/stability-failed-arm64"
cp "$OUT/api/run-103-original.json" "$OUT/api/run-103.json"
cp "$OUT/api/baseline-103/campaign-result.json" "$OUT/api/baseline-103-original.json"
jq '.workload_elapsed_seconds=86399' "$OUT/api/baseline-103-original.json" > "$OUT/api/baseline-103/campaign-result.json"
pin_baseline_result 103
expect stability_short_arm64_baseline 2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/request-stability-arm64.json" campaign-stability-60h "$OUT/stability-short-arm64"
cp "$OUT/api/baseline-103-original.json" "$OUT/api/baseline-103/campaign-result.json"
pin_baseline_result 103
cp "$FIXTURE/docs/campaign/approval.json" "$OUT/current-approval.json"
jq '.stages.stability.approved=false' "$OUT/current-approval.json" > "$FIXTURE/docs/campaign/approval.json"
make_request stability dev-arm64 > "$OUT/unapproved-stability.json"
expect stability_unapproved_resources 2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/unapproved-stability.json" campaign-stability-60h "$OUT/stability-unapproved-resources"
cp "$OUT/current-approval.json" "$FIXTURE/docs/campaign/approval.json"
expect exact_stability_limit 0 bash "$HELPER" window "$OUT/stability.stdout" 2000000000 2000013800
expect insufficient_stability_time 2 bash "$HELPER" window "$OUT/stability.stdout" 2000000000 2000013801
mkdir "$OUT/api/symlink-archive"
ln -s ../result-original.json "$OUT/api/symlink-archive/campaign-result.json"
zip -q -y -j "$OUT/api/symlink.zip" "$OUT/api/symlink-archive/campaign-result.json"
cp "$OUT/api/symlink.zip" "$OUT/api/result.zip"
jq --arg digest "sha256:$(hash "$OUT/api/result.zip")" --argjson size "$(stat -c %s "$OUT/api/result.zip")" '.artifacts[0].digest=$digest | .artifacts[0].size_in_bytes=$size' "$OUT/api/artifacts-original.json" > "$OUT/api/artifacts.json"
expect prior_symlink_member 2 bash "$HELPER" dispatch "$FIXTURE" "$OUT/request-baseline-amd64.json" campaign-baseline-24h "$OUT/prior-symlink-member"
jq -n --argjson count "$passed" '{schema_version:1,status:"passed",checks:$count,evidence_kind:"synthetic_fixture",node_launch_count:0,cloud_launch_count:0,claim_discharge:"pending"}' > "$OUT/summary.json"
printf 'PASS: %s campaign admission and duration checks. No node or cloud runner launched.\n' "$passed"
