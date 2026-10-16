From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lists.List.
Import ListNotations.

Definition legacy_protocol : nat := 1.
(** Ratified D-01 selects protocol 7 for the next fresh-genesis release;
    external FIP approval remains a release precondition, not a model axiom. *)
Definition current_protocol : nat := 7.

(** Genesis-policy domains are disjoint.  The v2 domain remains a decoder for
    protocol 6 history; the v3 domain is the protocol 7 fresh-genesis format.
    Merely changing the schedule's version must not reinterpret v2 bytes. *)
Inductive genesis_policy_domain :=
  | PolicyV1
  | PolicyV2
  | PolicyV3.

Inductive event_codec_epoch :=
  | LegacyEventCodec
  | CostV1EventCodec
  | CostV2EventCodec.

Definition event_epoch_for_policy (domain : genesis_policy_domain)
  : event_codec_epoch :=
  match domain with
  | PolicyV1 => LegacyEventCodec
  | PolicyV2 => CostV1EventCodec
  | PolicyV3 => CostV2EventCodec
  end.

Definition activated_policy_domain (version : nat)
  : option genesis_policy_domain :=
  if Nat.eqb version 6 then Some PolicyV2
  else if Nat.eqb version current_protocol then Some PolicyV3
  else None.

(** Adoption seals the epoch once from the authenticated policy.  An inactive
    historical policy keeps Legacy regardless of the schedule version; an
    active record is admitted only by its exact versioned domain. *)
Definition adopted_event_epoch (activated : bool) (version : nat)
  : option event_codec_epoch :=
  if activated then
    match activated_policy_domain version with
    | Some domain => Some (event_epoch_for_policy domain)
    | None => None
    end
  else Some LegacyEventCodec.

Definition decode_activated_policy
  (domain : genesis_policy_domain) (version : nat) : option nat :=
  match domain with
  | PolicyV1 => None
  | PolicyV2 => if Nat.eqb version 6 then Some version else None
  | PolicyV3 =>
      if Nat.eqb version current_protocol then Some version else None
  end.

Theorem protocol_6_policy_keeps_v2_domain :
  activated_policy_domain 6 = Some PolicyV2.
Proof. reflexivity. Qed.

Theorem current_policy_uses_fresh_v3_domain :
  activated_policy_domain current_protocol = Some PolicyV3.
Proof. reflexivity. Qed.

Theorem retired_domain_cannot_decode_current_protocol :
  decode_activated_policy PolicyV2 current_protocol = None.
Proof. reflexivity. Qed.

Theorem current_domain_cannot_decode_retired_protocol :
  decode_activated_policy PolicyV3 6 = None.
Proof. reflexivity. Qed.

Theorem activated_domains_select_distinct_event_epochs :
  event_epoch_for_policy PolicyV2 = CostV1EventCodec /\
  event_epoch_for_policy PolicyV3 = CostV2EventCodec /\
  CostV1EventCodec <> CostV2EventCodec.
Proof. repeat split; discriminate || reflexivity. Qed.

Theorem adopted_epoch_is_exactly_policy_versioned :
  (forall version,
    adopted_event_epoch false version = Some LegacyEventCodec) /\
  adopted_event_epoch true 6 = Some CostV1EventCodec /\
  adopted_event_epoch true current_protocol = Some CostV2EventCodec /\
  (forall version,
    version <> 6 -> version <> current_protocol ->
    adopted_event_epoch true version = None).
Proof.
  split; [intros version; reflexivity |].
  split; [reflexivity |].
  split; [reflexivity |].
  intros version Hsix Hcurrent.
  apply Nat.eqb_neq in Hsix.
  apply Nat.eqb_neq in Hcurrent.
  unfold adopted_event_epoch, activated_policy_domain.
  rewrite Hsix, Hcurrent.
  reflexivity.
Qed.

Theorem activated_policy_round_trip :
  forall version domain,
    activated_policy_domain version = Some domain ->
    decode_activated_policy domain version = Some version.
Proof.
  intros version domain Hdomain.
  unfold activated_policy_domain in Hdomain.
  destruct (Nat.eqb version 6) eqn:Hsix.
  - inversion Hdomain; subst. unfold decode_activated_policy.
    rewrite Hsix. reflexivity.
  - destruct (Nat.eqb version current_protocol) eqn:Hcurrent.
    + inversion Hdomain; subst. unfold decode_activated_policy.
      rewrite Hcurrent. reflexivity.
    + discriminate.
Qed.

Theorem accepted_activated_policy_has_exact_domain_and_version :
  forall domain version,
    decode_activated_policy domain version = Some version ->
    (domain = PolicyV2 /\ version = 6) \/
    (domain = PolicyV3 /\ version = current_protocol).
Proof.
  intros domain version Haccepted.
  destruct domain.
  - discriminate.
  - simpl in Haccepted.
    destruct (Nat.eqb version 6) eqn:Hsix; try discriminate.
    apply Nat.eqb_eq in Hsix.
    left. split; [reflexivity | exact Hsix].
  - simpl in Haccepted.
    destruct (Nat.eqb version current_protocol) eqn:Hcurrent;
      try discriminate.
    apply Nat.eqb_eq in Hcurrent.
    right. split; [reflexivity | exact Hcurrent].
Qed.

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
Print Assumptions protocol_6_policy_keeps_v2_domain.
Print Assumptions current_policy_uses_fresh_v3_domain.
Print Assumptions retired_domain_cannot_decode_current_protocol.
Print Assumptions current_domain_cannot_decode_retired_protocol.
Print Assumptions activated_domains_select_distinct_event_epochs.
Print Assumptions adopted_epoch_is_exactly_policy_versioned.
Print Assumptions activated_policy_round_trip.
Print Assumptions accepted_activated_policy_has_exact_domain_and_version.
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
