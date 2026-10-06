From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
From Stdlib Require Import ZArith.
Import ListNotations.
Open Scope Z_scope.

Inductive validator_economic_phase : Type :=
  | EconomicAbsent
  | EconomicBonded
  | EconomicActive
  | EconomicWithdrawing
  | EconomicQuarantined
  | EconomicBurned.

Scheme Equality for validator_economic_phase.

Record validator_economics : Type := {
  ve_general : Z;
  ve_fuel : Z;
  ve_pos : Z;
  ve_fuel_quarantine : Z;
  ve_cooperative : Z;
  ve_located : Z;
  ve_burned : Z;
  ve_authorized_issuance : Z;
  ve_stake_claim : Z;
  ve_reward_claim : Z;
  ve_generation : Z;
  ve_phase : validator_economic_phase;
  ve_halted : bool
}.

Definition physical_total (state : validator_economics) : Z :=
  ve_general state
  + ve_fuel state
  + ve_pos state
  + ve_fuel_quarantine state
  + ve_cooperative state
  + ve_located state
  + ve_burned state.

Definition claims_backed (state : validator_economics) : Prop :=
  ve_stake_claim state + ve_reward_claim state <= ve_pos state.

Definition non_negative (state : validator_economics) : Prop :=
  0 <= ve_general state /\
  0 <= ve_fuel state /\
  0 <= ve_pos state /\
  0 <= ve_fuel_quarantine state /\
  0 <= ve_cooperative state /\
  0 <= ve_located state /\
  0 <= ve_burned state /\
  0 <= ve_authorized_issuance state /\
  0 <= ve_stake_claim state /\
  0 <= ve_reward_claim state.

Definition fund_validator_fuel
  (amount : Z)
  (state : validator_economics) : validator_economics :=
  if (0 <? amount) && (amount <=? ve_general state) then
    {| ve_general := ve_general state - amount;
       ve_fuel := ve_fuel state + amount;
       ve_pos := ve_pos state;
       ve_fuel_quarantine := ve_fuel_quarantine state;
       ve_cooperative := ve_cooperative state;
       ve_located := ve_located state;
       ve_burned := ve_burned state;
       ve_authorized_issuance := ve_authorized_issuance state;
       ve_stake_claim := ve_stake_claim state;
       ve_reward_claim := ve_reward_claim state;
       ve_generation := ve_generation state;
       ve_phase := ve_phase state;
       ve_halted := ve_halted state |}
  else state.

Definition handler_cost : Z := 3.

Definition execute_handler
  (state : validator_economics) : validator_economics :=
  if handler_cost <=? ve_fuel state then
    {| ve_general := ve_general state;
       ve_fuel := ve_fuel state - handler_cost;
       ve_pos := ve_pos state;
       ve_fuel_quarantine := ve_fuel_quarantine state;
       ve_cooperative := ve_cooperative state;
       ve_located := ve_located state;
       ve_burned := ve_burned state + handler_cost;
       ve_authorized_issuance := ve_authorized_issuance state;
       ve_stake_claim := ve_stake_claim state;
       ve_reward_claim := ve_reward_claim state;
       ve_generation := ve_generation state;
       ve_phase := ve_phase state;
       ve_halted := ve_halted state |}
  else state.

Definition issue_epoch_fuel
  (eligible : bool)
  (amount : Z)
  (state : validator_economics) : validator_economics :=
  if eligible && negb (ve_halted state) && (0 <=? amount) then
    {| ve_general := ve_general state;
       ve_fuel := ve_fuel state + amount;
       ve_pos := ve_pos state;
       ve_fuel_quarantine := ve_fuel_quarantine state;
       ve_cooperative := ve_cooperative state;
       ve_located := ve_located state;
       ve_burned := ve_burned state;
       ve_authorized_issuance := ve_authorized_issuance state + amount;
       ve_stake_claim := ve_stake_claim state;
       ve_reward_claim := ve_reward_claim state;
       ve_generation := ve_generation state;
       ve_phase := ve_phase state;
       ve_halted := ve_halted state |}
  else state.

Definition bond
  (amount : Z)
  (state : validator_economics) : validator_economics :=
  if (0 <? amount) && (amount <=? ve_general state) then
    {| ve_general := ve_general state - amount;
       ve_fuel := ve_fuel state;
       ve_pos := ve_pos state + amount;
       ve_fuel_quarantine := ve_fuel_quarantine state;
       ve_cooperative := ve_cooperative state;
       ve_located := ve_located state;
       ve_burned := ve_burned state;
       ve_authorized_issuance := ve_authorized_issuance state;
       ve_stake_claim := ve_stake_claim state + amount;
       ve_reward_claim := ve_reward_claim state;
       ve_generation := ve_generation state + 1;
       ve_phase := EconomicBonded;
       ve_halted := ve_halted state |}
  else state.

