From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.

Inductive restore_phase :=
| RestoreIdle
| RestoreActive
| RestoreRunning
| RestoreTerminal.

Record restore_state := {
  restore_state_phase : restore_phase;
  restore_failures : nat;
  restore_generation : nat;
  restore_request_pending : bool;
  restore_retry_request_active : bool;
  restore_retry_request_generation : nat;
  restore_channels_ready : bool;
  restore_active_count : nat;
  restore_running_published : bool;
  restore_terminal_published : bool;
  restore_estimator_available : bool;
  restore_approved_persisted : bool
}.

Definition initial_restore_state : restore_state :=
  {| restore_state_phase := RestoreIdle;
     restore_failures := 0;
     restore_generation := 0;
     restore_request_pending := true;
     restore_retry_request_active := false;
     restore_retry_request_generation := 0;
     restore_channels_ready := true;
     restore_active_count := 0;
     restore_running_published := false;
     restore_terminal_published := false;
     restore_estimator_available := true;
     restore_approved_persisted := false |}.

Definition with_request_status
  (pending active : bool) (state : restore_state) : restore_state :=
  {| restore_state_phase := restore_state_phase state;
     restore_failures := restore_failures state;
     restore_generation := restore_generation state;
     restore_request_pending := pending;
     restore_retry_request_active := active;
     restore_retry_request_generation := restore_retry_request_generation state;
     restore_channels_ready := restore_channels_ready state;
     restore_active_count := restore_active_count state;
     restore_running_published := restore_running_published state;
     restore_terminal_published := restore_terminal_published state;
     restore_estimator_available := restore_estimator_available state;
     restore_approved_persisted := restore_approved_persisted state |}.

Definition terminalize_restore (state : restore_state) : restore_state :=
  {| restore_state_phase := RestoreTerminal;
     restore_failures := restore_failures state;
     restore_generation := restore_generation state;
     restore_request_pending := false;
     restore_retry_request_active := false;
     restore_retry_request_generation := restore_retry_request_generation state;
     restore_channels_ready := false;
     restore_active_count := 0;
     restore_running_published := false;
     restore_terminal_published := true;
     restore_estimator_available := restore_estimator_available state;
     restore_approved_persisted := restore_approved_persisted state |}.

Definition begin_restore (state : restore_state) : restore_state :=
  match restore_state_phase state with
  | RestoreIdle =>
      {| restore_state_phase := RestoreActive;
         restore_failures := restore_failures state;
         restore_generation := S (restore_generation state);
         restore_request_pending := false;
         restore_retry_request_active := restore_retry_request_active state;
         restore_retry_request_generation := restore_retry_request_generation state;
         restore_channels_ready := false;
         restore_active_count := 1;
         restore_running_published := false;
         restore_terminal_published := false;
         restore_estimator_available := restore_estimator_available state;
         restore_approved_persisted := restore_approved_persisted state |}
  | _ => state
  end.

Definition fail_restore
  (max_failures lease : nat) (state : restore_state) : restore_state :=
  match restore_state_phase state with
  | RestoreActive =>
      if Nat.eqb lease (restore_generation state)
      then
        if Nat.ltb (S (restore_failures state)) max_failures
        then
          {| restore_state_phase := RestoreIdle;
             restore_failures := S (restore_failures state);
             restore_generation := restore_generation state;
             restore_request_pending := false;
             restore_retry_request_active := true;
             restore_retry_request_generation := lease;
             restore_channels_ready := true;
             restore_active_count := 0;
             restore_running_published := false;
             restore_terminal_published := false;
             restore_estimator_available := restore_estimator_available state;
             restore_approved_persisted := restore_approved_persisted state |}
        else
          {| restore_state_phase := RestoreTerminal;
             restore_failures := S (restore_failures state);
             restore_generation := restore_generation state;
             restore_request_pending := false;
             restore_retry_request_active := false;
             restore_retry_request_generation := lease;
             restore_channels_ready := false;
             restore_active_count := 0;
             restore_running_published := false;
             restore_terminal_published := true;
             restore_estimator_available := restore_estimator_available state;
             restore_approved_persisted := restore_approved_persisted state |}
      else state
  | _ => state
  end.

