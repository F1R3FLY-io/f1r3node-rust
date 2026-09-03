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
STACK_FRONTIER_MODEL="$ROOT/formal/sage/cost_accounting/located_stack_frontier_model.sage"
EXCHANGE_MODEL="$ROOT/formal/sage/cost_accounting/exchange_conservation.sage"
VAULT_MODEL="$ROOT/formal/sage/cost_accounting/vault_backed_lifecycle.sage"
WORK_ROOT="$ROOT/target/verification/cost-accounted-rho/sage"
mkdir -p "$WORK_ROOT"
OUT="$(mktemp "$WORK_ROOT/result.XXXXXX.json")"
STACK_FRONTIER_OUT="$(mktemp "$WORK_ROOT/stack-frontier.XXXXXX.json")"
EXCHANGE_OUT="$(mktemp "$WORK_ROOT/exchange.XXXXXX.json")"
VAULT_OUT="$(mktemp "$WORK_ROOT/vault.XXXXXX.json")"
SAGE_STATE="$WORK_ROOT/dot-sage"
SAGE_TMP="$WORK_ROOT/tmp"
mkdir -p "$SAGE_STATE" "$SAGE_TMP"
trap 'rm -f "$OUT" "$STACK_FRONTIER_OUT" "$EXCHANGE_OUT" "$VAULT_OUT"; rm -rf "$SAGE_STATE" "$SAGE_TMP"' EXIT

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
echo "Checking located-stack conservation and frontier expansion (Sage bounded verification, ${RUNNER[0]})..."
"${RUNNER[@]}" "$STACK_FRONTIER_MODEL" --json-out "$STACK_FRONTIER_OUT"
echo "Checking SystemVault-backed reserve/settle lifecycle..."
"${RUNNER[@]}" "$VAULT_MODEL" --json-out "$VAULT_OUT"
if ! command -v sage >/dev/null 2>&1; then
  echo "error: sage is required for the Exchange model's scenario-schema loader" >&2
  exit 1
fi
echo "Checking first-class cost-stack Exchange conservation (Sage bounded verification)..."
DOT_SAGE="$SAGE_STATE" TMPDIR="$SAGE_TMP" sage "$EXCHANGE_MODEL" -- --json-out "$EXCHANGE_OUT"

# Gate on overall_pass.
if python3 - "$OUT" "$STACK_FRONTIER_OUT" "$EXCHANGE_OUT" "$VAULT_OUT" <<'PY'
import json, sys
with open(sys.argv[1]) as f:
    data = json.load(f)
ok = data.get("overall_pass") is True
for r in data.get("results", []):
    if r["failures"] != 0:
        print("  FAILED law: %s (failures=%d)" % (r["law"], r["failures"]))
with open(sys.argv[2]) as f:
    stack_frontier = json.load(f)
ok = ok and stack_frontier.get("overall_pass") is True
for name in ("stack", "frontier"):
    result = stack_frontier.get(name, {})
    if not result.get("passed"):
        print("  FAILED model: %s (violations=%d)" % (
            name, len(result.get("violations", []))))
with open(sys.argv[3]) as f:
    exchange = json.load(f)
records = exchange.get("records", [])
exchange_ok = len(records) == 2
for result in records:
    properties = result.get("deterministic_witness", {}).get("properties")
    if properties is None:
        properties = {}
        for key in ("properties_c", "properties_v"):
            properties.update(result.get("deterministic_witness", {}).get(key, {}))
    record_ok = (
        result.get("classification") == "confirmed_safe"
        and properties
        and all(properties.values())
    )
    exchange_ok = exchange_ok and record_ok
    if not record_ok:
        print("  FAILED model: exchange/%s" % result.get("name", "unnamed"))
ok = ok and exchange_ok
with open(sys.argv[4]) as f:
    vault = json.load(f)
vault_ok = vault.get("overall_pass") is True
if not vault_ok:
    print("  FAILED model: vault-backed lifecycle")
ok = ok and vault_ok
sys.exit(0 if ok else 1)
PY
then
  echo "Cost-monad Sage gate passed."
else
  echo "error: cost-monad Sage verification did not pass (overall_pass != true)" >&2
  exit 1
fi