Definition complete_withdrawal
  (state : validator_economics) : validator_economics :=
  let amount := ve_stake_claim state + ve_reward_claim state in
  if amount <=? ve_pos state then
    {| ve_general := ve_general state + amount;
       ve_fuel := ve_fuel state;
       ve_pos := ve_pos state - amount;
       ve_fuel_quarantine := ve_fuel_quarantine state;
       ve_cooperative := ve_cooperative state;
       ve_located := ve_located state;
       ve_burned := ve_burned state;
       ve_authorized_issuance := ve_authorized_issuance state;
       ve_stake_claim := 0;
       ve_reward_claim := 0;
       ve_generation := ve_generation state;
       ve_phase := EconomicAbsent;
       ve_halted := ve_halted state |}
  else state.

Definition slash_fuel (state : validator_economics) : validator_economics :=
  {| ve_general := ve_general state;
     ve_fuel := 0;
     ve_pos := ve_pos state;
     ve_fuel_quarantine := ve_fuel_quarantine state + ve_fuel state;
     ve_cooperative := ve_cooperative state;
     ve_located := ve_located state;
     ve_burned := ve_burned state;
     ve_authorized_issuance := ve_authorized_issuance state;
     ve_stake_claim := ve_stake_claim state;
     ve_reward_claim := ve_reward_claim state;
     ve_generation := ve_generation state;
     ve_phase := EconomicQuarantined;
     ve_halted := true |}.

Definition vindicate (state : validator_economics) : validator_economics :=
  {| ve_general := ve_general state;
     ve_fuel := ve_fuel state + ve_fuel_quarantine state;
     ve_pos := ve_pos state;
     ve_fuel_quarantine := 0;
     ve_cooperative := ve_cooperative state;
     ve_located := ve_located state;
     ve_burned := ve_burned state;
     ve_authorized_issuance := ve_authorized_issuance state;
     ve_stake_claim := ve_stake_claim state;
     ve_reward_claim := ve_reward_claim state;
     ve_generation := ve_generation state;
     ve_phase := EconomicActive;
     ve_halted := false |}.

Definition guilty_fuel
  (penalty : Z)
  (state : validator_economics) : validator_economics :=
  if (0 <=? penalty) && (penalty <=? ve_fuel_quarantine state) then
    {| ve_general := ve_general state;
       ve_fuel := ve_fuel state + ve_fuel_quarantine state - penalty;
       ve_pos := ve_pos state;
       ve_fuel_quarantine := 0;
       ve_cooperative := ve_cooperative state + penalty;
       ve_located := ve_located state;
       ve_burned := ve_burned state;
       ve_authorized_issuance := ve_authorized_issuance state;
       ve_stake_claim := ve_stake_claim state;
       ve_reward_claim := ve_reward_claim state;
       ve_generation := ve_generation state;
       ve_phase := EconomicActive;
       ve_halted := false |}
  else state.

Definition burn_quarantine (state : validator_economics) : validator_economics :=
  {| ve_general := ve_general state;
     ve_fuel := ve_fuel state;
     ve_pos := ve_pos state - ve_stake_claim state - ve_reward_claim state;
     ve_fuel_quarantine := 0;
     ve_cooperative := ve_cooperative state;
     ve_located := ve_located state;
     ve_burned := ve_burned state
                  + ve_fuel_quarantine state
                  + ve_stake_claim state
                  + ve_reward_claim state;
     ve_authorized_issuance := ve_authorized_issuance state;
     ve_stake_claim := 0;
     ve_reward_claim := 0;
     ve_generation := ve_generation state;
     ve_phase := EconomicBurned;
     ve_halted := true |}.

Definition proposer_capacity (state : validator_economics) : Z :=
  ve_fuel state / handler_cost.

Theorem top_up_conserves : forall state amount,
  physical_total (fund_validator_fuel amount state) = physical_total state.
Proof.
  intros state amount. unfold fund_validator_fuel.
  destruct ((0 <? amount) && (amount <=? ve_general state));
    unfold physical_total; simpl; lia.
Qed.

Theorem top_up_is_one_way : forall state amount,
  ve_general (fund_validator_fuel amount state) <= ve_general state.
