From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
Import ListNotations.

Inductive RequestPhase : Type :=
| RequestIdle
| RequestActive
| RequestQuarantined
| RequestReceived
| RequestAdmitted
| RequestObsolete.

Record RequestLifecycle : Type := {
  request_phase : RequestPhase;
  request_unresolved : bool;
  request_dependency_evidence : bool;
  request_attempts : nat;
  request_deadline : nat
}.

Definition clear_attempt_state (state : RequestLifecycle) : RequestLifecycle :=
  {| request_phase := request_phase state;
     request_unresolved := request_unresolved state;
     request_dependency_evidence := request_dependency_evidence state;
     request_attempts := 0;
     request_deadline := request_deadline state |}.

Definition exhaust_request
  (now quarantine_ticks : nat)
  (state : RequestLifecycle) : RequestLifecycle :=
  {| request_phase := RequestQuarantined;
     request_unresolved := request_unresolved state;
     request_dependency_evidence := request_dependency_evidence state;
     request_attempts := 0;
     request_deadline := now + quarantine_ticks |}.

Definition receive_request (state : RequestLifecycle) : RequestLifecycle :=
  {| request_phase := RequestReceived;
     request_unresolved := request_unresolved state;
     request_dependency_evidence := request_dependency_evidence state;
     request_attempts := 0;
     request_deadline := 0 |}.

Definition defer_request (state : RequestLifecycle) : RequestLifecycle :=
  {| request_phase := RequestActive;
     request_unresolved := request_unresolved state;
     request_dependency_evidence := request_dependency_evidence state;
     request_attempts := 0;
     request_deadline := 0 |}.

Definition resolve_request
  (terminal_phase : RequestPhase)
  (state : RequestLifecycle) : RequestLifecycle :=
  {| request_phase := terminal_phase;
     request_unresolved := false;
     request_dependency_evidence := false;
     request_attempts := 0;
     request_deadline := 0 |}.

Definition evidence_invariant (state : RequestLifecycle) : Prop :=
  request_unresolved state = true -> request_dependency_evidence state = true.

Theorem attempt_cleanup_preserves_dependency_evidence :
  forall state,
    request_dependency_evidence (clear_attempt_state state) =
    request_dependency_evidence state.
Proof. reflexivity. Qed.

Theorem attempt_cleanup_preserves_unresolved_obligation :
  forall state,
    request_unresolved (clear_attempt_state state) =
    request_unresolved state.
Proof. reflexivity. Qed.

Theorem attempt_cleanup_preserves_evidence_invariant :
  forall state,
    evidence_invariant state -> evidence_invariant (clear_attempt_state state).
Proof. intros state invariant unresolved. apply invariant. exact unresolved. Qed.

Theorem exhaustion_preserves_dependency_evidence :
  forall now quarantine_ticks state,
    request_dependency_evidence
      (exhaust_request now quarantine_ticks state) =
    request_dependency_evidence state.
Proof. reflexivity. Qed.

Theorem exhaustion_preserves_unresolved_obligation :
  forall now quarantine_ticks state,
    request_unresolved (exhaust_request now quarantine_ticks state) =
    request_unresolved state.
Proof. reflexivity. Qed.

Theorem exhaustion_suppresses_attempts :
  forall now quarantine_ticks state,
    request_attempts (exhaust_request now quarantine_ticks state) = 0.
Proof. reflexivity. Qed.

Theorem exhaustion_preserves_evidence_invariant :
  forall now quarantine_ticks state,
    evidence_invariant state ->
    evidence_invariant (exhaust_request now quarantine_ticks state).
Proof. intros now quarantine_ticks state invariant unresolved. apply invariant. exact unresolved. Qed.

Theorem receipt_preserves_dependency_evidence :
  forall state,
    request_dependency_evidence (receive_request state) =
    request_dependency_evidence state.
Proof. reflexivity. Qed.

Theorem receipt_preserves_unresolved_obligation :
  forall state,
    request_unresolved (receive_request state) = request_unresolved state.
Proof. reflexivity. Qed.

Theorem receipt_preserves_evidence_invariant :
  forall state,
    evidence_invariant state -> evidence_invariant (receive_request state).
Proof. intros state invariant unresolved. apply invariant. exact unresolved. Qed.

Theorem admission_deferral_preserves_dependency_evidence :
  forall state,
    request_dependency_evidence (defer_request state) =
    request_dependency_evidence state.
Proof. reflexivity. Qed.

