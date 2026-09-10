From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.

Inductive settled_ticket_phase :=
| TicketAvailable
| TicketClaimed
| TicketCommitted.

Record settled_history_evidence_checks := {
  target_hash_valid : bool;
  target_signature_valid : bool;
  anchor_hash_valid : bool;
  citer_hash_valid : bool;
  citer_signature_valid : bool;
  citation_valid : bool;
  citer_bond_valid : bool;
  citer_generation_valid : bool
}.

Definition settled_history_evidence_authentic
  (checks : settled_history_evidence_checks) : bool :=
  target_hash_valid checks &&
  target_signature_valid checks &&
  anchor_hash_valid checks &&
  citer_hash_valid checks &&
  citer_signature_valid checks &&
  citation_valid checks &&
  citer_bond_valid checks &&
  citer_generation_valid checks.

Inductive validated_settled_history_evidence
  (checks : settled_history_evidence_checks) : Prop :=
| ValidateSettledHistoryEvidence :
    settled_history_evidence_authentic checks = true ->
    validated_settled_history_evidence checks.

Record settled_ticket_state := {
  ticket_phase : settled_ticket_phase;
  ticket_owner : option nat;
  ticket_evidence : bool;
  ticket_buffer_edge : bool;
  ticket_reserved : bool;
  ticket_durable : bool;
  ticket_cleanup_pending : bool;
  ticket_ordinary_validation : bool;
  ticket_budget : nat
}.

Definition initial_ticket_state : settled_ticket_state :=
  {| ticket_phase := TicketAvailable;
     ticket_owner := None;
     ticket_evidence := true;
     ticket_buffer_edge := true;
     ticket_reserved := false;
     ticket_durable := false;
     ticket_cleanup_pending := false;
     ticket_ordinary_validation := false;
     ticket_budget := 0 |}.

Definition claim_ticket
  (worker : nat) (state : settled_ticket_state) : settled_ticket_state :=
  match ticket_phase state with
  | TicketAvailable =>
      if ticket_evidence state then
        {| ticket_phase := TicketClaimed;
           ticket_owner := Some worker;
           ticket_evidence := ticket_evidence state;
           ticket_buffer_edge := ticket_buffer_edge state;
           ticket_reserved := ticket_reserved state;
           ticket_durable := ticket_durable state;
           ticket_cleanup_pending := ticket_cleanup_pending state;
           ticket_ordinary_validation := ticket_ordinary_validation state;
           ticket_budget := ticket_budget state |}
      else state
  | _ => state
  end.

Definition reserve_ticket
  (maximum : nat) (state : settled_ticket_state) : settled_ticket_state :=
  match ticket_phase state, ticket_owner state, ticket_reserved state with
  | TicketClaimed, Some _, false =>
      if Nat.ltb (ticket_budget state) maximum then
        {| ticket_phase := TicketClaimed;
           ticket_owner := ticket_owner state;
           ticket_evidence := ticket_evidence state;
           ticket_buffer_edge := ticket_buffer_edge state;
           ticket_reserved := true;
           ticket_durable := ticket_durable state;
           ticket_cleanup_pending := ticket_cleanup_pending state;
           ticket_ordinary_validation := ticket_ordinary_validation state;
           ticket_budget := S (ticket_budget state) |}
      else state
  | _, _, _ => state
  end.

Definition rollback_ticket (state : settled_ticket_state) : settled_ticket_state :=
  match ticket_phase state, ticket_owner state with
  | TicketClaimed, Some _ =>
      {| ticket_phase := TicketAvailable;
         ticket_owner := None;
         ticket_evidence := ticket_evidence state;
         ticket_buffer_edge := ticket_buffer_edge state;
         ticket_reserved := false;
         ticket_durable := ticket_durable state;
         ticket_cleanup_pending := ticket_cleanup_pending state;
         ticket_ordinary_validation := ticket_ordinary_validation state;
         ticket_budget :=
           if ticket_reserved state then Nat.pred (ticket_budget state)
           else ticket_budget state |}
  | _, _ => state
  end.

Definition commit_ticket (state : settled_ticket_state) : settled_ticket_state :=
  match ticket_phase state, ticket_owner state, ticket_reserved state with
  | TicketClaimed, Some _, true =>
      {| ticket_phase := TicketCommitted;
         ticket_owner := None;
         ticket_evidence := ticket_evidence state;
         ticket_buffer_edge := ticket_buffer_edge state;
         ticket_reserved := false;
         ticket_durable := true;
         ticket_cleanup_pending := true;
         ticket_ordinary_validation := ticket_ordinary_validation state;
         ticket_budget := ticket_budget state |}
  | _, _, _ => state
  end.

