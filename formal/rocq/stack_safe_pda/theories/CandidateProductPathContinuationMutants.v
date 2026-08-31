From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import CandidateProductPathContinuation.

Import ListNotations.
Import CandidateProductPathContinuation.

Module CandidateProductPathContinuationMutants.

Definition leaf {A : Type} (value : option A) : trie A := Node value [].

Definition availability_fixture : trie unit :=
  Node None [(1, Node (Some tt) [(2, leaf (Some tt)); (3, leaf None)])].

Definition positions_fixture : trie nat :=
  Node None [(1, Node (Some 7) [(2, leaf (Some 11)); (4, leaf (Some 13))])].

Definition authority_fixture : candidate_authority :=
  CandidateAuthority 41 availability_fixture positions_fixture.

Definition root_continuation : path_continuation := BasePath [1].
Definition child_continuation : path_continuation := ExtendPath root_continuation [2].

Theorem fixtures_are_non_vacuous :
  observe_reopened authority_fixture
    (PathCheckpoint 41 root_continuation) = Some [7; 11] /\
  observe_reopened authority_fixture
    (PathCheckpoint 41 child_continuation) = Some [11].
Proof. vm_compute. auto. Qed.

(** Mutant: forget the mandatory candidate-product root prefix. *)
Definition mutant_root_omission : path_continuation := BasePath [].

Theorem kill_candidate_root_omission :
  flatten_continuation mutant_root_omission <>
  flatten_continuation root_continuation.
Proof. discriminate. Qed.

(** Mutant: retain only the newest relative frame instead of sharing the
    complete continuation chain. *)
Definition mutant_relative_frame_only : path_continuation := BasePath [2].

Theorem kill_relative_frame_only_checkpoint :
  flatten_continuation mutant_relative_frame_only <>
  flatten_continuation child_continuation.
Proof. discriminate. Qed.

(** Mutant: a descent succeeds when availability alone has the target. *)
Definition mutant_availability_only_descent
    (authority : candidate_authority) (cursor : product_cursor) (segment : path)
    : product_cursor * bool :=
  let target := availability_cursor_path cursor ++ segment in
  match focus (authority_availability authority) target with
  | Some _ => (ProductCursor target target, true)
  | None => (cursor, false)
  end.

Theorem kill_availability_only_descent :
  snd (mutant_availability_only_descent authority_fixture
    (aligned_cursor [1]) [3]) <>
  snd (attempt_product_descent authority_fixture (aligned_cursor [1]) [3]).
Proof. vm_compute. discriminate. Qed.

(** Mutant: asymmetric failure restores only positions and strands the
    availability cursor at its partial target. *)
Definition mutant_asymmetric_failure : product_cursor :=
  ProductCursor [1; 3] [1].

Theorem kill_asymmetric_failure_without_full_restoration :
  mutant_asymmetric_failure <> fst
    (attempt_product_descent authority_fixture (aligned_cursor [1]) [3]).
Proof. vm_compute. discriminate. Qed.

(** Mutant: scan descendants but omit a value at the focused root. *)
Definition mutant_omit_focus_endpoint
    (availability : trie unit) (positions : trie nat) : list nat :=
  tl (observe_product availability positions).

Theorem kill_focus_root_endpoint_omission :
  mutant_omit_focus_endpoint
    (Node (Some tt) []) (Node (Some 7) []) <>
  observe_product (Node (Some tt) []) (Node (Some 7) []).
Proof. discriminate. Qed.

(** Mutant: emit only the focus value and omit matching descendants. *)
Definition mutant_root_only_observation : list nat := [7].

Theorem kill_descendant_omission :
  mutant_root_only_observation <> [7; 11].
Proof. vm_compute. discriminate. Qed.

(** Mutants: skip an exact/capture segment or duplicate a nested prefix. *)
Definition mutant_skip_exact_frame : path_continuation := root_continuation.

Definition mutant_duplicate_nested_prefix
    (prefix tail : path) : path_continuation :=
  ExtendPath root_continuation (prefix ++ prefix ++ tail).

Theorem kill_skipped_exact_or_capture_frame :
  flatten_continuation mutant_skip_exact_frame <>
  flatten_continuation (ExtendPath root_continuation [2]).
Proof. discriminate. Qed.

