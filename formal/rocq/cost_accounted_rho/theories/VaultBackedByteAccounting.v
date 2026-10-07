From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From Stdlib Require Import Sorting.Permutation.
From CostAccountedRho Require Import AtomicCommAccounting VaultBackedCostLifecycle.
Import ListNotations.

Inductive byte_trigger_side : Type :=
  | ByteProducerTriggered
  | ByteConsumerTriggered.

Inductive byte_event_kind : Type :=
  | IntroductionEvent (persistent : bool)
  | CommunicationEvent (trigger : byte_trigger_side).

Record byte_cost_schedule : Type := {
  introduction_rate : nat;
  transfer_rate : nat;
  trace_rate : nat
}.

Record byte_accounting_event : Type := {
  byte_event_id : nat;
  byte_event_kind_of : byte_event_kind;
  introduction_byte_count : nat;
  transfer_participant_bytes : list nat;
  committed_trace_byte_count : nat
}.

Definition byte_cost_schedule_v1 : byte_cost_schedule :=
  {| introduction_rate := 1; transfer_rate := 1; trace_rate := 1 |}.

Fixpoint sum_nat (values : list nat) : nat :=
  match values with
  | [] => 0
  | value :: rest => value + sum_nat rest
  end.

Definition comm_execution_debit (event : byte_accounting_event) : nat :=
  match byte_event_kind_of event with
  | IntroductionEvent _ => 0
  | CommunicationEvent _ => 1
  end.

Definition quantitative_byte_debit
  (schedule : byte_cost_schedule)
  (event : byte_accounting_event)
  : nat :=
  introduction_rate schedule * introduction_byte_count event +
  transfer_rate schedule * sum_nat (transfer_participant_bytes event) +
  trace_rate schedule * committed_trace_byte_count event.

Definition total_event_debit
  (schedule : byte_cost_schedule)
  (event : byte_accounting_event)
  : nat :=
  comm_execution_debit event + quantitative_byte_debit schedule event.

Definition event_well_formed (event : byte_accounting_event) : Prop :=
  match byte_event_kind_of event with
  | IntroductionEvent _ =>
      transfer_participant_bytes event = [] /\
      committed_trace_byte_count event = 0
  | CommunicationEvent _ => introduction_byte_count event = 0
  end.

Definition introduction_event
  (identity bytes : nat)
  (persistent : bool)
  : byte_accounting_event :=
  {| byte_event_id := identity;
     byte_event_kind_of := IntroductionEvent persistent;
     introduction_byte_count := bytes;
     transfer_participant_bytes := [];
     committed_trace_byte_count := 0 |}.

Definition communication_event
  (identity : nat)
  (trigger : byte_trigger_side)
  (participants : list nat)
  (trace_bytes : nat)
  : byte_accounting_event :=
  {| byte_event_id := identity;
     byte_event_kind_of := CommunicationEvent trigger;
     introduction_byte_count := 0;
     transfer_participant_bytes := participants;
     committed_trace_byte_count := trace_bytes |}.

Fixpoint trace_debit
  (schedule : byte_cost_schedule)
  (events : list byte_accounting_event)
  : nat :=
  match events with
  | [] => 0
  | event :: rest => total_event_debit schedule event + trace_debit schedule rest
  end.

Fixpoint trace_execution_debit (events : list byte_accounting_event) : nat :=
  match events with
  | [] => 0
  | event :: rest => comm_execution_debit event + trace_execution_debit rest
  end.

Fixpoint trace_quantitative_debit
  (schedule : byte_cost_schedule)
  (events : list byte_accounting_event)
  : nat :=
  match events with
  | [] => 0
  | event :: rest =>
      quantitative_byte_debit schedule event + trace_quantitative_debit schedule rest
  end.

Theorem introduction_consumes_no_comm_execution_unit : forall identity bytes persistent,
  comm_execution_debit (introduction_event identity bytes persistent) = 0.
Proof.
  reflexivity.
Qed.

Theorem communication_consumes_one_execution_unit :
  forall identity trigger participants trace_bytes,
    comm_execution_debit
      (communication_event identity trigger participants trace_bytes) = 1.
Proof.
  reflexivity.
Qed.

Theorem communication_execution_unit_is_independent_of_join_arity :
  forall identity trigger left right trace_bytes,
    comm_execution_debit
      (communication_event identity trigger left trace_bytes) =
    comm_execution_debit
      (communication_event identity trigger right trace_bytes).
Proof.
  reflexivity.
Qed.

Theorem trigger_side_does_not_change_byte_debit :
  forall schedule identity participants trace_bytes,
    quantitative_byte_debit schedule
      (communication_event identity ByteProducerTriggered participants trace_bytes) =
    quantitative_byte_debit schedule
      (communication_event identity ByteConsumerTriggered participants trace_bytes).
Proof.
  reflexivity.
Qed.

Definition producer_first_lifecycle
  (producer_identity producer_bytes consumer_identity consumer_bytes
    communication_identity : nat)
  (participants : list nat)
  (trace_bytes : nat)
  : list byte_accounting_event :=
  [introduction_event producer_identity producer_bytes false;
   introduction_event consumer_identity consumer_bytes false;
   communication_event communication_identity ByteProducerTriggered participants trace_bytes].

Definition consumer_first_lifecycle
  (producer_identity producer_bytes consumer_identity consumer_bytes
    communication_identity : nat)
  (participants : list nat)
  (trace_bytes : nat)
  : list byte_accounting_event :=
  [introduction_event consumer_identity consumer_bytes false;
   introduction_event producer_identity producer_bytes false;
   communication_event communication_identity ByteConsumerTriggered participants trace_bytes].

Theorem trigger_arrival_order_does_not_change_total_debit :
  forall schedule producer_identity producer_bytes consumer_identity consumer_bytes
    communication_identity participants trace_bytes,
    trace_debit schedule
      (producer_first_lifecycle producer_identity producer_bytes consumer_identity
        consumer_bytes communication_identity participants trace_bytes) =
    trace_debit schedule
      (consumer_first_lifecycle producer_identity producer_bytes consumer_identity
        consumer_bytes communication_identity participants trace_bytes).
Proof.
  intros.
  unfold producer_first_lifecycle, consumer_first_lifecycle, trace_debit,
    total_event_debit, comm_execution_debit, quantitative_byte_debit,
    introduction_event, communication_event.
  simpl.
  lia.
