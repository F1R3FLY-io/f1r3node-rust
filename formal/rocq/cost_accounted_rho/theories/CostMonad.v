(* ════════════════════════════════════════════════════════════════════════
   CostMonad.v — the Cost endofunctor 𝔠 and its monad structure (CL3 + CL4).

   continued-gslt-cost-v2.tex's central thesis: cost accounting is an ENDOFUNCTOR
   𝔠 (indeed a MONAD) whose laws "descend from the laws of the two constituent
   monoids" (Prop "the cost monad", :1064-1071) — the signature commutative
   monoid (Sig,*,()) and the temporal token-stack free monoid (token,++,()).

   We realise 𝔠 as the WRITER monad over the PRODUCT of those two monoids: the
   cost GRADE is `(sig * token)` — the authority consumed (commutative, up to
   ≡sig) paired with the temporal stack (free, Leibniz, the modulus). This is the
   monad's essential structure: η meters trivially (the unit grade), μ FLATTENS
   two metering layers by COMBINING their grades (SAnd on signatures, tok_concat
   on the stack). The three monad laws + functoriality + the naturality of η,μ
   all reduce to the two monoids' laws (SignatureMonoid.v) — pointwise on the
   carrier, so NO functional extensionality is needed, and the nested-wrapping
   (𝔠²) flatten that `SSigned : proc → …` could not even state is here just grade
   multiplication. The monad is NON-idempotent: μ strictly accumulates the
   modulus (metering twice ≠ metering once). Axiom-free.                       *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import SignatureMonoid.

(* ── The cost grade: the product of the two monoids ─────────────────────── *)

Definition grade : Type := (sig * token)%type.
Definition grade_unit : grade := (SUnit, TUnit).
Definition grade_op (g1 g2 : grade) : grade :=
  (SAnd (fst g1) (fst g2), tok_concat (snd g1) (snd g2)).

(* Grade equivalence: ≡sig on the (commutative) authority, Leibniz on the (free)
   temporal stack — the product of the two monoids' equalities. *)
Definition grade_equiv (g1 g2 : grade) : Prop :=
  sig_equiv (fst g1) (fst g2) /\ snd g1 = snd g2.

Lemma grade_equiv_refl : forall g, grade_equiv g g.
Proof. intros [s t]; split; reflexivity. Qed.

Lemma grade_equiv_sym : forall g1 g2, grade_equiv g1 g2 -> grade_equiv g2 g1.
Proof. intros [s1 t1] [s2 t2] [Hs Ht]; split; [ symmetry; assumption | symmetry; assumption ]. Qed.

Lemma grade_equiv_trans : forall g1 g2 g3,
  grade_equiv g1 g2 -> grade_equiv g2 g3 -> grade_equiv g1 g3.
Proof.
  intros [s1 t1] [s2 t2] [s3 t3] [Hs12 Ht12] [Hs23 Ht23]; split.
  - eapply sige_trans; eassumption.
  - rewrite Ht12; assumption.
Qed.

(* The grade is a monoid up to grade_equiv — the two monoids' laws, componentwise. *)
Lemma grade_op_unit_l : forall g, grade_equiv (grade_op grade_unit g) g.
Proof. intros [s t]; unfold grade_op, grade_unit, grade_equiv; simpl; split; [ apply sig_monoid_unit_l | reflexivity ]. Qed.

Lemma grade_op_unit_r : forall g, grade_equiv (grade_op g grade_unit) g.
Proof. intros [s t]; unfold grade_op, grade_unit, grade_equiv; simpl; split; [ apply sig_monoid_unit_r | apply tok_concat_unit_r ]. Qed.

Lemma grade_op_assoc : forall g1 g2 g3,
  grade_equiv (grade_op (grade_op g1 g2) g3) (grade_op g1 (grade_op g2 g3)).
Proof. intros [s1 t1] [s2 t2] [s3 t3]; unfold grade_op, grade_equiv; simpl; split; [ apply sig_monoid_assoc | apply tok_concat_assoc ]. Qed.

Lemma grade_op_cong : forall g1 g1' g2 g2',
  grade_equiv g1 g1' -> grade_equiv g2 g2' -> grade_equiv (grade_op g1 g2) (grade_op g1' g2').
Proof.
  intros [s1 t1] [s1' t1'] [s2 t2] [s2' t2'] [Hs1 Ht1] [Hs2 Ht2];
  unfold grade_op, grade_equiv; simpl in *; split.
  - apply sige_and_cong; assumption.
  - rewrite Ht1, Ht2; reflexivity.
Qed.

(* ── The Cost endofunctor 𝔠 = (· × grade) (CL3) ─────────────────────────── *)

Definition cost (X : Type) : Type := (X * grade)%type.

Definition cost_map {X Y : Type} (f : X -> Y) (c : cost X) : cost Y :=
  (f (fst c), snd c).

Definition cost_equiv {X : Type} (c1 c2 : cost X) : Prop :=
  fst c1 = fst c2 /\ grade_equiv (snd c1) (snd c2).

Lemma cost_equiv_refl : forall {X} (c : cost X), cost_equiv c c.
Proof. intros X [x g]; split; [ reflexivity | apply grade_equiv_refl ]. Qed.

Lemma cost_equiv_sym : forall {X} (c1 c2 : cost X), cost_equiv c1 c2 -> cost_equiv c2 c1.
Proof. intros X [x1 g1] [x2 g2] [Hx Hg]; split; [ symmetry; assumption | apply grade_equiv_sym; assumption ]. Qed.

Lemma cost_equiv_trans : forall {X} (c1 c2 c3 : cost X),
  cost_equiv c1 c2 -> cost_equiv c2 c3 -> cost_equiv c1 c3.
Proof.
  intros X [x1 g1] [x2 g2] [x3 g3] [Hx12 Hg12] [Hx23 Hg23]; split.
  - rewrite Hx12; assumption.
  - eapply grade_equiv_trans; eassumption.
Qed.

(* Functor law 1: 𝔠 preserves identities. *)
Theorem cost_map_id : forall {X} (c : cost X), cost_equiv (cost_map (fun x => x) c) c.
Proof. intros X [x g]; unfold cost_map, cost_equiv; simpl; split; [ reflexivity | apply grade_equiv_refl ]. Qed.

(* Functor law 2: 𝔠 preserves composition. *)
Theorem cost_map_compose : forall {X Y Z} (f : X -> Y) (g : Y -> Z) (c : cost X),
  cost_equiv (cost_map (fun x => g (f x)) c) (cost_map g (cost_map f c)).
Proof. intros X Y Z f g [x gr]; unfold cost_map, cost_equiv; simpl; split; [ reflexivity | apply grade_equiv_refl ]. Qed.

(* ── The monad: unit η and multiplication μ (CL4) ───────────────────────── *)

Definition cost_eta {X : Type} (x : X) : cost X := (x, grade_unit).

(* μ flattens 𝔠² ⇒ 𝔠 by COMBINING the inner and outer grades (the nested-meter
   flatten that the old bare-proc SSigned could not even type). *)
Definition cost_mu {X : Type} (c : cost (cost X)) : cost X :=
  (fst (fst c), grade_op (snd (fst c)) (snd c)).

(* η is natural. *)
Theorem cost_eta_natural : forall {X Y} (f : X -> Y) (x : X),
  cost_equiv (cost_map f (cost_eta x)) (cost_eta (f x)).
Proof. intros X Y f x; unfold cost_map, cost_eta, cost_equiv; simpl; split; [ reflexivity | apply grade_equiv_refl ]. Qed.

(* μ is natural. *)
Theorem cost_mu_natural : forall {X Y} (f : X -> Y) (c : cost (cost X)),
  cost_equiv (cost_map f (cost_mu c)) (cost_mu (cost_map (cost_map f) c)).
Proof. intros X Y f [[x gi] go]; unfold cost_map, cost_mu, cost_equiv; simpl; split; [ reflexivity | apply grade_equiv_refl ]. Qed.

(* Monad left unit:  μ ∘ η_{𝔠X} = id  — reduces to the grade's right unit. *)
Theorem cost_left_unit : forall {X} (m : cost X), cost_equiv (cost_mu (cost_eta m)) m.
Proof. intros X [x g]; unfold cost_mu, cost_eta, cost_equiv; simpl; split; [ reflexivity | apply grade_op_unit_r ]. Qed.

(* Monad right unit:  μ ∘ 𝔠(η) = id  — reduces to the grade's left unit. *)
Theorem cost_right_unit : forall {X} (m : cost X), cost_equiv (cost_mu (cost_map cost_eta m)) m.
Proof. intros X [x g]; unfold cost_mu, cost_map, cost_eta, cost_equiv; simpl; split; [ reflexivity | apply grade_op_unit_l ]. Qed.

(* Monad associativity:  μ ∘ μ_𝔠 = μ ∘ 𝔠(μ)  — reduces to grade associativity. *)
Theorem cost_assoc : forall {X} (c : cost (cost (cost X))),
  cost_equiv (cost_mu (cost_mu c)) (cost_mu (cost_map cost_mu c)).
Proof.
  intros X [[[x gi] gm] go]; unfold cost_mu, cost_map, cost_equiv; simpl; split.
  - reflexivity.
  - apply grade_equiv_sym; apply grade_op_assoc.
Qed.

(* ── Non-idempotence: the cost monad charges CUMULATIVELY ───────────────── *)

(* μ adds the temporal moduli (the consumed-stack lengths) — token_size is the
   monoid homomorphism into (nat,+,0). This is why metering is non-idempotent. *)
Theorem cost_mu_modulus_accumulates : forall {X} (c : cost (cost X)),
  token_size (snd (snd (cost_mu c))) =
    token_size (snd (snd (fst c))) + token_size (snd (snd c)).
Proof. intros X [[x gi] go]; unfold cost_mu, grade_op; simpl; apply token_size_concat. Qed.

(* A concrete witness that 𝔠 is NOT idempotent: a doubly-metered term carries a
   strictly longer modulus than its inner layer, so μ cannot be undone. *)
Theorem cost_monad_not_idempotent :
  exists (c : cost (cost nat)), ~ cost_equiv (cost_mu c) (fst c).
Proof.
  exists (0, (SUnit, TGate SUnit TUnit), (SUnit, TGate SUnit TUnit)).
  unfold cost_mu, grade_op, cost_equiv, grade_equiv; simpl.
  intros [_ [_ Htok]]. inversion Htok.
Qed.
