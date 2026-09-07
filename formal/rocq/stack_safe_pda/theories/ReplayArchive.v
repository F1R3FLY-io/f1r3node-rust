From Stdlib Require Import Bool List Arith.

Import ListNotations.
Set Implicit Arguments.

Module ReplayArchive.

Definition abstract_bytes := list nat.
Definition abstract_root := nat.

Record context : Type := Context {
  fixture_epoch : nat;
  complete_context_encoding : abstract_bytes
}.

Record archive : Type := Archive {
  archived_context : context;
  expected_pre_root : abstract_root;
  expected_final_root : abstract_root;
  ordered_payload : abstract_bytes
}.

Record replay_request : Type := ReplayRequest {
  requested_context : context;
  actual_pre_root : abstract_root;
  supplied_ordered_payload : abstract_bytes;
  state_hash_cache_enabled : bool;
  replay_cache_enabled : bool
}.

Inductive replay_outcome : Type :=
| ActualReplayCompleted (post_root : abstract_root)
| RuntimeRefused
| CacheShortcut (post_root : abstract_root).

Record realization (DecodedPayload : Type) : Type := Realization {
  decode_payload : abstract_bytes -> option DecodedPayload;
  encode_payload : DecodedPayload -> abstract_bytes;
  exact_archive_integrity : archive -> Prop;
  context_is_complete : context -> Prop;
  genesis_realizes : context -> abstract_root -> Prop;
  actual_runtime_replay : replay_request -> abstract_root -> Prop
}.

Section Verification.

Context {DecodedPayload : Type} (meaning : realization DecodedPayload).

Definition codec_fixed_point (expected : archive) : Prop :=
  exists decoded,
    decode_payload meaning (ordered_payload expected) = Some decoded /\
    encode_payload meaning decoded = ordered_payload expected.

Definition archive_admitted (expected : archive) (loaded : option archive) : Prop :=
  loaded = Some expected /\
  exact_archive_integrity meaning expected /\
  context_is_complete meaning (archived_context expected) /\
  genesis_realizes meaning (archived_context expected) (expected_pre_root expected) /\
  codec_fixed_point expected.

Definition request_matches (expected : archive) (request : replay_request) : Prop :=
  requested_context request = archived_context expected /\
  actual_pre_root request = expected_pre_root expected /\
  supplied_ordered_payload request = ordered_payload expected /\
  state_hash_cache_enabled request = false /\ replay_cache_enabled request = false.

Definition accepted (expected : archive) (loaded : option archive)
    (request : replay_request) (outcome : replay_outcome) : Prop :=
  archive_admitted expected loaded /\
  request_matches expected request /\
  outcome = ActualReplayCompleted (expected_final_root expected) /\
  actual_runtime_replay meaning request (expected_final_root expected).

Theorem acceptance_preserves_exact_archive_context_roots_and_ordered_payload :
  forall expected loaded request outcome,
  accepted expected loaded request outcome ->
  loaded = Some expected /\
  requested_context request = archived_context expected /\
  actual_pre_root request = expected_pre_root expected /\
  supplied_ordered_payload request = ordered_payload expected /\
  outcome = ActualReplayCompleted (expected_final_root expected).
Proof. unfold accepted, archive_admitted, request_matches; intuition. Qed.

Theorem acceptance_requires_all_realization_premises :
  forall expected loaded request outcome,
  accepted expected loaded request outcome ->
  exact_archive_integrity meaning expected /\
  context_is_complete meaning (archived_context expected) /\
  genesis_realizes meaning (archived_context expected) (expected_pre_root expected) /\
  codec_fixed_point expected /\
  actual_runtime_replay meaning request (expected_final_root expected).
Proof. unfold accepted, archive_admitted; intuition. Qed.

Theorem acceptance_requires_successful_uncached_actual_replay :
  forall expected loaded request outcome,
  accepted expected loaded request outcome ->
  state_hash_cache_enabled request = false /\
  replay_cache_enabled request = false /\
  outcome = ActualReplayCompleted (expected_final_root expected) /\
  actual_runtime_replay meaning request (expected_final_root expected).
Proof. unfold accepted, request_matches; intuition. Qed.

Theorem missing_archive_refuses : forall expected request outcome,
  ~ accepted expected None request outcome.
