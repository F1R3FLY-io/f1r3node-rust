From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
Import ListNotations.

Section MultiShardState.

Context {Shard Task : Type}.
Variable shard_eq_dec : forall left right : Shard, {left = right} + {left <> right}.
Variable owner : Task -> Shard.

Record shard_account := {
  shard_ledger : nat;
  shard_recorded_roots : nat;
  shard_balance : nat;
  shard_charged : nat;
  shard_deposited : nat
}.

Definition shard_conserved (state : shard_account) : Prop :=
  shard_deposited state = shard_balance state + shard_charged state.

Definition shard_root_aligned (state : shard_account) : Prop :=
  shard_recorded_roots state = shard_ledger state.

Definition replace_shard
  (world : Shard -> shard_account)
  (target : Shard)
  (replacement : shard_account)
  : Shard -> shard_account :=
  fun shard =>
    if shard_eq_dec shard target then replacement else world shard.

Definition debit_account (state : shard_account) : shard_account :=
  {|
    shard_ledger := S (shard_ledger state);
    shard_recorded_roots := S (shard_recorded_roots state);
    shard_balance := Nat.pred (shard_balance state);
    shard_charged := S (shard_charged state);
    shard_deposited := shard_deposited state
  |}.

Definition commit_task
  (world : Shard -> shard_account)
  (task : Task)
  : Shard -> shard_account :=
  let target := owner task in
  let state := world target in
  if 0 <? shard_balance state
  then replace_shard world target (debit_account state)
  else world.

Definition top_up_shard
  (world : Shard -> shard_account)
  (target : Shard)
  (amount : nat)
  : Shard -> shard_account :=
  let state := world target in
  replace_shard world target
    {|
      shard_ledger := shard_ledger state;
      shard_recorded_roots := shard_recorded_roots state;
      shard_balance := shard_balance state + amount;
      shard_charged := shard_charged state;
      shard_deposited := shard_deposited state + amount
    |}.

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
  destruct (0 <? shard_balance (world (owner task))); try reflexivity.
  apply replace_shard_frames_other.
  exact distinct.
Qed.

Theorem top_up_frames_other_shards :
  forall world target amount shard,
    shard <> target ->
    top_up_shard world target amount shard = world shard.
Proof.
  intros world target amount shard distinct.
  unfold top_up_shard.
  apply replace_shard_frames_other.
  exact distinct.
Qed.

Theorem commit_task_preserves_per_shard_conservation :
  forall world task,
    (forall shard, shard_conserved (world shard)) ->
    forall shard, shard_conserved (commit_task world task shard).
Proof.
  intros world task conserved shard.
  unfold commit_task.
  destruct (0 <? shard_balance (world (owner task))) eqn:funded.
  - unfold replace_shard.
    destruct (shard_eq_dec shard (owner task)) as [same | distinct].
    + subst shard.
      specialize (conserved (owner task)).
      unfold shard_conserved, debit_account in *.
      simpl in *.
      apply Nat.ltb_lt in funded.
      lia.
    + exact (conserved shard).
  - exact (conserved shard).
Qed.

Theorem top_up_preserves_per_shard_conservation :
  forall world target amount,
    (forall shard, shard_conserved (world shard)) ->
    forall shard, shard_conserved (top_up_shard world target amount shard).
Proof.
  intros world target amount conserved shard.
  unfold top_up_shard, replace_shard.
  destruct (shard_eq_dec shard target) as [same | distinct].
  - subst shard.
    specialize (conserved target).
    unfold shard_conserved in *.
    simpl in *.
    lia.
  - exact (conserved shard).
Qed.

Theorem commit_task_preserves_per_shard_root_alignment :
  forall world task,
    (forall shard, shard_root_aligned (world shard)) ->
    forall shard, shard_root_aligned (commit_task world task shard).
Proof.
  intros world task aligned shard.
  unfold commit_task.
  destruct (0 <? shard_balance (world (owner task))).
  - unfold replace_shard.
    destruct (shard_eq_dec shard (owner task)) as [same | distinct].
    + subst shard.
      specialize (aligned (owner task)).
      unfold shard_root_aligned, debit_account in *.
      simpl in *.
      lia.
    + exact (aligned shard).
  - exact (aligned shard).
Qed.

Theorem top_up_preserves_per_shard_root_alignment :
  forall world target amount,
    (forall shard, shard_root_aligned (world shard)) ->
    forall shard, shard_root_aligned (top_up_shard world target amount shard).
Proof.
  intros world target amount aligned shard.
  unfold top_up_shard, replace_shard.
  destruct (shard_eq_dec shard target) as [same | distinct].
  - subst shard.
    exact (aligned target).
  - exact (aligned shard).
Qed.

Inductive shard_action : Type :=
  | CommitTask (task : Task)
  | TopUpShard (shard : Shard) (amount : nat).

Definition action_shard (action : shard_action) : Shard :=
  match action with
  | CommitTask task => owner task
  | TopUpShard shard _ => shard
  end.

Definition apply_action
  (world : Shard -> shard_account)
  (action : shard_action)
  : Shard -> shard_account :=
  match action with
  | CommitTask task => commit_task world task
  | TopUpShard shard amount => top_up_shard world shard amount
  end.

Fixpoint run_actions
  (world : Shard -> shard_account)
  (actions : list shard_action)
  : Shard -> shard_account :=
  match actions with
  | [] => world
  | action :: remaining => run_actions (apply_action world action) remaining
  end.

