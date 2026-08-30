From Stdlib Require Import Arith.PeanoNat Lists.List Lia.
Import ListNotations.

Inductive deploy_id_v6 := DeployIdV6 (digest : nat).

Definition deploy_id_v6_eq_dec :
  forall left right : deploy_id_v6, {left = right} + {left <> right}.
Proof.
  decide equality; apply Nat.eq_dec.
Defined.

Inductive storage_protocol := PreV6 | ProtocolV6.

Inductive deploy_lookup_id :=
| LegacyDeployId (signature : nat)
| EnvelopeDeployId (deploy_id : deploy_id_v6).

Definition select_wire_identity
  (protocol : storage_protocol)
  (legacy_signature : option nat)
  (envelope_id : option deploy_id_v6)
  : option deploy_lookup_id :=
  match protocol with
  | PreV6 =>
      match legacy_signature, envelope_id with
      | Some signature, None => Some (LegacyDeployId signature)
      | _, _ => None
      end
  | ProtocolV6 =>
      match legacy_signature, envelope_id with
      | None, Some deploy_id => Some (EnvelopeDeployId deploy_id)
      | _, _ => None
      end
  end.

Theorem protocol_v6_selects_explicit_envelope_identity :
  forall deploy_id,
    select_wire_identity ProtocolV6 None (Some deploy_id) =
      Some (EnvelopeDeployId deploy_id).
Proof.
  reflexivity.
Qed.

Theorem pre_v6_selects_explicit_legacy_identity :
  forall signature,
    select_wire_identity PreV6 (Some signature) None =
      Some (LegacyDeployId signature).
Proof.
  reflexivity.
Qed.

Theorem protocol_v6_legacy_only_identity_is_rejected :
  forall signature,
    select_wire_identity ProtocolV6 (Some signature) None = None.
Proof.
  reflexivity.
Qed.

Theorem pre_v6_envelope_only_identity_is_rejected :
  forall deploy_id,
    select_wire_identity PreV6 None (Some deploy_id) = None.
Proof.
  reflexivity.
Qed.

Theorem mixed_wire_identity_is_rejected :
  forall protocol signature deploy_id,
    select_wire_identity protocol (Some signature) (Some deploy_id) = None.
Proof.
  now destruct protocol.
Qed.

Theorem missing_wire_identity_is_rejected :
  forall protocol,
    select_wire_identity protocol None None = None.
Proof.
  now destruct protocol.
Qed.

Record admission_partition := {
  partition_candidates : list deploy_id_v6;
  partition_admitted : list deploy_id_v6;
  partition_rejected : list deploy_id_v6
}.

Definition partition_covers_candidates (partition : admission_partition) : Prop :=
  forall deploy,
    In deploy (partition_candidates partition) <->
    In deploy (partition_admitted partition) \/
    In deploy (partition_rejected partition).

Definition partition_is_disjoint (partition : admission_partition) : Prop :=
  forall deploy,
    In deploy (partition_admitted partition) ->
    ~ In deploy (partition_rejected partition).

Definition exact_partition (partition : admission_partition) : Prop :=
  NoDup (partition_candidates partition) /\
  NoDup (partition_admitted partition) /\
  NoDup (partition_rejected partition) /\
  partition_covers_candidates partition /\
  partition_is_disjoint partition.

Record verified_admission_partition := {
  verified_partition_value : admission_partition;
  verified_partition_exact : exact_partition verified_partition_value;
  verified_context_id : nat;
  verified_candidate_set_id : nat
}.

Record validated_replay_artifacts := {
  replay_partition : verified_admission_partition;
  replay_processed : list deploy_id_v6;
  replay_processed_exact :
    replay_processed =
      partition_admitted
        (verified_partition_value replay_partition);
  replay_pre_state : nat;
  replay_post_state : nat;
  replay_execution_id : nat;
  replay_payload_digest : nat;
  replay_evidence_digest : nat;
  replay_evidence : nat
}.

Definition legacy_primary_signature (_ : deploy_id_v6) : nat := 0.

Definition evidence_store := nat -> option nat.

Definition store_insert
  (key value : nat)
  (store : evidence_store)
  : evidence_store :=
  fun query => if Nat.eq_dec query key then Some value else store query.

Inductive publication_result :=
| Published (store : evidence_store)
| PublicationConflict (store : evidence_store).

