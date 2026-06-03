(* ════════════════════════════════════════════════════════════════════════
   CACostMonadInstances.v — Prop 9.1/9.2 as live RECORD INSTANCES of
   CategoryInterface (not merely the law conjunctions CACostMonadCat /
   CAAdjunctionI prove). Cost is instantiated as a [Monad] on the setoid category
   GCat (objects = setoids, morphisms = relation-respecting maps), and its
   canonical Kleisli resolution as an [Adjunction]. The base category MUST carry a
   grade-aware [heq]: the unit/associativity laws hold only up to [grade_equiv]
   (the unit grade absorbs by grade_op_unit_*, NOT Leibniz — the R-C obstruction),
   which is exactly the codomain relation of the setoid category. The law content
   is reused verbatim from CostMonad. Axiom-free.                                *)

From CostAccountedRho Require Import CostMonad.
From CostAccountedRho Require Import CategoryInterface.

(* ── The setoid category: objects carry an equivalence; morphisms respect it. ── *)
Record GSetoid : Type := {
  gs_car   : Type;
  gs_eq    : gs_car -> gs_car -> Prop;
  gs_refl  : forall x, gs_eq x x;
  gs_sym   : forall x y, gs_eq x y -> gs_eq y x;
  gs_trans : forall x y z, gs_eq x y -> gs_eq y z -> gs_eq x z
}.

Record GMor (A B : GSetoid) : Type := {
  sm_map  : gs_car A -> gs_car B;
  sm_resp : forall x y, gs_eq A x y -> gs_eq B (sm_map x) (sm_map y)
}.
Arguments sm_map {A B} _ _.
Arguments sm_resp {A B} _ _ _ _.

Definition GMor_id (A : GSetoid) : GMor A A :=
  {| sm_map := fun x => x; sm_resp := fun x y H => H |}.

Definition GMor_comp {A B C : GSetoid} (g : GMor B C) (f : GMor A B) : GMor A C :=
  {| sm_map  := fun x => sm_map g (sm_map f x);
     sm_resp := fun x y H => sm_resp g _ _ (sm_resp f _ _ H) |}.

Definition GMor_heq {A B : GSetoid} (f g : GMor A B) : Prop :=
  forall x, gs_eq B (sm_map f x) (sm_map g x).

