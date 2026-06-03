(* ════════════════════════════════════════════════════════════════════════
   CACostFunctor.v — Thm 7.1 (Cost is an endofunctor) + Prop 6.2 (closure),
   continued-gslt-cost-v2 §6/§7. The installed [cost] apparatus (CostMonad) is the
   writer construction on [Type] ([cost X := X * grade]); since [cost_map] never
   touches the accumulated grade, the functor laws hold on the nose, so [Cost] is
   realized as a genuine [CategoryInterface.Functor] over the category of types and
   functions ([TypeCatL], hom-equality = pointwise Leibniz). The headline
   [cost_is_endofunctor] additionally records the identity/composition laws as the
   conjunction the spec states; [cost_obj_closure] (Prop 6.2) records closure of the
   grade carrier under the monoid operations, reusing the SignatureMonoid laws.
   This module proves the functor laws on TypeCatL (the writer skeleton). The
   companion CACostFunctorCI.v (DR-23) discharges Thm 7.1 ON THE CONCRETE ciGSLT
   category CICat — a genuine [CostCI : Functor CICat CICat] whose morphism action
   preserves the gated transition AND the behavioural equivalence
   ([cost_ci_preserves_step]/[cost_ci_preserves_bisim]) — so the categorical
   obligation is no longer merely the TypeCatL skeleton. Quote-faithfulness (the
   second ciGSLT-morphism condition) remains CACategory's scope boundary (CIMor does
   not reify it). Axiom-free.                                                    *)

From CostAccountedRho Require Import CostMonad.
From CostAccountedRho Require Import CategoryInterface.

(* The category of types and functions, hom-equality = pointwise Leibniz (an
   equivalence on the function type, identifying no functions — no funext). *)
Definition TypeCatL : Category.
Proof.
  refine {| Obj := Type; Hom := fun A B => A -> B;
            heq := fun A B f g => forall x, f x = g x;
            cid := fun A => (fun x => x);
            ccomp := fun A B C g f => (fun x => g (f x)) |}.
  - intros A B f x. reflexivity.
  - intros A B f g Hfg x. symmetry. apply Hfg.
  - intros A B f g h Hfg Hgh x. rewrite Hfg. apply Hgh.
  - intros A B f x. reflexivity.
  - intros A B f x. reflexivity.
  - intros A B C D h g f x. reflexivity.
  - intros A B C g g' f f' Hg Hf x. rewrite Hf. apply Hg.
Defined.

(* Cost as a genuine endofunctor: object map [cost], morphism map [cost_map]. The
   laws hold by reflexivity on pairs (cost_map is grade-preserving). *)
Definition CostEndofunctor : Functor TypeCatL TypeCatL.
Proof.
  refine (@Build_Functor TypeCatL TypeCatL cost (fun A B f => cost_map f) _ _ _).
  - intros A B f g Hfg [x gr]. unfold cost_map; simpl. rewrite (Hfg x). reflexivity.
  - intros A [x gr]. unfold cost_map; reflexivity.
  - intros A B C g f [x gr]. unfold cost_map; reflexivity.
Defined.

(* Thm 7.1 — the functor laws as the spec states them (cost_equiv form). *)
Theorem cost_is_endofunctor :
  (forall (X : Type) (c : cost X), cost_equiv (cost_map (fun x => x) c) c)
  /\ (forall (X Y Z : Type) (f : X -> Y) (g : Y -> Z) (c : cost X),
        cost_equiv (cost_map (fun x => g (f x)) c) (cost_map g (cost_map f c))).
Proof. split; [ exact @cost_map_id | exact @cost_map_compose ]. Qed.

(* Prop 6.2 — the grade carrier is closed under the monoid operations (unit on
   both sides + associativity), up to the signature/token equivalence. *)
Theorem cost_obj_closure :
  (forall g, grade_equiv (grade_op grade_unit g) g)
  /\ (forall g, grade_equiv (grade_op g grade_unit) g)
  /\ (forall g1 g2 g3,
        grade_equiv (grade_op (grade_op g1 g2) g3) (grade_op g1 (grade_op g2 g3))).
Proof.
  split; [ exact grade_op_unit_l
         | split; [ exact grade_op_unit_r | exact grade_op_assoc ] ].
Qed.