Definition terminate_active_restore
  (lease : nat) (state : restore_state) : restore_state * bool :=
  match restore_state_phase state with
  | RestoreActive =>
      if Nat.eqb lease (restore_generation state)
      then (terminalize_restore state, true)
      else (state, false)
  | _ => (state, false)
  end.

Definition retry_request_succeeds
  (request_generation : nat) (state : restore_state) : restore_state :=
  if andb
       (restore_retry_request_active state)
       (Nat.eqb request_generation (restore_retry_request_generation state))
  then
    match restore_state_phase state with
    | RestoreIdle =>
        if Nat.eqb request_generation (restore_generation state)
        then with_request_status true false state
        else with_request_status false false state
    | _ => with_request_status false false state
    end
  else state.

Definition retry_request_fails
  (request_generation : nat) (state : restore_state) : restore_state * bool :=
  if andb
       (restore_retry_request_active state)
       (Nat.eqb request_generation (restore_retry_request_generation state))
  then
    match restore_state_phase state with
    | RestoreIdle =>
        if Nat.eqb request_generation (restore_generation state)
        then (terminalize_restore state, true)
        else (with_request_status false false state, false)
    | _ => (with_request_status false false state, false)
    end
  else (state, false).

Definition succeed_restore
  (lease : nat) (state : restore_state) : restore_state :=
  match restore_state_phase state with
  | RestoreActive =>
      if Nat.eqb lease (restore_generation state)
      then
        {| restore_state_phase := RestoreRunning;
           restore_failures := restore_failures state;
           restore_generation := restore_generation state;
           restore_request_pending := false;
           restore_retry_request_active := restore_retry_request_active state;
           restore_retry_request_generation := restore_retry_request_generation state;
           restore_channels_ready := false;
           restore_active_count := 0;
           restore_running_published := true;
           restore_terminal_published := false;
           restore_estimator_available := false;
           restore_approved_persisted := true |}
      else state
  | _ => state
  end.

Definition post_commit_notice_failure (state : restore_state) : restore_state := state.

Definition restart_restore : restore_state := initial_restore_state.

Theorem restore_claim_is_exclusive :
  forall state,
    restore_state_phase state = RestoreIdle ->
    restore_state_phase (begin_restore state) = RestoreActive /\
    restore_generation (begin_restore state) = S (restore_generation state) /\
    restore_active_count (begin_restore state) = 1 /\
    restore_request_pending (begin_restore state) = false /\
    restore_channels_ready (begin_restore state) = false.
Proof.
  intros state Hphase.
  unfold begin_restore.
  rewrite Hphase.
  repeat split; reflexivity.
Qed.

Theorem duplicate_restore_is_idempotent :
  forall state,
    restore_state_phase state <> RestoreIdle ->
    begin_restore state = state.
Proof.
  intros state Hphase.
  unfold begin_restore.
  destruct (restore_state_phase state); try reflexivity.
  contradiction.
Qed.

Theorem non_owner_failure_is_idempotent :
  forall max_failures lease state,
    restore_state_phase state <> RestoreActive \/
    lease <> restore_generation state ->
    fail_restore max_failures lease state = state.
Proof.
  intros max_failures lease state Hnot_owner.
  unfold fail_restore.
  destruct (restore_state_phase state); try reflexivity.
  destruct Hnot_owner as [Hphase | Hlease].
  - contradiction.
  - apply Nat.eqb_neq in Hlease.
    rewrite Hlease.
    reflexivity.
Qed.

