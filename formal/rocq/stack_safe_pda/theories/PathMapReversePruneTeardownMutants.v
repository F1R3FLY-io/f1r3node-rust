From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import PathMapReversePruneTeardown.

Import ListNotations.
Import PathMapReversePruneTeardown.

(** Each negative control changes one ownership, traversal, pruning, or
    publication decision.  The witnesses are intentionally small; the main
    theory separately quantifies over arbitrary finite tries and provides
    unbounded line, dense, and comb topology families. *)
Module PathMapReversePruneTeardownMutants.

Definition valued_line : trie :=
  Node (Some (SetMember 10))
    (FCons (Node (Some (BagMultiplicity 20 3)) FNil) FNil).

Definition root_par_trie : trie :=
  Node (Some (MapPar 7 [11; 12]))
    (FCons (Node (Some (SetMember 30)) FNil) FNil).

Definition topology_only_dense : trie := topology_dense 4.
Definition topology_only_comb : trie := topology_comb 3.

Theorem adversarial_fixtures_are_non_vacuous :
  tree_edges topology_only_dense = 4 /\
  tree_edges topology_only_comb = 6 /\
  reverse_payloads_tree topology_only_dense = [] /\
  reverse_payloads_tree topology_only_comb = [].
Proof. vm_compute. auto. Qed.

(** Mutant: enumerate only stored values.  Topology-only leaves and tails are
    then invisible and remain owned by recursive PathMap Drop. *)
Definition mutant_value_only_prune_widths (subject : trie) : list nat :=
  repeat 1 (length (reverse_payloads_tree subject)).

Theorem kill_value_only_walk_on_topology_only_comb :
  mutant_value_only_prune_widths topology_only_comb <>
  prune_widths (drain_tree topology_only_comb).
Proof. vm_compute. discriminate. Qed.

(** Mutant: an absent parent is not guarded, so one detach spans two logical
    edges instead of the exact immediate child edge. *)
Definition mutant_missing_barrier : list teardown_event := [PruneSpan 2].

Theorem kill_missing_immediate_parent_barrier :
  ~ branch_safe_segment false mutant_missing_barrier.
Proof. unfold branch_safe_segment, mutant_missing_barrier. discriminate. Qed.

(** Mutant: the barrier is installed after pruning rather than before it. *)
Definition mutant_wrong_level_barrier : list teardown_event :=
  [PruneSpan 1; InstallBarrier; DetachBarrier].

Theorem kill_wrong_level_or_late_barrier :
  ~ branch_safe_segment false mutant_wrong_level_barrier.
Proof. unfold branch_safe_segment, mutant_wrong_level_barrier. discriminate. Qed.

(** Mutant: a destruction-only barrier overwrites an existing real value. *)
Definition mutant_overwrite_real_parent : list teardown_event :=
  [InstallBarrier; PruneSpan 1; DetachBarrier].

Theorem kill_real_parent_value_overwrite :
  ~ branch_safe_segment true mutant_overwrite_real_parent.
Proof. unfold branch_safe_segment, mutant_overwrite_real_parent. discriminate. Qed.

(** Mutant: [remove_val(true)] is used at a terminal and recursively prunes a
    unary suffix. *)
Definition mutant_remove_value_with_recursive_prune : list teardown_event :=
  [PruneSpan 2].

Theorem kill_remove_value_with_recursive_prune :
  prune_widths mutant_remove_value_with_recursive_prune <> [1].
Proof. vm_compute. discriminate. Qed.

(** Mutants: a value endpoint is skipped or destroyed twice. *)
Definition mutant_skip_first_payload : list payload :=
  tl (reverse_payloads_tree valued_line).

Definition mutant_duplicate_first_payload : list payload :=
  match reverse_payloads_tree valued_line with
  | [] => []
  | first :: rest => first :: first :: rest
  end.

Theorem kill_skipped_payload :
  mutant_skip_first_payload <> detached_payloads (drain_tree valued_line).
Proof. vm_compute. discriminate. Qed.

Theorem kill_duplicated_payload :
  mutant_duplicate_first_payload <> detached_payloads (drain_tree valued_line).
