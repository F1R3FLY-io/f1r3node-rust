From Stdlib Require Import Arith.PeanoNat Lists.List Lia Sorting.Permutation.
Import ListNotations.

Fixpoint required_price_ceiling (ceilings : list nat) : option nat :=
  match ceilings with
  | [] => None
  | ceiling :: rest =>
      Some (match required_price_ceiling rest with
            | None => ceiling
            | Some remaining => Nat.min ceiling remaining
            end)
  end.

Theorem missing_price_consent_iff_empty : forall ceilings,
  required_price_ceiling ceilings = None <-> ceilings = [].
Proof.
  destruct ceilings as [|ceiling rest]; simpl; split; intros result;
    try reflexivity; discriminate.
Qed.

Theorem required_price_ceiling_checks_every_consent : forall ceilings ceiling price,
  required_price_ceiling ceilings = Some ceiling ->
  (price <= ceiling <-> Forall (fun maximum => price <= maximum) ceilings).
Proof.
  intros ceilings. induction ceilings as [|head rest IH]; intros ceiling price result.
  - discriminate.
  - simpl in result.
    destruct (required_price_ceiling rest) as [remaining|] eqn:tail.
    + inversion result; subst ceiling.
      rewrite Nat.min_glb_iff.
      rewrite (IH remaining price eq_refl).
      split.
      * intros [head_bound tail_bound]. constructor; assumption.
      * intros all_bounds. inversion all_bounds; auto.
    + apply missing_price_consent_iff_empty in tail. subst rest.
      inversion result; subst ceiling.
      split.
      * intros bound. constructor; [assumption|constructor].
      * intros bounds. inversion bounds; assumption.
Qed.

Theorem required_price_ceiling_is_permutation_invariant : forall left right,
  Permutation left right ->
  required_price_ceiling left = required_price_ceiling right.
Proof.
  intros left right same.
  destruct (required_price_ceiling left) as [a|] eqn:left_result;
    destruct (required_price_ceiling right) as [b|] eqn:right_result.
  - assert (a <= b) as ab.
    { apply (proj2 (required_price_ceiling_checks_every_consent right b a right_result)).
      apply (Permutation_Forall same).
      apply (proj1 (required_price_ceiling_checks_every_consent left a a left_result)). lia. }
    assert (b <= a) as ba.
    { apply (proj2 (required_price_ceiling_checks_every_consent left a b left_result)).
      apply (Permutation_Forall (Permutation_sym same)).
      apply (proj1 (required_price_ceiling_checks_every_consent right b b right_result)). lia. }
    f_equal. lia.
  - apply missing_price_consent_iff_empty in right_result. subst right.
    apply Permutation_sym in same. apply Permutation_nil in same.
    subst left. discriminate.
  - apply missing_price_consent_iff_empty in left_result. subst left.
    apply Permutation_nil in same. subst right. discriminate.
  - reflexivity.
Qed.

Theorem adding_required_consent_cannot_raise_ceiling : forall rest old added next,
  required_price_ceiling rest = Some old ->
  required_price_ceiling (added :: rest) = Some next ->
  next <= old /\ next <= added.
Proof.
  intros rest old added next before after.
  simpl in after. rewrite before in after. inversion after; subst next.
  split; [apply Nat.le_min_r|apply Nat.le_min_l].
Qed.

Theorem duplicate_consent_does_not_change_ceiling : forall head rest,
  required_price_ceiling (head :: head :: rest) =
  required_price_ceiling (head :: rest).
Proof.
  intros head rest. simpl.
  destruct (required_price_ceiling rest); f_equal; lia.
Qed.

Example highest_ceiling_would_violate_required_consent :
  required_price_ceiling [3; 5; 8] = Some 3 /\
  4 <= 8 /\ ~ Forall (fun maximum => 4 <= maximum) [3; 5; 8].
Proof.
  split; [reflexivity|]. split; [lia|].
  intros all_bounds. inversion all_bounds. lia.
Qed.

Example empty_consent_is_not_unlimited : required_price_ceiling [] = None.
Proof. reflexivity. Qed.

Print Assumptions missing_price_consent_iff_empty.
Print Assumptions required_price_ceiling_checks_every_consent.
Print Assumptions required_price_ceiling_is_permutation_invariant.
Print Assumptions adding_required_consent_cannot_raise_ceiling.
Print Assumptions duplicate_consent_does_not_change_ceiling.