Qed.

Theorem join_transfer_includes_every_participant :
  forall schedule identity trigger participants trace_bytes,
    quantitative_byte_debit schedule
      (communication_event identity trigger participants trace_bytes) =
    transfer_rate schedule * sum_nat participants +
    trace_rate schedule * trace_bytes.
Proof.
  intros.
  unfold quantitative_byte_debit, communication_event.
  simpl.
  lia.
Qed.

Lemma sum_nat_app : forall left right,
  sum_nat (left ++ right) = sum_nat left + sum_nat right.
Proof.
  intros left right.
  induction left as [| value rest IH]; simpl; lia.
Qed.

Theorem adding_join_participant_adds_exact_transfer_cost :
  forall schedule identity trigger participants participant trace_bytes,
    quantitative_byte_debit schedule
      (communication_event identity trigger (participants ++ [participant]) trace_bytes) =
    quantitative_byte_debit schedule
      (communication_event identity trigger participants trace_bytes) +
    transfer_rate schedule * participant.
Proof.
  intros.
  repeat rewrite join_transfer_includes_every_participant.
  rewrite sum_nat_app.
  simpl.
  lia.
Qed.

Theorem v1_debit_is_canonical_encoded_footprint : forall event,
  quantitative_byte_debit byte_cost_schedule_v1 event =
    introduction_byte_count event +
    sum_nat (transfer_participant_bytes event) +
    committed_trace_byte_count event.
Proof.
  intros.
  unfold quantitative_byte_debit, byte_cost_schedule_v1.
  simpl.
  lia.
Qed.

Theorem trace_debit_is_product_sum : forall schedule events,
  trace_debit schedule events =
    trace_execution_debit events + trace_quantitative_debit schedule events.
Proof.
  intros schedule events.
  induction events as [| event rest IH]; simpl; unfold total_event_debit in *; lia.
Qed.

Lemma sum_nat_permutation : forall left right,
  Permutation left right -> sum_nat left = sum_nat right.
Proof.
  intros left right permutation.
  induction permutation; simpl; lia.
Qed.

Theorem trace_debit_permutation_invariant : forall schedule left right,
  Permutation left right -> trace_debit schedule left = trace_debit schedule right.
Proof.
  intros schedule left right permutation.
  induction permutation; simpl; lia.
Qed.

Fixpoint repeated_deliveries
  (count identity trace_bytes : nat)
  (trigger : byte_trigger_side)
  (participants : list nat)
  : list byte_accounting_event :=
  match count with
  | 0 => []
  | S rest =>
      communication_event identity trigger participants trace_bytes ::
      repeated_deliveries rest identity trace_bytes trigger participants
  end.

Definition persistent_lifecycle
  (introduction_identity introduction_bytes delivery_count delivery_identity trace_bytes : nat)
  (trigger : byte_trigger_side)
  (participants : list nat)
  : list byte_accounting_event :=
  introduction_event introduction_identity introduction_bytes true ::
  repeated_deliveries
    delivery_count delivery_identity trace_bytes trigger participants.

Lemma repeated_deliveries_exact :
  forall schedule count identity trace_bytes trigger participants,
    trace_debit schedule
      (repeated_deliveries count identity trace_bytes trigger participants) =
    count * total_event_debit schedule
      (communication_event identity trigger participants trace_bytes).
Proof.
  intros schedule count identity trace_bytes trigger participants.
  induction count as [| count IH].
  - reflexivity.
  - simpl.
    rewrite IH.
    lia.
Qed.

Theorem persistent_introduction_is_charged_once_and_each_delivery_is_charged :
  forall schedule introduction_identity introduction_bytes delivery_count
    delivery_identity trace_bytes trigger participants,
    trace_debit schedule
      (persistent_lifecycle introduction_identity introduction_bytes delivery_count
        delivery_identity trace_bytes trigger participants) =
    total_event_debit schedule
      (introduction_event introduction_identity introduction_bytes true) +
    delivery_count * total_event_debit schedule
      (communication_event delivery_identity trigger participants trace_bytes).
Proof.
  intros.
  unfold persistent_lifecycle.
  simpl.
  apply f_equal.
  apply repeated_deliveries_exact.
Qed.

Fixpoint retry_introduction_debit
  (schedule : byte_cost_schedule)
  (persistent : bool)
  (attempts : nat)
  (event : byte_accounting_event)
  : nat :=
  match attempts with
  | 0 => 0
  | S remaining =>
      total_event_debit schedule event +
      if persistent then 0
      else retry_introduction_debit schedule false remaining event
  end.

Theorem stable_persistent_identity_is_charged_once_across_retries :
  forall schedule attempts event,
    retry_introduction_debit schedule true (S attempts) event =
    total_event_debit schedule event.
Proof.
  intros.
  cbn [retry_introduction_debit].
  lia.
Qed.

Theorem nonpersistent_identity_preserves_attempt_multiplicity :
  forall schedule attempts event,
    retry_introduction_debit schedule false attempts event =
    attempts * total_event_debit schedule event.
Proof.
  intros schedule attempts event.
  induction attempts as [| attempts IH].
  - reflexivity.
  - simpl.
    rewrite IH.
    lia.
Qed.

Definition peek_debit : nat := 0.

Theorem peek_neither_charges_nor_refunds : peek_debit = 0.
Proof.
  reflexivity.
Qed.

Definition checked_event_debit
  (maximum : nat)
  (schedule : byte_cost_schedule)
  (event : byte_accounting_event)
  : option nat :=
  let debit := total_event_debit schedule event in
  if debit <=? maximum then Some debit else None.

Record byte_accounting_state : Type := {
  reserved_debit : nat;
  spent_debit : nat;
  committed_event_count : nat
}.

Definition byte_state_valid (maximum : nat) (state : byte_accounting_state) : Prop :=
  spent_debit state <= reserved_debit state /\ reserved_debit state <= maximum.

Inductive byte_attempt_result : Type :=
  | ByteAccepted (state : byte_accounting_state)
  | ByteRejected (state : byte_accounting_state).