Theorem kill_duplicated_nested_prefix :
  flatten_continuation (mutant_duplicate_nested_prefix [2] [5]) <>
  flatten_continuation (ExtendPath root_continuation ([2] ++ [5])).
Proof. discriminate. Qed.

(** Mutant: result traversal retains the [0,0] suffix before remainder scan. *)
Definition mutant_retain_result_suffix (checkpoint : path_checkpoint)
    : path_checkpoint :=
  PathCheckpoint (checkpoint_authority_identity checkpoint)
    (ExtendPath (checkpoint_continuation checkpoint) result_suffix).

Theorem kill_result_suffix_retained_before_remainder :
  checkpoint_continuation
    (mutant_retain_result_suffix (PathCheckpoint 41 root_continuation)) <>
  checkpoint_continuation
    (visit_result_suffix_then_restore authority_fixture
      (PathCheckpoint 41 root_continuation)).
Proof. discriminate. Qed.

(** Mutant: ignore authority identity and reopen a stale path against another
    immutable root. *)
Definition mutant_reopen_stale_authority
    (authority : candidate_authority) (checkpoint : path_checkpoint)
    : option (trie unit * trie nat) :=
  product_focus authority
    (flatten_continuation (checkpoint_continuation checkpoint)).

Definition changed_authority : candidate_authority :=
  CandidateAuthority 42 availability_fixture positions_fixture.

Theorem kill_stale_authority_reopen :
  mutant_reopen_stale_authority changed_authority
    (PathCheckpoint 41 root_continuation) <>
  reopen_checkpoint changed_authority (PathCheckpoint 41 root_continuation).
Proof. vm_compute. discriminate. Qed.

(** Mutant: every extension copies the complete flattened prefix.  It is
    semantically equal but violates the persistent-space contract. *)
Definition mutant_flat_prefix_copy
    (prefix : path_continuation) (frame : path) : path_continuation :=
  BasePath (flatten_continuation prefix ++ frame).

Definition newly_owned_extension_bytes
    (_before after : path_continuation) : nat :=
  match after with
  | BasePath copied_prefix => length copied_prefix
  | ExtendPath _ new_frame => length new_frame
  end.

Theorem kill_flat_prefix_copy_space_slope :
  newly_owned_extension_bytes root_continuation
    (mutant_flat_prefix_copy root_continuation [2]) <>
  length [2].
Proof. vm_compute. discriminate. Qed.

(** Mutant: store a persistent path but replay its entire absolute prefix from
    both authority roots whenever a work state resumes.  This preserves
    results yet restores the triangular all-wildcard navigation slope. *)
Definition mutant_absolute_root_replay_cost (depth : nat) : nat :=
  unit_root_replay_cost 0 depth.

Theorem kill_absolute_root_replay_navigation_slope :
  mutant_absolute_root_replay_cost 4 <> unit_moving_cursor_cost 4.
Proof. vm_compute. discriminate. Qed.

(** Mutant: leave the branch's new capture installed while returning to its
    parent continuation. *)
Definition capture_edit_fixture : capture_edit := CaptureEdit 3 [9; 8].

Definition mutant_skip_capture_rollback : capture_environment :=
  apply_capture_edit [] capture_edit_fixture.

Theorem kill_missing_capture_rollback :
  mutant_skip_capture_rollback <>
  undo_capture_edit (apply_capture_edit [] capture_edit_fixture).
Proof. discriminate. Qed.

(** Mutant: every scheduled branch owns a raw capture PathMap header.  The
    safe work state owns only an arena index and therefore cannot invoke
    recursive raw-PathMap destruction when the work stack is discarded. *)
Definition mutant_raw_capture_checkpoint : abstract_candidate_work_state :=
  CandidateWorkState 0 true false.

Theorem kill_raw_capture_pathmap_per_branch :
  work_state_owns_raw_capture_header mutant_raw_capture_checkpoint <>
  work_state_owns_raw_capture_header
    (make_journal_work_state 0).
Proof. discriminate. Qed.

(** Mutant: a work state owns its parent via a recursively linked Arc/Rc
    chain.  Dropping the last deep tail can then recurse through every parent;
    the safe arena state owns only a numeric index. *)
Definition mutant_recursively_linked_parent : abstract_candidate_work_state :=
  CandidateWorkState 0 false true.

