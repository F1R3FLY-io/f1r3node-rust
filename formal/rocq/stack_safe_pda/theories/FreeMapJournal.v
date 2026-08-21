From Stdlib Require Import List ListDec Arith Bool Lia.
From StackSafePDA Require Import SpatialMatcher.

Import ListNotations.
Set Implicit Arguments.

(** Refinement model for the drive-local Rust [FreeMapTransaction].  A journal
    is newest-first here; this is the reverse view of the Rust [Vec], whose
    rollback loop pops the same entries in the same order. *)

Record undo_entry : Type := UndoEntry {
  undo_level : nat;
  undo_previous : option nat;
  undo_snapshot : free_map
}.

Record journal_transaction : Type := JournalTransaction {
  journal_map : free_map;
  journal_undo : list undo_entry
}.

Definition checkpoint (transaction : journal_transaction) : nat :=
  length (journal_undo transaction).

Definition restore_entry (state : free_map) (entry : undo_entry) : free_map :=
  fun level =>
    if Nat.eq_dec level (undo_level entry)
    then undo_previous entry
    else state level.

Definition journal_write
    (transaction : journal_transaction) (level value : nat)
    : journal_transaction :=
  JournalTransaction
    (insert_binding (journal_map transaction) level value)
    (UndoEntry level (journal_map transaction level) (journal_map transaction) ::
      journal_undo transaction).

Fixpoint rollback_steps
    (count : nat) (transaction : journal_transaction) : journal_transaction :=
  match count with
  | 0 => transaction
  | S rest =>
      match journal_undo transaction with
      | [] => transaction
      | entry :: undo =>
          rollback_steps rest
            (JournalTransaction (undo_snapshot entry) undo)
      end
  end.

Definition rollback_to
    (saved_checkpoint : nat) (transaction : journal_transaction)
    : journal_transaction :=
  rollback_steps
    (length (journal_undo transaction) - saved_checkpoint)
    transaction.

Fixpoint runtime_rollback_steps
    (count : nat) (transaction : journal_transaction) : journal_transaction :=
  match count with
  | 0 => transaction
  | S rest =>
      match journal_undo transaction with
      | [] => transaction
      | entry :: undo =>
          runtime_rollback_steps rest
            (JournalTransaction
              (restore_entry (journal_map transaction) entry)
              undo)
      end
  end.

Definition runtime_rollback_to
    (saved_checkpoint : nat) (transaction : journal_transaction)
    : journal_transaction :=
  runtime_rollback_steps
    (length (journal_undo transaction) - saved_checkpoint)
    transaction.

Definition transaction_equiv
    (first second : journal_transaction) : Prop :=
  (forall level, journal_map first level = journal_map second level) /\
  journal_undo first = journal_undo second.

Fixpoint journal_writes
    (transaction : journal_transaction) (writes : list (nat * nat))
    : journal_transaction :=
  match writes with
  | [] => transaction
  | (level, value) :: rest =>
      journal_writes (journal_write transaction level value) rest
  end.

Fixpoint ordered_extend
    (state : free_map) (writes : list (nat * nat)) : free_map :=
  match writes with
  | [] => state
  | (level, value) :: rest =>
      ordered_extend (insert_binding state level value) rest
  end.

Lemma restore_entry_after_insert :
  forall state level value candidate,
    restore_entry
      (insert_binding state level value)
      (UndoEntry level (state level) state) candidate = state candidate.
Proof.
  intros state level value candidate.
  unfold restore_entry, insert_binding. simpl.
  destruct (Nat.eq_dec candidate level) as [equal|different].
  - now subst candidate.
  - reflexivity.
Qed.

Theorem journal_write_refines_insert :
  forall transaction level value,
    journal_map (journal_write transaction level value) =
    insert_binding (journal_map transaction) level value.
Proof. reflexivity. Qed.

Lemma rollback_one_write :
  forall transaction level value,
    rollback_steps 1 (journal_write transaction level value) = transaction.
Proof.
  intros [state undo] level value. reflexivity.
Qed.

Lemma rollback_steps_add :
  forall first second transaction,
    rollback_steps (first + second) transaction =
    rollback_steps second (rollback_steps first transaction).
Proof.
  induction first as [|first IH]; intros second [state undo]; simpl.
  - reflexivity.
  - destruct undo as [|entry undo]; simpl.
    + destruct second; reflexivity.
    + apply IH.
Qed.