Definition publish_validated
  (artifacts : validated_replay_artifacts)
  (store : evidence_store)
  : publication_result :=
  let key := replay_execution_id artifacts in
  let value := replay_evidence artifacts in
  match store key with
  | None => Published (store_insert key value store)
  | Some existing =>
      if Nat.eq_dec existing value
      then Published store
      else PublicationConflict store
  end.

Definition peer_bytes_cannot_publish
  (_key value : nat)
  (store : evidence_store)
  : evidence_store := store.

Definition cache_store := nat -> option nat.

Definition cache_after_durable
  (artifacts : validated_replay_artifacts)
  (store : evidence_store)
  (proof :
    store (replay_execution_id artifacts) =
      Some (replay_evidence artifacts))
  (cache : cache_store)
  : cache_store :=
  fun query =>
    if Nat.eq_dec query (replay_execution_id artifacts)
    then Some (replay_evidence artifacts)
    else cache query.

Definition transactional_crash_outcome
  (before after observed : evidence_store)
  : Prop := observed = before \/ observed = after.

Theorem verified_partition_covers_every_candidate :
  forall verified deploy,
    In deploy
      (partition_candidates (verified_partition_value verified)) <->
    In deploy
      (partition_admitted (verified_partition_value verified)) \/
    In deploy
      (partition_rejected (verified_partition_value verified)).
Proof.
  intros verified deploy.
  destruct (verified_partition_exact verified) as [_ [_ [_ [covers _]]]].
  now apply covers.
Qed.

Theorem verified_partition_has_no_dual_disposition :
  forall verified deploy,
    In deploy
      (partition_admitted (verified_partition_value verified)) ->
    ~ In deploy
      (partition_rejected (verified_partition_value verified)).
Proof.
  intros verified deploy admitted.
  destruct (verified_partition_exact verified) as [_ [_ [_ [_ disjoint]]]].
  now apply disjoint.
Qed.

Theorem processed_evidence_is_exactly_admitted :
  forall artifacts,
    replay_processed artifacts =
      partition_admitted
        (verified_partition_value (replay_partition artifacts)).
Proof.
  apply replay_processed_exact.
Qed.

Theorem count_equality_does_not_establish_identity_equality :
  exists expected observed : list deploy_id_v6,
    length expected = length observed /\ expected <> observed.
Proof.
  exists [DeployIdV6 1], [DeployIdV6 2].
  split.
  - reflexivity.
  - discriminate.
Qed.

Theorem primary_signature_identity_is_not_injective :
  exists left right : deploy_id_v6,
    left <> right /\
    legacy_primary_signature left = legacy_primary_signature right.
Proof.
  exists (DeployIdV6 1), (DeployIdV6 2).
  split.
  - discriminate.
  - reflexivity.
Qed.

Theorem typed_deploy_identity_is_injective :
  forall left right,
    left = right ->
    match left, right with
    | DeployIdV6 left_digest, DeployIdV6 right_digest =>
        left_digest = right_digest
    end.
Proof.
  intros left right equal.
  subst right.
  now destruct left.
Qed.

Definition evidence_identity (deploy : deploy_id_v6) : deploy_id_v6 := deploy.
Definition reservation_identity (deploy : deploy_id_v6) : deploy_id_v6 := deploy.
Definition fee_identity (deploy : deploy_id_v6) : deploy_id_v6 := deploy.
Definition rng_identity (deploy : deploy_id_v6) : deploy_id_v6 := deploy.

Theorem evidence_identity_is_injective :
  forall left right,
    evidence_identity left = evidence_identity right -> left = right.
Proof.
  exact (fun left right equal => equal).
Qed.

Theorem reservation_identity_is_injective :
  forall left right,
    reservation_identity left = reservation_identity right -> left = right.
Proof.
  exact (fun left right equal => equal).
Qed.

Theorem fee_identity_is_injective :
  forall left right,
    fee_identity left = fee_identity right -> left = right.
Proof.
  exact (fun left right equal => equal).
Qed.

Theorem rng_identity_is_injective :
  forall left right,
    rng_identity left = rng_identity right -> left = right.
Proof.
  exact (fun left right equal => equal).
Qed.

Record v6_reservation_binding := {
  reservation_pre_state : nat;
  reservation_program : nat;
  reservation_deploy : deploy_id_v6
}.

Definition v6_reservation_binding_eq_dec :
  forall left right : v6_reservation_binding, {left = right} + {left <> right}.
Proof.
  decide equality; try apply deploy_id_v6_eq_dec; apply Nat.eq_dec.
