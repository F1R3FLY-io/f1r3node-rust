From Stdlib Require Import Bool List Arith.
From StackSafePDA Require Import ReplayArchive.

Import ListNotations.
Module A := ReplayArchive.ReplayArchive.
Import A.
Set Implicit Arguments.

Module ReplayArchiveMutants.

Definition toy_context : context := Context 1 [3; 5; 8].
Definition toy_archive : archive := Archive toy_context 7 9 [11; 11; 22].
Definition permuted_archive : archive := Archive toy_context 7 9 [22; 11; 11].

Definition request_for (stored : archive) : replay_request :=
  ReplayRequest (archived_context stored) (expected_pre_root stored)
    (ordered_payload stored) false false.

Definition toy_meaning : realization abstract_bytes :=
  @Realization abstract_bytes
    (fun bytes => match bytes with [] => None | _ => Some bytes end)
    (fun decoded => decoded)
    (fun stored => stored = toy_archive \/ stored = permuted_archive)
    (fun supplied => supplied = toy_context)
    (fun supplied root => supplied = toy_context /\ root = 7)
    (fun request post => fold_right Nat.add 0 (supplied_ordered_payload request) = 44 /\
      post = 9).

Definition rejecting_codec : realization abstract_bytes :=
  @Realization abstract_bytes (fun _ => None) (fun decoded => decoded)
    (exact_archive_integrity toy_meaning) (context_is_complete toy_meaning)
    (genesis_realizes toy_meaning) (actual_runtime_replay toy_meaning).

Definition with_runtime (runtime : replay_request -> abstract_root -> Prop)
    : realization abstract_bytes :=
  @Realization abstract_bytes (decode_payload toy_meaning) (encode_payload toy_meaning)
    (exact_archive_integrity toy_meaning) (context_is_complete toy_meaning)
    (genesis_realizes toy_meaning) runtime.

Theorem toy_fixture_is_admitted : archive_admitted toy_meaning toy_archive (Some toy_archive).
Proof.
  unfold archive_admitted, codec_fixed_point; cbn.
  repeat split; try reflexivity; try (left; reflexivity).
  exists [11; 11; 22]. split; reflexivity.
Qed.

Theorem toy_fixture_is_accepted :
  accepted toy_meaning toy_archive (Some toy_archive) (request_for toy_archive)
    (ActualReplayCompleted 9).
Proof.
  split; [exact toy_fixture_is_admitted|].
  unfold request_matches, request_for; cbn. repeat split; reflexivity.
Qed.

Theorem separately_archived_permutation_can_replay :
  ordered_payload toy_archive <> ordered_payload permuted_archive /\
  actual_runtime_replay toy_meaning (request_for toy_archive) 9 /\
  actual_runtime_replay toy_meaning (request_for permuted_archive) 9 /\
  accepted toy_meaning permuted_archive (Some permuted_archive)
    (request_for permuted_archive) (ActualReplayCompleted 9) /\
  ~ accepted toy_meaning toy_archive (Some toy_archive)
    (request_for permuted_archive) (ActualReplayCompleted 9).
Proof.
  repeat split; try discriminate; try reflexivity.
  - right. reflexivity.
  - exists [22; 11; 11]. split; reflexivity.
  - apply changed_payload_or_order_refuses. discriminate.
Qed.

Definition mutant_omit_archive_presence {Decoded} (meaning : realization Decoded)
    (expected : archive) (_loaded : option archive)
    (request : replay_request) (outcome : replay_outcome) : Prop :=
  accepted meaning expected (Some expected) request outcome.

Theorem kill_missing_archive_substitution :
  mutant_omit_archive_presence toy_meaning toy_archive None (request_for toy_archive)
    (ActualReplayCompleted 9) /\
  ~ accepted toy_meaning toy_archive None (request_for toy_archive) (ActualReplayCompleted 9).
Proof. split; [exact toy_fixture_is_accepted|apply missing_archive_refuses]. Qed.

