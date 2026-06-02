(* ════════════════════════════════════════════════════════════════════════
   CAStructEquiv.v — Native structural equivalence (DR-21 Option B).

   The spec's §3.2/§3.4 structural equivalence (cost-accounted-rho.tex) makes
   parallel composition a commutative monoid at BOTH levels: processes
   `(P, |, 0)` and signed terms `(T, ∥, ())`. With the native four-sort grammar,
   these are stated NATIVELY (where the old [SystemStructEquiv] stated the
   signed-term monoid as a [system]-level relation): a 3-way mutually-inductive
   congruence over [caproc] (≡c), [caname] (≡cn), and [signed_term] (≡st). The
   signed-term identity `()` is the empty stack [STStack TUnit].

   This is the relation under which [ca_step] is closed (CAReduction's
   ca_struct rule) and the carrier of the categorical layer's ≡st commutative
   monoid. Axiom-free.                                                         *)

From Stdlib Require Import Setoid.
From Stdlib Require Import Morphisms.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.

Reserved Notation "P ≡c Q" (at level 70, no associativity).
Reserved Notation "x ≡cn y" (at level 70, no associativity).
Reserved Notation "T ≡st U" (at level 70, no associativity).

(* ── The 3-way mutually-inductive congruence ────────────────────────────── *)

Inductive ca_equiv : caproc -> caproc -> Prop :=
  | cse_refl  : forall P, P ≡c P
  | cse_sym   : forall P Q, P ≡c Q -> Q ≡c P
  | cse_trans : forall P Q R, P ≡c Q -> Q ≡c R -> P ≡c R
  | cse_par_comm  : forall P Q, CPPar P Q ≡c CPPar Q P
  | cse_par_assoc : forall P Q R, CPPar (CPPar P Q) R ≡c CPPar P (CPPar Q R)
  | cse_par_nil   : forall P, CPPar P CPNil ≡c P
  | cse_par_cong  : forall P P' Q Q', P ≡c P' -> Q ≡c Q' -> CPPar P Q ≡c CPPar P' Q'
  | cse_input_cong  : forall x x' T T',
      x ≡cn x' -> T ≡st T' -> CPInput x T ≡c CPInput x' T'
  | cse_output_cong : forall x x' U U',
      x ≡cn x' -> U ≡st U' -> CPOutput x U ≡c CPOutput x' U'
  | cse_deref_cong  : forall x x', x ≡cn x' -> CPDeref x ≡c CPDeref x'
where "P ≡c Q" := (ca_equiv P Q)
with caname_equiv : caname -> caname -> Prop :=
  | csne_quote : forall T T', T ≡st T' -> CQuote T ≡cn CQuote T'
  | csne_var   : forall k, CNVar k ≡cn CNVar k
where "x ≡cn y" := (caname_equiv x y)
with st_equiv : signed_term -> signed_term -> Prop :=
  | sse_refl  : forall T, T ≡st T
  | sse_sym   : forall T U, T ≡st U -> U ≡st T
  | sse_trans : forall T U V, T ≡st U -> U ≡st V -> T ≡st V
  | sse_par_comm  : forall T U, STPar T U ≡st STPar U T
  | sse_par_assoc : forall T U V, STPar (STPar T U) V ≡st STPar T (STPar U V)
  | sse_par_unit  : forall T, STPar T (STStack TUnit) ≡st T   (* identity () *)
  | sse_par_cong  : forall T T' U U', T ≡st T' -> U ≡st U' -> STPar T U ≡st STPar T' U'
  | sse_signed_cong : forall P P' s, P ≡c P' -> STSigned P s ≡st STSigned P' s
where "T ≡st U" := (st_equiv T U).

(* ── caname equivalence is reflexive / symmetric / transitive ───────────── *)

Lemma csne_refl : forall x, x ≡cn x.
Proof. destruct x as [T | k]; [ apply csne_quote, sse_refl | apply csne_var ]. Qed.

Lemma csne_sym : forall x y, x ≡cn y -> y ≡cn x.
Proof.
  intros x y H. inversion H; subst.
  - apply csne_quote, sse_sym. assumption.
  - apply csne_var.
Qed.

