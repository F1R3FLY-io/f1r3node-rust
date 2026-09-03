From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lists.List.
Import ListNotations.

Definition legacy_protocol : nat := 5.
Definition current_protocol : nat := 6.

Definition supported_protocol (version : nat) : Prop :=
  version = current_protocol.

Definition supported_protocolb (version : nat) : bool :=
  Nat.eqb version current_protocol.

Definition ceremony_candidate (configured_version : nat) : nat :=
  configured_version.

Definition approver_accepts
  (configured_version candidate_version : nat) : bool :=
  Nat.eqb candidate_version configured_version.

Definition admit_approved (approved_version : nat) : option nat :=
  if supported_protocolb approved_version
  then Some approved_version
  else None.

Definition adopt_approved
  (_local_version approved_version : nat) : nat :=
  approved_version.

Definition adopt_network
  (approved_version : nat)
  (local_versions : list nat) : list nat :=
  map (fun local_version => adopt_approved local_version approved_version)
      local_versions.

Definition proposal_version (running_version : nat) : nat :=
  running_version.

Definition receiver_accepts
  (running_version proposed_version : nat) : bool :=
  Nat.eqb running_version proposed_version.

Inductive genesis_identity : Type :=
| LegacyBlessedIdentity
| ProtocolEnvelopeIdentity.

Definition genesis_occurrence_identity (version : nat) : genesis_identity :=
  if supported_protocolb version
  then ProtocolEnvelopeIdentity
  else LegacyBlessedIdentity.

Definition genesis_execution_identity (version : nat) : genesis_identity :=
  if supported_protocolb version
  then ProtocolEnvelopeIdentity
  else LegacyBlessedIdentity.

Definition genesis_replay_identity (version : nat) : genesis_identity :=
  genesis_execution_identity version.

Inductive genesis_deployer_identity : Type :=
| LegacyGroundDeployer (public_key : list nat)
| PrincipalDeployer (key_family : nat) (public_key : list nat)
| CompoundAuthorityDeployer.

Definition project_ground_custody
  (identity : genesis_deployer_identity) : option (list nat) :=
  match identity with
  | LegacyGroundDeployer public_key => Some public_key
  | PrincipalDeployer key_family public_key =>
      if Nat.eqb key_family 1 then Some public_key else None
  | CompoundAuthorityDeployer => None
  end.

Inductive funding_ground_wire : Type :=
| LegacyFundingGround
    (public_key : list nat)
    (canonical_key : bool)
| ProtocolFundingGround
    (key_family declared_length : nat)
    (public_key : list nat)
    (canonical_key : bool).

Definition project_funding_ground_custody
  (ground : funding_ground_wire) : option (list nat) :=
  match ground with
  | LegacyFundingGround public_key canonical_key =>
      if canonical_key then Some public_key else None
  | ProtocolFundingGround key_family declared_length public_key canonical_key =>
      if Nat.eqb key_family 1
         && Nat.eqb declared_length (length public_key)
         && canonical_key
      then Some public_key
      else None
  end.

Theorem supported_protocolb_spec :
  forall version,
    supported_protocolb version = true <-> supported_protocol version.
Proof.
  intros version.
  unfold supported_protocolb, supported_protocol.
  apply Nat.eqb_eq.
Qed.

Theorem unsupported_approved_fails_closed :
  forall version,
    ~ supported_protocol version ->
    admit_approved version = None.
Proof.
  intros version Hunsupported.
  unfold admit_approved.
  destruct (supported_protocolb version) eqn:Hsupported.
  - apply supported_protocolb_spec in Hsupported.
    contradiction.
  - reflexivity.
Qed.

Theorem supported_approved_is_admitted_exactly :
  forall version,
    supported_protocol version ->
    admit_approved version = Some version.
Proof.
  intros version Hsupported.
  unfold admit_approved.
  apply supported_protocolb_spec in Hsupported.
  rewrite Hsupported.
  reflexivity.
Qed.

Theorem legacy_approved_fails_closed :
  admit_approved legacy_protocol = None.
Proof.
  apply unsupported_approved_fails_closed.
  unfold supported_protocol, legacy_protocol, current_protocol.
  discriminate.
Qed.

Theorem mismatched_candidate_is_not_approved :
  forall configured_version candidate_version,
    candidate_version <> configured_version ->
    approver_accepts configured_version candidate_version = false.
Proof.
  intros configured_version candidate_version Hmismatch.
  unfold approver_accepts.
  apply Nat.eqb_neq.
  exact Hmismatch.
Qed.

Theorem configured_candidate_is_approved :
  forall configured_version,
    approver_accepts configured_version
      (ceremony_candidate configured_version) = true.