Theorem recoverable_failure_restores_retry_ownership :
  forall max_failures lease state,
    restore_state_phase state = RestoreActive ->
    lease = restore_generation state ->
    S (restore_failures state) < max_failures ->
    restore_state_phase (fail_restore max_failures lease state) = RestoreIdle /\
    restore_retry_request_active (fail_restore max_failures lease state) = true /\
    restore_retry_request_generation (fail_restore max_failures lease state) = lease /\
    restore_channels_ready (fail_restore max_failures lease state) = true /\
    restore_active_count (fail_restore max_failures lease state) = 0.
Proof.
  intros max_failures lease state Hphase Hlease Hbelow.
  unfold fail_restore.
  rewrite Hphase.
  apply Nat.eqb_eq in Hlease.
  rewrite Hlease.
  apply Nat.ltb_lt in Hbelow.
  rewrite Hbelow.
  repeat split; reflexivity.
Qed.

Theorem exhausted_failure_is_explicit :
  forall max_failures lease state,
    restore_state_phase state = RestoreActive ->
    lease = restore_generation state ->
    max_failures <= S (restore_failures state) ->
    restore_state_phase (fail_restore max_failures lease state) = RestoreTerminal /\
    restore_terminal_published (fail_restore max_failures lease state) = true /\
    restore_request_pending (fail_restore max_failures lease state) = false /\
    restore_retry_request_active (fail_restore max_failures lease state) = false /\
    restore_active_count (fail_restore max_failures lease state) = 0.
Proof.
  intros max_failures lease state Hphase Hlease Hexhausted.
  unfold fail_restore.
  rewrite Hphase.
  apply Nat.eqb_eq in Hlease.
  rewrite Hlease.
  apply Nat.ltb_ge in Hexhausted.
  rewrite Hexhausted.
  repeat split; reflexivity.
Qed.

Theorem recoverable_failure_preserves_estimator :
  forall max_failures lease state,
    restore_state_phase state = RestoreActive ->
    lease = restore_generation state ->
    restore_estimator_available (fail_restore max_failures lease state) =
    restore_estimator_available state.
Proof.
  intros max_failures lease state Hphase Hlease.
  unfold fail_restore.
  rewrite Hphase.
  apply Nat.eqb_eq in Hlease.
  rewrite Hlease.
  destruct (Nat.ltb (S (restore_failures state)) max_failures); reflexivity.
Qed.

Theorem active_termination_requires_current_lease :
  forall lease state,
    restore_state_phase state = RestoreActive ->
    lease = restore_generation state ->
    restore_state_phase (fst (terminate_active_restore lease state)) = RestoreTerminal /\
    snd (terminate_active_restore lease state) = true.
Proof.
  intros lease state Hphase Hlease.
  unfold terminate_active_restore.
  rewrite Hphase.
  apply Nat.eqb_eq in Hlease.
  rewrite Hlease.
  split; reflexivity.
Qed.

Theorem stale_active_termination_is_idempotent :
  forall lease state,
    lease <> restore_generation state ->
    terminate_active_restore lease state = (state, false).
Proof.
  intros lease state Hlease.
  unfold terminate_active_restore.
  destruct (restore_state_phase state); try reflexivity.
  apply Nat.eqb_neq in Hlease.
  rewrite Hlease.
  reflexivity.
Qed.

Theorem matching_idle_request_failure_is_terminal :
  forall request_generation state,
    restore_state_phase state = RestoreIdle ->
    restore_retry_request_active state = true ->
    request_generation = restore_retry_request_generation state ->
    request_generation = restore_generation state ->
    restore_state_phase (fst (retry_request_fails request_generation state)) =
      RestoreTerminal /\
    restore_terminal_published
      (fst (retry_request_fails request_generation state)) = true /\
    snd (retry_request_fails request_generation state) = true.
Proof.
  intros request_generation state Hphase Hactive Hrequest Hcurrent.
  unfold retry_request_fails.
  rewrite Hactive.
  apply Nat.eqb_eq in Hrequest.
  rewrite Hrequest.
  rewrite Hphase.
  apply Nat.eqb_eq in Hcurrent.
  rewrite Hcurrent.
  repeat split; reflexivity.
