From Stdlib Require Import Lists.List Arith Lia.
Import ListNotations.

Section RetryHandoff.
Context {Key Worker Policy : Type}.
Context (key_eq : forall x y : Key, {x = y} + {x <> y}).
Context (worker_eq : forall x y : Worker, {x = y} + {x <> y}).

Record retry_state := RetryState {
  tracked : list Key;
  ready : Key -> Prop;
  owner : Worker -> option Key;
  durable : Key -> Prop;
  terminal : Key -> Prop;
  seen : Key -> Prop;
  acknowledged : Key -> Prop;
  policy : Key -> Policy
}.

Definition owned (s : retry_state) (b : Key) : Prop :=
  exists w, owner s w = Some b.

Definition successor (s : retry_state) (b : Key) : Prop :=
  durable s b \/ terminal s b \/ ready s b.

Definition live_owner (s : retry_state) : Prop :=
  forall b, seen s b -> successor s b \/ owned s b.

Definition unique_lease (s : retry_state) : Prop :=
  forall w v b, owner s w = Some b -> owner s v = Some b -> w = v.

Definition invariant (cap : nat) (s : retry_state) : Prop :=
  length (tracked s) <= cap /\
  (forall b, ready s b -> In b (tracked s)) /\
  unique_lease s /\ live_owner s /\
  (forall b, acknowledged s b -> durable s b \/ terminal s b).

Definition initial (policies : Key -> Policy) : retry_state :=
  RetryState [] (fun _ => False) (fun _ => None) (fun _ => False)
    (fun _ => False) (fun _ => False) (fun _ => False) policies.

Definition add_key (b : Key) (keys : list Key) : list Key :=
  if in_dec key_eq b keys then keys else b :: keys.

Definition can_track (cap : nat) (s : retry_state) (b : Key) : Prop :=
  In b (tracked s) \/ length (tracked s) < cap.

Definition reserve (w : Worker) (b : Key) (s : retry_state) : retry_state :=
  RetryState (tracked s) (ready s)
    (fun v => if worker_eq v w then Some b else owner s v)
    (durable s) (terminal s) (seen s)
    (acknowledged s) (policy s).

Definition publish (b : Key) (s : retry_state) : retry_state :=
  RetryState (tracked s) (ready s) (owner s) (durable s) (terminal s)
    (fun k => k = b \/ seen s k) (acknowledged s) (policy s).

Definition receive (cap : nat) (b : Key) (s : retry_state) : retry_state :=
  RetryState
    (if lt_dec (length (tracked s)) cap then add_key b (tracked s) else tracked s)
    (fun k => ready s k /\ k <> b) (owner s) (durable s) (terminal s)
    (seen s) (acknowledged s) (policy s).

Definition reopen (b : Key) (s : retry_state) : retry_state :=
  RetryState (add_key b (tracked s)) (fun k => k = b \/ ready s k)
    (owner s) (durable s) (terminal s) (seen s) (acknowledged s) (policy s).

Inductive handoff_result :=
| Tracked : retry_state -> handoff_result
| AtCapacity : retry_state -> handoff_result.

Definition handoff (cap : nat) (b : Key) (s : retry_state) : handoff_result :=
  if in_dec key_eq b (tracked s) then Tracked (reopen b s)
  else if lt_dec (length (tracked s)) cap then Tracked (reopen b s)
  else AtCapacity s.

Definition release (w : Worker) (s : retry_state) : retry_state :=
  RetryState (tracked s) (ready s)
    (fun v => if worker_eq v w then None else owner s v)
    (durable s) (terminal s) (seen s) (acknowledged s) (policy s).

Definition commit (b : Key) (s : retry_state) : retry_state :=
  RetryState (tracked s) (ready s) (owner s)
    (fun k => k = b \/ durable s k) (terminal s)
    (seen s) (acknowledged s) (policy s).

Definition acknowledge (b : Key) (s : retry_state) : retry_state :=
  RetryState (remove key_eq b (tracked s)) (fun k => ready s k /\ k <> b)
    (owner s) (durable s) (terminal s) (seen s)
    (fun k => k = b \/ acknowledged s k) (policy s).

Definition resolve (b : Key) (s : retry_state) : retry_state :=
  RetryState (remove key_eq b (tracked s)) (fun k => ready s k /\ k <> b)
    (owner s) (fun k => durable s k /\ k <> b)
    (fun k => k = b \/ terminal s k) (seen s) (acknowledged s) (policy s).

