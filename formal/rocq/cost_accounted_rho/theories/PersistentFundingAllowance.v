From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
Import ListNotations.

Fixpoint reserved_total (slots : list (option nat)) : nat :=
  match slots with
  | [] => 0
  | None :: rest => reserved_total rest
  | Some amount :: rest => amount + reserved_total rest
  end.

Fixpoint close_slot (index : nat) (slots : list (option nat))
  : option (nat * list (option nat)) :=
  match index, slots with
  | 0, Some amount :: rest => Some (amount, None :: rest)
  | S index', slot :: rest =>
      match close_slot index' rest with
      | Some (amount, next) => Some (amount, slot :: next)
      | None => None
      end
  | _, _ => None
  end.

Record allowance := {
  issued : nat;
  available : nat;
  consumed : nat;
  active_slots : list (option nat);
  authorization_version : nat
}.

Definition conserved (state : allowance) : Prop :=
  available state + reserved_total (active_slots state) + consumed state =
  issued state.

Definition initial_allowance (amount : nat) : allowance :=
  {| issued := amount; available := amount; consumed := 0;
     active_slots := []; authorization_version := 0 |}.

Inductive allowance_command :=
| Reserve (expected amount : nat) (authorized : bool)
| Settle (index debit : nat)
| Abort (index : nat)
| Transfer (expected : nat) (authorized : bool)
| Expand (expected amount : nat) (authorized : bool)
| WalletTopUp (amount : nat).

Definition settle_allowance (state : allowance) (index debit : nat)
  : option allowance :=
  match close_slot index (active_slots state) with
  | None => None
  | Some (reserved, slots) =>
      if debit <=? reserved then
        Some {| issued := issued state;
                available := available state + (reserved - debit);
                consumed := consumed state + debit;
                active_slots := slots;
                authorization_version := authorization_version state |}
      else None
  end.

Definition allowance_step (state : allowance) (command : allowance_command)
  : option allowance :=
  match command with
  | Reserve expected amount authorized =>
      if authorized && (expected =? authorization_version state) &&
         (amount <=? available state) then
        Some {| issued := issued state;
                available := available state - amount;
                consumed := consumed state;
                active_slots := active_slots state ++ [Some amount];
                authorization_version := authorization_version state |}
      else None
  | Settle index debit => settle_allowance state index debit
  | Abort index => settle_allowance state index 0
  | Transfer expected authorized =>
      if authorized && (expected =? authorization_version state) then
        Some {| issued := issued state;
                available := available state;
                consumed := consumed state;
                active_slots := active_slots state;
                authorization_version := S (authorization_version state) |}
      else None
  | Expand expected amount authorized =>
      if authorized && (expected =? authorization_version state) then
        Some {| issued := issued state + amount;
                available := available state + amount;
                consumed := consumed state;
                active_slots := active_slots state;
                authorization_version := S (authorization_version state) |}
      else None
  | WalletTopUp _ => Some state
  end.

Fixpoint allowance_history (state : allowance) (commands : list allowance_command)
  : option allowance :=
  match commands with
  | [] => Some state
  | command :: rest =>
      match allowance_step state command with
      | Some next => allowance_history next rest
      | None => None
      end
  end.

Lemma reserved_total_append : forall left right,
  reserved_total (left ++ right) = reserved_total left + reserved_total right.
Proof.
  induction left as [|[amount|] rest IH]; intros; simpl; try rewrite IH; lia.
Qed.

Lemma close_slot_conserves : forall slots index amount next,
  close_slot index slots = Some (amount, next) ->
  reserved_total slots = amount + reserved_total next.
Proof.
  induction slots as [|slot rest IH]; intros index amount next H;
    destruct index; simpl in H; try discriminate.
  - destruct slot; inversion H; subst; simpl; reflexivity.
  - destruct (close_slot index rest) as [[found tail]|] eqn:E; try discriminate.
    inversion H; subst. specialize (IH _ _ _ E).
    destruct slot; simpl; lia.
Qed.

Lemma close_slot_stays_closed : forall slots index amount next,
  close_slot index slots = Some (amount, next) ->
  close_slot index next = None.