Proof. vm_compute. discriminate. Qed.

(** Mutant: root-value handling assumes all values have nonempty paths. *)
Definition mutant_omit_root_payload : list payload :=
  firstn 1 (reverse_payloads_tree valued_line).

Theorem kill_root_payload_omission :
  mutant_omit_root_payload <> detached_payloads (drain_tree valued_line).
Proof. vm_compute. discriminate. Qed.

(** Mutant: the last owner hands a nonempty trie directly to ordinary Drop. *)
Definition mutant_raw_unique_release (subject : trie) : release_result :=
  ReleaseResult [] (tree_edges subject) (tree_edges subject) None.

Theorem kill_raw_unique_release :
  residual_campaign_edges (mutant_raw_unique_release topology_only_comb) <> 0 /\
  recursively_dropped_unique_edges
    (mutant_raw_unique_release topology_only_comb) <> 0.
Proof. vm_compute. split; discriminate. Qed.

(** Mutant: two wrappers independently snapshot [strong_count = 2], both
    choose the nonlast path, and therefore neither executes anchor teardown. *)
Definition mutant_strong_count_snapshot_race : list anchor_event :=
  [DropNonlastWrapperArc; DropNonlastWrapperArc].

Definition safe_two_wrapper_drop_trace : list anchor_event :=
  anchor_transition_trace (nonlast_wrapper_drop 0) ++
  anchor_transition_trace final_wrapper_drop.

Theorem kill_snapshot_only_strong_count_race :
  mutant_strong_count_snapshot_race <> safe_two_wrapper_drop_trace.
Proof. vm_compute. discriminate. Qed.

(** Mutant: wrapper Clone also clones the raw PathMap header and its root
    value.  The anchor design permits exactly one of each immutable version. *)
Definition mutant_per_wrapper_raw_header_clone : anchor_state :=
  AnchorState 2 2 2.

Theorem kill_per_wrapper_raw_header_and_root_value_clone :
  mutant_per_wrapper_raw_header_clone <>
  anchor_transition_final (immutable_wrapper_clone 0).
Proof. discriminate. Qed.

(** Mutant: a manual family counter retains per-wrapper raw-root lifecycle
    work on a nonlast Drop.  The Arc-only wrapper must touch no raw header. *)
Definition mutant_manual_family_nonlast_trace : list anchor_event :=
  [CloneRawHeaderForFreshAnchor; DropNonlastWrapperArc].

Theorem kill_manual_family_count_protocol :
  mutant_manual_family_nonlast_trace <>
  anchor_transition_trace (nonlast_wrapper_drop 0).
Proof. discriminate. Qed.

(** Mutant: a failed or torn make_mut publishes a fresh raw header without
    registering the corresponding anchor root payload. *)
Definition mutant_unregistered_make_mut_anchor : anchor_state :=
  AnchorState 1 1 0.

Theorem kill_unregistered_make_mut_raw_header :
  ~ anchor_well_formed mutant_unregistered_make_mut_anchor.
Proof. simpl. intros [_ impossible]. discriminate. Qed.

(** Mutant: Drop of a pending writer/iterator bypasses its local closure. *)
Definition mutant_early_owner_drop
    (_ : campaign_owned_exit) (subject : trie) : release_result :=
  mutant_raw_unique_release subject.

Theorem kill_early_pending_writer_or_iterator_drop :
  residual_campaign_edges
    (mutant_early_owner_drop PendingWriterEarlyDrop topology_only_comb) <>
  residual_campaign_edges
    (close_campaign_owned_exit PendingWriterEarlyDrop topology_only_comb).
Proof. vm_compute. discriminate. Qed.

(** Mutant: destructive replace/remove releases a detached dense subtree while
    that subtree still owns branches. *)
Theorem kill_destructive_detached_subtree_raw_drop :
  residual_campaign_edges (mutant_raw_unique_release topology_only_dense) <>
  residual_campaign_edges
    (close_campaign_owned_exit
      DestructiveSubtreeReplacement topology_only_dense).
Proof. vm_compute. discriminate. Qed.

