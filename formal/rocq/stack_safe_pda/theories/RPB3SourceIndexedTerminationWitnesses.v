From Stdlib Require Import List.
From StackSafePDA Require Import RPB3RecursiveAuxiliarySourceReference
  RPB3SemanticReducer RPB3SourceIndexedFrames RPB3SourceIndexedTermination
  RPB3AdmissionMutants.

Import ListNotations.

Module Recursive := RPB3RecursiveAuxiliarySourceReference.
Module Frames := RPB3SourceIndexedFrames.SourceIndexedFrames.
Module Termination := RPB3SourceIndexedTermination.
Module Witness := RPB3AdmissionMutants.SourceIndexedMutants.

Theorem ordered_new_terminal_accepts :
  forall final,
    Termination.reaches
      (Recursive.recursive_par_reference Witness.witness_hash)
      Witness.witness_hash Witness.ordered_new_body
      (Termination.root_configuration 20 Witness.ordered_new_body) final ->
    Termination.terminal final ->
    exists root, Termination.terminal_root_result final = Some root.
Proof.
  intros final path done.
  rewrite (Termination.terminal_root_result_for_any_reach
    Witness.witness_hash 20 Witness.ordered_new_body final path done).
  vm_compute. eexists. reflexivity.
Qed.

Theorem reversed_new_terminal_refuses :
  forall final,
    Termination.reaches
      (Recursive.recursive_par_reference Witness.witness_hash)
      Witness.witness_hash Witness.reversed_new_body
      (Termination.root_configuration 20 Witness.reversed_new_body) final ->
    Termination.terminal final ->
    Termination.terminal_root_result final = None.
Proof.
  intros final path done.
  rewrite (Termination.terminal_root_result_for_any_reach
    Witness.witness_hash 20 Witness.reversed_new_body final path done).
  vm_compute. reflexivity.
Qed.

Theorem trailing_new_terminal_refuses :
  forall final,
    Termination.reaches
      (Recursive.recursive_par_reference Witness.witness_hash)
      Witness.witness_hash Witness.trailing_new_body
      (Termination.root_configuration 20 Witness.trailing_new_body) final ->
    Termination.terminal final ->
    Termination.terminal_root_result final = None.
Proof.
  intros final path done.
  rewrite (Termination.terminal_root_result_for_any_reach
    Witness.witness_hash 20 Witness.trailing_new_body final path done).
  vm_compute. reflexivity.
Qed.

Theorem truncated_private_unforgeable_terminal_refuses :
  forall final,
    Termination.reaches
      (Recursive.recursive_par_reference Witness.witness_hash)
      Witness.witness_hash Witness.truncated_private_unforgeable_body
      (Termination.root_configuration 60
        Witness.truncated_private_unforgeable_body) final ->
    Termination.terminal final ->
    Termination.terminal_root_result final = None.
Proof.
  intros final path done.
  rewrite (Termination.terminal_root_result_for_any_reach
    Witness.witness_hash 60 Witness.truncated_private_unforgeable_body
    final path done).
  vm_compute. reflexivity.
Qed.

Theorem ordered_new_claimed_score_controls :
  Frames.source_indexed_root_claimed_score Witness.witness_hash 20
    Witness.ordered_new_body
    (RPB3SemanticReducer.canonical_score_bytes
      Recursive.ordered_two_absent_value_maps_new_score) =
      Frames.ClaimedScoreAccepted /\
  Frames.source_indexed_root_claimed_score Witness.witness_hash 20
    Witness.ordered_new_body
    (RPB3SemanticReducer.canonical_score_bytes
      Recursive.ordered_two_absent_value_maps_new_score ++ [0]) =
      Frames.ClaimedScoreRejected.
Proof.
  split.
  - exact Witness.source_indexed_ordered_new_accepts_exact_claimed_score.
  - exact Witness.source_indexed_ordered_new_rejects_forged_claimed_score.
Qed.

Print Assumptions ordered_new_terminal_accepts.
Print Assumptions reversed_new_terminal_refuses.
Print Assumptions trailing_new_terminal_refuses.
Print Assumptions truncated_private_unforgeable_terminal_refuses.
Print Assumptions ordered_new_claimed_score_controls.
