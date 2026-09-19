#!/usr/bin/env bash
set -euo pipefail
P=docs/casper/cbc-evidence/runs/casper-profile-acceptance-20260919-01
OUT=target/casper-profile-acceptance/refresh
[[ ! -e "$OUT" ]]
mkdir "$OUT"
old="$(shasum -a 256 "$P/report.json" | awk '{print $1}')"
[[ "$old" == "$(shasum -a 256 target/casper-profile-acceptance/attempts/record-initial-report.json | awk '{print $1}')" ]]
spec=docs/claims/casper-soak-authority-finality.md
sha="$(shasum -a 256 "$spec" | awk '{print $1}')"
jq --arg spec "$spec" --arg sha "$sha" '.claim_digests[$spec]=$sha | .lifecycle_claim_001="outside-this-acceptance"' "$P/report.json" >"$OUT/report.json"
cp "$OUT/report.json" "$P/report.json"
new="$(shasum -a 256 "$P/report.json" | awk '{print $1}')"
: >"$OUT/ledgers.patch.txt"
for file in "$P/accepted-ledgers/"*.md; do
 path="docs/casper/cbc-evidence/$(basename "$file")"
 cmp "$file" "$path"
 data="$(awk '/^```json$/ {on=1;next} on && /^```$/ {exit} on {print}' "$file")"
 jq -e --arg old "$old" '.evidence.sha256==$old and .status=="discharged"' <<<"$data" >/dev/null
 target="$OUT/$(basename "$file")"
 {
  printf '# CbC Evidence: %s\n\nThe user accepted this bounded pre-merge profile binding. Node correctness and live execution remain outside this discharge.\n\n```json\n' "$(jq -r '.artifact.path' <<<"$data")"
  jq --arg new "$new" --arg spec "$spec" --arg sha "$sha" '.evidence.sha256=$new | if .claim==$spec then .claim_digests[$spec]=$sha else . end' <<<"$data"
  printf '```\n'
 } >"$target"
 status=0
 diff -u --label "a/$path" --label "b/$path" "$path" "$target" >>"$OUT/ledgers.patch.txt" || status=$?
 [[ "$status" == 1 ]]
done
git apply --check "$OUT/ledgers.patch.txt"
git apply "$OUT/ledgers.patch.txt"
for file in "$P/accepted-ledgers/"*.md; do cp "$OUT/$(basename "$file")" "$file"; done
for id in 001 002 003 004; do
 target/debug/check-casper-claims --root . --output "$OUT/claim-$id.json" --strict --claim "CLAIM-CASPER-SOAK-$id" >"$OUT/claim-$id.txt" 2>&1
 printf '0\n' >"$OUT/claim-$id.exit"
done
status=0
target/debug/check-casper-claims --root . --output "$OUT/bundle.json" --strict >"$OUT/bundle.txt" 2>&1 || status=$?
printf '%s\n' "$status" >"$OUT/bundle.exit"
[[ "$status" == 4 ]]
jq -e '[.claims[]|select(.status=="pending")|.claim_id]==["CLAIM-CASPER-SOAK-005","CLAIM-CASPER-SOAK-006","CLAIM-CASPER-SOAK-007","CLAIM-CASPER-SOAK-008"]' "$OUT/bundle.json" >/dev/null
printf 'The profile acceptance is recorded. CLAIM-001 has separate external acceptance.\n'
