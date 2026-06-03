(* ════════════════════════════════════════════════════════════════════════
   CAAdjunctionI.v — the DETACHABLE-LAYER section-retraction of the Cost apparatus
   (continued-gslt-cost-v2 §9, the "structurally … the apparatus is a detachable
   layer" remark, tex:1174-1184). Free installs the apparatus at the trivial unit
   grade (cost_install = η), Forget strips it; the round-trip Forget ∘ Free = id ON
   THE NOSE (cost_forget_install); both naturality squares hold (Leibniz); and
   Free ∘ Forget ≠ id (cost_install_forget_alters) — the behaviour-altering
   character. Every conjunct is an already-closed CAAdjunctions theorem. Axiom-free.

   SCOPE/ALIGNMENT NOTE. This proves Forget ∘ Free = id — the install/strip pair is a
   SECTION-RETRACTION. It is NOT, by itself, the free–forgetful RESOLUTION of the Cost
   monad of Prop adj1 (tex:1113-1116, "the induced monad Forget ∘ Free … is Cost"):
   that resolution requires Forget ∘ Free = Cost, the NON-IDEMPOTENT induced monad,
   which cannot equal id. The Cost-generating resolution is discharged separately by
   the KLEISLI adjunction CACostMonadInstances.cost_kleisli_adjunction, where
   Forget ∘ Free = cost_setoid = Cost on objects. The two pairs share the English
   names Free/Forget but are different functors between different categories (this one
   is object-trivial on the base; the Kleisli one lands in the resolving category) —
   do not read this id-retraction as generating Cost.                            *)

From CostAccountedRho Require Import CostMonad.
From CostAccountedRho Require Import CAAdjunctions.

(* The detachable-layer facts: Forget∘Free = id, naturality, behaviour-altering,
   install = η. (The Cost-GENERATING resolution is cost_kleisli_adjunction; see the
   header scope note.) *)
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