Definition restart (fresh_policy : Key -> Policy) (s : retry_state) : retry_state :=
  RetryState [] (fun _ => False) (fun _ => None) (durable s) (terminal s)
    (fun k => durable s k \/ terminal s k \/ acknowledged s k)
    (acknowledged s) fresh_policy.

Lemma add_key_contains : forall b keys k,
  k = b \/ In k keys -> In k (add_key b keys).
Proof.
  intros b keys k H. unfold add_key. destruct (in_dec key_eq b keys); simpl in *;
    destruct H; subst; auto.
Qed.

Lemma add_key_bound : forall cap b keys,
  length keys <= cap -> In b keys \/ length keys < cap ->
  length (add_key b keys) <= cap.
Proof.
  intros cap b keys HB HC. unfold add_key.
  destruct (in_dec key_eq b keys); simpl; intuition lia.
Qed.

Lemma remove_bound : forall b keys,
  length (remove key_eq b keys) <= length keys.
Proof.
  intros b keys. induction keys as [|k keys IH]; simpl; [lia|].
  destruct (key_eq b k); simpl; lia.
Qed.

Theorem initial_invariant : forall cap policies, invariant cap (initial policies).
Proof.
  intros. unfold invariant, unique_lease, live_owner, successor, owned, initial.
  simpl. repeat split; intros; try contradiction; try discriminate; lia.
Qed.

Theorem reserve_preserves : forall cap w b s,
  invariant cap s -> owner s w = None -> ~owned s b ->
  invariant cap (reserve w b s).
Proof.
  intros cap w b s [HB [HR [HU [HL HA]]]] HW HN.
  unfold invariant. simpl. split; [exact HB|]. split; [exact HR|]. split.
  - unfold unique_lease. simpl. intros u v k Eu Ev.
    destruct (worker_eq u w) as [->|NU]; destruct (worker_eq v w) as [->|NV].
    + reflexivity.
    + inversion Eu; subst. exfalso. apply HN. exists v. exact Ev.
    + inversion Ev; subst. exfalso. apply HN. exists u. exact Eu.
    + eapply HU; eauto.
  - split; [|exact HA]. unfold live_owner, successor, owned in *. simpl.
    intros k HS. destruct (HL k HS) as [HP|[v HV]]; [left; exact HP|].
    right. exists v. destruct (worker_eq v w); subst; congruence.
Qed.

Theorem publish_preserves : forall cap b s,
  invariant cap s -> owned s b -> invariant cap (publish b s).
Proof.
  intros cap b s [HB [HR [HU [HL HA]]]] HO.
  unfold invariant. simpl. split; [exact HB|]. split; [exact HR|].
  split; [exact HU|]. split; [|exact HA].
  unfold live_owner, successor, owned in *. simpl. intros k [E|HS];
    [subst; auto|apply HL; exact HS].
Qed.

Theorem receive_preserves : forall cap b s,
  invariant cap s -> owned s b -> invariant cap (receive cap b s).
Proof.
  intros cap b s [HB [HR [HU [HL HA]]]] HO.
  unfold invariant. simpl. split.
  - destruct (lt_dec (length (tracked s)) cap); auto using add_key_bound.
  - split.
    + intros k [HK HN]. destruct (lt_dec (length (tracked s)) cap);
        auto using add_key_contains.
    + split; [exact HU|]. split; [|exact HA].
      unfold live_owner, successor, owned in *. simpl. intros k HS.
      destruct (key_eq k b) as [->|HN]; [auto|].
      destruct (HL k HS) as [[HD|[HT|HRK]]|HW]; auto.
Qed.

Theorem reopen_preserves : forall cap b s,
  invariant cap s -> can_track cap s b -> invariant cap (reopen b s).
Proof.
  intros cap b s [HB [HR [HU [HL HA]]]] HC.
  unfold invariant. simpl. split; [apply add_key_bound; assumption|]. split.
  - intros k [E|HK]; apply add_key_contains; auto.
  - split; [exact HU|]. split; [|exact HA].
    unfold live_owner, successor, owned in *. simpl. intros k HS.
    destruct (HL k HS) as [[HD|[HT|HK]]|HW]; auto.
Qed.