Proof. unfold accepted, archive_admitted; intuition discriminate. Qed.

Theorem changed_archive_refuses : forall expected changed request outcome,
  changed <> expected -> ~ accepted expected (Some changed) request outcome.
Proof. unfold accepted, archive_admitted; intuition congruence. Qed.

Theorem absent_integrity_refuses : forall expected loaded request outcome,
  ~ exact_archive_integrity meaning expected ->
  ~ accepted expected loaded request outcome.
Proof. unfold accepted, archive_admitted; intuition. Qed.

Theorem incomplete_context_refuses : forall expected loaded request outcome,
  ~ context_is_complete meaning (archived_context expected) ->
  ~ accepted expected loaded request outcome.
Proof. unfold accepted, archive_admitted; intuition. Qed.

Theorem unestablished_genesis_refuses : forall expected loaded request outcome,
  ~ genesis_realizes meaning (archived_context expected) (expected_pre_root expected) ->
  ~ accepted expected loaded request outcome.
Proof. unfold accepted, archive_admitted; intuition. Qed.

Theorem malformed_payload_refuses : forall expected loaded request outcome,
  decode_payload meaning (ordered_payload expected) = None ->
  ~ accepted expected loaded request outcome.
Proof.
  unfold accepted, archive_admitted, codec_fixed_point.
  intros expected loaded request outcome malformed [[_ [_ [_ [_ [decoded [decoded_ok _]]]]]] _].
  congruence.
Qed.

Theorem non_fixed_point_payload_refuses : forall expected loaded request outcome,
  (forall decoded, decode_payload meaning (ordered_payload expected) = Some decoded ->
    encode_payload meaning decoded <> ordered_payload expected) ->
  ~ accepted expected loaded request outcome.
Proof.
  unfold accepted, archive_admitted, codec_fixed_point.
  intros expected loaded request outcome changed [[_ [_ [_ [_ [decoded [decoded_ok encoded_ok]]]]]] _].
  exact (changed decoded decoded_ok encoded_ok).
Qed.

Theorem changed_context_refuses : forall expected loaded request outcome,
  requested_context request <> archived_context expected ->
  ~ accepted expected loaded request outcome.
Proof. unfold accepted, request_matches; intuition. Qed.

Theorem changed_pre_root_refuses : forall expected loaded request outcome,
  actual_pre_root request <> expected_pre_root expected ->
  ~ accepted expected loaded request outcome.
Proof. unfold accepted, request_matches; intuition. Qed.

Theorem changed_payload_or_order_refuses : forall expected loaded request outcome,
  supplied_ordered_payload request <> ordered_payload expected ->
  ~ accepted expected loaded request outcome.
Proof. unfold accepted, request_matches; intuition. Qed.

Theorem changed_final_root_refuses : forall expected loaded request post,
  post <> expected_final_root expected ->
  ~ accepted expected loaded request (ActualReplayCompleted post).
Proof. unfold accepted; intuition congruence. Qed.

Theorem enabled_cache_refuses : forall expected loaded request outcome,
  state_hash_cache_enabled request = true \/ replay_cache_enabled request = true ->
  ~ accepted expected loaded request outcome.
Proof. unfold accepted, request_matches; intuition congruence. Qed.

Theorem cache_shortcut_refuses_even_with_the_expected_root :
  forall expected loaded request post,
  ~ accepted expected loaded request (CacheShortcut post).
Proof. unfold accepted; intuition discriminate. Qed.

Theorem failed_runtime_refuses : forall expected loaded request,
  ~ accepted expected loaded request RuntimeRefused.
Proof. unfold accepted; intuition discriminate. Qed.

Theorem absent_actual_replay_refuses : forall expected loaded request outcome,
  ~ actual_runtime_replay meaning request (expected_final_root expected) ->
  ~ accepted expected loaded request outcome.
Proof. unfold accepted; intuition. Qed.

Theorem accepted_payload_preserves_every_archived_position :
  forall expected loaded request outcome index,
  accepted expected loaded request outcome ->
  nth_error (supplied_ordered_payload request) index =
    nth_error (ordered_payload expected) index.
Proof.
  intros expected loaded request outcome index [_ [[_ [_ [same _]]] _]].
  now rewrite same.
Qed.