Qed.

Theorem stale_request_failure_preserves_new_restore :
  forall request_generation state,
    restore_state_phase state = RestoreActive ->
    restore_retry_request_active state = true ->
    request_generation = restore_retry_request_generation state ->
    request_generation < restore_generation state ->
    restore_state_phase (fst (retry_request_fails request_generation state)) =
      RestoreActive /\
    restore_generation (fst (retry_request_fails request_generation state)) =
      restore_generation state /\
    restore_active_count (fst (retry_request_fails request_generation state)) =
      restore_active_count state /\
    restore_channels_ready (fst (retry_request_fails request_generation state)) =
      restore_channels_ready state /\
    restore_estimator_available (fst (retry_request_fails request_generation state)) =
      restore_estimator_available state /\
    restore_terminal_published (fst (retry_request_fails request_generation state)) =
      restore_terminal_published state /\
    snd (retry_request_fails request_generation state) = false.
Proof.
  intros request_generation state Hphase Hactive Hrequest _.
  unfold retry_request_fails.
  rewrite Hactive.
  apply Nat.eqb_eq in Hrequest.
  rewrite Hrequest.
  rewrite Hphase.
  cbn.
  rewrite Hphase.
  repeat split; reflexivity.
Qed.

Theorem stale_request_success_preserves_new_restore :
  forall request_generation state,
    restore_state_phase state = RestoreActive ->
    restore_retry_request_active state = true ->
    request_generation = restore_retry_request_generation state ->
    request_generation < restore_generation state ->
    restore_state_phase (retry_request_succeeds request_generation state) =
      RestoreActive /\
    restore_generation (retry_request_succeeds request_generation state) =
      restore_generation state /\
    restore_active_count (retry_request_succeeds request_generation state) =
      restore_active_count state /\
    restore_estimator_available (retry_request_succeeds request_generation state) =
      restore_estimator_available state /\
    restore_terminal_published (retry_request_succeeds request_generation state) =
      restore_terminal_published state.
Proof.
  intros request_generation state Hphase Hactive Hrequest _.
  unfold retry_request_succeeds.
  rewrite Hactive.
  apply Nat.eqb_eq in Hrequest.
  rewrite Hrequest.
  rewrite Hphase.
  cbn.
  unfold with_request_status.
  cbn.
  rewrite Hphase.
  repeat split; reflexivity.
Qed.

Theorem superseded_request_failure_is_idempotent :
  forall request_generation state,
    request_generation <> restore_retry_request_generation state ->
    retry_request_fails request_generation state = (state, false).
Proof.
  intros request_generation state Hrequest.
  unfold retry_request_fails.
  apply Nat.eqb_neq in Hrequest.
  rewrite Hrequest.
  destruct (restore_retry_request_active state); reflexivity.
Qed.

Theorem superseded_request_success_is_idempotent :
  forall request_generation state,
    request_generation <> restore_retry_request_generation state ->
    retry_request_succeeds request_generation state = state.
Proof.
  intros request_generation state Hrequest.
  unfold retry_request_succeeds.
  apply Nat.eqb_neq in Hrequest.
  rewrite Hrequest.
  destruct (restore_retry_request_active state); reflexivity.
Qed.

Theorem request_failure_cannot_revoke_running :
  forall request_generation state,
    restore_state_phase state = RestoreRunning ->
    restore_state_phase (fst (retry_request_fails request_generation state)) =
      RestoreRunning /\
    restore_running_published (fst (retry_request_fails request_generation state)) =
      restore_running_published state /\
    snd (retry_request_fails request_generation state) = false.
