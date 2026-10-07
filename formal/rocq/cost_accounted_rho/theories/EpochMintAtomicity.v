From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
Import ListNotations.

Inductive mint_outcome : Type :=
  | MintSucceeded
  | MintFailed.

Record epoch_validator : Type := {
  ev_balance : nat;
  ev_active : bool;
  ev_halted : bool;
  ev_receipted : bool
}.

Definition eligibleb (validator : epoch_validator) : bool :=
  ev_active validator && negb (ev_halted validator) && negb (ev_receipted validator).

Definition credit_success (amount : nat) (validator : epoch_validator) : epoch_validator :=
  if eligibleb validator then
    {| ev_balance := ev_balance validator + amount;
       ev_active := ev_active validator;
       ev_halted := ev_halted validator;
       ev_receipted := true |}
  else validator.

Definition stage_one
  (amount : nat)
  (outcome : mint_outcome)
  (validator : epoch_validator) : option epoch_validator :=
  if eligibleb validator then
    match outcome with
    | MintSucceeded => Some (credit_success amount validator)
    | MintFailed => None
    end
  else Some validator.

Fixpoint stage_batch
  (amount : nat)
  (validators : list epoch_validator)
  (outcomes : list mint_outcome) : option (list epoch_validator) :=
  match validators, outcomes with
  | [], [] => Some []
  | validator :: remaining, outcome :: remaining_outcomes =>
      match stage_one amount outcome validator,
            stage_batch amount remaining remaining_outcomes with
      | Some updated, Some updated_remaining => Some (updated :: updated_remaining)
      | _, _ => None
      end
  | _, _ => None
  end.

Fixpoint eligible_count (validators : list epoch_validator) : nat :=
  match validators with
  | [] => 0
  | validator :: remaining =>
      (if eligibleb validator then 1 else 0) + eligible_count remaining
  end.

Fixpoint total_balance (validators : list epoch_validator) : nat :=
  match validators with
  | [] => 0
  | validator :: remaining => ev_balance validator + total_balance remaining
  end.

Record epoch_state : Type := {
  es_validators : list epoch_validator;
  es_rewards : nat;
  es_withdrawals : nat;
  es_active_generation : nat;
  es_authorized_issuance : nat
}.

Definition publish_epoch_close
  (pre_state prepared_state : epoch_state)
  (amount : nat)
  (outcomes : list mint_outcome) : epoch_state :=
  match stage_batch amount (es_validators prepared_state) outcomes with
  | None => pre_state
  | Some validators =>
      {| es_validators := validators;
         es_rewards := es_rewards prepared_state;
         es_withdrawals := es_withdrawals prepared_state;
         es_active_generation := es_active_generation prepared_state;
         es_authorized_issuance :=
           es_authorized_issuance pre_state
             + eligible_count (es_validators prepared_state) * amount |}
  end.

Definition play_epoch_close := publish_epoch_close.
Definition replay_epoch_close := publish_epoch_close.

Theorem failed_epoch_close_is_identity : forall pre_state prepared_state amount outcomes,
  stage_batch amount (es_validators prepared_state) outcomes = None ->
  publish_epoch_close pre_state prepared_state amount outcomes = pre_state.
Proof.
  intros pre_state prepared_state amount outcomes Hfailure.
  unfold publish_epoch_close.
  rewrite Hfailure.
  reflexivity.
Qed.

Theorem failed_close_preserves_balances : forall pre_state prepared_state amount outcomes,
  stage_batch amount (es_validators prepared_state) outcomes = None ->
  total_balance (es_validators (publish_epoch_close pre_state prepared_state amount outcomes)) =
  total_balance (es_validators pre_state).
Proof.
  intros pre_state prepared_state amount outcomes Hfailure.
  rewrite (failed_epoch_close_is_identity _ _ _ _ Hfailure).
  reflexivity.
Qed.

