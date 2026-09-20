#!/usr/bin/env bash
set -euo pipefail
fail() {
  printf 'Campaign rejected: %s\n' "$1" >&2
  exit 2
}
hash_file() { sha256sum "$1" | cut -d ' ' -f1; }
check_json() {
  [[ -f "$1" && "$(stat -c %s "$1")" -le 1048576 ]] || fail 'The JSON input is missing or exceeds its byte limit.'
  jq -se 'length==1 and (.[0]|type=="object")' "$1" >/dev/null 2>&1 || fail 'The input must contain exactly one JSON object.'
}
resolve_file() {
  local root="$1" path="$2" digest="$3" resolved
  [[ "$path" =~ ^[A-Za-z0-9_.][A-Za-z0-9_./-]*$ && "$path" != *'..'* && "$path" != *'//'* ]] || fail 'The artifact path is not permitted.'
  [[ "$digest" =~ ^[a-f0-9]{64}$ ]] || fail 'The artifact digest is invalid.'
  resolved="$(realpath -e -- "$root/$path" 2>/dev/null)" || fail 'The artifact does not exist.'
  [[ "$resolved" == "$root/"* && -f "$resolved" ]] || fail 'The artifact is outside the source root.'
  [[ "$(hash_file "$resolved")" == "$digest" ]] || fail 'The artifact bytes differ from their pin.'
  printf '%s\n' "$resolved"
}
plan() {
  [[ $# == 2 ]] || fail 'The plan command requires a source root and request.'
  local root request approval workload qualification path digest source_rows
  root="$(realpath -e -- "$1")" || fail 'The source root does not exist.'
  request="$2"
  check_json "$request"
  [[ "${GITHUB_EVENT_NAME:-}" == workflow_dispatch && "${GITHUB_RUN_ATTEMPT:-}" == 1 ]] || fail 'Only a first-attempt manual dispatch is permitted.'
  [[ "${CAMPAIGN_CONTROL_REVISION:-}" =~ ^[a-f0-9]{40}$ ]] || fail 'The workflow revision is missing.'
  jq -e '
    def sha: type=="string" and test("^[a-f0-9]{64}$");
    def run: type=="string" and test("^[1-9][0-9]{0,19}$");
    .schema_version==1 and (.campaign_id | type=="string" and test("^task-017-12-[a-z0-9-]{1,48}$")) and
    (.stage=="preflight" or .stage=="baseline" or .stage=="stability") and
    (.candidate_id=="dev-amd64" or .candidate_id=="dev-arm64") and
    (.approval|type=="object") and (.approval.sha256|sha) and (.baseline_run_ids|type=="object") and
    (if .stage=="preflight" then .preflight_run_id==null and .baseline_run_ids=={}
     elif .stage=="baseline" then (.preflight_run_id|run) and .baseline_run_ids=={}
     else (.preflight_run_id|run) and (.baseline_run_ids|keys)==["dev-amd64","dev-arm64"] and all(.baseline_run_ids[];run) and
       ([.preflight_run_id,.baseline_run_ids[]]|length)==([.preflight_run_id,.baseline_run_ids[]]|unique|length) end)
  ' "$request" >/dev/null 2>&1 || fail 'The campaign request is invalid.'
  approval="$(resolve_file "$root" "$(jq -r .approval.path "$request")" "$(jq -r .approval.sha256 "$request")")"
  check_json "$approval"
  jq -e --slurpfile r "$request" --arg control "$CAMPAIGN_CONTROL_REVISION" '
    def revision: type=="string" and test("^[a-f0-9]{40}$");
    def digest: type=="string" and test("^sha256:[a-f0-9]{64}$");
    .schema_version==1 and .status=="approved" and .phase=="pre_pr216_merge" and
    .campaign_id==$r[0].campaign_id and .harness_revision==$control and (.external_harness_revision|revision) and
    (.candidates|keys)==["dev-amd64","dev-arm64"] and
    (.preflight_candidate_id=="dev-amd64" or .preflight_candidate_id=="dev-arm64") and
    ($r[0].stage!="preflight" or .preflight_candidate_id==$r[0].candidate_id) and
    (.source_digests|type=="object" and length>=6 and length<=512) and
    all(.source_digests[];type=="string" and test("^[a-f0-9]{64}$")) and
    (.candidates["dev-amd64"].platform=="linux/amd64") and (.candidates["dev-arm64"].platform=="linux/arm64") and
    (.candidates["dev-amd64"].node_revision==.candidates["dev-arm64"].node_revision) and
    all(.candidates[];(.node_revision|revision) and (.image_digest|digest) and (.image_config_digest|digest) and (.node_binary_digest|digest)) and
    (.stages[$r[0].stage] as $s | $s.approved==true and $s.memory_gb==64 and
      if $r[0].stage=="preflight" then $s.duration_seconds==0 and $s.runner_max_seconds==14400 and $s.max_launches==1
      elif $r[0].stage=="baseline" then $s.duration_seconds==86400 and $s.runner_max_seconds==93600 and $s.max_launches==2
      else $s.duration_seconds==216000 and $s.runner_max_seconds==230400 and $s.max_launches==2 end)
  ' "$approval" >/dev/null 2>&1 || fail 'The campaign approval does not authorize this request.'
  for path in scripts/casper-soak/campaign.sh scripts/casper-soak/test-campaign.sh .github/workflows/merge-recovery-soak.yml .github/actions/soak-segment/action.yml scripts/run-merge-recovery-soak.sh scripts/run-integration-preflight.sh; do
    jq -e --arg path "$path" '.source_digests|has($path)' "$approval" >/dev/null || fail 'A required control source is not pinned.'
  done
  source_rows="$(jq -r '.source_digests|to_entries[]|[.key,.value]|@tsv' "$approval")" || fail 'The control source inventory cannot be read.'
  while IFS=$'\t' read -r path digest; do
    resolve_file "$root" "$path" "$digest" >/dev/null
  done <<< "$source_rows"
  local member
  for member in dev-amd64 dev-arm64; do
    workload="$(resolve_file "$root" "$(jq -r --arg id "$member" '.candidates[$id].workload.path' "$approval")" "$(jq -r --arg id "$member" '.candidates[$id].workload.sha256' "$approval")")"
    qualification="$(resolve_file "$root" "$(jq -r --arg id "$member" '.candidates[$id].qualification.path' "$approval")" "$(jq -r --arg id "$member" '.candidates[$id].qualification.sha256' "$approval")")"
    check_json "$workload"
    check_json "$qualification"
    jq -e --slurpfile a "$approval" '
      .schema_version==1 and .policy_variant=="current-dev-load" and
      .external_harness_revision==$a[0].external_harness_revision and
      ((.profile_id=="current-dev-load" and .providers==["docker","subprocess"] and
        .test_path=="integration-tests/test/tests/custom/test_load.py" and (.test_sha256|type=="string" and test("^[a-f0-9]{64}$")) and
        .preflight_profile_path=="integration-tests/test/full-suite.txt" and (.preflight_profile_sha256|type=="string" and test("^[a-f0-9]{64}$"))) or
       (.profile_id=="casper-authority-publication" and .providers==["docker"] and
        (.entrypoint.path|type=="string") and (.entrypoint.sha256|type=="string" and test("^[a-f0-9]{64}$")) and
        .required_capabilities==["authority_finality","publication"])) and
      (.required_capabilities|type=="array" and length>0 and length<=32) and
      all(.required_capabilities[];type=="string" and test("^[a-z][a-z0-9_-]{0,63}$")) and
      (.required_capabilities|length)==(.required_capabilities|unique|length)
    ' "$workload" >/dev/null 2>&1 || fail 'The executable workload is not supported.'
    jq -e --slurpfile a "$approval" --slurpfile w "$workload" --arg id "$member" '
      $a[0].candidates[$id] as $c | . as $q |
      .schema_version==1 and .status=="qualified" and .evidence_kind=="node_observation" and .candidate_id==$id and
      .node_revision==$c.node_revision and .node_binary_digest==$c.node_binary_digest and .image_digest==$c.image_digest and
      .workload_sha256==$c.workload.sha256 and all($w[0].required_capabilities[]; $q.capabilities[.]=="qualified")
    ' "$qualification" >/dev/null 2>&1 || fail 'A required live capability is not qualified for these exact inputs.'
  done
  local identity
  identity="$(jq -cS '{campaign_id,phase,harness_revision,external_harness_revision,source_digests,candidates,preflight_candidate_id}' "$approval" | sha256sum | cut -d ' ' -f1)"
  jq -n --slurpfile a "$approval" --slurpfile r "$request" --arg identity "$identity" --arg approval_digest "$(hash_file "$approval")" '
    $a[0] as $a | $r[0] as $r | $a.candidates[$r.candidate_id] as $c | $a.stages[$r.stage] as $s |
    {schema_version:1,campaign_id:$r.campaign_id,stage:$r.stage,candidate_id:$r.candidate_id,
     phase:$a.phase,campaign_candidates:$a.candidates,
     identity_digest:$identity,approval_digest:$approval_digest,harness_revision:$a.harness_revision,
     external_harness_revision:$a.external_harness_revision,node_revision:$c.node_revision,platform:$c.platform,
     runner_arch:(if $c.platform=="linux/amd64" then "x64" else "arm64" end),
     launcher_arch:($c.platform|split("/")[1]),image_digest:$c.image_digest,image_config_digest:$c.image_config_digest,
     node_binary_digest:$c.node_binary_digest,image_reference:("docker.io/f1r3flyindustries/f1r3fly-rust@"+$c.image_digest),
     workload:$c.workload,qualification:$c.qualification,duration_seconds:$s.duration_seconds,
     runner_max_seconds:$s.runner_max_seconds,memory_gb:$s.memory_gb,max_launches:1,cleanup_reserve_seconds:600,
     required_prior_runs:((if $r.stage=="preflight" then [] else [{run_id:$r.preflight_run_id,stage:"preflight",candidate_id:$a.preflight_candidate_id}] end) +
       (if $r.stage=="stability" then [$r.baseline_run_ids|to_entries[]|{run_id:.value,stage:"baseline",candidate_id:.key}] else [] end)),
     admission:"requires-external-checks",node_launch_count:0,soak_verdict:"pending"}
  '
}
window() {
  [[ $# == 3 ]] || fail 'The window command requires a plan and two epoch values.'
  local plan_file="$1" created="$2" now="$3"
  check_json "$plan_file"
  [[ "$created" =~ ^[1-9][0-9]{0,9}$ && "$now" =~ ^[1-9][0-9]{0,9}$ ]] || fail 'The epoch value is invalid.'
  jq -e --argjson created "$created" --argjson now "$now" '
    .schema_version==1 and .cleanup_reserve_seconds==600 and .memory_gb==64 and
    ((.stage=="baseline" and .duration_seconds==86400 and .runner_max_seconds==93600) or
     (.stage=="stability" and .duration_seconds==216000 and .runner_max_seconds==230400)) and
    $created<=4102444800 and $now>= $created and
    $now+.duration_seconds+.cleanup_reserve_seconds<= $created+.runner_max_seconds
  ' "$plan_file" >/dev/null 2>&1 || fail 'The remaining runner budget cannot contain the full workload and cleanup.'
  jq --argjson created "$created" --argjson now "$now" '{schema_version:1,duration_seconds,workload_start_epoch:$now,workload_deadline_epoch:($now+.duration_seconds),runner_created_epoch:$created,runner_deadline_epoch:($created+.runner_max_seconds),cleanup_reserve_seconds}' "$plan_file"
}
api_get() {
  local endpoint="$1" output="$2"
  (ulimit -f 2048; timeout --signal=TERM --kill-after=5 60 gh api --hostname github.com --method GET "$endpoint" > "$output") 2> "$output.stderr" || fail 'The prior-run evidence download failed.'
}
prior() {
  [[ $# == 5 ]] || fail 'The prior command requires a plan, run, result, stage, and candidate.'
  local plan_file="$1" run_file="$2" result_file="$3" stage="$4" candidate="$5"
  check_json "$plan_file"
  check_json "$run_file"
  check_json "$result_file"
  jq -e --slurpfile p "$plan_file" --slurpfile r "$run_file" --arg stage "$stage" --arg candidate "$candidate" '
    $p[0] as $p | $r[0] as $r | $p.campaign_candidates[$candidate] as $c |
    .schema_version==1 and .run_id==($r.id|tostring) and .run_attempt==1 and
    .repository=="F1R3FLY-io/f1r3node-rust" and .admission=="admitted" and .result=="passed" and
    .evidence_kind=="node_observation" and .cloud_launch_count==1 and
    (.node_launch_count|type=="number" and floor==. and .>0) and
    .cleanup.complete==true and .host_protection.status=="passed" and
    .plan.identity_digest==$p.identity_digest and .plan.campaign_id==$p.campaign_id and
    .plan.harness_revision==$p.harness_revision and .plan.external_harness_revision==$p.external_harness_revision and
    .plan.phase==$p.phase and .plan.stage==$stage and .plan.candidate_id==$candidate and
    .plan.node_revision==$c.node_revision and .plan.platform==$c.platform and
    .plan.image_digest==$c.image_digest and .plan.image_config_digest==$c.image_config_digest and
    .plan.node_binary_digest==$c.node_binary_digest and .plan.workload==$c.workload and .plan.qualification==$c.qualification and
    (if $stage=="preflight" then .plan.duration_seconds==0 and .integration_preflight=="passed"
     elif $stage=="baseline" then .plan.duration_seconds==86400 and .soak_verdict=="passed" and
       (.workload_elapsed_seconds|type=="number" and floor==. and .>=86400) and
       .measurement_completeness=="complete" and .profile_verdicts.authority_finality=="passed" and .profile_verdicts.publication=="passed"
     else false end)
  ' "$result_file" >/dev/null 2>&1 || fail 'The prior result does not satisfy this campaign stage.'
}
verify_prior_runs() {
  local plan_file="$1" output="$2" rows run_id stage candidate dir artifact_id digest
  rows="$(jq -r '.required_prior_runs[]|[.run_id,.stage,.candidate_id]|@tsv' "$plan_file")" || fail 'The prior-run inventory cannot be read.'
  [[ -n "$rows" ]] || return 0
  while IFS=$'\t' read -r run_id stage candidate; do
    [[ "$run_id" =~ ^[1-9][0-9]{0,19}$ && "$run_id" != "$GITHUB_RUN_ID" ]] || fail 'A prior-run identifier is invalid or references this run.'
    dir="$output/prior-$run_id"
    mkdir "$dir"
    api_get "repos/F1R3FLY-io/f1r3node-rust/actions/runs/$run_id" "$dir/run.json"
    check_json "$dir/run.json"
    jq -e --arg id "$run_id" --slurpfile p "$plan_file" '
      (.id|tostring)==$id and .run_attempt==1 and .path==".github/workflows/merge-recovery-soak.yml" and
      .repository.full_name=="F1R3FLY-io/f1r3node-rust" and .head_sha==$p[0].harness_revision and
      .event=="workflow_dispatch" and .status=="completed" and .conclusion=="success"
    ' "$dir/run.json" >/dev/null 2>&1 || fail 'The prior run is not a successful first-attempt campaign run at this revision.'
    api_get "repos/F1R3FLY-io/f1r3node-rust/actions/runs/$run_id/artifacts?per_page=100" "$dir/artifacts.json"
    check_json "$dir/artifacts.json"
    jq -e --arg name "casper-campaign-result-$run_id-1" --arg id "$run_id" --slurpfile p "$plan_file" '
      (.artifacts|type=="array") and .total_count==(.artifacts|length) and .total_count<=100 and
      ([.artifacts[]|select(.name==$name)] as $a | ($a|length)==1 and
       ($a[0] | .expired==false and (.id|type=="number" and .>0 and floor==.) and
        (.size_in_bytes|type=="number" and .>0 and .<=2097152) and
        (.digest|type=="string" and test("^sha256:[a-f0-9]{64}$")) and
        (.workflow_run.id|tostring)==$id and .workflow_run.head_sha==$p[0].harness_revision))
    ' "$dir/artifacts.json" >/dev/null 2>&1 || fail 'The prior result artifact is missing, ambiguous, expired, or mismatched.'
    jq --arg name "casper-campaign-result-$run_id-1" '.artifacts[]|select(.name==$name)' "$dir/artifacts.json" > "$dir/artifact.json"
    artifact_id="$(jq -r .id "$dir/artifact.json")"
    [[ "$artifact_id" =~ ^[1-9][0-9]{0,19}$ ]] || fail 'The artifact identifier is invalid.'
    digest="$(jq -r .digest "$dir/artifact.json")"
    api_get "repos/F1R3FLY-io/f1r3node-rust/actions/artifacts/$artifact_id/zip" "$dir/result.zip"
    [[ "sha256:$(hash_file "$dir/result.zip")" == "$digest" ]] || fail 'The downloaded result archive differs from its digest.'
    [[ "$(stat -c %s "$dir/result.zip")" == "$(jq -r .size_in_bytes "$dir/artifact.json")" ]] || fail 'The downloaded archive size differs.'
    unzip -Z1 "$dir/result.zip" > "$dir/members.txt" 2> "$dir/members.stderr" || fail 'The result archive cannot be listed.'
    [[ "$(< "$dir/members.txt")" == campaign-result.json ]] || fail 'The result archive must contain exactly one named result.'
    zipinfo -l "$dir/result.zip" > "$dir/zipinfo.txt" || fail 'The result archive metadata cannot be read.'
    awk '$NF=="campaign-result.json" {if(substr($1,1,1)!="-") exit 1; n++} END {if(n!=1) exit 1}' "$dir/zipinfo.txt" || fail 'The archived result is not a regular file.'
    (unzip -p "$dir/result.zip" campaign-result.json | head -c 1048577) > "$dir/result.json" 2> "$dir/result.stderr" || fail 'The result cannot be read within its limit.'
    prior "$plan_file" "$dir/run.json" "$dir/result.json" "$stage" "$candidate"
  done <<< "$rows"
}
dispatch() (
  [[ $# == 4 ]] || fail 'The dispatch command requires a source root, request, selection, and new output directory.'
  root="$(realpath -e -- "$1")" || fail 'The source root does not exist.'
  request="$2" selection="$3" out="$4"
  [[ ! -e "$out" && ! -L "$out" ]] || fail 'The evidence directory already exists.'
  mkdir -p -- "$out"
  out="$(realpath -e -- "$out")"
  planning=not-run prior_runs=not-run reason=invalid_dispatch_input
  trap 'status=$?; jq -n --argjson status "$status" --arg planning "$planning" --arg prior "$prior_runs" --arg reason "$reason" \
    '\''{schema_version:1,scope:"manual-campaign-admission-only",exit_code:$status,admission:(if $status==3 then "blocked" else "invalid_input" end),planning:$planning,prior_runs:$prior,reason:$reason,cloud_launch_count:0,node_launch_count:0,soak_verdict:"non_passing",task_complete:false,claim_discharge:"pending",execution_enabled:false}'\'' > "$out/report.json" || exit 2' EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
  [[ -f "$request" && "$(stat -c %s "$request")" -le 1048576 ]] || fail 'The request is missing or exceeds its byte limit.'
  cp -- "$request" "$out/request.json"
  request="$out/request.json"
  check_json "$request"
  [[ "${GITHUB_EVENT_NAME:-}" == workflow_dispatch && "${GITHUB_RUN_ATTEMPT:-}" == 1 && "${GITHUB_RUN_ID:-}" =~ ^[1-9][0-9]{0,19}$ ]] || fail 'Only a first-attempt manual campaign run is permitted.'
  [[ "${GITHUB_REPOSITORY:-}" == F1R3FLY-io/f1r3node-rust ]] || fail 'The campaign repository is not supported.'
  [[ "${INPUT_TARGET_REF:-dev}" == dev && "${INPUT_RETRY_ATTEMPT:-0}" == 0 ]] || fail 'Legacy target and retry overrides are not permitted.'
  [[ -z "${INPUT_SCHEDULED_SLOT:-}${INPUT_WINDOW_END:-}${INPUT_SERIES:-}${INPUT_RESTART_OF_RUN_ID:-}${INPUT_CANDIDATE_TAG:-}" ]] || fail 'Campaign mode cannot use legacy scheduling, restart, or candidate inputs.'
  for value in "${INPUT_PREFLIGHT_ONLY:-false}" "${INPUT_SKIP_PREFLIGHT:-false}" "${INPUT_CANARY:-false}" "${INPUT_INJECT_PROTECTION_BREACH:-false}"; do
    [[ "$value" == false ]] || fail 'Campaign mode cannot use legacy preflight, canary, or injection flags.'
  done
  case "$selection" in
    campaign-preflight) stage=preflight ;;
    campaign-baseline-24h) stage=baseline ;;
    campaign-stability-60h) stage=stability ;;
    *) fail 'Select an explicit campaign duration.' ;;
  esac
  jq -e --arg stage "$stage" '.stage==$stage' "$request" >/dev/null || fail 'The campaign selection and request stage differ.'
  [[ "$(hash_file "${BASH_SOURCE[0]}")" == "$(hash_file "$root/scripts/casper-soak/campaign.sh")" ]] || fail 'The executing helper differs from the supplied source root.'
  planning=failed reason=planning_rejected
  if bash "$root/scripts/casper-soak/campaign.sh" plan "$root" "$request" > "$out/plan.json" 2> "$out/plan.stderr"; then planning=passed; else exit "$?"; fi
  prior_runs=failed reason=prior_run_rejected
  verify_prior_runs "$out/plan.json" "$out"
  prior_runs=verified-records-only
  if [[ "$stage" == preflight ]]; then prior_runs=not-required; fi
  reason=campaign_execution_not_implemented
  jq -n '{schema_version:1,blockers:["eligible_casper_workload_missing","authority_publication_live_adapters_unqualified","source_bound_acceptance_pending","approval_authentication_missing","persistent_launch_reservation_missing","independent_instance_lifetime_enforcement_missing"],baseline_machine_limit:3,replacement_launches:0,repeat_launches:0,stability_resource_decision:"required",recovery_qualification:"requires_actual_pr216_merge"}' > "$out/blockers.json"
  printf 'Campaign blocked: execution prerequisites remain unmet. No node or cloud runner launched.\n' >&2
  exit 3
)
case "${1:-}" in
  plan) shift; plan "$@" ;;
  window) shift; window "$@" ;;
  prior) shift; prior "$@" ;;
  verify-priors) shift; [[ $# == 2 ]] || fail "The prior verifier requires a plan and output directory."; verify_prior_runs "$@" ;;
  dispatch) shift; dispatch "$@" ;;
  *) fail 'Select the plan, window, prior, or dispatch command.' ;;
esac
