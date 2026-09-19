#!/usr/bin/env bash
set -euo pipefail
RUN=docs/casper/cbc-evidence/runs/casper-driver-refresh-acceptance-20260919-01
PREVIOUS=docs/casper/cbc-evidence/runs/casper-driver-rebind-acceptance-20260918-01
SPEC=docs/claims/casper-soak-harness.md
LOG=docs/work-logs/casper-driver-rebind-acceptance.md
PLAN=docs/tdd-plans/casper-soak-harness.md
CHANGED=scripts/casper-soak/src/host_control.rs
ACCEPTED_SHA256=fb6dafc47a5df2073c4ca618ddf81c4a292229b20e5b08a1f0cfeae306e185bb
ACCEPTED_COMMIT=f9273621c8887947b56d0093a71486338312138e
RESPONSE="${1:?the approval response is required}"
[[ "$RESPONSE" == approved ]]
[[ -f "$RUN/review-report.json" && ! -e "$RUN/report.json" ]]
[[ "$(<"$RUN/production-equivalence/result.txt")" == identical ]]
git diff --quiet HEAD -- "$CHANGED" "$SPEC" "$LOG" "$PLAN" docs/casper/cbc-evidence/*.md
shasum -a 256 -c "$RUN/review-source-files.sha256" --quiet
shasum -a 256 -c "$RUN/evidence/binding-check/sources.sha256" --quiet
jq -e '.status=="passed" and .exit_code==0' "$RUN/evidence/binding-check/report.json" >/dev/null
jq -e '.driver_invocations==91 and .registered_cases==48 and .matched_exits==true' "$RUN/evidence/binding-check/counts.json" >/dev/null
[[ "$(shasum -a 256 "$CHANGED" | awk '{print $1}')" == "$(jq -r '.change.current_sha256' "$RUN/review-report.json")" ]]
BASE="$(git rev-parse HEAD)"
STAMP="$(date -u +%FT%TZ)"
WORK="$(mktemp -d)"
trap 'rm -r "$WORK"' EXIT

awk 'BEGIN{b=0;seen=0}
/^```yaml$/ && seen==0 {b=1;seen=1;print;next}
b==1 && /^```$/ {b=0;print;next}
b==1 && $0=="status: pending" {print "status: discharged";next}
b==1 && $0=="binding: pending" {print "binding: passed";next}
{print}' "$SPEC" >"$WORK/spec.md"
grep -q '^status: discharged$' "$WORK/spec.md"
grep -q '^binding: passed$' "$WORK/spec.md"
awk '{
 if ($0 ~ /^Later on 2026-09-18/) {
  sub(/The claim returns to pending until a new acceptance binds the current source\./, "The claim returned to pending until a new acceptance bound the current source.")
  print
  print ""
  print "On 2026-09-19 the user accepted the refreshed binding for the current source. The [refresh acceptance record](../work-logs/casper-driver-rebind-acceptance.md#refresh-acceptance-2026-09-19) identifies the approved source and the retained evidence. CLAIM-CASPER-SOAK-001 is discharged again for the repaired pre-merge harness."
  next
 }
 print
}' "$WORK/spec.md" >"$SPEC"
grep -q 'refresh-acceptance-2026-09-19' "$SPEC"

cat >>"$LOG" <<EOF

## Refresh acceptance (2026-09-19)

The user replied \`approved\` to the request for acceptance of the refreshed binding on 2026-09-19. The approval covers only CLAIM-CASPER-SOAK-001 in the pre-merge phase.

The refreshed baseline is \`$BASE\`. The only accepted source that changed is \`$CHANGED\`. Its production region, with the test module removed, is byte-identical to the accepted file at commit \`${ACCEPTED_COMMIT:0:9}\`.

The [refresh package](../casper/cbc-evidence/runs/casper-driver-refresh-acceptance-20260919-01/report.json) records the approval, the source digests, the production-region comparison, and the isolated binding check. That check passed on this baseline with 91 driver invocations across 48 registered cases.

The model bounds, B44, kernel assumptions, and Docker daemon containment limits remain unchanged. Profile claims 002 through 008 and the soak status remain pending. No node campaign, external repin, waiver, commit, or push is authorized by this acceptance.
EOF

sed -i 's/The harness source changed again after that acceptance, so CLAIM-CASPER-SOAK-001 is pending until a new acceptance binds the current source\./The harness source changed again after that acceptance, and the user accepted the refreshed binding on 2026-09-19. CLAIM-CASPER-SOAK-001 is discharged for that scope./' "$PLAN"
grep -q 'accepted the refreshed binding on 2026-09-19' "$PLAN"

cut -c67- "$RUN/review-source-files.sha256" | while IFS= read -r path; do shasum -a 256 "$path"; done >"$RUN/source-files.sha256"
jq -Rn '[inputs | capture("^(?<sha>[0-9a-f]{64})  (?<path>.+)$")] | map({key:.path,value:.sha}) | from_entries' <"$RUN/source-files.sha256" >"$WORK/sources.json"
SPEC_HASH="$(shasum -a 256 "$SPEC" | awk '{print $1}')"
CURRENT_HASH="$(shasum -a 256 "$CHANGED" | awk '{print $1}')"
jq --arg base "$BASE" --arg stamp "$STAMP" --arg response "$RESPONSE" --arg spec "$SPEC" --arg spec_hash "$SPEC_HASH" \
 --arg log "$LOG" --arg plan "$PLAN" --slurpfile sources "$WORK/sources.json" '
 .status="accepted-bounded-binding-refresh" | .claim_discharge="discharged" | .base_commit=$base | .verified_at=$stamp |
 .source_digests=$sources[0] | .claim_digests={($spec):$spec_hash} |
 .tiers={refutation:"bounded-safety-pass",construction:"not-applicable",binding:"passed"} |
 .approval={response:$response,scope:"Refreshed bounded H01-H10 binding for the reordered host-control source only",recorded_at:$stamp} |
 .metadata_changes=[$spec,$log,$plan]' "$RUN/review-report.json" >"$RUN/report.json"
REPORT_HASH="$(shasum -a 256 "$RUN/report.json" | awk '{print $1}')"
PREVIOUS_LEDGERS_HASH="$(shasum -a 256 "$PREVIOUS/ledgers.tar.gz" | awk '{print $1}')"

while IFS= read -r name; do
 ledger="docs/casper/cbc-evidence/$name.md"
 awk '/^```json$/ {selected=1;next} selected && /^```$/ {exit} selected {print}' "$ledger" >"$WORK/ledger.json"
 path="$(jq -r .artifact.path "$WORK/ledger.json")"
 hash="$(shasum -a 256 "$path" | awk '{print $1}')"
 committed="$(git show "$BASE:$path" | shasum -a 256 | awk '{print $1}')"
 commit_is_base=false
 [[ "$committed" != "$hash" ]] || commit_is_base=true
 {
  printf '# CbC Evidence: %s\n\nThe user accepted the refreshed bounded H01–H10 binding review on 2026-09-19. This record discharges only CLAIM-CASPER-SOAK-001 in the pre-merge phase.\n\nProfile claims, node soaks, post-merge work, and inherited containment limits remain separate. The previous acceptance package preserves the earlier records.\n\n```json\n' "$path"
  jq --arg base "$BASE" --argjson cib "$commit_is_base" --arg hash "$hash" --arg spec "$SPEC" --arg spec_hash "$SPEC_HASH" \
   --arg report "$RUN/report.json" --arg report_hash "$REPORT_HASH" --arg stamp "$STAMP" \
   --arg previous "$PREVIOUS/ledgers.tar.gz" --arg previous_hash "$PREVIOUS_LEDGERS_HASH" --arg member "./$ledger" \
   --arg changed "$CHANGED" --arg accepted "$ACCEPTED_SHA256" --arg accepted_commit "$ACCEPTED_COMMIT" '
   .artifact.commit=$base | .artifact.commit_is_base=$cib | .artifact.sha256=$hash |
   .claim_digests={($spec):$spec_hash} | .status="discharged" |
   .evidence={kind:"accepted-bounded-refutation-and-binding-refresh",ref:$report,sha256:$report_hash} |
   .tiers.binding="passed" | .phase_status.pre_pr216_merge="discharged" | .verified_at=$stamp |
   .previous_ledger={ref:$previous,sha256:$previous_hash,member:$member} | del(.drift) |
   if .artifact.path==$changed then .refresh={accepted_commit:$accepted_commit,accepted_sha256:$accepted,production_region_identical:true,reason:"The execute function moved above the test module to satisfy clippy. The production region is byte-identical."} else . end' "$WORK/ledger.json"
  printf '```\n'
 } >"$ledger"
done <"$RUN/ledgers.txt"

audit_exit=0
cargo run --locked -p casper-soak --bin check-casper-claims -- --output "$RUN/claim-audit.json" >"$WORK/audit.txt" 2>&1 || audit_exit=$?
strict_exit=0
cargo run --locked -p casper-soak --bin check-casper-claims -- --strict --output "$RUN/claim-audit-strict.json" >"$WORK/strict.txt" 2>&1 || strict_exit=$?
[[ "$audit_exit" == 0 ]]
jq -e '.claims[] | select(.claim_id=="CLAIM-CASPER-SOAK-001") | .status=="discharged"' "$RUN/claim-audit.json" >/dev/null
jq -n --arg report "$REPORT_HASH" --argjson audit "$audit_exit" --argjson strict "$strict_exit" --arg base "$BASE" --arg stamp "$STAMP" \
 --argjson ledgers "$(wc -l <"$RUN/ledgers.txt" | tr -d ' ')" --argjson sources "$(wc -l <"$RUN/source-files.sha256" | tr -d ' ')" '
 {schema_version:1,status:"refresh-acceptance-verification-passed",report_sha256:$report,base_commit:$base,verified_at:$stamp,
  source_files:$sources,canonical_ledgers:$ledgers,current_source_matches:true,production_region_identical:true,
  claim_audit_exit:$audit,claim_001_status:"discharged",strict_audit_exit:$strict,strict_audit_note:"Claims 002 through 008 remain pending, so the full strict audit returns 4.",
  binding_check:{driver_invocations:91,registered_cases:48,matched_exits:true,status:"passed"}}' >"$RUN/validation.json"
(cd "$RUN" && find . -type f ! -name artifacts.sha256 | sed 's|^\./||' | sort | while IFS= read -r file; do shasum -a 256 "$file"; done) >"$RUN/artifacts.sha256"
printf 'Refresh acceptance recorded at %s on %s.\n' "$STAMP" "$BASE"
