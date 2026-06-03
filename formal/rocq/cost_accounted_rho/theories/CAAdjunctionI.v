(* ════════════════════════════════════════════════════════════════════════
   CAAdjunctionI.v — Prop 9.2 (Free ⊣ Forget), continued-gslt-cost-v2 §9. The
   structural-and-strict resolution generating the Cost monad: Forget ∘ Free = id
   on the nose (cost_forget_install), both naturality squares hold (Leibniz),
   and Free ∘ Forget ≠ id (cost_install_forget_alters) — the behaviour-altering
   character that makes the resolution non-trivial. The induced monad unit is
   Cost's own η (cost_install = cost_eta, definitionally). Every conjunct is an
   already-closed CAAdjunctions theorem. Axiom-free.                            *)

From CostAccountedRho Require Import CostMonad.
From CostAccountedRho Require Import CAAdjunctions.

(* Prop 9.2 — the free/forget adjunction data + the resolution's defining facts. *)
Theorem free_forget_adjunction :
  (forall (X : Type) (x : X), cost_forget (cost_install x) = x)
  /\ (forall (X Y : Type) (f : X -> Y) (x : X), cost_map f (cost_install x) = cost_install (f x))
  /\ (forall (X Y : Type) (f : X -> Y) (c : cost X), f (cost_forget c) = cost_forget (cost_map f c))
  /\ (exists (c : cost nat), cost_install (cost_forget c) <> c)
  /\ (forall (X : Type) (x : X), cost_install x = cost_eta x).
Proof.
  split; [ exact @cost_forget_install |
  split; [ exact @cost_install_natural |
  split; [ exact @cost_forget_natural |
  split; [ exact cost_install_forget_alters | intros; reflexivity ] ] ] ].
Qed.
