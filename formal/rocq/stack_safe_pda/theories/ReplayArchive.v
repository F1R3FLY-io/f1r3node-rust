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

End ReplayArchive.
