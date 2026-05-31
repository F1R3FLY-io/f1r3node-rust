/-
  CostAccountedRho.FuelGateSafety — Lean 4 mirror of the validator-scoped
  S1 (fuel-gate token safety) obligation from the Rocq development
  `formal/rocq/cost_accounted_rho/theories/FuelGateSafety.v`
  (Workstream E, stage E3; DR-12).

  SCOPE (DR-12). The Rocq module proves fuel-gate safety at the level of the
  TRANSLATED processes: a fuel gate `P_tr P s = PInput (N_tr s) …` can only be
  funded by a token `T_tr (TGate s' t) = POutput (N_tr s') …` whose channel
  matches, i.e. whose top-level COMM redex requires `N_tr s = N_tr s'`. Per
  DR-12 the Lean validator mirror does NOT port `proc`/`RhoSyntax`, the
  translation `P_tr`/`T_tr`/`N_tr`, or the reduction relation `rho_step`.
  Instead it mirrors the LOAD-BEARING KERNEL of every S1 theorem.

  The Rocq theorems `fuel_gate_rejects_mismatched_token` (FuelGateSafety.v:277),
  `_ground` (:298) and `_cross_axis` (:317) each `simpl; inversion`-reduce the
  COMM-redex name equality to an equality of `N_tr`-translated channel names,
  then discharge it via exactly one of the three audited disjointness
  hypotheses on the abstract quote/ground channel-builders:
    * `hp_injective`   (FuelGateSafety.v:225) — distinct quote bytes ⇒ distinct names;
    * `gp_injective`   (FuelGateSafety.v:228) — distinct ground bytes ⇒ distinct names;
    * `gp_hp_disjoint` (FuelGateSafety.v:234) — a ground name never equals a quote name.

  THE KERNEL = NAME DISJOINTNESS. We model channel names by

      inductive ChanName | quoteName (bs : List Bool) | groundName (bs : List Bool)

  whose constructors carry the Def-3.3 AXIS (quote vs. ground) and the BYTES.
  Lean's auto-generated theory gives, for free, exactly the three audited facts:
    * `ChanName.quoteName.injEq`  is the Lean image of `hp_injective`
      (and of `N_tr_quote_injective`, FuelGateSafety.v:240);
    * `ChanName.groundName.injEq` is the Lean image of `gp_injective`
      (and of `N_tr_ground_injective`, FuelGateSafety.v:252);
    * `quoteName _ ≠ groundName _` (distinct constructors) is the Lean image of
      `gp_hp_disjoint` (and of `N_tr_ground_quote_distinct`, FuelGateSafety.v:265).

  NON-VACUITY. `gate_fires` is a real boolean decision on `ChanName` equality,
  and the `gate_fires_self` witness (a gate FIRING on a matching token) proves
  the firing relation is inhabited, so the three rejection theorems are not
  vacuously true over a never-firing gate. Each rejection theorem genuinely USES
  constructor injectivity / disjointness (a wrong model — e.g. one collapsing the
  bytes, or identifying the two axes — would make the theorem FALSE).

  DEPENDENCY-FREE: core `Init` only (no mathlib/batteries). `List Bool`
  equality is decidable in core, so `gate_fires` and every disequality below are
  provable fully offline with `simp`/`decide`/`omega`.
-/

namespace CostAccountedRho

/- ═══════════════════════════════════════════════════════════════════════════
   S1 — fuel-gate token safety  (cost-accounted-rho §6.3; FuelGateSafety.v)
   ═══════════════════════════════════════════════════════════════════════════ -/

/-- The channel-name model: the validator-relevant kernel of the Rocq `name`
    image `N_tr hp gp s`. A name carries the Def-3.3 AXIS as its constructor
    (quote vs. ground) and the signature BYTES as its payload (Rocq `N_tr` on the
    atomic `SQuote`/`SGround` arms; FuelGateSafety.v:240-272). The two
    constructors are the leftmost atomic component of the gate/token signature
    that the COMM rule keys on. Lean derives `DecidableEq`, constructor
    injectivity, and constructor disjointness — precisely the audited facts the
    Rocq proofs invoke. -/
inductive ChanName : Type
  | quoteName  : List Bool → ChanName   -- N_tr (SQuote bs) — cryptographic-quote axis
  | groundName : List Bool → ChanName   -- N_tr (SGround bs) — ground axis
  deriving DecidableEq

open ChanName

/-- The fuel-gate firing predicate: a gate keyed to channel-name `gateName`
    fires against a token on channel-name `tokenName` IFF the two names are
    equal. This is the boolean shadow of the Rocq COMM-redex name match — the
    top-level `rs_comm` redex `PPar (PInput (N_tr s) …) (POutput (N_tr s') …)`
    fires exactly when `N_tr s = N_tr s'` (FuelGateSafety.v:281-283, the redex
    shape whose channel equality the three theorems refute). `DecidableEq` makes
    the gate's accept/reject verdict a single comparison. -/
