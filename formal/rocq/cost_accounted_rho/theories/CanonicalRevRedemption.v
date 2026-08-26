From Stdlib Require Import Arith.PeanoNat Lia.

Inductive redemption_outcome : Type :=
| Vindicated
| Guilty (penalty : nat)
| Burned.

Record rev_custody : Type := {
  liquid_fuel : nat;
  quarantined_fuel : nat;
  bonded_stake : nat;
  quarantined_stake : nat;
  coop_balance : nat;
  burned_balance : nat
}.

Definition custody_total (state : rev_custody) : nat :=
  liquid_fuel state
  + quarantined_fuel state
  + bonded_stake state
  + quarantined_stake state
  + coop_balance state
  + burned_balance state.

Definition slash_custody (state : rev_custody) : rev_custody :=
  {| liquid_fuel := 0;
     quarantined_fuel := quarantined_fuel state + liquid_fuel state;
     bonded_stake := 0;
     quarantined_stake := quarantined_stake state + bonded_stake state;
     coop_balance := coop_balance state;
     burned_balance := burned_balance state |}.

Definition redeem_custody
  (authorized : bool)
  (outcome : redemption_outcome)
  (state : rev_custody)
  : rev_custody :=
  if authorized then
    match outcome with
    | Vindicated =>
        {| liquid_fuel := liquid_fuel state + quarantined_fuel state;
           quarantined_fuel := 0;
           bonded_stake := bonded_stake state + quarantined_stake state;
           quarantined_stake := 0;
           coop_balance := coop_balance state;
           burned_balance := burned_balance state |}
    | Guilty penalty =>
        if penalty <? quarantined_stake state then
          let fuel_penalty := Nat.min penalty (quarantined_fuel state) in
          {| liquid_fuel := liquid_fuel state + quarantined_fuel state - fuel_penalty;
             quarantined_fuel := 0;
             bonded_stake := bonded_stake state + quarantined_stake state - penalty;
             quarantined_stake := 0;
             coop_balance := coop_balance state + penalty + fuel_penalty;
             burned_balance := burned_balance state |}
        else state
    | Burned =>
        {| liquid_fuel := liquid_fuel state;
           quarantined_fuel := 0;
           bonded_stake := bonded_stake state;
           quarantined_stake := 0;
           coop_balance := coop_balance state;
           burned_balance := burned_balance state
             + quarantined_fuel state
             + quarantined_stake state |}
    end
  else state.

Theorem slash_custody_conserves :
  forall state,
    custody_total (slash_custody state) = custody_total state.
Proof.
  intros state. destruct state. unfold slash_custody, custody_total. simpl. lia.
Qed.

Theorem unauthorized_redemption_is_identity :
  forall state outcome,
    redeem_custody false outcome state = state.
Proof.
  reflexivity.
Qed.

Theorem vindicated_redemption_conserves :
  forall state,
    custody_total (redeem_custody true Vindicated state) = custody_total state.
Proof.
  intros state. destruct state. unfold redeem_custody, custody_total. simpl. lia.
Qed.

Theorem guilty_redemption_conserves :
  forall state penalty,
    custody_total (redeem_custody true (Guilty penalty) state) = custody_total state.
Proof.
  intros state penalty. destruct state.
  unfold redeem_custody, custody_total. simpl.
  destruct (penalty <? quarantined_stake0) eqn:Hpartial; simpl.
  - apply Nat.ltb_lt in Hpartial.
    pose proof (Nat.le_min_r penalty quarantined_fuel0).
    lia.
  - lia.
Qed.

Theorem guilty_redemption_credits_both_roles_once :
  forall state penalty,
    penalty < quarantined_stake state ->
    let fuel_penalty := Nat.min penalty (quarantined_fuel state) in
    coop_balance (redeem_custody true (Guilty penalty) state)
      = coop_balance state + penalty + fuel_penalty.
Proof.
  intros state penalty Hpartial. unfold redeem_custody.
  assert (Hltb : (penalty <? quarantined_stake state) = true).
  { apply Nat.ltb_lt. exact Hpartial. }
  rewrite Hltb. reflexivity.
Qed.

Theorem full_guilty_confiscation_is_rejected :
  forall state penalty,
    quarantined_stake state <= penalty ->
    redeem_custody true (Guilty penalty) state = state.
Proof.
  intros state penalty Hfull. unfold redeem_custody.
  apply Nat.ltb_ge in Hfull. rewrite Hfull. reflexivity.
Qed.

Theorem burned_redemption_conserves_canonical_rev :
  forall state,
    custody_total (redeem_custody true Burned state) = custody_total state.
Proof.
  intros state. destruct state. unfold redeem_custody, custody_total. simpl. lia.
Qed.

Theorem burned_redemption_removes_circulating_claims :
  forall state,
    quarantined_fuel (redeem_custody true Burned state) = 0
    /\ quarantined_stake (redeem_custody true Burned state) = 0
    /\ burned_balance (redeem_custody true Burned state)
       = burned_balance state + quarantined_fuel state + quarantined_stake state.
Proof.
  intros state. repeat split; reflexivity.
Qed.

Print Assumptions slash_custody_conserves.
Print Assumptions unauthorized_redemption_is_identity.
Print Assumptions vindicated_redemption_conserves.
Print Assumptions guilty_redemption_conserves.
Print Assumptions guilty_redemption_credits_both_roles_once.
Print Assumptions full_guilty_confiscation_is_rejected.
Print Assumptions burned_redemption_conserves_canonical_rev.
Print Assumptions burned_redemption_removes_circulating_claims.
