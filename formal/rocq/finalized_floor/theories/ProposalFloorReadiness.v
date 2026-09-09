From Stdlib Require Import Bool.Bool.
From FinalizedFloor Require Import CertifiedReplayAnchor.

Inductive certified_context_relation : Type :=
| MatchingContext
| StrictStatePreservingDescendant
| RegressiveFloor
| ConflictingFloor
| MismatchedSameFloorContext.

Inductive recovery_deferral_reason : Type :=
| ProposalReady
| CertifiedSlotsIncomplete
| CertifiedValidatorInactive
| RecoveryPermitStale.

Definition classify_proposal_readiness
    (permit_required permit_fresh : bool)
    (_ : certified_context_relation)
    (slots_complete proposer_active : bool)
    : recovery_deferral_reason :=
  if andb permit_required (negb permit_fresh) then RecoveryPermitStale
  else if negb slots_complete then CertifiedSlotsIncomplete
  else if negb proposer_active then CertifiedValidatorInactive
  else ProposalReady.

Definition finalizer_holds (relation : certified_context_relation) : bool :=
  match relation with
  | MatchingContext | StrictStatePreservingDescendant => false
  | RegressiveFloor | ConflictingFloor | MismatchedSameFloorContext => true
  end.

Definition pending_work_ready
  (fresh_ready retry_ready terminal : bool) : bool :=
  andb (negb terminal) (orb fresh_ready retry_ready).

Definition selection_has_work := pending_work_ready.

Theorem ready_requires_complete_certified_authority :
  forall permit_required permit_fresh relation slots_complete proposer_active,
    classify_proposal_readiness permit_required permit_fresh relation
      slots_complete proposer_active = ProposalReady ->
    slots_complete = true /\
    proposer_active = true /\
    (permit_required = false \/ permit_fresh = true).
Proof.
  intros [] [] [] [] []; cbn; intros; try discriminate; intuition.
Qed.

Theorem candidate_evidence_does_not_gate_proposal :
  forall permit_required permit_fresh relation_left relation_right slots_complete proposer_active,
    classify_proposal_readiness permit_required permit_fresh relation_left
      slots_complete proposer_active =
    classify_proposal_readiness permit_required permit_fresh relation_right
      slots_complete proposer_active.
Proof.
  intros.
  destruct permit_required, permit_fresh, slots_complete, proposer_active;
    reflexivity.
Qed.

Theorem certified_authority_enables_every_candidate_relation :
  forall permit_required permit_fresh relation,
    (permit_required = false \/ permit_fresh = true) ->
    classify_proposal_readiness permit_required permit_fresh relation true true =
      ProposalReady.
Proof.
  intros [] [] relation Hpermit; cbn; intuition discriminate.
Qed.

Theorem held_finalizer_cannot_disable_certified_proposal :
  forall relation,
    finalizer_holds relation = true ->
    classify_proposal_readiness false true relation true true = ProposalReady.
Proof.
  intros []; reflexivity.
Qed.

Definition proposal_floor_readiness_spec : Prop :=
  (forall permit_required permit_fresh relation slots_complete proposer_active,
     classify_proposal_readiness permit_required permit_fresh relation
       slots_complete proposer_active = ProposalReady ->
     slots_complete = true /\
     proposer_active = true /\
     (permit_required = false \/ permit_fresh = true))
  /\
  (forall permit_required permit_fresh relation_left relation_right slots_complete proposer_active,
     classify_proposal_readiness permit_required permit_fresh relation_left
       slots_complete proposer_active =
     classify_proposal_readiness permit_required permit_fresh relation_right
       slots_complete proposer_active)
  /\
  (forall permit_required permit_fresh relation,
     (permit_required = false \/ permit_fresh = true) ->
     classify_proposal_readiness permit_required permit_fresh relation true true =
       ProposalReady)
  /\
  (forall relation,
     finalizer_holds relation = true ->
     classify_proposal_readiness false true relation true true = ProposalReady).

Theorem proposal_floor_readiness_contract : proposal_floor_readiness_spec.
Proof.
  exact
    (conj ready_requires_complete_certified_authority
      (conj candidate_evidence_does_not_gate_proposal
        (conj certified_authority_enables_every_candidate_relation
          held_finalizer_cannot_disable_certified_proposal))).
Qed.

Print Assumptions proposal_floor_readiness_contract.

Theorem retry_only_work_is_ready :
  pending_work_ready false true false = true.
Proof. reflexivity. Qed.