Theorem complete_evidence_is_accepted : forall expected loaded request outcome,
  archive_admitted expected loaded -> request_matches expected request ->
  outcome = ActualReplayCompleted (expected_final_root expected) ->
  actual_runtime_replay meaning request (expected_final_root expected) ->
  accepted expected loaded request outcome.
Proof. unfold accepted; intuition. Qed.

End Verification.

Print Assumptions acceptance_preserves_exact_archive_context_roots_and_ordered_payload.
Print Assumptions acceptance_requires_all_realization_premises.
Print Assumptions acceptance_requires_successful_uncached_actual_replay.
Print Assumptions missing_archive_refuses.
Print Assumptions changed_archive_refuses.
Print Assumptions absent_integrity_refuses.
Print Assumptions incomplete_context_refuses.
Print Assumptions unestablished_genesis_refuses.
Print Assumptions malformed_payload_refuses.
Print Assumptions non_fixed_point_payload_refuses.
Print Assumptions changed_context_refuses.
Print Assumptions changed_pre_root_refuses.
Print Assumptions changed_payload_or_order_refuses.
Print Assumptions changed_final_root_refuses.
Print Assumptions enabled_cache_refuses.
Print Assumptions cache_shortcut_refuses_even_with_the_expected_root.
Print Assumptions failed_runtime_refuses.
Print Assumptions absent_actual_replay_refuses.
Print Assumptions accepted_payload_preserves_every_archived_position.
Print Assumptions complete_evidence_is_accepted.

Definition realization_with_start {Decoded} (meaning : realization Decoded)
    (start_realizes : context -> abstract_root -> Prop) : realization Decoded :=
  @Realization Decoded (decode_payload meaning) (encode_payload meaning)
    (exact_archive_integrity meaning) (context_is_complete meaning)
    start_realizes (actual_runtime_replay meaning).

Section BootstrapReplay.

Context {Decoded : Type} (meaning : realization Decoded).
Variable trusted_bootstrap : abstract_root.
Variable genesis : archive.
Variable loaded_genesis : option archive.
Variable genesis_request : replay_request.
Variable genesis_outcome : replay_outcome.
Variable compatible_contexts : context -> context -> Prop.

Definition replayed_genesis_realization : realization Decoded :=
  realization_with_start meaning (fun workload_context workload_pre =>
    expected_pre_root genesis = trusted_bootstrap /\
    accepted meaning genesis loaded_genesis genesis_request genesis_outcome /\
    compatible_contexts (archived_context genesis) workload_context /\
    workload_pre = expected_final_root genesis).

Definition bootstrap_replay_accepted (workload : archive) (loaded_workload : option archive)
    (workload_request : replay_request) (workload_outcome : replay_outcome) : Prop :=
  accepted replayed_genesis_realization workload loaded_workload
    workload_request workload_outcome.

Theorem bootstrap_replay_has_checked_start_evidence :
  forall workload loaded request outcome,
  bootstrap_replay_accepted workload loaded request outcome ->
  expected_pre_root genesis = trusted_bootstrap /\
  accepted meaning genesis loaded_genesis genesis_request genesis_outcome /\
  compatible_contexts (archived_context genesis) (archived_context workload) /\
  expected_pre_root workload = expected_final_root genesis.
Proof.
  intros workload loaded request outcome [[_ [_ [_ [start _]]]] _].
  exact start.
Qed.

Theorem bootstrap_replay_stitches_separate_root_obligations :
  forall workload loaded request outcome,
  bootstrap_replay_accepted workload loaded request outcome ->
  actual_pre_root genesis_request = trusted_bootstrap /\
  genesis_outcome = ActualReplayCompleted (expected_pre_root workload) /\
  actual_pre_root request = expected_final_root genesis /\
  outcome = ActualReplayCompleted (expected_final_root workload).
Proof.
  intros workload loaded request outcome replay.
  pose proof (bootstrap_replay_has_checked_start_evidence replay)
    as [bootstrap [genesis_ok [_ bridge]]].
  pose proof (acceptance_preserves_exact_archive_context_roots_and_ordered_payload genesis_ok)
    as [_ [_ [genesis_pre [_ genesis_post]]]].
  pose proof (acceptance_preserves_exact_archive_context_roots_and_ordered_payload replay)
    as [_ [_ [workload_pre [_ workload_post]]]].
  repeat split; congruence.
