#!/usr/bin/env bash
# Multi-prover cross-witness (LOCAL-ONLY, fail-soft): the cost-accounted token
# discipline projected onto the authority view as Tamarin multiset rewriting —
# token gates as linear facts. Tamarin discharges no-double-spend, channel
# unforgeability (fire-needs-mint), and atomic combined-token consumption
# (no partial funding within a join), corroborating the Rocq no-leak / stack-
# consumption invariants at the security projection.
#
# Fail-soft: absent OR non-loadable tamarin-prover is reported and skipped
# (exit 0). A loadable tamarin that does not verify every lemma IS a failure.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SPTHY="$ROOT/formal/tamarin/cost_accounting/token_authority.spthy"

echo "Checking cost-accounted rho token authority (Tamarin)..."

if [ ! -f "$SPTHY" ]; then
  echo "error: Tamarin theory not found at $SPTHY" >&2
  exit 1
fi

if ! command -v tamarin-prover >/dev/null 2>&1; then
  echo "  tamarin-prover not found on PATH — skipped (fail-soft)."
  exit 0
fi

# A present-but-non-loadable install (e.g. a missing GHC shared library) is
# treated as unavailable, not as a model failure.
if ! tamarin-prover --version >/dev/null 2>&1; then
  echo "  tamarin-prover present but failed to load (toolchain issue) — skipped (fail-soft)."
  exit 0
fi

out="$(timeout 300 tamarin-prover --prove "$SPTHY" 2>&1 || true)"
# Tamarin prints one "verified"/"falsified" verdict per lemma in its summary.
falsified="$(printf '%s\n' "$out" | grep -c 'falsified' || true)"
verified="$(printf '%s\n' "$out" | grep -c 'verified' || true)"
if [ "$falsified" != "0" ]; then
  echo "  Tamarin falsified a lemma:" >&2
  printf '%s\n' "$out" | grep -E 'falsified|verified' >&2
  exit 1
fi
if [ "$verified" -lt 3 ]; then
  echo "  Tamarin did not verify all 3 lemmas (verified=$verified)" >&2
  printf '%s\n' "$out" | tail -20 >&2
  exit 1
fi
echo "  Tamarin verified all lemmas (no_double_spend, fire_needs_mint, join_atomic)."
echo "Tamarin token-authority cross-witness passed."