Proof.
  intros configured_version.
  unfold approver_accepts, ceremony_candidate.
  apply Nat.eqb_refl.
Qed.

Theorem adoption_erases_local_version_drift :
  forall local_version approved_version,
    adopt_approved local_version approved_version = approved_version.
Proof.
  reflexivity.
Qed.

Theorem network_adoption_is_uniform :
  forall approved_version local_versions,
    adopt_network approved_version local_versions =
    repeat approved_version (length local_versions).
Proof.
  intros approved_version local_versions.
  induction local_versions as [|local_version local_versions IH].
  - reflexivity.
  - simpl. rewrite IH. reflexivity.
Qed.

Theorem all_receivers_accept_approved_proposal :
  forall approved_version local_versions,
    Forall
      (fun running_version =>
        receiver_accepts running_version
          (proposal_version approved_version) = true)
      (adopt_network approved_version local_versions).
Proof.
  intros approved_version local_versions.
  induction local_versions as [|local_version local_versions IH].
  - constructor.
  - simpl. constructor.
    + unfold receiver_accepts, proposal_version, adopt_approved.
      apply Nat.eqb_refl.
    + exact IH.
Qed.

Theorem current_ceremony_end_to_end :
  forall candidate_version approved_version local_versions,
    candidate_version = ceremony_candidate current_protocol ->
    approver_accepts current_protocol candidate_version = true ->
    approved_version = candidate_version ->
    approved_version = current_protocol /\
    adopt_network approved_version local_versions =
      repeat current_protocol (length local_versions) /\
    Forall
      (fun running_version =>
        receiver_accepts running_version
          (proposal_version approved_version) = true)
      (adopt_network approved_version local_versions).
Proof.
  intros candidate_version approved_version local_versions.
  intros Hcandidate _ Happroved.
  unfold ceremony_candidate in Hcandidate.
  subst candidate_version.
  subst approved_version.
  split.
  - reflexivity.
  - split.
    + apply network_adoption_is_uniform.
    + apply all_receivers_accept_approved_proposal.
Qed.

Theorem supported_recovery_end_to_end :
  forall approved_version local_versions,
    supported_protocol approved_version ->
    admit_approved approved_version = Some approved_version /\
    adopt_network approved_version local_versions =
      repeat approved_version (length local_versions) /\
    Forall
      (fun running_version =>
        receiver_accepts running_version
          (proposal_version approved_version) = true)
      (adopt_network approved_version local_versions).
Proof.
  intros approved_version local_versions Hsupported.
  split.
  - apply supported_approved_is_admitted_exactly.
    exact Hsupported.
  - split.
    + apply network_adoption_is_uniform.
    + apply all_receivers_accept_approved_proposal.
Qed.

Theorem current_genesis_occurrence_is_envelope_bound :
  genesis_occurrence_identity current_protocol = ProtocolEnvelopeIdentity.
Proof.
  unfold genesis_occurrence_identity, supported_protocolb.
  rewrite Nat.eqb_refl.
  reflexivity.
Qed.

Theorem current_genesis_execution_is_envelope_bound :
  genesis_execution_identity current_protocol = ProtocolEnvelopeIdentity.
Proof.
  unfold genesis_execution_identity, supported_protocolb.
  rewrite Nat.eqb_refl.
  reflexivity.
Qed.

Theorem genesis_replay_matches_construction :
  forall version,
    genesis_replay_identity version = genesis_execution_identity version.
Proof.
  intros version.
  unfold genesis_replay_identity.
  reflexivity.
Qed.

Theorem legacy_genesis_identity_is_byte_preserving :
  genesis_occurrence_identity legacy_protocol = LegacyBlessedIdentity /\
  genesis_execution_identity legacy_protocol = LegacyBlessedIdentity /\
  genesis_replay_identity legacy_protocol = LegacyBlessedIdentity.
Proof.
  repeat split; reflexivity.
Qed.

Theorem protocol_principal_projects_same_ground_custody :
  forall public_key,
    project_ground_custody (PrincipalDeployer 1 public_key) =
    project_ground_custody (LegacyGroundDeployer public_key).
Proof.
  intros public_key.
  reflexivity.
Qed.

Theorem non_custody_principal_is_rejected :
  forall key_family public_key,
    key_family <> 1 ->
    project_ground_custody (PrincipalDeployer key_family public_key) = None.
Proof.
  intros key_family public_key Hfamily.
  unfold project_ground_custody.
  apply Nat.eqb_neq in Hfamily.
  rewrite Hfamily.
  reflexivity.
Qed.

Theorem compound_authority_is_not_ground_custody :
  project_ground_custody CompoundAuthorityDeployer = None.
Proof. reflexivity. Qed.

