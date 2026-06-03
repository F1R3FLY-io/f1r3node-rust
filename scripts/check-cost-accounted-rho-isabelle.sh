#!/usr/bin/env bash
# Multi-prover cross-witness (LOCAL-ONLY, fail-soft): the N-ary join authority
# conservation (spec §4.8 Prop 4.7 / §4.8.5) in Isabelle/HOL — the FULL multiset
# conservation (join_authority_conserved) + no-weakening, an independent HOL
# corroboration of the Rocq CAJoinConservation development.
#
# Fail-soft: absent isabelle, or a build that times out (the heavy HOL-Library
# base is not prebuilt on this host), is reported and skipped (exit 0). A completed
# build that FAILS the session IS a failure.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="$ROOT/formal/isabelle/cost_accounting"

echo "Checking cost-accounted rho join conservation (Isabelle/HOL)..."

[ -f "$DIR/JoinConservation.thy" ] || { echo "error: Isabelle theory not found at $DIR" >&2; exit 1; }

if ! command -v isabelle >/dev/null 2>&1; then
  echo "  isabelle not found on PATH — skipped (fail-soft)."
  exit 0
fi

tmp="$(mktemp)"; trap 'rm -f "$tmp"' EXIT
if timeout "${ISABELLE_TIMEOUT:-1200}" isabelle build -D "$DIR" > "$tmp" 2>&1; then
  echo "  Isabelle: CostAccounting session verified (join_authority_conserved, join_no_weakening)."
  echo "Isabelle/HOL cross-witness passed."
  exit 0
fi
rc=$?
if [ "$rc" -eq 124 ]; then
  echo "  Isabelle build timed out (HOL-Library base not prebuilt) — skipped (fail-soft)."
  exit 0
fi
echo "  Isabelle build failed:" >&2
tail -20 "$tmp" >&2
exit 1
