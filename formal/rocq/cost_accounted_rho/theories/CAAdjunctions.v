(* ════════════════════════════════════════════════════════════════════════
   CAAdjunctions.v — Adjunction I: Free ⊣ Forget for the Cost monad (CL5).

   continued-gslt-cost-v2's first adjunction: the cost-accounting apparatus is a
   DETACHABLE layer. Free installs it (the trivial unit grade — the unmetered
   embedding η), Forget strips it. The apparatus is "structure-preserving but
   behaviour-altering": Forget∘Free = id (the round-trip recovers the bare term),
   while Free∘Forget ALTERS (re-installing the trivial grade discards any
   accumulated grade). Both functors are natural. Realised structurally on the
   Cost (writer) monad of CostMonad.v. Axiom-free.

   Adjunction II (internalisation of Cost(G) into the Turing-complete base up to
   weak bisimulation, Prop. adj2) is delivered in CAInternalisation.v: the
   retraction Imp_G ∘ η_G ≈ id_G up to weak bisimulation. Its claim is the
   UNIT-grade (cost-free η_G) retraction — where the freely-available unit token
   fires the gate as an administrative reduction — NOT the full metered
   translation at arbitrary grades (whose strong bisimulation is force-limited,
   docs §3a; that is a separate, stronger statement the paper does not assert for
   Adjunction II). The operational faithfulness of the full metered translation
   is ca_translation_progresses (CATranslationFaithfulness).                    *)

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import SignatureMonoid.
From CostAccountedRho Require Import CostMonad.

(* Free: install the trivial cost apparatus (the unit grade). *)
Definition cost_install {X : Type} (x : X) : cost X := cost_eta x.

(* Forget: strip the cost apparatus, recovering the bare term. *)
Definition cost_forget {X : Type} (c : cost X) : X := fst c.

(* Forget ∘ Free = id — installing then stripping recovers the term. *)
Theorem cost_forget_install : forall {X} (x : X), cost_forget (cost_install x) = x.
Proof. intros X x. unfold cost_forget, cost_install, cost_eta. reflexivity. Qed.

(* Free is natural (commutes with the underlying function map). *)
Theorem cost_install_natural : forall {X Y} (f : X -> Y) (x : X),
  cost_map f (cost_install x) = cost_install (f x).
Proof. intros X Y f x. unfold cost_install, cost_map, cost_eta. reflexivity. Qed.

(* Forget is natural. *)
Theorem cost_forget_natural : forall {X Y} (f : X -> Y) (c : cost X),
  f (cost_forget c) = cost_forget (cost_map f c).
Proof. intros X Y f c. unfold cost_forget, cost_map. reflexivity. Qed.

(* Behaviour-altering: Free ∘ Forget ≠ id — re-installing after forgetting
   discards a non-trivial accumulated grade. *)
Theorem cost_install_forget_alters :
  exists (c : cost nat), cost_install (cost_forget c) <> c.
Proof.
  exists (0, (SGround nil, TUnit)).
  unfold cost_install, cost_forget, cost_eta, grade_unit. simpl.
  intro H. inversion H.
Qed.

(* The forgetful image of the unit IS the installed term (the unit of the
   adjunction splits): Forget (η x) = x, packaged as the round-trip identity. *)
Theorem cost_adjunction_unit_splits : forall {X} (x : X),
  cost_forget (cost_install x) = x /\ cost_install x = cost_eta x.
Proof. intros X x. split; [ apply cost_forget_install | reflexivity ]. Qed.