Proof.
  intros state amount. unfold fund_validator_fuel.
  destruct ((0 <? amount) && (amount <=? ve_general state)) eqn:Hgate; simpl.
  - apply andb_true_iff in Hgate as [Hpositive _].
    apply Z.ltb_lt in Hpositive. lia.
  - lia.
Qed.

Theorem handler_conserves_with_burn : forall state,
  physical_total (execute_handler state) = physical_total state.
Proof.
  intros state. unfold execute_handler.
  destruct (handler_cost <=? ve_fuel state);
    unfold physical_total, handler_cost; simpl; lia.
Qed.

Theorem handler_never_debits_general : forall state,
  ve_general (execute_handler state) = ve_general state.
Proof.
  intros state. unfold execute_handler.
  destruct (handler_cost <=? ve_fuel state); reflexivity.
Qed.

Theorem handler_burn_is_exact : forall state,
  handler_cost <= ve_fuel state ->
  ve_burned (execute_handler state) = ve_burned state + handler_cost /\
  ve_fuel (execute_handler state) = ve_fuel state - handler_cost.
Proof.
  intros state Hfunded. unfold execute_handler.
  apply Z.leb_le in Hfunded. rewrite Hfunded. split; reflexivity.
Qed.

Theorem epoch_issue_changes_only_fuel_and_issuance : forall state amount,
  0 <= amount ->
  ve_halted state = false ->
  ve_general (issue_epoch_fuel true amount state) = ve_general state /\
  ve_fuel (issue_epoch_fuel true amount state) = ve_fuel state + amount /\
  ve_authorized_issuance (issue_epoch_fuel true amount state) =
    ve_authorized_issuance state + amount.
Proof.
  intros state amount Hamount Hhalted. unfold issue_epoch_fuel.
  rewrite Hhalted. simpl. apply Z.leb_le in Hamount. rewrite Hamount.
  repeat split; reflexivity.
Qed.

Theorem epoch_issue_accounts_exactly : forall state eligible amount,
  physical_total (issue_epoch_fuel eligible amount state)
  = physical_total state
    + (ve_authorized_issuance (issue_epoch_fuel eligible amount state)
       - ve_authorized_issuance state).
Proof.
  intros state eligible amount. unfold issue_epoch_fuel.
  destruct (eligible && negb (ve_halted state) && (0 <=? amount));
    unfold physical_total; simpl; lia.
Qed.

Theorem bond_conserves_and_cannot_mint : forall state amount,
  physical_total (bond amount state) = physical_total state /\
  ve_authorized_issuance (bond amount state) = ve_authorized_issuance state /\
  ve_fuel (bond amount state) = ve_fuel state.
Proof.
  intros state amount. unfold bond.
  destruct ((0 <? amount) && (amount <=? ve_general state));
    unfold physical_total; simpl; repeat split; try reflexivity; lia.
Qed.

Theorem withdrawal_conserves_and_preserves_fuel : forall state,
  physical_total (complete_withdrawal state) = physical_total state /\
  ve_fuel (complete_withdrawal state) = ve_fuel state /\
  ve_authorized_issuance (complete_withdrawal state) =
    ve_authorized_issuance state.
Proof.
  intros state. unfold complete_withdrawal.
  destruct (ve_stake_claim state + ve_reward_claim state <=? ve_pos state);
    unfold physical_total; simpl; repeat split; try reflexivity; lia.
Qed.

Theorem slash_conserves_and_preserves_general : forall state,
  physical_total (slash_fuel state) = physical_total state /\
  ve_general (slash_fuel state) = ve_general state /\
  ve_fuel (slash_fuel state) = 0.
Proof.
  intros state. unfold physical_total, slash_fuel. simpl. lia.
Qed.

Theorem vindication_conserves : forall state,
  physical_total (vindicate state) = physical_total state /\
  ve_general (vindicate state) = ve_general state.
Proof.
  intros state. unfold physical_total, vindicate. simpl. lia.
Qed.

Theorem guilty_fuel_conserves : forall state penalty,
  physical_total (guilty_fuel penalty state) = physical_total state /\
  ve_general (guilty_fuel penalty state) = ve_general state.
Proof.
  intros state penalty. unfold guilty_fuel.
  destruct ((0 <=? penalty) && (penalty <=? ve_fuel_quarantine state));
    unfold physical_total; simpl; split; try reflexivity; lia.
Qed.

Theorem burn_quarantine_conserves_with_burn : forall state,
  physical_total (burn_quarantine state) = physical_total state.
Proof.
  intros state. unfold physical_total, burn_quarantine. simpl. lia.