Lemma transaction_equiv_refl :
  forall transaction, transaction_equiv transaction transaction.
Proof. intros transaction. split; reflexivity. Qed.

Lemma transaction_equiv_trans :
  forall first second third,
    transaction_equiv first second ->
    transaction_equiv second third ->
    transaction_equiv first third.
Proof.
  intros first second third [maps_first undo_first] [maps_second undo_second].
  split.
  - intro level. now rewrite maps_first, maps_second.
  - now rewrite undo_first, undo_second.
Qed.

Lemma runtime_rollback_steps_add :
  forall first second transaction,
    runtime_rollback_steps (first + second) transaction =
    runtime_rollback_steps second (runtime_rollback_steps first transaction).
Proof.
  induction first as [|first IH]; intros second [state undo]; simpl.
  - reflexivity.
  - destruct undo as [|entry undo]; simpl.
    + destruct second; reflexivity.
    + apply IH.
Qed.

Lemma runtime_rollback_steps_respects_transaction_equiv :
  forall count first second,
    transaction_equiv first second ->
    transaction_equiv
      (runtime_rollback_steps count first)
      (runtime_rollback_steps count second).
Proof.
  induction count as [|count IH];
    intros [first_map first_undo] [second_map second_undo] [maps undo]; simpl in *.
  - split; assumption.
  - subst second_undo. destruct first_undo as [|entry undo_log]; simpl.
    + split; [exact maps|reflexivity].
    + apply IH. split.
      * intro level.
        change
          (restore_entry first_map entry level =
           restore_entry second_map entry level).
        unfold restore_entry.
        destruct (Nat.eq_dec level (undo_level entry)); [reflexivity|apply maps].
      * reflexivity.
Qed.

Lemma runtime_rollback_one_write_refines_snapshot :
  forall transaction level value,
    transaction_equiv
      (runtime_rollback_steps 1 (journal_write transaction level value))
      transaction.
Proof.
  intros [state undo] level value. split; simpl.
  - apply restore_entry_after_insert.
  - reflexivity.
Qed.

Lemma journal_writes_undo_length :
  forall transaction writes,
    length (journal_undo (journal_writes transaction writes)) =
    length writes + length (journal_undo transaction).
Proof.
  intros transaction writes. revert transaction.
  induction writes as [|[level value] rest IH]; intro transaction; simpl.
  - reflexivity.
  - rewrite IH. simpl. lia.
Qed.

Lemma journal_writes_app :
  forall transaction first second,
    journal_writes transaction (first ++ second) =
    journal_writes (journal_writes transaction first) second.
Proof.
  intros transaction first. revert transaction.
  induction first as [|[level value] rest IH]; intros transaction second; simpl.
  - reflexivity.
  - apply IH.
Qed.

Lemma journal_writes_singleton :
  forall transaction level value,
    journal_writes transaction [(level, value)] =
    journal_write transaction level value.
Proof. reflexivity. Qed.

Lemma journal_writes_rollback_all :
  forall transaction writes,
    rollback_steps (length writes) (journal_writes transaction writes) = transaction.
Proof.
  intros transaction writes. revert transaction.
  induction writes using rev_ind; intro transaction.
  - reflexivity.
  - destruct x as [level value].
    rewrite journal_writes_app, length_app, journal_writes_singleton. simpl length.
    rewrite Nat.add_comm, rollback_steps_add, rollback_one_write.
    apply IHwrites.
Qed.

Lemma journal_writes_runtime_rollback_all :
  forall transaction writes,
    transaction_equiv
      (runtime_rollback_steps (length writes)
        (journal_writes transaction writes))
      transaction.
Proof.
  intros transaction writes. revert transaction.
  induction writes using rev_ind; intro transaction.
  - apply transaction_equiv_refl.
  - destruct x as [level value].
    rewrite journal_writes_app, length_app, journal_writes_singleton. simpl length.
    rewrite Nat.add_comm, runtime_rollback_steps_add.
    eapply transaction_equiv_trans.
    + apply runtime_rollback_steps_respects_transaction_equiv.
      apply runtime_rollback_one_write_refines_snapshot.
    + apply IHwrites.
Qed.

Theorem rollback_to_checkpoint_restores_checkpoint_map :
  forall saved writes,
    rollback_to (checkpoint saved) (journal_writes saved writes) = saved.
