From Stdlib Require Import Arith.PeanoNat Bool.Bool Lia.

Record wallet_lollipop_state : Type := {
  sponsor_balance : nat;
  outer_balance : nat;
  slot_balance : nat;
  gateway_balance : nat;
  proposer_balance : nat;
  burned_cost : nat;
  protocol_minted_supply : nat
}.

Definition accounted_value (state : wallet_lollipop_state) : nat :=
  sponsor_balance state +
  outer_balance state +
  slot_balance state +
  gateway_balance state +
  proposer_balance state +
  burned_cost state.

Definition fund_located_purses
  (outer_amount slot_amount : nat)
  (state : wallet_lollipop_state)
  : option wallet_lollipop_state :=
  if Nat.leb (outer_amount + slot_amount) (sponsor_balance state) then
    Some {|
      sponsor_balance := sponsor_balance state - outer_amount - slot_amount;
      outer_balance := outer_balance state + outer_amount;
      slot_balance := slot_balance state + slot_amount;
      gateway_balance := gateway_balance state;
      proposer_balance := proposer_balance state;
      burned_cost := burned_cost state;
      protocol_minted_supply := protocol_minted_supply state
    |}
  else None.

Definition continuation_activated
  (funding_committed gateway_authenticated outer_committed : bool)
  : bool := funding_committed && gateway_authenticated && outer_committed.

Definition settle_lollipop_continuation
  (funding_committed gateway_authenticated outer_committed
    slot_capability_present : bool)
  (outer_certified_bound slot_certified_bound
    outer_realized_cost slot_realized_cost fee : nat)
  (state : wallet_lollipop_state)
  : option wallet_lollipop_state :=
  if continuation_activated
      funding_committed gateway_authenticated outer_committed &&
      slot_capability_present then
    if Nat.leb outer_certified_bound (outer_balance state) then
      if Nat.leb slot_certified_bound (slot_balance state) then
        if Nat.leb outer_realized_cost outer_certified_bound then
          if Nat.leb slot_realized_cost slot_certified_bound then
            if Nat.leb fee (gateway_balance state) then
              Some {|
                sponsor_balance := sponsor_balance state;
                outer_balance := outer_balance state - outer_realized_cost;
                slot_balance := slot_balance state - slot_realized_cost;
                gateway_balance := gateway_balance state - fee;
                proposer_balance := proposer_balance state + fee;
                burned_cost :=
                  burned_cost state + outer_realized_cost + slot_realized_cost;
                protocol_minted_supply := protocol_minted_supply state
              |}
            else None
          else None
        else None
      else None
    else None
  else None.

Definition replay_lollipop_continuation := settle_lollipop_continuation.

Definition address_only_authorizes_draw
  (_address_published capability_present : bool)
  : bool := capability_present.

Definition outer_address (outer_unforgeable : nat) : nat := outer_unforgeable.
Definition slot_address (slot_unforgeable : nat) : nat := slot_unforgeable.

Theorem outer_address_is_canonical : forall outer,
  outer_address outer = outer.
Proof.
  reflexivity.
Qed.

Theorem slot_address_is_canonical : forall slot,
  slot_address slot = slot.
Proof.
  reflexivity.
Qed.

Theorem located_addresses_are_injective :
  (forall left right,
    outer_address left = outer_address right -> left = right) /\
  (forall left right,
    slot_address left = slot_address right -> left = right).
Proof.
  split; intros left right equality; exact equality.
Qed.

Theorem public_addresses_are_not_draw_capabilities :
  forall outer slot capability,
    outer <> capability ->
    slot <> capability ->
    outer_address outer <> capability /\ slot_address slot <> capability.
Proof.
  intros outer slot capability outer_distinct slot_distinct.
  split; assumption.
Qed.

Theorem public_address_alone_never_authorizes_draw : forall published,
  address_only_authorizes_draw published false = false.
Proof.
  reflexivity.
Qed.

Theorem retained_slot_capability_authorizes_draw : forall published,
  address_only_authorizes_draw published true = true.
Proof.
  reflexivity.