Definition deliver_duplicate (state : settled_ticket_state) : settled_ticket_state := state.

Definition commit_validated_ticket
  (checks : settled_history_evidence_checks)
  (_ : validated_settled_history_evidence checks)
  (state : settled_ticket_state) : settled_ticket_state :=
  commit_ticket state.

Definition finish_ticket_cleanup (state : settled_ticket_state) : settled_ticket_state :=
  match ticket_phase state with
  | TicketCommitted =>
      {| ticket_phase := TicketCommitted;
         ticket_owner := ticket_owner state;
         ticket_evidence := false;
         ticket_buffer_edge := false;
         ticket_reserved := ticket_reserved state;
         ticket_durable := ticket_durable state;
         ticket_cleanup_pending := false;
         ticket_ordinary_validation := ticket_ordinary_validation state;
         ticket_budget := ticket_budget state |}
  | _ => state
  end.

Definition restart_ticket (state : settled_ticket_state) : settled_ticket_state :=
  if ticket_durable state then
    {| ticket_phase := TicketCommitted;
       ticket_owner := None;
       ticket_evidence := ticket_buffer_edge state;
       ticket_buffer_edge := ticket_buffer_edge state;
       ticket_reserved := false;
       ticket_durable := true;
       ticket_cleanup_pending := ticket_buffer_edge state;
       ticket_ordinary_validation := ticket_ordinary_validation state;
       ticket_budget := 1 |}
  else
    {| ticket_phase := TicketAvailable;
       ticket_owner := None;
       ticket_evidence := ticket_buffer_edge state;
       ticket_buffer_edge := ticket_buffer_edge state;
       ticket_reserved := false;
       ticket_durable := false;
       ticket_cleanup_pending := false;
       ticket_ordinary_validation := ticket_ordinary_validation state;
       ticket_budget := 0 |}.

Definition exclusive_owner (state : settled_ticket_state) : Prop :=
  match ticket_owner state with
  | None => True
  | Some _ => ticket_phase state = TicketClaimed
  end.

Theorem claim_ticket_has_one_owner :
  forall worker state,
    exclusive_owner state -> exclusive_owner (claim_ticket worker state).
Proof.
  intros worker state safe.
  unfold claim_ticket.
  destruct (ticket_phase state) eqn:phase.
  - destruct (ticket_evidence state); unfold exclusive_owner; simpl; auto.
  - exact safe.
  - exact safe.
Qed.

Theorem rollback_restores_available_evidence :
  forall state worker,
    ticket_phase state = TicketClaimed ->
    ticket_owner state = Some worker ->
    ticket_evidence state = true ->
    ticket_buffer_edge state = true ->
    ticket_phase (rollback_ticket state) = TicketAvailable /\
    ticket_owner (rollback_ticket state) = None /\
    ticket_evidence (rollback_ticket state) = true /\
    ticket_buffer_edge (rollback_ticket state) = true.
Proof.
  intros state worker phase owner evidence edge.
  unfold rollback_ticket.
  rewrite phase, owner.
  simpl.
  rewrite evidence, edge.
  repeat split.
Qed.

Theorem rollback_releases_reserved_budget :
  forall state worker,
    ticket_phase state = TicketClaimed ->
    ticket_owner state = Some worker ->
    ticket_reserved state = true ->
    ticket_budget state > 0 ->
    ticket_budget (rollback_ticket state) + 1 = ticket_budget state.
Proof.
  intros state worker phase owner reserved positive.
  unfold rollback_ticket.
  rewrite phase, owner.
  simpl.
  rewrite reserved.
  simpl.
  lia.
Qed.

Theorem commit_establishes_durable_cleanup_pending :
  forall state worker,
    ticket_phase state = TicketClaimed ->
    ticket_owner state = Some worker ->
    ticket_reserved state = true ->
    ticket_phase (commit_ticket state) = TicketCommitted /\
    ticket_durable (commit_ticket state) = true /\
    ticket_buffer_edge (commit_ticket state) = ticket_buffer_edge state /\
    ticket_cleanup_pending (commit_ticket state) = true /\
    ticket_owner (commit_ticket state) = None.
Proof.
  intros state worker phase owner reserved.
  unfold commit_ticket.
  rewrite phase, owner, reserved.
  repeat split.
Qed.

Theorem duplicate_delivery_is_stuttering :
  forall state, deliver_duplicate state = state.
Proof.
  reflexivity.
Qed.

Theorem unauthentic_evidence_has_no_validated_witness :
  forall checks,
    settled_history_evidence_authentic checks = false ->
    ~ validated_settled_history_evidence checks.