Theorem current_genesis_identity_end_to_end :
  genesis_occurrence_identity current_protocol = ProtocolEnvelopeIdentity /\
  genesis_execution_identity current_protocol = ProtocolEnvelopeIdentity /\
  genesis_replay_identity current_protocol = ProtocolEnvelopeIdentity /\
  genesis_replay_identity current_protocol =
    genesis_execution_identity current_protocol /\
  (forall public_key,
    project_ground_custody (PrincipalDeployer 1 public_key) =
    project_ground_custody (LegacyGroundDeployer public_key)).
Proof.
  split.
  - exact current_genesis_occurrence_is_envelope_bound.
  - split.
    + exact current_genesis_execution_is_envelope_bound.
    + split.
      * reflexivity.
      * split.
        -- apply genesis_replay_matches_construction.
        -- apply protocol_principal_projects_same_ground_custody.
Qed.

Theorem canonical_protocol_funding_projects_legacy_custody :
  forall public_key,
    project_funding_ground_custody
      (ProtocolFundingGround 1 (length public_key) public_key true) =
    project_funding_ground_custody
      (LegacyFundingGround public_key true).
Proof.
  intros public_key.
  simpl.
  rewrite Nat.eqb_refl.
  reflexivity.
Qed.

Theorem unsupported_funding_family_is_rejected :
  forall key_family declared_length public_key canonical_key,
    key_family <> 1 ->
    project_funding_ground_custody
      (ProtocolFundingGround
        key_family declared_length public_key canonical_key) = None.
Proof.
  intros key_family declared_length public_key canonical_key Hfamily.
  simpl.
  apply Nat.eqb_neq in Hfamily.
  rewrite Hfamily.
  reflexivity.
Qed.

Theorem incorrect_funding_key_length_is_rejected :
  forall declared_length public_key canonical_key,
    declared_length <> length public_key ->
    project_funding_ground_custody
      (ProtocolFundingGround
        1 declared_length public_key canonical_key) = None.
Proof.
  intros declared_length public_key canonical_key Hlength.
  simpl.
  apply Nat.eqb_neq in Hlength.
  rewrite Hlength.
  reflexivity.
Qed.

Theorem noncanonical_funding_key_is_rejected :
  forall key_family declared_length public_key,
    project_funding_ground_custody
      (ProtocolFundingGround
        key_family declared_length public_key false) = None.
Proof.
  intros key_family declared_length public_key.
  simpl.
  destruct (Nat.eqb key_family 1);
    destruct (Nat.eqb declared_length (length public_key));
    reflexivity.
Qed.

Theorem funding_ground_custody_projection_correct :
  (forall public_key,
    project_funding_ground_custody
      (ProtocolFundingGround 1 (length public_key) public_key true) =
    project_funding_ground_custody
      (LegacyFundingGround public_key true))
  /\
  (forall key_family declared_length public_key canonical_key,
    key_family <> 1 ->
    project_funding_ground_custody
      (ProtocolFundingGround
        key_family declared_length public_key canonical_key) = None)
  /\
  (forall declared_length public_key canonical_key,
    declared_length <> length public_key ->
    project_funding_ground_custody
      (ProtocolFundingGround
        1 declared_length public_key canonical_key) = None)
  /\
  (forall key_family declared_length public_key,
    project_funding_ground_custody
      (ProtocolFundingGround
        key_family declared_length public_key false) = None).
Proof.
  exact (conj canonical_protocol_funding_projects_legacy_custody
    (conj unsupported_funding_family_is_rejected
      (conj incorrect_funding_key_length_is_rejected
        noncanonical_funding_key_is_rejected))).
Qed.

Print Assumptions funding_ground_custody_projection_correct.

Print Assumptions supported_protocolb_spec.
Print Assumptions unsupported_approved_fails_closed.
Print Assumptions supported_approved_is_admitted_exactly.
Print Assumptions legacy_approved_fails_closed.
Print Assumptions mismatched_candidate_is_not_approved.
Print Assumptions configured_candidate_is_approved.
Print Assumptions adoption_erases_local_version_drift.
Print Assumptions network_adoption_is_uniform.
Print Assumptions all_receivers_accept_approved_proposal.
Print Assumptions current_ceremony_end_to_end.
Print Assumptions supported_recovery_end_to_end.
Print Assumptions current_genesis_occurrence_is_envelope_bound.
Print Assumptions current_genesis_execution_is_envelope_bound.
Print Assumptions genesis_replay_matches_construction.
Print Assumptions legacy_genesis_identity_is_byte_preserving.
Print Assumptions protocol_principal_projects_same_ground_custody.
Print Assumptions non_custody_principal_is_rejected.
Print Assumptions compound_authority_is_not_ground_custody.
Print Assumptions current_genesis_identity_end_to_end.