Qed.

Theorem capacity_ignores_general : forall left right,
  ve_fuel left = ve_fuel right ->
  proposer_capacity left = proposer_capacity right.
Proof.
  intros left right Hfuel. unfold proposer_capacity. rewrite Hfuel. reflexivity.
Qed.

Theorem zero_general_funded_capacity : forall state,
  ve_general state = 0 ->
  handler_cost <= ve_fuel state ->
  1 <= proposer_capacity state.
Proof.
  intros state _ Hfuel. unfold proposer_capacity. unfold handler_cost in Hfuel |- *.
  apply (Z.div_le_lower_bound (ve_fuel state) 3 1).
  - lia.
  - exact Hfuel.
Qed.

Theorem positive_general_cannot_supply_empty_fuel : forall state,
  0 < ve_general state ->
  ve_fuel state = 0 ->
  proposer_capacity state = 0.
Proof.
  intros state _ Hfuel. unfold proposer_capacity. rewrite Hfuel. reflexivity.
Qed.

Inductive economic_transition : Z -> validator_economics -> validator_economics -> Prop :=
  | ETTopUp : forall state amount,
      economic_transition 0 state (fund_validator_fuel amount state)
  | ETHandler : forall state,
      economic_transition 0 state (execute_handler state)
  | ETEpoch : forall state eligible amount,
      economic_transition
        (ve_authorized_issuance (issue_epoch_fuel eligible amount state)
         - ve_authorized_issuance state)
        state
        (issue_epoch_fuel eligible amount state)
  | ETBond : forall state amount,
      economic_transition 0 state (bond amount state)
  | ETWithdrawal : forall state,
      economic_transition 0 state (complete_withdrawal state)
  | ETSlash : forall state,
      economic_transition 0 state (slash_fuel state)
  | ETVindicate : forall state,
      economic_transition 0 state (vindicate state)
  | ETGuilty : forall state penalty,
      economic_transition 0 state (guilty_fuel penalty state)
  | ETBurn : forall state,
      economic_transition 0 state (burn_quarantine state).

Inductive economic_trace : validator_economics -> Z -> validator_economics -> Prop :=
  | EconomicTraceRefl : forall state,
      economic_trace state 0 state
  | EconomicTraceStep : forall state middle final issued tail_issued,
      economic_transition issued state middle ->
      economic_trace middle tail_issued final ->
      economic_trace state (issued + tail_issued) final.

Lemma transition_accounts_exactly : forall issued state next,
  economic_transition issued state next ->
  physical_total next = physical_total state + issued.
Proof.
  intros issued state next Hstep. inversion Hstep; subst.
  - rewrite top_up_conserves. lia.
  - rewrite handler_conserves_with_burn. lia.
  - apply epoch_issue_accounts_exactly.
  - destruct (bond_conserves_and_cannot_mint state amount) as [H _]. lia.
  - destruct (withdrawal_conserves_and_preserves_fuel state) as [H _]. lia.
  - destruct (slash_conserves_and_preserves_general state) as [H _]. lia.
  - destruct (vindication_conserves state) as [H _]. lia.
  - destruct (guilty_fuel_conserves state penalty) as [H _]. lia.
  - rewrite burn_quarantine_conserves_with_burn. lia.
Qed.

Theorem arbitrary_trace_has_no_arbitrage : forall initial issued final,
  economic_trace initial issued final ->
  physical_total final = physical_total initial + issued.
Proof.
  intros initial issued final Htrace. induction Htrace.
  - lia.
  - pose proof (transition_accounts_exactly _ _ _ H) as Hstep.
    lia.
Qed.

Record two_validator_economics : Type := {
  first_economics : validator_economics;
  second_economics : validator_economics
}.

Definition transition_first
  (operation : validator_economics -> validator_economics)
  (state : two_validator_economics) : two_validator_economics :=
  {| first_economics := operation (first_economics state);
     second_economics := second_economics state |}.

Definition transition_second
  (operation : validator_economics -> validator_economics)
  (state : two_validator_economics) : two_validator_economics :=
  {| first_economics := first_economics state;
     second_economics := operation (second_economics state) |}.

Theorem distinct_validator_transitions_commute : forall operation_a operation_b state,
  transition_first operation_a (transition_second operation_b state) =
  transition_second operation_b (transition_first operation_a state).
Proof.
  intros operation_a operation_b [first second]. reflexivity.
Qed.

Print Assumptions arbitrary_trace_has_no_arbitrage.
Print Assumptions distinct_validator_transitions_commute.
