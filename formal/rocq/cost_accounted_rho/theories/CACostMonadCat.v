(* ════════════════════════════════════════════════════════════════════════
   CACostMonadCat.v — Prop 9.1 (Cost is a monad), continued-gslt-cost-v2 §9. The
   monad data (η = cost_eta, μ = cost_mu) and the three monad laws, the η/μ
   naturality squares, and the non-idempotence witness that distinguishes Cost
   from a closure monad — every conjunct an already-closed CostMonad theorem.
   Unlike the functor laws (CACostFunctor, which hold on the nose), the monad
   unit/associativity laws hold only up to the grade equivalence cost_equiv
   (the unit grade absorbs by grade_op_unit_r, NOT Leibniz), so the headline is
   the conjunction of cost_equiv facts the spec states. Axiom-free.             *)

From CostAccountedRho Require Import CostMonad.

(* Prop 9.1 — the monad laws, naturality, and non-idempotence. *)
Theorem cost_is_monad :
  (forall (X : Type) (m : cost X), cost_equiv (cost_mu (cost_eta m)) m)
  /\ (forall (X : Type) (m : cost X), cost_equiv (cost_mu (cost_map cost_eta m)) m)
  /\ (forall (X : Type) (c : cost (cost (cost X))),
        cost_equiv (cost_mu (cost_mu c)) (cost_mu (cost_map cost_mu c)))
  /\ (forall (X Y : Type) (f : X -> Y) (x : X),
        cost_equiv (cost_map f (cost_eta x)) (cost_eta (f x)))
  /\ (forall (X Y : Type) (f : X -> Y) (c : cost (cost X)),
        cost_equiv (cost_map f (cost_mu c)) (cost_mu (cost_map (cost_map f) c)))
  /\ (exists (c : cost (cost nat)), ~ cost_equiv (cost_mu c) (fst c)).
Proof.
  split; [ exact @cost_left_unit |
  split; [ exact @cost_right_unit |
  split; [ exact @cost_assoc |
  split; [ exact @cost_eta_natural |
  split; [ exact @cost_mu_natural | exact cost_monad_not_idempotent ] ] ] ] ].
Qed.

(* ════════════════════════════════════════════════════════════════════════
   CA-P-119 — Eilenberg–Moore algebras of the cost monad (continued-gslt-cost-v2
   §9.1 EM gesture: "a calculus that knows how to PAY" — a coherent discharge
   α : 𝔠(A) → A interpreting the consumed tokens). An EM-algebra is a carrier A
   with a discharge α respecting the two algebra laws (up to a carrier
   equivalence): α ∘ η = id  and  α ∘ μ = α ∘ 𝔠α. We exhibit two payers:
     • the GROUND payer (X, fst): discharge α = fst pays off the grade and keeps
       the bare value — the laws hold ON THE NOSE (Leibniz), so it is the maximal
       "pay everything" algebra;
     • the FREE algebra (𝔠X, μ): the canonical algebra whose two laws ARE
       cost_left_unit / cost_assoc.
   This discharges the spec's one-sentence EM-algebra claim with a concrete,
   inhabited structure. Axiom-free. *)

Record CostEMAlgebra : Type := {
  em_carrier : Type;
  em_eq      : em_carrier -> em_carrier -> Prop;
  em_alpha   : cost em_carrier -> em_carrier;
  (* α ∘ η = id : discharging a trivially-metered value returns it. *)
  em_unit    : forall a : em_carrier, em_eq (em_alpha (cost_eta a)) a;
  (* α ∘ μ = α ∘ 𝔠α : discharging a flattened double-meter = discharging the
     inner meters then the outer (the "pay coherently" square). *)
  em_mult    : forall c : cost (cost em_carrier),
                 em_eq (em_alpha (cost_mu c)) (em_alpha (cost_map em_alpha c))
}.

(* The ground payer: α = fst discharges the whole grade, returning the value.
   Both EM laws are definitional (the projection ignores the accumulated grade). *)
Definition CostGroundPayer (X : Type) : CostEMAlgebra :=
  {| em_carrier := X;
     em_eq      := @eq X;
     em_alpha   := @fst X grade;
     em_unit    := fun a => eq_refl;
     em_mult    := fun c => eq_refl |}.

(* The free (canonical) algebra: α = μ; its two EM laws are exactly the monad's
   left-unit and associativity, up to cost_equiv. *)
Definition CostFreeAlgebra (X : Type) : CostEMAlgebra :=
  {| em_carrier := cost X;
     em_eq      := @cost_equiv X;
     em_alpha   := @cost_mu X;
     em_unit    := @cost_left_unit X;
     em_mult    := @cost_assoc X |}.

(* CA-P-119 headline: a payer object exists and its discharge is coherent with
   η and μ (the two EM-algebra laws hold for the witness). *)
Theorem cost_payer_discharges_coherently :
  exists A : CostEMAlgebra,
    (forall a : em_carrier A, em_eq A (em_alpha A (cost_eta a)) a)
    /\ (forall c : cost (cost (em_carrier A)),
          em_eq A (em_alpha A (cost_mu c)) (em_alpha A (cost_map (em_alpha A) c))).
Proof.
  exists (CostGroundPayer nat); split.
  - apply em_unit.
  - apply em_mult.
Qed.
