From Stdlib Require Import Arith Bool Lia Lists.List.
Import ListNotations.

Record signal := Signal { pending : bool; stopped : bool }.
Definition request (s : signal) : signal :=
  if stopped s then s else Signal true false.
Definition take (s : signal) : bool * signal :=
  (pending s && negb (stopped s), Signal false (stopped s)).
Definition stop (_ : signal) : signal := Signal false true.
Inductive operation := Request | Take | Stop.
Definition apply (op : operation) (s : signal) : signal :=
  match op with Request => request s | Take => snd (take s) | Stop => stop s end.
Fixpoint history (ops : list operation) (s : signal) : signal :=
  match ops with [] => s | op :: tail => history tail (apply op s) end.

Theorem open_request_is_pending : forall p, pending (request (Signal p false)) = true.
Proof. reflexivity. Qed.
Theorem stopped_request_preserves_state : forall p, request (Signal p true) = Signal p true.
Proof. reflexivity. Qed.
Theorem duplicate_requests_coalesce : forall s, request (request s) = request s.
Proof. intros [p s]. destruct s; reflexivity. Qed.
Theorem take_consumes_pending : forall p, take (Signal p false) = (p, Signal false false).
Proof. intros []; reflexivity. Qed.
Theorem second_take_has_no_work : forall s, fst (take (snd (take s))) = false.
Proof. intros [p s]. reflexivity. Qed.
Theorem request_after_take_is_retained : forall p,
  fst (take (request (snd (take (Signal p false))))) = true.
Proof. reflexivity. Qed.
Theorem stop_discards_pending : forall s, stop s = Signal false true.
Proof. reflexivity. Qed.
Theorem stopped_history_remains_closed : forall ops,
  history ops (Signal false true) = Signal false true.
Proof. induction ops as [|[] tail IH]; simpl; assumption || reflexivity. Qed.

Definition advance (remaining budget : nat) : nat := remaining - Nat.min remaining budget.
Definition combine_failure (previous current : bool) : bool := previous || current.
Fixpoint failure_history (errors : list bool) (prior : bool) : bool :=
  match errors with [] => prior | error :: tail => failure_history tail (combine_failure prior error) end.

Theorem page_visits_do_not_exceed_budget : forall remaining budget,
  Nat.min remaining budget <= budget.
Proof. apply Nat.le_min_r. Qed.
Theorem page_preserves_total_visits : forall remaining budget,
  advance remaining budget + Nat.min remaining budget = remaining.
Proof. intros. unfold advance. pose proof (Nat.le_min_l remaining budget). lia. Qed.
Theorem positive_page_makes_progress : forall remaining budget,
  remaining > 0 -> budget > 0 -> advance remaining budget < remaining.
Proof. intros. unfold advance. destruct remaining; destruct budget; simpl in *; lia. Qed.
Theorem failed_page_remains_failed : forall current,
  combine_failure true current = true.
Proof. reflexivity. Qed.
Theorem error_history_preserves_failure : forall errors, failure_history errors true = true.
Proof. induction errors; simpl; auto. Qed.
Theorem error_in_history_blocks_proposal : forall errors prior,
  In true errors -> failure_history errors prior = true.
Proof.
  induction errors as [|error tail IH]; intros prior H; simpl in *; [contradiction|].
  destruct H as [H|H].
  - subst error. destruct prior; apply error_history_preserves_failure.
  - apply IH. exact H.
Qed.

Inductive demand := Idle | Work (proposal : bool) | Stopped.
Definition request_demand (proposal : bool) (state : demand) : demand :=
  match state with
  | Idle => Work proposal
  | Work prior => Work (prior || proposal)
  | Stopped => Stopped
  end.
Definition take_demand (state : demand) : demand * demand :=
  match state with Stopped => (Stopped, Stopped) | _ => (state, Idle) end.
Theorem demand_request_cannot_restart_stop : forall proposal,
  request_demand proposal Stopped = Stopped.
Proof. reflexivity. Qed.
Theorem demand_request_preserves_proposal : forall proposal,
  request_demand proposal (Work true) = Work true.
Proof. reflexivity. Qed.
Theorem demand_requests_commute : forall state a b,
  request_demand a (request_demand b state) = request_demand b (request_demand a state).
Proof. intros [] [] []; reflexivity || destruct proposal; reflexivity. Qed.
Theorem demand_take_cannot_restart_stop : take_demand Stopped = (Stopped, Stopped).
Proof. reflexivity. Qed.
Theorem demand_take_preserves_proposal : forall proposal,
  take_demand (Work proposal) = (Work proposal, Idle).
Proof. reflexivity. Qed.
Theorem demand_new_request_survives_old_take : forall old new,
  request_demand new (snd (take_demand (Work old))) = Work new.
Proof. reflexivity. Qed.

Print Assumptions open_request_is_pending.
Print Assumptions stopped_request_preserves_state.
Print Assumptions duplicate_requests_coalesce.
Print Assumptions take_consumes_pending.
Print Assumptions second_take_has_no_work.
Print Assumptions request_after_take_is_retained.
Print Assumptions stop_discards_pending.
Print Assumptions stopped_history_remains_closed.
Print Assumptions page_visits_do_not_exceed_budget.
Print Assumptions page_preserves_total_visits.
Print Assumptions positive_page_makes_progress.
Print Assumptions failed_page_remains_failed.
Print Assumptions error_history_preserves_failure.
Print Assumptions error_in_history_blocks_proposal.
Print Assumptions demand_request_cannot_restart_stop.
Print Assumptions demand_request_preserves_proposal.
Print Assumptions demand_requests_commute.
Print Assumptions demand_take_cannot_restart_stop.
Print Assumptions demand_take_preserves_proposal.
Print Assumptions demand_new_request_survives_old_take.
