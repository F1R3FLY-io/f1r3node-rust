From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Lia.
Import ListNotations.

From FinalizedFloor Require Import Foundation.

Definition BondGeneration := nat.

Record ExactParentAuthority : Type := mkExactParentAuthority {
  exact_authority_root : BlockHash;
  exact_authority_validator : Validator;
  exact_authority_generation : BondGeneration;
  exact_authority_stake : nat
}.

Record SignedSenderGenerationClaim : Type := mkSignedSenderGenerationClaim {
  claim_block_hash : BlockHash;
  claim_protocol_version : nat;
  claim_parent_root : BlockHash;
  claim_sender : Validator;
  claim_generation : BondGeneration;
  claim_sequence : nat
}.

Record SenderGenerationCertificate : Type := mkSenderGenerationCertificate {
  certificate_block_hash : BlockHash;
  certificate_protocol_version : nat;
  certificate_parent_root : BlockHash;
  certificate_sender : Validator;
  certificate_generation : BondGeneration;
  certificate_sequence : nat;
  certificate_stake : nat
}.

Inductive sender_generation_certified :
  ExactParentAuthority ->
  SignedSenderGenerationClaim ->
  SenderGenerationCertificate ->
  Prop :=
| CertifySenderGeneration : forall authority claim,
    claim_parent_root claim = exact_authority_root authority ->
    claim_sender claim = exact_authority_validator authority ->
    claim_generation claim = exact_authority_generation authority ->
    exact_authority_stake authority > 0 ->
    sender_generation_certified authority claim
      (mkSenderGenerationCertificate
        (claim_block_hash claim)
        (claim_protocol_version claim)
        (exact_authority_root authority)
        (exact_authority_validator authority)
        (exact_authority_generation authority)
        (claim_sequence claim)
        (exact_authority_stake authority)).

Theorem sender_certificate_uses_exact_parent_root :
  forall authority claim certificate,
    sender_generation_certified authority claim certificate ->
    certificate_parent_root certificate = exact_authority_root authority.
Proof. intros authority claim certificate Hcertificate; inversion Hcertificate; reflexivity. Qed.

Theorem sender_certificate_generation_is_parent_derived :
  forall authority claim certificate,
    sender_generation_certified authority claim certificate ->
    certificate_generation certificate = exact_authority_generation authority /\
    claim_generation claim = exact_authority_generation authority.
Proof.
  intros authority claim certificate Hcertificate.
  inversion Hcertificate; subst; simpl. auto.
Qed.

Theorem sender_certificate_requires_positive_authority :
  forall authority claim certificate,
    sender_generation_certified authority claim certificate ->
    certificate_stake certificate > 0.
Proof.
  intros authority claim certificate Hcertificate.
  inversion Hcertificate; subst; simpl; assumption.
Qed.

Theorem mismatched_header_generation_cannot_be_certified :
  forall authority claim certificate,
    claim_generation claim <> exact_authority_generation authority ->
    ~ sender_generation_certified authority claim certificate.
Proof.
  intros authority claim certificate Hmismatch Hcertificate.
  apply Hmismatch.
  destruct (@sender_certificate_generation_is_parent_derived
    authority claim certificate Hcertificate) as [_ Hclaim].
  exact Hclaim.
Qed.

Inductive CertifiedDisposition : Type :=
| DispositionAccepted
| DispositionSlashableInvalid.

Record CertifiedBlockRecord : Type := mkCertifiedBlockRecord {
  certified_record_certificate : SenderGenerationCertificate;
  certified_record_disposition : CertifiedDisposition
}.

Inductive AdmissionRequest : Type :=
| CertifiedApprovedGenesisAdmission
| CertifiedEvidenceBlockAdmission (record : CertifiedBlockRecord)
| CertifiedUncertifiedDiagnosticAdmission (block_hash : BlockHash).

Definition admission_certificate
  (request : AdmissionRequest) : option SenderGenerationCertificate :=
  match request with
  | CertifiedEvidenceBlockAdmission record => Some (certified_record_certificate record)
  | _ => None
  end.

Theorem non_genesis_evidence_admission_has_certificate :
  forall record,
    admission_certificate (CertifiedEvidenceBlockAdmission record) =
      Some (certified_record_certificate record).
Proof. reflexivity. Qed.

Theorem diagnostic_invalid_has_no_evidence_certificate :
  forall block_hash,
    admission_certificate (CertifiedUncertifiedDiagnosticAdmission block_hash) = None.
Proof. reflexivity. Qed.

Record CertifiedObservation : Type := mkCertifiedObservation {
  observation_hash : BlockHash;
  observation_sender : Validator;
  observation_generation : BondGeneration;
  observation_sequence : nat;
  observation_parent_root : BlockHash
}.

Definition observation_of_certificate
  (certificate : SenderGenerationCertificate) : CertifiedObservation :=
  mkCertifiedObservation
    (certificate_block_hash certificate)
    (certificate_sender certificate)
    (certificate_generation certificate)
    (certificate_sequence certificate)
    (certificate_parent_root certificate).