Definition attempt_byte_event
  (maximum : nat)
  (schedule : byte_cost_schedule)
  (state : byte_accounting_state)
  (event : byte_accounting_event)
  : byte_attempt_result :=
  match checked_event_debit maximum schedule event with
  | None => ByteRejected state
  | Some debit =>
      if spent_debit state + debit <=? reserved_debit state then
        ByteAccepted
          {| reserved_debit := reserved_debit state;
             spent_debit := spent_debit state + debit;
             committed_event_count := S (committed_event_count state) |}
      else ByteRejected state
  end.

Fixpoint run_byte_trace
  (maximum : nat)
  (schedule : byte_cost_schedule)
  (state : byte_accounting_state)
  (events : list byte_accounting_event)
  : byte_attempt_result :=
  match events with
  | [] => ByteAccepted state
  | event :: rest =>
      match attempt_byte_event maximum schedule state event with
      | ByteAccepted next => run_byte_trace maximum schedule next rest
      | ByteRejected unchanged => ByteRejected unchanged
      end
  end.

Theorem accepted_byte_event_is_exact :
  forall maximum schedule state event next,
    attempt_byte_event maximum schedule state event = ByteAccepted next ->
    reserved_debit next = reserved_debit state /\
    spent_debit next = spent_debit state + total_event_debit schedule event /\
    committed_event_count next = S (committed_event_count state).
Proof.
  intros maximum schedule state event next accepted.
  unfold attempt_byte_event, checked_event_debit in accepted.
  destruct (total_event_debit schedule event <=? maximum) eqn:within_maximum;
    try discriminate.
  destruct (spent_debit state + total_event_debit schedule event <=?
    reserved_debit state) eqn:within_reservation; try discriminate.
  inversion accepted; subst; clear accepted.
  repeat split; reflexivity.
Qed.

Theorem rejected_byte_event_is_atomic :
  forall maximum schedule state event rejected,
    attempt_byte_event maximum schedule state event = ByteRejected rejected ->
    rejected = state.
Proof.
  intros maximum schedule state event rejected result.
  unfold attempt_byte_event, checked_event_debit in result.
  destruct (total_event_debit schedule event <=? maximum);
    destruct (spent_debit state + total_event_debit schedule event <=?
      reserved_debit state); try discriminate; inversion result; reflexivity.
Qed.

Theorem accepted_byte_event_preserves_hard_ceiling :
  forall maximum schedule state event next,
    byte_state_valid maximum state ->
    attempt_byte_event maximum schedule state event = ByteAccepted next ->
    byte_state_valid maximum next.
Proof.
  intros maximum schedule state event next [spent_fits reserved_fits] accepted.
  unfold attempt_byte_event, checked_event_debit in accepted.
  destruct (total_event_debit schedule event <=? maximum) eqn:within_maximum;
    try discriminate.
  destruct (spent_debit state + total_event_debit schedule event <=?
    reserved_debit state) eqn:within_reservation; try discriminate.
  apply Nat.leb_le in within_reservation.
  inversion accepted; subst; clear accepted.
  split; simpl; assumption.
Qed.

Theorem run_byte_trace_acceptance_is_exact :
  forall maximum schedule events state final,
    run_byte_trace maximum schedule state events = ByteAccepted final ->
    reserved_debit final = reserved_debit state /\
    spent_debit final = spent_debit state + trace_debit schedule events /\
    committed_event_count final = committed_event_count state + length events.
Proof.
  intros maximum schedule events.
  induction events as [| event rest IH]; intros state final accepted.
  - simpl in accepted.
    inversion accepted; subst; clear accepted.
    repeat split; simpl; lia.
  - simpl in accepted.
    destruct (attempt_byte_event maximum schedule state event) as [next|unchanged]
      eqn:attempt; try discriminate.
    pose proof (accepted_byte_event_is_exact _ _ _ _ _ attempt) as
      [reservation_exact [spent_exact count_exact]].
    pose proof (IH next final accepted) as
      [tail_reservation [tail_spent tail_count]].
    simpl.
    repeat split; lia.
Qed.

Theorem run_byte_trace_preserves_hard_ceiling :
  forall maximum schedule events state final,
    byte_state_valid maximum state ->
    run_byte_trace maximum schedule state events = ByteAccepted final ->
    byte_state_valid maximum final.
Proof.
  intros maximum schedule events.
  induction events as [| event rest IH]; intros state final valid accepted.
  - simpl in accepted.
    inversion accepted; subst; assumption.
  - simpl in accepted.
    destruct (attempt_byte_event maximum schedule state event) as [next|unchanged]
      eqn:attempt; try discriminate.
    apply IH with (state := next); try assumption.
    eapply accepted_byte_event_preserves_hard_ceiling; eauto.
Qed.

Theorem accepted_permutations_have_identical_settlement :
  forall maximum schedule left right initial left_final right_final,
    Permutation left right ->
    run_byte_trace maximum schedule initial left = ByteAccepted left_final ->
    run_byte_trace maximum schedule initial right = ByteAccepted right_final ->
    spent_debit left_final = spent_debit right_final /\
    reserved_debit left_final = reserved_debit right_final /\
    committed_event_count left_final = committed_event_count right_final.
Proof.
  intros maximum schedule left right initial left_final right_final permutation
    left_run right_run.
  pose proof (run_byte_trace_acceptance_is_exact _ _ _ _ _ left_run) as
    [left_reserved [left_spent left_count]].
  pose proof (run_byte_trace_acceptance_is_exact _ _ _ _ _ right_run) as
    [right_reserved [right_spent right_count]].
  pose proof (trace_debit_permutation_invariant schedule left right permutation).
  pose proof (Permutation_length permutation).
  repeat split; lia.
Qed.

Definition byte_trigger_side_eq_dec :
  forall left right : byte_trigger_side, {left = right} + {left <> right}.
Proof.
  decide equality.
Defined.

Definition byte_event_kind_eq_dec :
  forall left right : byte_event_kind, {left = right} + {left <> right}.
Proof.
  decide equality; try apply byte_trigger_side_eq_dec; apply Bool.bool_dec.
Defined.

Definition byte_accounting_event_eq_dec :
  forall left right : byte_accounting_event, {left = right} + {left <> right}.
Proof.
  decide equality;
    try apply Nat.eq_dec;
    try apply byte_event_kind_eq_dec.
  apply list_eq_dec, Nat.eq_dec.
Defined.