Definition mutant_omit_codec {Decoded} (meaning : realization Decoded)
    (expected : archive) (loaded : option archive)
    (request : replay_request) (outcome : replay_outcome) : Prop :=
  loaded = Some expected /\ exact_archive_integrity meaning expected /\
  context_is_complete meaning (archived_context expected) /\
  genesis_realizes meaning (archived_context expected) (expected_pre_root expected) /\
  request_matches expected request /\
  outcome = ActualReplayCompleted (expected_final_root expected) /\
  actual_runtime_replay meaning request (expected_final_root expected).

Theorem kill_missing_codec_validation :
  mutant_omit_codec rejecting_codec toy_archive (Some toy_archive)
    (request_for toy_archive) (ActualReplayCompleted 9) /\
  ~ accepted rejecting_codec toy_archive (Some toy_archive)
    (request_for toy_archive) (ActualReplayCompleted 9).
Proof.
  split.
  - unfold mutant_omit_codec, request_matches; cbn.
    repeat split; try reflexivity. left. reflexivity.
  - apply malformed_payload_refuses. reflexivity.
Qed.

Definition mutant_omit_request_binding {Decoded} (meaning : realization Decoded)
    (expected : archive) (loaded : option archive)
    (request : replay_request) (outcome : replay_outcome) : Prop :=
  archive_admitted meaning expected loaded /\
  state_hash_cache_enabled request = false /\ replay_cache_enabled request = false /\
  outcome = ActualReplayCompleted (expected_final_root expected) /\
  actual_runtime_replay meaning request (expected_final_root expected).

Definition changed_context_request : replay_request :=
  ReplayRequest (Context 2 [3; 5; 8]) 7 [11; 11; 22] false false.
Definition changed_pre_request : replay_request :=
  ReplayRequest toy_context 8 [11; 11; 22] false false.

Theorem kill_missing_request_context_binding :
  mutant_omit_request_binding toy_meaning toy_archive (Some toy_archive)
    changed_context_request (ActualReplayCompleted 9) /\
  ~ accepted toy_meaning toy_archive (Some toy_archive)
    changed_context_request (ActualReplayCompleted 9).
Proof.
  split.
  - split; [exact toy_fixture_is_admitted|]. cbn. repeat split; reflexivity.
  - apply changed_context_refuses. discriminate.
Qed.

Theorem kill_missing_request_pre_root_binding :
  mutant_omit_request_binding toy_meaning toy_archive (Some toy_archive)
    changed_pre_request (ActualReplayCompleted 9) /\
  ~ accepted toy_meaning toy_archive (Some toy_archive)
    changed_pre_request (ActualReplayCompleted 9).
Proof.
  split.
  - split; [exact toy_fixture_is_admitted|]. cbn. repeat split; reflexivity.
  - apply changed_pre_root_refuses. discriminate.
Qed.

Theorem kill_missing_ordered_payload_binding :
  mutant_omit_request_binding toy_meaning toy_archive (Some toy_archive)
    (request_for permuted_archive) (ActualReplayCompleted 9) /\
  ~ accepted toy_meaning toy_archive (Some toy_archive)
    (request_for permuted_archive) (ActualReplayCompleted 9).
Proof.
  split.
  - split; [exact toy_fixture_is_admitted|]. cbn. repeat split; reflexivity.
  - apply changed_payload_or_order_refuses. discriminate.
Qed.

Definition mutant_omit_cache_configuration {Decoded} (meaning : realization Decoded)
    (expected : archive) (loaded : option archive)
    (request : replay_request) (outcome : replay_outcome) : Prop :=
  archive_admitted meaning expected loaded /\
  requested_context request = archived_context expected /\
  actual_pre_root request = expected_pre_root expected /\
  supplied_ordered_payload request = ordered_payload expected /\
  outcome = ActualReplayCompleted (expected_final_root expected) /\
  actual_runtime_replay meaning request (expected_final_root expected).

Definition cache_enabled_request : replay_request :=
  ReplayRequest toy_context 7 [11; 11; 22] true true.

Theorem kill_enabled_cache_configuration :
  mutant_omit_cache_configuration toy_meaning toy_archive (Some toy_archive)
    cache_enabled_request (ActualReplayCompleted 9) /\
  ~ accepted toy_meaning toy_archive (Some toy_archive)
    cache_enabled_request (ActualReplayCompleted 9).
Proof.
  split.
  - split; [exact toy_fixture_is_admitted|]. cbn. repeat split; reflexivity.
  - apply enabled_cache_refuses. left. reflexivity.