Theorem handoff_tracked_ready : forall cap b s after,
  handoff cap b s = Tracked after ->
  can_track cap s b /\ ready after b /\ policy after = policy s /\ owner after = owner s.
Proof.
  intros cap b s after H. unfold handoff in H.
  destruct (in_dec key_eq b (tracked s));
    [|destruct (lt_dec (length (tracked s)) cap)]; try discriminate;
    inversion H; subst; unfold can_track; simpl; auto.
Qed.

Theorem handoff_capacity_preserves_owner : forall cap b s after,
  handoff cap b s = AtCapacity after ->
  after = s /\ ~can_track cap s b.
Proof.
  intros cap b s after H. unfold handoff in H.
  destruct (in_dec key_eq b (tracked s)); [discriminate|].
  destruct (lt_dec (length (tracked s)) cap); [discriminate|].
  inversion H; subst. unfold can_track. tauto.
Qed.

Theorem release_preserves : forall cap w s,
  invariant cap s -> (forall b, owner s w = Some b -> seen s b -> successor s b) ->
  invariant cap (release w s).
Proof.
  intros cap w s [HB [HR [HU [HL HA]]]] HS.
  unfold invariant. simpl. split; [exact HB|]. split; [exact HR|]. split.
  - unfold unique_lease in *. simpl. intros u v b Eu Ev.
    destruct (worker_eq u w); [discriminate|].
    destruct (worker_eq v w); [discriminate|]. eauto.
  - split; [|exact HA]. unfold live_owner, successor, owned in *. simpl.
    intros b HBK. destruct (HL b HBK) as [HP|[v HV]]; [auto|].
    destruct (worker_eq v w) as [E|N].
    + subst. left. apply HS; assumption.
    + right. exists v. destruct (worker_eq v w); congruence.
Qed.

Theorem commit_preserves : forall cap b s,
  invariant cap s -> invariant cap (commit b s).
Proof.
  intros cap b s [HB [HR [HU [HL HA]]]].
  unfold invariant, live_owner, successor, owned in *. simpl.
  repeat split; auto.
  - intros k HK. destruct (HL k HK) as [[HD|[HT|HRK]]|HW]; auto.
  - intros k HK. destruct (HA k HK); auto.
Qed.

Theorem acknowledge_preserves : forall cap b s,
  invariant cap s -> durable s b \/ terminal s b ->
  invariant cap (acknowledge b s).
Proof.
  intros cap b s [HB [HR [HU [HL HA]]]] HP.
  unfold invariant. simpl. split; [pose proof (remove_bound b (tracked s)); lia|].
  split.
  - intros k [HK HN]. apply in_in_remove; auto.
  - split; [exact HU|]. split.
    + unfold live_owner, successor, owned in *. simpl. intros k HK.
      destruct (key_eq k b) as [->|HN]; [tauto|].
      destruct (HL k HK) as [[HD|[HT|HRK]]|HW]; auto.
    + intros k [E|HK]; [subst; exact HP|apply HA; exact HK].
Qed.

Theorem resolve_preserves : forall cap b s,
  invariant cap s -> invariant cap (resolve b s).
Proof.
  intros cap b s [HB [HR [HU [HL HA]]]].
  unfold invariant. simpl. split; [pose proof (remove_bound b (tracked s)); lia|].
  split.
  - intros k [HK HN]. apply in_in_remove; auto.
  - split; [exact HU|]. split.
    + unfold live_owner, successor, owned in *. simpl. intros k HK.
      destruct (key_eq k b) as [->|HN]; [auto|].
      destruct (HL k HK) as [[HD|[HT|HRK]]|HW]; auto.
    + intros k HK. destruct (key_eq k b) as [->|HN]; [auto|].
      destruct (HA k HK); auto.
Qed.

Theorem restart_preserves : forall cap fresh_policy s,
  invariant cap s -> invariant cap (restart fresh_policy s).
Proof.
  intros cap fresh_policy s [HB [HR [HU [HL HA]]]].
  unfold invariant, unique_lease, live_owner, successor, owned in *. simpl.
  repeat split; try lia; try tauto; try discriminate.
  intros b [HD|[HT|HK]]; auto. destruct (HA b HK); auto.
Qed.

Inductive local_step (cap : nat) : retry_state -> retry_state -> Prop :=
| Reserve : forall w b s, owner s w = None -> ~owned s b ->
    local_step cap s (reserve w b s)