Qed.

Theorem funding_success_is_exact : forall state outer_amount slot_amount funded,
  fund_located_purses outer_amount slot_amount state = Some funded ->
  sponsor_balance funded + outer_amount + slot_amount = sponsor_balance state /\
  outer_balance funded = outer_balance state + outer_amount /\
  slot_balance funded = slot_balance state + slot_amount /\
  gateway_balance funded = gateway_balance state /\
  proposer_balance funded = proposer_balance state /\
  burned_cost funded = burned_cost state /\
  protocol_minted_supply funded = protocol_minted_supply state.
Proof.
  intros state outer_amount slot_amount funded result.
  unfold fund_located_purses in result.
  destruct (Nat.leb (outer_amount + slot_amount) (sponsor_balance state))
    eqn:sufficient; try discriminate.
  apply Nat.leb_le in sufficient.
  inversion result; subst; clear result.
  repeat split; simpl; lia.
Qed.

Theorem funding_success_is_conserving : forall state outer_amount slot_amount funded,
  fund_located_purses outer_amount slot_amount state = Some funded ->
  accounted_value funded = accounted_value state.
Proof.
  intros state outer_amount slot_amount funded result.
  pose proof
    (funding_success_is_exact state outer_amount slot_amount funded result) as
    [sponsor_exact [outer_exact [slot_exact [gateway_exact
      [proposer_exact [burned_exact minted_exact]]]]]].
  unfold accounted_value.
  lia.
Qed.

Theorem funding_success_is_not_minting : forall state outer_amount slot_amount funded,
  fund_located_purses outer_amount slot_amount state = Some funded ->
  protocol_minted_supply funded = protocol_minted_supply state.
Proof.
  intros state outer_amount slot_amount funded result.
  pose proof
    (funding_success_is_exact state outer_amount slot_amount funded result) as
    [_ [_ [_ [_ [_ [_ minted_exact]]]]]].
  exact minted_exact.
Qed.

Theorem insufficient_sponsor_rejects_both_purses_atomically :
  forall state outer_amount slot_amount,
    sponsor_balance state < outer_amount + slot_amount ->
    fund_located_purses outer_amount slot_amount state = None.
Proof.
  intros state outer_amount slot_amount insufficient.
  unfold fund_located_purses.
  apply Nat.leb_gt in insufficient.
  now rewrite insufficient.
Qed.

Theorem continuation_activation_requires_prior_funding :
  forall gateway outer,
    continuation_activated false gateway outer = false.
Proof.
  reflexivity.
Qed.

Theorem continuation_activation_requires_gateway_authentication :
  forall funded outer,
    continuation_activated funded false outer = false.
Proof.
  intros funded outer.
  destruct funded; reflexivity.
Qed.

Theorem continuation_activation_requires_outer_authority :
  forall funded gateway,
    continuation_activated funded gateway false = false.
Proof.
  intros funded gateway.
  destruct funded, gateway; reflexivity.
Qed.

Theorem fully_authorized_continuation_activates :
  continuation_activated true true true = true.
Proof.
  reflexivity.
Qed.

Theorem settlement_requires_prior_funding :
  forall state gateway outer capability outer_bound slot_bound
    outer_actual slot_actual fee,
    settle_lollipop_continuation
      false gateway outer capability outer_bound slot_bound
      outer_actual slot_actual fee state = None.
Proof.
  reflexivity.
Qed.

Theorem settlement_requires_gateway_authentication :
  forall state funded outer capability outer_bound slot_bound
    outer_actual slot_actual fee,
    settle_lollipop_continuation
      funded false outer capability outer_bound slot_bound
      outer_actual slot_actual fee state = None.
Proof.
  intros state funded outer capability outer_bound slot_bound
    outer_actual slot_actual fee.
  destruct funded; reflexivity.
Qed.

Theorem settlement_requires_outer_authority :
  forall state funded gateway capability outer_bound slot_bound
    outer_actual slot_actual fee,
    settle_lollipop_continuation
      funded gateway false capability outer_bound slot_bound
      outer_actual slot_actual fee state = None.