def gate_fires (gateName tokenName : ChanName) : Bool :=
  decide (gateName = tokenName)

/-- Firing soundness: the boolean `gate_fires` is `true` iff the gate and token
    names are equal — so a gate is FUNDED only by a token on its own channel.
    This is the explicit "gate funded only by matching name" reading; the three
    rejection theorems below combine it with the name-disequalities. -/
theorem gate_fires_iff_names_eq (gateName tokenName : ChanName) :
    gate_fires gateName tokenName = true ↔ gateName = tokenName := by
  unfold gate_fires
  exact decide_eq_true_iff

/-- NON-VACUITY WITNESS: a gate keyed to `quoteName bs` DOES fire against a
    matching token on the very same name. The firing relation is inhabited, so
    the rejection theorems are not vacuously true over a gate that never fires.
    (Compare the Rocq `t_tr_gate_shape`, FuelGateSafety.v:127 — a token on the
    matching channel is exactly what the gate consumes.) -/
theorem gate_fires_self (bs : List Bool) :
    gate_fires (quoteName bs) (quoteName bs) = true := by
  unfold gate_fires
  simp

/-- A second non-vacuity witness on the ground axis: a ground gate fires against
    a ground token of the same bytes. -/
theorem gate_fires_self_ground (bs : List Bool) :
    gate_fires (groundName bs) (groundName bs) = true := by
  unfold gate_fires
  simp

/-- S1, quote-axis mismatch (Rocq `fuel_gate_rejects_mismatched_token`,
    FuelGateSafety.v:277; via `N_tr_quote_injective` :240 / `hp_injective` :225):
    distinct quote byte strings produce DISTINCT gate/token names, so a quote
    fuel gate for `bs1` can never be funded by a quote token for `bs2 ≠ bs1`.

    NON-VACUOUS: the proof USES `quoteName` injectivity — a model that dropped
    the bytes (so all quote names were equal) would make this FALSE. -/
theorem fuel_gate_rejects_mismatched_token (bs1 bs2 : List Bool) :
    bs1 ≠ bs2 → quoteName bs1 ≠ quoteName bs2 := by
  intro hneq heq
  apply hneq
  injection heq

/-- S1, ground-axis mismatch (Rocq `fuel_gate_rejects_mismatched_token_ground`,
    FuelGateSafety.v:298; via `N_tr_ground_injective` :252 / `gp_injective` :228):
    distinct ground byte strings produce DISTINCT gate/token names. -/
theorem fuel_gate_rejects_mismatched_token_ground (bs1 bs2 : List Bool) :
    bs1 ≠ bs2 → groundName bs1 ≠ groundName bs2 := by
  intro hneq heq
  apply hneq
  injection heq

/-- S1, CROSS-AXIS mismatch (Rocq `fuel_gate_rejects_cross_axis_token`,
    FuelGateSafety.v:317; via `N_tr_ground_quote_distinct` :265 / `gp_hp_disjoint`
    :234): a ground fuel gate can NEVER be funded by a cryptographic-quote token,
    regardless of bytes — an attacker holding a ground key for one axis cannot
    synthesise fuel for the other axis. Stated to match the Rocq direction
    (ground gate vs. quote token): `groundName bs1 ≠ quoteName bs2`.

    NON-VACUOUS: the proof USES constructor disjointness — a model that
    identified the two axes (collapsed `quoteName`/`groundName` into one
    constructor) would make this FALSE. -/
theorem fuel_gate_rejects_cross_axis_token (bs1 bs2 : List Bool) :
    groundName bs1 ≠ quoteName bs2 := by
  intro heq
  exact ChanName.noConfusion heq

/-- The "funded only by matching name" property in its operational reading
    (combining `gate_fires_iff_names_eq` with the disjointness theorems):
    a quote gate for `bs1` does NOT fire against a quote token for `bs2 ≠ bs1`.
    This is the boolean form of `fuel_gate_no_top_comm_mismatched`
    (FuelGateSafety.v:333) restricted to the kernel name-match decision: the
    `rs_comm` verdict is `false`, i.e. NO fuel theft. -/
theorem fuel_gate_no_fire_mismatched (bs1 bs2 : List Bool) :
    bs1 ≠ bs2 → gate_fires (quoteName bs1) (quoteName bs2) = false := by
  intro hneq
  unfold gate_fires
  simp only [decide_eq_false_iff_not]
  exact fuel_gate_rejects_mismatched_token bs1 bs2 hneq

/-- Cross-axis form of `fuel_gate_no_fire_mismatched`: a ground gate never fires
    against a quote token (the boolean form of `fuel_gate_rejects_cross_axis_token`). -/
theorem fuel_gate_no_fire_cross_axis (bs1 bs2 : List Bool) :
    gate_fires (groundName bs1) (quoteName bs2) = false := by
  unfold gate_fires
  simp only [decide_eq_false_iff_not]
  exact fuel_gate_rejects_cross_axis_token bs1 bs2

end CostAccountedRho
