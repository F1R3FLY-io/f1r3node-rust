(* ════════════════════════════════════════════════════════════════════════
   CACategory.v — the concrete ciGSLT category, reified skeletally-but-honestly
   (continued-gslt-cost-v2 §6). A [CIObj] carries exactly the structure the
   abstract §6-§9 proofs touch: a state carrier, an intra-carrier graded
   transition [cstep], and a behavioural equivalence [cbisim] (with carried
   reflexivity/symmetry/transitivity). Morphisms are transition-preserving,
   equivalence-respecting carrier maps; the hom-equality [mor_heq f g := ∀x,
   cbisim (f x)(g x)] is POINTWISE behavioural equality, so the hom-setoid needs
   no functional extensionality. The concrete rho object [Rho_ciGSLT] (carrier
   [signed_term], transition [graded_step], equivalence [graded_bisim]) is the
   non-vacuity witness and is built from hypothesis-free symbols, so this module's
   results are closed under the global context. The full §6 object
   (K,Kp,Ke,K',compute,cf) is a scope boundary: only the fragment the abstract
   proofs consume is reified. Axiom-free.                                        *)

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CAGradedTransition.
From CostAccountedRho Require Import CAGradedAdequacy.
From CostAccountedRho Require Import CategoryInterface.

(* ── R-A: graded_bisim is transitive (CAGradedAdequacy supplies refl/sym only) — *)
Lemma graded_bisim_trans : forall S T U,
  graded_bisim S T -> graded_bisim T U -> graded_bisim S U.
Proof.
  cofix CH. intros S T U HST HTU.
  destruct HST as [Sa Ta HSTf HSTb].
  destruct HTU as [Tb Ub HTUf HTUb].
  apply gbisim_intro.
  - intros g S' Hstep.
    destruct (HSTf g S' Hstep) as [T' [HstepT HbST']].
    destruct (HTUf g T' HstepT) as [U' [HstepU HbTU']].
    exists U'. split; [ exact HstepU | exact (CH _ _ _ HbST' HbTU') ].
  - intros g U' Hstep.
    destruct (HTUb g U' Hstep) as [T' [HstepT HbT'U']].
    destruct (HSTb g T' HstepT) as [S' [HstepS HbS'T']].
    exists S'. split; [ exact HstepS | exact (CH _ _ _ HbS'T' HbT'U') ].
Qed.

(* ── A skeletal ciGSLT object: only the structure the abstract proofs touch. ── *)
Record CIObj : Type := {
  carrier : Type;
  cstep   : carrier -> sig -> carrier -> Prop;
  cbisim  : carrier -> carrier -> Prop;
  cbisim_refl  : forall x, cbisim x x;
  cbisim_sym   : forall x y, cbisim x y -> cbisim y x;
  cbisim_trans : forall x y z, cbisim x y -> cbisim y z -> cbisim x z
}.

(* Morphisms: transition-preserving (a forward simulation on the interacting
   sort) and equivalence-respecting carrier maps. [mor_cong] is what makes
   composition a congruence for the hom-setoid (R-B). *)
Record CIMor (G H : CIObj) : Type := {
  mor_map  : carrier G -> carrier H;
  mor_pres : forall x g x', cstep G x g x' -> cstep H (mor_map x) g (mor_map x');
  mor_cong : forall x y, cbisim G x y -> cbisim H (mor_map x) (mor_map y)
}.

Arguments mor_map {G H} _ _.
Arguments mor_pres {G H} _ {x g x'} _.
Arguments mor_cong {G H} _ {x y} _.

Definition mor_heq {G H} (f g : CIMor G H) : Prop :=
  forall x, cbisim H (mor_map f x) (mor_map g x).

Definition CIMor_id (G : CIObj) : CIMor G G :=
  {| mor_map := fun x => x;
     mor_pres := fun x g x' H => H;
     mor_cong := fun x y H => H |}.

Definition CIMor_comp {G H K} (f : CIMor H K) (g : CIMor G H) : CIMor G K :=
  {| mor_map := fun x => mor_map f (mor_map g x);
     mor_pres := fun x gr x' Hs => mor_pres f (mor_pres g Hs);
     mor_cong := fun x y Hb => mor_cong f (mor_cong g Hb) |}.

(* ── The ciGSLT category as a CategoryInterface.Category. ──────────────────── *)
Definition CICat : Category.
Proof.
  refine {| Obj := CIObj; Hom := CIMor; heq := @mor_heq;
            cid := CIMor_id; ccomp := @CIMor_comp |}.
  - intros G H f x. apply cbisim_refl.
  - intros G H f g Hfg x. apply cbisim_sym. apply Hfg.
  - intros G H f g h Hfg Hgh x. eapply cbisim_trans; [ apply Hfg | apply Hgh ].
  - intros G H f x. apply cbisim_refl.
  - intros G H f x. apply cbisim_refl.
  - intros G H K L h g f x. apply cbisim_refl.
  - (* ccomp_proper: cbisim K (g(f x))(g'(f' x)), from mor_heq g g', mor_heq f f' *)
    intros G H K g g' f f' Hg Hf x.
    apply (cbisim_trans K (mor_map g (mor_map f x))
                          (mor_map g (mor_map f' x))
                          (mor_map g' (mor_map f' x))).
    + apply (mor_cong g). apply Hf.        (* cbisim K (g(f x))(g(f' x)) — pushed by g *)
    + apply Hg.                            (* cbisim K (g(f' x))(g'(f' x)) *)
Defined.

(* ── The concrete rho object: the non-vacuity witness (hypothesis-free). ────── *)
Definition Rho_ciGSLT : CIObj :=
  {| carrier := signed_term;
     cstep   := graded_step;
     cbisim  := graded_bisim;
     cbisim_refl  := graded_bisim_refl;
     cbisim_sym   := graded_bisim_sym;
     cbisim_trans := graded_bisim_trans |}.

(* The reified category is non-vacuous: the rho object has a real graded step
   (a Rule-1 redex on the unit signature). *)
Theorem rho_object_nonvacuous :
  exists (G : Obj CICat) (x : carrier G) (gr : sig) (y : carrier G), cstep G x gr y.
Proof.
  exists Rho_ciGSLT.
  exists (STPar (STSigned (CPPar (CPInput (CNVar 0) (STStack TUnit))
                                 (CPOutput (CNVar 0) (STStack TUnit))) SUnit)
                (STStack (TGate SUnit TUnit))).
  exists SUnit.
  eexists. simpl. apply g_rule1.
Qed.