(** Mutant: a [Par] value is detached and recursively dropped instead of being
    transferred to the explicit Par teardown PDA. *)
Definition mutant_recursive_par_disposal : list teardown_event :=
  [DetachOriginal (MapPar 7 [11; 12])].

Theorem kill_recursive_par_disposal :
  queued_par_jobs mutant_recursive_par_disposal <>
  queued_par_jobs (dispose_payload (MapPar 7 [11; 12])).
Proof. vm_compute. discriminate. Qed.

(** Mutants: reject publishes a private partial map, or accept publishes twice. *)
Definition mutant_early_publication (failure : build_failure) : builder_result :=
  BuilderResult [PublishComplete] 1 0 0 (Some failure).

Definition mutant_double_publication : builder_result :=
  BuilderResult [PublishComplete; PublishComplete] 2 0 0 None.

Theorem kill_early_partial_publication :
  publication_count (mutant_early_publication FramingFailure) <>
  publication_count
    (finalize_private_builder
      (RejectPrivate FramingFailure) topology_only_dense).
Proof. discriminate. Qed.

Theorem kill_double_complete_publication :
  publication_count mutant_double_publication <>
  publication_count
    (finalize_private_builder AcceptComplete topology_only_dense).
Proof. discriminate. Qed.

(** Mutant: deferred ACT failure is reported before an earlier child-PDA
    failure. *)
Definition mutant_deferred_act_first : option build_failure :=
  Some DeferredActFailure.

Theorem kill_error_precedence_inversion :
  mutant_deferred_act_first <>
  select_failure false true true true true.
Proof. discriminate. Qed.

Inductive teardown_mutant : Type :=
| ValueOnlyWalk
| MissingBarrier
| WrongLevelBarrier
| OverwriteRealParent
| RecursiveRemoveValuePrune
| SkippedPayload
| DuplicatedPayload
| RootPayloadOmission
| RawUniqueRelease
| StrongCountSnapshotRace
| PerWrapperRawHeaderClone
| ManualFamilyCounter
| UnregisteredMakeMutAnchor
| EarlyOwnerDrop
| DetachedSubtreeRawDrop
| RecursiveParDisposal
| EarlyPartialPublication
| DoubleCompletePublication
| ErrorPrecedenceInversion.

Definition all_teardown_mutants : list teardown_mutant :=
  [ValueOnlyWalk; MissingBarrier; WrongLevelBarrier; OverwriteRealParent;
   RecursiveRemoveValuePrune; SkippedPayload; DuplicatedPayload;
   RootPayloadOmission; RawUniqueRelease; StrongCountSnapshotRace;
   PerWrapperRawHeaderClone; ManualFamilyCounter;
   UnregisteredMakeMutAnchor; EarlyOwnerDrop;
   DetachedSubtreeRawDrop; RecursiveParDisposal; EarlyPartialPublication;
   DoubleCompletePublication; ErrorPrecedenceInversion].