Fixpoint byte_trace_eqb
  (left right : list byte_accounting_event)
  : bool :=
  match left, right with
  | [], [] => true
  | left_event :: left_rest, right_event :: right_rest =>
      if byte_accounting_event_eq_dec left_event right_event
      then byte_trace_eqb left_rest right_rest
      else false
  | _, _ => false
  end.

Theorem byte_trace_eqb_true_iff : forall left right,
  byte_trace_eqb left right = true <-> left = right.
Proof.
  induction left as [| left_event left_rest IH]; intros right;
    destruct right as [| right_event right_rest]; simpl.
  - tauto.
  - split; discriminate.
  - split; discriminate.
  - destruct (byte_accounting_event_eq_dec left_event right_event)
      as [same_event | different_event].
    + subst right_event.
      rewrite IH.
      split.
      * now intros ->.
      * now inversion 1.
    + split.
      * discriminate.
      * inversion 1.
        contradiction.
Qed.

Definition replay_byte_trace_accepts
  (committed replayed : list byte_accounting_event)
  : bool :=
  byte_trace_eqb committed replayed.

Theorem replay_byte_trace_accepts_iff_exact : forall committed replayed,
  replay_byte_trace_accepts committed replayed = true <-> committed = replayed.
Proof.
  apply byte_trace_eqb_true_iff.
Qed.

Theorem replay_acceptance_binds_event_kind_and_amount :
  forall committed replayed,
    replay_byte_trace_accepts [committed] [replayed] = true ->
    byte_event_kind_of committed = byte_event_kind_of replayed /\
    total_event_debit byte_cost_schedule_v1 committed =
      total_event_debit byte_cost_schedule_v1 replayed.
Proof.
  intros committed replayed accepted.
  apply replay_byte_trace_accepts_iff_exact in accepted.
  inversion accepted.
  auto.
Qed.

Theorem replay_rejects_changed_event_kind :
  forall committed replayed,
    byte_event_kind_of committed <> byte_event_kind_of replayed ->
    replay_byte_trace_accepts [committed] [replayed] = false.
Proof.
  intros committed replayed different_kind.
  destruct (replay_byte_trace_accepts [committed] [replayed]) eqn:accepted.
  - exfalso.
    apply replay_byte_trace_accepts_iff_exact in accepted.
    inversion accepted.
    apply different_kind.
    exact (f_equal byte_event_kind_of H0).
  - reflexivity.
Qed.

Definition credit_vault_liquid (amount : nat) (ledger : vault_ledger) : vault_ledger :=
  {| liquid := liquid ledger + amount;
     held := held ledger;
     consumed := consumed ledger;
     fee_paid := fee_paid ledger;
     protocol_minted := protocol_minted ledger |}.

Definition top_up_vault
  (amount source : nat)
  (ledger : vault_ledger)
  : option (nat * vault_ledger) :=
  if amount <=? source then
    Some (source - amount, credit_vault_liquid amount ledger)
  else None.

Theorem top_up_is_a_conserving_transfer :
  forall amount source ledger source_after topped,
    top_up_vault amount source ledger = Some (source_after, topped) ->
    source_after + canonical_value topped = source + canonical_value ledger /\
    held topped = held ledger /\
    consumed topped = consumed ledger /\
    protocol_minted topped = protocol_minted ledger.
Proof.
  intros amount source ledger source_after topped result.
  unfold top_up_vault in result.
  destruct (amount <=? source) eqn:sufficient; try discriminate.
  apply Nat.leb_le in sufficient.
  inversion result; subst; clear result.
  unfold credit_vault_liquid, canonical_value.
  simpl.
  repeat split; lia.
Qed.

Lemma settlement_commutes_with_liquid_credit :
  forall ledger amount bound actual fee settled,
    settle bound actual fee ledger = Some settled ->
    settle bound actual fee (credit_vault_liquid amount ledger) =
      Some (credit_vault_liquid amount settled).
Proof.
  intros [ledger_liquid ledger_held ledger_consumed ledger_fee ledger_minted]
    amount bound actual fee settled result.
  unfold settle, credit_vault_liquid in *.
  simpl in *.
  destruct (bound <=? ledger_held) eqn:held_sufficient;
    destruct (actual + fee <=? bound) eqn:within_bound;
    simpl in result; try discriminate.
  inversion result; subst; clear result.
  simpl.
  f_equal.
  f_equal.
  lia.
Qed.

Theorem top_up_commutes_with_running_settlement :
  forall amount source ledger source_after topped bound actual fee settled,
    top_up_vault amount source ledger = Some (source_after, topped) ->
    settle bound actual fee ledger = Some settled ->
    exists topped_settled,
      settle bound actual fee topped = Some topped_settled /\
      top_up_vault amount source settled = Some (source_after, topped_settled).
Proof.
  intros amount source ledger source_after topped bound actual fee settled
    top_up_result settlement_result.
  pose proof
    (top_up_is_a_conserving_transfer _ _ _ _ _ top_up_result) as
    [_ [held_same [consumed_same minted_same]]].
  unfold top_up_vault in top_up_result.
  destruct (amount <=? source) eqn:sufficient; try discriminate.
  inversion top_up_result; subst; clear top_up_result.
  exists (credit_vault_liquid amount settled).
  split.
  - now apply settlement_commutes_with_liquid_credit.
  - unfold top_up_vault.
    now rewrite sufficient.
Qed.

Theorem top_up_does_not_expand_inflight_reservation :
  forall amount source ledger source_after topped,
    top_up_vault amount source ledger = Some (source_after, topped) ->
    held topped = held ledger /\
    consumed topped = consumed ledger /\
    fee_paid topped = fee_paid ledger.
Proof.
  intros amount source ledger source_after topped result.
  pose proof (top_up_is_a_conserving_transfer
    amount source ledger source_after topped result) as
    [_ [held_exact [consumed_exact _]]].
  unfold top_up_vault in result.
  destruct (amount <=? source); try discriminate.
  inversion result; subst; clear result.
  repeat split; assumption || reflexivity.
Qed.

Definition introduction_authority_registry := nat -> option nat.

Definition empty_introduction_authority_registry : introduction_authority_registry :=
  fun _ => None.

Inductive introduction_registration_result : Type :=
  | IntroductionInserted (registry : introduction_authority_registry)
  | IntroductionIdempotent (registry : introduction_authority_registry)
  | IntroductionConflict (registry : introduction_authority_registry).

