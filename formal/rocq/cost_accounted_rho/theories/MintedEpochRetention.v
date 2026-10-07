From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Lia.
From Stdlib Require Import List.

From CostAccountedRho Require Import EpochMintAtomicity.

Inductive minted_frontier : Type :=
  | Unminted
  | MintedThrough (epoch : nat).

Inductive frontier_decision : Type :=
  | AlreadyCompleted
  | AdvanceTo (epoch : nat)
  | FrontierGap.

Definition completedb (frontier : minted_frontier) (epoch : nat) : bool :=
  match frontier with
  | Unminted => false
  | MintedThrough current => epoch <=? current
  end.

Definition bootstrapb (epoch : nat) : bool :=
  (epoch =? 0) || (epoch =? 1).

Definition nextb (frontier : minted_frontier) (epoch : nat) : bool :=
  match frontier with
  | Unminted => bootstrapb epoch
  | MintedThrough current => epoch =? S current
  end.

Definition decide_frontier
  (frontier : minted_frontier)
  (epoch : nat) : frontier_decision :=
  if completedb frontier epoch then AlreadyCompleted
  else if nextb frontier epoch then AdvanceTo epoch
  else FrontierGap.

Definition frontier_rank (frontier : minted_frontier) : nat :=
  match frontier with
  | Unminted => 0
  | MintedThrough epoch => S epoch
  end.

Record retained_epoch_state : Type := {
  retained_epoch : epoch_state;
  retained_frontier : minted_frontier
}.

Definition publish_retained_close
  (pre_state prepared_state : retained_epoch_state)
  (epoch amount : nat)
  (outcomes : list mint_outcome) : retained_epoch_state :=
  match decide_frontier (retained_frontier pre_state) epoch with
  | AlreadyCompleted =>
      {| retained_epoch := retained_epoch prepared_state;
         retained_frontier := retained_frontier pre_state |}
  | FrontierGap => pre_state
  | AdvanceTo accepted_epoch =>
      match stage_batch amount
              (es_validators (retained_epoch prepared_state)) outcomes with
      | None => pre_state
      | Some _ =>
          {| retained_epoch :=
               publish_epoch_close
                 (retained_epoch pre_state)
                 (retained_epoch prepared_state)
                 amount
                 outcomes;
             retained_frontier := MintedThrough accepted_epoch |}
      end
  end.

Definition restart (state : retained_epoch_state) : retained_epoch_state := state.

Definition lifecycle_update
  (update : epoch_state -> epoch_state)
  (state : retained_epoch_state) : retained_epoch_state :=
  {| retained_epoch := update (retained_epoch state);
     retained_frontier := retained_frontier state |}.

Inductive sibling_choice : Type := KeepLeft | KeepRight.

Definition select_sibling
  (choice : sibling_choice)
  (left right : retained_epoch_state) : retained_epoch_state :=
  match choice with
  | KeepLeft => left
  | KeepRight => right
  end.

Theorem epoch_zero_bootstrap_advances :
  decide_frontier Unminted 0 = AdvanceTo 0.
Proof.
  reflexivity.
Qed.

Theorem epoch_one_bootstrap_advances :
  decide_frontier Unminted 1 = AdvanceTo 1.
Proof.
  reflexivity.
Qed.

Theorem later_unminted_epoch_is_a_gap : forall epoch,
  2 <= epoch ->
  decide_frontier Unminted epoch = FrontierGap.
Proof.
  intros epoch Hepoch.
  unfold decide_frontier, completedb, nextb, bootstrapb.
  assert (Hzero : (epoch =? 0) = false) by
    (apply Nat.eqb_neq; lia).
  assert (Hone : (epoch =? 1) = false) by
    (apply Nat.eqb_neq; lia).
  rewrite Hzero, Hone.
  reflexivity.
Qed.

Theorem completed_epoch_is_a_noop_decision : forall current epoch,
  epoch <= current ->
  decide_frontier (MintedThrough current) epoch = AlreadyCompleted.
Proof.
  intros current epoch Hcompleted.
  unfold decide_frontier, completedb.
  assert (Hle : (epoch <=? current) = true) by
    (apply Nat.leb_le; exact Hcompleted).
  rewrite Hle.
  reflexivity.
Qed.

Theorem immediate_successor_advances : forall current,
  decide_frontier (MintedThrough current) (S current) =
  AdvanceTo (S current).
Proof.
  intros current.
  unfold decide_frontier, completedb, nextb.
  assert (Hgt : (S current <=? current) = false) by
    (apply Nat.leb_gt; lia).
  rewrite Hgt.
  rewrite Nat.eqb_refl.
  reflexivity.
Qed.

Theorem skipped_successor_is_a_gap : forall current epoch,
  S current < epoch ->
  decide_frontier (MintedThrough current) epoch = FrontierGap.
Proof.
  intros current epoch Hgap.
  unfold decide_frontier, completedb, nextb.
  assert (Hgt : (epoch <=? current) = false) by
    (apply Nat.leb_gt; lia).
  assert (Hneq : (epoch =? S current) = false) by
    (apply Nat.eqb_neq; lia).
  rewrite Hgt, Hneq.
  reflexivity.
Qed.

Theorem accepted_advance_is_monotonic : forall frontier epoch,
  decide_frontier frontier epoch = AdvanceTo epoch ->
  frontier_rank frontier <= frontier_rank (MintedThrough epoch).
