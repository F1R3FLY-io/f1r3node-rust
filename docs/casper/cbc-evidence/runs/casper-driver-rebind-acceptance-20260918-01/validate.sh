#!/usr/bin/env bash
set -euo pipefail
ROOT="$PWD"
RUN=docs/casper/cbc-evidence/runs/casper-driver-rebind-acceptance-20260918-01
REVIEW=docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01
OUT="${1:-target/casper-driver-acceptance-validation}"
[[ ! -e "$OUT" ]]
mkdir -p "$OUT"/{source,evidence,ledgers}
(cd "$RUN" && shasum -a 256 -c artifacts.sha256) >"$OUT/package.txt"
(cd "$REVIEW" && shasum -a 256 -c artifacts.sha256) >"$OUT/review-package.txt"
for tree in source evidence ledgers; do gtar -xzf "$RUN/$tree.tar.gz" -C "$OUT/$tree"; done
(cd "$OUT/source" && shasum -a 256 -c "$ROOT/$RUN/source-files.sha256") >"$OUT/source-archive.txt"
shasum -a 256 -c "$RUN/source-files.sha256" >"$OUT/current-source.txt"
(cd "$OUT/evidence" && shasum -a 256 -c "$ROOT/$RUN/evidence-files.sha256") >"$OUT/evidence-archive.txt"
(cd "$OUT/ledgers" && shasum -a 256 -c "$ROOT/$RUN/ledgers.sha256") >"$OUT/ledger-archive.txt"
shasum -a 256 -c "$RUN/ledgers.sha256" >"$OUT/current-ledgers.txt"
shasum -a 256 -c "$RUN/preserved-compatibility.sha256" >"$OUT/preserved-compatibility.txt"
jq -e --slurpfile reviewed "$REVIEW/report.json" '
 $reviewed[0].source_digests as $old |
 ([.source_digests | to_entries[] | select(.value!=$old[.key]) | .key] | sort)
 == (["docs/claims/casper-soak-harness.md","docs/work-logs/casper-driver-source-rebind.md","scripts/ci/check-casper-soak-bindings.sh","scripts/casper-soak/src/host_control.rs"] | sort)' "$RUN/report.json" >/dev/null
REPORT_HASH="$(shasum -a 256 "$RUN/report.json" | awk '{print $1}')"
SPEC_HASH="$(shasum -a 256 docs/claims/casper-soak-harness.md | awk '{print $1}')"
for candidate in "$REVIEW"/candidate-ledgers/*.md; do
 ledger="docs/casper/cbc-evidence/${candidate##*/}"
 awk '/^```json$/ {selected=1;next} selected && /^```$/ {exit} selected {print}' "$ledger" >"$OUT/ledger.json"
 path="$(jq -r .artifact.path "$OUT/ledger.json")"
 hash="$(shasum -a 256 "$path" | awk '{print $1}')"
 jq -e --arg hash "$hash" --arg report "$REPORT_HASH" --arg spec "$SPEC_HASH" '
 .status=="discharged" and .claim_ids==["CLAIM-CASPER-SOAK-001"] and .artifact.sha256==$hash
 and .evidence.sha256==$report and .claim_digests["docs/claims/casper-soak-harness.md"]==$spec
 and .tiers.binding=="passed" and .phase_status.pre_pr216_merge=="discharged"
 and .phase_status.post_pr216_merge=="blocked" and .soak=="pending" and .waiver==null' "$OUT/ledger.json" >/dev/null
 jq -e --arg path "$path" --arg hash "$hash" '.source_digests[$path]==$hash' "$RUN/report.json" >/dev/null
done
target/debug/check-casper-claims --root . --claim CLAIM-CASPER-SOAK-001 --strict --output "$OUT/claim-001.json"
for claim in 002 003 004 005 006 007 008; do
 status=0
 target/debug/check-casper-claims --root . --claim "CLAIM-CASPER-SOAK-$claim" --strict --output "$OUT/claim-$claim.json" || status=$?
 [[ "$status" == 4 ]]
done
target/debug/check-casper-bindings --evidence "$OUT/evidence/accepted-final/evidence" --output "$OUT/bindings.json"
jq -e '.registered_cases==48 and .driver_invocations==91 and .matched_exits==true' "$OUT/bindings.json" >/dev/null
jq -e '.exit_code==0 and .status=="passed"' "$OUT/evidence/accepted-final/report.json" >/dev/null
jq -e '.exit_code==1 and .status=="failed"' "$OUT/evidence/accepted-state/report.json" >/dev/null
[[ "$(grep -c 'test result: ok. 7 passed' "$OUT/evidence/host-repeat/65534-tests.txt")" == 50 ]]
grep -Fq 'test result: ok. 8 passed' "$OUT/evidence/host-repeat/0-tests.txt"
grep -Fq 'samples=1000 missing_immediately=28 visible_after_delay=28' "$OUT/evidence/startup-probe.txt"
cmp "$OUT/evidence/reviewed-production.rs" "$OUT/evidence/current-production.rs"
for check in fmt bash ste; do [[ "$(<"$OUT/evidence/$check.exit")" == 0 ]]; done
for container in "$OUT/evidence/host-repeat/0-finished.json" "$OUT/evidence/host-repeat/65534-finished.json" "$OUT/evidence/accepted-final/finished.json"; do
 jq -e '.[0] | (.Mounts|length)==0 and .HostConfig.NetworkMode=="none" and .HostConfig.PidMode=="" and .HostConfig.Privileged==false and .HostConfig.CapDrop==["ALL"] and .State.Running==false and .State.OOMKilled==false and .State.ExitCode==0' "$container" >/dev/null
done
if grep -RIlF "$HOME" "$OUT/source" "$OUT/evidence" "$OUT/ledgers" >"$OUT/private-paths.txt"; then exit 2; fi
jq -n --arg report "$REPORT_HASH" --argjson sources "$(wc -l <"$RUN/source-files.sha256" | tr -d ' ')" --argjson evidence "$(wc -l <"$RUN/evidence-files.sha256" | tr -d ' ')" --argjson ledgers "$(wc -l <"$RUN/ledgers.sha256" | tr -d ' ')" \
 '{schema_version:1,status:"accepted-state-verification-passed",report_sha256:$report,source_files:$sources,evidence_files:$evidence,canonical_ledgers:$ledgers,current_source_matches:true,reviewed_package_unchanged:true,preserved_legacy_compatibility_records:6,strict_claim_001_exit:0,strict_other_claims_exit:4,accepted_state_tests:29,registered_cases:48,driver_invocations:91,host_control_repeat_runs:50,host_control_tests_per_repeat:7,root_host_control_tests:8,failed_acceptance_run_retained:true,fixture_startup_probe_retained:true,production_code_outside_test_module_unchanged:true,construction:"not-applicable",soak:"pending",node_execution:false}' >"$OUT/validation.json"
