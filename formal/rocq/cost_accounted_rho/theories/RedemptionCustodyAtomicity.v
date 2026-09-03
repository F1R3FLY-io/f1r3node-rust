From Stdlib Require Import Arith.PeanoNat Bool.Bool Lia.

Inductive RestorablePhase : Type :=
| RestoreBonded
| RestorePendingWithdraw
| RestoreWithdrawing.

Inductive LifecyclePhase : Type :=
| LifecycleBonded
| LifecyclePendingWithdraw
| LifecycleWithdrawing
| LifecycleQuarantined (origin : RestorablePhase)
| LifecycleWithdrawn
| LifecycleBurned.

Inductive RedemptionOutcome : Type :=
| RedemptionVindicated
| RedemptionGuilty (penalty : nat)
| RedemptionBurned.

Scheme Equality for RedemptionOutcome.

Record RedemptionRequest : Type := mkRedemptionRequest {
  request_generation : nat;
  request_outcome : RedemptionOutcome
}.

Record CustodyState : Type := mkCustodyState {
  custody_phase : LifecyclePhase;
  custody_generation : nat;
  custody_bond : nat;
  custody_reward : nat;
  custody_pos_vault : nat;
  custody_fuel : nat;
  custody_wallet : nat;
  custody_cooperative_stake : nat;
  custody_cooperative_fuel : nat;
  custody_burned_stake : nat;
  custody_burned_fuel : nat;
  custody_receipt : option RedemptionRequest
}.

Definition restore_phase (phase : RestorablePhase) : LifecyclePhase :=
  match phase with
  | RestoreBonded => LifecycleBonded
  | RestorePendingWithdraw => LifecyclePendingWithdraw
  | RestoreWithdrawing => LifecycleWithdrawing
  end.

Definition request_matches
  (left right : RedemptionRequest) : bool :=
  Nat.eqb (request_generation left) (request_generation right)
  && RedemptionOutcome_beq (request_outcome left) (request_outcome right).

Definition stake_total (state : CustodyState) : nat :=
  custody_bond state
  + custody_reward state
  + custody_wallet state
  + custody_cooperative_stake state
  + custody_burned_stake state.

Definition fuel_total (state : CustodyState) : nat :=
  custody_fuel state
  + custody_cooperative_fuel state
  + custody_burned_fuel state.

Definition physical_stake_total (state : CustodyState) : nat :=
  custody_pos_vault state
  + custody_wallet state
  + custody_cooperative_stake state
  + custody_burned_stake state.

Definition pos_claim_is_covered (state : CustodyState) : Prop :=
  custody_pos_vault state = custody_bond state + custody_reward state.

Definition vindicated_state
  (request : RedemptionRequest)
  (origin : RestorablePhase)
  (state : CustodyState) : CustodyState :=
  mkCustodyState
    (restore_phase origin)
    (custody_generation state)
    (custody_bond state)
    (custody_reward state)
    (custody_pos_vault state)
    (custody_fuel state)
    (custody_wallet state)
    (custody_cooperative_stake state)
    (custody_cooperative_fuel state)
    (custody_burned_stake state)
    (custody_burned_fuel state)
    (Some request).

Definition guilty_state
  (request : RedemptionRequest)
  (origin : RestorablePhase)
  (penalty : nat)
  (state : CustodyState) : CustodyState :=
  let fuel_penalty := Nat.min penalty (custody_fuel state) in
  mkCustodyState
    (restore_phase origin)
    (custody_generation state)
    (custody_bond state - penalty)
    (custody_reward state)
    (custody_pos_vault state - penalty)
    (custody_fuel state - fuel_penalty)
    (custody_wallet state)
    (custody_cooperative_stake state + penalty)
    (custody_cooperative_fuel state + fuel_penalty)
    (custody_burned_stake state)
    (custody_burned_fuel state)
    (Some request).

Definition burned_state
  (request : RedemptionRequest)
  (state : CustodyState) : CustodyState :=
  mkCustodyState
    LifecycleBurned
    (custody_generation state)
    0
    0
    (custody_pos_vault state - custody_bond state - custody_reward state)
    0
    (custody_wallet state)
    (custody_cooperative_stake state)
    (custody_cooperative_fuel state)
    (custody_burned_stake state + custody_bond state + custody_reward state)
    (custody_burned_fuel state + custody_fuel state)
    (Some request).