Definition register_introduction_authority
  (registry : introduction_authority_registry)
  (identity payer : nat)
  : introduction_registration_result :=
  match registry identity with
  | None =>
      IntroductionInserted
        (fun candidate =>
          if Nat.eq_dec candidate identity then Some payer else registry candidate)
  | Some existing =>
      if Nat.eqb existing payer
      then IntroductionIdempotent registry
      else IntroductionConflict registry
  end.

Definition resolve_introduction_authority
  (registry : introduction_authority_registry)
  (identity deploy_payer : nat)
  : nat :=
  match registry identity with
  | Some registered => registered
  | None => deploy_payer
  end.

Definition resolve_and_pin_introduction_authority
  (registry : introduction_authority_registry)
  (identity deploy_payer : nat)
  : introduction_authority_registry * nat :=
  match registry identity with
  | Some registered => (registry, registered)
  | None =>
      (fun candidate =>
         if Nat.eq_dec candidate identity then Some deploy_payer
         else registry candidate,
       deploy_payer)
  end.

Theorem empty_registry_resolves_to_deploy_payer : forall identity deploy_payer,
  resolve_introduction_authority
    empty_introduction_authority_registry identity deploy_payer = deploy_payer.
Proof.
  reflexivity.
Qed.

Theorem fallback_resolution_is_atomically_pinned : forall identity deploy_payer,
  snd
    (resolve_and_pin_introduction_authority
      empty_introduction_authority_registry identity deploy_payer) = deploy_payer /\
  fst
    (resolve_and_pin_introduction_authority
      empty_introduction_authority_registry identity deploy_payer) identity =
      Some deploy_payer.
Proof.
  intros identity deploy_payer.
  split.
  - reflexivity.
  - simpl.
    destruct (Nat.eq_dec identity identity); congruence.
Qed.

Theorem registered_resolution_returns_the_committed_payer :
  forall registry identity registered deploy_payer,
    registry identity = Some registered ->
    resolve_and_pin_introduction_authority registry identity deploy_payer =
      (registry, registered).
Proof.
  intros registry identity registered deploy_payer committed.
  unfold resolve_and_pin_introduction_authority.
  now rewrite committed.
Qed.

Theorem fallback_pin_rejects_a_late_conflicting_registration :
  forall identity deploy_payer conflicting,
    deploy_payer <> conflicting ->
    register_introduction_authority
      (fst
        (resolve_and_pin_introduction_authority
          empty_introduction_authority_registry identity deploy_payer))
      identity conflicting =
      IntroductionConflict
        (fst
          (resolve_and_pin_introduction_authority
            empty_introduction_authority_registry identity deploy_payer)).
Proof.
  intros identity deploy_payer conflicting different.
  unfold resolve_and_pin_introduction_authority.
  simpl.
  unfold register_introduction_authority.
  destruct (Nat.eq_dec identity identity) as [_ | impossible].
  - apply Nat.eqb_neq in different.
    now rewrite different.
  - contradiction.
Qed.

Theorem inserted_introduction_authority_is_stable :
  forall registry identity payer next,
    register_introduction_authority registry identity payer =
      IntroductionInserted next ->
    forall deploy_payer,
      resolve_introduction_authority next identity deploy_payer = payer.
Proof.
  intros registry identity payer next inserted deploy_payer.
  unfold register_introduction_authority in inserted.
  destruct (registry identity) as [existing |] eqn:current.
  - destruct (existing =? payer); discriminate.
  - inversion inserted; subst; clear inserted.
    unfold resolve_introduction_authority.
    destruct (Nat.eq_dec identity identity) as [_ | impossible].
    + reflexivity.
    + contradiction.
Qed.

Theorem same_payer_registration_is_idempotent :
  forall registry identity payer,
    registry identity = Some payer ->
    register_introduction_authority registry identity payer =
      IntroductionIdempotent registry.
Proof.
  intros registry identity payer registered.
  unfold register_introduction_authority.
  rewrite registered, Nat.eqb_refl.
  reflexivity.
Qed.

Theorem conflicting_registration_is_rejected_without_overwrite :
  forall registry identity existing conflicting,
    existing <> conflicting ->
    registry identity = Some existing ->
    register_introduction_authority registry identity conflicting =
      IntroductionConflict registry /\
    resolve_introduction_authority registry identity conflicting = existing.
Proof.
  intros registry identity existing conflicting different registered.
  split.
  - unfold register_introduction_authority.
    rewrite registered.
    apply Nat.eqb_neq in different.
    now rewrite different.
  - unfold resolve_introduction_authority.
    now rewrite registered.
Qed.

Theorem deploy_reset_discards_every_registered_introduction_authority :
  forall (registry : introduction_authority_registry)
    (identity first_payer next_payer : nat),
    registry identity = Some first_payer ->
    resolve_introduction_authority
      empty_introduction_authority_registry identity next_payer = next_payer.
Proof.
  intros.
  apply empty_registry_resolves_to_deploy_payer.
Qed.

Record introduction_attribution : Type := {
  introduction_sponsor : nat;
  stored_interaction_authority : option nat
}.

Definition introduction_draw
  (attribution : introduction_attribution)
  (selected amount : nat)
  : nat :=
  if Nat.eq_dec (introduction_sponsor attribution) selected then amount else 0.

Theorem authority_neutral_stack_keeps_storage_neutral_and_charges_sponsor :
  forall sponsor amount,
    stored_interaction_authority
      {| introduction_sponsor := sponsor;
         stored_interaction_authority := None |} = None /\
    introduction_draw
      {| introduction_sponsor := sponsor;
         stored_interaction_authority := None |}
      sponsor amount = amount.
Proof.
  intros sponsor amount.
  split.
  - reflexivity.
  - unfold introduction_draw.
    simpl.
    destruct (Nat.eq_dec sponsor sponsor) as [_ | impossible].
    + reflexivity.
    + contradiction.
Qed.

Theorem lollipop_receiver_introduction_charges_outer_not_continuation :
  forall outer continuation amount stored,
    outer <> continuation ->
    introduction_draw
      {| introduction_sponsor := outer;
         stored_interaction_authority := stored |}
      outer amount = amount /\
    introduction_draw
      {| introduction_sponsor := outer;
         stored_interaction_authority := stored |}
      continuation amount = 0.