Theorem terminal_work_is_never_ready :
  forall fresh_ready retry_ready,
    pending_work_ready fresh_ready retry_ready true = false.
Proof. intros [] []; reflexivity. Qed.

Theorem fresh_to_retry_transfer_preserves_readiness :
  pending_work_ready true false false =
  pending_work_ready false true false.
Proof. reflexivity. Qed.

Theorem heartbeat_and_selection_share_pending_work_classifier :
  forall fresh_ready retry_ready terminal,
    pending_work_ready fresh_ready retry_ready terminal =
    selection_has_work fresh_ready retry_ready terminal.
Proof. reflexivity. Qed.

Theorem pending_work_readiness_contract :
  pending_work_ready false true false = true /\
  (forall fresh_ready retry_ready,
    pending_work_ready fresh_ready retry_ready true = false) /\
  pending_work_ready true false false = pending_work_ready false true false /\
  (forall fresh_ready retry_ready terminal,
    pending_work_ready fresh_ready retry_ready terminal =
    selection_has_work fresh_ready retry_ready terminal).
Proof.
  exact
    (conj retry_only_work_is_ready
      (conj terminal_work_is_never_ready
        (conj fresh_to_retry_transfer_preserves_readiness
          heartbeat_and_selection_share_pending_work_classifier))).
Qed.

Print Assumptions pending_work_readiness_contract.

Section ValidatedSignedFloorReadiness.

Context {Floor State Digest Validator Generation Effect : Type}.

Variable floor_eq_dec : forall left right : Floor, {left = right} + {left <> right}.
Variable state_eq_dec : forall left right : State, {left = right} + {left <> right}.
Variable digest_eq_dec : forall left right : Digest, {left = right} + {left <> right}.
Variable validator_eq_dec :
  forall left right : Validator, {left = right} + {left <> right}.
Variable generation_eq_dec :
  forall left right : Generation, {left = right} + {left <> right}.
Variable committee_of_state : State -> list Validator.
Variable state_of_floor : Floor -> State.
Variable height_of_floor : Floor -> nat.
Variable digest_certificate :
  (@signed_floor_certificate Floor State Digest) -> Digest.

Inductive validated_proposal_outcome :=
| StartProposal
| DeferFloorArtifact (artifact : missing_floor_artifact)
| RejectFloorCapture (reason : invalid_floor_reason)
| DeferCertifiedAuthority (reason : recovery_deferral_reason).

Definition classify_validated_proposal
  (capture : @signed_floor_capture Floor State Digest Validator Generation Effect)
  (permit_required permit_fresh : bool)
  (relation : certified_context_relation)
  (slots_complete proposer_active : bool)
  : validated_proposal_outcome :=
  match classify_signed_floor floor_eq_dec state_eq_dec digest_eq_dec
          validator_eq_dec generation_eq_dec committee_of_state
          state_of_floor height_of_floor digest_certificate capture with
  | SignedFloorDeferred artifact => DeferFloorArtifact artifact
  | SignedFloorInvalid reason => RejectFloorCapture reason
  | SignedFloorAccepted =>
      match classify_proposal_readiness
              permit_required permit_fresh relation slots_complete proposer_active with
      | ProposalReady => StartProposal
      | reason => DeferCertifiedAuthority reason
      end
  end.

Theorem start_requires_validated_floor_and_authority :
  forall capture permit_required permit_fresh relation slots_complete proposer_active,
    classify_validated_proposal capture permit_required permit_fresh relation
      slots_complete proposer_active = StartProposal ->
    classify_signed_floor floor_eq_dec state_eq_dec digest_eq_dec
      validator_eq_dec generation_eq_dec committee_of_state
      state_of_floor height_of_floor digest_certificate capture = SignedFloorAccepted /\
    classify_proposal_readiness permit_required permit_fresh relation
      slots_complete proposer_active = ProposalReady.
Proof.
  intros capture permit_required permit_fresh relation slots_complete proposer_active
    Hstart.
  unfold classify_validated_proposal in Hstart.
  destruct (classify_signed_floor floor_eq_dec state_eq_dec digest_eq_dec
    validator_eq_dec generation_eq_dec committee_of_state
    state_of_floor height_of_floor digest_certificate capture) eqn:Hfloor;
    try discriminate Hstart.
  destruct (classify_proposal_readiness permit_required permit_fresh relation
    slots_complete proposer_active) eqn:Hauthority; try discriminate Hstart.
  split; reflexivity.
Qed.

