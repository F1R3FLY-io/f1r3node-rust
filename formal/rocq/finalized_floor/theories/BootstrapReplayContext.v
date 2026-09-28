From Stdlib Require Import Lists.List.
Import ListNotations.

Section BootstrapReplay.

Context {Context Root : Type}.
Variable replay : Context -> Root -> Root.

Record ConsensusBlock := {
  block_context : Context;
  block_pre_root : Root;
  block_post_root : Root;
  block_replay_valid : replay block_context block_pre_root = block_post_root
}.

Definition replay_from_consensus_data (block : ConsensusBlock) : Root :=
  replay (block_context block) (block_pre_root block).

Definition replay_history (history : list ConsensusBlock) : list Root :=
  map replay_from_consensus_data history.

Definition declared_history_roots (history : list ConsensusBlock) : list Root :=
  map block_post_root history.

Theorem consensus_block_replay_matches_declared_root :
  forall block,
    replay_from_consensus_data block = block_post_root block.
Proof.
  intros block.
  destruct block as [context pre post valid].
  exact valid.
Qed.

Theorem consensus_history_replay_matches_declared_roots :
  forall history,
    replay_history history = declared_history_roots history.
Proof.
  intros history.
  induction history as [| block tail IH].
  - reflexivity.
  - simpl.
    rewrite consensus_block_replay_matches_declared_root.
    rewrite IH.
    reflexivity.
Qed.

End BootstrapReplay.

Definition boolean_replay (context root : bool) : bool := xorb context root.

Definition boolean_consensus_block : @ConsensusBlock bool bool boolean_replay.
Proof.
  refine {| block_context := false;
            block_pre_root := false;
            block_post_root := false |}.
  reflexivity.
Defined.

Example ambient_context_replay_can_diverge :
  boolean_replay true (block_pre_root boolean_replay boolean_consensus_block) <>
  block_post_root boolean_replay boolean_consensus_block.
Proof.
  discriminate.
Qed.
