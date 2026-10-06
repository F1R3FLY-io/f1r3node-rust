From Stdlib Require Import Arith.PeanoNat Lists.List Lia.

Import ListNotations.

Definition backing_sum (backing : list nat) : nat :=
  fold_right Nat.add 0 backing.

Definition authenticated_capacity (fee : nat) (backing : list nat) : nat :=
  backing_sum backing - fee.

Fixpoint frontier_attempts
  (demand fee : nat)
  (known frontier : list nat)
  : nat :=
  if demand <=? authenticated_capacity fee known then
    0
  else
    match frontier with
    | [] => 0
    | next :: rest =>
        S (frontier_attempts demand fee (known ++ [next]) rest)
    end.

Definition speculative_attempt
  (known : list nat)
  (discovered : nat)
  : list nat * nat :=
  (known, discovered).

Definition replay_capacity := authenticated_capacity.

Definition candidate_independent_capacity
  (fee : nat)
  (prestate : list nat)
  (_candidate_created : list nat)
  : nat :=
  authenticated_capacity fee prestate.

Lemma fold_add_accumulator :
  forall values accumulator,
    fold_right Nat.add accumulator values =
    fold_right Nat.add 0 values + accumulator.
Proof.
  intros values accumulator.
  induction values as [| value rest IH].
  - reflexivity.
  - simpl.
    rewrite IH.
    lia.
Qed.

Lemma backing_sum_app :
  forall left right,
    backing_sum (left ++ right) = backing_sum left + backing_sum right.
Proof.
  intros left right.
  unfold backing_sum.
  rewrite fold_right_app.
  apply fold_add_accumulator.
Qed.

Theorem authenticated_capacity_append_monotone :
  forall fee known next,
    authenticated_capacity fee known <=
    authenticated_capacity fee (known ++ [next]).
Proof.
  intros fee known next.
  unfold authenticated_capacity.
  rewrite backing_sum_app.
  simpl.
  apply Nat.sub_le_mono_r.
  apply Nat.le_add_r.
Qed.

Theorem positive_backing_strictly_expands_capacity :
  forall fee known next,
    fee <= backing_sum known ->
    0 < next ->
    authenticated_capacity fee known <
    authenticated_capacity fee (known ++ [next]).
Proof.
  intros fee known next fee_bounded positive.
  unfold authenticated_capacity.
  rewrite backing_sum_app.
  simpl.
  lia.
Qed.

Theorem authenticated_prefix_is_bounded_by_total :
  forall fee known frontier,
    authenticated_capacity fee known <=
    authenticated_capacity fee (known ++ frontier).
Proof.
  intros fee known frontier.
  unfold authenticated_capacity.
  rewrite backing_sum_app.
  apply Nat.sub_le_mono_r.
  apply Nat.le_add_r.
Qed.

Theorem frontier_retry_count_is_finite :
  forall demand fee known frontier,
    frontier_attempts demand fee known frontier <= length frontier.
Proof.
  intros demand fee known frontier.
  revert known.
  induction frontier as [| next rest IH]; intros known.
  - unfold frontier_attempts.
    destruct (demand <=? authenticated_capacity fee known); reflexivity.
  - simpl.
    destruct (demand <=? authenticated_capacity fee known) eqn:fits.
    + lia.
    + apply le_n_S.
      apply IH.
Qed.

Theorem speculative_exhaustion_does_not_publish_state :
  forall known discovered,
    fst (speculative_attempt known discovered) = known.
Proof.
  reflexivity.
Qed.

Theorem discovered_backing_conserves_total_supply :
  forall known next frontier,
    backing_sum (known ++ next :: frontier) =
    backing_sum ((known ++ [next]) ++ frontier).
Proof.
  intros known next frontier.
  repeat rewrite backing_sum_app.
  simpl.
  lia.
Qed.

Theorem candidate_created_supply_cannot_expand_prestate_capacity :
  forall fee prestate first_candidate second_candidate,
    candidate_independent_capacity fee prestate first_candidate =
    candidate_independent_capacity fee prestate second_candidate.
Proof.
  reflexivity.
Qed.

Theorem replay_uses_the_same_authenticated_capacity :
  forall fee backing,
    replay_capacity fee backing = authenticated_capacity fee backing.
Proof.
  reflexivity.
Qed.

Definition initial_physical_inventory
  (custody : nat -> nat) (selected frontier : list nat) : list nat :=
  nodup Nat.eq_dec (map custody (selected ++ frontier)).

Theorem initial_inventory_contains_every_selected_custody :
  forall custody selected frontier signer,
    In signer selected ->
    In (custody signer) (initial_physical_inventory custody selected frontier).
Proof.
  intros custody selected frontier signer Hselected.
  unfold initial_physical_inventory.
  apply nodup_In.
  apply in_map.
  apply in_or_app.
  now left.
Qed.

Theorem initial_inventory_has_no_unexplained_custody :
  forall custody selected frontier key,
    In key (initial_physical_inventory custody selected frontier) <->
    exists signer, custody signer = key /\
      (In signer selected \/ In signer frontier).
Proof.
  intros custody selected frontier key.
  unfold initial_physical_inventory.
  rewrite nodup_In, in_map_iff.
  split; intros [signer [Hkey Hin]]; exists signer; split; auto.
  - now apply in_app_iff in Hin.
  - now apply in_app_iff.
Qed.

Theorem initial_inventory_counts_each_custody_once :
  forall custody selected frontier,
    NoDup (initial_physical_inventory custody selected frontier).
Proof.
  intros custody selected frontier.
  unfold initial_physical_inventory.
  apply NoDup_nodup.
Qed.

Theorem initial_inventory_is_order_independent :
  forall custody selected frontier key,
    In key (initial_physical_inventory custody selected frontier) <->
    In key (initial_physical_inventory custody frontier selected).
Proof.
  intros custody selected frontier key.
  rewrite !initial_inventory_has_no_unexplained_custody.
  split; intros [signer [Hkey Hin]]; exists signer; tauto.
Qed.

Print Assumptions initial_inventory_contains_every_selected_custody.
Print Assumptions initial_inventory_has_no_unexplained_custody.
Print Assumptions initial_inventory_counts_each_custody_once.
Print Assumptions initial_inventory_is_order_independent.
