#!/usr/bin/env bash
set -euo pipefail
REPOSITORY=F1R3FLY-io/f1r3node-rust
SOAK_WORKFLOW=.github/workflows/merge-recovery-soak.yml
ISSUE=473
SOURCE_PULL=436
WINDOW_SECONDS=216000
BOT='github-actions[bot]'
fail() {
  printf 'Obligation check rejected: %s\n' "$1" >&2
  exit 2
}
hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f1
  else
    shasum -a 256 "$1" | cut -d ' ' -f1
  fi
}
size_of() { wc -c < "$1" | tr -d '[:space:]'; }
check_json() {
  [[ -f "$1" && "$(size_of "$1")" -le 1048576 ]] || fail 'A JSON input is missing or exceeds its byte limit.'
  jq -se 'length==1 and (.[0]|type=="object")' "$1" >/dev/null 2>&1 || fail 'An input must contain exactly one JSON object.'
}
new_directory() {
  [[ ! -e "$1" && ! -L "$1" ]] || fail 'The evidence directory already exists.'
  mkdir -p -- "$1"
}
attempt() {
  set +e
  (set -e; "$@")
  code=$?
  set -e
}
limited() {
  if command -v timeout >/dev/null 2>&1; then
    timeout --signal=TERM --kill-after=5 60 "$@"
  else
    "$@"
  fi
}
api_get() {
  local endpoint="$1" output="$2"
  limited gh api --hostname github.com --method GET "$endpoint" > "$output" 2> "$output.stderr" || fail 'A GitHub request failed.'
}
api_send() {
  local method="$1" endpoint="$2" payload="$3" output="$4"
  limited gh api --hostname github.com --method "$method" "$endpoint" --input "$payload" > "$output" 2> "$output.stderr" || fail 'A GitHub update failed.'
}
comparison() {
  local base="$1" head="$2" output="$3"
  limited gh api --hostname github.com --method GET "repos/$REPOSITORY/compare/$base...$head?per_page=1" 2> "$output.stderr" |
    jq -c --arg base "$base" --arg head "$head" '{base:$base,head:$head,status,behind_by}' > "$output" || fail 'A revision comparison failed.'
  check_json "$output"
}
evaluate() {
  [[ $# == 5 ]] || fail 'The evaluate command requires a run, artifact, result, facts, and new output directory.'
  local run="$1" artifact="$2" result="$3" facts="$4" out="$5"
  check_json "$run"
  check_json "$artifact"
  check_json "$result"
  check_json "$facts"
  jq -e --slurpfile r "$run" --slurpfile a "$artifact" --slurpfile f "$facts" \
    --arg repository "$REPOSITORY" --arg workflow "$SOAK_WORKFLOW" --arg digest "$(hash_file "$result")" '
    $r[0] as $r | $a[0] as $a | $f[0] as $f |
    ($r.id|type=="number" and floor==. and .>0) and
    $r.repository.full_name==$repository and $r.path==$workflow and
    .schema_version==1 and .repository==$repository and .run_id==($r.id|tostring) and
    (.plan|type=="object") and (.plan.stage|type=="string") and
    $a.name==("casper-campaign-result-"+($r.id|tostring)+"-1") and $a.expired==false and
    $a.workflow_run.id==$r.id and
    $f.schema_version==1 and $f.result_sha256==$digest and
    ($f.result_archive_sha256|type=="string" and test("^[a-f0-9]{64}$")) and
    $a.digest==("sha256:"+$f.result_archive_sha256)
  ' "$result" >/dev/null 2>&1 || fail 'The run, artifact, result, and facts do not describe one campaign result.'
  if ! jq -e '.plan.stage=="stability"' "$result" >/dev/null; then
    printf 'Not applicable: the campaign stage is not the 60-hour stability stage.\n' >&2
    exit 4
  fi
  jq -e --slurpfile r "$run" --slurpfile f "$facts" '
    def revision: type=="string" and test("^[a-f0-9]{40}$");
    def digest: type=="string" and test("^sha256:[a-f0-9]{64}$");
    def count: type=="number" and floor==. and .>=0;
    def compared: type=="object" and (.base|revision) and (.head|revision) and (.behind_by|count) and
      (.status=="ahead" or .status=="identical" or .status=="behind" or .status=="diverged");
    $r[0] as $r | $f[0] as $f | .plan as $p |
    ($r.run_attempt|count) and ($r.event|type=="string" and test("^[a-z_]{1,32}$")) and
    ($r.status|type=="string" and test("^[a-z_]{1,32}$")) and
    ($r.conclusion==null or ($r.conclusion|type=="string" and test("^[a-z_]{1,32}$"))) and
    ($p.node_revision|revision) and ($p.harness_revision|revision) and
    ($p.platform=="linux/amd64" or $p.platform=="linux/arm64") and
    ($p.candidate_id|type=="string" and test("^[a-z0-9][a-z0-9-]{0,31}$")) and
    ($p.phase|type=="string" and test("^[a-z0-9_]{1,32}$")) and
    ($p.image_digest|digest) and ($p.image_config_digest|digest) and ($p.node_binary_digest|digest) and
    ($p.workload.sha256|type=="string" and test("^[a-f0-9]{64}$")) and
    ($p.workload.path|type=="string" and test("^[A-Za-z0-9_.][A-Za-z0-9_./-]{0,199}$")) and
    ($p.duration_seconds|count) and
    (.seeds==null or (.seeds|type=="array" and length>0 and length<=64 and all(.[];count))) and
    ((.started_epoch==null and .finished_epoch==null) or
     ((.started_epoch|count) and (.finished_epoch|count) and .finished_epoch>=.started_epoch)) and
    ($f.master_revision|revision) and
    ($f.revision_in_master|compared) and
    $f.revision_in_master.base==$p.node_revision and $f.revision_in_master.head==$f.master_revision and
    (if $f.required_commit_source=="pull_request_not_merged"
     then $f.required_commit==null and $f.required_in_revision==null
     else ($f.required_commit_source=="pull_request" or $f.required_commit_source=="dispatch_input") and
       ($f.required_commit|revision) and ($f.required_in_revision|compared) and
       $f.required_in_revision.base==$f.required_commit and $f.required_in_revision.head==$p.node_revision end)
  ' "$result" >/dev/null 2>&1 || fail 'The stability result or its facts have a field that is not valid.'
  new_directory "$out"
  jq -n --slurpfile r "$run" --slurpfile x "$result" --slurpfile f "$facts" \
    --arg workflow "$SOAK_WORKFLOW" --argjson window "$WINDOW_SECONDS" --argjson issue "$ISSUE" '
    def ancestor: (.status=="ahead" or .status=="identical") and .behind_by==0;
    def known($values): if . as $v | $values | index($v) then . else "unknown" end;
    $r[0] as $r | $x[0] as $x | $f[0] as $f | $x.plan as $p |
    [{id:1,criterion:"The soaked revision is on master and contains the required commit.",
      satisfied:($f.required_in_revision!=null and ($f.required_in_revision|ancestor) and ($f.revision_in_master|ancestor))},
     {id:2,criterion:"The workflow is Merge Recovery Soak, with the stage campaign-stability-60h.",
      satisfied:($r.path==$workflow and $r.event=="workflow_dispatch" and $p.stage=="stability")},
     {id:3,criterion:"The workload window is 216,000 seconds, and the run completes the full window.",
      satisfied:($p.duration_seconds==$window and
        ($x.workload_elapsed_seconds|type=="number" and floor==. and .>=$window) and
        $x.measurement_completeness=="complete")},
     {id:4,criterion:"The terminal verdict is a pass.",
      satisfied:($r.status=="completed" and $r.conclusion=="success" and $r.run_attempt==1 and
        $x.run_attempt==1 and $x.result=="passed" and $x.soak_verdict=="passed" and
        $x.admission=="admitted" and $x.evidence_kind=="node_observation" and $x.cloud_launch_count==1 and
        ($x.node_launch_count|type=="number" and floor==. and .>0) and
        $x.cleanup.complete==true and $x.host_protection.status=="passed")}] as $criteria |
    {schema_version:1,obligation:"O1",issue:$issue,run_id:($r.id|tostring),run_attempt:$r.run_attempt,
     run_event:$r.event,run_status:$r.status,run_conclusion:($r.conclusion // "none"),
     candidate_id:$p.candidate_id,platform:$p.platform,architecture:($p.platform|split("/")[1]),
     phase:$p.phase,stage:$p.stage,node_revision:$p.node_revision,harness_revision:$p.harness_revision,
     image_digest:$p.image_digest,image_config_digest:$p.image_config_digest,
     node_binary_digest:$p.node_binary_digest,workload:{path:$p.workload.path,sha256:$p.workload.sha256},
     duration_seconds:$p.duration_seconds,
     workload_elapsed_seconds:(if ($x.workload_elapsed_seconds|type)=="number" then $x.workload_elapsed_seconds else null end),
     measurement_completeness:($x.measurement_completeness|known(["complete","partial","missing"])),
     result:($x.result|known(["passed","non_passing","pending"])),
     soak_verdict:($x.soak_verdict|known(["passed","non_passing","pending"])),
     required_commit:$f.required_commit,required_commit_source:$f.required_commit_source,
     master_revision:$f.master_revision,result_archive_sha256:$f.result_archive_sha256,
     result_sha256:$f.result_sha256,criteria:$criteria,
     verdict:(if all($criteria[];.satisfied) then "satisfied" else "not_satisfied" end),
     seeds:$x.seeds,workload_window_start:$x.started_epoch,workload_window_end:$x.finished_epoch,
     unavailable_evidence:([(if $x.seeds==null then "seeds" else empty end),
       (if $x.started_epoch==null then "workload_window_start" else empty end),
       (if $x.finished_epoch==null then "workload_window_end" else empty end)])}
  ' > "$out/verdict.json"
  jq -e '.verdict=="satisfied"' "$out/verdict.json" >/dev/null || exit 3
}
comment() {
  [[ $# == 1 ]] || fail 'The comment command requires a verdict.'
  check_json "$1"
  jq -e '.schema_version==1 and .obligation=="O1" and (.verdict=="satisfied" or .verdict=="not_satisfied")' "$1" >/dev/null 2>&1 ||
    fail 'The verdict record is not valid.'
  jq -r '
    "<!-- soak-obligation O1 run=\(.run_id) architecture=\(.architecture) revision=\(.node_revision) verdict=\(.verdict) -->",
    "## O1: 60-hour stability soak, \(.architecture)",
    "",
    "**Result for this architecture:** \(if .verdict=="satisfied" then "criteria 1 to 4 are satisfied" else "not satisfied" end)",
    "",
    "| Item | Value |",
    "|------|-------|",
    "| Run ID and attempt | \(.run_id), attempt \(.run_attempt) |",
    "| Run state | \(.run_status), \(.run_conclusion) |",
    "| Candidate and platform | `\(.candidate_id)`, `\(.platform)` |",
    "| Soaked revision | `\(.node_revision)` |",
    "| Harness revision | `\(.harness_revision)` |",
    "| Required commit | \(if .required_commit then "`\(.required_commit)`" else "not available" end) (\(.required_commit_source)) |",
    "| Revision of master at the check | `\(.master_revision)` |",
    "| Image digest | `\(.image_digest)` |",
    "| Image configuration digest | `\(.image_config_digest)` |",
    "| Node binary digest | `\(.node_binary_digest)` |",
    "| Workload definition | `\(.workload.path)`, `sha256:\(.workload.sha256)` |",
    "| Requested window | \(.duration_seconds) seconds |",
    "| Completed workload time | \(if .workload_elapsed_seconds then "\(.workload_elapsed_seconds) seconds" else "not recorded" end) |",
    "| Measurement completeness | \(.measurement_completeness) |",
    "| Seeds | \(if .seeds then (.seeds|map(tostring)|join(", ")) else "not in the result record" end) |",
    "| Start of the workload window | \(if .workload_window_start then (.workload_window_start|todate) else "not in the result record" end) |",
    "| End of the workload window | \(if .workload_window_end then (.workload_window_end|todate) else "not in the result record" end) |",
    "| Terminal verdict | \(.soak_verdict) |",
    "| Digest of the result archive | `sha256:\(.result_archive_sha256)` |",
    "| Digest of the result record | `sha256:\(.result_sha256)` |",
    "",
    "| # | Criterion | Satisfied |",
    "|---|-----------|-----------|",
    (.criteria[] | "| \(.id) | \(.criterion) | \(if .satisfied then "yes" else "no" end) |"),
    "",
    (if (.unavailable_evidence|length)>0
     then "The result record does not contain these evidence items: \(.unavailable_evidence|join(", ")).", ""
     else empty end),
    (if .verdict=="satisfied"
     then "Criterion 5 needs a satisfied result for the other architecture at the same revision."
     else "The obligation stays open. This result stays in the record." end)
  ' "$1"
}
pair() {
  [[ $# == 3 ]] || fail 'The pair command requires two verdicts and a new output directory.'
  local first="$1" second="$2" out="$3"
  check_json "$first"
  check_json "$second"
  jq -e --slurpfile b "$second" '
    $b[0] as $b |
    .schema_version==1 and $b.schema_version==1 and .obligation=="O1" and $b.obligation=="O1" and
    .verdict=="satisfied" and $b.verdict=="satisfied" and .run_id!=$b.run_id and
    ([.architecture,$b.architecture]|sort)==["amd64","arm64"] and
    .node_revision==$b.node_revision and .required_commit==$b.required_commit and
    .required_commit!=null and
    (.unavailable_evidence|type=="array") and ($b.unavailable_evidence|type=="array")
  ' "$first" >/dev/null 2>&1 || exit 3
  new_directory "$out"
  jq -n --slurpfile a "$first" --slurpfile b "$second" '
    {schema_version:1,obligation:"O1",status:"complete",node_revision:$a[0].node_revision,
     required_commit:$a[0].required_commit,
     unavailable_evidence:(($a[0].unavailable_evidence+$b[0].unavailable_evidence)|unique),
     evidence_complete:(($a[0].unavailable_evidence+$b[0].unavailable_evidence)==[]),
     runs:([$a[0],$b[0]]|sort_by(.architecture)|map({run_id,architecture,result_sha256}))}
  ' > "$out/obligation.json"
}
completion() {
  [[ $# == 1 ]] || fail 'The completion command requires an obligation record.'
  check_json "$1"
  jq -r '
    "<!-- soak-obligation O1 complete revision=\(.node_revision) -->",
    "## O1: criteria 1 to 5 are satisfied",
    "",
    "| Architecture | Run ID | Digest of the result record |",
    "|--------------|--------|-----------------------------|",
    (.runs[] | "| \(.architecture) | \(.run_id) | `sha256:\(.result_sha256)` |"),
    "",
    "Soaked revision: `\(.node_revision)`",
    "",
    (if .evidence_complete
     then "The check marks obligation O1 in the issue text. The check does not close this issue."
     else "The check does not mark obligation O1. The result records do not contain these evidence items: \(.unavailable_evidence|join(", ")).",
       "",
       "A maintainer must add these items to this record and then mark obligation O1." end)
  ' "$1"
}
mark() {
  [[ $# == 1 ]] || fail 'The mark command requires the issue text.'
  [[ -f "$1" && "$(size_of "$1")" -le 262144 ]] || fail 'The issue text is missing or exceeds its byte limit.'
  awk '
    /^- \[ \] \*\*O1\. / {open++}
    /^- \[x\] \*\*O1\. / {marked++}
    END {exit !((open==1 && marked==0) || (open==0 && marked==1))}
  ' "$1" || fail 'The issue text must contain exactly one line for obligation O1.'
  awk '/^- \[ \] \*\*O1\. / {sub(/^- \[ \]/,"- [x]")} {print}' "$1"
}
collect() (
  [[ $# == 2 || $# == 3 ]] || fail 'The collect command requires a run, a new output directory, and an optional required commit.'
  run_id="$1" out="$2" required="${3:-}"
  [[ "$run_id" =~ ^[1-9][0-9]{0,19}$ ]] || fail 'The run identifier is not valid.'
  [[ -z "$required" || "$required" =~ ^[a-f0-9]{40}$ ]] || fail 'The required commit is not valid.'
  new_directory "$out"
  out="$(cd "$out" && pwd)"
  api_get "repos/$REPOSITORY/actions/runs/$run_id" "$out/run.json"
  check_json "$out/run.json"
  jq -e --arg id "$run_id" '(.id|tostring)==$id' "$out/run.json" >/dev/null 2>&1 || fail 'The run record has a different identifier.'
  if ! jq -e --arg workflow "$SOAK_WORKFLOW" '.path==$workflow' "$out/run.json" >/dev/null; then
    printf 'Not applicable: the run is not a soak workflow run.\n' >&2
    exit 4
  fi
  jq -e '.status=="completed"' "$out/run.json" >/dev/null || fail 'The soak run is not complete.'
  api_get "repos/$REPOSITORY/actions/runs/$run_id/artifacts?per_page=100" "$out/artifacts.json"
  check_json "$out/artifacts.json"
  name="casper-campaign-result-$run_id-1"
  jq -e '(.artifacts|type=="array") and .total_count==(.artifacts|length) and .total_count<=100' "$out/artifacts.json" >/dev/null 2>&1 ||
    fail 'The artifact inventory is not complete.'
  matches="$(jq --arg name "$name" '[.artifacts[]|select(.name==$name)]|length' "$out/artifacts.json")"
  if [[ "$matches" == 0 ]]; then
    printf 'Not applicable: the run has no campaign result.\n' >&2
    exit 4
  fi
  [[ "$matches" == 1 ]] || fail 'The campaign result artifact is ambiguous.'
  jq --arg name "$name" '.artifacts[]|select(.name==$name)' "$out/artifacts.json" > "$out/artifact.json"
  jq -e '
    .expired==false and (.id|type=="number" and .>0 and floor==.) and
    (.size_in_bytes|type=="number" and .>0 and .<=2097152) and
    (.digest|type=="string" and test("^sha256:[a-f0-9]{64}$"))
  ' "$out/artifact.json" >/dev/null 2>&1 || fail 'The campaign result artifact is expired or not valid.'
  api_get "repos/$REPOSITORY/actions/artifacts/$(jq -r .id "$out/artifact.json")/zip" "$out/result.zip"
  archive="$(hash_file "$out/result.zip")"
  [[ "sha256:$archive" == "$(jq -r .digest "$out/artifact.json")" ]] || fail 'The downloaded result archive differs from its digest.'
  [[ "$(size_of "$out/result.zip")" == "$(jq -r .size_in_bytes "$out/artifact.json")" ]] || fail 'The downloaded archive size differs.'
  unzip -Z1 "$out/result.zip" > "$out/members.txt" 2> "$out/members.stderr" || fail 'The result archive cannot be listed.'
  [[ "$(< "$out/members.txt")" == campaign-result.json ]] || fail 'The result archive must contain exactly one named result.'
  unzip -Z "$out/result.zip" > "$out/zipinfo.txt" 2> "$out/zipinfo.stderr" || fail 'The result archive metadata cannot be read.'
  awk '$NF=="campaign-result.json" {if(substr($1,1,1)!="-") exit 1; n++} END {if(n!=1) exit 1}' "$out/zipinfo.txt" ||
    fail 'The archived result is not a regular file.'
  (unzip -p "$out/result.zip" campaign-result.json | head -c 1048577) > "$out/result.json" 2> "$out/result.stderr" ||
    fail 'The result cannot be read within its limit.'
  check_json "$out/result.json"
  if ! jq -e '.plan.stage=="stability"' "$out/result.json" >/dev/null 2>&1; then
    printf 'Not applicable: the campaign stage is not the 60-hour stability stage.\n' >&2
    exit 4
  fi
  revision="$(jq -r '.plan.node_revision // ""' "$out/result.json")"
  [[ "$revision" =~ ^[a-f0-9]{40}$ ]] || fail 'The soaked revision is not valid.'
  source=dispatch_input
  if [[ -z "$required" ]]; then
    api_get "repos/$REPOSITORY/pulls/$SOURCE_PULL" "$out/pull.json"
    check_json "$out/pull.json"
    if jq -e '.merged==true and (.merge_commit_sha|type=="string" and test("^[a-f0-9]{40}$"))' "$out/pull.json" >/dev/null 2>&1; then
      source=pull_request
      required="$(jq -r .merge_commit_sha "$out/pull.json")"
    else
      source=pull_request_not_merged
    fi
  fi
  api_get "repos/$REPOSITORY/git/ref/heads/master" "$out/master.json"
  check_json "$out/master.json"
  master="$(jq -r '.object.sha // ""' "$out/master.json")"
  [[ "$master" =~ ^[a-f0-9]{40}$ ]] || fail 'The revision of master is not valid.'
  comparison "$revision" "$master" "$out/revision-in-master.json"
  if [[ -n "$required" ]]; then
    comparison "$required" "$revision" "$out/required-in-revision.json"
  else
    printf 'null\n' > "$out/required-in-revision.json"
  fi
  jq -n --arg source "$source" --arg required "$required" --arg master "$master" \
    --arg archive "$archive" --arg result "$(hash_file "$out/result.json")" \
    --slurpfile m "$out/revision-in-master.json" --slurpfile q "$out/required-in-revision.json" '
    {schema_version:1,required_commit:(if $required=="" then null else $required end),
     required_commit_source:$source,master_revision:$master,revision_in_master:$m[0],
     required_in_revision:$q[0],result_archive_sha256:$archive,result_sha256:$result}
  ' > "$out/facts.json"
  evaluate "$out/run.json" "$out/artifact.json" "$out/result.json" "$out/facts.json" "$out/evaluation"
)
comments() {
  local output="$1"
  limited gh api --hostname github.com --method GET --paginate "repos/$REPOSITORY/issues/$ISSUE/comments?per_page=100" \
    > "$output.raw" 2> "$output.stderr" || fail 'The comment inventory cannot be read.'
  [[ "$(size_of "$output.raw")" -le 8388608 ]] || fail 'The comment inventory exceeds its byte limit.'
  jq -s --arg bot "$BOT" '[.[][]|select(.user.login==$bot)|.body|select(type=="string")]' "$output.raw" > "$output" 2>/dev/null ||
    fail 'The comment inventory is not valid.'
}
publish() {
  local body="$1" marker="$2" out="$3" name="$4"
  comments "$out/$name-comments.json"
  if jq -e --arg marker "$marker" 'any(.[];contains($marker))' "$out/$name-comments.json" >/dev/null; then
    printf 'The record already has this comment.\n' >&2
    return 0
  fi
  jq -n --rawfile body "$body" '{body:$body}' > "$out/$name-payload.json"
  api_send POST "repos/$REPOSITORY/issues/$ISSUE/comments" "$out/$name-payload.json" "$out/$name-response.json"
}
record() {
  [[ $# == 1 ]] || fail 'The record command requires the output directory of a collection.'
  local out="$1" verdict="$1/evaluation/verdict.json" run_id architecture revision other required partners partner code
  check_json "$verdict"
  run_id="$(jq -r .run_id "$verdict")"
  architecture="$(jq -r .architecture "$verdict")"
  revision="$(jq -r .node_revision "$verdict")"
  comment "$verdict" > "$out/comment.md"
  publish "$out/comment.md" "<!-- soak-obligation O1 run=$run_id " "$out" result
  jq -e '.verdict=="satisfied"' "$verdict" >/dev/null || return 0
  other=arm64
  [[ "$architecture" == amd64 ]] || other=amd64
  required=''
  if jq -e '.required_commit_source=="dispatch_input"' "$verdict" >/dev/null; then
    required="$(jq -r .required_commit "$verdict")"
  fi
  comments "$out/partner-comments.json"
  partners="$(jq -r --arg other "$other" --arg revision "$revision" --arg run "$run_id" '
    [.[]|capture("<!-- soak-obligation O1 run=(?<run>[1-9][0-9]{0,19}) architecture=(?<architecture>amd64|arm64) revision=(?<revision>[a-f0-9]{40}) verdict=(?<verdict>satisfied|not_satisfied) -->")?|
     select(.verdict=="satisfied" and .architecture==$other and .revision==$revision and .run!=$run)|.run]|unique|.[-3:][]
  ' "$out/partner-comments.json")"
  for partner in $partners; do
    attempt collect "$partner" "$out/partner-$partner" "$required"
    [[ "$code" == 0 ]] || continue
    attempt pair "$verdict" "$out/partner-$partner/evaluation/verdict.json" "$out/pair"
    [[ "$code" == 0 ]] || continue
    completion "$out/pair/obligation.json" > "$out/completion.md"
    publish "$out/completion.md" "<!-- soak-obligation O1 complete revision=$revision " "$out" completion
    jq -e '.evidence_complete==true' "$out/pair/obligation.json" >/dev/null || return 0
    api_get "repos/$REPOSITORY/issues/$ISSUE" "$out/issue.json"
    check_json "$out/issue.json"
    jq -e '.state=="open" and (.body|type=="string")' "$out/issue.json" >/dev/null || fail 'The obligation issue is not open.'
    jq -r .body "$out/issue.json" > "$out/issue-body.md"
    attempt mark "$out/issue-body.md" > "$out/issue-body-marked.md"
    [[ "$code" == 0 ]] || fail 'The issue text cannot be marked.'
    if ! cmp -s "$out/issue-body.md" "$out/issue-body-marked.md"; then
      jq -n --rawfile body "$out/issue-body-marked.md" '{body:($body|rtrimstr("\n"))}' > "$out/issue-payload.json"
      api_send PATCH "repos/$REPOSITORY/issues/$ISSUE" "$out/issue-payload.json" "$out/issue-response.json"
    fi
    return 0
  done
}
execute() {
  [[ $# == 2 || $# == 3 ]] || fail 'The execute command requires a run, a new output directory, and an optional required commit.'
  local code
  attempt collect "$@"
  case "$code" in
    0|3) record "$2" ;;
    4) printf 'The run does not apply to obligation O1. The check made no record.\n' >&2 ;;
    *) exit "$code" ;;
  esac
}
case "${1:-}" in
  evaluate) shift; evaluate "$@" ;;
  comment) shift; comment "$@" ;;
  pair) shift; pair "$@" ;;
  completion) shift; completion "$@" ;;
  mark) shift; mark "$@" ;;
  collect) shift; collect "$@" ;;
  record) shift; record "$@" ;;
  execute) shift; execute "$@" ;;
  *) fail 'Select the evaluate, comment, pair, completion, mark, collect, record, or execute command.' ;;
esac