Definition resolve_redemption
  (request : RedemptionRequest)
  (state : CustodyState) : CustodyState * bool :=
  match custody_receipt state with
  | Some prior =>
      if request_matches prior request then (state, true) else (state, false)
  | None =>
      match custody_phase state with
      | LifecycleQuarantined origin =>
          if Nat.eqb (request_generation request) (custody_generation state) then
            match request_outcome request with
            | RedemptionVindicated =>
                (vindicated_state request origin state, true)
            | RedemptionGuilty penalty =>
                if penalty <? custody_bond state then
                  (guilty_state request origin penalty state, true)
                else (state, false)
            | RedemptionBurned =>
                (burned_state request state, true)
            end
          else (state, false)
      | _ => (state, false)
      end
  end.

Definition execute_redemption
  (evaluation_succeeded : bool)
  (request : RedemptionRequest)
  (state : CustodyState) : CustodyState * bool :=
  if evaluation_succeeded
  then resolve_redemption request state
  else (state, false).

Lemma request_matches_self :
  forall request,
    request_matches request request = true.
Proof.
  intros [generation outcome]. unfold request_matches. simpl.
  rewrite Nat.eqb_refl.
  destruct outcome; simpl; try rewrite Nat.eqb_refl; reflexivity.
Qed.

Theorem failed_evaluation_publishes_nothing :
  forall request state,
    execute_redemption false request state = (state, false).
Proof.
  reflexivity.
Qed.

Theorem unauthorized_generation_is_effect_free :
  forall state request origin,
    custody_receipt state = None ->
    custody_phase state = LifecycleQuarantined origin ->
    request_generation request <> custody_generation state ->
    resolve_redemption request state = (state, false).
Proof.
  intros state request origin Hreceipt Hphase Hgeneration.
  unfold resolve_redemption. rewrite Hreceipt, Hphase.
  apply Nat.eqb_neq in Hgeneration. rewrite Hgeneration. reflexivity.
Qed.

Theorem full_guilty_penalty_is_effect_free :
  forall state generation origin penalty,
    custody_receipt state = None ->
    custody_phase state = LifecycleQuarantined origin ->
    custody_generation state = generation ->
    custody_bond state <= penalty ->
    resolve_redemption
      (mkRedemptionRequest generation (RedemptionGuilty penalty)) state
      = (state, false).
Proof.
  intros state generation origin penalty Hreceipt Hphase Hgeneration Hfull.
  unfold resolve_redemption. rewrite Hreceipt, Hphase. simpl.
  rewrite <- Hgeneration, Nat.eqb_refl.
  apply Nat.ltb_ge in Hfull. rewrite Hfull. reflexivity.
Qed.

Theorem exact_retry_is_idempotent :
  forall state request,
    custody_receipt state = Some request ->
    resolve_redemption request state = (state, true).
Proof.
  intros state request Hreceipt. unfold resolve_redemption.
  rewrite Hreceipt, request_matches_self. reflexivity.
Qed.

Theorem conflicting_retry_is_effect_free :
  forall state prior request,
    custody_receipt state = Some prior ->
    request_matches prior request = false ->
    resolve_redemption request state = (state, false).
Proof.
  intros state prior request Hreceipt Hconflict. unfold resolve_redemption.
  rewrite Hreceipt, Hconflict. reflexivity.
Qed.

Theorem vindication_restores_exact_phase :
  forall state generation origin,
    custody_receipt state = None ->
    custody_phase state = LifecycleQuarantined origin ->
    custody_generation state = generation ->
    custody_phase
      (fst (resolve_redemption
        (mkRedemptionRequest generation RedemptionVindicated) state))
      = restore_phase origin.
Proof.
  intros state generation origin Hreceipt Hphase Hgeneration.
  unfold resolve_redemption. rewrite Hreceipt, Hphase. simpl.
  rewrite <- Hgeneration, Nat.eqb_refl. reflexivity.
Qed.

Theorem guilty_restores_exact_phase :
  forall state generation origin penalty,
    custody_receipt state = None ->
    custody_phase state = LifecycleQuarantined origin ->
    custody_generation state = generation ->
    penalty < custody_bond state ->
    custody_phase
      (fst (resolve_redemption
        (mkRedemptionRequest generation (RedemptionGuilty penalty)) state))
      = restore_phase origin.
