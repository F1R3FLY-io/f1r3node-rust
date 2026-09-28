#!/usr/bin/env bash
set -euo pipefail
ROOT="$PWD"
OUT="${1:?An unused fixture directory is required.}"
[[ ! -e "$OUT" ]]
mkdir -p "$OUT/base/docs/claims" "$OUT/base/docs/plans" "$OUT/base/docs/casper/design" "$OUT/base/formal/tlaplus/casper_soak"
cp docs/claims/casper-soak-*.md "$OUT/base/docs/claims/"
cp docs/plans/casper-ratified-soak-2026-09-16.md "$OUT/base/docs/plans/"
cp docs/casper/design/soak-{interface-contract.md,candidate-matrix.jsonc} "$OUT/base/docs/casper/design/"
cp formal/tlaplus/casper_soak/verification-plan.jsonc "$OUT/base/formal/tlaplus/casper_soak/"
CHECK="$ROOT/docs/casper/cbc-evidence/runs/casper-preparation-completion-20260918-01/verify.sh"
bash "$CHECK" "$OUT/base" structure >"$OUT/base.txt" 2>&1
for defect in decision control node_scope dispatch merge_gate; do
 cp -R "$OUT/base" "$OUT/$defect"
 case "$defect" in
  decision)
   path="$OUT/$defect/docs/plans/casper-ratified-soak-2026-09-16.md"
   awk '!/^\| D-12 \|/' "$path" >"$OUT/changed"
   ;;
  control)
   path="$OUT/$defect/formal/tlaplus/casper_soak/verification-plan.jsonc"
   jq '.negative_controls[0].expected_exit=0' "$path" >"$OUT/changed"
   ;;
  node_scope)
   path="$OUT/$defect/docs/claims/casper-soak-harness.md"
   awk '{print} /^artifacts:$/ {print "  - node/src/lib.rs"}' "$path" >"$OUT/changed"
   ;;
  dispatch)
   path="$OUT/$defect/docs/casper/design/soak-candidate-matrix.jsonc"
   jq '.candidates[0].admission="allowed"' "$path" >"$OUT/changed"
   ;;
  merge_gate)
   path="$OUT/$defect/docs/casper/design/soak-candidate-matrix.jsonc"
   jq '.post_merge_gate.accepted_handoff=true' "$path" >"$OUT/changed"
   ;;
 esac
 mv "$OUT/changed" "$path"
 status=0
 bash "$CHECK" "$OUT/$defect" structure >"$OUT/$defect.txt" 2>&1 || status=$?
 printf '%s\n' "$status" >"$OUT/$defect.exit"
 [[ "$status" != 0 ]]
 printf '%s: refused with exit %s\n' "$defect" "$status"
done
printf 'One clean document fixture and five refusal fixtures passed.\n'