Proof.
  intros saved writes.
  unfold rollback_to, checkpoint.
  rewrite journal_writes_undo_length.
  replace (length writes + length (journal_undo saved) -
           length (journal_undo saved)) with (length writes) by lia.
  apply journal_writes_rollback_all.
Qed.

Theorem runtime_rollback_to_checkpoint_restores_checkpoint_map :
  forall saved writes level,
    journal_map
      (runtime_rollback_to (checkpoint saved)
        (journal_writes saved writes)) level =
    journal_map saved level.
Proof.
  intros saved writes level.
  unfold runtime_rollback_to, checkpoint.
  rewrite journal_writes_undo_length.
  replace (length writes + length (journal_undo saved) -
           length (journal_undo saved)) with (length writes) by lia.
  exact (proj1 (journal_writes_runtime_rollback_all saved writes) level).
Qed.

Theorem nested_rollback_preserves_outer_history :
  forall entry outer_writes inner_writes,
    rollback_to
      (checkpoint (journal_writes entry outer_writes))
      (journal_writes (journal_writes entry outer_writes) inner_writes) =
    journal_writes entry outer_writes.
Proof. intros. apply rollback_to_checkpoint_restores_checkpoint_map. Qed.

Theorem inner_success_history_remains_outer_rollbackable :
  forall entry outer_writes inner_writes,
    rollback_to (checkpoint entry)
      (journal_writes
        (journal_writes entry outer_writes)
        inner_writes) = entry.
Proof.
  intros entry outer_writes inner_writes.
  rewrite <- journal_writes_app.
  apply rollback_to_checkpoint_restores_checkpoint_map.
Qed.

Theorem repeated_overwrite_rollback_restores_baseline :
  forall entry level first second third,
    rollback_to (checkpoint entry)
      (journal_writes entry
        [(level, first); (level, second); (level, third)]) = entry.
Proof. intros. apply rollback_to_checkpoint_restores_checkpoint_map. Qed.

(** Deltas contain an optional replacement at each level.  [Some None] is the
    total-map deletion case; journaled matcher writes are insert-only, so the
    executable [BindingDelta] realizes only [Some (Some value)]. *)

Definition binding_delta := nat -> option (option nat).

Definition option_nat_eqb (first second : option nat) : bool :=
  match first, second with
  | None, None => true
  | Some first_value, Some second_value => Nat.eqb first_value second_value
  | _, _ => false
  end.

Lemma option_nat_eqb_eq :
  forall first second, option_nat_eqb first second = true <-> first = second.
Proof.
  intros [first|] [second|]; simpl.
  - rewrite Nat.eqb_eq. split.
    + intro equal. now subst.
    + intro equal. now injection equal.
  - split; discriminate.
  - split; discriminate.
  - split; reflexivity.
Qed.

Definition delta_between (baseline final : free_map) : binding_delta :=
  fun level =>
    if option_nat_eqb (baseline level) (final level)
    then None
    else Some (final level).

Definition apply_binding_delta
    (baseline : free_map) (delta : binding_delta) : free_map :=
  fun level =>
    match delta level with
    | None => baseline level
    | Some replacement => replacement
    end.

Definition empty_binding_delta : binding_delta := fun _ => None.

Definition take_delta
    (saved : journal_transaction) (current : journal_transaction)
    : binding_delta * journal_transaction :=
  (delta_between (journal_map saved) (journal_map current),
   rollback_to (checkpoint saved) current).

Definition runtime_take_delta
    (saved : journal_transaction) (current : journal_transaction)
    : binding_delta * journal_transaction :=
  (delta_between (journal_map saved) (journal_map current),
   runtime_rollback_to (checkpoint saved) current).

Lemma snapshot_take_delta_restores_checkpoint :
  forall saved writes,
    snd (take_delta saved (journal_writes saved writes)) = saved.
Proof.
  intros saved writes. unfold take_delta. simpl.
  apply rollback_to_checkpoint_restores_checkpoint_map.
Qed.

Theorem take_delta_restores_checkpoint :
  forall saved writes level,
    journal_map
      (snd (runtime_take_delta saved (journal_writes saved writes))) level =
    journal_map saved level.
Proof.
  intros saved writes level. unfold runtime_take_delta. simpl.
  apply runtime_rollback_to_checkpoint_restores_checkpoint_map.
Qed.

Lemma delta_between_replays_exactly :
  forall baseline final level,
    apply_binding_delta baseline (delta_between baseline final) level = final level.
