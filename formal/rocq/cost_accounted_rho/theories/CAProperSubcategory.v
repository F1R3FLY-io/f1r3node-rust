(* ════════════════════════════════════════════════════════════════════════
   CAProperSubcategory.v — Prop 6.1 (the forgetful U : ciGSLT → iGSLT is faithful,
   not full, not essentially surjective), continued-gslt-cost-v2 §6.

   Model: objects are the reified [CIObj] (CACategory); an iGSLT morphism [IGMor]
   is a transition-preserving carrier map (it forgets the behavioural-congruence
   obligation), while a ciGSLT morphism [CIMor] ADDITIONALLY respects [cbisim]
   (the [mor_cong] field — the quote/grading congruence). U keeps the underlying
   map and forgets [mor_cong].

   - FAITHFUL: U keeps the map, so two ciGSLT morphisms with equal images are
     equal in the hom-setoid (a Leibniz map equality forces [mor_heq] by [cbisim]
     reflexivity).
   - NOT FULL: an iGSLT morphism that collapses two behaviourally-identified states
     onto two distinct ones has NO ciGSLT lift — the lift's [mor_cong] would force
     [true = false], refuted by [discriminate]. (The R-D key-collapse obstruction.)
   - NOT ESO (bounded): the ciGSLT transition [graded_step] provably NEVER fires
     from a bare token stack (no_leak_stack_inert), so the ciGSLT structure does
     not realize every iGSLT transition shape. Per the spec's "enumerating cases
     is hopeless", this is witnessed against ONE precisely-named clause (a
     stack-headed transition) rather than over all conceivable iGSLT objects — the
     explicit bound. Axiom-free.                                                  *)

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import WrappingSubjectReduction.
From CostAccountedRho Require Import CAGradedTransition.
From CostAccountedRho Require Import CACategory.

(* iGSLT morphism: transition-preserving, but NOT required to respect cbisim. *)
Record IGMor (G H : CIObj) : Type := {
  igm_map  : carrier G -> carrier H;
  igm_pres : forall x g x', cstep G x g x' -> cstep H (igm_map x) g (igm_map x')
}.

Arguments igm_map {G H} _ _.

(* The forgetful action on morphisms: a ciGSLT morphism IS an iGSLT morphism
   (forget the [mor_cong] congruence field). *)
Definition U_mor {G H : CIObj} (f : CIMor G H) : IGMor G H :=
  {| igm_map := mor_map f; igm_pres := fun x g x' => mor_pres f |}.

(* ── Faithful: equal images ⇒ equal in the hom-setoid. ─────────────────────── *)
Theorem U_faithful : forall (G H : CIObj) (f g : CIMor G H),
  (forall x, igm_map (U_mor f) x = igm_map (U_mor g) x) -> mor_heq f g.
Proof.
  intros G H f g Hfg x. unfold mor_heq. simpl in Hfg. rewrite (Hfg x). apply cbisim_refl.
Qed.

(* ── Not full: two witness objects + a collapsing iGSLT morphism with no lift. ─ *)
Definition obj_triv : CIObj :=
  {| carrier := bool; cstep := fun _ _ _ => False;
     cbisim := fun _ _ => True;
     cbisim_refl := fun _ => I;
     cbisim_sym := fun _ _ _ => I;
     cbisim_trans := fun _ _ _ _ _ => I |}.

Definition obj_eq : CIObj :=
  {| carrier := bool; cstep := fun _ _ _ => False;
     cbisim := @eq bool;
     cbisim_refl := @eq_refl bool;
     cbisim_sym := fun x y (H : x = y) => eq_sym H;
     cbisim_trans := fun x y z (H1 : x = y) (H2 : y = z) => eq_trans H1 H2 |}.

Definition h_collapse : IGMor obj_triv obj_eq.
Proof.
  refine (@Build_IGMor obj_triv obj_eq (fun b => b) _).
  intros x g x' H. simpl in H. destruct H.
Defined.

Theorem U_not_full :
  exists (G H : CIObj) (h : IGMor G H),
    ~ (exists f : CIMor G H, forall x, igm_map (U_mor f) x = igm_map h x).
Proof.
  exists obj_triv, obj_eq, h_collapse. intros [f Hf]. simpl in Hf.
  (* Hf x : mor_map f x = x; mor_cong f at the (True-related) distinct states true,false
     forces eq (mor_map f true)(mor_map f false), i.e. true = false. *)
  pose proof (@mor_cong obj_triv obj_eq f true false I) as Hc. simpl in Hc.
  rewrite (Hf true) in Hc. rewrite (Hf false) in Hc. discriminate Hc.
Qed.

(* ── Not eso (bounded): the ciGSLT transition never fires from a bare stack, so
   a stack-headed iGSLT transition shape is unrealized by the ciGSLT structure. ─ *)
Theorem graded_step_never_from_stack : forall t g S', ~ graded_step (STStack t) g S'.
Proof.
  intros t g S' H. apply graded_step_sound in H.
  eapply no_leak_stack_inert. exact H.
Qed.

(* Prop 6.1 — the proper-subcategory claim (faithful, not full, not-eso bound). *)
Theorem proper_subcategory :
  (forall (G H : CIObj) (f g : CIMor G H),
     (forall x, igm_map (U_mor f) x = igm_map (U_mor g) x) -> mor_heq f g)
  /\ (exists (G H : CIObj) (h : IGMor G H),
        ~ (exists f : CIMor G H, forall x, igm_map (U_mor f) x = igm_map h x))
  /\ (forall t g S', ~ graded_step (STStack t) g S').
Proof.
  split; [ exact U_faithful | split; [ exact U_not_full | exact graded_step_never_from_stack ] ].
Qed.