Definition teardown_mutant_killed (mutant : teardown_mutant) : Prop :=
  match mutant with
  | ValueOnlyWalk =>
      mutant_value_only_prune_widths topology_only_comb <>
      prune_widths (drain_tree topology_only_comb)
  | MissingBarrier => ~ branch_safe_segment false mutant_missing_barrier
  | WrongLevelBarrier =>
      ~ branch_safe_segment false mutant_wrong_level_barrier
  | OverwriteRealParent =>
      ~ branch_safe_segment true mutant_overwrite_real_parent
  | RecursiveRemoveValuePrune =>
      prune_widths mutant_remove_value_with_recursive_prune <> [1]
  | SkippedPayload =>
      mutant_skip_first_payload <> detached_payloads (drain_tree valued_line)
  | DuplicatedPayload =>
      mutant_duplicate_first_payload <>
      detached_payloads (drain_tree valued_line)
  | RootPayloadOmission =>
      mutant_omit_root_payload <> detached_payloads (drain_tree valued_line)
  | RawUniqueRelease =>
      residual_campaign_edges (mutant_raw_unique_release topology_only_comb) <> 0
  | StrongCountSnapshotRace =>
      mutant_strong_count_snapshot_race <> safe_two_wrapper_drop_trace
  | PerWrapperRawHeaderClone =>
      mutant_per_wrapper_raw_header_clone <>
      anchor_transition_final (immutable_wrapper_clone 0)
  | ManualFamilyCounter =>
      mutant_manual_family_nonlast_trace <>
      anchor_transition_trace (nonlast_wrapper_drop 0)
  | UnregisteredMakeMutAnchor =>
      ~ anchor_well_formed mutant_unregistered_make_mut_anchor
  | EarlyOwnerDrop =>
      residual_campaign_edges
        (mutant_early_owner_drop PendingWriterEarlyDrop topology_only_comb) <>
      residual_campaign_edges
        (close_campaign_owned_exit PendingWriterEarlyDrop topology_only_comb)
  | DetachedSubtreeRawDrop =>
      residual_campaign_edges (mutant_raw_unique_release topology_only_dense) <>
      residual_campaign_edges
        (close_campaign_owned_exit
          DestructiveSubtreeReplacement topology_only_dense)
  | RecursiveParDisposal =>
      queued_par_jobs mutant_recursive_par_disposal <>
      queued_par_jobs (dispose_payload (MapPar 7 [11; 12]))
  | EarlyPartialPublication =>
      publication_count (mutant_early_publication FramingFailure) <>
      publication_count
        (finalize_private_builder
          (RejectPrivate FramingFailure) topology_only_dense)
  | DoubleCompletePublication =>
      publication_count mutant_double_publication <>
      publication_count
        (finalize_private_builder AcceptComplete topology_only_dense)
  | ErrorPrecedenceInversion =>
      mutant_deferred_act_first <>
      select_failure false true true true true
  end.

Theorem every_teardown_mutant_has_a_causal_counterexample : forall mutant,
  teardown_mutant_killed mutant.
Proof.
  intro mutant. destruct mutant; cbn.
  - apply kill_value_only_walk_on_topology_only_comb.
  - apply kill_missing_immediate_parent_barrier.
  - apply kill_wrong_level_or_late_barrier.
  - apply kill_real_parent_value_overwrite.
  - apply kill_remove_value_with_recursive_prune.
  - apply kill_skipped_payload.
  - apply kill_duplicated_payload.
  - apply kill_root_payload_omission.
  - exact (proj1 kill_raw_unique_release).
  - apply kill_snapshot_only_strong_count_race.
  - apply kill_per_wrapper_raw_header_and_root_value_clone.
  - apply kill_manual_family_count_protocol.
  - apply kill_unregistered_make_mut_raw_header.
  - apply kill_early_pending_writer_or_iterator_drop.
  - apply kill_destructive_detached_subtree_raw_drop.
  - apply kill_recursive_par_disposal.
  - apply kill_early_partial_publication.
  - apply kill_double_complete_publication.
  - apply kill_error_precedence_inversion.
Qed.

Theorem teardown_mutant_registry_is_unique_and_complete :
  NoDup all_teardown_mutants /\
  Forall teardown_mutant_killed all_teardown_mutants.
Proof.
  split.
  - repeat constructor; simpl; intuition discriminate.
  - repeat constructor; apply every_teardown_mutant_has_a_causal_counterexample.
Qed.

Theorem teardown_mutant_registry_has_exactly_nineteen_entries :
  length all_teardown_mutants = 19.
Proof. reflexivity. Qed.

Print Assumptions adversarial_fixtures_are_non_vacuous.
Print Assumptions kill_snapshot_only_strong_count_race.
Print Assumptions kill_per_wrapper_raw_header_and_root_value_clone.
Print Assumptions kill_early_pending_writer_or_iterator_drop.
Print Assumptions kill_destructive_detached_subtree_raw_drop.
Print Assumptions every_teardown_mutant_has_a_causal_counterexample.
Print Assumptions teardown_mutant_registry_is_unique_and_complete.
Print Assumptions teardown_mutant_registry_has_exactly_nineteen_entries.

End PathMapReversePruneTeardownMutants.