Proof.
  intros baseline final level.
  unfold apply_binding_delta, delta_between.
  destruct (option_nat_eqb (baseline level) (final level)) eqn:equal; simpl.
  - now apply option_nat_eqb_eq in equal.
  - reflexivity.
Qed.

Theorem take_delta_replay_reconstructs_pre_rollback_map :
  forall saved writes level,
    apply_binding_delta
      (journal_map saved)
      (fst (runtime_take_delta saved (journal_writes saved writes))) level =
    journal_map (journal_writes saved writes) level.
Proof. intros. apply delta_between_replays_exactly. Qed.

Theorem take_delta_omits_baseline_equal_values :
  forall saved current level,
    journal_map saved level = journal_map current level ->
    fst (runtime_take_delta saved current) level = None.
Proof.
  intros saved current level equal.
  unfold runtime_take_delta, delta_between. simpl.
  rewrite equal.
  destruct (journal_map current level) as [value|]; simpl.
  - now rewrite Nat.eqb_refl.
  - reflexivity.
Qed.

Theorem empty_binding_delta_is_identity :
  forall state level,
    apply_binding_delta state empty_binding_delta level = state level.
Proof. reflexivity. Qed.

Lemma journal_writes_refines_ordered_extend :
  forall transaction writes,
    journal_map (journal_writes transaction writes) =
    ordered_extend (journal_map transaction) writes.
Proof.
  intros transaction writes. revert transaction.
  induction writes as [|[level value] rest IH]; intro transaction; simpl.
  - reflexivity.
  - apply IH.
Qed.

Definition added_levels
    (baseline : free_map) (writes : list (nat * nat)) : list nat :=
  filter
    (fun level =>
      match baseline level with
      | None => true
      | Some _ => false
      end)
    (map fst writes).

Definition updates_valid
    (baseline : free_map) (writes : list (nat * nat)) : Prop :=
  NoDup (added_levels baseline writes).

Definition updates_valid_dec
    (baseline : free_map) (writes : list (nat * nat))
    : {updates_valid baseline writes} + {~ updates_valid baseline writes} :=
  NoDup_dec Nat.eq_dec (added_levels baseline writes).

Definition validate_then_apply
    (baseline : free_map) (writes : list (nat * nat)) : bool * free_map :=
  if updates_valid_dec baseline writes
  then (true, ordered_extend baseline writes)
  else (false, baseline).

Theorem validate_then_apply_refusal_is_atomic :
  forall baseline writes,
    fst (validate_then_apply baseline writes) = false ->
    snd (validate_then_apply baseline writes) = baseline.
Proof.
  intros baseline writes refused.
  unfold validate_then_apply in *.
  destruct (updates_valid_dec baseline writes); simpl in *.
  - discriminate.
  - reflexivity.
Qed.

Theorem journaled_aggregate_refines_ordered_extend :
  forall transaction writes,
    updates_valid (journal_map transaction) writes ->
    journal_map (journal_writes transaction writes) =
    snd (validate_then_apply (journal_map transaction) writes).
Proof.
  intros transaction writes valid.
  unfold validate_then_apply.
  destruct (updates_valid_dec (journal_map transaction) writes) as [accepted|refused].
  - simpl. apply journal_writes_refines_ordered_extend.
  - contradiction.
Qed.

Definition finish_and
    (entry_checkpoint : nat) (accepted : bool)
    (current : journal_transaction) : journal_transaction :=
  if accepted then current else rollback_to entry_checkpoint current.

Definition finish_or
    (entry_checkpoint : nat) (current : journal_transaction)
    : journal_transaction :=
  rollback_to entry_checkpoint current.

Definition finish_not := finish_or.

Definition runtime_finish_and
    (entry_checkpoint : nat) (accepted : bool)
    (current : journal_transaction) : journal_transaction :=
  if accepted then current else runtime_rollback_to entry_checkpoint current.

Definition runtime_finish_or
    (entry_checkpoint : nat) (current : journal_transaction)
    : journal_transaction :=
  runtime_rollback_to entry_checkpoint current.

Definition runtime_finish_not := runtime_finish_or.

Theorem and_success_retains_bindings :
  forall saved writes,
    runtime_finish_and (checkpoint saved) true (journal_writes saved writes) =
    journal_writes saved writes.
Proof. reflexivity. Qed.

Theorem and_refusal_restores_entry_map :
  forall saved writes level,
    journal_map
      (runtime_finish_and
        (checkpoint saved) false (journal_writes saved writes)) level =
    journal_map saved level.