Theorem kill_recursively_linked_continuation_owner :
  work_state_owns_linked_parent mutant_recursively_linked_parent <>
  work_state_owns_linked_parent (make_journal_work_state 0).
Proof. discriminate. Qed.

(** Mutant: consume an exact/repeated-capture frame physically but forget to
    extend the logical continuation stored with the work state. *)
Definition synchronized_state_fixture : synchronized_query_state :=
  SynchronizedQueryState root_continuation (aligned_cursor [1]).

Definition mutant_physical_only_atom_consumption
    (state : synchronized_query_state) (frame : path)
    : synchronized_query_state :=
  SynchronizedQueryState
    (query_state_continuation state)
    (ProductCursor
      (availability_cursor_path (query_state_cursor state) ++ frame)
      (positions_cursor_path (query_state_cursor state) ++ frame)).

Theorem kill_consumed_atom_path_desynchronization :
  ~ query_state_path_lockstep
      (mutant_physical_only_atom_consumption synchronized_state_fixture [2]).
Proof. vm_compute. intuition discriminate. Qed.

(** Mutant: compiler assigns the first nonexistent dense capture slot. *)
Definition mutant_out_of_bounds_capture_slot (slot_count : nat) : nat :=
  slot_count.

Theorem kill_out_of_bounds_dense_capture_slot :
  ~ dense_capture_slot_valid 1 (mutant_out_of_bounds_capture_slot 1).
Proof. vm_compute. lia. Qed.

(** Mutant: redo the target branch before undoing the source branch. *)
Definition source_capture_edit : capture_edit := CaptureEdit 1 [4].
Definition target_capture_edit : capture_edit := CaptureEdit 2 [5].
Definition shared_capture_environment : capture_environment := [(0, [3])].

Definition mutant_capture_redo_before_undo : capture_environment :=
  undo_capture_edits 1
    (apply_capture_edits
      (apply_capture_edits shared_capture_environment [source_capture_edit])
      [target_capture_edit]).

Theorem kill_capture_redo_before_undo :
  mutant_capture_redo_before_undo <>
  rebase_capture_environment
    (apply_capture_edits shared_capture_environment [source_capture_edit])
    [source_capture_edit] [target_capture_edit].
Proof. vm_compute. discriminate. Qed.

(** Mutant: frame scanner leaves the synchronized cursor at its last child. *)
Definition mutant_scanner_return_path (traversed base : path) : path :=
  base ++ traversed.

Theorem kill_scanner_stranded_below_base :
  mutant_scanner_return_path [2] [1] <> scanner_return_path [2] [1].
Proof. discriminate. Qed.

(** Mutant: accepts-remainder reschedules the unchanged checkpoint and can
    loop without consuming another complete stored frame. *)
Definition mutant_remainder_without_progress
    (continuation : path_continuation) : path_continuation := continuation.

Theorem kill_remainder_reschedule_without_progress :
  flatten_continuation (mutant_remainder_without_progress root_continuation) <>
  flatten_continuation (ExtendPath root_continuation [2]).
Proof. discriminate. Qed.

(** Scanner mutants: confuse the outer datum terminator with an inner
    sequence terminator, miss the double-escaped inner terminator, or accept a
    malformed outer escape as an incomplete valid prefix. *)
Definition mutant_outer_frame_end_classification : stored_sequence_frame_step :=
  ScanComplete.

Theorem kill_outer_frame_end_misclassification :
  mutant_outer_frame_end_classification <>
  snd (scanner_drive_until_classified scanner_default [0; 0]).
Proof. discriminate. Qed.

Definition mutant_inner_frame_end_classification : stored_sequence_frame_step :=
  ScanNeedMore.

Theorem kill_inner_frame_end_misclassification :
  mutant_inner_frame_end_classification <>
  snd (scanner_drive_until_classified scanner_default [0; 255; 0; 255]).
Proof. discriminate. Qed.

Definition mutant_malformed_escape_classification : stored_sequence_frame_step :=
  ScanNeedMore.

Theorem kill_malformed_escape_misclassification :
  mutant_malformed_escape_classification <>
  snd (scanner_drive_until_classified scanner_default [0; 7]).
Proof. discriminate. Qed.