Lemma action_frames_other_shards :
  forall world action shard,
    shard <> action_shard action ->
    apply_action world action shard = world shard.
Proof.
  intros world [task | target amount] shard distinct; simpl in *.
  - apply commit_task_frames_other_shards.
    exact distinct.
  - apply top_up_frames_other_shards.
    exact distinct.
Qed.

Lemma apply_action_at_target_depends_only_on_target :
  forall left_world right_world action,
    left_world (action_shard action) = right_world (action_shard action) ->
    apply_action left_world action (action_shard action) =
      apply_action right_world action (action_shard action).
Proof.
  intros left_world right_world [task | target amount] same; simpl in *.
  - unfold commit_task.
    rewrite same.
    destruct (0 <? shard_balance (right_world (owner task))).
    + repeat rewrite replace_shard_at_target.
      reflexivity.
    + exact same.
  - unfold top_up_shard.
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
    rewrite (induction_hypothesis (apply_action world action) protected remaining_foreign).
    apply action_frames_other_shards.
    exact current_foreign.
Qed.

Theorem action_trace_preserves_per_shard_conservation :
  forall actions world,
    (forall shard, shard_conserved (world shard)) ->
    forall shard, shard_conserved (run_actions world actions shard).
Proof.
  induction actions as [|action remaining induction_hypothesis];
    intros world conserved shard; simpl.
  - apply conserved.
  - apply induction_hypothesis.
    destruct action as [task | target amount]; simpl.
    + apply commit_task_preserves_per_shard_conservation.
      exact conserved.
    + apply top_up_preserves_per_shard_conservation.
      exact conserved.
Qed.

Theorem action_trace_preserves_per_shard_root_alignment :
  forall actions world,
    (forall shard, shard_root_aligned (world shard)) ->
    forall shard, shard_root_aligned (run_actions world actions shard).
Proof.
  induction actions as [|action remaining induction_hypothesis];
    intros world aligned shard; simpl.
  - apply aligned.
  - apply induction_hypothesis.
    destruct action as [task | target amount]; simpl.
    + apply commit_task_preserves_per_shard_root_alignment.
      exact aligned.
    + apply top_up_preserves_per_shard_root_alignment.
      exact aligned.
Qed.

Fixpoint admitted_commits
  (count : nat)
  (state : shard_account)
  : shard_account :=
  match count with
  | 0 => state
  | S remaining => admitted_commits remaining (debit_account state)
  end.

Lemma admitted_commits_ledger :
  forall count state,
    shard_ledger (admitted_commits count state) =
    shard_ledger state + count.
Proof.
  induction count as [|count induction_hypothesis]; intros state; simpl.
  - lia.
  - rewrite induction_hypothesis.
    simpl.
    lia.
Qed.

Lemma admitted_commits_balance :
  forall count state,
    shard_balance (admitted_commits count state) =
    shard_balance state - count.
Proof.
  induction count as [|count induction_hypothesis]; intros state; simpl.
  - lia.
  - rewrite induction_hypothesis.
    simpl.
    lia.
Qed.

Lemma admitted_commits_recorded_roots :
  forall count state,
    shard_recorded_roots (admitted_commits count state) =
    shard_recorded_roots state + count.
Proof.
  induction count as [|count induction_hypothesis]; intros state; simpl.
  - lia.
  - rewrite induction_hypothesis.
    simpl.
    lia.
Qed.

Lemma admitted_commits_charged :
  forall count state,
    shard_charged (admitted_commits count state) =
    shard_charged state + count.
Proof.
  induction count as [|count induction_hypothesis]; intros state; simpl.
  - lia.
  - rewrite induction_hypothesis.
    simpl.
    lia.
Qed.

Lemma admitted_commits_deposited :
  forall count state,
    shard_deposited (admitted_commits count state) =
    shard_deposited state.
Proof.
  induction count as [|count induction_hypothesis]; intros state; simpl.
  - reflexivity.
  - rewrite induction_hypothesis.
    reflexivity.
Qed.

Theorem admitted_serial_commits_have_no_lost_updates :
  forall count state,
    shard_ledger (admitted_commits count state) = shard_ledger state + count /\
    shard_recorded_roots (admitted_commits count state) =
      shard_recorded_roots state + count /\
    shard_charged (admitted_commits count state) = shard_charged state + count.
Proof.
  intros count state.
  split.
  - apply admitted_commits_ledger.
  - split.
    + apply admitted_commits_recorded_roots.
    + apply admitted_commits_charged.
Qed.

Theorem admitted_serial_commits_preserve_conservation :
  forall count state,
    count <= shard_balance state ->
    shard_conserved state ->
    shard_conserved (admitted_commits count state).
Proof.
  intros count state funded conserved.
  unfold shard_conserved in *.
  rewrite admitted_commits_deposited.
  rewrite admitted_commits_balance.
  rewrite admitted_commits_charged.
  lia.
Qed.

End MultiShardState.

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

Print Assumptions foreign_action_trace_preserves_protected_shard.
Print Assumptions action_trace_preserves_per_shard_conservation.
Print Assumptions action_trace_preserves_per_shard_root_alignment.
Print Assumptions distinct_shard_actions_commute_pointwise.
Print Assumptions admitted_serial_commits_have_no_lost_updates.
Print Assumptions admitted_serial_commits_preserve_conservation.
Print Assumptions shared_worker_capstone.
