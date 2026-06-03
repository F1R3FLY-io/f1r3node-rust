#!/usr/bin/env bash
# Multi-prover cross-witness (LOCAL-ONLY, fail-soft): the cost-accounted gated
# reduction skeleton, first-order-encoded as a TPDB TRS, checked for termination
# (TTT2) and confluence (CSI) — independent corroboration of the Rocq funded-SN
# (CAStrongNormalization.funded_step_decreases) and local confluence
# (CAConfluence.ca_local_confluence) at the rewrite-skeleton projection.
#
# Fail-soft: a missing tool is reported and skipped (exit 0), never a failure —
# but a PRESENT tool that does not return YES IS a failure.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TRS="$ROOT/formal/rewriting/cost_accounting/ca_rules.trs"

echo "Checking cost-accounted rho rewrite skeleton (TTT2 termination + CSI confluence)..."

if [ ! -f "$TRS" ]; then
  echo "error: TRS encoding not found at $TRS" >&2
  exit 1
fi

if ! command -v ttt2 >/dev/null 2>&1 && ! command -v csi >/dev/null 2>&1; then
  echo "  ttt2 and csi not found on PATH — skipped (fail-soft)."
  exit 0
fi

status=0

if command -v ttt2 >/dev/null 2>&1; then
  out="$(timeout 180 ttt2 "$TRS" 2>&1 | head -1 || true)"
  if [ "$out" = "YES" ]; then
    echo "  TTT2 termination: YES"
  else
    echo "  TTT2 termination: '$out' (expected YES)" >&2
    status=1
  fi
else
  echo "  ttt2 not found — termination leg skipped."
fi

if command -v csi >/dev/null 2>&1; then
  out="$(timeout 180 csi "$TRS" 2>&1 | head -1 || true)"
  if [ "$out" = "YES" ]; then
    echo "  CSI confluence: YES"
  else
    echo "  CSI confluence: '$out' (expected YES)" >&2
    status=1
  fi
else
  echo "  csi not found — confluence leg skipped."
fi

if [ "$status" -eq 0 ]; then
  echo "Rewriting cross-witness passed."
else
  echo "error: rewriting cross-witness failed" >&2
fi
exit "$status"
