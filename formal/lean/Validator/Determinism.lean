import CostAccountedRho.StepDeterminism

/-
  Validator.Determinism — Lean 4 mirror of the validator-scoped P3 (verdict
  determinism) obligation (Workstream E, stage E4; DR-12).

  KERNEL (P3). The validator's VERDICT is a DETERMINISTIC FUNCTION of
  (system, pre-state): given the same system and the same pre-state, any two
  successor states a validator could compute coincide. This is the validator-
  contract re-statement of the E3 step-determinism result
  `CostAccountedRho.ca_step_deterministic`
  (`formal/lean/CostAccountedRho/StepDeterminism.lean`;
   Rocq `ca_step_deterministic`, StepDeterminism.v:156-222): in a
  `single_token_sys`, `ca_step` admits at most one successor.

  SCOPE (DR-12). This is a THIN, FAITHFUL wrapper: it does NOT re-derive the
  step relation — it imports E3 and lifts the proven step-determinism to the
  validator-verdict vocabulary. P3's FULL state-machine SCHEDULE-INDEPENDENCE (a
  multi-step / multi-thread interleaving property) is additionally discharged by
  the TLA+ model `RuntimeBudgetReplay.tla`
  (`ConsumedAndVerdictScheduleIndependent`); the Lean clause is the single-step
  DETERMINISM KERNEL that schedule-independence rests on. We state both the
  single-step verdict determinism and a bounded two-step lift to show the kernel
  composes (the n-step closure is the TLA+ obligation, not re-proved here).

  NON-VACUITY. The premise `single_token_sys s ∧ ca_step s t` is INHABITED:
  `CostAccountedRho.ca_step_one_token_example` is a concrete single-token system
  that actually steps. `validator_verdict_example` re-exposes that witness at the
  validator level, so `validator_verdict_deterministic` is NOT vacuously true
  over an empty step relation; and a model whose step were non-deterministic
  would make it FALSE.

  DEPENDENCY-FREE: core `Init` only (no mathlib/batteries); all content is
  inherited from the E3 mirror.
-/

namespace Validator

open CostAccountedRho
open CostAccountedRho.System

/-- The validator OUTCOME relation on a system: `validator_outcome s t` holds
    when `t` is a state the validator may compute as the successor of `s` under
    the cost-accounted reduction — i.e. exactly `ca_step s t` (Rocq `ca_step`,
    CostAccountedReduction.v:95-187, kernel mirrored in E3). Naming the step
    relation as the validator "outcome" is the vocabulary bridge from the
    calculus-level determinism to the validator-contract obligation P3. -/
abbrev validator_outcome (s t : System) : Prop := ca_step s t

/-- P3 HEADLINE — the VALIDATOR VERDICT IS DETERMINISTIC (validator-contract
    re-statement of `CostAccountedRho.ca_step_deterministic`, StepDeterminism.v:
    156-222): for a single-token system `s` (the invariant that holds at every
    point of a single-deploy execution — one token released at a time), any two
    successors `t1`, `t2` a validator could compute coincide. So two honest
    validators replaying the same pre-state reach the SAME post-state — the
    verdict is a function of (system, pre-state), never of scheduling within the
    single-token frontier.

    This is a THIN FAITHFUL LIFT: the proof IS `ca_step_deterministic` (proven in
    E3 by induction-on-first-step / inversion-on-second-step). Stating it in the
    validator vocabulary makes P3 a named clause of the behavioral contract
    without weakening or re-deriving the kernel. -/
theorem validator_verdict_deterministic
    {s t1 t2 : System}
    (hsingle : single_token_sys s)
    (hstep1 : validator_outcome s t1)
    (hstep2 : validator_outcome s t2) :
    t1 = t2 :=
  ca_step_deterministic hsingle hstep1 hstep2

/-- NON-VACUITY WITNESS (P3): a concrete single-token system that actually has a
    verdict — `SPar (SSigned 0) (SToken 3)` steps to `SPar (SSigned 1) (SToken 3)`
    (reusing E3's `ca_step_one_token_example`). This inhabits the premise of
    `validator_verdict_deterministic`, so the determinism claim is non-vacuous. -/
theorem validator_verdict_example :
    validator_outcome (SPar (SSigned 0) (SToken 3)) (SPar (SSigned 1) (SToken 3)) :=
  ca_step_one_token_example

/-- The witness system is in the domain `validator_verdict_deterministic`
    quantifies over (its token-node count is 1 ≤ 1), so the determinism premise
    `single_token_sys s` is genuinely satisfiable alongside an actual step. -/
theorem validator_verdict_example_single :
    single_token_sys (SPar (SSigned 0) (SToken 3)) :=
  ca_step_one_token_example_single

/-- P3 (bounded lift) — the verdict is deterministic across a TWO-STEP unfolding:
    if `s` is single-token and reduces `s → m1 → t1` and `s → m2 → t2` along
    `ca_step`, then `t1 = t2`. The intermediate states coincide by the headline
    (`m1 = m2`), single-tokenness is preserved (`sys_token_node_count_monotonic`,
    StepDeterminism.v:115-121), and a second application of the headline closes
    the second step. This shows the determinism KERNEL COMPOSES; the full n-step
    schedule-independence closure is the TLA+ `RuntimeBudgetReplay.tla`
    obligation (`ConsumedAndVerdictScheduleIndependent`), not re-proved here. -/
theorem validator_verdict_deterministic_two_step
    {s m1 m2 t1 t2 : System}
    (hsingle : single_token_sys s)
    (h1a : validator_outcome s m1) (h1b : validator_outcome m1 t1)
    (h2a : validator_outcome s m2) (h2b : validator_outcome m2 t2) :
    t1 = t2 := by
  -- First steps agree.
  have hmid : m1 = m2 := ca_step_deterministic hsingle h1a h2a
  subst hmid
  -- single_token_sys is preserved into m1, so the headline applies to the 2nd step.
  have hsingle_m1 : single_token_sys m1 := by
    have hmono : sys_token_node_count m1 ≤ sys_token_node_count s :=
      sys_token_node_count_monotonic h1a
    unfold single_token_sys at hsingle ⊢
    omega
  exact ca_step_deterministic hsingle_m1 h1b h2b

end Validator