Proof.
  intros checks invalid witness.
  inversion witness.
  congruence.
Qed.

Theorem validated_commit_uses_durable_ticket_transition :
  forall checks (witness : validated_settled_history_evidence checks) state,
    commit_validated_ticket checks witness state = commit_ticket state.
Proof.
  reflexivity.
Qed.

Theorem duplicate_delivery_preserves_budget :
  forall state,
    ticket_budget (deliver_duplicate state) = ticket_budget state.
Proof.
  reflexivity.
Qed.

Theorem committed_retry_is_idempotent :
  forall state worker,
    ticket_phase state = TicketCommitted ->
    claim_ticket worker state = state /\
    reserve_ticket (S (ticket_budget state)) state = state /\
    commit_ticket state = state.
Proof.
  intros state worker phase.
  unfold claim_ticket, reserve_ticket, commit_ticket.
  rewrite phase.
  repeat split.
Qed.

Theorem postcommit_cleanup_preserves_admission :
  forall state,
    ticket_phase state = TicketCommitted ->
    ticket_phase (finish_ticket_cleanup state) = TicketCommitted /\
    ticket_durable (finish_ticket_cleanup state) = ticket_durable state /\
    ticket_buffer_edge (finish_ticket_cleanup state) = false /\
    ticket_evidence (finish_ticket_cleanup state) = false /\
    ticket_budget (finish_ticket_cleanup state) = ticket_budget state.
Proof.
  intros state phase.
  unfold finish_ticket_cleanup.
  rewrite phase.
  repeat split.
Qed.

Theorem restart_reconciles_committed_admission :
  forall state,
    ticket_durable state = true ->
    ticket_phase (restart_ticket state) = TicketCommitted /\
    ticket_durable (restart_ticket state) = true /\
    ticket_buffer_edge (restart_ticket state) = ticket_buffer_edge state /\
    ticket_cleanup_pending (restart_ticket state) = ticket_buffer_edge state /\
    ticket_budget (restart_ticket state) = 1 /\
    ticket_owner (restart_ticket state) = None.
Proof.
  intros state durable.
  unfold restart_ticket.
  rewrite durable.
  repeat split.
Qed.

Theorem restart_recovers_uncommitted_evidence :
  forall state,
    ticket_durable state = false ->
    ticket_buffer_edge state = true ->
    ticket_phase (restart_ticket state) = TicketAvailable /\
    ticket_evidence (restart_ticket state) = true /\
    ticket_budget (restart_ticket state) = 0 /\
    ticket_owner (restart_ticket state) = None.
Proof.
  intros state durable edge.
  unfold restart_ticket.
  rewrite durable.
  simpl.
  rewrite edge.
  repeat split.
Qed.

Theorem settled_ticket_transaction_contract :
  (forall worker state,
    exclusive_owner state -> exclusive_owner (claim_ticket worker state)) /\
  (forall state, deliver_duplicate state = state) /\
  (forall state worker,
    ticket_phase state = TicketClaimed ->
    ticket_owner state = Some worker ->
    ticket_reserved state = true ->
    ticket_phase (commit_ticket state) = TicketCommitted /\
    ticket_durable (commit_ticket state) = true /\
    ticket_buffer_edge (commit_ticket state) = ticket_buffer_edge state /\
    ticket_cleanup_pending (commit_ticket state) = true /\
    ticket_owner (commit_ticket state) = None) /\
  (forall state,
    ticket_durable state = true ->
    ticket_phase (restart_ticket state) = TicketCommitted).
Proof.
  split.
  - exact claim_ticket_has_one_owner.
  - split.
    + exact duplicate_delivery_is_stuttering.
    + split.
      * exact commit_establishes_durable_cleanup_pending.
      * intros state durable.
        apply restart_reconciles_committed_admission in durable.
        exact (proj1 durable).
Qed.

Print Assumptions claim_ticket_has_one_owner.
Print Assumptions rollback_restores_available_evidence.
Print Assumptions rollback_releases_reserved_budget.
Print Assumptions commit_establishes_durable_cleanup_pending.
Print Assumptions duplicate_delivery_is_stuttering.
Print Assumptions unauthentic_evidence_has_no_validated_witness.
Print Assumptions validated_commit_uses_durable_ticket_transition.
Print Assumptions duplicate_delivery_preserves_budget.
Print Assumptions committed_retry_is_idempotent.
Print Assumptions postcommit_cleanup_preserves_admission.
Print Assumptions restart_reconciles_committed_admission.
Print Assumptions restart_recovers_uncommitted_evidence.
Print Assumptions settled_ticket_transaction_contract.