Proof.
  induction slots as [|slot rest IH]; intros index amount next H;
    destruct index; simpl in H; try discriminate.
  - destruct slot; inversion H; subst; reflexivity.
  - destruct (close_slot index rest) as [[found tail]|] eqn:E; try discriminate.
    inversion H; subst. simpl. rewrite (IH _ _ _ E). reflexivity.
Qed.

Lemma settle_allowance_conserves : forall state index debit next,
  conserved state -> settle_allowance state index debit = Some next ->
  conserved next.
Proof.
  intros state index debit next Hvalid H.
  unfold settle_allowance in H.
  destruct (close_slot index (active_slots state)) as [[reserved slots]|] eqn:E;
    try discriminate.
  destruct (debit <=? reserved) eqn:B; try discriminate.
  apply Nat.leb_le in B. apply close_slot_conserves in E.
  inversion H; subst. unfold conserved in *; simpl; lia.
Qed.

Theorem allowance_step_conserves : forall state command next,
  conserved state -> allowance_step state command = Some next -> conserved next.
Proof.
  intros state command next Hvalid H. destruct command; simpl in H.
  - destruct (authorized && (expected =? authorization_version state) &&
      (amount <=? available state)) eqn:B; try discriminate.
    apply andb_true_iff in B as [_ B]. apply Nat.leb_le in B.
    inversion H; subst. unfold conserved in *; simpl.
    rewrite reserved_total_append; simpl; lia.
  - eapply settle_allowance_conserves; eauto.
  - eapply settle_allowance_conserves; eauto.
  - destruct (authorized && (expected =? authorization_version state));
      inversion H; subst; exact Hvalid.
  - destruct (authorized && (expected =? authorization_version state));
      inversion H; subst. unfold conserved in *; simpl; lia.
  - inversion H; subst; exact Hvalid.
Qed.

Theorem arbitrary_mixed_history_conserves : forall commands state next,
  conserved state -> allowance_history state commands = Some next ->
  conserved next.
Proof.
  induction commands as [|command rest IH]; intros state next Hvalid H; simpl in H.
  - inversion H; subst; exact Hvalid.
  - destruct (allowance_step state command) as [middle|] eqn:E; try discriminate.
    apply (IH middle next).
    + exact (allowance_step_conserves state command middle Hvalid E).
    + exact H.
Qed.

Theorem allowance_step_never_resets_consumption : forall state command next,
  allowance_step state command = Some next -> consumed state <= consumed next.
Proof.
  intros state command next H. destruct command; simpl in H.
  - destruct (authorized && (expected =? authorization_version state) &&
      (amount <=? available state)); inversion H; subst; simpl; lia.
  - unfold settle_allowance in H.
    destruct (close_slot index (active_slots state)) as [[reserved slots]|];
      try discriminate.
    destruct (debit <=? reserved); inversion H; subst; simpl; lia.
  - unfold settle_allowance in H.
    destruct (close_slot index (active_slots state)) as [[reserved slots]|];
      inversion H; subst; simpl; lia.
  - destruct (authorized && (expected =? authorization_version state));
      inversion H; subst; simpl; lia.
  - destruct (authorized && (expected =? authorization_version state));
      inversion H; subst; simpl; lia.
  - inversion H; subst; lia.
Qed.

Theorem arbitrary_mixed_history_never_resets_consumption : forall commands state next,
  allowance_history state commands = Some next -> consumed state <= consumed next.
Proof.
  induction commands as [|command rest IH]; intros state next H; simpl in H.
  - inversion H; subst; lia.
  - destruct (allowance_step state command) as [middle|] eqn:E; try discriminate.
    specialize (IH _ _ H).
    pose proof (allowance_step_never_resets_consumption _ _ _ E). lia.
Qed.

Theorem settlement_cannot_repeat : forall state index debit next retry,
  settle_allowance state index debit = Some next ->
  settle_allowance next index retry = None.
Proof.
  intros state index debit next retry H. unfold settle_allowance in H.
  destruct (close_slot index (active_slots state)) as [[reserved slots]|] eqn:E;
    try discriminate.
  destruct (debit <=? reserved); inversion H; subst.
  unfold settle_allowance; simpl. rewrite (close_slot_stays_closed _ _ _ _ E).
  reflexivity.
