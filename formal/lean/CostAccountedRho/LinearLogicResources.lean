/-
  CostAccountedRho.LinearLogicResources — Lean 4 mirror of the validator-scoped
  S2 (funding) and S3 (linearity) obligations from the Rocq development
  `formal/rocq/cost_accounted_rho/theories/LinearLogicResources.v`
  (Workstream E, stage E2; DR-12).

  SCOPE. The Rocq `ll_formula` carries nine constructors; the spec's PURE
  multiplicative core (the Δ_s fragment of cost-accounted-rho §7.5–§7.7) is
  exactly the three multiplicative constructors `LLUnit | LLAtom | LLTensor`.
  The ILLE extension connectives (`LLThreshold`/`LLPlus`/`LLWith`/`LLBang`/
  `LLWhyNot`/`LLLolly`) are a documented out-of-spec extension that all carry
  `Δ_s = 0` (Rocq `delta_s`, lines 558-563), so they are OUT OF SCOPE for the
  Lean validator mirror. Mirroring only the 3 core constructors keeps every
  statement below logically equivalent to its Rocq counterpart on the core
  fragment, which is the entire domain the funding gate evaluates under the
  s₀ collapse (Rocq comment, lines 542-548).

  DEPENDENCY-FREE: core `Init` only (no mathlib/batteries). `List.Perm` and
  `List.Perm.length_eq` used by `ll_linear_no_contraction` are part of Lean
  core, so the faithful `¬ List.Perm …` form of the Rocq `~ Permutation …`
  is provable fully offline.
-/

namespace CostAccountedRho

/-- The pure multiplicative core of the Rocq `ll_formula` (Rocq:7-16).
    Only the three Δ_s-relevant constructors are mirrored; the ILLE extension
    connectives are out of scope (all carry Δ_s = 0) per the module header. -/
inductive LLFormula : Type
  | LLUnit : LLFormula
  | LLAtom : Nat → LLFormula
  | LLTensor : LLFormula → LLFormula → LLFormula

open LLFormula

/- ═══════════════════════════════════════════════════════════════════════════
   S2 — funding  (cost-accounted-rho §7.5; Rocq lines 553-699)
   ═══════════════════════════════════════════════════════════════════════════ -/

/-- The pure demand `Δ_s` (cost-accounted-rho Def 17), on the linear-logic
    image of a signature (Rocq `delta_s`, lines 553-564): `LLUnit ↦ 0`,
    `LLAtom _ ↦ 1`, `LLTensor f1 f2 ↦ delta_s f1 + delta_s f2`. -/
def delta_s : LLFormula → Nat
  | LLUnit => 0
  | LLAtom _ => 1
  | LLTensor f1 f2 => delta_s f1 + delta_s f2

/-- `Δ_s` is additive over the multiplicative tensor (cost-accounted-rho Def 17
    `Δ_s(T | U) = Δ_s(T) + Δ_s(U)`; Rocq `delta_s_tensor_additive`, line 571). -/
theorem delta_s_tensor_additive (f1 f2 : LLFormula) :
    delta_s (LLTensor f1 f2) = delta_s f1 + delta_s f2 := rfl

/-- The funding obligation as a predicate over supply balance `n` and demand
    `d`: holds iff supply meets or exceeds demand (cost-accounted-rho Def 19
    `Σ_s ≥ Δ_s`; Rocq `funds`, line 579 — `funds n d := d <= n`). -/
def funds (n d : Nat) : Prop := d ≤ n

/-- Decidability of the funding check (cost-accounted-rho Thm 20; Rocq
    `funding_decidable`, line 587). The Rocq statement is the sumbool
    `{funds n (delta_s f)} + {~ funds n (delta_s f)}`, which over a `Prop` IS
    `Decidable (funds n (delta_s f))`. Because `Decidable` lives in `Type` (it
    is data — the verdict witness), the faithful mirror is a `def` (not a
    `theorem`, which would require a `Prop` codomain); `#print axioms` reports
    its kernel-axiom footprint exactly as for a theorem. Provable since `≤` on
    `Nat` is decidable, so the validator can ALWAYS reach an accept/reject
    verdict by one integer comparison. -/
