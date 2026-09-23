#!/usr/bin/env bash
set -euo pipefail
REVIEW=docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01
RUN=docs/casper/cbc-evidence/runs/casper-driver-rebind-acceptance-20260918-01
WORK=target/casper-driver-acceptance
SPEC=docs/claims/casper-soak-harness.md
BASE="$(git rev-parse HEAD)"
STAMP="$(date -u +%FT%TZ)"
[[ ! -e "$RUN" ]]
mkdir -p "$RUN"
jq -r '.source_digests | keys[]' "$REVIEW/report.json" | while IFS= read -r path; do shasum -a 256 "$path"; done >"$RUN/source-files.sha256"
jq -Rn '[inputs | capture("^(?<sha>[0-9a-f]{64})  (?<path>.+)$")] | map({key:.path,value:.sha}) | from_entries' <"$RUN/source-files.sha256" >"$WORK/sources.json"
SPEC_HASH="$(shasum -a 256 "$SPEC" | awk '{print $1}')"
REVIEW_HASH="$(shasum -a 256 "$REVIEW/report.json" | awk '{print $1}')"
PREVIOUS_HASH="$(shasum -a 256 "$REVIEW/previous.tar.gz" | awk '{print $1}')"
jq -n --arg base "$BASE" --arg stamp "$STAMP" --arg spec "$SPEC" --arg spec_hash "$SPEC_HASH" --arg review "$REVIEW/report.json" --arg review_hash "$REVIEW_HASH" --arg previous "$REVIEW/previous.tar.gz" --arg previous_hash "$PREVIOUS_HASH" --slurpfile sources "$WORK/sources.json" --slurpfile reviewed "$REVIEW/report.json" '
 {schema_version:1,status:"accepted-bounded-binding",claim_ids:["CLAIM-CASPER-SOAK-001"],claim_discharge:"discharged",phase:"pre_pr216_merge",base_commit:$base,verified_at:$stamp,source_digests:$sources[0],claim_digests:{($spec):$spec_hash},tiers:{refutation:"bounded-safety-pass",construction:"not-applicable",binding:"passed"},scope:"bounded-harness-only",soak:"pending",waiver:null,approval:{response:"approved",scope:"Repaired bounded H01-H10 binding review only",recorded_at:$stamp,reviewed_report:{ref:$review,sha256:$review_hash}},reviewed_verification:$reviewed[0].tests,refutation:$reviewed[0].refutation,previous_ledgers:{ref:$previous,sha256:$previous_hash,count:37},support_changes:[{path:"scripts/ci/check-casper-soak-bindings.sh",reviewed_sha256:$reviewed[0].source_digests["scripts/ci/check-casper-soak-bindings.sh"],accepted_sha256:$sources[0]["scripts/ci/check-casper-soak-bindings.sh"],reason:"Copy the declared disk-test artifact into the isolated source-audit package. Driver behavior and assertions are unchanged."}],metadata_changes:["docs/claims/casper-soak-harness.md","docs/work-logs/casper-driver-source-rebind.md"],acceptance_state_checks:"validation.json",node_execution:false,policy_activation:false,limits:$reviewed[0].limits}' >"$RUN/report.json"
REPORT_HASH="$(shasum -a 256 "$RUN/report.json" | awk '{print $1}')"
for candidate in "$REVIEW"/candidate-ledgers/*.md; do
 name="${candidate##*/}"
 ledger="docs/casper/cbc-evidence/$name"
 awk '/^```json$/ {selected=1;next} selected && /^```$/ {exit} selected {print}' "$candidate" >"$WORK/candidate.json"
 path="$(jq -r .artifact.path "$WORK/candidate.json")"
 hash="$(shasum -a 256 "$path" | awk '{print $1}')"
 prior=null
 if [[ -e "$ledger" ]]; then
  awk '/^```json$/ {selected=1;next} selected && /^```$/ {exit} selected {print}' "$ledger" >"$WORK/old.json"
  jq -e '.claim_ids==["CLAIM-CASPER-SOAK-001"] and .status=="discharged"' "$WORK/old.json" >/dev/null
  prior="$(jq -cn --arg ref "$REVIEW/previous.tar.gz" --arg sha "$PREVIOUS_HASH" --arg member "./$ledger" '{ref:$ref,sha256:$sha,member:$member}')"
 fi
 commit_is_base=false
 committed="$(git show "$BASE:$path" | shasum -a 256 | awk '{print $1}')"
 [[ "$committed" == "$hash" ]] || commit_is_base=true
 {
  printf '# CbC Evidence: %s\n\nThe user accepted the repaired bounded H01–H10 binding review. This record discharges only CLAIM-CASPER-SOAK-001 in the pre-merge phase.\n\nProfile claims, node soaks, post-merge work, and inherited containment limits remain separate. The linked archive preserves the previous acceptance records.\n\n```json\n' "$path"
  jq --arg base "$BASE" --argjson commit_is_base "$commit_is_base" --arg hash "$hash" --arg spec "$SPEC" --arg spec_hash "$SPEC_HASH" --arg report "$RUN/report.json" --arg report_hash "$REPORT_HASH" --arg stamp "$STAMP" --argjson previous "$prior" --arg candidate "$candidate" '
   .artifact.commit=$base | .artifact.commit_is_base=$commit_is_base | .artifact.sha256=$hash |
   .claim_digests={($spec):$spec_hash} | .status="discharged" |
   .evidence={kind:"accepted-bounded-refutation-and-binding",ref:$report,sha256:$report_hash} |
   .previous_ledger=$previous | .review_candidate=$candidate |
   .tiers.binding="passed" | .phase_status.pre_pr216_merge="discharged" | .verified_at=$stamp' "$WORK/candidate.json"
  printf '```\n'
 } >"$ledger"
 link="docs/cbc-evidence/$name"
 if [[ ! -e "$link" && ! -L "$link" ]]; then
  ln -s "../casper/cbc-evidence/$name" "$link"
 elif [[ -L "$link" ]]; then
  [[ "$(readlink "$link")" == "../casper/cbc-evidence/$name" ]]
 else
  printf '%s\n' "$link" >>"$RUN/preserved-compatibility-records.txt"
 fi
done
cp "$WORK/record.sh" "$RUN/record-acceptance.sh"
git diff -- scripts/ci/check-casper-soak-bindings.sh >"$RUN/packaging.patch.txt"
