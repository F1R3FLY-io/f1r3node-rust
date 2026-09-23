#!/usr/bin/env bash
set -euo pipefail
RUN=docs/casper/cbc-evidence/runs/casper-driver-rebind-acceptance-20260918-01
REVIEW=docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01
WORK=target/casper-driver-acceptance
PATH_CHANGED=scripts/casper-soak/src/host_control.rs
HASH="$(shasum -a 256 "$PATH_CHANGED" | awk '{print $1}')"
STAMP="$(date -u +%FT%TZ)"
PRODUCTION_HASH="$(shasum -a 256 "$WORK/current-production.rs" | awk '{print $1}')"
cmp "$WORK/reviewed-production.rs" "$WORK/current-production.rs"
jq --arg path "$PATH_CHANGED" --arg hash "$HASH" --arg stamp "$STAMP" --arg production "$PRODUCTION_HASH" --slurpfile reviewed "$REVIEW/report.json" '
 .source_digests[$path]=$hash | .verified_at=$stamp |
 .support_changes += [{path:$path,reviewed_sha256:$reviewed[0].source_digests[$path],accepted_sha256:$hash,reason:"Require a child readiness receipt in the test fixture before observing its owner marker or OOM setting. Production code outside cfg(test) is byte-identical.",production_region_sha256:$production}] |
 .acceptance_state_failure={result:"retained",suite:"host-control",test:"oom_preference_uses_the_pinned_process_directory",observed:"0 instead of 1000",diagnostic:{samples:1000,owner_marker_missing_immediately:28,owner_marker_visible_after_delay:28},fixture_recheck:{unprivileged_repetitions:50,tests_per_repetition:7,root_tests:8}}' "$RUN/report.json" >"$WORK/refreshed-report.json"
cp "$WORK/refreshed-report.json" "$RUN/report.json"
jq -r '.source_digests | to_entries[] | .value+"  "+.key' "$RUN/report.json" >"$RUN/source-files.sha256"
REPORT_HASH="$(shasum -a 256 "$RUN/report.json" | awk '{print $1}')"
for candidate in "$REVIEW"/candidate-ledgers/*.md; do
 ledger="docs/casper/cbc-evidence/${candidate##*/}"
 awk '/^```json$/ {selected=1;next} selected && /^```$/ {exit} selected {print}' "$ledger" >"$WORK/ledger.json"
 artifact="$(jq -r .artifact.path "$WORK/ledger.json")"
 {
  printf '# CbC Evidence: %s\n\nThe user accepted the repaired bounded H01–H10 binding review. This record discharges only CLAIM-CASPER-SOAK-001 in the pre-merge phase.\n\nProfile claims, node soaks, post-merge work, and inherited containment limits remain separate. The linked archive preserves the previous acceptance records.\n\n```json\n' "$artifact"
  jq --arg report "$REPORT_HASH" --arg path "$PATH_CHANGED" --arg hash "$HASH" --arg stamp "$STAMP" '
   .evidence.sha256=$report | .verified_at=$stamp |
   if .artifact.path==$path then .artifact.sha256=$hash | .artifact.commit_is_base=true else . end' "$WORK/ledger.json"
  printf '```\n'
 } >"$ledger"
done
cp "$WORK/refresh.sh" "$RUN/refresh-acceptance.sh"
cp "$WORK/fixture-readiness.patch.txt" "$RUN/fixture-readiness.patch.txt"