Qed.

Definition mutant_trust_reported_root {Decoded} (meaning : realization Decoded)
    (expected : archive) (loaded : option archive)
    (request : replay_request) (outcome : replay_outcome) : Prop :=
  archive_admitted meaning expected loaded /\ request_matches expected request /\
  match outcome with
  | ActualReplayCompleted post | CacheShortcut post => post = expected_final_root expected
  | RuntimeRefused => False
  end.

Theorem kill_cached_root_as_actual_execution :
  mutant_trust_reported_root toy_meaning toy_archive (Some toy_archive)
    (request_for toy_archive) (CacheShortcut 9) /\
  ~ accepted toy_meaning toy_archive (Some toy_archive)
    (request_for toy_archive) (CacheShortcut 9).
Proof.
  split.
  - split; [exact toy_fixture_is_admitted|]. unfold request_matches; cbn.
    repeat split; reflexivity.
  - apply cache_shortcut_refuses_even_with_the_expected_root.
Qed.

Theorem kill_unrealized_success_report :
  let absent := with_runtime (fun _ _ => False) in
  mutant_trust_reported_root absent toy_archive (Some toy_archive)
    (request_for toy_archive) (ActualReplayCompleted 9) /\
  ~ accepted absent toy_archive (Some toy_archive)
    (request_for toy_archive) (ActualReplayCompleted 9).
Proof.
  cbn zeta. split.
  - split; [exact toy_fixture_is_admitted|]. unfold request_matches; cbn.
    repeat split; reflexivity.
  - apply absent_actual_replay_refuses. exact (fun impossible => impossible).
Qed.

Definition mutant_omit_final_root {Decoded} (meaning : realization Decoded)
    (expected : archive) (loaded : option archive)
    (request : replay_request) (outcome : replay_outcome) : Prop :=
  archive_admitted meaning expected loaded /\ request_matches expected request /\
  exists post, outcome = ActualReplayCompleted post /\ actual_runtime_replay meaning request post.

Theorem kill_missing_final_root_comparison :
  let different := with_runtime (fun _ post => post = 10) in
  mutant_omit_final_root different toy_archive (Some toy_archive)
    (request_for toy_archive) (ActualReplayCompleted 10) /\
  ~ accepted different toy_archive (Some toy_archive)
    (request_for toy_archive) (ActualReplayCompleted 10).
Proof.
  cbn zeta. split.
  - split; [exact toy_fixture_is_admitted|]. split.
    + unfold request_matches; cbn. repeat split; reflexivity.
    + exists 10. split; reflexivity.
  - apply changed_final_root_refuses. discriminate.
Qed.

Print Assumptions toy_fixture_is_admitted.
Print Assumptions toy_fixture_is_accepted.
Print Assumptions separately_archived_permutation_can_replay.
Print Assumptions kill_missing_archive_substitution.
Print Assumptions kill_missing_codec_validation.
Print Assumptions kill_missing_request_context_binding.
Print Assumptions kill_missing_request_pre_root_binding.
Print Assumptions kill_missing_ordered_payload_binding.
Print Assumptions kill_enabled_cache_configuration.
Print Assumptions kill_cached_root_as_actual_execution.
Print Assumptions kill_unrealized_success_report.
Print Assumptions kill_missing_final_root_comparison.

Definition bootstrap_context : context := Context 1 [4].
Definition genesis_fixture : archive := Archive bootstrap_context 4 7 [2].
Definition unstitched_workload : archive := Archive toy_context 8 9 [11; 11; 22].

Definition v2_meaning : realization abstract_bytes :=
  @Realization abstract_bytes (decode_payload toy_meaning) (encode_payload toy_meaning)
    (fun stored => stored = genesis_fixture \/ stored = toy_archive \/ stored = unstitched_workload)
    (fun supplied => supplied = bootstrap_context \/ supplied = toy_context)
    (fun supplied root => supplied = bootstrap_context /\ root = 4)
    (fun request post =>
      (supplied_ordered_payload request = [2] /\ post = 7) \/
      (supplied_ordered_payload request = [11; 11; 22] /\ post = 9)).