def funding_decidable (n : Nat) (f : LLFormula) :
    Decidable (funds n (delta_s f)) :=
  inferInstanceAs (Decidable (delta_s f ≤ n))

/-- A depth-`n` token stack of a single signature `s`, reflected to its
    linear-logic image as an `n`-fold tensor of the atom `a`, bottoming out at
    `LLUnit` (the empty stack `()`); Rocq `sig_stack`, lines 608-612. -/
def sig_stack (a : Nat) : Nat → LLFormula
  | 0 => LLUnit
  | n + 1 => LLTensor (LLAtom a) (sig_stack a n)

/-- `Σ_s` of a stack is just `Δ_s` of its linear-logic image — supply and
    demand use the SAME per-layer accounting (Rocq `sigma_s`, line 618). -/
def sigma_s (f : LLFormula) : Nat := delta_s f

/-- Decision-8 fidelity (cost-accounted-rho §7.5; Rocq
    `sigma_s_balance_eq_stack_count`, line 625): the balance `n` equals `Σ_s`
    of a depth-`n` `s`-stack, so storing supply as the integer balance loses
    no information relative to the paper's stack representation. -/
theorem sigma_s_balance_eq_stack_count (a n : Nat) :
    sigma_s (sig_stack a n) = n := by
  unfold sigma_s
  induction n with
  | zero => rfl
  | succ k ih => simp only [sig_stack, delta_s, ih]; omega

/-- The gate's boolean funding check over the balance, at the spec obligation
    (margin 0): accept iff `delta_s f ≤ n` (Rocq `is_funded_balance`, line 646,
    `Nat.leb (delta_s f) n`). `Nat.ble` is the core boolean `≤`; `Nat.ble_eq`
    gives a clean soundness proof. -/
def is_funded_balance (n : Nat) (f : LLFormula) : Bool :=
  Nat.ble (delta_s f) n

/-- Decision-8 soundness (cost-accounted-rho §7.5; Rocq
    `funding_check_balance_sound`, line 653): the boolean balance-read funding
    check is `true` iff the funding obligation `Σ_s ≥ Δ_s` holds — both
    directions, so the gate neither admits an under-funded deploy nor rejects a
    funded one. -/
theorem funding_check_balance_sound (n : Nat) (f : LLFormula) :
    is_funded_balance n f = true ↔ funds n (delta_s f) := by
  unfold is_funded_balance funds
  rw [Nat.ble_eq]

/-- Bridge corollary (cost-accounted-rho §7.5; Rocq
    `funding_check_balance_sound_against_stack`, line 667): the gate reading the
    balance `n` (= `Σ_s` of the depth-`n` stack) accepts the demand `delta_s f`
    iff that demand fits within the stack's supply. -/
theorem funding_check_balance_sound_against_stack (a n : Nat) (f : LLFormula) :
    is_funded_balance (sigma_s (sig_stack a n)) f = true ↔ funds n (delta_s f) := by
  rw [funding_check_balance_sound, sigma_s_balance_eq_stack_count]

/- ═══════════════════════════════════════════════════════════════════════════
   S3 — linearity  (cost-accounted-rho §7.6/§7.7; Rocq lines 78-134, 324-393)
   ═══════════════════════════════════════════════════════════════════════════ -/

/-- The atoms a formula consumes (Rocq `ll_consumed_atoms`, lines 78-90, on the
    core fragment): `LLUnit ↦ []`, `LLAtom a ↦ [a]`, `LLTensor f1 f2 ↦` the
    concatenation of the two sub-lists. -/
def ll_consumed_atoms : LLFormula → List Nat
  | LLUnit => []
  | LLAtom a => [a]
  | LLTensor f1 f2 => ll_consumed_atoms f1 ++ ll_consumed_atoms f2

/-- The linear context type (Rocq `linear_ctx := list ll_formula`, line 106). -/
abbrev LinearCtx := List LLFormula

