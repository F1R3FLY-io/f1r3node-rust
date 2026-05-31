import Validator.SlashAuthorization
import Validator.Determinism

/-
  Validator — Lean 4 mirror of the validator / platform obligations and the
  named behavioral-contract aggregator (Workstream E, DR-12).

  Scope (DR-12): the validator obligation set ONLY. Submodules added by E4/E6:
    * `Validator.SlashAuthorization` — P1 (`stale_evidence_not_authorized`, `bm_slash_lookup`).
    * `Validator.Determinism`        — P3 (verdict-determinism wrapper over `ca_step_deterministic`).
    * `Validator.Contract`           — the aggregator: `validator_contract_built_in_*` bundling
                                       S1–S4 + P1 + P3 as named contract clauses (E6 milestone:
                                       "built-in validator proven in all three provers").

  This root module is the E1 scaffold; dependency-free (core `Init` only).
-/

namespace Validator

/-- E1 scaffold marker for the validator library. -/
theorem scaffold_validator_ok : True := trivial

end Validator

/-- Top-level re-export for the axiom-gate driver. -/
theorem scaffold_validator_ok : True := Validator.scaffold_validator_ok