Proof.
  intros. unfold runtime_finish_and.
  apply runtime_rollback_to_checkpoint_restores_checkpoint_map.
Qed.

Theorem or_unconditionally_restores_entry_map :
  forall saved writes level,
    journal_map
      (runtime_finish_or
        (checkpoint saved) (journal_writes saved writes)) level =
    journal_map saved level.
Proof. intros. apply runtime_rollback_to_checkpoint_restores_checkpoint_map. Qed.

Theorem not_unconditionally_restores_entry_map :
  forall saved writes level,
    journal_map
      (runtime_finish_not
        (checkpoint saved) (journal_writes saved writes)) level =
    journal_map saved level.
Proof. intros. apply runtime_rollback_to_checkpoint_restores_checkpoint_map. Qed.

Definition finish_edge
    (saved : journal_transaction) (accepted : bool)
    (current : journal_transaction)
    : option binding_delta * journal_transaction :=
  if accepted
  then (Some (fst (take_delta saved current)), snd (take_delta saved current))
  else (None, rollback_to (checkpoint saved) current).

Definition runtime_finish_edge
    (saved : journal_transaction) (accepted : bool)
    (current : journal_transaction)
    : option binding_delta * journal_transaction :=
  if accepted
  then
    (Some (fst (runtime_take_delta saved current)),
     snd (runtime_take_delta saved current))
  else (None, runtime_rollback_to (checkpoint saved) current).

Theorem edge_success_returns_exact_delta_and_restores :
  forall saved writes,
    fst (runtime_finish_edge saved true (journal_writes saved writes)) =
      Some (delta_between
        (journal_map saved)
        (journal_map (journal_writes saved writes))) /\
    forall level,
      journal_map
        (snd (runtime_finish_edge saved true (journal_writes saved writes))) level =
      journal_map saved level.
Proof.
  intros saved writes.
  split; [reflexivity|].
  apply take_delta_restores_checkpoint.
Qed.

Theorem edge_refusal_restores_without_delta :
  forall saved writes,
    fst (runtime_finish_edge saved false (journal_writes saved writes)) = None /\
    forall level,
      journal_map
        (snd (runtime_finish_edge saved false (journal_writes saved writes))) level =
      journal_map saved level.
Proof.
  intros saved writes. split; [reflexivity|].
  apply runtime_rollback_to_checkpoint_restores_checkpoint_map.
Qed.

Definition finish_root := runtime_finish_and.
Definition abort_drop := runtime_finish_or.

Theorem root_refusal_restores_initial_map :
  forall initial writes level,
    journal_map
      (finish_root (checkpoint initial) false (journal_writes initial writes)) level =
    journal_map initial level.
Proof. intros. apply and_refusal_restores_entry_map. Qed.

Theorem abort_drop_restores_initial_map :
  forall initial writes level,
    journal_map
      (abort_drop (checkpoint initial) (journal_writes initial writes)) level =
    journal_map initial level.
Proof. intros. apply or_unconditionally_restores_entry_map. Qed.

Theorem undo_length_equals_completed_uncommitted_writes :
  forall transaction writes,
    checkpoint (journal_writes transaction writes) - checkpoint transaction =
    length writes.
Proof.
  intros transaction writes.
  unfold checkpoint. rewrite journal_writes_undo_length. lia.
Qed.

Lemma ordered_extend_unchanged_outside_written_levels :
  forall baseline writes level,
    ~ In level (map fst writes) ->
    ordered_extend baseline writes level = baseline level.
Proof.
  intros baseline writes. revert baseline.
  induction writes as [|[write_level value] rest IH]; intros baseline level absent; simpl.
  - reflexivity.
  - rewrite IH.
    + unfold insert_binding.
      destruct (Nat.eq_dec level write_level) as [equal|different].
      * exfalso. apply absent. left. now symmetry.
      * reflexivity.
    + intro present. apply absent. now right.
Qed.

Definition delta_changed (delta : binding_delta) (level : nat) : Prop :=
  exists replacement, delta level = Some replacement.

Definition delta_changedb (delta : binding_delta) (level : nat) : bool :=
  match delta level with
  | Some _ => true
  | None => false
  end.

Definition segment_support (writes : list (nat * nat)) : list nat :=
  nodup Nat.eq_dec (map fst writes).