Proof.
  intros request_generation state Hphase.
  unfold retry_request_fails.
  destruct (restore_retry_request_active state) eqn:Hactive.
  - destruct
      (Nat.eqb request_generation (restore_retry_request_generation state))
      eqn:Hrequest.
    + cbn.
      unfold with_request_status.
      cbn.
      rewrite Hphase.
      repeat split; reflexivity.
    + cbn.
      repeat split; assumption || reflexivity.
  - cbn.
    repeat split; assumption || reflexivity.
Qed.

Theorem running_commit_is_last_and_permanent :
  forall lease state,
    restore_state_phase state = RestoreActive ->
    lease = restore_generation state ->
    let running := succeed_restore lease state in
    restore_state_phase running = RestoreRunning /\
    restore_running_published running = true /\
    restore_approved_persisted running = true /\
    restore_estimator_available running = false /\
    post_commit_notice_failure running = running.
Proof.
  intros lease state Hphase Hlease.
  unfold succeed_restore.
  rewrite Hphase.
  apply Nat.eqb_eq in Hlease.
  rewrite Hlease.
  repeat split; reflexivity.
Qed.

Theorem restart_reconstitutes_request_authority :
  restore_state_phase restart_restore = RestoreIdle /\
  restore_failures restart_restore = 0 /\
  restore_generation restart_restore = 0 /\
  restore_request_pending restart_restore = true /\
  restore_retry_request_active restart_restore = false /\
  restore_channels_ready restart_restore = true /\
  restore_active_count restart_restore = 0 /\
  restore_estimator_available restart_restore = true.
Proof.
  repeat split; reflexivity.
Qed.

Theorem restore_retry_ownership_contract :
  (forall state,
    restore_state_phase state <> RestoreIdle ->
    begin_restore state = state)
  /\
  (forall max_failures lease state,
    restore_state_phase state <> RestoreActive \/
    lease <> restore_generation state ->
    fail_restore max_failures lease state = state)
  /\
  (forall max_failures lease state,
    restore_state_phase state = RestoreActive ->
    lease = restore_generation state ->
    S (restore_failures state) < max_failures ->
    restore_state_phase (fail_restore max_failures lease state) = RestoreIdle /\
    restore_retry_request_active (fail_restore max_failures lease state) = true /\
    restore_retry_request_generation (fail_restore max_failures lease state) = lease /\
    restore_channels_ready (fail_restore max_failures lease state) = true /\
    restore_active_count (fail_restore max_failures lease state) = 0)
  /\
  (forall max_failures lease state,
    restore_state_phase state = RestoreActive ->
    lease = restore_generation state ->
    max_failures <= S (restore_failures state) ->
    restore_state_phase (fail_restore max_failures lease state) = RestoreTerminal /\
    restore_terminal_published (fail_restore max_failures lease state) = true /\
    restore_request_pending (fail_restore max_failures lease state) = false /\
    restore_retry_request_active (fail_restore max_failures lease state) = false /\
    restore_active_count (fail_restore max_failures lease state) = 0)
  /\
  (forall lease state,
    restore_state_phase state = RestoreActive ->
    lease = restore_generation state ->
    restore_state_phase (fst (terminate_active_restore lease state)) = RestoreTerminal /\
    snd (terminate_active_restore lease state) = true)
  /\
  (forall request_generation state,
    restore_state_phase state = RestoreIdle ->
    restore_retry_request_active state = true ->
    request_generation = restore_retry_request_generation state ->
    request_generation = restore_generation state ->
    restore_state_phase (fst (retry_request_fails request_generation state)) =
      RestoreTerminal /\
    restore_terminal_published
      (fst (retry_request_fails request_generation state)) = true /\
    snd (retry_request_fails request_generation state) = true)
  /\
  (forall request_generation state,
    restore_state_phase state = RestoreActive ->
    restore_retry_request_active state = true ->
    request_generation = restore_retry_request_generation state ->
    request_generation < restore_generation state ->
    restore_state_phase (fst (retry_request_fails request_generation state)) =
      RestoreActive /\
    restore_generation (fst (retry_request_fails request_generation state)) =
      restore_generation state /\
    restore_active_count (fst (retry_request_fails request_generation state)) =
      restore_active_count state /\
    restore_channels_ready (fst (retry_request_fails request_generation state)) =
      restore_channels_ready state /\
    restore_estimator_available (fst (retry_request_fails request_generation state)) =
      restore_estimator_available state /\
    restore_terminal_published (fst (retry_request_fails request_generation state)) =
      restore_terminal_published state /\
    snd (retry_request_fails request_generation state) = false)
  /\
  (forall request_generation state,
    restore_state_phase state = RestoreActive ->
    restore_retry_request_active state = true ->
    request_generation = restore_retry_request_generation state ->
    request_generation < restore_generation state ->
    restore_state_phase (retry_request_succeeds request_generation state) =
      RestoreActive /\
    restore_generation (retry_request_succeeds request_generation state) =
      restore_generation state /\
    restore_active_count (retry_request_succeeds request_generation state) =
      restore_active_count state /\
    restore_estimator_available (retry_request_succeeds request_generation state) =
      restore_estimator_available state /\
    restore_terminal_published (retry_request_succeeds request_generation state) =
      restore_terminal_published state)
  /\
  (forall request_generation state,
    request_generation <> restore_retry_request_generation state ->
    retry_request_fails request_generation state = (state, false))
  /\
  (forall request_generation state,
    request_generation <> restore_retry_request_generation state ->
    retry_request_succeeds request_generation state = state)
  /\
  (forall request_generation state,
    restore_state_phase state = RestoreRunning ->
    restore_state_phase (fst (retry_request_fails request_generation state)) =
      RestoreRunning /\
    restore_running_published (fst (retry_request_fails request_generation state)) =
      restore_running_published state /\
    snd (retry_request_fails request_generation state) = false)
  /\
  (forall lease state,
    restore_state_phase state = RestoreActive ->
    lease = restore_generation state ->
    let running := succeed_restore lease state in
    restore_state_phase running = RestoreRunning /\
    restore_running_published running = true /\
    restore_approved_persisted running = true /\
    restore_estimator_available running = false /\
    post_commit_notice_failure running = running).