Proof.
  intros outer continuation amount stored different.
  split; unfold introduction_draw; simpl.
  - destruct (Nat.eq_dec outer outer) as [_ | impossible].
    + reflexivity.
    + contradiction.
  - destruct (Nat.eq_dec outer continuation) as [same | distinct].
    + contradiction.
    + reflexivity.
Qed.

Theorem continuation_created_introduction_charges_continuation :
  forall continuation amount stored,
    introduction_draw
      {| introduction_sponsor := continuation;
         stored_interaction_authority := stored |}
      continuation amount = amount.
Proof.
  intros continuation amount stored.
  unfold introduction_draw.
  simpl.
  destruct (Nat.eq_dec continuation continuation) as [_ | impossible].
  - reflexivity.
  - contradiction.
Qed.

Theorem stored_interaction_authority_cannot_redirect_introduction_charge :
  forall sponsor left_stored right_stored selected amount,
    introduction_draw
      {| introduction_sponsor := sponsor;
         stored_interaction_authority := left_stored |}
      selected amount =
    introduction_draw
      {| introduction_sponsor := sponsor;
         stored_interaction_authority := right_stored |}
      selected amount.
Proof.
  reflexivity.
Qed.

Definition scoped_introduction_draw
  (metered : bool)
  (attribution : introduction_attribution)
  (selected amount : nat)
  : nat :=
  if metered then introduction_draw attribution selected amount else 0.

Theorem unmetered_introduction_has_no_authority_debit :
  forall attribution selected amount,
    scoped_introduction_draw false attribution selected amount = 0.
Proof.
  reflexivity.
Qed.

Definition authenticated_byte_admission
  (prestate_balance candidate_created_supply certified_bound : nat)
  : bool :=
  certified_bound <=? prestate_balance.

Theorem candidate_created_stack_cannot_supply_prestate_byte_capacity :
  forall prestate_balance candidate_created_supply certified_bound,
    prestate_balance < certified_bound ->
    authenticated_byte_admission
      prestate_balance candidate_created_supply certified_bound = false.
Proof.
  intros prestate_balance candidate_created_supply certified_bound insufficient.
  unfold authenticated_byte_admission.
  apply Nat.leb_gt.
  exact insufficient.
Qed.

Section LocatedByteSettlement.

Context {purse : Type}.
Context (purse_eq_dec : forall left right : purse, {left = right} + {left <> right}).

Definition purse_vector := purse -> nat.

Fixpoint purse_multiplicity
  (selected : purse)
  (components : list purse)
  : nat :=
  match components with
  | [] => 0
  | current :: rest =>
      (if purse_eq_dec current selected then 1 else 0) +
      purse_multiplicity selected rest
  end.

Record located_byte_event : Type := {
  located_event_id : nat;
  located_event_kind : byte_event_kind;
  located_event_amount : nat;
  located_event_components : list purse
}.

Definition located_event_draw
  (event : located_byte_event)
  (selected : purse)
  : nat :=
  located_event_amount event *
    purse_multiplicity selected (located_event_components event).

Fixpoint located_trace_draw
  (events : list located_byte_event)
  (selected : purse)
  : nat :=
  match events with
  | [] => 0
  | event :: rest =>
      located_event_draw event selected + located_trace_draw rest selected
  end.

Record located_byte_state : Type := {
  located_liquid : purse_vector;
  located_reserved : purse_vector;
  located_spent : purse_vector;
  located_burned : purse_vector;
  located_committed_count : nat
}.

Definition located_event_fits
  (state : located_byte_state)
  (event : located_byte_event)
  : Prop :=
  forall selected,
    located_spent state selected + located_event_draw event selected <=
    located_reserved state selected.

Inductive located_attempt_result : Type :=
  | LocatedAccepted (state : located_byte_state)
  | LocatedRejected (state : located_byte_state).

Definition attempt_located_byte_event
  (state : located_byte_state)
  (event : located_byte_event)
  (decision : {located_event_fits state event} + {~ located_event_fits state event})
  : located_attempt_result :=
  if decision then
    LocatedAccepted
      {| located_liquid := located_liquid state;
         located_reserved := located_reserved state;
         located_spent := fun selected =>
           located_spent state selected + located_event_draw event selected;
         located_burned := located_burned state;
         located_committed_count := S (located_committed_count state) |}
  else LocatedRejected state.

Theorem rejected_located_byte_event_is_atomic :
  forall state event decision,
    ~ located_event_fits state event ->
    attempt_located_byte_event state event decision = LocatedRejected state.
Proof.
  intros state event decision rejected.
  unfold attempt_located_byte_event.
  destruct decision; contradiction || reflexivity.
Qed.

Theorem accepted_located_byte_event_is_component_exact :
  forall state event decision next,
    attempt_located_byte_event state event decision = LocatedAccepted next ->
    forall selected,
      located_reserved next selected = located_reserved state selected /\
      located_spent next selected =
        located_spent state selected + located_event_draw event selected /\
      located_liquid next selected = located_liquid state selected /\
      located_burned next selected = located_burned state selected.
Proof.
  intros state event decision next accepted selected.
  unfold attempt_located_byte_event in accepted.
  destruct decision; try discriminate.
  inversion accepted; subst; clear accepted.
  repeat split; reflexivity.
Qed.

Theorem located_event_does_not_debit_an_unselected_purse :
  forall event selected,
    ~ In selected (located_event_components event) ->
    located_event_draw event selected = 0.
Proof.
  intros event selected absent.
  unfold located_event_draw.
  assert (purse_multiplicity selected (located_event_components event) = 0) as zero.
  {
    induction (located_event_components event) as [| current rest IH].
    - reflexivity.
    - simpl.
      destruct (purse_eq_dec current selected) as [same | different].
      + subst current.
        exfalso.
        apply absent.
        now left.
      + rewrite IH.
        * reflexivity.
        * intros present.
          apply absent.
          now right.
  }
  rewrite zero.
  lia.
Qed.

Theorem located_event_debits_a_single_purse_exactly :
  forall identity kind amount selected,
    located_event_draw
      {| located_event_id := identity;
         located_event_kind := kind;
         located_event_amount := amount;
         located_event_components := [selected] |}
      selected = amount.
Proof.
  intros.
  unfold located_event_draw.
  simpl.
  destruct (purse_eq_dec selected selected) as [_ | impossible].
  - lia.
  - contradiction.
Qed.

