From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.

Inductive exact_deploy_status :=
| StatusPending
| StatusFinalized
| StatusFailed
| StatusExpired.

Inductive deploy_wait_outcome :=
| WaitPending
| WaitSucceeded
| WaitTerminalError
| WaitHistoryCorruption
| WaitTimedOut.

Inductive lfb_observation_class :=
| ObservationBaseline
| ObservationStable
| ObservationStrictProgress
| ObservationRegression
| ObservationRevision.

Definition classify_lfb_observation
  (baseline_known : bool)
  (previous_height previous_hash next_height next_hash : nat)
  : lfb_observation_class :=
  if negb baseline_known
  then ObservationBaseline
  else if next_height <? previous_height
       then ObservationRegression
       else if (next_height =? previous_height) && negb (next_hash =? previous_hash)
            then ObservationRevision
            else if previous_height <? next_height
                 then ObservationStrictProgress
                 else ObservationStable.

Definition progress_time_after_observation
  (observation : lfb_observation_class)
  (observed_at previous_progress_at : nat) : nat :=
  match observation with
  | ObservationStrictProgress => observed_at
  | _ => previous_progress_at
  end.

Definition history_corruption
  (observation : lfb_observation_class) : bool :=
  match observation with
  | ObservationRegression | ObservationRevision => true
  | _ => false
  end.

Definition classify_lfb_wait_observation
  (observation : lfb_observation_class) : deploy_wait_outcome :=
  if history_corruption observation
  then WaitHistoryCorruption
  else WaitPending.

Definition progress_deadline_expired
  (now last_progress_at stall_timeout absolute_timeout : nat) : bool :=
  (absolute_timeout <=? now) || (stall_timeout <=? now - last_progress_at).

Definition fixed_deadline_expired
  (now timeout : nat) : bool :=
  timeout <=? now.

Definition classify_deploy_wait
  (status : exact_deploy_status)
  (now last_progress_at stall_timeout absolute_timeout : nat)
  : deploy_wait_outcome :=
  if progress_deadline_expired
       now last_progress_at stall_timeout absolute_timeout
  then WaitTimedOut
  else
    match status with
    | StatusFinalized => WaitSucceeded
    | StatusFailed | StatusExpired => WaitTerminalError
    | StatusPending => WaitPending
    end.

Theorem exact_success_requires_exact_finalized_status :
  forall status now last_progress_at stall_timeout absolute_timeout,
    classify_deploy_wait
      status now last_progress_at stall_timeout absolute_timeout =
      WaitSucceeded ->
    status = StatusFinalized.
Proof.
  intros status now last_progress_at stall_timeout absolute_timeout Hsuccess.
  unfold classify_deploy_wait in Hsuccess.
  destruct
    (progress_deadline_expired
      now last_progress_at stall_timeout absolute_timeout);
    try discriminate.
  destruct status; congruence.
Qed.

Theorem in_budget_failed_or_expired_is_terminal_error :
  forall status now last_progress_at stall_timeout absolute_timeout,
    progress_deadline_expired
      now last_progress_at stall_timeout absolute_timeout = false ->
    status = StatusFailed \/ status = StatusExpired ->
    classify_deploy_wait
      status now last_progress_at stall_timeout absolute_timeout =
      WaitTerminalError.
Proof.
  intros status now last_progress_at stall_timeout absolute_timeout
    Hbudget Hterminal.
  unfold classify_deploy_wait.
  rewrite Hbudget.
  destruct Hterminal as [-> | ->]; reflexivity.
Qed.

Theorem only_strict_height_progress_renews_stall_budget :
  forall observation observed_at previous_progress_at,
    observation <> ObservationStrictProgress ->
    progress_time_after_observation
      observation observed_at previous_progress_at = previous_progress_at.
Proof.
  intros observation observed_at previous_progress_at Hnotstrict.
  destruct observation; simpl; congruence.
Qed.

Theorem strict_height_progress_renews_stall_budget :
  forall observed_at previous_progress_at,
    progress_time_after_observation
      ObservationStrictProgress observed_at previous_progress_at = observed_at.
Proof.
  reflexivity.
Qed.

Theorem first_observation_establishes_baseline_without_renewal :
  forall previous_height previous_hash next_height next_hash observed_at
    previous_progress_at,
    classify_lfb_observation
      false previous_height previous_hash next_height next_hash =
      ObservationBaseline /\
    progress_time_after_observation
      ObservationBaseline observed_at previous_progress_at = previous_progress_at.