Definition v2_compatible (genesis_context workload_context : context) : Prop :=
  genesis_context = bootstrap_context /\ workload_context = toy_context.

Definition v2_accept (trusted : abstract_root) (loaded : option archive)
    (genesis_request : replay_request) (genesis_result : replay_outcome)
    (workload : archive) : Prop :=
  @bootstrap_replay_accepted abstract_bytes v2_meaning trusted genesis_fixture
    loaded genesis_request genesis_result v2_compatible
    workload (Some workload) (request_for workload) (ActualReplayCompleted 9).

Theorem v2_genesis_fixture_is_accepted :
  accepted v2_meaning genesis_fixture (Some genesis_fixture)
    (request_for genesis_fixture) (ActualReplayCompleted 7).
Proof.
  unfold accepted, archive_admitted, codec_fixed_point, request_matches; cbn.
  repeat split; try reflexivity; try (left; reflexivity); try (left; split; reflexivity).
  exists [2]. split; reflexivity.
Qed.

Theorem v2_composed_fixture_is_accepted :
  v2_accept 4 (Some genesis_fixture) (request_for genesis_fixture)
    (ActualReplayCompleted 7) toy_archive.
Proof.
  unfold v2_accept. apply bootstrap_replay_accepts_complete_composed_evidence.
  - reflexivity.
  - exact v2_genesis_fixture_is_accepted.
  - split; reflexivity.
  - reflexivity.
  - reflexivity.
  - right. left. reflexivity.
  - right. reflexivity.
  - exists [11; 11; 22]. split; reflexivity.
  - unfold request_matches; cbn. repeat split; reflexivity.
  - reflexivity.
  - right. split; reflexivity.
Qed.

Theorem v2_acceptance_does_not_require_original_workload_genesis_realization :
  v2_accept 4 (Some genesis_fixture) (request_for genesis_fixture)
    (ActualReplayCompleted 7) toy_archive /\
  ~ genesis_realizes v2_meaning (archived_context toy_archive) (expected_pre_root toy_archive).
Proof.
  split; [exact v2_composed_fixture_is_accepted|]. cbn. intuition discriminate.
Qed.

Definition unchecked_genesis_start : realization abstract_bytes :=
  realization_with_start v2_meaning (fun workload_context workload_pre =>
    expected_pre_root genesis_fixture = 4 /\
    v2_compatible (archived_context genesis_fixture) workload_context /\
    workload_pre = expected_final_root genesis_fixture).

Definition mutant_omit_genesis_stage (_loaded : option archive)
    (_request : replay_request) (_outcome : replay_outcome) : Prop :=
  accepted unchecked_genesis_start toy_archive (Some toy_archive)
    (request_for toy_archive) (ActualReplayCompleted 9).

Lemma unchecked_genesis_stage_accepts_workload : forall loaded request outcome,
  mutant_omit_genesis_stage loaded request outcome.
Proof.
  intros. unfold mutant_omit_genesis_stage, accepted, archive_admitted,
    codec_fixed_point, request_matches; cbn.
  repeat split; try reflexivity; try (right; left; reflexivity);
    try (right; reflexivity); try (right; split; reflexivity).
  exists [11; 11; 22]. split; reflexivity.
Qed.

Theorem kill_missing_genesis_stage :
  mutant_omit_genesis_stage None (request_for genesis_fixture) (ActualReplayCompleted 7) /\
  ~ v2_accept 4 None (request_for genesis_fixture) (ActualReplayCompleted 7) toy_archive.
Proof.
  split; [apply unchecked_genesis_stage_accepts_workload|].
  unfold v2_accept. apply bootstrap_replay_refuses_missing_genesis. reflexivity.
Qed.

Theorem kill_cached_genesis_stage :
  mutant_omit_genesis_stage (Some genesis_fixture) (request_for genesis_fixture) (CacheShortcut 7) /\
  ~ v2_accept 4 (Some genesis_fixture) (request_for genesis_fixture) (CacheShortcut 7) toy_archive.
Proof.
  split; [apply unchecked_genesis_stage_accepts_workload|].
  unfold v2_accept. apply bootstrap_replay_refuses_genesis_cache_shortcut with (cached_root := 7).
  reflexivity.
Qed.

