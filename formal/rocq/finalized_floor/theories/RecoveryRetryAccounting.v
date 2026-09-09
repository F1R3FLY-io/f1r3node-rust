From Stdlib Require Import Arith Bool Lia Lists.List.
Import ListNotations.

Section Retry.
Context {Key : Type}.

Record retry_state := RetryState {
  remaining : nat;
  selected : nat;
  cancelled : nat;
  pending : option Key;
  outstanding : option Key;
  failed : bool;
  finished : bool
}.

Inductive retry_operation :=
| Select (key : Key)
| EmptyVisit
| Retry
| Resolve
| RecordFailure
| Finish
| Cancel.

Definition retry_initial (count : nat) : retry_state :=
  RetryState count 0 0 None None false false.

Definition retry_apply (op : retry_operation) (s : retry_state) : retry_state :=
  if finished s then s else
  match op with
  | Select key =>
      match pending s, remaining s with
      | None, S rest =>
          RetryState rest (S (selected s)) (cancelled s)
            (Some key) (Some key) (failed s) false
      | _, _ => s
      end
  | EmptyVisit =>
      match pending s, remaining s with
      | None, S rest =>
          RetryState rest (S (selected s)) (cancelled s) None None (failed s) false
      | _, _ => s
      end
  | Retry => s
  | Resolve =>
      RetryState (remaining s) (selected s) (cancelled s)
        None None (failed s) false
  | RecordFailure =>
      RetryState (remaining s) (selected s) (cancelled s)
        None None true false
  | Finish =>
      match pending s, remaining s with
      | None, 0 =>
          RetryState 0 (selected s) (cancelled s) None None (failed s) true
      | _, _ => s
      end
  | Cancel =>
      RetryState 0 (selected s) (cancelled s + remaining s)
        None None true true
  end.

Fixpoint retry_history (ops : list retry_operation) (s : retry_state) : retry_state :=
  match ops with
  | [] => s
  | op :: tail => retry_history tail (retry_apply op s)
  end.

Definition retry_invariant (count : nat) (s : retry_state) : Prop :=
  pending s = outstanding s /\
  remaining s + selected s + cancelled s = count /\
  (finished s = true -> remaining s = 0 /\ outstanding s = None).

Theorem retry_initial_invariant : forall count,
  retry_invariant count (retry_initial count).
Proof.
  intros. unfold retry_invariant, retry_initial. simpl. intuition lia.
Qed.

Theorem retry_step_preserves_invariant : forall count op s,
  retry_invariant count s -> retry_invariant count (retry_apply op s).
Proof.
  intros count op [r v c p w f done] H.
  unfold retry_invariant in *. simpl in *.
  destruct H as [Hpw [Htotal Hdone]].
  subst w. destruct done; [exact (conj eq_refl (conj Htotal Hdone))|].
  destruct op; simpl; try (intuition lia).
  - destruct p; destruct r; simpl; intuition lia.
  - destruct p; destruct r; simpl; intuition lia.
  - destruct p; destruct r; simpl; intuition lia.
Qed.

Theorem retry_history_preserves_invariant : forall ops count s,
  retry_invariant count s -> retry_invariant count (retry_history ops s).
Proof.
  induction ops; simpl; intros; auto using retry_step_preserves_invariant.
Qed.

Theorem retries_preserve_candidate : forall s,
  retry_apply Retry s = s.
Proof. intros [r v c p w f []]; reflexivity. Qed.

Theorem retries_consume_no_selection : forall s,
  selected (retry_apply Retry s) = selected s.
Proof. intros. rewrite retries_preserve_candidate. reflexivity. Qed.

Theorem selection_is_charged_once : forall key r v c f,
  retry_apply (Select key) (RetryState (S r) v c None None f false) =
  RetryState r (S v) c (Some key) (Some key) f false.
Proof. reflexivity. Qed.

Theorem empty_visit_consumes_exactly_one_selection : forall r v c f,
  retry_apply EmptyVisit (RetryState (S r) v c None None f false) =
  RetryState r (S v) c None None f false.
Proof. reflexivity. Qed.

Theorem preselection_failure_consumes_exactly_one_selection : forall r v c f,
  retry_apply RecordFailure
    (retry_apply EmptyVisit (RetryState (S r) v c None None f false)) =
  RetryState r (S v) c None None true false.
Proof. reflexivity. Qed.

Theorem pending_blocks_reselection : forall key next r v c f,
  retry_apply (Select next) (RetryState r v c (Some key) (Some key) f false) =
  RetryState r v c (Some key) (Some key) f false.
Proof. intros. destruct r; reflexivity. Qed.

Theorem pending_blocks_finish : forall key r v c f,
  finished (retry_apply Finish (RetryState r v c (Some key) (Some key) f false)) = false.
Proof. intros. destruct r; reflexivity. Qed.

Theorem completed_history_has_no_obligation : forall ops count,
  finished (retry_history ops (retry_initial count)) = true ->
  outstanding (retry_history ops (retry_initial count)) = None.
Proof.
  intros ops count H.
  pose proof (retry_history_preserves_invariant ops count _ (retry_initial_invariant count)) as I.
  destruct I as [_ [_ I]]. exact (proj2 (I H)).
