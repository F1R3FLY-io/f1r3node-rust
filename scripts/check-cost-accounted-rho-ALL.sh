#!/usr/bin/env bash
# Umbrella gate (LOCAL-ONLY): run every cost-accounted-rho verification gate and
# report a per-gate PASS / SKIP / FAIL matrix. The Rocq proofs gate is the
# AUTHORITATIVE leg (its failure fails the suite); the multi-prover cross-witness
# gates are fail-soft (a SKIP — tool absent/non-loadable — is not a failure).
#
# Env: SKIP_HEAVY=1 omits the slow legs (Rocq proofs, Lean) for a quick
# cross-witness sweep. Default runs every gate found.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SELF="$(basename "${BASH_SOURCE[0]}")"

heavy_re='check-cost-accounted-rho-(proofs|lean)\.sh$'

declare -a names verdicts
overall=0

for gate in "$ROOT"/scripts/check-cost-accounted-rho-*.sh; do
  base="$(basename "$gate")"
  [ "$base" = "$SELF" ] && continue
  if [ "${SKIP_HEAVY:-0}" = "1" ] && [[ "$base" =~ $heavy_re ]]; then
    names+=("$base"); verdicts+=("SKIP(heavy)"); continue
  fi
  out="$(bash "$gate" 2>&1)"; rc=$?
  if [ "$rc" -ne 0 ]; then
    verdict="FAIL"; overall=1
  elif printf '%s\n' "$out" | grep -qiE 'skipped'; then
    verdict="SKIP"
  else
    verdict="PASS"
  fi
  names+=("$base"); verdicts+=("$verdict")
done

echo ""
echo "════════ cost-accounted-rho verification matrix ════════"
for i in "${!names[@]}"; do
  printf "  %-8s %s\n" "${verdicts[$i]}" "${names[$i]}"
done
echo "════════════════════════════════════════════════════════"
if [ "$overall" -eq 0 ]; then
  echo "All present gates passed (skips are tool-absent cross-witnesses)."
else
  echo "error: at least one gate FAILED" >&2
fi
exit "$overall"
