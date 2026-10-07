#!/usr/bin/env bash
# Multi-prover cross-witness (LOCAL-ONLY, fail-soft): the cost-accounted token
# discipline projected onto the authority view as a ProVerif applied-pi model —
# the SECOND runnable security witness alongside the Tamarin leg. Under an active
# Dolev-Yao attacker controlling the network, ProVerif discharges the
# secrecy+authentication view: fire-needs-mint and join-needs-mint
# (channel unforgeability — an unminted/forged token cannot fire) and
# minting-key secrecy. The LINEAR no-double-spend stays with Tamarin + Rocq
# (ll_no_double_spend_single_witness), which ProVerif's monotonic abstraction
# does not capture.
#
# Fail-soft: absent proverif is reported and skipped (exit 0). A runnable
# proverif that does not prove every query (any "is false" / "cannot be proved")
# IS a failure.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PV="$ROOT/formal/proverif/cost_accounting/token_authority.pv"

echo "Checking cost-accounted rho token authority (ProVerif)..."

if [ ! -f "$PV" ]; then
  echo "error: ProVerif model not found at $PV" >&2
  exit 1
fi

if ! command -v proverif >/dev/null 2>&1; then
  echo "  proverif not found on PATH — skipped (fail-soft)."
  exit 0
fi

out="$(timeout 300 proverif "$PV" 2>&1 || true)"

# ProVerif prints one "RESULT ... is true."/"is false."/"cannot be proved." per
# query. Any non-true verdict is a failure; we expect all 3 queries to be true.
if printf '%s\n' "$out" | grep -qE 'is false\.|cannot be proved\.'; then
  echo "  ProVerif did not prove a query:" >&2
  printf '%s\n' "$out" | grep -E '^RESULT|is false|cannot be proved' >&2
  exit 1
fi

true_count="$(printf '%s\n' "$out" | grep -cE 'RESULT .* is true\.' || true)"
if [ "$true_count" -lt 3 ]; then
  echo "  ProVerif proved fewer than the 3 expected queries (true=$true_count)" >&2
  printf '%s\n' "$out" | tail -20 >&2
  exit 1
fi

echo "  ProVerif proved all queries (fire_needs_mint, join_needs_mint, minting_key_secret)."
echo "ProVerif token-authority cross-witness passed."
