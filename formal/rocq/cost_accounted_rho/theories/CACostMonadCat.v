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
