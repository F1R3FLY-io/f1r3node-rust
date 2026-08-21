From Stdlib Require Import Lists.List.

Set Implicit Arguments.

Section DetectorProduct.

Context {Validator LocalState : Type}.
Variable validator_eq_dec : forall x y : Validator, {x = y} + {x <> y}.
Variable local_step : LocalState -> LocalState -> Prop.
Variable local_invariant : LocalState -> Prop.

Definition DetectorProductState := Validator -> LocalState.

Definition detector_product_invariant (state : DetectorProductState) : Prop :=
  forall validator, local_invariant (state validator).

Definition detector_product_step
  (before after : DetectorProductState) : Prop :=
  exists validator,
    local_step (before validator) (after validator) /\
    forall other, other <> validator -> after other = before other.

Inductive detector_product_steps :
  DetectorProductState -> DetectorProductState -> Prop :=
| detector_product_steps_refl :
    forall state, detector_product_steps state state
| detector_product_steps_next :
    forall before middle after,
      detector_product_steps before middle ->
      detector_product_step middle after ->
      detector_product_steps before after.

Theorem detector_product_step_preserves_pointwise_invariant :
  (forall before after,
      local_invariant before ->
      local_step before after ->
      local_invariant after) ->
  forall before after,
    detector_product_invariant before ->
    detector_product_step before after ->
    detector_product_invariant after.
Proof.
  intros Hlocal before after Hbefore [validator [Hstep Hstable]] other.
  destruct (validator_eq_dec other validator) as [Heq | Hneq].
  - subst other. eapply Hlocal; [apply Hbefore | exact Hstep].
  - rewrite (Hstable other Hneq). apply Hbefore.
Qed.

Theorem detector_product_steps_preserve_pointwise_invariant :
  (forall before after,
      local_invariant before ->
      local_step before after ->
      local_invariant after) ->
  forall before after,
    detector_product_invariant before ->
    detector_product_steps before after ->
    detector_product_invariant after.
Proof.
  intros Hlocal before after Hbefore Hsteps.
  induction Hsteps.
  - exact Hbefore.
  - apply detector_product_step_preserves_pointwise_invariant with (before := middle).
    + exact Hlocal.
    + apply IHHsteps. exact Hbefore.
    + exact H.
Qed.

End DetectorProduct.