Proof.
  split; [exact duplicate_restore_is_idempotent |].
  split; [exact non_owner_failure_is_idempotent |].
  split; [exact recoverable_failure_restores_retry_ownership |].
  split; [exact exhausted_failure_is_explicit |].
  split; [exact active_termination_requires_current_lease |].
  split; [exact matching_idle_request_failure_is_terminal |].
  split; [exact stale_request_failure_preserves_new_restore |].
  split; [exact stale_request_success_preserves_new_restore |].
  split; [exact superseded_request_failure_is_idempotent |].
  split; [exact superseded_request_success_is_idempotent |].
  split; [exact request_failure_cannot_revoke_running |].
  exact running_commit_is_last_and_permanent.
Qed.

Print Assumptions restore_claim_is_exclusive.
Print Assumptions duplicate_restore_is_idempotent.
Print Assumptions non_owner_failure_is_idempotent.
Print Assumptions recoverable_failure_restores_retry_ownership.
Print Assumptions exhausted_failure_is_explicit.
Print Assumptions recoverable_failure_preserves_estimator.
Print Assumptions active_termination_requires_current_lease.
Print Assumptions stale_active_termination_is_idempotent.
Print Assumptions matching_idle_request_failure_is_terminal.
Print Assumptions stale_request_failure_preserves_new_restore.
Print Assumptions stale_request_success_preserves_new_restore.
Print Assumptions superseded_request_failure_is_idempotent.
Print Assumptions superseded_request_success_is_idempotent.
Print Assumptions request_failure_cannot_revoke_running.
Print Assumptions running_commit_is_last_and_permanent.
Print Assumptions restart_reconstitutes_request_authority.
Print Assumptions restore_retry_ownership_contract.