Theorem missing_floor_state_blocks_proposal_without_rejection :
  forall capture permit_required permit_fresh relation slots_complete proposer_active,
    capture_certificate_available capture = true ->
    capture_block_available capture = true ->
    capture_metadata_available capture = true ->
    capture_state_available capture = false ->
    classify_validated_proposal capture permit_required permit_fresh relation
      slots_complete proposer_active = DeferFloorArtifact MissingFloorState.
Proof.
  intros capture permit_required permit_fresh relation slots_complete proposer_active
    Hcertificate Hblock Hmetadata Hstate.
  unfold classify_validated_proposal.
  rewrite (missing_state_defers floor_eq_dec state_eq_dec digest_eq_dec
    validator_eq_dec generation_eq_dec committee_of_state
    state_of_floor height_of_floor digest_certificate capture
    Hcertificate Hblock Hmetadata Hstate).
  reflexivity.
Qed.

Theorem candidate_relation_does_not_gate_validated_proposal :
  forall capture permit_required permit_fresh relation_left relation_right
    slots_complete proposer_active,
    classify_validated_proposal capture permit_required permit_fresh relation_left
      slots_complete proposer_active =
    classify_validated_proposal capture permit_required permit_fresh relation_right
      slots_complete proposer_active.
Proof.
  intros capture permit_required permit_fresh relation_left relation_right
    slots_complete proposer_active.
  unfold classify_validated_proposal.
  destruct (classify_signed_floor floor_eq_dec state_eq_dec digest_eq_dec
    validator_eq_dec generation_eq_dec committee_of_state
    state_of_floor height_of_floor digest_certificate capture); reflexivity.
Qed.

Theorem concurrent_finalizer_candidate_cannot_change_validated_proposal :
  forall capture candidate permit_required permit_fresh relation
    slots_complete proposer_active,
    classify_validated_proposal
      (with_candidate_floor capture candidate)
      permit_required permit_fresh relation slots_complete proposer_active =
    classify_validated_proposal capture permit_required permit_fresh relation
      slots_complete proposer_active.
Proof.
  intros capture candidate permit_required permit_fresh relation
    slots_complete proposer_active.
  unfold classify_validated_proposal.
  rewrite (candidate_evidence_cannot_replace_signed_floor floor_eq_dec state_eq_dec
    digest_eq_dec validator_eq_dec generation_eq_dec committee_of_state
    state_of_floor height_of_floor digest_certificate capture candidate).
  reflexivity.
Qed.

Definition validated_signed_floor_readiness_contract : Prop :=
  (forall capture permit_required permit_fresh relation slots_complete proposer_active,
    classify_validated_proposal capture permit_required permit_fresh relation
      slots_complete proposer_active = StartProposal ->
    classify_signed_floor floor_eq_dec state_eq_dec digest_eq_dec
      validator_eq_dec generation_eq_dec committee_of_state
      state_of_floor height_of_floor digest_certificate capture = SignedFloorAccepted /\
    classify_proposal_readiness permit_required permit_fresh relation
      slots_complete proposer_active = ProposalReady) /\
  (forall capture permit_required permit_fresh relation slots_complete proposer_active,
    capture_certificate_available capture = true ->
    capture_block_available capture = true ->
    capture_metadata_available capture = true ->
    capture_state_available capture = false ->
    classify_validated_proposal capture permit_required permit_fresh relation
      slots_complete proposer_active = DeferFloorArtifact MissingFloorState) /\
  (forall capture candidate permit_required permit_fresh relation
    slots_complete proposer_active,
    classify_validated_proposal
      (with_candidate_floor capture candidate)
      permit_required permit_fresh relation slots_complete proposer_active =
    classify_validated_proposal capture permit_required permit_fresh relation
      slots_complete proposer_active).

Theorem validated_signed_floor_readiness_correct :
  validated_signed_floor_readiness_contract.
Proof.
  unfold validated_signed_floor_readiness_contract.
  split.
  - apply start_requires_validated_floor_and_authority.
  - split.
    + apply missing_floor_state_blocks_proposal_without_rejection.
    + apply concurrent_finalizer_candidate_cannot_change_validated_proposal.
Qed.

End ValidatedSignedFloorReadiness.

Print Assumptions start_requires_validated_floor_and_authority.
Print Assumptions missing_floor_state_blocks_proposal_without_rejection.
Print Assumptions candidate_relation_does_not_gate_validated_proposal.
Print Assumptions concurrent_finalizer_candidate_cannot_change_validated_proposal.
Print Assumptions validated_signed_floor_readiness_correct.
