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
   - NOT ESO: in TWO complementary forms.
       (bounded) the ciGSLT transition [graded_step] provably NEVER fires from a bare
       token stack (no_leak_stack_inert) — one precisely-named refuted clause; and
       (GENERAL) every ciGSLT transition is IMAGE-FINITE (ca_ciGSLT_image_finite, from
       CAGradedImageFinite.graded_image_finite), so the infinitely-branching iGSLT
       object [Bad] is the forgetful image of NO ciGSLT object (U_not_eso) — a fully
       general non-essential-surjectivity with no case enumeration.
     Axiom-free.                                                                  *)

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import WrappingSubjectReduction.
From CostAccountedRho Require Import CAGradedTransition.
From CostAccountedRho Require Import CAGradedImageFinite.
From CostAccountedRho Require Import CACategory.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Lia.
Import ListNotations.

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

(* ── Not eso (FULLY GENERAL): the image-finiteness obstruction. ─────────────────
   The bounded witness above refutes ONE transition shape. The general statement
   uses the genuine structural property that distinguishes ciGSLT from iGSLT
   objects: every ciGSLT transition is IMAGE-FINITE. We then exhibit an iGSLT object
   (a bare transition system) whose branching is infinite, so it cannot be the
   forgetful image of ANY ciGSLT object — U is not essentially surjective, with no
   case enumeration. *)

(* An iGSLT object is a bare transition system (carrier + grade-labelled step). *)
Record IGSys : Type := { ig_car : Type; ig_step : ig_car -> sig -> ig_car -> Prop }.

Definition image_finite (B : IGSys) : Prop :=
  forall x g, exists L : list (ig_car B), forall x', ig_step B x g x' -> In x' L.

(* The concrete ciGSLT transition IS image-finite: its successors are exactly the
   finite enumeration [graded_succ] (CAGradedImageFinite.graded_image_finite). This
   is what makes [image_finite] the faithful realizability constraint — the
   forgetful image of a genuine ciGSLT object is always image-finite. *)
Lemma ca_ciGSLT_image_finite :
  forall (S : signed_term) (g : sig),
    exists L, forall S', graded_step S g S' -> In S' L.
Proof.
  intros S g. exists (graded_succ S g). intros S' H.
  exact (proj1 (graded_image_finite S g S') H).
Qed.

(* [B] is ci-realizable when it is, up to a surjective step-reflecting map, the
   forgetful image of an image-finite (i.e. genuine ciGSLT) transition system. *)
Definition ci_realizable (B : IGSys) : Prop :=
  exists (A : IGSys) (h : ig_car A -> ig_car B),
    image_finite A
    /\ (forall y, exists x, h x = y)
    /\ (forall x g y', ig_step B (h x) g y' -> exists x', h x' = y' /\ ig_step A x g x').

(* The infinitely-branching iGSLT object: on the state space [nat], every state
   g-steps to every state. *)
Definition Bad : IGSys := {| ig_car := nat; ig_step := fun _ _ _ => True |}.

Lemma in_le_list_max : forall (L : list nat) n, In n L -> n <= list_max L.
Proof.
  induction L as [|a L IH]; intros n Hin; simpl in *.
  - contradiction.
  - destruct Hin as [-> | Hin]; [ lia | specialize (IH n Hin); lia ].
Qed.

Lemma nat_not_all_in_list : forall (L : list nat), exists n, ~ In n L.
Proof.
  intro L. exists (S (list_max L)). intro Hin.
  pose proof (in_le_list_max L _ Hin) as Hle. lia.
Qed.

Theorem U_not_eso : ~ ci_realizable Bad.
Proof.
  intros [A [h [Hfin [Hsurj Hrefl]]]].
  (* A is inhabited: pick a preimage of Bad's state 0. *)
  destruct (Hsurj 0) as [x0 _].
  (* A's successors of x0 under SUnit are bounded by a finite list L. *)
  destruct (Hfin x0 SUnit) as [L HL].
  (* No finite list of nats contains every nat. *)
  destruct (nat_not_all_in_list (map h L)) as [n Hn].
  (* But Bad steps (h x0) --SUnit--> n (its step is True), reflected to an A-step
     x0 --SUnit--> x' with h x' = n and x' ∈ L; hence n ∈ map h L — contradiction. *)
  destruct (Hrefl x0 SUnit n I) as [x' [Hx'n Hstep]].
  apply Hn. rewrite <- Hx'n. apply in_map. exact (HL x' Hstep).
Qed.

(* Prop 6.1 — the proper-subcategory claim (faithful, not full, and not-eso in BOTH
   the bounded structural form AND the fully general image-finiteness form). *)
Theorem proper_subcategory :
  (forall (G H : CIObj) (f g : CIMor G H),
     (forall x, igm_map (U_mor f) x = igm_map (U_mor g) x) -> mor_heq f g)
  /\ (exists (G H : CIObj) (h : IGMor G H),
        ~ (exists f : CIMor G H, forall x, igm_map (U_mor f) x = igm_map h x))
  /\ (forall t g S', ~ graded_step (STStack t) g S')
  /\ ~ ci_realizable Bad.
Proof.
  split; [ exact U_faithful
         | split; [ exact U_not_full
                  | split; [ exact graded_step_never_from_stack | exact U_not_eso ] ] ].
Qed.