Proof.
  intros state funded gateway capability outer_bound slot_bound
    outer_actual slot_actual fee.
  destruct funded, gateway; reflexivity.
Qed.

Theorem settlement_requires_retained_slot_capability :
  forall state funded gateway outer outer_bound slot_bound
    outer_actual slot_actual fee,
    settle_lollipop_continuation
      funded gateway outer false outer_bound slot_bound
      outer_actual slot_actual fee state = None.
Proof.
  intros state funded gateway outer outer_bound slot_bound
    outer_actual slot_actual fee.
  destruct funded, gateway, outer; reflexivity.
Qed.

Theorem insufficient_outer_purse_rejects_atomically :
  forall state outer_bound slot_bound outer_actual slot_actual fee,
    outer_balance state < outer_bound ->
    settle_lollipop_continuation
      true true true true outer_bound slot_bound
      outer_actual slot_actual fee state = None.
Proof.
  intros state outer_bound slot_bound outer_actual slot_actual fee insufficient.
  unfold settle_lollipop_continuation, continuation_activated.
  simpl.
  apply Nat.leb_gt in insufficient.
  now rewrite insufficient.
Qed.

Theorem insufficient_slot_purse_rejects_atomically :
  forall state outer_bound slot_bound outer_actual slot_actual fee,
    slot_balance state < slot_bound ->
    settle_lollipop_continuation
      true true true true outer_bound slot_bound
      outer_actual slot_actual fee state = None.
Proof.
  intros state outer_bound slot_bound outer_actual slot_actual fee insufficient.
  unfold settle_lollipop_continuation, continuation_activated.
  simpl.
  apply Nat.leb_gt in insufficient.
  destruct (outer_bound <=? outer_balance state); now rewrite insufficient || reflexivity.
Qed.

Theorem outer_realized_over_bound_rejects_atomically :
  forall state outer_bound slot_bound outer_actual slot_actual fee,
    outer_bound < outer_actual ->
    settle_lollipop_continuation
      true true true true outer_bound slot_bound
      outer_actual slot_actual fee state = None.
Proof.
  intros state outer_bound slot_bound outer_actual slot_actual fee over_bound.
  unfold settle_lollipop_continuation, continuation_activated.
  simpl.
  apply Nat.leb_gt in over_bound.
  destruct (outer_bound <=? outer_balance state),
    (slot_bound <=? slot_balance state); now rewrite over_bound || reflexivity.
Qed.

Theorem slot_realized_over_bound_rejects_atomically :
  forall state outer_bound slot_bound outer_actual slot_actual fee,
    slot_bound < slot_actual ->
    settle_lollipop_continuation
      true true true true outer_bound slot_bound
      outer_actual slot_actual fee state = None.
Proof.
  intros state outer_bound slot_bound outer_actual slot_actual fee over_bound.
  unfold settle_lollipop_continuation, continuation_activated.
  simpl.
  apply Nat.leb_gt in over_bound.
  destruct (outer_bound <=? outer_balance state),
    (slot_bound <=? slot_balance state),
    (outer_actual <=? outer_bound); now rewrite over_bound || reflexivity.
Qed.

Theorem insufficient_gateway_fee_rejects_atomically :
  forall state outer_bound slot_bound outer_actual slot_actual fee,
    gateway_balance state < fee ->
    settle_lollipop_continuation
      true true true true outer_bound slot_bound
      outer_actual slot_actual fee state = None.
Proof.
  intros state outer_bound slot_bound outer_actual slot_actual fee insufficient.
  unfold settle_lollipop_continuation, continuation_activated.
  simpl.
  apply Nat.leb_gt in insufficient.
  destruct (outer_bound <=? outer_balance state),
    (slot_bound <=? slot_balance state),
    (outer_actual <=? outer_bound),
    (slot_actual <=? slot_bound); now rewrite insufficient || reflexivity.
Qed.