| Receive : forall b s, owned s b -> local_step cap s (receive cap b s)
| Publish : forall b s, owned s b -> local_step cap s (publish b s)
| Reopen : forall b s, can_track cap s b -> local_step cap s (reopen b s)
| Release : forall w s, (forall b, owner s w = Some b -> seen s b -> successor s b) ->
    local_step cap s (release w s)
| Commit : forall b s, local_step cap s (commit b s)
| Acknowledge : forall b s, durable s b \/ terminal s b ->
    local_step cap s (acknowledge b s)
| Resolve : forall b s, local_step cap s (resolve b s)
| FailRetryOrRefuse : forall s, local_step cap s s.

Inductive local_history (cap : nat) : retry_state -> retry_state -> Prop :=
| NoSteps : forall s, local_history cap s s
| MoreSteps : forall before middle after,
    local_history cap before middle -> local_step cap middle after ->
    local_history cap before after.

Theorem local_step_preserves : forall cap before after,
  local_step cap before after -> invariant cap before -> invariant cap after.
Proof.
  intros cap before after H. destruct H; intros;
    eauto using reserve_preserves, receive_preserves, publish_preserves, reopen_preserves,
      release_preserves, commit_preserves, acknowledge_preserves, resolve_preserves.
Qed.

Theorem local_step_preserves_policy : forall cap before after,
  local_step cap before after -> policy after = policy before.
Proof. intros cap before after H. destruct H; reflexivity. Qed.

Theorem local_history_preserves : forall cap before after,
  local_history cap before after -> invariant cap before ->
  invariant cap after /\ policy after = policy before.
Proof.
  intros cap before after H. induction H; intros HI; [auto|].
  destruct (IHlocal_history HI) as [HM HP]. split.
  - eapply local_step_preserves; eauto.
  - rewrite (local_step_preserves_policy _ _ _ H0). exact HP.
Qed.

Theorem every_local_history_has_an_owner : forall cap policies s b,
  local_history cap (initial policies) s -> seen s b -> successor s b \/ owned s b.
Proof.
  intros cap policies s b HH HS.
  destruct (local_history_preserves _ _ _ HH (initial_invariant cap policies))
    as [[_ [_ [_ [HL _]]]] _]. exact (HL b HS).
Qed.

Theorem every_local_history_preserves_policy : forall cap policies s,
  local_history cap (initial policies) s -> policy s = policies.
Proof.
  intros cap policies s HH.
  destruct (local_history_preserves _ _ _ HH (initial_invariant cap policies))
    as [_ HP]. exact HP.
Qed.

Theorem release_without_successor_is_unsafe : forall s w b,
  unique_lease s -> seen s b -> owner s w = Some b -> ~successor s b ->
  ~live_owner (release w s).
Proof.
  intros s w b HU HS HW HN HL.
  specialize (HL b HS). destruct HL as [HP|[v HV]]; [apply HN; exact HP|].
  simpl in HV. destruct (worker_eq v w) as [E|N]; [discriminate|].
  apply N. eapply HU; eauto.
Qed.

Theorem delayed_receipt_is_unsafe : forall cap s b,
  seen s b -> ~durable s b -> ~terminal s b -> ~owned s b ->
  ~live_owner (receive cap b s).
Proof.
  intros cap s b HS HD HT HO HL.
  specialize (HL b HS). unfold successor, owned, receive in HL. simpl in HL.
  destruct HL as [[D|[T|[R N]]]|O]; auto.
Qed.

End RetryHandoff.

Print Assumptions initial_invariant.
Print Assumptions reserve_preserves.
Print Assumptions publish_preserves.
Print Assumptions receive_preserves.
Print Assumptions reopen_preserves.
Print Assumptions handoff_tracked_ready.
Print Assumptions handoff_capacity_preserves_owner.
Print Assumptions release_preserves.
Print Assumptions commit_preserves.
Print Assumptions acknowledge_preserves.
Print Assumptions resolve_preserves.
Print Assumptions restart_preserves.
Print Assumptions local_step_preserves.
Print Assumptions local_step_preserves_policy.
Print Assumptions local_history_preserves.
Print Assumptions every_local_history_has_an_owner.
Print Assumptions every_local_history_preserves_policy.
Print Assumptions release_without_successor_is_unsafe.
Print Assumptions delayed_receipt_is_unsafe.