Qed.

Theorem bootstrap_replay_preserves_both_exact_archives_contexts_and_payloads :
  forall workload loaded request outcome,
  bootstrap_replay_accepted workload loaded request outcome ->
  loaded_genesis = Some genesis /\ loaded = Some workload /\
  requested_context genesis_request = archived_context genesis /\
  requested_context request = archived_context workload /\
  supplied_ordered_payload genesis_request = ordered_payload genesis /\
  supplied_ordered_payload request = ordered_payload workload.
Proof.
  intros workload loaded request outcome replay.
  pose proof (bootstrap_replay_has_checked_start_evidence replay) as [_ [genesis_ok _]].
  pose proof (acceptance_preserves_exact_archive_context_roots_and_ordered_payload genesis_ok).
  pose proof (acceptance_preserves_exact_archive_context_roots_and_ordered_payload replay).
  intuition.
Qed.

Theorem bootstrap_replay_requires_both_uncached_actual_executions :
  forall workload loaded request outcome,
  bootstrap_replay_accepted workload loaded request outcome ->
  state_hash_cache_enabled genesis_request = false /\
  replay_cache_enabled genesis_request = false /\
  state_hash_cache_enabled request = false /\ replay_cache_enabled request = false /\
  actual_runtime_replay meaning genesis_request (expected_final_root genesis) /\
  actual_runtime_replay meaning request (expected_final_root workload).
Proof.
  intros workload loaded request outcome replay.
  pose proof (bootstrap_replay_has_checked_start_evidence replay) as [_ [genesis_ok _]].
  pose proof (acceptance_requires_successful_uncached_actual_replay genesis_ok).
  pose proof (acceptance_requires_successful_uncached_actual_replay replay).
  cbn in *. intuition.
Qed.

Theorem bootstrap_replay_requires_realized_bootstrap_not_regenerated_genesis :
  forall workload loaded request outcome,
  bootstrap_replay_accepted workload loaded request outcome ->
  genesis_realizes meaning (archived_context genesis) trusted_bootstrap /\
  genesis_realizes replayed_genesis_realization
    (archived_context workload) (expected_pre_root workload).
Proof.
  intros workload loaded request outcome replay.
  pose proof (bootstrap_replay_has_checked_start_evidence replay)
    as [bootstrap [genesis_ok remainder]].
  pose proof (acceptance_requires_all_realization_premises genesis_ok)
    as [_ [_ [realized _]]].
  split; [now rewrite <- bootstrap|].
  exact (bootstrap_replay_has_checked_start_evidence replay).
Qed.

Theorem bootstrap_replay_refuses_unaccepted_genesis :
  forall workload loaded request outcome,
  ~ accepted meaning genesis loaded_genesis genesis_request genesis_outcome ->
  ~ bootstrap_replay_accepted workload loaded request outcome.
Proof.
  intros workload loaded request outcome refused replay.
  apply refused. exact (proj1 (proj2 (bootstrap_replay_has_checked_start_evidence replay))).
Qed.

Theorem bootstrap_replay_refuses_missing_genesis :
  forall workload loaded request outcome,
  loaded_genesis = None -> ~ bootstrap_replay_accepted workload loaded request outcome.
Proof.
  intros workload loaded request outcome missing.
  apply bootstrap_replay_refuses_unaccepted_genesis.
  rewrite missing. apply missing_archive_refuses.
Qed.

Theorem bootstrap_replay_refuses_wrong_bootstrap :
  forall workload loaded request outcome,
  expected_pre_root genesis <> trusted_bootstrap ->
  ~ bootstrap_replay_accepted workload loaded request outcome.
Proof.
  intros workload loaded request outcome different replay.
  exact (different (proj1 (bootstrap_replay_has_checked_start_evidence replay))).
Qed.

Theorem bootstrap_replay_refuses_wrong_bridge :
  forall workload loaded request outcome,
  expected_pre_root workload <> expected_final_root genesis ->
  ~ bootstrap_replay_accepted workload loaded request outcome.
Proof.
  intros workload loaded request outcome different replay.
  exact (different (proj2 (proj2 (proj2 (bootstrap_replay_has_checked_start_evidence replay))))).
Qed.

