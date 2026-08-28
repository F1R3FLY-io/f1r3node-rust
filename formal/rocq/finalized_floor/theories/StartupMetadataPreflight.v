From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lists.List.
Import ListNotations.

Inductive join_path :=
| StoredApprovedBlock
| RequestedApprovedBlock.

Record startup_state := {
  metadata_verified : bool;
  running_event_published : bool;
  engine_running : bool;
  process_alive : bool;
  exit_nonzero : bool;
  startup_failure : option nat
}.

Definition initial_startup_state : startup_state :=
  {| metadata_verified := false;
     running_event_published := false;
     engine_running := false;
     process_alive := true;
     exit_nonzero := false;
     startup_failure := None |}.

Definition report_first_failure
  (existing : option nat) (failure : nat) : option nat :=
  match existing with
  | Some first => Some first
  | None => Some failure
  end.

Definition verify_metadata
  (path : join_path) (matches : bool) (state : startup_state)
  : startup_state :=
  if matches
  then
    {| metadata_verified := true;
       running_event_published := running_event_published state;
       engine_running := engine_running state;
       process_alive := process_alive state;
       exit_nonzero := exit_nonzero state;
       startup_failure := startup_failure state |}
  else
    match path with
    | StoredApprovedBlock =>
        {| metadata_verified := false;
           running_event_published := running_event_published state;
           engine_running := engine_running state;
           process_alive := false;
           exit_nonzero := true;
           startup_failure := startup_failure state |}
    | RequestedApprovedBlock =>
        {| metadata_verified := false;
           running_event_published := running_event_published state;
           engine_running := engine_running state;
           process_alive := process_alive state;
           exit_nonzero := exit_nonzero state;
           startup_failure := report_first_failure (startup_failure state) 1 |}
    end.

Definition publish_running (state : startup_state) : startup_state :=
  if metadata_verified state
  then
    {| metadata_verified := true;
       running_event_published := true;
       engine_running := true;
       process_alive := process_alive state;
       exit_nonzero := exit_nonzero state;
       startup_failure := startup_failure state |}
  else state.

Definition supervise_startup_failure (state : startup_state) : startup_state :=
  match startup_failure state with
  | Some _ =>
      {| metadata_verified := metadata_verified state;
         running_event_published := running_event_published state;
         engine_running := false;
         process_alive := false;
         exit_nonzero := true;
         startup_failure := startup_failure state |}
  | None => state
  end.

Definition complete_startup (path : join_path) (matches : bool) : startup_state :=
  let checked := verify_metadata path matches initial_startup_state in
  let published := publish_running checked in
  match path with
  | StoredApprovedBlock => published
  | RequestedApprovedBlock => supervise_startup_failure published
  end.

Theorem report_first_failure_preserves_existing :
  forall first later,
    report_first_failure (Some first) later = Some first.
Proof.
  reflexivity.
Qed.

Theorem publish_running_requires_verified :
  forall state,
    engine_running state = false ->
    engine_running (publish_running state) = true ->
    metadata_verified (publish_running state) = true.
Proof.
  intros state Hinitial Hrunning.
  unfold publish_running in *.
  destruct (metadata_verified state); simpl in *; congruence.
Qed.

Theorem mismatch_never_publishes_running :
  forall path,
    running_event_published (complete_startup path false) = false /\
    engine_running (complete_startup path false) = false.
Proof.
  destruct path; split; reflexivity.
Qed.

Theorem mismatch_exits_nonzero :
  forall path,
    process_alive (complete_startup path false) = false /\
    exit_nonzero (complete_startup path false) = true.
Proof.
  destruct path; split; reflexivity.
Qed.

Theorem requested_mismatch_is_supervised :
  startup_failure (verify_metadata RequestedApprovedBlock false initial_startup_state) = Some 1 /\
  process_alive (complete_startup RequestedApprovedBlock false) = false /\
  exit_nonzero (complete_startup RequestedApprovedBlock false) = true.
Proof.
  repeat split; reflexivity.
Qed.

Theorem matching_startup_runs_only_after_verification :
  forall path,
    metadata_verified (complete_startup path true) = true /\
    running_event_published (complete_startup path true) = true /\
    engine_running (complete_startup path true) = true /\
    process_alive (complete_startup path true) = true /\
    exit_nonzero (complete_startup path true) = false.
Proof.
  destruct path; repeat split; reflexivity.
Qed.

Theorem startup_metadata_preflight_end_to_end :
  forall path matches,
    let final := complete_startup path matches in
    (engine_running final = true -> metadata_verified final = true) /\
    (matches = false ->
      running_event_published final = false /\
      engine_running final = false /\
      process_alive final = false /\
      exit_nonzero final = true).
Proof.
  intros path matches.
  destruct path, matches; simpl; repeat split; congruence.
Qed.

Print Assumptions report_first_failure_preserves_existing.
Print Assumptions publish_running_requires_verified.
Print Assumptions mismatch_never_publishes_running.
Print Assumptions mismatch_exits_nonzero.
Print Assumptions requested_mismatch_is_supervised.
Print Assumptions matching_startup_runs_only_after_verification.
Print Assumptions startup_metadata_preflight_end_to_end.
