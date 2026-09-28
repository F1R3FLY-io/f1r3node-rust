#!/usr/bin/env bash
set -euo pipefail
RUN=docs/casper/cbc-evidence/runs/casper-driver-refresh-acceptance-20260919-01
SPEC=docs/claims/casper-soak-harness.md
CHANGED=scripts/casper-soak/src/host_control.rs
OUT="${1:-target/casper-driver-refresh-validation}"
[[ ! -e "$OUT" ]]
mkdir -p "$OUT"
(cd "$RUN" && shasum -a 256 -c artifacts.sha256) >"$OUT/package.txt"
shasum -a 256 -c "$RUN/source-files.sha256" >"$OUT/current-source.txt"
grep -v -E "  docs/(claims|casper/cbc-evidence)/" "$RUN/evidence/binding-check/sources.sha256" | shasum -a 256 -c >"$OUT/binding-sources.txt"
"$RUN/production-equivalence/strip-tests.sh" <"$CHANGED" >"$OUT/current-production.rs"
cmp "$RUN/production-equivalence/accepted-production.rs" "$OUT/current-production.rs"
REPORT_HASH="$(shasum -a 256 "$RUN/report.json" | awk '{print $1}')"
SPEC_HASH="$(shasum -a 256 "$SPEC" | awk '{print $1}')"
jq -e --arg spec "$SPEC" --arg spec_hash "$SPEC_HASH" '
 .status=="accepted-bounded-binding-refresh" and .claim_discharge=="discharged" and .approval.response=="approved"
 and .claim_digests[$spec]==$spec_hash and .tiers.binding=="passed"' "$RUN/report.json" >/dev/null
ledgers=0
while IFS= read -r name; do
 ledger="docs/casper/cbc-evidence/$name.md"
 awk '/^```json$/ {selected=1;next} selected && /^```$/ {exit} selected {print}' "$ledger" >"$OUT/ledger.json"
 path="$(jq -r .artifact.path "$OUT/ledger.json")"
 hash="$(shasum -a 256 "$path" | awk '{print $1}')"
 jq -e --arg hash "$hash" --arg report "$REPORT_HASH" --arg spec "$SPEC" --arg spec_hash "$SPEC_HASH" --arg run "$RUN/report.json" '
  .status=="discharged" and (.claim_ids | index("CLAIM-CASPER-SOAK-001") != null) and .artifact.sha256==$hash
  and .evidence.ref==$run and .evidence.sha256==$report and .claim_digests[$spec]==$spec_hash
  and .tiers.binding=="passed" and .phase_status.pre_pr216_merge=="discharged" and (.drift == null)' "$OUT/ledger.json" >/dev/null
 jq -e --arg path "$path" '.source_digests[$path]' "$RUN/report.json" >/dev/null
 ledgers=$((ledgers + 1))
done <"$RUN/ledgers.txt"
[[ "$ledgers" == "$(wc -l <"$RUN/ledgers.txt" | tr -d ' ')" ]]
cargo run --locked -p casper-soak --bin check-casper-claims -- --output "$OUT/claim-audit.json" >"$OUT/claim-audit.txt" 2>&1
jq -e '.claims[] | select(.claim_id=="CLAIM-CASPER-SOAK-001") | .status=="discharged"' "$OUT/claim-audit.json" >/dev/null
printf 'Refresh acceptance package valid: %s ledgers, report %s.\n' "$ledgers" "${REPORT_HASH:0:12}"
