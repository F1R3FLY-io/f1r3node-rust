From Stdlib Require Import List Arith Lia.

Import ListNotations.

Inductive SnapshotSource : Type :=
| OrdinaryRuntime
| ReplayRuntime.

Inductive replay_prefix : nat -> list nat -> list SnapshotSource -> Prop :=
| replay_prefix_zero : replay_prefix 0 [0] []
| replay_prefix_step :
    forall count roots sources,
      replay_prefix count roots sources ->
      replay_prefix
        (S count)
        (roots ++ [S count])
        (sources ++ [OrdinaryRuntime]).

Lemma replay_prefix_shape :
  forall count roots sources,
    replay_prefix count roots sources ->
    roots = seq 0 (S count) /\
    sources = repeat OrdinaryRuntime count.
Proof.
  intros count roots sources replay.
  induction replay.
  - split; reflexivity.
  - destruct IHreplay as [roots_shape sources_shape].
    subst roots sources.
    split.
    + rewrite (seq_S (S count) 0).
      replace (0 + S count) with (S count) by lia.
      reflexivity.
    + simpl.
      rewrite repeat_cons.
      reflexivity.
Qed.

Theorem pre_state_materialized_before_snapshot :
  forall count roots sources index,
    replay_prefix count roots sources ->
    index <= count ->
    In index roots.
Proof.
  intros count roots sources index replay within_prefix.
  pose proof (replay_prefix_shape _ _ _ replay) as [roots_shape _].
  subst roots.
  apply in_seq.
  lia.
Qed.

Theorem next_snapshot_pre_state_is_materialized :
  forall count roots sources,
    replay_prefix count roots sources ->
    In count roots.
Proof.
  intros count roots sources replay.
  eapply pre_state_materialized_before_snapshot; eauto.
Qed.

Theorem all_snapshots_use_ordinary_runtime :
  forall count roots sources,
    replay_prefix count roots sources ->
    Forall (fun source => source = OrdinaryRuntime) sources.
Proof.
  intros count roots sources replay.
  pose proof (replay_prefix_shape _ _ _ replay) as [_ sources_shape].
  subst sources.
  apply Forall_forall.
  intros source contained.
  exact (repeat_spec count OrdinaryRuntime source contained).
Qed.

Theorem accepted_post_state_is_exact :
  forall count roots sources,
    replay_prefix count roots sources ->
    last roots 0 = count.
Proof.
  intros count roots sources replay.
  pose proof (replay_prefix_shape _ _ _ replay) as [roots_shape _].
  subst roots.
  rewrite seq_S.
  rewrite last_last.
  lia.
Qed.

Theorem independent_validator_replay_agrees :
  forall count left_roots left_sources right_roots right_sources,
    replay_prefix count left_roots left_sources ->
    replay_prefix count right_roots right_sources ->
    last left_roots 0 = last right_roots 0.
Proof.
  intros count left_roots left_sources right_roots right_sources left right.
  rewrite (accepted_post_state_is_exact _ _ _ left).
  rewrite (accepted_post_state_is_exact _ _ _ right).
  reflexivity.
Qed.

Theorem eager_second_snapshot_is_not_materialized_by_genesis :
  ~ In 1 [0].
Proof.
  simpl.
  lia.
Qed.

Theorem replay_runtime_is_not_an_ordinary_snapshot_source :
  ReplayRuntime <> OrdinaryRuntime.
Proof.
  discriminate.
Qed.
