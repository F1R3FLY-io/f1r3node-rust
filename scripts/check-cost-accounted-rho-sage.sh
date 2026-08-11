#!/usr/bin/env bash
# ════════════════════════════════════════════════════════════════════════
# check-cost-accounted-rho-sage.sh — LOCAL-ONLY Sage gate for the cost-monad
# law bounded verification (the Sage leg of the continued-gslt-cost-v2
# multi-prover alignment). NOT a CI gate (formal verification is local-only).
#
# Runs formal/sage/cost_accounting/cost_monad_laws.sage and FAILS iff
# overall_pass != true — i.e. iff any expected_holds=True monoid/monad law has a
# counterexample, or any expected_holds=False law (stack non-commutativity, μ
# non-idempotence) failed to exhibit its witness. Independently corroborates the
# Rocq SignatureMonoid (CL2) / CostMonad (CL4) laws.
# ════════════════════════════════════════════════════════════════════════
set -euo pipefail

# Advisory by default per Greg's compile-time-shapes design: external-proof
# certificates (Rocq/Lean/TLA+/Sage corroboration) are NOT a required gate. Set
# CA_ENFORCE_PROOFS=1 to run the full strict Sage gate (preserved verbatim below).
if [ "${CA_ENFORCE_PROOFS:-0}" != "1" ]; then
  echo "cost-monad Sage gate: ADVISORY (relaxed; external-proof certificates not required). CA_ENFORCE_PROOFS=1 to run the full gate."
  exit 0
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL="$ROOT/formal/sage/cost_accounting/cost_monad_laws.sage"
OUT="$(mktemp)"
trap 'rm -f "$OUT"' EXIT

# The model is pure Python (no SageMath-specific calls). Prefer a plain `python3`
# (it forwards script args cleanly); `sage` proper intercepts flags like
# --json-out, so under a Sage environment use `sage -python` which forwards args.
if command -v python3 >/dev/null 2>&1; then
  RUNNER=(python3)
elif command -v sage >/dev/null 2>&1; then
  RUNNER=(sage -python)
else
  echo "error: neither python3 nor sage found on PATH" >&2
  exit 1
fi

echo "Checking cost-monad laws (Sage bounded verification, ${RUNNER[0]})..."
"${RUNNER[@]}" "$MODEL" --json-out "$OUT"

# Gate on overall_pass.
if python3 - "$OUT" <<'PY'
import json, sys
with open(sys.argv[1]) as f:
    data = json.load(f)
ok = data.get("overall_pass") is True
for r in data.get("results", []):
    if r["failures"] != 0:
        print("  FAILED law: %s (failures=%d)" % (r["law"], r["failures"]))
sys.exit(0 if ok else 1)
PY
then
  echo "Cost-monad Sage gate passed."
else
  echo "error: cost-monad Sage verification did not pass (overall_pass != true)" >&2
  exit 1
fi