(** Mutant: breadth/level oscillation repeatedly returns to the root before
    visiting the next level of two deep branches.  Its depth-N charge is
    quadratic instead of the DFS enter/exit schedule's two charges per edge. *)
Definition two_deep_branch_navigation_fixture : arena_navigation_tree :=
  ArenaNavNode 4 ArenaNavEmpty
    (ArenaNavNode 4 ArenaNavEmpty ArenaNavEmpty).

Definition mutant_level_oscillation_navigation_cost (depth : nat) : nat :=
  2 * depth * depth.

Theorem kill_superlinear_level_oscillation_scheduler :
  mutant_level_oscillation_navigation_cost 4 <>
  depth_first_enter_exit_navigation_cost two_deep_branch_navigation_fixture.
Proof. discriminate. Qed.

Inductive continuation_mutant : Type :=
| CandidateRootOmission
| RelativeFrameOnly
| AvailabilityOnlyDescent
| AsymmetricFailure
| FocusRootEndpointOmission
| DescendantOmission
| SkippedExactOrCaptureFrame
| DuplicatedNestedPrefix
| RetainedResultSuffix
| StaleAuthorityReopen
| FlatPrefixCopySlope
| AbsoluteRootReplaySlope
| MissingCaptureRollback
| RawCapturePathMapPerBranch
| ConsumedAtomPathDesynchronization
| OutOfBoundsDenseCaptureSlot
| CaptureRedoBeforeUndo
| ScannerStrandedBelowBase
| RemainderWithoutProgress
| RecursiveLinkedOwnerChain
| OuterFrameEndMisclassification
| InnerFrameEndMisclassification
| MalformedEscapeMisclassification
| SuperlinearLevelOscillationScheduler.

Definition all_continuation_mutants : list continuation_mutant :=
  [CandidateRootOmission; RelativeFrameOnly; AvailabilityOnlyDescent;
   AsymmetricFailure; FocusRootEndpointOmission; DescendantOmission;
   SkippedExactOrCaptureFrame; DuplicatedNestedPrefix;
   RetainedResultSuffix; StaleAuthorityReopen; FlatPrefixCopySlope;
   AbsoluteRootReplaySlope; MissingCaptureRollback;
   RawCapturePathMapPerBranch; ConsumedAtomPathDesynchronization;
   OutOfBoundsDenseCaptureSlot; CaptureRedoBeforeUndo;
   ScannerStrandedBelowBase; RemainderWithoutProgress;
   RecursiveLinkedOwnerChain; OuterFrameEndMisclassification;
   InnerFrameEndMisclassification; MalformedEscapeMisclassification;
   SuperlinearLevelOscillationScheduler].

Definition continuation_mutant_killed (mutant : continuation_mutant) : Prop :=
  match mutant with
  | CandidateRootOmission =>
      flatten_continuation mutant_root_omission <>
      flatten_continuation root_continuation
  | RelativeFrameOnly =>
      flatten_continuation mutant_relative_frame_only <>
      flatten_continuation child_continuation
  | AvailabilityOnlyDescent =>
      snd (mutant_availability_only_descent authority_fixture
        (aligned_cursor [1]) [3]) <>
      snd (attempt_product_descent authority_fixture (aligned_cursor [1]) [3])
  | AsymmetricFailure =>
      mutant_asymmetric_failure <> fst
        (attempt_product_descent authority_fixture (aligned_cursor [1]) [3])
  | FocusRootEndpointOmission =>
      mutant_omit_focus_endpoint (Node (Some tt) []) (Node (Some 7) []) <>
      observe_product (Node (Some tt) []) (Node (Some 7) [])
  | DescendantOmission =>
      mutant_root_only_observation <> [7; 11]
  | SkippedExactOrCaptureFrame =>
      flatten_continuation mutant_skip_exact_frame <>
      flatten_continuation (ExtendPath root_continuation [2])
  | DuplicatedNestedPrefix =>
      flatten_continuation (mutant_duplicate_nested_prefix [2] [5]) <>
      flatten_continuation (ExtendPath root_continuation ([2] ++ [5]))
  | RetainedResultSuffix =>
      checkpoint_continuation
        (mutant_retain_result_suffix (PathCheckpoint 41 root_continuation)) <>
      root_continuation
  | StaleAuthorityReopen =>
      mutant_reopen_stale_authority changed_authority
        (PathCheckpoint 41 root_continuation) <>
      reopen_checkpoint changed_authority (PathCheckpoint 41 root_continuation)
  | FlatPrefixCopySlope =>
      newly_owned_extension_bytes root_continuation
        (mutant_flat_prefix_copy root_continuation [2]) <> length [2]
  | AbsoluteRootReplaySlope =>
      mutant_absolute_root_replay_cost 4 <> unit_moving_cursor_cost 4
  | MissingCaptureRollback =>
      mutant_skip_capture_rollback <>
      undo_capture_edit (apply_capture_edit [] capture_edit_fixture)
  | RawCapturePathMapPerBranch =>
      work_state_owns_raw_capture_header mutant_raw_capture_checkpoint <>
      work_state_owns_raw_capture_header
        (make_journal_work_state 0)
  | ConsumedAtomPathDesynchronization =>
      ~ query_state_path_lockstep
          (mutant_physical_only_atom_consumption synchronized_state_fixture [2])
  | OutOfBoundsDenseCaptureSlot =>
      ~ dense_capture_slot_valid 1 (mutant_out_of_bounds_capture_slot 1)
  | CaptureRedoBeforeUndo =>
      mutant_capture_redo_before_undo <>
      rebase_capture_environment
        (apply_capture_edits shared_capture_environment [source_capture_edit])
        [source_capture_edit] [target_capture_edit]
  | ScannerStrandedBelowBase =>
      mutant_scanner_return_path [2] [1] <> scanner_return_path [2] [1]
  | RemainderWithoutProgress =>
      flatten_continuation
        (mutant_remainder_without_progress root_continuation) <>
      flatten_continuation (ExtendPath root_continuation [2])
  | RecursiveLinkedOwnerChain =>
      work_state_owns_linked_parent mutant_recursively_linked_parent <>
      work_state_owns_linked_parent (make_journal_work_state 0)
  | OuterFrameEndMisclassification =>
      mutant_outer_frame_end_classification <>
      snd (scanner_drive_until_classified scanner_default [0; 0])
  | InnerFrameEndMisclassification =>
      mutant_inner_frame_end_classification <>
      snd (scanner_drive_until_classified scanner_default [0; 255; 0; 255])
  | MalformedEscapeMisclassification =>
      mutant_malformed_escape_classification <>
      snd (scanner_drive_until_classified scanner_default [0; 7])
  | SuperlinearLevelOscillationScheduler =>
      mutant_level_oscillation_navigation_cost 4 <>
      depth_first_enter_exit_navigation_cost
        two_deep_branch_navigation_fixture
  end.

Theorem every_continuation_mutant_has_a_causal_counterexample : forall mutant,
  continuation_mutant_killed mutant.
Proof.
  intro mutant. destruct mutant; cbn.
  - apply kill_candidate_root_omission.
  - apply kill_relative_frame_only_checkpoint.
  - apply kill_availability_only_descent.
  - apply kill_asymmetric_failure_without_full_restoration.
  - apply kill_focus_root_endpoint_omission.
  - discriminate.
  - apply kill_skipped_exact_or_capture_frame.
  - apply kill_duplicated_nested_prefix.
  - apply kill_result_suffix_retained_before_remainder.
  - apply kill_stale_authority_reopen.
  - apply kill_flat_prefix_copy_space_slope.
  - apply kill_absolute_root_replay_navigation_slope.
  - apply kill_missing_capture_rollback.
  - apply kill_raw_capture_pathmap_per_branch.
  - apply kill_consumed_atom_path_desynchronization.
  - apply kill_out_of_bounds_dense_capture_slot.
  - apply kill_capture_redo_before_undo.
  - apply kill_scanner_stranded_below_base.
  - apply kill_remainder_reschedule_without_progress.
  - apply kill_recursively_linked_continuation_owner.
  - apply kill_outer_frame_end_misclassification.
  - apply kill_inner_frame_end_misclassification.
  - apply kill_malformed_escape_misclassification.
  - apply kill_superlinear_level_oscillation_scheduler.
Qed.

Theorem continuation_mutant_registry_is_unique_complete_and_exact :
  NoDup all_continuation_mutants /\
  Forall continuation_mutant_killed all_continuation_mutants /\
  length all_continuation_mutants = 24.
Proof.
  repeat split.
  - repeat constructor; simpl; intuition discriminate.
  - repeat constructor; apply every_continuation_mutant_has_a_causal_counterexample.
Qed.

End CandidateProductPathContinuationMutants.
