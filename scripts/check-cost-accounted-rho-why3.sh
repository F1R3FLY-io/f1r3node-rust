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
log_file="$ROOT/target/verification/cost-accounted-rho-why3.log"
mkdir -p "$(dirname "$log_file")"
: >"$log_file"
strict="${CA_ENFORCE_PROOFS:-0}"

echo "Checking cost-accounted rho join conservation (Why3)..."

if [ ! -f "$MLW" ]; then
  echo "error: Why3 development not found at $MLW" | tee "$log_file" >&2
  exit 1
fi

if ! command -v why3 >/dev/null 2>&1; then
  if [ "$strict" = "1" ]; then
    echo "error: Why3 is required but not on PATH" | tee "$log_file" >&2
    exit 1
  fi
  echo "  why3 not found on PATH — skipped (fail-soft)." | tee "$log_file"
  exit 0
fi

provers_list="$(why3 config list-provers 2>/dev/null || true)"
prover=""
for p in alt-ergo cvc5 cvc4 z3 vampire eprover; do
  if printf '%s\n' "$provers_list" | grep -qiE "$p"; then prover="$p"; break; fi
done
if [ -z "$prover" ]; then
  if [ "$strict" = "1" ]; then
    echo "error: Why3 has no usable prover configured" | tee "$log_file" >&2
    exit 1
  fi
  echo "  why3 present but no usable prover configured (run 'why3 config detect') — skipped (fail-soft)." | tee "$log_file"
  exit 0
fi

if timeout 300 why3 prove -P "$prover" "$MLW" >"$log_file" 2>&1; then
  rc=0
else
  rc=$?
fi
if [ "$rc" -ne 0 ]; then
  echo "  Why3 ($prover) failed with exit $rc; output: $log_file" >&2
  cat "$log_file" >&2
  exit 1
fi
n_goals="$(grep -c '^Goal ' "$log_file" || true)"
n_valid="$(grep -c '^Prover result is: Valid ' "$log_file" || true)"
if [ "$n_goals" -lt 3 ] || [ "$n_goals" -ne "$n_valid" ] \
   || ! grep -Fxq "Goal sig_size_pos'vc." "$log_file" \
   || ! grep -Fxq "Goal key_ge'vc." "$log_file" \
   || ! grep -Fxq 'Goal join_no_weakening.' "$log_file"; then
  echo "  Why3 ($prover) validated $n_valid of $n_goals goals; output: $log_file" >&2
  cat "$log_file" >&2
  exit 1
fi
echo "  Why3 ($prover): $n_valid goals Valid (sig_size_pos, key_ge, join_no_weakening); output: $log_file."
echo "Why3 join-conservation cross-witness passed."
