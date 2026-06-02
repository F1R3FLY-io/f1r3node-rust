/-
  CostAccountedRho.CostMonad — Lean 4 mirror of the Cost monad's law substrate
  from "Continued Interactive GSLTs and the Cost Endofunctor"
  (continued-gslt-cost-v2.tex), staged behind the Rocq SignatureMonoid (CL2) +
  ContinuedGSLTCapstone (Cost_Monad_Laws). The Lean leg of the multi-prover
  alignment (Stage 7).

  The monad paper's Prop "the cost monad" (:1064): the monad's unit/associativity
  laws "descend from the laws of the two constituent monoids" — the SIGNATURE
  commutative monoid (Sig, *, ()) up to the congruence ≡sig, and the temporal
  token-stack FREE monoid (List Sig, ++, []). This module mirrors both and the
  bundled law theorem, dependency-free (core `Init` only — no mathlib/batteries).

  SCOPE (DR-12, as the existing Lean mirrors): a minimal faithful model of the
  laws, not a port of the full Rocq grammar. Lean is a second independent witness
  staged behind the Qed-closed Rocq results, never the primary authority.
-/

namespace CostAccountedRho.CostMonad

/-- The core signature grammar (Def 3.3), mirroring the Rocq `sig`. -/
inductive Sig : Type
  | unit   : Sig
  | ground : List Bool → Sig
  | quote  : List Bool → Sig
  | and    : Sig → Sig → Sig
  deriving DecidableEq

open Sig

/-- The signature congruence ≡sig (SAnd is a free constructor, so the monoid laws
    hold up to this congruence — mirrors Rocq `sig_equiv`). -/
inductive SigEquiv : Sig → Sig → Prop
  | refl    : ∀ s, SigEquiv s s
  | symm    : ∀ s t, SigEquiv s t → SigEquiv t s
  | trans   : ∀ s t u, SigEquiv s t → SigEquiv t u → SigEquiv s u
  | andComm : ∀ s t, SigEquiv (and s t) (and t s)
  | andAssoc: ∀ s t u, SigEquiv (and (and s t) u) (and s (and t u))
  | andUnitL: ∀ s, SigEquiv (and unit s) s
  | andUnitR: ∀ s, SigEquiv (and s unit) s
  | andCong : ∀ s s' t t', SigEquiv s s' → SigEquiv t t' → SigEquiv (and s t) (and s' t')

/- ── the signature commutative monoid (Sig, *, ()) up to ≡sig ── -/

theorem sig_monoid_comm (s t : Sig) : SigEquiv (and s t) (and t s) :=
  SigEquiv.andComm s t

theorem sig_monoid_assoc (s t u : Sig) :
    SigEquiv (and (and s t) u) (and s (and t u)) :=
  SigEquiv.andAssoc s t u

theorem sig_monoid_unit_l (s : Sig) : SigEquiv (and unit s) s := SigEquiv.andUnitL s
theorem sig_monoid_unit_r (s : Sig) : SigEquiv (and s unit) s := SigEquiv.andUnitR s

/- ── the temporal token-stack FREE monoid (List Sig, ++, []) ── -/

def stackConcat (a b : List Sig) : List Sig := a ++ b

theorem stack_concat_assoc (a b c : List Sig) :
    stackConcat (stackConcat a b) c = stackConcat a (stackConcat b c) :=
  List.append_assoc a b c

theorem stack_concat_unit_l (a : List Sig) : stackConcat [] a = a := rfl

theorem stack_concat_unit_r (a : List Sig) : stackConcat a [] = a :=
  List.append_nil a

/-- The size (cell count) is a monoid homomorphism into (Nat, +, 0) — what makes
    the temporal grade (the consumed-stack modulus) additive. -/
theorem stack_size_concat (a b : List Sig) :
    (stackConcat a b).length = a.length + b.length := by
  unfold stackConcat; exact List.length_append

/-- The free monoid is NOT commutative (the temporal stack records order). -/
theorem stack_concat_not_commutative :
    ∃ a b : List Sig, stackConcat a b ≠ stackConcat b a :=
  ⟨[unit], [ground []], by decide⟩

/- ── the cost monad's laws descend from the two monoids ── -/

theorem cost_monad_laws :
    (∀ s t, SigEquiv (and s t) (and t s))
  ∧ (∀ s t u, SigEquiv (and (and s t) u) (and s (and t u)))
  ∧ (∀ s, SigEquiv (and unit s) s)
  ∧ (∀ s, SigEquiv (and s unit) s)
  ∧ (∀ a b c, stackConcat (stackConcat a b) c = stackConcat a (stackConcat b c))
  ∧ (∀ a, stackConcat [] a = a)
  ∧ (∀ a, stackConcat a [] = a) :=
  ⟨sig_monoid_comm, sig_monoid_assoc, sig_monoid_unit_l, sig_monoid_unit_r,
   stack_concat_assoc, stack_concat_unit_l, stack_concat_unit_r⟩

end CostAccountedRho.CostMonad
