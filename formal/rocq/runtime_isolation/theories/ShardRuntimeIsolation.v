From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
Import ListNotations.

Section ShardState.

Context {Shard Task : Type}.
Variable shard_eq_dec : forall left right : Shard, {left = right} + {left <> right}.
Variable owner : Task -> Shard.

Record shard_state := {
  shard_ledger : nat;
  shard_recorded_roots : nat;
  shard_version : nat
}.

Definition shard_aligned (state : shard_state) : Prop :=
  shard_recorded_roots state = shard_ledger state /\
  shard_version state = shard_ledger state.

Definition replace_shard
  (world : Shard -> shard_state)
  (target : Shard)
  (replacement : shard_state)
  : Shard -> shard_state :=
  fun shard =>
    if shard_eq_dec shard target then replacement else world shard.

Definition advance_shard (state : shard_state) : shard_state :=
  {|
    shard_ledger := S (shard_ledger state);
    shard_recorded_roots := S (shard_recorded_roots state);
    shard_version := S (shard_version state)
  |}.

Definition commit_task
  (world : Shard -> shard_state)
  (task : Task)
  : Shard -> shard_state :=
  let target := owner task in
  replace_shard world target (advance_shard (world target)).

Lemma replace_shard_at_target :
  forall world target replacement,
    replace_shard world target replacement target = replacement.
Proof.
  intros world target replacement.
  unfold replace_shard.
  destruct (shard_eq_dec target target); congruence.
Qed.

Lemma replace_shard_frames_other :
  forall world target replacement other,
    other <> target ->
    replace_shard world target replacement other = world other.
Proof.
  intros world target replacement other distinct.
  unfold replace_shard.
  destruct (shard_eq_dec other target); congruence.
Qed.

Theorem commit_task_frames_other_shards :
  forall world task shard,
    shard <> owner task ->
    commit_task world task shard = world shard.
Proof.
  intros world task shard distinct.
  unfold commit_task.
  apply replace_shard_frames_other.
  exact distinct.
Qed.

Theorem commit_task_preserves_alignment :
  forall world task,
    (forall shard, shard_aligned (world shard)) ->
    forall shard, shard_aligned (commit_task world task shard).
Proof.
  intros world task aligned shard.
  unfold commit_task, replace_shard.
  destruct (shard_eq_dec shard (owner task)) as [same | distinct].
  - subst shard.
    specialize (aligned (owner task)).
    unfold shard_aligned, advance_shard in *.
    simpl in *.
    lia.
  - exact (aligned shard).
Qed.

Inductive shard_action : Type :=
  | CommitTask (task : Task).

Definition action_shard (action : shard_action) : Shard :=
  match action with
  | CommitTask task => owner task
  end.

Definition apply_action
  (world : Shard -> shard_state)
  (action : shard_action)
  : Shard -> shard_state :=
  match action with
  | CommitTask task => commit_task world task
  end.

Fixpoint run_actions
  (world : Shard -> shard_state)
  (actions : list shard_action)
  : Shard -> shard_state :=
  match actions with
  | [] => world
  | action :: remaining => run_actions (apply_action world action) remaining
  end.

Lemma action_frames_other_shards :
  forall world action shard,
    shard <> action_shard action ->
    apply_action world action shard = world shard.
Proof.
  intros world [task] shard distinct.
  apply commit_task_frames_other_shards.
  exact distinct.
Qed.

Lemma apply_action_at_target_depends_only_on_target :
  forall left_world right_world action,
    left_world (action_shard action) = right_world (action_shard action) ->
    apply_action left_world action (action_shard action) =
      apply_action right_world action (action_shard action).
Proof.
  intros left_world right_world [task] same.
  simpl in *.
  unfold commit_task.
  repeat rewrite replace_shard_at_target.
  rewrite same.
  reflexivity.
Qed.

Theorem distinct_shard_actions_commute_pointwise :
  forall world left right shard,
    action_shard left <> action_shard right ->
    apply_action (apply_action world left) right shard =
      apply_action (apply_action world right) left shard.
Proof.
  intros world left right shard distinct.
  destruct (shard_eq_dec shard (action_shard left)) as [is_left | not_left].
  - subst shard.
    rewrite action_frames_other_shards by exact distinct.
    symmetry.
    apply apply_action_at_target_depends_only_on_target.
    apply action_frames_other_shards.
    exact distinct.
  - destruct (shard_eq_dec shard (action_shard right)) as [is_right | not_right].
    + subst shard.
      rewrite (action_frames_other_shards
        (apply_action world right) left (action_shard right) not_left).
      apply apply_action_at_target_depends_only_on_target.
      apply action_frames_other_shards.
      intro same.
      apply distinct.
      symmetry.
      exact same.
    + rewrite action_frames_other_shards by exact not_right.
      rewrite action_frames_other_shards by exact not_left.
      rewrite action_frames_other_shards by exact not_left.
      rewrite action_frames_other_shards by exact not_right.
      reflexivity.