Qed.

Theorem transfer_preserves_allowance_stocks : forall state expected authorized next,
  allowance_step state (Transfer expected authorized) = Some next ->
  issued next = issued state /\ available next = available state /\
  consumed next = consumed state /\ active_slots next = active_slots state /\
  authorization_version next = S (authorization_version state).
Proof.
  intros state expected authorized next H. simpl in H.
  destruct (authorized && (expected =? authorization_version state));
    inversion H; subst; repeat split; reflexivity.
Qed.

Theorem top_up_does_not_expand_allowance : forall state amount,
  allowance_step state (WalletTopUp amount) = Some state.
Proof. reflexivity. Qed.

Example transfer_and_top_up_preserve_remaining_allowance :
  match allowance_history (initial_allowance 1000)
    [Reserve 0 100 true; Settle 0 40; Transfer 0 true;
     WalletTopUp 500; Reserve 1 200 true; Abort 1] with
  | Some state => (issued state, available state, consumed state,
                   reserved_total (active_slots state), authorization_version state)
  | None => (0, 0, 0, 0, 0)
  end = (1000, 960, 40, 0, 1).
Proof. reflexivity. Qed.

Example stale_transfer_cannot_reserve :
  allowance_history (initial_allowance 10)
    [Transfer 0 true; Reserve 0 1 true] = None.
Proof. reflexivity. Qed.

Example competing_reservations_cannot_overdraw :
  allowance_history (initial_allowance 10)
    [Reserve 0 6 true; Reserve 0 6 true] = None.
Proof. reflexivity. Qed.

Example insufficient_allowance_rejects_despite_top_up :
  allowance_history (initial_allowance 10)
    [WalletTopUp 100; Reserve 0 11 true] = None.
Proof. reflexivity. Qed.

Definition allowance_observation_valid (state : allowance) : bool :=
  available state + reserved_total (active_slots state) + consumed state =?
  issued state.

Definition issued_change_valid (before after : allowance)
  (command : allowance_command) : bool :=
  match command with
  | Expand _ amount _ => issued after =? issued before + amount
  | _ => issued after =? issued before
  end.

Definition generation_change_valid (before after : allowance)
  (command : allowance_command) : bool :=
  match command with
  | Transfer _ _ | Expand _ _ _ =>
      authorization_version after =? S (authorization_version before)
  | _ => authorization_version after =? authorization_version before
  end.

Definition generated_commands : list allowance_command :=
  [Reserve 0 0 true; Reserve 0 1 true; Reserve 0 3 true;
   Reserve 1 1 true; Reserve 0 1 false;
   Settle 0 0; Settle 0 1; Settle 0 3; Settle 1 1;
   Abort 0; Abort 1;
   Transfer 0 true; Transfer 1 true; Transfer 0 false;
   Expand 0 1 true; Expand 1 1 true; Expand 0 1 false;
   WalletTopUp 0; WalletTopUp 100].

Fixpoint check_generated_histories (depth : nat) (state : allowance) : bool :=
  allowance_observation_valid state &&
  match depth with
  | 0 => true
  | S rest => forallb (fun command =>
      match allowance_step state command with
      | None => check_generated_histories rest state
      | Some next =>
          (consumed state <=? consumed next) &&
          issued_change_valid state next command &&
          generation_change_valid state next command &&
          check_generated_histories rest next
      end) generated_commands
  end.

Example generated_zero_allowance_histories :
  check_generated_histories 4 (initial_allowance 0) = true.
Proof. vm_compute. reflexivity. Qed.

Example generated_funded_allowance_histories :
  check_generated_histories 4 (initial_allowance 2) = true.
Proof. vm_compute. reflexivity. Qed.

Example closed_slot_stays_unavailable_after_new_reservation :
  allowance_history (initial_allowance 10)
    [Reserve 0 2 true; Settle 0 1; Reserve 0 2 true; Settle 0 1] = None.
Proof. reflexivity. Qed.