Defined.

Definition verify_v6_reservation
  (expected actual : v6_reservation_binding) : bool :=
  if v6_reservation_binding_eq_dec expected actual then true else false.

Theorem exact_v6_reservation_is_accepted :
  forall binding, verify_v6_reservation binding binding = true.
Proof.
  intros binding.
  unfold verify_v6_reservation.
  destruct (v6_reservation_binding_eq_dec binding binding); congruence.
Qed.

Theorem mutated_v6_reservation_is_rejected :
  forall expected actual,
    expected <> actual -> verify_v6_reservation expected actual = false.
Proof.
  intros expected actual different.
  unfold verify_v6_reservation.
  destruct (v6_reservation_binding_eq_dec expected actual); congruence.
Qed.

Definition evidence_consumed_exactly_once
  (expected supplied : list deploy_id_v6) : Prop :=
  supplied = expected /\ NoDup supplied.

Theorem exact_evidence_consumption_rejects_extra_entries :
  forall expected supplied extra,
    evidence_consumed_exactly_once expected supplied ->
    supplied <> expected ++ [extra].
Proof.
  intros expected supplied extra [same _] appended.
  subst supplied.
  apply (f_equal (@length deploy_id_v6)) in appended.
  rewrite length_app in appended.
  simpl in appended.
  lia.
Qed.

Theorem exact_evidence_consumption_rejects_duplicates :
  forall expected supplied deploy,
    evidence_consumed_exactly_once expected supplied ->
    In deploy supplied ->
    ~ evidence_consumed_exactly_once expected (deploy :: supplied).
Proof.
  intros expected supplied deploy [_ unique] present [_ duplicate].
  inversion duplicate; contradiction.
Qed.

Theorem absent_publication_inserts_complete_evidence :
  forall artifacts store,
    store (replay_execution_id artifacts) = None ->
    exists published,
      publish_validated artifacts store = Published published /\
      published (replay_execution_id artifacts) =
        Some (replay_evidence artifacts).
Proof.
  intros artifacts store absent.
  unfold publish_validated.
  rewrite absent.
  eexists.
  split.
  - reflexivity.
  - unfold store_insert.
    destruct
      (Nat.eq_dec
        (replay_execution_id artifacts)
        (replay_execution_id artifacts)); congruence.
Qed.

Theorem identical_publication_is_idempotent :
  forall artifacts store,
    store (replay_execution_id artifacts) =
      Some (replay_evidence artifacts) ->
    publish_validated artifacts store = Published store.
Proof.
  intros artifacts store identical.
  unfold publish_validated.
  rewrite identical.
  destruct
    (Nat.eq_dec
      (replay_evidence artifacts)
      (replay_evidence artifacts)); congruence.
Qed.

Theorem conflicting_publication_preserves_existing_store :
  forall artifacts store existing,
    store (replay_execution_id artifacts) = Some existing ->
    existing <> replay_evidence artifacts ->
    publish_validated artifacts store = PublicationConflict store.
Proof.
  intros artifacts store existing present different.
  unfold publish_validated.
  rewrite present.
  destruct
    (Nat.eq_dec existing (replay_evidence artifacts)); congruence.
Qed.

Theorem peer_bytes_leave_store_unchanged :
  forall key value store query,
    peer_bytes_cannot_publish key value store query = store query.
Proof.
  reflexivity.
Qed.

Theorem durable_proof_precedes_cache_entry :
  forall artifacts store durable cache,
    cache_after_durable artifacts store durable cache
      (replay_execution_id artifacts) =
    Some (replay_evidence artifacts).
Proof.
  intros artifacts store durable cache.
  unfold cache_after_durable.
  destruct
    (Nat.eq_dec
      (replay_execution_id artifacts)
      (replay_execution_id artifacts)); congruence.
Qed.

Theorem crash_exposes_before_or_after_transaction :
  forall before after observed key,
    transactional_crash_outcome before after observed ->
    observed key = before key \/ observed key = after key.
Proof.
  intros before after observed key outcome.
  destruct outcome as [same | same]; subst observed; auto.
Qed.

Theorem validators_with_equal_artifacts_publish_equal_evidence :
  forall left right,
    left = right ->
    replay_execution_id left = replay_execution_id right /\
    replay_evidence_digest left = replay_evidence_digest right /\
    replay_evidence left = replay_evidence right.
Proof.
  intros left right equal.
  now subst right.
Qed.