Theorem compound_authority_debits_every_component :
  forall identity kind amount left right,
    left <> right ->
    let event :=
      {| located_event_id := identity;
         located_event_kind := kind;
         located_event_amount := amount;
         located_event_components := [left; right] |} in
    located_event_draw event left = amount /\
    located_event_draw event right = amount /\
    located_event_draw event left + located_event_draw event right = amount + amount.
Proof.
  intros identity kind amount left right different event.
  subst event.
  unfold located_event_draw.
  cbn [purse_multiplicity].
  remember (purse_eq_dec left left) as left_left eqn:left_left_eq.
  remember (purse_eq_dec right left) as right_left eqn:right_left_eq.
  remember (purse_eq_dec left right) as left_right eqn:left_right_eq.
  remember (purse_eq_dec right right) as right_right eqn:right_right_eq.
  destruct left_left as [left_reflexive | left_irreflexive].
  2: contradiction.
  destruct right_left as [right_is_left | right_not_left].
  - exfalso.
    apply different.
    symmetry.
    exact right_is_left.
  - destruct left_right as [left_is_right | left_not_right].
    + contradiction.
    + destruct right_right as [right_reflexive | right_irreflexive].
      2: contradiction.
      change
        (amount *
           ((if purse_eq_dec left left then 1 else 0) +
            ((if purse_eq_dec right left then 1 else 0) + 0)) = amount /\
         amount *
           ((if purse_eq_dec left right then 1 else 0) +
            ((if purse_eq_dec right right then 1 else 0) + 0)) = amount /\
         amount *
           ((if purse_eq_dec left left then 1 else 0) +
            ((if purse_eq_dec right left then 1 else 0) + 0)) +
         amount *
           ((if purse_eq_dec left right then 1 else 0) +
            ((if purse_eq_dec right right then 1 else 0) + 0)) = amount + amount).
      rewrite <- left_left_eq, <- right_left_eq, <- left_right_eq, <- right_right_eq.
      simpl.
      repeat split; nia.
Qed.

Theorem outer_surplus_cannot_fund_an_underfunded_continuation :
  forall state identity kind amount continuation,
    located_reserved state continuation <
      located_spent state continuation + amount ->
    ~ located_event_fits state
      {| located_event_id := identity;
         located_event_kind := kind;
         located_event_amount := amount;
         located_event_components := [continuation] |}.
Proof.
  intros state identity kind amount continuation underfunded funded.
  specialize (funded continuation).
  rewrite located_event_debits_a_single_purse_exactly in funded.
  lia.
Qed.

Theorem lollipop_byte_trace_preserves_local_ownership :
  forall outer continuation outer_id continuation_id outer_kind continuation_kind
    outer_amount continuation_amount,
    outer <> continuation ->
    let events :=
      [{| located_event_id := outer_id;
          located_event_kind := outer_kind;
          located_event_amount := outer_amount;
          located_event_components := [outer] |};
       {| located_event_id := continuation_id;
          located_event_kind := continuation_kind;
          located_event_amount := continuation_amount;
          located_event_components := [continuation] |}] in
    located_trace_draw events outer = outer_amount /\
    located_trace_draw events continuation = continuation_amount.
Proof.
  intros outer continuation outer_id continuation_id outer_kind continuation_kind
    outer_amount continuation_amount different events.
  subst events.
  cbn [located_trace_draw].
  unfold located_event_draw.
  cbn [purse_multiplicity].
  remember (purse_eq_dec outer outer) as outer_outer eqn:outer_outer_eq.
  remember (purse_eq_dec continuation outer) as continuation_outer
    eqn:continuation_outer_eq.
  remember (purse_eq_dec outer continuation) as outer_continuation
    eqn:outer_continuation_eq.
  remember (purse_eq_dec continuation continuation) as continuation_continuation
    eqn:continuation_continuation_eq.
  destruct outer_outer as [outer_reflexive | outer_irreflexive].
  2: contradiction.
  destruct continuation_outer as [continuation_is_outer | continuation_not_outer].
  - exfalso.
    apply different.
    symmetry.
    exact continuation_is_outer.
  - destruct outer_continuation as [outer_is_continuation | outer_not_continuation].
    + contradiction.
    + destruct continuation_continuation as
        [continuation_reflexive | continuation_irreflexive].
      2: contradiction.
      change
        (outer_amount *
           ((if purse_eq_dec outer outer then 1 else 0) + 0) +
         (continuation_amount *
           ((if purse_eq_dec continuation outer then 1 else 0) + 0) + 0) =
           outer_amount /\
         outer_amount *
           ((if purse_eq_dec outer continuation then 1 else 0) + 0) +
         (continuation_amount *
           ((if purse_eq_dec continuation continuation then 1 else 0) + 0) + 0) =
           continuation_amount).
      rewrite <- outer_outer_eq, <- continuation_outer_eq,
        <- outer_continuation_eq, <- continuation_continuation_eq.
      simpl.
      split; nia.
Qed.

Theorem located_trace_draw_permutation_invariant :
  forall left right,
    Permutation left right ->
    forall selected,
      located_trace_draw left selected = located_trace_draw right selected.
Proof.
  intros left right permutation.
  induction permutation; intros selected; simpl; try lia.
  - now rewrite IHpermutation.
  - etransitivity; eauto.
Qed.

Definition credit_located_liquid
  (target : purse)
  (amount : nat)
  (state : located_byte_state)
  : located_byte_state :=
  {| located_liquid := fun selected =>
       if purse_eq_dec target selected
       then located_liquid state selected + amount
       else located_liquid state selected;
     located_reserved := located_reserved state;
     located_spent := located_spent state;
     located_burned := located_burned state;
     located_committed_count := located_committed_count state |}.

Theorem located_top_up_preserves_inflight_reservation_and_spend :
  forall target amount state selected,
    located_reserved (credit_located_liquid target amount state) selected =
      located_reserved state selected /\
    located_spent (credit_located_liquid target amount state) selected =
      located_spent state selected /\
    located_burned (credit_located_liquid target amount state) selected =
      located_burned state selected.
Proof.
  intros.
  repeat split; reflexivity.
Qed.

Theorem located_top_up_cannot_change_inflight_acceptance :
  forall target amount state event,
    located_event_fits (credit_located_liquid target amount state) event <->
    located_event_fits state event.
