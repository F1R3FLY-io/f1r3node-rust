#!/usr/bin/env bash
set -euo pipefail
fail() {
  printf 'Campaign rejected: %s\n' "$1" >&2
  exit 2
}
hash_file() { sha256sum "$1" | cut -d ' ' -f1; }
check_json() {
  [[ -f "$1" && "$(stat -c %s "$1")" -le 1048576 ]] || fail 'The JSON input is missing or exceeds its byte limit.'
  jq -e 'type=="object"' "$1" >/dev/null 2>&1 || fail 'The input is not a JSON object.'
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
    (.source_digests|type=="object" and length>=5 and length<=512) and
    all(.source_digests[];type=="string" and test("^[a-f0-9]{64}$")) and
    (.candidates["dev-amd64"].platform=="linux/amd64") and (.candidates["dev-arm64"].platform=="linux/arm64") and
    (.candidates["dev-amd64"].node_revision==.candidates["dev-arm64"].node_revision) and
    all(.candidates[];(.node_revision|revision) and (.image_digest|digest) and (.image_config_digest|digest) and (.node_binary_digest|digest)) and
    (.stages[$r[0].stage] as $s | $s.approved==true and $s.memory_gb==64 and
      if $r[0].stage=="preflight" then $s.duration_seconds==0 and $s.runner_max_seconds==14400 and $s.max_launches==1
      elif $r[0].stage=="baseline" then $s.duration_seconds==86400 and $s.runner_max_seconds==93600 and $s.max_launches==2
      else $s.duration_seconds==216000 and $s.runner_max_seconds==230400 and $s.max_launches==2 end)
  ' "$approval" >/dev/null 2>&1 || fail 'The campaign approval does not authorize this request.'
  for path in scripts/casper-soak/campaign.sh .github/workflows/merge-recovery-soak.yml .github/actions/soak-segment/action.yml scripts/run-merge-recovery-soak.sh scripts/run-integration-preflight.sh; do
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
      .schema_version==1 and .profile_id=="current-dev-load" and .policy_variant=="current-dev-load" and
      .external_harness_revision==$a[0].external_harness_revision and .providers==["docker","subprocess"] and
      .test_path=="integration-tests/test/tests/custom/test_load.py" and (.test_sha256|type=="string" and test("^[a-f0-9]{64}$")) and
      .preflight_profile_path=="integration-tests/test/full-suite.txt" and (.preflight_profile_sha256|type=="string" and test("^[a-f0-9]{64}$")) and
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
case "${1:-}" in
  plan) shift; plan "$@" ;;
  window) shift; window "$@" ;;
  *) fail 'Select the plan or window command.' ;;
esac