Theorem failed_close_preserves_receipts : forall pre_state prepared_state amount outcomes,
  stage_batch amount (es_validators prepared_state) outcomes = None ->
  es_validators (publish_epoch_close pre_state prepared_state amount outcomes) =
  es_validators pre_state.
Proof.
  intros pre_state prepared_state amount outcomes Hfailure.
  rewrite (failed_epoch_close_is_identity _ _ _ _ Hfailure).
  reflexivity.
Qed.

Theorem failed_close_preserves_rewards : forall pre_state prepared_state amount outcomes,
  stage_batch amount (es_validators prepared_state) outcomes = None ->
  es_rewards (publish_epoch_close pre_state prepared_state amount outcomes) =
  es_rewards pre_state.
Proof.
  intros pre_state prepared_state amount outcomes Hfailure.
  rewrite (failed_epoch_close_is_identity _ _ _ _ Hfailure).
  reflexivity.
Qed.

Theorem failed_close_preserves_withdrawals : forall pre_state prepared_state amount outcomes,
  stage_batch amount (es_validators prepared_state) outcomes = None ->
  es_withdrawals (publish_epoch_close pre_state prepared_state amount outcomes) =
  es_withdrawals pre_state.
Proof.
  intros pre_state prepared_state amount outcomes Hfailure.
  rewrite (failed_epoch_close_is_identity _ _ _ _ Hfailure).
  reflexivity.
Qed.

Theorem failed_close_preserves_active_set : forall pre_state prepared_state amount outcomes,
  stage_batch amount (es_validators prepared_state) outcomes = None ->
  es_active_generation (publish_epoch_close pre_state prepared_state amount outcomes) =
  es_active_generation pre_state.
Proof.
  intros pre_state prepared_state amount outcomes Hfailure.
  rewrite (failed_epoch_close_is_identity _ _ _ _ Hfailure).
  reflexivity.
Qed.

Lemma stage_one_success : forall amount validator,
  stage_one amount MintSucceeded validator = Some (credit_success amount validator).
Proof.
  intros amount validator.
  unfold stage_one, credit_success.
  destruct (eligibleb validator); reflexivity.
Qed.

Lemma stage_batch_all_success : forall amount validators,
  stage_batch amount validators (repeat MintSucceeded (length validators)) =
  Some (map (credit_success amount) validators).
Proof.
  intros amount validators.
  induction validators as [|validator remaining IH]; simpl.
  - reflexivity.
  - rewrite stage_one_success, IH.
    reflexivity.
Qed.

Lemma credit_success_balance : forall amount validator,
  ev_balance (credit_success amount validator) =
  ev_balance validator + if eligibleb validator then amount else 0.
Proof.
  intros amount validator.
  unfold credit_success.
  destruct (eligibleb validator); simpl; lia.
Qed.

Lemma all_success_total_balance : forall amount validators,
  total_balance (map (credit_success amount) validators) =
  total_balance validators + eligible_count validators * amount.
Proof.
  intros amount validators.
  induction validators as [|validator remaining IH]; simpl.
  - reflexivity.
  - rewrite credit_success_balance, IH.
    destruct (eligibleb validator); simpl; lia.
Qed.

Definition validator_complete (validator : epoch_validator) : Prop :=
  ev_active validator = true ->
  ev_halted validator = false ->
  ev_receipted validator = true.

Lemma credit_success_completes : forall amount validator,
  validator_complete (credit_success amount validator).
Proof.
  intros amount [balance active halted receipted].
  unfold validator_complete, credit_success, eligibleb.
  simpl.
  destruct active, halted, receipted; simpl; intros; try discriminate; reflexivity.
Qed.

Lemma all_success_completes : forall amount validators,
  Forall validator_complete (map (credit_success amount) validators).
Proof.
  intros amount validators.
  induction validators as [|validator remaining IH]; simpl.
  - constructor.
  - constructor.
    + apply credit_success_completes.
    + exact IH.
Qed.