Theorem settlement_success_is_component_exact :
  forall state outer_bound slot_bound outer_actual slot_actual fee settled,
    settle_lollipop_continuation
      true true true true outer_bound slot_bound
      outer_actual slot_actual fee state = Some settled ->
    sponsor_balance settled = sponsor_balance state /\
    outer_balance settled + outer_actual = outer_balance state /\
    slot_balance settled + slot_actual = slot_balance state /\
    gateway_balance settled + fee = gateway_balance state /\
    proposer_balance settled = proposer_balance state + fee /\
    burned_cost settled = burned_cost state + outer_actual + slot_actual /\
    protocol_minted_supply settled = protocol_minted_supply state.
Proof.
  intros state outer_bound slot_bound outer_actual slot_actual fee settled result.
  unfold settle_lollipop_continuation, continuation_activated in result.
  simpl in result.
  destruct (outer_bound <=? outer_balance state) eqn:outer_funded;
    try discriminate.
  destruct (slot_bound <=? slot_balance state) eqn:slot_funded;
    try discriminate.
  destruct (outer_actual <=? outer_bound) eqn:outer_within;
    try discriminate.
  destruct (slot_actual <=? slot_bound) eqn:slot_within;
    try discriminate.
  destruct (fee <=? gateway_balance state) eqn:fee_funded;
    try discriminate.
  apply Nat.leb_le in outer_funded.
  apply Nat.leb_le in slot_funded.
  apply Nat.leb_le in outer_within.
  apply Nat.leb_le in slot_within.
  apply Nat.leb_le in fee_funded.
  inversion result; subst; clear result.
  repeat split; simpl; lia.
Qed.

Theorem settlement_success_refunds_both_unused_bounds :
  forall state outer_bound slot_bound outer_actual slot_actual fee settled,
    settle_lollipop_continuation
      true true true true outer_bound slot_bound
      outer_actual slot_actual fee state = Some settled ->
    outer_balance settled =
      outer_balance state - outer_bound + (outer_bound - outer_actual) /\
    slot_balance settled =
      slot_balance state - slot_bound + (slot_bound - slot_actual).
Proof.
  intros state outer_bound slot_bound outer_actual slot_actual fee settled result.
  unfold settle_lollipop_continuation, continuation_activated in result.
  simpl in result.
  destruct (outer_bound <=? outer_balance state) eqn:outer_funded;
    try discriminate.
  destruct (slot_bound <=? slot_balance state) eqn:slot_funded;
    try discriminate.
  destruct (outer_actual <=? outer_bound) eqn:outer_within;
    try discriminate.
  destruct (slot_actual <=? slot_bound) eqn:slot_within;
    try discriminate.
  destruct (fee <=? gateway_balance state); try discriminate.
  apply Nat.leb_le in outer_funded.
  apply Nat.leb_le in slot_funded.
  apply Nat.leb_le in outer_within.
  apply Nat.leb_le in slot_within.
  inversion result; subst; clear result.
  simpl.
  split; lia.
Qed.

Theorem settlement_success_is_conserving :
  forall state outer_bound slot_bound outer_actual slot_actual fee settled,
    settle_lollipop_continuation
      true true true true outer_bound slot_bound
      outer_actual slot_actual fee state = Some settled ->
    accounted_value settled = accounted_value state.
Proof.
  intros state outer_bound slot_bound outer_actual slot_actual fee settled result.
  pose proof
    (settlement_success_is_component_exact
      state outer_bound slot_bound outer_actual slot_actual fee settled result) as
    [sponsor_exact [outer_exact [slot_exact [gateway_exact
      [proposer_exact [burned_exact minted_exact]]]]]].
  unfold accounted_value.
  lia.
Qed.

Theorem wallet_funding_then_lollipop_is_conserving :
  forall initial funded settled outer_amount slot_amount
    outer_bound slot_bound outer_actual slot_actual fee,
    fund_located_purses outer_amount slot_amount initial = Some funded ->
    settle_lollipop_continuation
      true true true true outer_bound slot_bound
      outer_actual slot_actual fee funded = Some settled ->
    accounted_value settled = accounted_value initial.