Proof.
  intros.
  split; reflexivity.
Qed.

Theorem finalized_history_anomalies_fail_loudly :
  history_corruption ObservationRegression = true /\
  history_corruption ObservationRevision = true /\
  history_corruption ObservationBaseline = false /\
  history_corruption ObservationStable = false /\
  history_corruption ObservationStrictProgress = false.
Proof.
  repeat split; reflexivity.
Qed.

Theorem finalized_history_anomalies_are_terminal_observer_errors :
  classify_lfb_wait_observation ObservationRegression =
    WaitHistoryCorruption /\
  classify_lfb_wait_observation ObservationRevision =
    WaitHistoryCorruption.
Proof.
  split; reflexivity.
Qed.

Theorem concrete_revision_and_regression_are_detected :
  classify_lfb_observation true 6 10 6 11 = ObservationRevision /\
  classify_lfb_observation true 6 10 5 9 = ObservationRegression.
Proof.
  split; reflexivity.
Qed.

Theorem absolute_deadline_cannot_be_renewed :
  forall now last_progress_at stall_timeout absolute_timeout,
    absolute_timeout <= now ->
    progress_deadline_expired
      now last_progress_at stall_timeout absolute_timeout = true.
Proof.
  intros now last_progress_at stall_timeout absolute_timeout Habsolute.
  unfold progress_deadline_expired.
  apply Nat.leb_le in Habsolute.
  rewrite Habsolute.
  reflexivity.
Qed.

Theorem expired_observation_cannot_report_terminal_success :
  forall status now last_progress_at stall_timeout absolute_timeout,
    absolute_timeout <= now ->
    classify_deploy_wait
      status now last_progress_at stall_timeout absolute_timeout =
      WaitTimedOut.
Proof.
  intros status now last_progress_at stall_timeout absolute_timeout Habsolute.
  unfold classify_deploy_wait.
  rewrite
    (absolute_deadline_cannot_be_renewed
      now last_progress_at stall_timeout absolute_timeout Habsolute).
  reflexivity.
Qed.

Theorem terminal_response_at_deadline_is_timeout :
  classify_deploy_wait StatusFinalized 8 5 3 8 = WaitTimedOut /\
  classify_deploy_wait StatusFailed 8 5 3 8 = WaitTimedOut /\
  classify_deploy_wait StatusExpired 8 5 3 8 = WaitTimedOut.
Proof.
  repeat split; reflexivity.
Qed.

Theorem fixed_deadline_rejects_valid_intermediate_progress_trace :
  fixed_deadline_expired 45 45 = true /\
  progress_deadline_expired 45 43 45 135 = false.
Proof.
  split; reflexivity.
Qed.

Theorem reproduced_trace_succeeds_only_at_exact_terminality :
  classify_deploy_wait StatusPending 45 43 45 135 = WaitPending /\
  classify_deploy_wait StatusFinalized 49 43 45 135 = WaitSucceeded.
Proof.
  split; reflexivity.
Qed.

Theorem no_progress_trace_is_stall_bounded :
  forall start stall_timeout absolute_timeout,
    stall_timeout <= absolute_timeout ->
    progress_deadline_expired
      (start + stall_timeout) start stall_timeout absolute_timeout = true.
Proof.
  intros start stall_timeout absolute_timeout Hbound.
  unfold progress_deadline_expired.
  replace (start + stall_timeout - start) with stall_timeout by lia.
  rewrite Nat.leb_refl.
  destruct (absolute_timeout <=? start + stall_timeout); reflexivity.
Qed.

Print Assumptions exact_success_requires_exact_finalized_status.
Print Assumptions in_budget_failed_or_expired_is_terminal_error.
Print Assumptions only_strict_height_progress_renews_stall_budget.
Print Assumptions strict_height_progress_renews_stall_budget.
Print Assumptions first_observation_establishes_baseline_without_renewal.
Print Assumptions finalized_history_anomalies_fail_loudly.
Print Assumptions finalized_history_anomalies_are_terminal_observer_errors.
Print Assumptions concrete_revision_and_regression_are_detected.
Print Assumptions absolute_deadline_cannot_be_renewed.
Print Assumptions expired_observation_cannot_report_terminal_success.
Print Assumptions terminal_response_at_deadline_is_timeout.
Print Assumptions fixed_deadline_rejects_valid_intermediate_progress_trace.
Print Assumptions reproduced_trace_succeeds_only_at_exact_terminality.
Print Assumptions no_progress_trace_is_stall_bounded.
