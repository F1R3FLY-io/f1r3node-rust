From Stdlib Require Import Bool Arith Lists.List Lia.
Import ListNotations.

Inductive Phase := Pending | Presence | Admission | Ready | Authorized | CallbackSucceeded
                 | Succeeded | Failed | Cancelled.

Record Ticket := ticket {
  context_id : nat;
  request_id : nat;
  phase : Phase;
  callback_required : bool
}.

Definition same_key (a b : Ticket) :=
  Nat.eqb (context_id a) (context_id b) && Nat.eqb (request_id a) (request_id b).

Definition set_phase (t : Ticket) p :=
  ticket (context_id t) (request_id t) p (callback_required t).

Definition terminal p :=
  match p with Succeeded | Failed | Cancelled => true | _ => false end.

Definition cancel_ticket t :=
  if terminal (phase t) then t else set_phase t Cancelled.

Record State := state {
  current_context : option nat;
  current_ticket : option Ticket;
  active_scan : option Ticket;
  stopped : bool
}.

Definition live s key :=
  negb (stopped s) &&
  match current_context s, current_ticket s with
  | Some c, Some t => Nat.eqb c (context_id key) && same_key t key
  | _, _ => false
  end.

Definition may_authorize s key :=
  live s key &&
  match current_ticket s with
  | Some t => match phase t with Ready => callback_required t | _ => false end
  | None => false
  end.

Definition may_succeed s key :=
  live s key &&
  match current_ticket s with
  | Some t => match phase t with
              | Ready => negb (callback_required t)
              | CallbackSucceeded => callback_required t
              | _ => false
              end
  | None => false
  end.

Definition authorize s key :=
  if may_authorize s key then
    Some (state (current_context s)
      (option_map (fun t => set_phase t Authorized) (current_ticket s))
      (active_scan s) (stopped s))
  else None.

Definition succeed s key :=
  if may_succeed s key then
    Some (state (current_context s)
      (option_map (fun t => set_phase t Succeeded) (current_ticket s))
      (active_scan s) (stopped s))
  else None.

Definition may_record_callback s key :=
  live s key &&
  match current_ticket s with
  | Some t => match phase t with Authorized => callback_required t | _ => false end
  | None => false
  end.

Definition record_callback s key :=
  if may_record_callback s key then
    Some (state (current_context s)
      (option_map (fun t => set_phase t CallbackSucceeded) (current_ticket s))
      (active_scan s) (stopped s))
  else None.

Definition cancel s key :=
  state (current_context s)
    (option_map (fun t => if same_key t key then cancel_ticket t else t)
      (current_ticket s)) (active_scan s) (stopped s).

Definition stop s :=
  state None (option_map cancel_ticket (current_ticket s)) (active_scan s) true.

Definition retire s key :=
  state (current_context s) (current_ticket s)
    (match active_scan s with
     | Some t => if same_key t key then None else Some t
     | None => None
     end) (stopped s).

Theorem same_key_exact : forall a b,
  same_key a b = true <-> context_id a = context_id b /\ request_id a = request_id b.
Proof. intros. unfold same_key. rewrite andb_true_iff, !Nat.eqb_eq. tauto. Qed.

Theorem live_requires_exact_origin : forall s key,
  live s key = true -> stopped s = false /\
  current_context s = Some (context_id key) /\
  exists t, current_ticket s = Some t /\ same_key t key = true.
Proof.
  intros s key H. unfold live in H. apply andb_true_iff in H as [S H].
  apply negb_true_iff in S.
  destruct (current_context s) as [c|] eqn:C; [|discriminate].
  destruct (current_ticket s) as [t|] eqn:T; [|discriminate].
  apply andb_true_iff in H as [E K]. apply Nat.eqb_eq in E.
  subst. repeat split; auto. exists t. auto.
Qed.

Theorem authorization_requires_ready : forall s key s',
  authorize s key = Some s' ->
  live s key = true /\ exists t, current_ticket s = Some t /\
  phase t = Ready /\ callback_required t = true.
Proof.
  intros s key s' H. unfold authorize in H.
  destruct (may_authorize s key) eqn:M; [|discriminate].
  unfold may_authorize in M. apply andb_true_iff in M as [L M].
  split; [exact L|]. destruct (current_ticket s) as [t|] eqn:T; [|discriminate].
  destruct (phase t) eqn:P; try discriminate. exists t. auto.
Qed.

Theorem success_requires_exact_live_request : forall s key s',
  succeed s key = Some s' -> live s key = true.
Proof.
  intros s key s' H. unfold succeed in H.
  destruct (may_succeed s key) eqn:M; [|discriminate].
  unfold may_succeed in M. apply andb_true_iff in M. tauto.
Qed.

Theorem stop_prevents_authorization : forall s key, authorize (stop s) key = None.
Proof. intros. unfold authorize, may_authorize, live, stop. simpl. reflexivity. Qed.

Theorem stop_prevents_success : forall s key, succeed (stop s) key = None.
Proof. intros. unfold succeed, may_succeed, live, stop. simpl. reflexivity. Qed.