Proof.
  intros state generation origin penalty Hreceipt Hphase Hgeneration Hpartial.
  unfold resolve_redemption. rewrite Hreceipt, Hphase. simpl.
  rewrite <- Hgeneration, Nat.eqb_refl.
  apply Nat.ltb_lt in Hpartial. rewrite Hpartial. reflexivity.
Qed.

Theorem resolution_conserves_canonical_rev :
  forall request state,
    stake_total (fst (resolve_redemption request state)) = stake_total state
    /\ fuel_total (fst (resolve_redemption request state)) = fuel_total state.
Proof.
  intros [request_generation0 request_outcome0]
    [phase generation bond reward pos_vault fuel wallet cooperative_stake
      cooperative_fuel burned_stake burned_fuel receipt].
  unfold resolve_redemption.
  simpl.
  destruct receipt as [prior |].
  - destruct (request_matches prior
      (mkRedemptionRequest request_generation0 request_outcome0));
      simpl; split; reflexivity.
  - destruct phase; try (simpl; split; reflexivity).
    destruct (Nat.eqb request_generation0 generation); [| simpl; split; reflexivity].
    destruct request_outcome0 as [| penalty |]; simpl.
    + split; reflexivity.
    + destruct (penalty <? bond) eqn:Hpartial.
      * apply Nat.ltb_lt in Hpartial.
        pose proof (Nat.le_min_r penalty fuel).
        split; unfold stake_total, fuel_total; simpl; lia.
      * split; reflexivity.
    + split; unfold stake_total, fuel_total; simpl; lia.
Qed.

Theorem resolution_preserves_physical_custody :
  forall request state,
    pos_claim_is_covered state ->
    pos_claim_is_covered (fst (resolve_redemption request state))
    /\ physical_stake_total (fst (resolve_redemption request state))
       = physical_stake_total state.
Proof.
  intros [request_generation0 request_outcome0]
    [phase generation bond reward pos_vault fuel wallet cooperative_stake
      cooperative_fuel burned_stake burned_fuel receipt] Hcovered.
  unfold pos_claim_is_covered in Hcovered.
  simpl in Hcovered.
  unfold resolve_redemption.
  simpl.
  destruct receipt as [prior |].
  - destruct (request_matches prior
      (mkRedemptionRequest request_generation0 request_outcome0));
      simpl; split; try assumption; reflexivity.
  - destruct phase; try (simpl; split; try assumption; reflexivity).
    destruct (Nat.eqb request_generation0 generation); [| simpl; split; try assumption; reflexivity].
    destruct request_outcome0 as [| penalty |]; simpl.
    + split; try assumption; reflexivity.
    + destruct (penalty <? bond) eqn:Hpartial.
      * apply Nat.ltb_lt in Hpartial.
        split; unfold pos_claim_is_covered, physical_stake_total; simpl; lia.
      * split; try assumption; reflexivity.
    + split; unfold pos_claim_is_covered, physical_stake_total; simpl; lia.
Qed.

Record TwoValidatorCustody : Type := mkTwoValidatorCustody {
  first_validator : CustodyState;
  second_validator : CustodyState
}.

Definition resolve_first
  (request : RedemptionRequest)
  (state : TwoValidatorCustody) : TwoValidatorCustody :=
  mkTwoValidatorCustody
    (fst (resolve_redemption request (first_validator state)))
    (second_validator state).

Definition resolve_second
  (request : RedemptionRequest)
  (state : TwoValidatorCustody) : TwoValidatorCustody :=
  mkTwoValidatorCustody
    (first_validator state)
    (fst (resolve_redemption request (second_validator state))).

Theorem distinct_validator_resolutions_commute :
  forall state first_request second_request,
    resolve_first first_request (resolve_second second_request state)
    = resolve_second second_request (resolve_first first_request state).
Proof.
  intros [first second] first_request second_request. reflexivity.
Qed.

Print Assumptions failed_evaluation_publishes_nothing.
Print Assumptions unauthorized_generation_is_effect_free.
Print Assumptions full_guilty_penalty_is_effect_free.
Print Assumptions exact_retry_is_idempotent.
Print Assumptions conflicting_retry_is_effect_free.
Print Assumptions vindication_restores_exact_phase.
Print Assumptions guilty_restores_exact_phase.
Print Assumptions resolution_conserves_canonical_rev.
Print Assumptions resolution_preserves_physical_custody.
Print Assumptions distinct_validator_resolutions_commute.
