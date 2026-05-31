import CostAccountedRho.LinearLogicResources
import CostAccountedRho.FuelGateSafety
import CostAccountedRho.StepDeterminism

/-
  CostAccountedRho — Lean 4 mirror of the validator-scoped subset of the Rocq
  cost-accounted calculus corpus (Workstream E, DR-12).

  Scope (DR-12): the validator obligation set ONLY. Submodules added by E2/E3:
    * `CostAccountedRho.LinearLogicResources` — S2 (`funding_decidable`) + S3
      (`ll_no_double_spend_single_witness`, `ll_linear_no_contraction`).
    * `CostAccountedRho.FuelGateSafety`        — S1 (`fuel_gate_rejects_mismatched_token`).
    * `CostAccountedRho.StepDeterminism`       — S4/P3 (`ca_step_deterministic`, `core_token_demand`).

  Explicitly NOT mirrored in Lean (stays Rocq-only, staged behind Rocq):
  StrongNormalization, Confluence, Translation, Bisimulation, Replication,
  MultiSignerRefinement, Settlement, Exchange.

  This root module is the E1 scaffold. It is dependency-free (core `Init`
  only) so `lake build` is fully offline.
-/

namespace CostAccountedRho

/-- E1 scaffold marker. Replaced/augmented by the S1–S4 mirrors in E2/E3;
    retained so the axiom gate has a stable anchor theorem from the start. -/
theorem scaffold_cost_accounted_ok : True := trivial

end CostAccountedRho

/-- Re-export at the top level so the axiom-gate driver can `#print axioms`
    it without a namespace prefix. -/
theorem scaffold_cost_accounted_ok : True := CostAccountedRho.scaffold_cost_accounted_ok
