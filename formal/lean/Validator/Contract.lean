/-
  Validator.Contract — the named VALIDATOR BEHAVIORAL CONTRACT (Workstream E,
  stage E6; DR-12). This is the "built-in validator proven in Lean" milestone:
  it bundles the seven contract obligations (S1–S4 spec + P1,P3 platform; P2 is
  discharged by Rocq+TLA+) as named `validator_contract_built_in_*` clauses, each
  a definitional alias of the per-obligation result proven in E2–E4. Naming them
  here gives a single Lean surface a CUSTOM validator (or a reviewer) can point at
  as "the contract." The SAME contract is discharged in Rocq
  (`formal/rocq/validator/`) and TLA+ (`formal/tlaplus/validator/`).

  Each clause is a `def` (an alias copies the obligation's exact type without a
  fragile restatement; `#print axioms` reports its kernel footprint identically to
  a `theorem`, so the axiom gate validates these clauses just as it does the
  underlying obligations). The clause TYPE is the obligation it re-exports.

  Contract ↔ obligation ↔ spec map (see docs/theory/cost-accounting-impl/
  workstream-e-validator-contract.md for the full table):
    S1  token-present / reject-malformed   §6.3   ← FuelGateSafety (E3)
    S2  accept iff Σ_s ≥ Δ_s (decidable)    §7.6   ← LinearLogicResources (E2)
    S3  linear no-double-spend              §7.7   ← LinearLogicResources (E2)
    S4  for-comp = atomic funded txn        §7.1   ← StepDeterminism (E3)
    P1  slash-authorization soundness       DR-12  ← SlashAuthorization (E4)
    P3  determinism / replay-equivalence    DR-12  ← Determinism (E4)
-/

import CostAccountedRho
import Validator.SlashAuthorization
import Validator.Determinism

namespace Validator

/-- Contract clause S1 (§6.3 token-presence): a ground fuel gate can never be
    funded by a cryptographic-quote token — distinct axes give distinct channel
    names, so the COMM cannot fire across axes. -/
def validator_contract_built_in_S1 :=
  CostAccountedRho.fuel_gate_rejects_cross_axis_token

/-- Contract clause S2 (§7.6 acceptance): the funding obligation `Σ_s ≥ Δ_s` is
    DECIDABLE — the validator always reaches an accept/reject verdict by one
    integer comparison, before any execution. Carries the verdict as data
    (`Decidable`), hence `@[reducible] def`. -/
@[reducible] def validator_contract_built_in_S2 :=
  CostAccountedRho.funding_decidable

/-- Contract clause S3 (§7.7 linearity): no double-spend from a single witness —
    a linear token, once consumed, cannot be consumed again. -/
def validator_contract_built_in_S3 :=
  CostAccountedRho.ll_no_double_spend_single_witness

/-- Contract clause S4 (§7.1 transaction atomicity): in a single-token system the
    per-COMM step is DETERMINISTIC — the transaction outcome is a function of the
    redex. -/
def validator_contract_built_in_S4 :=
  @CostAccountedRho.ca_step_deterministic

/-- Contract clause P1 (slash-authorization, effect): slashing a validator zeros
    exactly that validator's bond. -/
def validator_contract_built_in_P1 :=
  bm_slash_lookup

/-- Contract clause P1 (slash-authorization, determinism): slashing a SET of
    validators is order-independent at the bond-lookup level — the
    consensus-critical multi-parent-merge determinism of slashing. -/
def validator_contract_built_in_P1_order_independent :=
  bm_slash_many_order_independent

/-- Contract clause P1 (slash-authorization, soundness): stale-epoch evidence
    cannot authorize slashing a rebonded key. -/
def validator_contract_built_in_P1_stale_evidence :=
  stale_evidence_not_authorized

/-- Contract clause P3 (determinism / replay-equivalence): the validator verdict
    is a deterministic function of the system — identical across schedules. P3's
    full multi-step schedule-independence is additionally discharged by TLA+
    `RuntimeBudgetReplay.tla`. -/
def validator_contract_built_in_P3 :=
  @validator_verdict_deterministic

end Validator