Proof.
  intros.
  unfold located_event_fits, credit_located_liquid.
  simpl.
  tauto.
Qed.

Record located_settlement : Type := {
  settled_liquid : purse_vector;
  settled_burned : purse_vector
}.

Definition settle_located_bytes
  (state : located_byte_state)
  : located_settlement :=
  {| settled_liquid := fun selected =>
       located_liquid state selected +
       (located_reserved state selected - located_spent state selected);
     settled_burned := fun selected =>
       located_burned state selected + located_spent state selected |}.

Theorem located_settlement_conserves_each_purse :
  forall state selected,
    located_spent state selected <= located_reserved state selected ->
    settled_liquid (settle_located_bytes state) selected +
      settled_burned (settle_located_bytes state) selected =
    located_liquid state selected + located_burned state selected +
      located_reserved state selected.
Proof.
  intros state selected sufficient.
  unfold settle_located_bytes.
  simpl.
  lia.
Qed.

End LocatedByteSettlement.

Theorem single_cell_authority_settlement_matches_processed_trace :
  forall schedule events physical_authority_debit,
    physical_authority_debit = trace_execution_debit events ->
    physical_authority_debit + trace_quantitative_debit schedule events =
      trace_debit schedule events.
Proof.
  intros schedule events physical_authority_debit physical_is_execution.
  subst physical_authority_debit.
  symmetry.
  apply trace_debit_is_product_sum.
Qed.

Theorem byte_trace_settlement_conserves_rev :
  forall ledger maximum schedule events bound final physical_authority_debit settled,
    run_byte_trace maximum schedule
      {| reserved_debit := bound;
         spent_debit := 0;
         committed_event_count := 0 |}
      events = ByteAccepted final ->
    settle bound
      (physical_authority_debit + trace_quantitative_debit schedule events)
      0 ledger = Some settled ->
    canonical_value settled = canonical_value ledger.
Proof.
  intros ledger maximum schedule events bound final physical_authority_debit settled
    run settlement.
  pose proof
    (settlement_is_conserving_and_refunds_exactly
      ledger bound
      (physical_authority_debit + trace_quantitative_debit schedule events)
      0 settled settlement) as
    [conserves _].
  exact conserves.
Qed.

Theorem byte_trace_refines_single_comm_execution :
  forall schedule identity trigger participants trace_bytes,
    comm_execution_debit
      (communication_event identity trigger participants trace_bytes) =
      comm_charge
        {| comm_identity := identity;
           comm_arity := length participants;
           comm_trigger := ProducerTriggered |} /\
    total_event_debit schedule
      (communication_event identity trigger participants trace_bytes) =
      1 + quantitative_byte_debit schedule
        (communication_event identity trigger participants trace_bytes).
Proof.
  intros.
  split; reflexivity.
Qed.

Print Assumptions introduction_consumes_no_comm_execution_unit.
Print Assumptions communication_consumes_one_execution_unit.
Print Assumptions communication_execution_unit_is_independent_of_join_arity.
Print Assumptions trigger_side_does_not_change_byte_debit.
Print Assumptions trigger_arrival_order_does_not_change_total_debit.
Print Assumptions join_transfer_includes_every_participant.
Print Assumptions adding_join_participant_adds_exact_transfer_cost.
Print Assumptions v1_debit_is_canonical_encoded_footprint.
Print Assumptions trace_debit_is_product_sum.
Print Assumptions trace_debit_permutation_invariant.
Print Assumptions persistent_introduction_is_charged_once_and_each_delivery_is_charged.
Print Assumptions stable_persistent_identity_is_charged_once_across_retries.
Print Assumptions nonpersistent_identity_preserves_attempt_multiplicity.
Print Assumptions peek_neither_charges_nor_refunds.
Print Assumptions rejected_byte_event_is_atomic.
Print Assumptions run_byte_trace_acceptance_is_exact.
Print Assumptions run_byte_trace_preserves_hard_ceiling.
Print Assumptions accepted_permutations_have_identical_settlement.
Print Assumptions byte_trace_eqb_true_iff.
Print Assumptions replay_byte_trace_accepts_iff_exact.
Print Assumptions replay_acceptance_binds_event_kind_and_amount.
Print Assumptions replay_rejects_changed_event_kind.
Print Assumptions top_up_is_a_conserving_transfer.
Print Assumptions top_up_commutes_with_running_settlement.
Print Assumptions top_up_does_not_expand_inflight_reservation.
Print Assumptions empty_registry_resolves_to_deploy_payer.
Print Assumptions fallback_resolution_is_atomically_pinned.
Print Assumptions registered_resolution_returns_the_committed_payer.
Print Assumptions fallback_pin_rejects_a_late_conflicting_registration.
Print Assumptions inserted_introduction_authority_is_stable.
Print Assumptions same_payer_registration_is_idempotent.
Print Assumptions conflicting_registration_is_rejected_without_overwrite.
Print Assumptions deploy_reset_discards_every_registered_introduction_authority.
Print Assumptions authority_neutral_stack_keeps_storage_neutral_and_charges_sponsor.
Print Assumptions lollipop_receiver_introduction_charges_outer_not_continuation.
Print Assumptions continuation_created_introduction_charges_continuation.
Print Assumptions stored_interaction_authority_cannot_redirect_introduction_charge.
Print Assumptions unmetered_introduction_has_no_authority_debit.
Print Assumptions candidate_created_stack_cannot_supply_prestate_byte_capacity.
Print Assumptions rejected_located_byte_event_is_atomic.
Print Assumptions accepted_located_byte_event_is_component_exact.
Print Assumptions located_event_does_not_debit_an_unselected_purse.
Print Assumptions located_event_debits_a_single_purse_exactly.
Print Assumptions compound_authority_debits_every_component.
Print Assumptions outer_surplus_cannot_fund_an_underfunded_continuation.
Print Assumptions lollipop_byte_trace_preserves_local_ownership.
Print Assumptions located_trace_draw_permutation_invariant.
Print Assumptions located_top_up_preserves_inflight_reservation_and_spend.
Print Assumptions located_top_up_cannot_change_inflight_acceptance.
Print Assumptions located_settlement_conserves_each_purse.
Print Assumptions single_cell_authority_settlement_matches_processed_trace.
Print Assumptions byte_trace_settlement_conserves_rev.
Print Assumptions byte_trace_refines_single_comm_execution.
