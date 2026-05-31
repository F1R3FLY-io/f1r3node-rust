#!/usr/bin/env bash
# scripts/check-cost-accounted-rho-lean.sh
#
# LOCAL-ONLY Lean 4 proof gate for the validator behavioral contract
# (Workstream E, DR-12). Mirrors check-cost-accounted-rho-proofs.sh:
#   1. hygiene gate  — no `sorry`/`admit`/`native_decide`/bare `axiom`
#                      (the Lean analogues of Coq `Admitted.`/`Axiom`),
#                      and no TODO/FIXME/placeholder.
#   2. build gate    — offline `lake build` (core `Init` only; no mathlib).
#   3. axiom gate    — `#print axioms` for every contract theorem shows no
#                      `sorryAx` and (because bare `axiom` is banned) no user
#                      axiom; only Lean's foundational kernel axioms
#                      (propext / Classical.choice / Quot.sound) are allowed.
#
# Per team policy this is NOT a CI gate (formal verification stays local).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LEAN_DIR="$ROOT/formal/lean"
export PATH="$HOME/.elan/bin:$PATH"

command -v lake >/dev/null 2>&1 || { echo "error: lake (elan) not found on PATH (expected ~/.elan/bin)" >&2; exit 1; }

echo "Checking validator Lean proof hygiene..."
# `sorry`/`admit` = unfinished proof; `native_decide` adds compiler-trust
# axioms; a bare `axiom` declaration is an unproven assumption.
if rg -n -g '*.lean' '\bsorry\b|\badmit\b|native_decide|^[[:space:]]*axiom[[:space:]]|TODO|FIXME|placeholder' "$LEAN_DIR"; then
  echo "ERROR: forbidden token in Lean sources (sorry/admit/native_decide/axiom/TODO/FIXME/placeholder)." >&2
  exit 1
fi

echo "Building Lean validator obligations (offline; core Init only)..."
( cd "$LEAN_DIR" && lake build )

# The contract theorem list. E6 expands this to the full S1-S4 / P1,P3 set.
# E2 added the S2 (funding) + S3 (linearity) mirrors of the Rocq
# LinearLogicResources.v obligations (fully-qualified so they resolve after
# `import CostAccountedRho`). E3 added the S1 (fuel-gate token safety) mirror of
# FuelGateSafety.v and the S4 (transaction demand + single-token step
# determinism) mirror of StepDeterminism.v + LinearLogicResources.v:481.
CONTRACT_THEOREMS=(
  scaffold_cost_accounted_ok
  scaffold_validator_ok
  # S2 — funding (cost-accounted-rho §7.5; Rocq LinearLogicResources.v 553-699)
  CostAccountedRho.delta_s_tensor_additive
  CostAccountedRho.funding_decidable
  CostAccountedRho.sigma_s_balance_eq_stack_count
  CostAccountedRho.funding_check_balance_sound
  CostAccountedRho.funding_check_balance_sound_against_stack
  # S3 — linearity (cost-accounted-rho §7.6/§7.7; Rocq 324-393)
  CostAccountedRho.ll_consume_linear_once_atom_exhausts
  CostAccountedRho.ll_no_double_spend_single_witness
  CostAccountedRho.ll_double_spend_requires_duplicate_witness
  CostAccountedRho.ll_linear_no_contraction
  # S1 — fuel-gate token safety (cost-accounted-rho §6.3; Rocq FuelGateSafety.v 277-328)
  CostAccountedRho.fuel_gate_rejects_mismatched_token
  CostAccountedRho.fuel_gate_rejects_mismatched_token_ground
  CostAccountedRho.fuel_gate_rejects_cross_axis_token
  CostAccountedRho.gate_fires_iff_names_eq
  CostAccountedRho.fuel_gate_no_fire_mismatched
  CostAccountedRho.fuel_gate_no_fire_cross_axis
  CostAccountedRho.gate_fires_self
  # S4 — transaction demand (§7.1; Rocq LinearLogicResources.v:481) +
  #      single-token step determinism (Rocq StepDeterminism.v 69-222)
  CostAccountedRho.core_token_demand_and_additive
  CostAccountedRho.ca_step_deterministic
  CostAccountedRho.ca_step_requires_token_node
  CostAccountedRho.sys_token_node_count_monotonic
  CostAccountedRho.token_split_zero
  CostAccountedRho.no_token_no_step
  CostAccountedRho.ca_step_one_token_example
)

echo "Axiom gate: #print axioms must show no sorryAx and no user axiom..."
DRIVER="$LEAN_DIR/.axiom_gate_driver.lean"
{
  echo "import CostAccountedRho"
  echo "import Validator"
  for thm in "${CONTRACT_THEOREMS[@]}"; do
    echo "#print axioms $thm"
  done
} > "$DRIVER"
AXIOM_OUT="$( cd "$LEAN_DIR" && lake env lean "$DRIVER" 2>&1 )"
rm -f "$DRIVER"
echo "$AXIOM_OUT"
if echo "$AXIOM_OUT" | rg -q "sorryAx"; then
  echo "ERROR: a contract theorem depends on sorryAx (a sorry leaked into a proof)." >&2
  exit 1
fi

echo "Lean validator proof hygiene check passed."