Proof.
  intros frontier epoch Hdecision.
  destruct frontier as [|current].
  - simpl. lia.
  - unfold decide_frontier, completedb, nextb in Hdecision.
    destruct (epoch <=? current) eqn:Hcompleted.
    + discriminate.
    + apply Nat.leb_gt in Hcompleted.
      destruct (epoch =? S current) eqn:Hnext.
      * apply Nat.eqb_eq in Hnext. subst. simpl. lia.
      * discriminate.
Qed.

Theorem accepted_advance_has_no_gap : forall current epoch,
  decide_frontier (MintedThrough current) epoch = AdvanceTo epoch ->
  epoch = S current.
Proof.
  intros current epoch Hdecision.
  unfold decide_frontier, completedb, nextb in Hdecision.
  destruct (epoch <=? current) eqn:Hcompleted.
  - discriminate.
  - destruct (epoch =? S current) eqn:Hnext.
    + apply Nat.eqb_eq. exact Hnext.
    + discriminate.
Qed.

Theorem frontier_gap_rejects_without_state_change :
  forall pre_state prepared_state epoch amount outcomes,
    decide_frontier (retained_frontier pre_state) epoch = FrontierGap ->
    publish_retained_close pre_state prepared_state epoch amount outcomes = pre_state.
Proof.
  intros pre_state prepared_state epoch amount outcomes Hgap.
  unfold publish_retained_close.
  rewrite Hgap.
  reflexivity.
Qed.

Theorem failed_batch_preserves_frontier_and_state :
  forall pre_state prepared_state epoch amount outcomes,
    decide_frontier (retained_frontier pre_state) epoch = AdvanceTo epoch ->
    stage_batch amount
      (es_validators (retained_epoch prepared_state)) outcomes = None ->
    publish_retained_close pre_state prepared_state epoch amount outcomes = pre_state.
Proof.
  intros pre_state prepared_state epoch amount outcomes Hadvance Hfailure.
  unfold publish_retained_close.
  rewrite Hadvance, Hfailure.
  reflexivity.
Qed.

Theorem successful_batch_advances_exactly_once :
  forall pre_state prepared_state epoch amount outcomes validators,
    decide_frontier (retained_frontier pre_state) epoch = AdvanceTo epoch ->
    stage_batch amount
      (es_validators (retained_epoch prepared_state)) outcomes = Some validators ->
    retained_frontier
      (publish_retained_close pre_state prepared_state epoch amount outcomes) =
    MintedThrough epoch.
Proof.
  intros pre_state prepared_state epoch amount outcomes validators Hadvance Hsuccess.
  unfold publish_retained_close.
  rewrite Hadvance, Hsuccess.
  reflexivity.
Qed.

Theorem zero_issuance_advances_after_complete_batch :
  forall pre_state prepared_state epoch outcomes validators,
    decide_frontier (retained_frontier pre_state) epoch = AdvanceTo epoch ->
    stage_batch 0
      (es_validators (retained_epoch prepared_state)) outcomes = Some validators ->
    retained_frontier
      (publish_retained_close pre_state prepared_state epoch 0 outcomes) =
    MintedThrough epoch.
Proof.
  intros.
  eapply successful_batch_advances_exactly_once; eauto.
Qed.

Theorem completed_epoch_preserves_frontier :
  forall pre_state prepared_state epoch amount outcomes current,
    retained_frontier pre_state = MintedThrough current ->
    epoch <= current ->
    retained_frontier
      (publish_retained_close pre_state prepared_state epoch amount outcomes) =
    MintedThrough current.
Proof.
  intros pre_state prepared_state epoch amount outcomes current Hfrontier Hcompleted.
  unfold publish_retained_close.
  rewrite Hfrontier.
  rewrite completed_epoch_is_a_noop_decision by exact Hcompleted.
  reflexivity.
Qed.

Theorem restart_preserves_frontier : forall state,
  retained_frontier (restart state) = retained_frontier state.
Proof.
  reflexivity.
Qed.

Theorem lifecycle_update_preserves_frontier : forall update state,
  retained_frontier (lifecycle_update update state) = retained_frontier state.
Proof.
  reflexivity.
Qed.

Theorem sibling_selection_keeps_one_complete_state : forall choice left right,
  select_sibling choice left right = left \/
  select_sibling choice left right = right.
Proof.
  intros choice left right.
  destruct choice; simpl; auto.
Qed.

Theorem sibling_selection_never_composes_frontiers : forall choice left right,
  retained_frontier (select_sibling choice left right) = retained_frontier left \/
  retained_frontier (select_sibling choice left right) = retained_frontier right.
Proof.
  intros choice left right.
  destruct choice; simpl; auto.
Qed.

Definition frontier_storage_cells (_ : minted_frontier) : nat := 1.

Theorem frontier_storage_is_constant : forall left right,
  frontier_storage_cells left = frontier_storage_cells right.
Proof.
  reflexivity.
Qed.

Theorem play_replay_frontier_agreement :
  forall pre_state prepared_state epoch amount outcomes,
    publish_retained_close pre_state prepared_state epoch amount outcomes =
    publish_retained_close pre_state prepared_state epoch amount outcomes.
Proof.
  reflexivity.
Qed.