Theorem bootstrap_replay_refuses_incompatible_contexts :
  forall workload loaded request outcome,
  ~ compatible_contexts (archived_context genesis) (archived_context workload) ->
  ~ bootstrap_replay_accepted workload loaded request outcome.
Proof.
  intros workload loaded request outcome incompatible replay.
  exact (incompatible (proj1 (proj2 (proj2 (bootstrap_replay_has_checked_start_evidence replay))))).
Qed.

Theorem bootstrap_replay_refuses_genesis_cache_shortcut :
  forall workload loaded request outcome cached_root,
  genesis_outcome = CacheShortcut cached_root ->
  ~ bootstrap_replay_accepted workload loaded request outcome.
Proof.
  intros workload loaded request outcome cached_root cached.
  apply bootstrap_replay_refuses_unaccepted_genesis.
  rewrite cached. apply cache_shortcut_refuses_even_with_the_expected_root.
Qed.

Theorem bootstrap_replay_refuses_failed_genesis :
  forall workload loaded request outcome,
  genesis_outcome = RuntimeRefused ->
  ~ bootstrap_replay_accepted workload loaded request outcome.
Proof.
  intros workload loaded request outcome failed.
  apply bootstrap_replay_refuses_unaccepted_genesis.
  rewrite failed. apply failed_runtime_refuses.
Qed.

Theorem bootstrap_replay_refuses_enabled_genesis_cache :
  forall workload loaded request outcome,
  state_hash_cache_enabled genesis_request = true \/
    replay_cache_enabled genesis_request = true ->
  ~ bootstrap_replay_accepted workload loaded request outcome.
Proof.
  intros workload loaded request outcome cached.
  apply bootstrap_replay_refuses_unaccepted_genesis.
  now apply enabled_cache_refuses.
Qed.

Theorem bootstrap_replay_accepts_complete_composed_evidence :
  forall workload loaded request outcome,
  expected_pre_root genesis = trusted_bootstrap ->
  accepted meaning genesis loaded_genesis genesis_request genesis_outcome ->
  compatible_contexts (archived_context genesis) (archived_context workload) ->
  expected_pre_root workload = expected_final_root genesis ->
  loaded = Some workload -> exact_archive_integrity meaning workload ->
  context_is_complete meaning (archived_context workload) ->
  codec_fixed_point meaning workload -> request_matches workload request ->
  outcome = ActualReplayCompleted (expected_final_root workload) ->
  actual_runtime_replay meaning request (expected_final_root workload) ->
  bootstrap_replay_accepted workload loaded request outcome.
Proof.
  intros workload loaded request outcome bootstrap genesis_ok compatible bridge
    present integrity complete fixed matches result actual.
  split.
  - unfold archive_admitted. split; [exact present|].
    split; [exact integrity|]. split; [exact complete|]. split.
    + change (expected_pre_root genesis = trusted_bootstrap /\
        accepted meaning genesis loaded_genesis genesis_request genesis_outcome /\
        compatible_contexts (archived_context genesis) (archived_context workload) /\
        expected_pre_root workload = expected_final_root genesis).
      auto.
    + exact fixed.
  - split; [exact matches|]. split; [exact result|exact actual].
Qed.

End BootstrapReplay.

Print Assumptions bootstrap_replay_has_checked_start_evidence.
Print Assumptions bootstrap_replay_stitches_separate_root_obligations.
Print Assumptions bootstrap_replay_preserves_both_exact_archives_contexts_and_payloads.
Print Assumptions bootstrap_replay_requires_both_uncached_actual_executions.
Print Assumptions bootstrap_replay_requires_realized_bootstrap_not_regenerated_genesis.
Print Assumptions bootstrap_replay_refuses_unaccepted_genesis.
Print Assumptions bootstrap_replay_refuses_missing_genesis.
Print Assumptions bootstrap_replay_refuses_wrong_bootstrap.
Print Assumptions bootstrap_replay_refuses_wrong_bridge.
Print Assumptions bootstrap_replay_refuses_incompatible_contexts.
Print Assumptions bootstrap_replay_refuses_genesis_cache_shortcut.
Print Assumptions bootstrap_replay_refuses_failed_genesis.
Print Assumptions bootstrap_replay_refuses_enabled_genesis_cache.
Print Assumptions bootstrap_replay_accepts_complete_composed_evidence.

End ReplayArchive.