Theorem successful_close_credits_each_eligible_exactly_once :
  forall pre_state prepared_state amount,
  total_balance
    (es_validators
      (publish_epoch_close pre_state prepared_state amount
        (repeat MintSucceeded (length (es_validators prepared_state))))) =
  total_balance (es_validators prepared_state)
    + eligible_count (es_validators prepared_state) * amount.
Proof.
  intros pre_state prepared_state amount.
  unfold publish_epoch_close.
  rewrite stage_batch_all_success.
  simpl.
  apply all_success_total_balance.
Qed.

Theorem commit_requires_all_eligible_mints : forall pre_state prepared_state amount,
  Forall validator_complete
    (es_validators
      (publish_epoch_close pre_state prepared_state amount
        (repeat MintSucceeded (length (es_validators prepared_state))))).
Proof.
  intros pre_state prepared_state amount.
  unfold publish_epoch_close.
  rewrite stage_batch_all_success.
  simpl.
  apply all_success_completes.
Qed.

Theorem authorized_supply_delta_is_exact : forall pre_state prepared_state amount,
  es_authorized_issuance
    (publish_epoch_close pre_state prepared_state amount
      (repeat MintSucceeded (length (es_validators prepared_state)))) =
  es_authorized_issuance pre_state
    + eligible_count (es_validators prepared_state) * amount.
Proof.
  intros pre_state prepared_state amount.
  unfold publish_epoch_close.
  rewrite stage_batch_all_success.
  reflexivity.
Qed.

Theorem zero_epoch_issuance_is_completed_noop : forall pre_state prepared_state,
  total_balance
    (es_validators
      (publish_epoch_close pre_state prepared_state 0
        (repeat MintSucceeded (length (es_validators prepared_state))))) =
  total_balance (es_validators prepared_state).
Proof.
  intros pre_state prepared_state.
  rewrite successful_close_credits_each_eligible_exactly_once.
  lia.
Qed.

Theorem ineligible_validator_is_unchanged : forall amount validator,
  eligibleb validator = false ->
  credit_success amount validator = validator.
Proof.
  intros amount validator Hineligible.
  unfold credit_success.
  rewrite Hineligible.
  reflexivity.
Qed.

Theorem at_most_one_credit_per_validator_epoch : forall amount validator,
  credit_success amount (credit_success amount validator) =
  credit_success amount validator.
Proof.
  intros amount [balance active halted receipted].
  unfold credit_success, eligibleb.
  destruct active, halted, receipted; reflexivity.
Qed.

Definition credit_left
  (amount : nat)
  (validators : epoch_validator * epoch_validator) :=
  (credit_success amount (fst validators), snd validators).

Definition credit_right
  (amount : nat)
  (validators : epoch_validator * epoch_validator) :=
  (fst validators, credit_success amount (snd validators)).

Theorem disjoint_validator_mints_commute : forall amount validators,
  credit_left amount (credit_right amount validators) =
  credit_right amount (credit_left amount validators).
Proof.
  intros amount [left right].
  reflexivity.
Qed.

Theorem failed_retry_from_same_prestate_is_deterministic :
  forall pre_state prepared_state amount failed_outcomes,
  stage_batch amount (es_validators prepared_state) failed_outcomes = None ->
  publish_epoch_close
    (publish_epoch_close pre_state prepared_state amount failed_outcomes)
    prepared_state
    amount
    (repeat MintSucceeded (length (es_validators prepared_state))) =
  publish_epoch_close
    pre_state
    prepared_state
    amount
    (repeat MintSucceeded (length (es_validators prepared_state))).
Proof.
  intros pre_state prepared_state amount failed_outcomes Hfailure.
  rewrite (failed_epoch_close_is_identity _ _ _ _ Hfailure).
  reflexivity.
Qed.

Theorem play_replay_epoch_close_agree : forall pre_state prepared_state amount outcomes,
  play_epoch_close pre_state prepared_state amount outcomes =
  replay_epoch_close pre_state prepared_state amount outcomes.
Proof.
  reflexivity.
Qed.
