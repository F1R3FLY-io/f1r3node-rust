(* ════════════════════════════════════════════════════════════════════════
   CategoryInterface.v — a bespoke axiom-free setoid scaffold for the abstract
   category theory of continued-gslt-cost-v2 (§6-§9).

   No category-theory library is installed, and the available ones assume
   functional extensionality / UIP, which the cost-accounted-rho axiom gate
   forbids. This module supplies the minimal interface the §6-§9 claims touch,
   built on a HOM-SETOID: each hom-set is a [Type] [Hom a b] carrying a
   Prop-valued equivalence [heq] (its reflexivity/symmetry/transitivity are
   record fields, not a global instance), and composition is a congruence for
   [heq] (also a field). Every law is Prop-valued and stated up to [heq]; no
   field is an equality of functions, so no extensionality is ever needed. The
   Adjunction is presented in unit/counit + triangle-identity form. Mirrors the
   SignatureMonoid.v setoid idiom (a custom [≡] with carried congruences).
   The object/index arguments of [heq]/[ccomp]/[cid] are implicit and inferred
   from the hom values (the Category from the [Hom] type), so clients never pass
   them. Axiom-free.                                                            *)

Record Category : Type := {
  Obj  : Type;
  Hom  : Obj -> Obj -> Type;
  heq  : forall {a b}, Hom a b -> Hom a b -> Prop;

  heq_refl  : forall {a b} (f : Hom a b), heq f f;
  heq_sym   : forall {a b} (f g : Hom a b), heq f g -> heq g f;
  heq_trans : forall {a b} (f g h : Hom a b), heq f g -> heq g h -> heq f h;

  cid  : forall a, Hom a a;
  ccomp : forall {a b c}, Hom b c -> Hom a b -> Hom a c;

  ccomp_id_l : forall {a b} (f : Hom a b), heq (ccomp (cid b) f) f;
  ccomp_id_r : forall {a b} (f : Hom a b), heq (ccomp f (cid a)) f;
  ccomp_assoc : forall {a b c d} (h : Hom c d) (g : Hom b c) (f : Hom a b),
      heq (ccomp h (ccomp g f)) (ccomp (ccomp h g) f);

  ccomp_proper : forall {a b c} (g g' : Hom b c) (f f' : Hom a b),
      heq g g' -> heq f f' -> heq (ccomp g f) (ccomp g' f')
}.

(* Make the Category and the object indices implicit (inferred from the hom
   arguments, or from [a : Obj C] for [cid]); clients write [ccomp g f], [heq f g],
   [cid a] with no annotations. *)
Arguments heq {_ _ _} _ _.
Arguments cid {_} _.
Arguments ccomp {_ _ _ _} _ _.
Arguments heq_refl {_ _ _} _.
Arguments heq_sym {_ _ _} _ _ _.
Arguments heq_trans {_ _ _} _ _ _ _ _.
Arguments ccomp_id_l {_ _ _} _.
Arguments ccomp_id_r {_ _ _} _.
Arguments ccomp_assoc {_ _ _ _ _} _ _ _.
Arguments ccomp_proper {_ _ _ _} _ _ _ _ _.

(* ── Functor: object/morphism maps, congruence + identity/composition
   preservation up to the codomain's [heq]. ────────────────────────────────── *)
Record Functor (C D : Category) : Type := {
  Fobj : Obj C -> Obj D;
  Fmor : forall {a b}, Hom C a b -> Hom D (Fobj a) (Fobj b);

  Fmor_proper : forall {a b} (f g : Hom C a b), heq f g -> heq (Fmor f) (Fmor g);
  Fmor_id : forall a, heq (Fmor (cid a)) (cid (Fobj a));
  Fmor_comp : forall {a b c} (g : Hom C b c) (f : Hom C a b),
      heq (Fmor (ccomp g f)) (ccomp (Fmor g) (Fmor f))
}.

Arguments Fobj {C D} _ _.
Arguments Fmor {C D} _ {a b} _.
Arguments Fmor_proper {C D} _ {a b} _ _ _.
Arguments Fmor_id {C D} _ _.
Arguments Fmor_comp {C D} _ {a b c} _ _.

Definition Functor_id (C : Category) : Functor C C.
Proof.
  refine {| Fobj := fun a => a; Fmor := fun a b f => f |}; intros.
  - assumption.
  - apply heq_refl.
  - apply heq_refl.
Defined.

Definition Functor_comp {C D E} (G : Functor D E) (F : Functor C D) : Functor C E.
Proof.
  refine {| Fobj := fun a => Fobj G (Fobj F a);
            Fmor := fun a b f => Fmor G (Fmor F f) |}; intros.
  - apply Fmor_proper. apply Fmor_proper. assumption.
  - eapply heq_trans; [ apply Fmor_proper; apply Fmor_id | apply Fmor_id ].
  - eapply heq_trans; [ apply Fmor_proper; apply Fmor_comp | apply Fmor_comp ].
Defined.

(* ── Natural transformation: components + naturality up to [heq]. ──────────── *)
Record NaturalTransformation {C D} (F G : Functor C D) : Type := {
  Ncomp : forall a, Hom D (Fobj F a) (Fobj G a);
  Nnatural : forall {a b} (f : Hom C a b),
      heq (ccomp (Ncomp b) (Fmor F f)) (ccomp (Fmor G f) (Ncomp a))
}.

Arguments Ncomp {C D F G} _ _.

(* ── Monad on [C] in (T, η, μ) form; laws stated pointwise (per object), which
   is equality of natural transformations WITHOUT funext. ──────────────────── *)
Record Monad (C : Category) : Type := {
  Tf  : Functor C C;
  Meta : forall a, Hom C a (Fobj Tf a);
  Mmu  : forall a, Hom C (Fobj Tf (Fobj Tf a)) (Fobj Tf a);

  Meta_natural : forall {a b} (f : Hom C a b),
      heq (ccomp (Meta b) f) (ccomp (Fmor Tf f) (Meta a));
  Mmu_natural : forall {a b} (f : Hom C a b),
      heq (ccomp (Mmu b) (Fmor Tf (Fmor Tf f))) (ccomp (Fmor Tf f) (Mmu a));

  Mleft_unit  : forall a, heq (ccomp (Mmu a) (Meta (Fobj Tf a))) (cid (Fobj Tf a));
  Mright_unit : forall a, heq (ccomp (Mmu a) (Fmor Tf (Meta a))) (cid (Fobj Tf a));
  Massoc : forall a,
      heq (ccomp (Mmu a) (Mmu (Fobj Tf a))) (ccomp (Mmu a) (Fmor Tf (Mmu a)))
}.

Arguments Tf {C} _.
Arguments Meta {C} _ _.
Arguments Mmu {C} _ _.

(* ── Adjunction F ⊣ G in unit/counit + triangle-identity form. ────────────── *)
Record Adjunction {C D} (F : Functor C D) (G : Functor D C) : Type := {
  Aunit   : forall a, Hom C a (Fobj G (Fobj F a));
  Acounit : forall b, Hom D (Fobj F (Fobj G b)) b;

  Atriangle_F : forall a,
      heq (ccomp (Acounit (Fobj F a)) (Fmor F (Aunit a))) (cid (Fobj F a));
  Atriangle_G : forall b,
      heq (ccomp (Fmor G (Acounit b)) (Aunit (Fobj G b))) (cid (Fobj G b))
}.

Arguments Aunit {C D F G} _ _.
Arguments Acounit {C D F G} _ _.