Lemma csne_trans : forall x y z, x ≡cn y -> y ≡cn z -> x ≡cn z.
Proof.
  intros x y z Hxy Hyz. inversion Hxy; subst; inversion Hyz; subst.
  - apply csne_quote. eapply sse_trans; eauto.
  - apply csne_var.
Qed.

(* ── monoid helper lemmas at both levels ────────────────────────────────── *)

Lemma cse_par_cong_l : forall P P' Q, P ≡c P' -> CPPar P Q ≡c CPPar P' Q.
Proof. intros. apply cse_par_cong; [assumption | apply cse_refl]. Qed.
Lemma cse_par_cong_r : forall P Q Q', Q ≡c Q' -> CPPar P Q ≡c CPPar P Q'.
Proof. intros. apply cse_par_cong; [apply cse_refl | assumption]. Qed.
Lemma cse_nil_par : forall P, CPPar CPNil P ≡c P.
Proof.
  intro. eapply cse_trans; [ apply cse_par_comm | apply cse_par_nil ].
Qed.

Lemma sse_par_cong_l : forall T T' U, T ≡st T' -> STPar T U ≡st STPar T' U.
Proof. intros. apply sse_par_cong; [assumption | apply sse_refl]. Qed.
Lemma sse_par_cong_r : forall T U U', U ≡st U' -> STPar T U ≡st STPar T U'.
Proof. intros. apply sse_par_cong; [apply sse_refl | assumption]. Qed.
Lemma sse_nil_par : forall T, STPar (STStack TUnit) T ≡st T.
Proof.
  intro. eapply sse_trans; [ apply sse_par_comm | apply sse_par_unit ].
Qed.

(* 4-element cross swap at the signed-term level (used by the split rules). *)
Lemma sse_par_cross : forall A B C D,
  STPar (STPar A B) (STPar C D) ≡st STPar (STPar A C) (STPar B D).
Proof.
  intros A B C D.
  eapply sse_trans; [ apply sse_par_assoc |].
  eapply sse_trans; [ apply sse_par_cong_r; apply sse_sym, sse_par_assoc |].
  eapply sse_trans; [ apply sse_par_cong_r; apply sse_par_cong_l; apply sse_par_comm |].
  eapply sse_trans; [ apply sse_par_cong_r; apply sse_par_assoc |].
  apply sse_sym, sse_par_assoc.
Qed.

(* Right-rotate triple at the signed-term level. *)
Lemma sse_par_rotr : forall A B C,
  STPar A (STPar B C) ≡st STPar B (STPar A C).
Proof.
  intros A B C.
  eapply sse_trans; [ apply sse_sym, sse_par_assoc |].
  eapply sse_trans; [ apply sse_par_cong_l; apply sse_par_comm |].
  apply sse_par_assoc.
Qed.

(* ── setoid registration ────────────────────────────────────────────────── *)

Add Parametric Relation : caproc ca_equiv
  reflexivity proved by cse_refl
  symmetry proved by cse_sym
  transitivity proved by cse_trans
  as ca_equiv_rel.

Add Parametric Relation : caname caname_equiv
  reflexivity proved by csne_refl
  symmetry proved by csne_sym
  transitivity proved by csne_trans
  as caname_equiv_rel.

Add Parametric Relation : signed_term st_equiv
  reflexivity proved by sse_refl
  symmetry proved by sse_sym
  transitivity proved by sse_trans
  as st_equiv_rel.

Add Parametric Morphism : CPPar with signature
  ca_equiv ==> ca_equiv ==> ca_equiv as CPPar_morphism.
Proof. intros. apply cse_par_cong; assumption. Qed.

Add Parametric Morphism : STPar with signature
  st_equiv ==> st_equiv ==> st_equiv as STPar_morphism.
Proof. intros. apply sse_par_cong; assumption. Qed.

Add Parametric Morphism (s : sig) : (fun P => STSigned P s) with signature
  ca_equiv ==> st_equiv as STSigned_morphism.
Proof. intros. apply sse_signed_cong; assumption. Qed.

Add Parametric Morphism : CPDeref with signature
  caname_equiv ==> ca_equiv as CPDeref_morphism.
Proof. intros. apply cse_deref_cong; assumption. Qed.

Add Parametric Morphism : CQuote with signature
  st_equiv ==> caname_equiv as CQuote_morphism.
Proof. intros. apply csne_quote; assumption. Qed.
