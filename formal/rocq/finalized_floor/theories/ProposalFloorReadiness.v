From Stdlib Require Import Bool.Bool.

Inductive certified_context_relation : Type :=
| MatchingContext
| StrictStatePreservingDescendant
| RegressiveFloor
| ConflictingFloor
| MismatchedSameFloorContext.

Inductive recovery_deferral_reason : Type :=
| ProposalReady
| FloorMaterializationPending
| CandidateFloorRegression
| CandidateFloorConflict
| CertifiedContextMismatch
| CandidateSlotsIncomplete
| CandidateValidatorInactive
| RecoveryPermitStale.

Definition classify_proposal_readiness
    (permit_required permit_fresh : bool)
    (relation : certified_context_relation)
    (slots_complete proposer_active : bool)
    : recovery_deferral_reason :=
  if andb permit_required (negb permit_fresh) then RecoveryPermitStale
  else
    match relation with
    | MatchingContext =>
        if negb slots_complete then CandidateSlotsIncomplete
        else if negb proposer_active then CandidateValidatorInactive
        else ProposalReady
    | StrictStatePreservingDescendant => FloorMaterializationPending
    | RegressiveFloor => CandidateFloorRegression
    | ConflictingFloor => CandidateFloorConflict
    | MismatchedSameFloorContext => CertifiedContextMismatch
    end.

Definition requests_finalization (reason : recovery_deferral_reason) : bool :=
  match reason with
  | FloorMaterializationPending => true
  | _ => false
  end.

Definition materializable (relation : certified_context_relation) : bool :=
  match relation with
  | StrictStatePreservingDescendant => true
  | _ => false
  end.

Definition preserves_committed_state (relation : certified_context_relation) : bool :=
  match relation with
  | MatchingContext | StrictStatePreservingDescendant => true
  | _ => false
  end.

Theorem ready_requires_matching_complete_authority :
  forall permit_required permit_fresh relation slots_complete proposer_active,
    classify_proposal_readiness permit_required permit_fresh relation
      slots_complete proposer_active = ProposalReady ->
    relation = MatchingContext /\
    slots_complete = true /\
    proposer_active = true /\
    (permit_required = false \/ permit_fresh = true).
Proof.
  intros [] [] [] [] []; cbn; intros; try discriminate; intuition.
Qed.

Theorem floor_pending_identifies_strict_state_preserving_descendant :
  forall permit_required permit_fresh relation slots_complete proposer_active,
    classify_proposal_readiness permit_required permit_fresh relation
      slots_complete proposer_active = FloorMaterializationPending ->
    relation = StrictStatePreservingDescendant /\
    (permit_required = false \/ permit_fresh = true).
Proof.
  intros [] [] [] [] []; cbn; intros; try discriminate; intuition.
Qed.

Theorem finalization_request_identifies_floor_pending :
  forall reason,
    requests_finalization reason = true ->
    reason = FloorMaterializationPending.
Proof.
  intros []; cbn; intros; try discriminate; reflexivity.
Qed.

Theorem classified_request_is_materializable_and_state_preserving :
  forall permit_required permit_fresh relation slots_complete proposer_active,
    requests_finalization
      (classify_proposal_readiness permit_required permit_fresh relation
        slots_complete proposer_active) = true ->
    materializable relation = true /\ preserves_committed_state relation = true.
Proof.
  intros [] [] [] [] []; cbn; intros; try discriminate; intuition.
Qed.

Theorem permanent_context_failures_never_request_finalization :
  forall reason,
    reason = CandidateFloorRegression \/
    reason = CandidateFloorConflict \/
    reason = CertifiedContextMismatch ->
    requests_finalization reason = false.
Proof.
  intros reason [-> | [-> | ->]]; reflexivity.
Qed.

Theorem proposal_floor_readiness_contract :
  (forall permit_required permit_fresh relation slots_complete proposer_active,
     classify_proposal_readiness permit_required permit_fresh relation
       slots_complete proposer_active = ProposalReady ->
     relation = MatchingContext /\
     slots_complete = true /\
     proposer_active = true /\
     (permit_required = false \/ permit_fresh = true))
  /\
  (forall permit_required permit_fresh relation slots_complete proposer_active,
     classify_proposal_readiness permit_required permit_fresh relation
       slots_complete proposer_active = FloorMaterializationPending ->
     relation = StrictStatePreservingDescendant /\
     (permit_required = false \/ permit_fresh = true))
  /\
  (forall permit_required permit_fresh relation slots_complete proposer_active,
     requests_finalization
       (classify_proposal_readiness permit_required permit_fresh relation
         slots_complete proposer_active) = true ->
     materializable relation = true /\ preserves_committed_state relation = true)
  /\
  (forall reason,
     reason = CandidateFloorRegression \/
     reason = CandidateFloorConflict \/
     reason = CertifiedContextMismatch ->
     requests_finalization reason = false).
Proof.
  exact
    (conj ready_requires_matching_complete_authority
      (conj floor_pending_identifies_strict_state_preserving_descendant
        (conj classified_request_is_materializable_and_state_preserving
          permanent_context_failures_never_request_finalization))).
Qed.

Print Assumptions proposal_floor_readiness_contract.
