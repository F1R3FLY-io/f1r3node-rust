From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

Section Materialization.

Context {Block : Type}.
Variable block_eq_dec : forall left right : Block, {left = right} + {left <> right}.

Definition cache_equiv (left right : list Block) : Prop :=
  forall block, In block left <-> In block right.

Definition materialize_one (block : Block) (cache : list Block) : list Block :=
  if in_dec block_eq_dec block cache then cache else block :: cache.

Definition materialize_all (required cache : list Block) : list Block :=
  fold_right materialize_one cache required.

Definition snapshot_required (parents latest_messages : list Block) : list Block :=
  parents ++ latest_messages.

Definition snapshot_ready (parents latest_messages cache : list Block) : Prop :=
  forall block,
    In block (snapshot_required parents latest_messages) -> In block cache.

Lemma materialize_one_membership :
  forall candidate block cache,
    In candidate (materialize_one block cache) <->
    candidate = block \/ In candidate cache.
Proof.
  intros candidate block cache.
  unfold materialize_one.
  destruct (in_dec block_eq_dec block cache) as [Hpresent | Habsent].
  - split.
    + intros Hin. right. exact Hin.
    + intros [Heq | Hin].
      * subst candidate. exact Hpresent.
      * exact Hin.
  - simpl.
    split.
    + intros [Heq | Hin].
      * left. symmetry. exact Heq.
      * right. exact Hin.
    + intros [Heq | Hin].
      * left. symmetry. exact Heq.
      * right. exact Hin.
Qed.

Lemma materialize_all_membership :
  forall candidate required cache,
    In candidate (materialize_all required cache) <->
    In candidate required \/ In candidate cache.
Proof.
  intros candidate required.
  induction required as [|block tail IH]; intros cache.
  - simpl. tauto.
  - simpl.
    rewrite materialize_one_membership.
    rewrite IH.
    simpl.
    firstorder congruence.
Qed.

Theorem snapshot_materialization_complete :
  forall parents latest_messages cache,
    snapshot_ready
      parents latest_messages
      (materialize_all (snapshot_required parents latest_messages) cache).
Proof.
  intros parents latest_messages cache block Hrequired.
  apply materialize_all_membership.
  left.
  exact Hrequired.
Qed.

Theorem snapshot_materialization_preserves_cache :
  forall parents latest_messages cache block,
    In block cache ->
    In block (materialize_all (snapshot_required parents latest_messages) cache).
Proof.
  intros parents latest_messages cache block Hcached.
  apply materialize_all_membership.
  right.
  exact Hcached.
Qed.

Theorem materialization_order_transparent :
  forall left right cache,
    Permutation left right ->
    cache_equiv (materialize_all left cache) (materialize_all right cache).
Proof.
  intros left right cache Hpermutation block.
  repeat rewrite materialize_all_membership.
  split.
  - intros [Hin | Hcached].
    + left. eapply Permutation_in; eauto.
    + right. exact Hcached.
  - intros [Hin | Hcached].
    + left. eapply Permutation_in.
      * symmetry. exact Hpermutation.
      * exact Hin.
    + right. exact Hcached.
Qed.

Theorem concurrent_materialization_commutes :
  forall snapshot_blocks finalizer_blocks cache,
    cache_equiv
      (materialize_all snapshot_blocks
        (materialize_all finalizer_blocks cache))
      (materialize_all finalizer_blocks
        (materialize_all snapshot_blocks cache)).
Proof.
  intros snapshot_blocks finalizer_blocks cache block.
  repeat rewrite materialize_all_membership.
  tauto.
Qed.

Theorem snapshot_materialization_idempotent :
  forall required cache,
    cache_equiv
      (materialize_all required (materialize_all required cache))
      (materialize_all required cache).
Proof.
  intros required cache block.
  repeat rewrite materialize_all_membership.
  tauto.
Qed.

Theorem finalized_floor_snapshot_materialization_correct :
  forall parents latest_messages finalizer_blocks cache,
    snapshot_ready
      parents latest_messages
      (materialize_all
        (snapshot_required parents latest_messages)
        (materialize_all finalizer_blocks cache))
    /\
    cache_equiv
      (materialize_all
        (snapshot_required parents latest_messages)
        (materialize_all finalizer_blocks cache))
      (materialize_all finalizer_blocks
        (materialize_all
          (snapshot_required parents latest_messages)
          cache))
    /\
    cache_equiv
      (materialize_all
        (snapshot_required parents latest_messages)
        (materialize_all
          (snapshot_required parents latest_messages)
          cache))
      (materialize_all
        (snapshot_required parents latest_messages)
        cache).
Proof.
  intros parents latest_messages finalizer_blocks cache.
  split.
  - apply snapshot_materialization_complete.
  - split.
    + apply concurrent_materialization_commutes.
    + apply snapshot_materialization_idempotent.
Qed.

End Materialization.

Inductive witness_block : Type :=
| ParentBlock
| OffParentLatestBlock.

Definition witness_block_eq_dec :
  forall left right : witness_block, {left = right} + {left <> right}.
Proof.
  decide equality.
Defined.

Theorem parent_only_materialization_is_incomplete :
  ~ snapshot_ready
      [ParentBlock]
      [OffParentLatestBlock]
      (materialize_all witness_block_eq_dec [ParentBlock] []).
Proof.
  unfold snapshot_ready, snapshot_required.
  intros Hready.
  assert (Hin : In OffParentLatestBlock ([ParentBlock] ++ [OffParentLatestBlock])).
  { simpl. tauto. }
  specialize (Hready OffParentLatestBlock Hin).
  simpl in Hready.
  destruct Hready as [Heq | Habsurd].
  - discriminate Heq.
  - contradiction.
Qed.

Print Assumptions finalized_floor_snapshot_materialization_correct.
Print Assumptions parent_only_materialization_is_incomplete.