Theorem admission_deferral_preserves_evidence_invariant :
  forall state,
    evidence_invariant state -> evidence_invariant (defer_request state).
Proof. intros state invariant unresolved. apply invariant. exact unresolved. Qed.

Theorem terminal_resolution_clears_dependency_evidence :
  forall terminal state,
    request_dependency_evidence (resolve_request terminal state) = false.
Proof. reflexivity. Qed.

Theorem terminal_resolution_closes_unresolved_obligation :
  forall terminal state,
    request_unresolved (resolve_request terminal state) = false.
Proof. reflexivity. Qed.

Theorem terminal_resolution_establishes_evidence_invariant :
  forall terminal state,
    evidence_invariant (resolve_request terminal state).
Proof. intros terminal state unresolved. discriminate unresolved. Qed.

Record DependencyQueue : Type := {
  queue_parent_missing : bool;
  queue_waiters : nat;
  queue_ready : nat
}.

Definition prune_waiting_child (state : DependencyQueue) : DependencyQueue :=
  if Nat.ltb 1 (queue_waiters state) then
    {| queue_parent_missing := queue_parent_missing state;
       queue_waiters := Nat.pred (queue_waiters state);
       queue_ready := queue_ready state |}
  else state.

Definition no_false_waiter_wakeup (state : DependencyQueue) : Prop :=
  queue_parent_missing state = true -> queue_ready state = 0.

Theorem waiting_child_pruning_preserves_missing_parent :
  forall state,
    queue_parent_missing (prune_waiting_child state) =
    queue_parent_missing state.
Proof.
  intros state. unfold prune_waiting_child.
  destruct (Nat.ltb 1 (queue_waiters state)); reflexivity.
Qed.

Theorem waiting_child_pruning_does_not_wake_sibling :
  forall state,
    queue_ready (prune_waiting_child state) = queue_ready state.
Proof.
  intros state. unfold prune_waiting_child.
  destruct (Nat.ltb 1 (queue_waiters state)); reflexivity.
Qed.

Theorem waiting_child_pruning_preserves_no_false_wakeup :
  forall state,
    no_false_waiter_wakeup state ->
    no_false_waiter_wakeup (prune_waiting_child state).
Proof.
  intros state safe missing.
  rewrite waiting_child_pruning_does_not_wake_sibling.
  apply safe.
  rewrite <- waiting_child_pruning_preserves_missing_parent.
  exact missing.
Qed.

Theorem request_quarantine_lifecycle_correct :
  (forall state,
    evidence_invariant state -> evidence_invariant (clear_attempt_state state)) /\
  (forall now quarantine_ticks state,
    evidence_invariant state ->
    evidence_invariant (exhaust_request now quarantine_ticks state)) /\
  (forall state,
    evidence_invariant state -> evidence_invariant (receive_request state)) /\
  (forall state,
    evidence_invariant state -> evidence_invariant (defer_request state)) /\
  (forall terminal state,
    evidence_invariant (resolve_request terminal state)) /\
  (forall state,
    no_false_waiter_wakeup state ->
    no_false_waiter_wakeup (prune_waiting_child state)).
Proof.
  repeat split.
  - exact attempt_cleanup_preserves_evidence_invariant.
  - exact exhaustion_preserves_evidence_invariant.
  - exact receipt_preserves_evidence_invariant.
  - exact admission_deferral_preserves_evidence_invariant.
  - exact terminal_resolution_establishes_evidence_invariant.
  - exact waiting_child_pruning_preserves_no_false_wakeup.
Qed.

Section EvidenceCapacity.

Context {Hash : Type}.

Definition admit_evidence
  (capacity : nat)
  (hash : Hash)
  (tracked : list Hash) : list Hash :=
  if Nat.ltb (length tracked) capacity then hash :: tracked else tracked.

Theorem evidence_admission_is_bounded :
  forall capacity hash tracked,
    length tracked <= capacity ->
    length (admit_evidence capacity hash tracked) <= capacity.
Proof.
  intros capacity hash tracked bounded.
  unfold admit_evidence.
  destruct (Nat.ltb_spec0 (length tracked) capacity).
  - simpl. lia.
  - exact bounded.
Qed.

Theorem full_capacity_preserves_existing_evidence :
  forall capacity hash tracked,
    capacity <= length tracked ->
    admit_evidence capacity hash tracked = tracked.
Proof.
  intros capacity hash tracked full.
  unfold admit_evidence.
  destruct (Nat.ltb_spec0 (length tracked) capacity).
  - lia.
  - reflexivity.
Qed.

End EvidenceCapacity.
