#!/usr/bin/env bash
set -euo pipefail
ROOT="$PWD"
RUN=docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01
OUT="${1:-target/casper-driver-rebind-validation}"
[[ ! -e "$OUT" ]]
mkdir -p "$OUT"/{source,evidence,previous,support}
(cd "$RUN" && shasum -a 256 -c artifacts.sha256) >"$OUT/package-check.txt"
for tree in source evidence previous support; do
 gtar -xzf "$RUN/$tree.tar.gz" -C "$OUT/$tree"
done
(cd "$OUT/source" && shasum -a 256 -c "$ROOT/$RUN/source-files.sha256") >"$OUT/source-archive-check.txt"
shasum -a 256 -c "$RUN/source-files.sha256" >"$OUT/current-source-check.txt"
(cd "$OUT/evidence" && shasum -a 256 -c "$ROOT/$RUN/evidence-files.sha256") >"$OUT/evidence-archive-check.txt"
(cd "$OUT/previous" && shasum -a 256 -c "$ROOT/$RUN/previous-ledgers.sha256") >"$OUT/previous-archive-check.txt"
shasum -a 256 -c "$RUN/previous-ledgers.sha256" >"$OUT/canonical-ledger-check.txt"
REPORT_HASH="$(shasum -a 256 "$RUN/report.json" | awk '{print $1}')"
SPEC_HASH="$(shasum -a 256 docs/claims/casper-soak-harness.md | awk '{print $1}')"
ledgers=0
for ledger in "$RUN"/candidate-ledgers/*.md; do
 awk '/^```json$/ {selected=1;next} selected && /^```$/ {exit} selected {print}' "$ledger" >"$OUT/ledger.json"
 artifact="$(jq -r .artifact.path "$OUT/ledger.json")"
 hash="$(shasum -a 256 "$artifact" | awk '{print $1}')"
 jq -e --arg hash "$hash" --arg spec "$SPEC_HASH" --arg report "$REPORT_HASH" '
 .status=="pending" and .artifact.sha256==$hash and .evidence.sha256==$report and .waiver==null
 and .claim_ids==["CLAIM-CASPER-SOAK-001"] and .tiers.binding=="pending-review"
 and .claim_digests["docs/claims/casper-soak-harness.md"]==$spec' "$OUT/ledger.json" >/dev/null
 jq -e --arg artifact "$artifact" --arg hash "$hash" '.source_digests[$artifact]==$hash' "$RUN/report.json" >/dev/null
 ledgers=$((ledgers+1))
done
[[ "$ledgers" == 39 ]]
target/debug/check-casper-bindings --evidence "$OUT/evidence/bindings-verified/evidence" --output "$OUT/bindings.json" >"$OUT/bindings.txt" 2>&1
jq -e '.driver_invocations==91 and .registered_cases==48 and .matched_exits==true' "$OUT/bindings.json" >/dev/null
jq -e '.status=="passed" and (.results|length)==11 and all(.results[]; .outcome=="passed") and .results[0].distinct_states==43424 and .results[0].states_generated==66208' "$OUT/evidence/models-verified/report.json" >/dev/null
jq -r '.results[] | [.log,.log_sha256] | @tsv' "$OUT/evidence/models-verified/report.json" | while IFS=$'\t' read -r log expected; do
 actual="$(shasum -a 256 "$OUT/evidence/models-verified/$log" | awk '{print $1}')"
 if [[ "$actual" != "$expected" ]]; then
  awk -F '\t' -v name="evidence/models-verified/$log" -v before="$expected" -v after="$actual" '$1==name && $2==before && $3==after {found=1} END {exit !found}' "$RUN/redactions.tsv"
 fi
done
for claim in 001 004; do
 status=0
 target/debug/check-casper-claims --root . --strict --claim "CLAIM-CASPER-SOAK-$claim" --output "$OUT/claim-$claim.json" >"$OUT/claim-$claim.txt" 2>&1 || status=$?
 [[ "$status" == 4 ]]
 jq -e '.claim_discharge=="pending"' "$OUT/claim-$claim.json" >/dev/null
done
[[ "$(<"$OUT/evidence/marker-verified/before.exit")" == 1 ]]
[[ "$(<"$OUT/evidence/marker-verified/after.exit")" == 0 ]]
for item in disk-verified models-verified fmt ste bash; do [[ "$(<"$OUT/evidence/$item.exit")" == 0 ]]; done
for uid in 0 65534; do
 [[ "$(<"$OUT/evidence/host-controls-verified/$uid-exit.txt")" == 0 ]]
 jq -e '.[0] | (.Mounts|length)==0 and .HostConfig.NetworkMode=="none" and .HostConfig.CapDrop==["ALL"] and .HostConfig.PidMode=="" and .State.Running==false and .State.OOMKilled==false and .State.ExitCode==0' "$OUT/evidence/host-controls-verified/$uid-finished.json" >/dev/null
done
for scenario in "$OUT"/evidence/disk-verified/*/test-exit.txt; do [[ "$(<"$scenario")" == 0 ]]; done
[[ "$(find "$OUT/evidence/disk-verified" -name test-exit.txt | wc -l | tr -d ' ')" == 42 ]]
[[ "$(<"$OUT/evidence/profile-regressions/exit.txt")" == 0 ]]
[[ "$(<"$OUT/evidence/profile-regressions/current-binary-check.exit")" == 0 ]]
if grep -RIlF "$HOME" "$OUT/source" "$OUT/evidence" "$OUT/previous" "$OUT/support" >"$OUT/private-paths.txt"; then
 printf 'A private path remains.\n' >&2; exit 2
fi
sources="$(wc -l <"$RUN/source-files.sha256" | tr -d ' ')"
evidence="$(wc -l <"$RUN/evidence-files.sha256" | tr -d ' ')"
previous="$(wc -l <"$RUN/previous-ledgers.sha256" | tr -d ' ')"
jq -n --arg report "$REPORT_HASH" --argjson sources "$sources" --argjson evidence "$evidence" --argjson previous "$previous" --argjson candidates "$ledgers" \
 '{schema_version:1,status:"evidence-integrity-passed-binding-review-pending",report_sha256:$report,source_files:$sources,evidence_files:$evidence,preserved_canonical_ledgers:$previous,pending_candidate_ledgers:$candidates,current_source_matches:true,archive_checks_passed:true,canonical_acceptance_unchanged:true,binding_inventory_passed:true,model_results_verified:true,strict_claim_001_exit:4,strict_claim_004_exit:4,claim_discharge:"pending",node_execution:false,tool_warnings:["The file observer reported blind-write warnings for generated files. The generation script required an absent destination and did not overwrite canonical ledgers."]}' >"$OUT/validation.json"