Qed.

Theorem foreign_action_trace_preserves_protected_shard :
  forall actions world protected,
    Forall (fun action => protected <> action_shard action) actions ->
    run_actions world actions protected = world protected.
Proof.
  intros actions.
  induction actions as [|action remaining induction_hypothesis];
    intros world protected foreign; simpl.
  - reflexivity.
  - inversion foreign as [|? ? current_foreign remaining_foreign]; subst.
    rewrite (induction_hypothesis
      (apply_action world action) protected remaining_foreign).
    apply action_frames_other_shards.
    exact current_foreign.
Qed.

Theorem action_trace_preserves_alignment :
  forall actions world,
    (forall shard, shard_aligned (world shard)) ->
    forall shard, shard_aligned (run_actions world actions shard).
Proof.
  induction actions as [|action remaining induction_hypothesis];
    intros world aligned shard; simpl.
  - apply aligned.
  - apply induction_hypothesis.
    destruct action as [task].
    apply commit_task_preserves_alignment.
    exact aligned.
Qed.

End ShardState.

Section SharedWorkerCapacity.

Context {Task : Type}.
Variable task_eq_dec : forall left right : Task, {left = right} + {left <> right}.

Definition acquire_worker
  (capacity : nat)
  (running : list Task)
  (task : Task)
  : list Task :=
  if in_dec task_eq_dec task running
  then running
  else if length running <? capacity then task :: running else running.

Definition release_worker
  (running : list Task)
  (task : Task)
  : list Task :=
  remove task_eq_dec task running.

Theorem acquire_worker_preserves_unique_ownership :
  forall capacity running task,
    NoDup running ->
    NoDup (acquire_worker capacity running task).
Proof.
  intros capacity running task unique.
  unfold acquire_worker.
  destruct (in_dec task_eq_dec task running) as [present | absent].
  - exact unique.
  - destruct (length running <? capacity).
    + constructor; assumption.
    + exact unique.
Qed.

Theorem acquire_worker_preserves_capacity :
  forall capacity running task,
    length running <= capacity ->
    length (acquire_worker capacity running task) <= capacity.
Proof.
  intros capacity running task bounded.
  unfold acquire_worker.
  destruct (in_dec task_eq_dec task running).
  - exact bounded.
  - destruct (length running <? capacity) eqn:available.
    + apply Nat.ltb_lt in available.
      simpl.
      lia.
    + exact bounded.
Qed.

Lemma remove_preserves_unique_ownership :
  forall task running,
    NoDup running ->
    NoDup (remove task_eq_dec task running).
Proof.
  intros task running.
  induction running as [|current remaining induction_hypothesis]; intro unique; simpl.
  - constructor.
  - inversion unique as [|? ? absent unique_remaining]; subst.
    destruct (task_eq_dec task current) as [same | distinct].
    + apply induction_hypothesis.
      exact unique_remaining.
    + constructor.
      * intro present.
        apply in_remove in present as [present _].
        contradiction.
      * apply induction_hypothesis.
        exact unique_remaining.
Qed.

Theorem release_worker_preserves_unique_ownership :
  forall running task,
    NoDup running ->
    NoDup (release_worker running task).
Proof.
  intros running task unique.
  unfold release_worker.
  apply remove_preserves_unique_ownership.
  exact unique.
Qed.

Theorem release_worker_preserves_capacity :
  forall capacity running task,
    length running <= capacity ->
    length (release_worker running task) <= capacity.
Proof.
  intros capacity running task bounded.
  unfold release_worker.
  eapply Nat.le_trans.
  - apply remove_length_le.
  - exact bounded.
Qed.

Theorem shared_worker_capstone :
  forall capacity running task,
    NoDup running ->
    length running <= capacity ->
    NoDup (acquire_worker capacity running task) /\
    length (acquire_worker capacity running task) <= capacity /\
    NoDup (release_worker running task) /\
    length (release_worker running task) <= capacity.
Proof.
  intros capacity running task unique bounded.
  repeat split.
  - apply acquire_worker_preserves_unique_ownership.
    exact unique.
  - apply acquire_worker_preserves_capacity.
    exact bounded.
  - apply release_worker_preserves_unique_ownership.
    exact unique.
  - apply release_worker_preserves_capacity.
    exact bounded.
Qed.

End SharedWorkerCapacity.

Print Assumptions commit_task_frames_other_shards.
Print Assumptions commit_task_preserves_alignment.
Print Assumptions distinct_shard_actions_commute_pointwise.
Print Assumptions foreign_action_trace_preserves_protected_shard.
Print Assumptions action_trace_preserves_alignment.
Print Assumptions shared_worker_capstone.
