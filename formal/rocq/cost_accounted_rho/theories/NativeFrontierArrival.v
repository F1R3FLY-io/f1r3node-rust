From Stdlib Require Import Lists.List Sorting.Permutation Arith.PeanoNat Bool.Bool Lia.
From CostAccountedRho Require Import SerializedNativeFrontier.
Import ListNotations.

Definition canonical_frontier (bound : nat) (arrivals : list nat) : list nat :=
  filter (fun operation => existsb (Nat.eqb operation) arrivals) (seq 0 bound).

Lemma arrival_membership_permutation : forall operation left right,
  Permutation left right ->
  existsb (Nat.eqb operation) left = existsb (Nat.eqb operation) right.
Proof.
  intros operation left right permuted.
  assert (forward : forall source target,
    Permutation source target ->
    existsb (Nat.eqb operation) source = true ->
    existsb (Nat.eqb operation) target = true).
  {
    intros source target permutation present.
    apply existsb_exists in present.
    destruct present as [item [member equal]].
    apply existsb_exists.
    exists item. split; [eapply Permutation_in; eauto|exact equal].
  }
  destruct (existsb (Nat.eqb operation) left) eqn:left_member;
    destruct (existsb (Nat.eqb operation) right) eqn:right_member;
    try reflexivity.
  - pose proof (forward _ _ permuted left_member). congruence.
  - pose proof (forward _ _ (Permutation_sym permuted) right_member). congruence.
Qed.

Theorem same_arrivals_have_one_canonical_frontier : forall bound left right,
  Permutation left right ->
  canonical_frontier bound left = canonical_frontier bound right.
Proof.
  intros bound left right permuted.
  unfold canonical_frontier.
  apply filter_ext.
  intro operation.
  apply arrival_membership_permutation.
  exact permuted.
Qed.

Theorem canonical_frontier_covers_bounded_arrival : forall bound arrivals operation,
  operation < bound ->
  In operation arrivals ->
  In operation (canonical_frontier bound arrivals).
Proof.
  intros bound arrivals operation bounded member.
  unfold canonical_frontier.
  apply filter_In. split.
  - apply in_seq. lia.
  - apply existsb_exists.
    exists operation. split; [exact member|apply Nat.eqb_refl].
Qed.

Theorem canonical_frontier_is_complete_for_unique_bounded_arrivals :
  forall bound arrivals,
    NoDup arrivals ->
    (forall operation, In operation arrivals -> operation < bound) ->
    Permutation arrivals (canonical_frontier bound arrivals).
Proof.
  intros bound arrivals unique bounded.
  apply NoDup_Permutation.
  - exact unique.
  - unfold canonical_frontier. apply NoDup_filter. apply seq_NoDup.
  - intro operation. split.
    + intro member.
      apply canonical_frontier_covers_bounded_arrival.
      * apply bounded. exact member.
      * exact member.
    + unfold canonical_frontier. intro present.
      apply filter_In in present.
      destruct present as [_ included].
      apply existsb_exists in included.
      destruct included as [item [member equal]].
      apply Nat.eqb_eq in equal. subst. exact member.
Qed.

Theorem arrival_permutation_preserves_serial_root :
  forall step_root root input bound left right,
    Permutation left right ->
    final_frontier_root step_root root input (canonical_frontier bound left) =
    final_frontier_root step_root root input (canonical_frontier bound right).
Proof.
  intros step_root root input bound left right permuted.
  now rewrite (same_arrivals_have_one_canonical_frontier _ _ _ permuted).
Qed.

Print Assumptions same_arrivals_have_one_canonical_frontier.
Print Assumptions canonical_frontier_covers_bounded_arrival.
Print Assumptions canonical_frontier_is_complete_for_unique_bounded_arrivals.
Print Assumptions arrival_permutation_preserves_serial_root.