Definition same_certified_fault_key
  (left right : CertifiedObservation) : Prop :=
  observation_sender left = observation_sender right /\
  observation_generation left = observation_generation right /\
  observation_sequence left = observation_sequence right.

Definition certified_objective_pair
  (left right : CertifiedObservation) : Prop :=
  observation_hash left <> observation_hash right /\
  same_certified_fault_key left right.

Theorem certified_pair_has_identical_validator_generation_sequence :
  forall left right,
    certified_objective_pair left right ->
    observation_sender left = observation_sender right /\
    observation_generation left = observation_generation right /\
    observation_sequence left = observation_sequence right.
Proof.
  intros left right [_ Hkey]. exact Hkey.
Qed.

Theorem certified_objective_pair_is_symmetric :
  forall left right,
    certified_objective_pair left right ->
    certified_objective_pair right left.
Proof.
  intros left right [Hhash [Hsender [Hgeneration Hsequence]]].
  repeat split; congruence.
Qed.

Definition canonical_observation_hash_pair
  (left right : CertifiedObservation) : BlockHash * BlockHash :=
  (Nat.min (observation_hash left) (observation_hash right),
   Nat.max (observation_hash left) (observation_hash right)).

Theorem canonical_observation_pair_is_arrival_independent :
  forall left right,
    canonical_observation_hash_pair left right =
    canonical_observation_hash_pair right left.
Proof.
  intros left right. unfold canonical_observation_hash_pair.
  rewrite Nat.min_comm, Nat.max_comm. reflexivity.
Qed.

Record CertifiedStorage : Type := mkCertifiedStorage {
  durable_certified_metadata : list CertifiedBlockRecord;
  durable_evidence_index : list CertifiedObservation
}.

Definition observations_from_metadata
  (metadata : list CertifiedBlockRecord) : list CertifiedObservation :=
  map
    (fun record =>
      observation_of_certificate (certified_record_certificate record))
    metadata.

Definition repair_certified_secondary_indexes
  (storage : CertifiedStorage) : CertifiedStorage :=
  mkCertifiedStorage
    (durable_certified_metadata storage)
    (observations_from_metadata (durable_certified_metadata storage)).

Definition certified_index_complete (storage : CertifiedStorage) : Prop :=
  forall record,
    In record (durable_certified_metadata storage) ->
    In (observation_of_certificate (certified_record_certificate record))
       (durable_evidence_index storage).

Theorem repair_makes_certified_index_complete :
  forall storage,
    certified_index_complete (repair_certified_secondary_indexes storage).
Proof.
  intros storage.
  unfold certified_index_complete, repair_certified_secondary_indexes; simpl.
  intros record Hrecord. unfold observations_from_metadata.
  apply in_map_iff.
  exists record. split; [reflexivity | exact Hrecord].
Qed.

Definition duplicate_retry_repair
  (storage : CertifiedStorage) : CertifiedStorage :=
  repair_certified_secondary_indexes storage.

Theorem duplicate_retry_repairs_metadata_index_crash_window :
  forall storage,
    certified_index_complete (duplicate_retry_repair storage).
Proof. exact repair_makes_certified_index_complete. Qed.

Definition metadata_certificates_sound
  (authorities : BlockHash -> option ExactParentAuthority)
  (claims : BlockHash -> option SignedSenderGenerationClaim)
  (storage : CertifiedStorage) : Prop :=
  forall record,
    In record (durable_certified_metadata storage) ->
    exists authority claim,
      authorities
        (certificate_block_hash (certified_record_certificate record)) =
        Some authority /\
      claims
        (certificate_block_hash (certified_record_certificate record)) =
        Some claim /\
      sender_generation_certified authority claim
        (certified_record_certificate record).

Theorem repaired_evidence_is_derived_from_sound_metadata :
  forall authorities claims storage observation,
    metadata_certificates_sound authorities claims storage ->
    In observation
      (durable_evidence_index (repair_certified_secondary_indexes storage)) ->
    exists record authority claim,
      In record (durable_certified_metadata storage) /\
      observation =
        observation_of_certificate (certified_record_certificate record) /\
      authorities
        (certificate_block_hash (certified_record_certificate record)) =
        Some authority /\
      claims
        (certificate_block_hash (certified_record_certificate record)) =
        Some claim /\
      sender_generation_certified authority claim
        (certified_record_certificate record).
Proof.
  intros authorities claims storage observation Hsound Hobservation.
  unfold repair_certified_secondary_indexes in Hobservation; simpl in Hobservation.
  unfold observations_from_metadata in Hobservation.
  apply in_map_iff in Hobservation.
  destruct Hobservation as [record [Hobservation Hrecord]].
  specialize (Hsound record Hrecord).
  destruct Hsound as [authority [claim [Hauthority [Hclaim Hcertificate]]]].
  exists record, authority, claim. repeat split; auto.
Qed.

Print Assumptions sender_certificate_generation_is_parent_derived.
Print Assumptions mismatched_header_generation_cannot_be_certified.
Print Assumptions repaired_evidence_is_derived_from_sound_metadata.