/-- The flattened atom multiset of a linear context (Rocq `linear_ctx_atoms`,
    lines 109-110): `concat (map ll_consumed_atoms delta)`. `List.flatMap id ∘
    map` = `List.flatMap ll_consumed_atoms`. -/
def linear_ctx_atoms (delta : LinearCtx) : List Nat :=
  delta.flatMap ll_consumed_atoms

/-- The number of occurrences of atom `a` in a linear context (Rocq
    `linear_atom_count`, lines 112-113: `count_occ Nat.eq_dec …`). -/
def linear_atom_count (delta : LinearCtx) (a : Nat) : Nat :=
  (linear_ctx_atoms delta).count a

/-- Consume exactly ONE `LLAtom a` witness from a linear context if present
    (`some` remaining), else `none` (Rocq `consume_linear_atom`, lines 115-134).
    Mirrors the Rocq structural recursion exactly: at an `LLAtom a` head with
    `target = a`, return the tail; otherwise (non-matching atom, or any other
    head) recurse into the tail and re-prepend the head on success. A single
    matching atom witness is consumed. -/
def consume_linear_atom (target : Nat) : LinearCtx → Option LinearCtx
  | [] => none
  | h :: t =>
      match h with
      | LLAtom a =>
          if target = a then some t
          else
            match consume_linear_atom target t with
            | some t' => some (h :: t')
            | none => none
      | _ =>
          match consume_linear_atom target t with
          | some t' => some (h :: t')
          | none => none

/-- One linear consume exhausts a single-atom context (Rocq
    `ll_consume_linear_once_atom_exhausts`, line 357):
    `consume_linear_atom a [LLAtom a] = some []`. -/
theorem ll_consume_linear_once_atom_exhausts (a : Nat) :
    consume_linear_atom a [LLAtom a] = some [] := by
  simp [consume_linear_atom]

/-- NO double-spend from a single witness (cost-accounted-rho §7.6 Remark 21;
    Rocq `ll_no_double_spend_single_witness`, line 367): after consuming the one
    `LLAtom a`, a SECOND consume on the remainder fails. A single linear atom
    cannot be consumed twice. -/
theorem ll_no_double_spend_single_witness (a : Nat) :
    (match consume_linear_atom a [LLAtom a] with
      | some delta => consume_linear_atom a delta
      | none => none) = none := by
  rw [ll_consume_linear_once_atom_exhausts]; rfl

/-- Double-spend REQUIRES a duplicate witness (cost-accounted-rho §7.7; Rocq
    `ll_double_spend_requires_duplicate_witness`, line 379): with TWO `LLAtom a`
    witnesses, both consumes succeed, leaving `[]`. -/
theorem ll_double_spend_requires_duplicate_witness (a : Nat) :
    (match consume_linear_atom a [LLAtom a, LLAtom a] with
      | some delta => consume_linear_atom a delta
      | none => none) = some [] := by
  simp [consume_linear_atom]

/-- NO contraction in the linear zone (cost-accounted-rho §7.6; Rocq
    `ll_linear_no_contraction`, lines 324-333): the atom multiset of a single
    `LLAtom a` is NOT a permutation of that of `LLTensor (LLAtom a) (LLAtom a)`
    — one occurrence cannot be silently duplicated into two.

    FORM: the Rocq statement is `~ Permutation (…) (…)`. `List.Perm` and
    `List.Perm.length_eq` are available in Lean core offline, so this is the
    FAITHFUL `¬ List.Perm` mirror (not the weaker length-disequality fallback):
    a permutation preserves length (`length_eq`), but the two atom multisets
    have lengths 1 and 2, so no permutation exists. The `1 ≠ 2` is the
    load-bearing content, discharged by `omega` after `length_eq`. -/
theorem ll_linear_no_contraction (a : Nat) :
    ¬ List.Perm
        (linear_ctx_atoms [LLAtom a])
        (linear_ctx_atoms [LLTensor (LLAtom a) (LLAtom a)]) := by
  intro hperm
  have hlen := hperm.length_eq
  simp [linear_ctx_atoms, ll_consumed_atoms] at hlen

end CostAccountedRho