Qed.

Theorem no_history_exceeds_selected_budget : forall ops count,
  selected (retry_history ops (retry_initial count)) <= count.
Proof.
  intros.
  pose proof (retry_history_preserves_invariant ops count _ (retry_initial_invariant count)) as I.
  destruct I as [_ [I _]]. lia.
Qed.

Theorem failure_is_sticky : forall op s,
  failed s = true -> failed (retry_apply op s) = true.
Proof.
  intros op [r v c p w f done] H. simpl in H. subst f.
  destruct done; [reflexivity|]. destruct op; simpl; try reflexivity.
  - destruct p; destruct r; reflexivity.
  - destruct p; destruct r; reflexivity.
  - destruct p; destruct r; reflexivity.
Qed.

Theorem history_failure_is_sticky : forall ops s,
  failed s = true -> failed (retry_history ops s) = true.
Proof.
  induction ops; simpl; intros; auto using failure_is_sticky.
Qed.

End Retry.

Record page_state := PageState {
  allowance : nat;
  spent : nat;
  loads : nat;
  load_permit : bool;
  suspended : bool
}.

Inductive page_operation :=
| BeginAttempt | Load | EndAttempt | Suspend | Resume.

Definition page_initial (size : nat) : page_state :=
  PageState size 0 0 false false.

Definition page_apply (size : nat) (op : page_operation) (s : page_state) : page_state :=
  match op with
  | BeginAttempt =>
      match allowance s, load_permit s, suspended s with
      | S rest, false, false => PageState rest (S (spent s)) (loads s) true false
      | _, _, _ => s
      end
  | Load =>
      if load_permit s && negb (suspended s)
      then PageState (allowance s) (spent s) (S (loads s)) false false
      else s
  | EndAttempt =>
      PageState (allowance s) (spent s) (loads s) false (suspended s)
  | Suspend => PageState (allowance s) (spent s) (loads s) false true
  | Resume => if suspended s then page_initial size else s
  end.

Definition page_invariant (size : nat) (s : page_state) : Prop :=
  spent s + allowance s = size /\
  loads s + (if load_permit s then 1 else 0) <= spent s /\
  (suspended s = true -> load_permit s = false).

Fixpoint page_history (size : nat) (ops : list page_operation) (s : page_state) : page_state :=
  match ops with
  | [] => s
  | op :: tail => page_history size tail (page_apply size op s)
  end.

Theorem page_initial_invariant : forall size,
  page_invariant size (page_initial size).
Proof. intros. unfold page_invariant, page_initial. simpl. intuition lia. Qed.

Theorem page_step_preserves_invariant : forall size op s,
  page_invariant size s -> page_invariant size (page_apply size op s).
Proof.
  intros size op [a v l p b] H.
  unfold page_invariant in *. simpl in *.
  destruct op; destruct a; destruct p; destruct b; simpl in *; intuition lia.
Qed.

Theorem page_history_preserves_invariant : forall ops size s,
  page_invariant size s -> page_invariant size (page_history size ops s).
Proof.
  induction ops; simpl; intros; auto using page_step_preserves_invariant.
Qed.

Theorem page_loads_are_bounded : forall size ops,
  loads (page_history size ops (page_initial size)) <= size.
Proof.
  intros.
  pose proof (page_history_preserves_invariant ops size _ (page_initial_invariant size)) as I.
  destruct I as [I [J _]]. lia.
Qed.

Theorem resume_without_suspend_cannot_refill : forall size a v l p,
  page_apply size Resume (PageState a v l p false) = PageState a v l p false.
Proof. reflexivity. Qed.

Theorem synchronous_release_preserves_budget : forall size s,
  allowance (page_apply size EndAttempt s) = allowance s /\
  spent (page_apply size EndAttempt s) = spent s.
Proof. intros. split; reflexivity. Qed.

Theorem no_second_load_without_new_attempt : forall size a v l b,
  page_apply size Load (PageState a v l false b) = PageState a v l false b.
Proof. reflexivity. Qed.

Print Assumptions retry_initial_invariant.
Print Assumptions retry_step_preserves_invariant.
Print Assumptions retry_history_preserves_invariant.
Print Assumptions retries_preserve_candidate.
Print Assumptions retries_consume_no_selection.
Print Assumptions selection_is_charged_once.
Print Assumptions empty_visit_consumes_exactly_one_selection.
Print Assumptions preselection_failure_consumes_exactly_one_selection.
Print Assumptions pending_blocks_reselection.
Print Assumptions pending_blocks_finish.
Print Assumptions completed_history_has_no_obligation.
Print Assumptions no_history_exceeds_selected_budget.
Print Assumptions failure_is_sticky.
Print Assumptions history_failure_is_sticky.
Print Assumptions page_initial_invariant.
Print Assumptions page_step_preserves_invariant.
Print Assumptions page_history_preserves_invariant.
Print Assumptions page_loads_are_bounded.
Print Assumptions resume_without_suspend_cannot_refill.
Print Assumptions synchronous_release_preserves_budget.
Print Assumptions no_second_load_without_new_attempt.
