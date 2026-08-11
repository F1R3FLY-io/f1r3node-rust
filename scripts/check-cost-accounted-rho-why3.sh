#!/usr/bin/env bash
# Multi-prover cross-witness (LOCAL-ONLY, fail-soft): the N-ary join conservation
# algebra (spec §4.8 Prop 4.7 / §4.8.5) discharged first-order by an SMT/ATP
# backend via Why3 — independent corroboration of CAJoinConservation.join_no_weakening
# (a compound token cannot be discharged as the receiver authority alone).
#
# Fail-soft: absent why3, or why3 with no usable prover, is reported and skipped
# (exit 0). A present why3+prover that does not return Valid for every goal IS a
# failure.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MLW="$ROOT/formal/why3/cost_accounting/join_conservation.mlw"

echo "Checking cost-accounted rho join conservation (Why3)..."

if [ ! -f "$MLW" ]; then
  echo "error: Why3 development not found at $MLW" >&2
  exit 1
fi

if ! command -v why3 >/dev/null 2>&1; then
  echo "  why3 not found on PATH — skipped (fail-soft)."
  exit 0
fi

provers_list="$(why3 config list-provers 2>/dev/null || true)"
prover=""
for p in alt-ergo cvc5 cvc4 z3 vampire eprover; do
  if printf '%s\n' "$provers_list" | grep -qiE "$p"; then prover="$p"; break; fi
done
if [ -z "$prover" ]; then
  echo "  why3 present but no usable prover configured (run 'why3 config detect') — skipped (fail-soft)."
  exit 0
fi

out="$(timeout 300 why3 prove -P "$prover" "$MLW" 2>&1 || true)"
if printf '%s\n' "$out" | grep -qiE 'Unknown|Timeout|Failure|Invalid|StepLimitExceeded|error'; then
  echo "  Why3 ($prover) did not discharge a goal:" >&2
  printf '%s\n' "$out" | grep -iE 'Goal|result|Unknown|Timeout|Failure|Invalid' >&2
  exit 1
fi
n_valid="$(printf '%s\n' "$out" | grep -c 'Valid' || true)"
if [ "$n_valid" -lt 3 ]; then
  echo "  Why3 ($prover) returned only $n_valid Valid goals (expected 3)." >&2
  printf '%s\n' "$out" | tail -20 >&2
  exit 1
fi
echo "  Why3 ($prover): $n_valid goals Valid (sig_size_pos, key_ge, join_no_weakening)."
echo "Why3 join-conservation cross-witness passed."