Theorem callback_observation_requires_authorization : forall s key s',
  record_callback s key = Some s' ->
  live s key = true /\ exists t, current_ticket s = Some t /\
  phase t = Authorized /\ callback_required t = true.
Proof.
  intros s key s' H. unfold record_callback in H.
  destruct (may_record_callback s key) eqn:M; [|discriminate].
  unfold may_record_callback in M. apply andb_true_iff in M as [L M].
  split; [exact L|]. destruct (current_ticket s) as [t|] eqn:T; [|discriminate].
  destruct (phase t) eqn:P; try discriminate. exists t. auto.
Qed.

Theorem required_callback_success_requires_observation : forall s key s' t,
  current_ticket s = Some t -> callback_required t = true ->
  succeed s key = Some s' -> phase t = CallbackSucceeded.
Proof.
  intros s key s' t T C H. unfold succeed in H.
  destruct (may_succeed s key) eqn:M; [|discriminate].
  unfold may_succeed in M. apply andb_true_iff in M as [_ M].
  rewrite T in M. simpl in M. destruct (phase t); try discriminate; rewrite C in M; discriminate || reflexivity.
Qed.

Theorem stop_prevents_callback_observation : forall s key,
  record_callback (stop s) key = None.
Proof. intros. unfold record_callback, may_record_callback, live, stop. simpl. reflexivity. Qed.

Theorem old_cancel_preserves_replacement : forall s key t,
  current_ticket s = Some t -> same_key t key = false ->
  current_ticket (cancel s key) = Some t.
Proof. intros. unfold cancel. simpl. rewrite H. simpl. rewrite H0. reflexivity. Qed.

Theorem cancellation_preserves_committed_success : forall s key t,
  current_ticket s = Some t -> phase t = Succeeded ->
  current_ticket (cancel s key) = Some t.
Proof.
  intros. unfold cancel. simpl. rewrite H. simpl.
  destruct (same_key t key); [|reflexivity].
  unfold cancel_ticket. rewrite H0. reflexivity.
Qed.

Theorem stop_preserves_committed_success : forall s t,
  current_ticket s = Some t -> phase t = Succeeded ->
  current_ticket (stop s) = Some t.
Proof.
  intros. unfold stop. simpl. rewrite H. simpl.
  unfold cancel_ticket. rewrite H0. reflexivity.
Qed.

Theorem cancellation_does_not_retire_active_work : forall s key,
  active_scan (cancel s key) = active_scan s.
Proof. reflexivity. Qed.

Theorem old_retirement_preserves_new_work : forall s key t,
  active_scan s = Some t -> same_key t key = false ->
  active_scan (retire s key) = Some t.
Proof. intros. unfold retire. simpl. rewrite H, H0. reflexivity. Qed.

Theorem exact_retirement_releases_work : forall s key t,
  active_scan s = Some t -> same_key t key = true ->
  active_scan (retire s key) = None.
Proof. intros. unfold retire. simpl. rewrite H, H0. reflexivity. Qed.

Inductive CleanupStep : State -> State -> Prop :=
| CancelStep : forall s key, CleanupStep s (cancel s key)
| StopStep : forall s, CleanupStep s (stop s)
| RetireStep : forall s key, CleanupStep s (retire s key).

Theorem cleanup_preserves_committed_success : forall s s' t,
  CleanupStep s s' -> current_ticket s = Some t -> phase t = Succeeded ->
  current_ticket s' = Some t.
Proof.
  intros s s' t Step T P. destruct Step.
  - eapply cancellation_preserves_committed_success; eauto.
  - eapply stop_preserves_committed_success; eauto.
  - exact T.
Qed.

Inductive CleanupHistory : State -> State -> Prop :=
| CleanupNil : forall s, CleanupHistory s s
| CleanupCons : forall s middle last,
    CleanupStep s middle -> CleanupHistory middle last -> CleanupHistory s last.

Theorem arbitrary_cleanup_history_preserves_success : forall s s' t,
  CleanupHistory s s' -> current_ticket s = Some t -> phase t = Succeeded ->
  current_ticket s' = Some t.
Proof.
  intros s s' t History. induction History; intros T P; [exact T|].
  apply IHHistory; [|exact P]. eapply cleanup_preserves_committed_success; eauto.
Qed.

Print Assumptions same_key_exact.
Print Assumptions live_requires_exact_origin.
Print Assumptions authorization_requires_ready.
Print Assumptions success_requires_exact_live_request.
Print Assumptions stop_prevents_authorization.
Print Assumptions stop_prevents_success.
Print Assumptions callback_observation_requires_authorization.
Print Assumptions required_callback_success_requires_observation.
Print Assumptions stop_prevents_callback_observation.
Print Assumptions old_cancel_preserves_replacement.
Print Assumptions cancellation_preserves_committed_success.
Print Assumptions stop_preserves_committed_success.
Print Assumptions cancellation_does_not_retire_active_work.
Print Assumptions old_retirement_preserves_new_work.
Print Assumptions exact_retirement_releases_work.
Print Assumptions cleanup_preserves_committed_success.
Print Assumptions arbitrary_cleanup_history_preserves_success.