Definition GCat : Category.
Proof.
  refine {| Obj := GSetoid; Hom := GMor;
            heq := @GMor_heq; cid := GMor_id; ccomp := @GMor_comp |}.
  - intros A B f x. apply gs_refl.
  - intros A B f g H x. apply gs_sym, H.
  - intros A B f g h H1 H2 x. eapply gs_trans; [ apply H1 | apply H2 ].
  - intros A B f x. apply gs_refl.
  - intros A B f x. apply gs_refl.
  - intros A B C D f g h x. apply gs_refl.
  - intros A B C g g' f f' Hg Hf x. simpl.
    eapply gs_trans; [ exact (sm_resp g _ _ (Hf x)) | exact (Hg (sm_map f' x)) ].
Defined.

(* ── Cost lifted to a setoid endofunctor on GCat. ─────────────────────────────
   On the carrier: value up to A's relation, grade up to grade_equiv. *)
Definition cost_setoid (A : GSetoid) : GSetoid :=
  {| gs_car := cost (gs_car A);
     gs_eq  := fun c d => gs_eq A (fst c) (fst d) /\ grade_equiv (snd c) (snd d);
     gs_refl  := fun c => conj (gs_refl A (fst c)) (grade_equiv_refl (snd c));
     gs_sym   := fun c d H => conj (gs_sym A _ _ (proj1 H)) (grade_equiv_sym _ _ (proj2 H));
     gs_trans := fun c d e H1 H2 =>
                   conj (gs_trans A _ _ _ (proj1 H1) (proj1 H2))
                        (grade_equiv_trans _ _ _ (proj2 H1) (proj2 H2)) |}.

Definition cost_GMor {A B : GSetoid} (f : GMor A B) :
  GMor (cost_setoid A) (cost_setoid B) :=
  @Build_GMor (cost_setoid A) (cost_setoid B)
    (fun c => cost_map (sm_map f) c)
    (fun c d H => conj (sm_resp f _ _ (proj1 H)) (proj2 H)).

Definition CostFunctorG : Functor GCat GCat.
Proof.
  refine (@Build_Functor GCat GCat cost_setoid (@cost_GMor) _ _ _).
  - intros A B f g H [cv cg]. split; [ exact (H cv) | apply grade_equiv_refl ].
  - intros A [cv cg]. split; [ apply gs_refl | apply grade_equiv_refl ].
  - intros A B C f g [cv cg]. split; [ apply gs_refl | apply grade_equiv_refl ].
Defined.

(* ── Prop 9.1 — Cost IS a Monad on GCat (record instance). ──────────────────── *)
Definition cost_eta_GMor (A : GSetoid) : GMor A (cost_setoid A) :=
  @Build_GMor A (cost_setoid A)
    (fun x => cost_eta x)
    (fun x y H => conj H (grade_equiv_refl grade_unit)).

Definition cost_mu_GMor (A : GSetoid) :
  GMor (cost_setoid (cost_setoid A)) (cost_setoid A) :=
  @Build_GMor (cost_setoid (cost_setoid A)) (cost_setoid A)
    (fun c => cost_mu c)
    (fun c d H =>
       conj (proj1 (proj1 H)) (grade_op_cong _ _ _ _ (proj2 (proj1 H)) (proj2 H))).

Definition cost_monad_instance : Monad GCat.
Proof.
  refine (@Build_Monad GCat CostFunctorG cost_eta_GMor cost_mu_GMor _ _ _ _ _).
  - (* Meta_natural *) intros A B f x. split; [ apply gs_refl | apply grade_equiv_refl ].
  - (* Mmu_natural *)  intros A B f [[cv ci] co].
      split; [ apply gs_refl | apply grade_equiv_refl ].
  - (* Mleft_unit *)   intros A [cv cg]. split; [ apply gs_refl | apply grade_op_unit_r ].
  - (* Mright_unit *)  intros A [cv cg]. split; [ apply gs_refl | apply grade_op_unit_l ].
  - (* Massoc *)       intros A [[[cv ci] cm] co]. split;
      [ apply gs_refl | apply grade_equiv_sym; apply grade_op_assoc ].
Defined.

(* ── Prop 9.2 — the Kleisli resolution Free ⊣ Forget (record instance). ─────────
   The canonical adjunction generating the Cost monad: Forget ∘ Free = Cost, and
   the two triangle identities ARE the monad unit laws (Mleft_unit / Mright_unit),
   here up to grade_equiv. *)
Definition kl_comp {A B C : GSetoid}
  (g : GMor B (cost_setoid C)) (f : GMor A (cost_setoid B)) : GMor A (cost_setoid C) :=
  @Build_GMor A (cost_setoid C)
    (fun a => cost_mu (cost_map (sm_map g) (sm_map f a)))
    (fun a a' Ha =>
       conj (proj1 (sm_resp g _ _ (proj1 (sm_resp f _ _ Ha))))
            (grade_op_cong _ _ _ _
               (proj2 (sm_resp g _ _ (proj1 (sm_resp f _ _ Ha))))
               (proj2 (sm_resp f _ _ Ha)))).

Definition KleisliCat : Category.
Proof.
  refine {| Obj := GSetoid;
            Hom := fun A B => GMor A (cost_setoid B);
            heq := fun A B f g => @GMor_heq A (cost_setoid B) f g;
            cid := cost_eta_GMor;
            ccomp := @kl_comp |}.
  - intros A B f x. apply gs_refl.
  - intros A B f g H x. apply gs_sym, H.
  - intros A B f g h H1 H2 x. eapply gs_trans; [ apply H1 | apply H2 ].
  - (* ccomp_id_l : η_B ∘ₖ f ≡ f *)
    intros A B f x. split; [ apply gs_refl | apply grade_op_unit_l ].
  - (* ccomp_id_r : g ∘ₖ η_A ≡ g *)
    intros A B f x. split; [ apply gs_refl | apply grade_op_unit_r ].
  - (* ccomp_assoc *)
    intros A B C D f g h x. split; [ apply gs_refl | apply grade_equiv_sym; apply grade_op_assoc ].
  - (* ccomp_proper *)
    intros A B C g g' f f' Hg Hf x.
    pose proof (Hf x) as HfB.
    pose proof (sm_resp g _ _ (proj1 HfB)) as HgB.
    pose proof (Hg (fst (sm_map f' x))) as HggC.
    split.
    + eapply gs_trans; [ exact (proj1 HgB) | exact (proj1 HggC) ].
    + apply grade_op_cong;
        [ eapply grade_equiv_trans; [ exact (proj2 HgB) | exact (proj2 HggC) ]
        | exact (proj2 HfB) ].
Defined.

Definition Free : Functor GCat KleisliCat.
Proof.
  refine (@Build_Functor GCat KleisliCat (fun A => A)
            (fun A B f => @GMor_comp A B (cost_setoid B) (cost_eta_GMor B) f) _ _ _).
  - intros A B f g H x. split; [ exact (H x) | apply grade_equiv_refl ].
  - intros A x. split; [ apply gs_refl | apply grade_equiv_refl ].
  - intros A B C f g x. split; [ apply gs_refl | apply grade_equiv_sym; apply grade_op_unit_l ].
Defined.

Definition Forget : Functor KleisliCat GCat.
Proof.
  refine (@Build_Functor KleisliCat GCat cost_setoid
            (fun A B f => @GMor_comp (cost_setoid A) (cost_setoid (cost_setoid B)) (cost_setoid B)
                            (cost_mu_GMor B) (cost_GMor f)) _ _ _).
  - intros A B f g H [cv cg]. split;
      [ exact (proj1 (H cv)) | apply grade_op_cong; [ exact (proj2 (H cv)) | apply grade_equiv_refl ] ].
  - intros A [cv cg]. split; [ apply gs_refl | apply grade_op_unit_l ].
  - intros A B C f g [cv cg]. split; [ apply gs_refl | apply grade_op_assoc ].
Defined.

Definition cost_kleisli_adjunction : Adjunction Free Forget.
Proof.
  refine (@Build_Adjunction GCat KleisliCat Free Forget
            cost_eta_GMor (fun b => GMor_id (cost_setoid b)) _ _).
  - (* Atriangle_F : ε_{Fa} ∘ₖ F(η_a) ≡ id  — the Mleft_unit triangle *)
    intros a x. split; [ apply gs_refl | apply grade_op_unit_l ].
  - (* Atriangle_G : G(ε_b) ∘ η_{Gb} ≡ id  — the Mright_unit triangle *)
    intros b [cv cg]. split; [ apply gs_refl | apply grade_op_unit_r ].
Defined.
