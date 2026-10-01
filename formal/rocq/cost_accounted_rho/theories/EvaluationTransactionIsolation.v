From Stdlib Require Import Arith.PeanoNat Lia.

Record evaluation_transaction := {
  prior_witness : nat;
  attempted_work : nat;
  base_state : nat;
  current_state : nat;
  active_checkpoint : nat;
  mergeable_evidence : nat
}.

Definition parser_failure_result
  (_ : evaluation_transaction)
  : nat := 0.

Definition reducer_failure_result
  (state : evaluation_transaction)
  : nat := attempted_work state.

Definition rollback_rejected_evaluation
  (state : evaluation_transaction)
  : evaluation_transaction :=
  {|
    prior_witness := prior_witness state;
    attempted_work := attempted_work state;
    base_state := base_state state;
    current_state := base_state state;
    active_checkpoint := base_state state;
    mergeable_evidence := 0
  |}.

Definition finalize_replay
  (state : evaluation_transaction)
  (final_state_matches : bool)
  : evaluation_transaction :=
  if final_state_matches then
    {|
      prior_witness := prior_witness state;
      attempted_work := attempted_work state;
      base_state := base_state state;
      current_state := current_state state;
      active_checkpoint := current_state state;
      mergeable_evidence := 1
    |}
  else rollback_rejected_evaluation state.

Theorem parser_failure_cannot_reuse_prior_witness :
  forall state,
    parser_failure_result state = 0.
Proof.
  reflexivity.
Qed.

Theorem reducer_failure_retains_exact_attempted_work :
  forall state,
    reducer_failure_result state = attempted_work state.
Proof.
  reflexivity.
Qed.

Theorem rejected_play_restores_its_base_state :
  forall state,
    current_state (rollback_rejected_evaluation state) = base_state state.
Proof.
  reflexivity.
Qed.

Theorem rejected_replay_restores_its_base_state :
  forall state,
    current_state (rollback_rejected_evaluation state) = base_state state.
Proof.
  reflexivity.
Qed.

Theorem rejected_replay_discards_its_prevalidation_checkpoint :
  forall state,
    active_checkpoint (rollback_rejected_evaluation state) = base_state state.
Proof.
  reflexivity.
Qed.

Theorem rollback_preserves_attempted_work :
  forall state,
    attempted_work (rollback_rejected_evaluation state) = attempted_work state.
Proof.
  reflexivity.
Qed.

Theorem rejected_replay_publishes_no_mergeable_evidence :
  forall state,
    mergeable_evidence (finalize_replay state false) = 0.
Proof.
  reflexivity.
Qed.

Theorem accepted_replay_publishes_mergeable_evidence :
  forall state,
    mergeable_evidence (finalize_replay state true) = 1.
Proof.
  reflexivity.
Qed.

Theorem published_mergeable_evidence_requires_final_state_match :
  forall state final_state_matches,
    mergeable_evidence (finalize_replay state final_state_matches) = 1 ->
    final_state_matches = true.
Proof.
  intros state final_state_matches evidence_published.
  destruct final_state_matches; simpl in *; try reflexivity; discriminate.
Qed.

Print Assumptions parser_failure_cannot_reuse_prior_witness.
Print Assumptions reducer_failure_retains_exact_attempted_work.
Print Assumptions rejected_play_restores_its_base_state.
Print Assumptions rejected_replay_restores_its_base_state.
Print Assumptions rejected_replay_discards_its_prevalidation_checkpoint.
Print Assumptions rollback_preserves_attempted_work.
Print Assumptions rejected_replay_publishes_no_mergeable_evidence.
Print Assumptions accepted_replay_publishes_mergeable_evidence.
Print Assumptions published_mergeable_evidence_requires_final_state_match.
