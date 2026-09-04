From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.

Inductive ValidationDisposition :=
| Pending
| Accepted
| ObjectiveInvalid.

Inductive RecoveryArtifact :=
| MissingBlockArtifact (identity : nat)
| MissingStateArtifact (identity : nat).

Inductive ValidationDeferral :=
| AlreadyBuffered
| AwaitingBlock (identity : nat)
| AwaitingState (identity : nat).

Inductive NodeHistory :=
| GenesisRooted
| TruncatedHistory.

Inductive CertifiedDeferral :=
| BufferedDependency
| MissingArtifact (artifact : RecoveryArtifact)
| LocalArtifactFault (artifact : RecoveryArtifact).

Definition deferral_artifact
  (deferral : ValidationDeferral) : option RecoveryArtifact :=
  match deferral with
  | AlreadyBuffered => None
  | AwaitingBlock identity => Some (MissingBlockArtifact identity)
  | AwaitingState identity => Some (MissingStateArtifact identity)
  end.

Definition certified_artifact
  (deferral : CertifiedDeferral) : option RecoveryArtifact :=
  match deferral with
  | BufferedDependency => None
  | MissingArtifact artifact => Some artifact
  | LocalArtifactFault artifact => Some artifact
  end.

Definition certify_deferral
  (history : NodeHistory)
  (deferral : ValidationDeferral) : CertifiedDeferral :=
  match deferral with
  | AlreadyBuffered => BufferedDependency
  | AwaitingBlock identity =>
      let artifact := MissingBlockArtifact identity in
      match history with
      | GenesisRooted => LocalArtifactFault artifact
      | TruncatedHistory => MissingArtifact artifact
      end
  | AwaitingState identity =>
      let artifact := MissingStateArtifact identity in
      match history with
      | GenesisRooted => LocalArtifactFault artifact
      | TruncatedHistory => MissingArtifact artifact
      end
  end.

Definition certified_deferral_disposition
  (_ : CertifiedDeferral) : ValidationDisposition := Pending.

Definition artifact_eqb
  (left right : RecoveryArtifact) : bool :=
  match left, right with
  | MissingBlockArtifact left_id, MissingBlockArtifact right_id =>
      Nat.eqb left_id right_id
  | MissingStateArtifact left_id, MissingStateArtifact right_id =>
      Nat.eqb left_id right_id
  | _, _ => false
  end.

Definition recovery_releases
  (recovered : RecoveryArtifact)
  (waiting : ValidationDeferral) : bool :=
  match deferral_artifact waiting with
  | Some required => artifact_eqb recovered required
  | None => false
  end.

Definition RecoveryRequests := RecoveryArtifact -> bool.

Definition request_artifact
  (artifact : RecoveryArtifact)
  (outstanding : RecoveryRequests) : RecoveryRequests :=
  fun candidate => artifact_eqb candidate artifact || outstanding candidate.

Theorem certified_deferral_preserves_artifact_identity :
  forall history deferral artifact,
    deferral_artifact deferral = Some artifact ->
    certified_artifact (certify_deferral history deferral) = Some artifact.
Proof.
  intros history deferral artifact named.
  destruct history, deferral; simpl in *; inversion named; reflexivity.
Qed.

Theorem block_and_state_deferrals_never_collapse :
  forall history block_id state_id,
    certify_deferral history (AwaitingBlock block_id) <>
    certify_deferral history (AwaitingState state_id).
Proof.
  intros history block_id state_id.
  destruct history; simpl; discriminate.
Qed.

Theorem genesis_guard_retains_typed_block_fault :
  forall identity,
    certify_deferral GenesisRooted (AwaitingBlock identity) =
    LocalArtifactFault (MissingBlockArtifact identity).
Proof.
  reflexivity.
Qed.

Theorem genesis_guard_retains_typed_state_fault :
  forall identity,
    certify_deferral GenesisRooted (AwaitingState identity) =
    LocalArtifactFault (MissingStateArtifact identity).
Proof.
  reflexivity.
Qed.

Theorem truncated_history_retains_typed_missing_dependency :
  forall deferral artifact,
    deferral_artifact deferral = Some artifact ->
    certify_deferral TruncatedHistory deferral = MissingArtifact artifact.
Proof.
  intros deferral artifact named.
  destruct deferral; simpl in *; inversion named; reflexivity.
Qed.

Theorem typed_deferral_never_creates_objective_invalidity :
  forall history deferral,
    certified_deferral_disposition (certify_deferral history deferral) = Pending.
Proof.
  reflexivity.
Qed.

Theorem exact_block_recovery_releases_block_waiter :
  forall identity,
    recovery_releases
      (MissingBlockArtifact identity)
      (AwaitingBlock identity) = true.
