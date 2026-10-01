/-
  CostAccountedRho.StepDeterminism — Lean 4 mirror of the validator-scoped
  S4 (transaction demand + single-token step determinism) obligation from
    `formal/rocq/cost_accounted_rho/theories/StepDeterminism.v`        (S4 step)
    `formal/rocq/cost_accounted_rho/theories/LinearLogicResources.v:481` (demand)
  (Workstream E, stage E3; DR-12).

  SCOPE (DR-12). Per DR-12 the Lean validator mirror does NOT port `proc`/
  `RhoSyntax`, the full Rocq `system` (whose `SSigned` carries a `proc` and whose
  `SToken` carries the right-nested `token`), the five concrete COMM rules of the
  Rocq `ca_step` (CostAccountedReduction.v:95-187), or Confluence.v's per-rule
  determinism machinery. We mirror the LOAD-BEARING KERNEL of S4 over a minimal
  faithful model.

  STEP OPTION CHOSEN: (A) CONCRETE minimal `ca_step`. We define a small concrete
  step relation over a minimal `System` and prove `ca_step_deterministic` by
  induction/inversion — no parametric per-rule-determinism hypothesis is needed,
  so the result is strictly self-contained and the axiom gate stays clean. This
  is more faithful than the abstract dispatch because the determinism content
  (the base-rule's local determinism AND the PAR-rule single-token dispatch) is
  PROVEN here rather than assumed. The model abstracts the `proc`/`token`
  payloads to `Nat` (an abstract process-id / token-stack depth): this is the
  minimal payload that keeps (i) the base rule's local determinism non-trivial —
  it transforms the process id and decrements the token depth as a genuine
  function of the redex — and (ii) `sys_token_node_count` faithful to the Rocq
  `SToken`-constructor count. We do NOT need the term structure of `proc`/`token`
  for any S4 obligation, so abstracting it is faithful to the kernel.

  NON-VACUITY. `ca_step_one_token_example` exhibits a CONCRETE single-token
  system that actually steps, so `ca_step_deterministic` is not vacuously true
  over an empty step relation. The base rule genuinely requires an `SToken` node
  (mirroring `ca_step_requires_token_node`, StepDeterminism.v:87).

  DEPENDENCY-FREE: core `Init` only (no mathlib/batteries). `omega` (core)
  discharges the `Nat` arithmetic; `induction`/`cases` discharge the relation.
-/

namespace CostAccountedRho

/- ═══════════════════════════════════════════════════════════════════════════
   Part 1 — transaction demand  (cost-accounted-rho §7.1;
            LinearLogicResources.v:481-487 `core_token_demand`)
   ═══════════════════════════════════════════════════════════════════════════ -/

/-- The core signature grammar (Def 3.3), mirroring the Rocq `sig`
    (CostAccountedSyntax.v:93-97): `SUnit | SGround bs | SQuote bs | SAnd s1 s2`.
    Carries the Def-3.3 ground/quote axis split that `core_token_demand` sums. -/
inductive Sig : Type
  | SUnit  : Sig                  -- () — unit signature (no token gated)
  | SGround : List Bool → Sig     -- g — ground axis (gates one token)
  | SQuote : List Bool → Sig      -- #P — quote axis (gates one token)
  | SAnd   : Sig → Sig → Sig      -- s₁ & s₂ — compound (sums components)

open Sig

/-- The core token demand of a signature (Rocq `core_token_demand`,
    LinearLogicResources.v:481-487): each atomic axis (ground or quote) gates
    exactly one token; `SUnit` gates none; `SAnd` sums its components. This is
    the per-COMM linear-token obligation of the cost-accounted core. -/
def core_token_demand : Sig → Nat
  | SUnit => 0
  | SGround _ => 1
  | SQuote _ => 1
  | SAnd s1 s2 => core_token_demand s1 + core_token_demand s2

/-- §7.1 transaction-demand ADDITIVITY (the additive content of the Rocq
    `core_token_demand` `SAnd` arm, LinearLogicResources.v:486; cf. the proved
    `core_demand_invariant_under_extension`, :492, and `delta_s` additivity
    in the E2 mirror): the demand of a compound signature is the sum of its
    components' demands. A composite transaction needs exactly the sum of the
    fuel its conjuncts need — no contraction, no inflation. -/
theorem core_token_demand_and_additive (s1 s2 : Sig) :
    core_token_demand (SAnd s1 s2) = core_token_demand s1 + core_token_demand s2 :=
  rfl

/- ═══════════════════════════════════════════════════════════════════════════
   Part 2 — single-token step determinism  (StepDeterminism.v:69-222)
   ═══════════════════════════════════════════════════════════════════════════ -/

/-- The system model, mirroring the Rocq `system` (CostAccountedSyntax.v:136-139):
    `SSigned P s | SToken t | SPar S1 S2`. Per DR-12 the `proc` payload of
    `SSigned` and the `token` payload of `SToken` are abstracted to `Nat` (an
    abstract process-id / token-stack depth); this is the minimal payload that
    keeps the base rule's local determinism non-trivial while leaving
    `sys_token_node_count` an exact mirror of the Rocq `SToken`-constructor
    count. -/
inductive System : Type
  | SSigned : Nat → System            -- P^s — process (abstract id) sealed under a signature
  | SToken  : Nat → System            -- T — free token of (abstract) stack depth
  | SPar    : System → System → System -- S₁ ∥ S₂ — parallel composition
  deriving DecidableEq

open System

/-- The NUMBER OF `SToken` CONSTRUCTORS in a system (Rocq `sys_token_node_count`,
    StepDeterminism.v:69-74): `SSigned ↦ 0`, `SToken ↦ 1`, `SPar ↦` sum. Counts
    token LEAVES, not fuel depth. -/
def sys_token_node_count : System → Nat
  | SSigned _ => 0
  | SToken _ => 1
  | SPar s1 s2 => sys_token_node_count s1 + sys_token_node_count s2

/-- The single-token invariant (Rocq `single_token_sys`, StepDeterminism.v:76-77):
    at most one `SToken` leaf anywhere in the parallel tree. Holds at every point
    of a single-deploy execution — the token chain releases one token at a time. -/
def single_token_sys (s : System) : Prop :=
  sys_token_node_count s ≤ 1

/-- The CONCRETE cost-accounted step relation (option A), the minimal faithful
    image of the Rocq `ca_step` (CostAccountedReduction.v:95-187). The base rule
    `ca_base` is the kernel of the five Rocq COMM rules: it fires on a redex of
    shape `SPar (SSigned p) (SToken d)` — exactly one `SToken` leaf — and
    produces `SPar (SSigned p') (SToken d')` where the successor `(p', d')` is a
    FUNCTION of the redex (`p' = p + 1`, `d' = d`, mirroring "the body is
    substituted and one gate is stripped while the token leaf persists"; cf. Rocq
    Rule 1, CostAccountedReduction.v:103-108, which preserves the `SToken` leaf
    `SToken t`). Because the successor is a function of the inputs, the base rule
    is LOCALLY DETERMINISTIC. `ca_par_l`/`ca_par_r` are the contextual closures
    (Rocq `ca_par_l`/`ca_par_r`, :178-185).

    EVERY rule requires an `SToken` node (the base rule contains one literally;
    the PAR rules recurse into a stepping sub-system), mirroring
    `ca_step_requires_token_node` (StepDeterminism.v:87). -/
inductive ca_step : System → System → Prop
  | ca_base : ∀ (p d : Nat),
      ca_step (SPar (SSigned p) (SToken d))
              (SPar (SSigned (p + 1)) (SToken d))
  | ca_par_l : ∀ {s1 s1' : System} (s2 : System),
      ca_step s1 s1' →
      ca_step (SPar s1 s2) (SPar s1' s2)
  | ca_par_r : ∀ (s1 : System) {s2 s2' : System},
      ca_step s2 s2' →
      ca_step (SPar s1 s2) (SPar s1 s2')

/-- NON-VACUITY WITNESS: a concrete single-token system that actually steps.
    `SPar (SSigned 0) (SToken 3)` has exactly one `SToken` leaf and reduces by
    the base rule to `SPar (SSigned 1) (SToken 3)`. This proves the step relation
    is inhabited on single-token systems, so `ca_step_deterministic` is NOT
    vacuously true. -/
theorem ca_step_one_token_example :
    ca_step (SPar (SSigned 0) (SToken 3)) (SPar (SSigned 1) (SToken 3)) :=
  ca_step.ca_base 0 3

/-- This witness system satisfies the single-token invariant (count = 1 ≤ 1),
    so the non-vacuity witness is in the domain `ca_step_deterministic` quantifies
    over. -/
theorem ca_step_one_token_example_single :
    single_token_sys (SPar (SSigned 0) (SToken 3)) := by
  unfold single_token_sys
  decide

/-- Every `ca_step` requires at least one `SToken` node in the source (Rocq
    `ca_step_requires_token_node`, StepDeterminism.v:87-92): case analysis on the
    rule — the base rule has one literally; the PAR rules inherit one from the
    stepping sub-system by induction. -/
theorem ca_step_requires_token_node {s t : System} :
    ca_step s t → sys_token_node_count s ≥ 1 := by
  intro h
  induction h with
  | ca_base p d => simp [sys_token_node_count]
  | ca_par_l s2 _ ih => simp only [sys_token_node_count]; omega
  | ca_par_r s1 _ ih => simp only [sys_token_node_count]; omega

/-- A system with no `SToken` nodes cannot step (Rocq `no_token_no_step`,
    StepDeterminism.v:96-102): direct corollary of `ca_step_requires_token_node`. -/
theorem no_token_no_step {s : System} (h0 : sys_token_node_count s = 0) :
    ∀ t, ¬ ca_step s t := by
  intro t hstep
  have h1 := ca_step_requires_token_node hstep
  omega

/-- Arithmetic helper (Rocq `token_split_zero`, StepDeterminism.v:105-107, `lia`):
    if `a + b ≤ 1` and `a ≥ 1` then `b = 0`. Powers the PAR dispatch — once one
    branch is known to hold the single token, the other branch is token-free. -/
theorem token_split_zero (a b : Nat) : a + b ≤ 1 → a ≥ 1 → b = 0 := by
  omega

/-- Every `ca_step` preserves or decreases the `SToken`-constructor count (Rocq
    `sys_token_node_count_monotonic`, StepDeterminism.v:115-121): the base rule
    keeps the single leaf (`SToken d ↦ SToken d`); the PAR rules recurse. Needed
    so `single_token_sys` is preserved along a single-token reduction path. -/
theorem sys_token_node_count_monotonic {s s' : System} :
    ca_step s s' → sys_token_node_count s' ≤ sys_token_node_count s := by
  intro h
  induction h with
  | ca_base p d => simp [sys_token_node_count]
  | ca_par_l s2 _ ih => simp only [sys_token_node_count]; omega
  | ca_par_r s1 _ ih => simp only [sys_token_node_count]; omega

/-- HEADLINE THEOREM — single-token step determinism (Rocq `ca_step_deterministic`,
    StepDeterminism.v:156-222): in a `single_token_sys`, `ca_step` is
    DETERMINISTIC — from any state there is at most one successor.

    PROOF (mirroring the Rocq strategy: induction on the first step, inversion on
    the second):
    * BASE/BASE: both reductions are the base rule on the same redex, whose
      successor is a function of `(p, d)`, so the two successors coincide.
    * BASE vs. PAR: the base redex is `SPar (SSigned p) (SToken d)`; a PAR rule
      stepping the left branch would need `SSigned p` to step (impossible — no
      rule has an `SSigned` source), and stepping the right branch would need
      `SToken d` to step (impossible — likewise). These competing reductions are
      ruled out by inversion (no `ca_step` has an `SSigned`/`SToken` source).
    * PAR_L vs. PAR_L: the single token lives in the left branch; the inductive
      hypothesis (which applies because `single_token_sys s1`) forces the inner
      successors equal.
    * PAR_L vs. PAR_R: the left branch holds the token (count ≥ 1), so by
      `token_split_zero` the right branch is token-free and cannot step
      (`no_token_no_step`) — contradiction.
    * symmetric for the PAR_R first-step cases.

    NON-VACUOUS: `ca_step_one_token_example` is a real single-token system that
    steps, so the universally-quantified conclusion has inhabited premises; and a
    model whose base rule were non-deterministic (two distinct successors) would
    make this FALSE. -/
theorem ca_step_deterministic {s t1 t2 : System} :
    single_token_sys s → ca_step s t1 → ca_step s t2 → t1 = t2 := by
  intro hsingle hstep1
  induction hstep1 generalizing t2 with
  | ca_base p d =>
      -- The source is `SPar (SSigned p) (SToken d)`. Only the base rule can fire:
      -- a PAR rule would require `SSigned`/`SToken` to step, impossible.
      intro hstep2
      cases hstep2 with
      | ca_base p' d' => rfl
      | ca_par_l s2 hsub => exact absurd hsub (by intro h; cases h)
      | ca_par_r s1 hsub => exact absurd hsub (by intro h; cases h)
  | @ca_par_l a a' s2 hsub ih =>
      -- Source `SPar a s2`; the left branch `a` steps. The token lives in `a`.
      intro hstep2
      unfold single_token_sys sys_token_node_count at hsingle
      have hcount_a : sys_token_node_count a ≥ 1 := ca_step_requires_token_node hsub
      have hzero_s2 : sys_token_node_count s2 = 0 :=
        token_split_zero _ _ hsingle hcount_a
      cases hstep2 with
      | ca_base p d =>
          -- Then `a = SSigned p` (which cannot step). Contradiction via hsub.
          exact absurd hsub (by intro h; cases h)
      | @ca_par_l b b' s2'' hsub2 =>
          -- Both step the left branch. IH gives `a' = b'`.
          have hsingle_a : single_token_sys a := by
            unfold single_token_sys; omega
          have := ih hsingle_a hsub2
          rw [this]
      | ca_par_r s1'' hsub2 =>
          -- The right branch `s2` steps, but it is token-free. Contradiction.
          exact absurd hsub2 (no_token_no_step hzero_s2 _)
  | @ca_par_r s1 b b' hsub ih =>
      -- Source `SPar s1 b`; the right branch `b` steps. The token lives in `b`.
      intro hstep2
      unfold single_token_sys sys_token_node_count at hsingle
      have hcount_b : sys_token_node_count b ≥ 1 := ca_step_requires_token_node hsub
      have hzero_s1 : sys_token_node_count s1 = 0 := by omega
      cases hstep2 with
      | ca_base p d =>
          -- Then `b = SToken d` (which cannot step). Contradiction via hsub.
          exact absurd hsub (by intro h; cases h)
      | ca_par_l s2'' hsub2 =>
          -- The left branch `s1` steps, but it is token-free. Contradiction.
          exact absurd hsub2 (no_token_no_step hzero_s1 _)
      | @ca_par_r s1'' c c' hsub2 =>
          -- Both step the right branch. IH gives `b' = c'`.
          have hsingle_b : single_token_sys b := by
            unfold single_token_sys; omega
          have := ih hsingle_b hsub2
          rw [this]

end CostAccountedRho
