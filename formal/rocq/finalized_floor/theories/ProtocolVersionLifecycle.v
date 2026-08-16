From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lists.List.
Import ListNotations.

Definition legacy_protocol : nat := 1.
Definition current_protocol : nat := 2.

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