Proof.
  intros identity.
  unfold recovery_releases, deferral_artifact, artifact_eqb.
  apply Nat.eqb_refl.
Qed.

Theorem exact_state_recovery_releases_state_waiter :
  forall identity,
    recovery_releases
      (MissingStateArtifact identity)
      (AwaitingState identity) = true.
Proof.
  intros identity.
  unfold recovery_releases, deferral_artifact, artifact_eqb.
  apply Nat.eqb_refl.
Qed.

Theorem state_recovery_never_releases_block_waiter :
  forall state_id block_id,
    recovery_releases
      (MissingStateArtifact state_id)
      (AwaitingBlock block_id) = false.
Proof.
  reflexivity.
Qed.

Theorem block_recovery_never_releases_state_waiter :
  forall block_id state_id,
    recovery_releases
      (MissingBlockArtifact block_id)
      (AwaitingState state_id) = false.
Proof.
  reflexivity.
Qed.

Theorem duplicate_recovery_request_is_idempotent :
  forall artifact outstanding candidate,
    request_artifact artifact (request_artifact artifact outstanding) candidate =
    request_artifact artifact outstanding candidate.
Proof.
  intros artifact outstanding candidate.
  unfold request_artifact.
  destruct (artifact_eqb candidate artifact); reflexivity.
Qed.

Theorem independent_recovery_requests_commute :
  forall left right outstanding candidate,
    request_artifact left (request_artifact right outstanding) candidate =
    request_artifact right (request_artifact left outstanding) candidate.
Proof.
  intros left right outstanding candidate.
  unfold request_artifact.
  destruct (artifact_eqb candidate left),
           (artifact_eqb candidate right),
           (outstanding candidate); reflexivity.
Qed.

Inductive QueueState :=
| Blocked
| Ready
| InFlight
| Deferred
| Terminal.

Record RecoveryState := {
  queue_state : QueueState;
  validation_disposition : ValidationDisposition;
  recovery_outstanding : bool
}.

Definition defer_local_fault (state : RecoveryState) : RecoveryState :=
  {| queue_state := Deferred;
     validation_disposition := validation_disposition state;
     recovery_outstanding := true |}.

Definition recovery_request_failed (state : RecoveryState) : RecoveryState := state.

Definition recovery_request_succeeded (state : RecoveryState) : RecoveryState :=
  {| queue_state := Ready;
     validation_disposition := validation_disposition state;
     recovery_outstanding := false |}.

Definition outstanding_count (state : RecoveryState) : nat :=
  if recovery_outstanding state then 1 else 0.

Definition regular_parent_satisfied (state : RecoveryState) : bool :=
  match validation_disposition state with
  | Accepted => true
  | Pending => false
  | ObjectiveInvalid => false
  end.

Theorem local_fault_preserves_consensus_disposition :
  forall state,
    validation_disposition (defer_local_fault state) =
    validation_disposition state.
Proof.
  reflexivity.
Qed.

Theorem local_fault_leaves_ready_queue :
  forall state,
    queue_state (defer_local_fault state) <> Ready.
Proof.
  intros state impossible.
  discriminate.
Qed.

Theorem local_fault_opens_exactly_one_recovery :
  forall state,
    outstanding_count (defer_local_fault state) = 1.
Proof.
  reflexivity.
Qed.

Theorem failed_recovery_does_not_restore_ready_state :
  forall state,
    queue_state state = Deferred ->
    queue_state (recovery_request_failed state) <> Ready.
Proof.
  intros state deferred.
  unfold recovery_request_failed.
  rewrite deferred.
  discriminate.
Qed.

Theorem successful_recovery_reopens_without_invalidating :
  forall state,
    queue_state (recovery_request_succeeded state) = Ready /\
    validation_disposition (recovery_request_succeeded state) =
      validation_disposition state /\
    recovery_outstanding (recovery_request_succeeded state) = false.
Proof.
  intros state.
  repeat split.
Qed.

Theorem regular_child_requires_valid_parent :
  forall state,
    regular_parent_satisfied state = true ->
    validation_disposition state = Accepted.
Proof.
  intros [queue disposition outstanding].
  destruct disposition; simpl; intros satisfied.
  - discriminate.
  - reflexivity.
  - discriminate.
Qed.

Theorem objective_invalid_parent_does_not_release_regular_child :
  forall queue outstanding,
    regular_parent_satisfied
      {| queue_state := queue;
         validation_disposition := ObjectiveInvalid;
         recovery_outstanding := outstanding |} = false.
Proof.
  reflexivity.
Qed.
