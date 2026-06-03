#!/usr/bin/env bash
# Multi-prover cross-witness (LOCAL-ONLY, fail-soft): the accounting runtime's
# budget-conservation reconciliation core, Verus-verified (correspondence→proof).
# Corroborates CASettlement.charged_plus_refund_eq_escrow on the Rust functional
# core (Creusot is the contracts-on-the-pure-fns alternative; the lock-free CAS
# linearizability is the Iris leg).
#
# Fail-soft: absent verus, OR a verus whose required rust toolchain is not
# installed, is reported and skipped (exit 0). A runnable verus that reports a
# verification error IS a failure.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RS="$ROOT/formal/verus/cost_accounting/budget_conservation.rs"

echo "Checking cost-accounted rho budget conservation (Verus)..."

if [ ! -f "$RS" ]; then
  echo "error: Verus development not found at $RS" >&2
  exit 1
fi

if ! command -v verus >/dev/null 2>&1; then
  echo "  verus not found on PATH — skipped (fail-soft)."
  exit 0
fi

out="$(timeout 300 verus "$RS" 2>&1 || true)"

# A verus whose pinned rust toolchain is absent cannot run — treat as unavailable.
if printf '%s\n' "$out" | grep -qiE 'required rust toolchain.*not found|toolchain .* not installed|rustup .* install'; then
  echo "  verus present but its required rust toolchain is absent — skipped (fail-soft)."
  exit 0
fi

if printf '%s\n' "$out" | grep -qE '0 errors|verification results:: verified'; then
  echo "  Verus: verified (budget_split_conserves, refund_bounded, debit_monotone)."
  echo "Verus budget-conservation cross-witness passed."
  exit 0
fi
echo "  Verus did not verify:" >&2
printf '%s\n' "$out" | tail -15 >&2
exit 1