Theorem delta_support_bounded_by_segment_writes :
  forall saved writes,
    let delta := fst (runtime_take_delta saved (journal_writes saved writes)) in
    (forall level, delta_changed delta level -> In level (segment_support writes)) /\
    length (filter (delta_changedb delta) (segment_support writes)) <= length writes.
Proof.
  intros saved writes. simpl. split.
  - intros level [replacement changed].
    unfold take_delta, delta_between in changed. simpl in changed.
    destruct (option_nat_eqb
      (journal_map saved level)
      (journal_map (journal_writes saved writes) level)) eqn:equal;
      [discriminate|].
    destruct (in_dec Nat.eq_dec level (map fst writes)) as [present|absent].
    + unfold segment_support. now apply nodup_In.
    + rewrite journal_writes_refines_ordered_extend in equal.
      rewrite ordered_extend_unchanged_outside_written_levels in equal by exact absent.
      destruct (journal_map saved level) as [value|]; simpl in equal.
      * now rewrite Nat.eqb_refl in equal.
      * discriminate.
  - eapply Nat.le_trans.
    + apply filter_length_le.
    + unfold segment_support.
      eapply Nat.le_trans.
      * apply NoDup_incl_length.
        -- apply NoDup_nodup.
        -- intros level present.
           rewrite <- (nodup_In Nat.eq_dec (map fst writes) level).
           exact present.
      * rewrite length_map. apply Nat.le_refl.
Qed.

Theorem bounded_active_path_writes_imply_linear_journal_retention :
  forall transaction writes writes_per_depth depth,
    length writes <= writes_per_depth * depth ->
    checkpoint (journal_writes transaction writes) - checkpoint transaction <=
    writes_per_depth * depth.
Proof.
  intros transaction writes writes_per_depth depth bound.
  now rewrite undo_length_equals_completed_uncommitted_writes.
Qed.

Definition snapshot_and
    (entry final : free_map) (accepted : bool) : free_map :=
  if accepted then final else entry.

Definition snapshot_isolated (entry : free_map) : free_map := entry.

Theorem journal_connective_control_refines_snapshot_semantics :
  forall saved writes,
    (forall level,
      journal_map
        (runtime_finish_and
          (checkpoint saved) true (journal_writes saved writes)) level =
      snapshot_and
        (journal_map saved)
        (journal_map (journal_writes saved writes)) true level) /\
    (forall level,
      journal_map
        (runtime_finish_and
          (checkpoint saved) false (journal_writes saved writes)) level =
      snapshot_and
        (journal_map saved)
        (journal_map (journal_writes saved writes)) false level) /\
    (forall level,
      journal_map
        (runtime_finish_or
          (checkpoint saved) (journal_writes saved writes)) level =
      snapshot_isolated (journal_map saved) level) /\
    (forall level,
      journal_map
        (runtime_finish_not
          (checkpoint saved) (journal_writes saved writes)) level =
      snapshot_isolated (journal_map saved) level).
Proof.
  intros saved writes.
  split.
  - intro level. reflexivity.
  - split.
    + intro level. simpl. apply and_refusal_restores_entry_map.
    + split.
      * intro level. apply or_unconditionally_restores_entry_map.
      * intro level. apply not_unconditionally_restores_entry_map.
Qed.

Theorem journal_transaction_refines_snapshot_transaction :
  forall saved writes,
    journal_map (journal_writes saved writes) =
      ordered_extend (journal_map saved) writes /\
    journal_map (rollback_to (checkpoint saved) (journal_writes saved writes)) =
      journal_map saved /\
    (forall level,
      journal_map
        (runtime_rollback_to (checkpoint saved)
          (journal_writes saved writes)) level =
      journal_map saved level) /\
    (forall level,
      apply_binding_delta
        (journal_map saved)
        (fst (runtime_take_delta saved (journal_writes saved writes))) level =
        journal_map (journal_writes saved writes) level) /\
    (forall level,
      journal_map
        (snd (runtime_take_delta saved (journal_writes saved writes))) level =
      journal_map saved level).
Proof.
  intros saved writes. repeat split.
  - apply journal_writes_refines_ordered_extend.
  - now rewrite rollback_to_checkpoint_restores_checkpoint_map.
  - apply runtime_rollback_to_checkpoint_restores_checkpoint_map.
  - apply take_delta_replay_reconstructs_pre_rollback_map.
  - apply take_delta_restores_checkpoint.
Qed.

Print Assumptions journal_connective_control_refines_snapshot_semantics.
Print Assumptions journal_transaction_refines_snapshot_transaction.