Theorem kill_failed_genesis_stage :
  mutant_omit_genesis_stage (Some genesis_fixture) (request_for genesis_fixture) RuntimeRefused /\
  ~ v2_accept 4 (Some genesis_fixture) (request_for genesis_fixture) RuntimeRefused toy_archive.
Proof.
  split; [apply unchecked_genesis_stage_accepts_workload|].
  unfold v2_accept. apply bootstrap_replay_refuses_failed_genesis. reflexivity.
Qed.

Definition cached_genesis_request : replay_request :=
  ReplayRequest bootstrap_context 4 [2] true true.

Theorem kill_cache_enabled_genesis_stage :
  mutant_omit_genesis_stage (Some genesis_fixture) cached_genesis_request (ActualReplayCompleted 7) /\
  ~ v2_accept 4 (Some genesis_fixture) cached_genesis_request (ActualReplayCompleted 7) toy_archive.
Proof.
  split; [apply unchecked_genesis_stage_accepts_workload|].
  unfold v2_accept. apply bootstrap_replay_refuses_enabled_genesis_cache. left. reflexivity.
Qed.

Definition mutant_omit_bootstrap_comparison (_trusted : abstract_root) : Prop :=
  v2_accept (expected_pre_root genesis_fixture) (Some genesis_fixture)
    (request_for genesis_fixture) (ActualReplayCompleted 7) toy_archive.

Theorem kill_missing_bootstrap_comparison :
  mutant_omit_bootstrap_comparison 5 /\
  ~ v2_accept 5 (Some genesis_fixture) (request_for genesis_fixture)
    (ActualReplayCompleted 7) toy_archive.
Proof.
  split; [exact v2_composed_fixture_is_accepted|].
  unfold v2_accept. apply bootstrap_replay_refuses_wrong_bootstrap. discriminate.
Qed.

Definition unchecked_bridge_start : realization abstract_bytes :=
  realization_with_start v2_meaning (fun workload_context _workload_pre =>
    expected_pre_root genesis_fixture = 4 /\
    accepted v2_meaning genesis_fixture (Some genesis_fixture)
      (request_for genesis_fixture) (ActualReplayCompleted 7) /\
    v2_compatible (archived_context genesis_fixture) workload_context).

Definition mutant_omit_bridge_comparison : Prop :=
  accepted unchecked_bridge_start unstitched_workload (Some unstitched_workload)
    (request_for unstitched_workload) (ActualReplayCompleted 9).

Theorem kill_missing_bridge_comparison :
  mutant_omit_bridge_comparison /\
  ~ v2_accept 4 (Some genesis_fixture) (request_for genesis_fixture)
    (ActualReplayCompleted 7) unstitched_workload.
Proof.
  split.
  - unfold mutant_omit_bridge_comparison. split.
    + unfold archive_admitted. split; [reflexivity|].
      split; [right; right; reflexivity|]. split; [right; reflexivity|]. split.
      * change (expected_pre_root genesis_fixture = 4 /\
          accepted v2_meaning genesis_fixture (Some genesis_fixture)
            (request_for genesis_fixture) (ActualReplayCompleted 7) /\
          v2_compatible (archived_context genesis_fixture) (archived_context unstitched_workload)).
        split; [reflexivity|]. split; [exact v2_genesis_fixture_is_accepted|].
        split; reflexivity.
      * exists [11; 11; 22]. split; reflexivity.
    + split; [unfold request_matches; cbn; repeat split; reflexivity|].
      split; [reflexivity|]. right. split; reflexivity.
  - unfold v2_accept. apply bootstrap_replay_refuses_wrong_bridge. discriminate.
Qed.

Print Assumptions v2_genesis_fixture_is_accepted.
Print Assumptions v2_composed_fixture_is_accepted.
Print Assumptions v2_acceptance_does_not_require_original_workload_genesis_realization.
Print Assumptions kill_missing_genesis_stage.
Print Assumptions kill_cached_genesis_stage.
Print Assumptions kill_failed_genesis_stage.
Print Assumptions kill_cache_enabled_genesis_stage.
Print Assumptions kill_missing_bootstrap_comparison.
Print Assumptions kill_missing_bridge_comparison.

End ReplayArchiveMutants.
