#!/usr/bin/env bash
# Multi-prover cross-witness (LOCAL-ONLY, fail-soft): the N-ary join authority
# conservation (spec §4.8 Prop 4.7 / §4.8.4 / §4.8.5) checked SYMBOLICALLY by
# Apalache (SMT) over all symbolic authority valuations — Conservation + partition
# invariance + NoWeakening. Symbolic in the values (vs TLC's concrete enumeration);
# corroborates CAJoinConservation and the Why3 leg.
#
# Fail-soft: absent apalache-mc is reported and skipped (exit 0). A present
# apalache that does not report NoError IS a failure.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TLA="$ROOT/formal/tlaplus/cost_accounted_rho/NaryJoin.tla"

echo "Checking cost-accounted rho N-ary join conservation (Apalache symbolic)..."

if [ ! -f "$TLA" ]; then
  echo "error: TLA+ spec not found at $TLA" >&2
  exit 1
fi

if ! command -v apalache-mc >/dev/null 2>&1; then
  echo "  apalache-mc not found on PATH — skipped (fail-soft)."
  exit 0
fi

outdir="$(mktemp -d)"
out="$(cd "$outdir" && timeout 300 apalache-mc check \
        --init=Init --next=Next --inv=Inv --length=1 "$TLA" 2>&1 || true)"
rm -rf "$outdir"

if printf '%s\n' "$out" | grep -qE 'The outcome is: NoError|EXITCODE: OK'; then
  echo "  Apalache: invariant holds (Conservation + partition invariance + NoWeakening)."
  echo "Apalache N-ary-join cross-witness passed."
  exit 0
fi
echo "  Apalache did not verify the invariant:" >&2
printf '%s\n' "$out" | grep -iE 'error|violat|outcome|EXITCODE' | tail -20 >&2
exit 1