Proof.
  intros initial funded settled outer_amount slot_amount
    outer_bound slot_bound outer_actual slot_actual fee funding settlement.
  rewrite (settlement_success_is_conserving
    funded outer_bound slot_bound outer_actual slot_actual fee settled settlement).
  exact (funding_success_is_conserving
    initial outer_amount slot_amount funded funding).
Qed.

Theorem wallet_funding_then_lollipop_is_component_exact :
  forall initial funded settled outer_amount slot_amount
    outer_bound slot_bound outer_actual slot_actual fee,
    fund_located_purses outer_amount slot_amount initial = Some funded ->
    settle_lollipop_continuation
      true true true true outer_bound slot_bound
      outer_actual slot_actual fee funded = Some settled ->
    sponsor_balance settled + outer_amount + slot_amount = sponsor_balance initial /\
    outer_balance settled + outer_actual = outer_balance initial + outer_amount /\
    slot_balance settled + slot_actual = slot_balance initial + slot_amount /\
    gateway_balance settled + fee = gateway_balance initial /\
    proposer_balance settled = proposer_balance initial + fee /\
    burned_cost settled = burned_cost initial + outer_actual + slot_actual /\
    protocol_minted_supply settled = protocol_minted_supply initial.
Proof.
  intros initial funded settled outer_amount slot_amount
    outer_bound slot_bound outer_actual slot_actual fee funding settlement.
  pose proof
    (funding_success_is_exact initial outer_amount slot_amount funded funding) as
    [fund_sponsor [fund_outer [fund_slot [fund_gateway
      [fund_proposer [fund_burned fund_minted]]]]]].
  pose proof
    (settlement_success_is_component_exact
      funded outer_bound slot_bound outer_actual slot_actual fee settled settlement) as
    [settled_sponsor [settled_outer [settled_slot [settled_gateway
      [settled_proposer [settled_burned settled_minted]]]]]].
  repeat split; lia.
Qed.

Theorem replay_uses_identical_staged_settlement :
  forall state funded gateway outer capability outer_bound slot_bound
    outer_actual slot_actual fee,
    replay_lollipop_continuation
      funded gateway outer capability outer_bound slot_bound
      outer_actual slot_actual fee state =
    settle_lollipop_continuation
      funded gateway outer capability outer_bound slot_bound
      outer_actual slot_actual fee state.
Proof.
  reflexivity.
Qed.

Print Assumptions outer_address_is_canonical.
Print Assumptions slot_address_is_canonical.
Print Assumptions located_addresses_are_injective.
Print Assumptions public_addresses_are_not_draw_capabilities.
Print Assumptions public_address_alone_never_authorizes_draw.
Print Assumptions retained_slot_capability_authorizes_draw.
Print Assumptions funding_success_is_exact.
Print Assumptions funding_success_is_conserving.
Print Assumptions funding_success_is_not_minting.
Print Assumptions insufficient_sponsor_rejects_both_purses_atomically.
Print Assumptions continuation_activation_requires_prior_funding.
Print Assumptions continuation_activation_requires_gateway_authentication.
Print Assumptions continuation_activation_requires_outer_authority.
Print Assumptions fully_authorized_continuation_activates.
Print Assumptions settlement_requires_prior_funding.
Print Assumptions settlement_requires_gateway_authentication.
Print Assumptions settlement_requires_outer_authority.
Print Assumptions settlement_requires_retained_slot_capability.
Print Assumptions insufficient_outer_purse_rejects_atomically.
Print Assumptions insufficient_slot_purse_rejects_atomically.
Print Assumptions outer_realized_over_bound_rejects_atomically.
Print Assumptions slot_realized_over_bound_rejects_atomically.
Print Assumptions insufficient_gateway_fee_rejects_atomically.
Print Assumptions settlement_success_is_component_exact.
Print Assumptions settlement_success_refunds_both_unused_bounds.
Print Assumptions settlement_success_is_conserving.
Print Assumptions wallet_funding_then_lollipop_is_conserving.
Print Assumptions wallet_funding_then_lollipop_is_component_exact.
Print Assumptions replay_uses_identical_staged_settlement.
