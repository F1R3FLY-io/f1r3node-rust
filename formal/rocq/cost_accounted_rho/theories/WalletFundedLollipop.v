From Stdlib Require Import Arith.PeanoNat Bool.Bool Lia.

Record wallet_lollipop_state : Type := {
  sponsor_balance : nat;
  slot_balance : nat;
  gateway_balance : nat;
  proposer_balance : nat;
  burned_cost : nat;
  protocol_minted_supply : nat
}.

Definition accounted_value (state : wallet_lollipop_state) : nat :=
  sponsor_balance state +
  slot_balance state +
  gateway_balance state +
  proposer_balance state +
  burned_cost state.

Definition fund_slot
  (amount : nat)
  (state : wallet_lollipop_state)
  : option wallet_lollipop_state :=
  if Nat.leb amount (sponsor_balance state) then
    Some {|
      sponsor_balance := sponsor_balance state - amount;
      slot_balance := slot_balance state + amount;
      gateway_balance := gateway_balance state;
      proposer_balance := proposer_balance state;
      burned_cost := burned_cost state;
      protocol_minted_supply := protocol_minted_supply state
    |}
  else None.

Definition settle_lollipop_continuation
  (outer_committed slot_capability_present : bool)
  (certified_bound realized_cost fee : nat)
  (state : wallet_lollipop_state)
  : option wallet_lollipop_state :=
  if outer_committed && slot_capability_present then
    if Nat.leb certified_bound (slot_balance state) then
      if Nat.leb realized_cost certified_bound then
        if Nat.leb fee (gateway_balance state) then
          Some {|
            sponsor_balance := sponsor_balance state;
            slot_balance := slot_balance state - realized_cost;
            gateway_balance := gateway_balance state - fee;
            proposer_balance := proposer_balance state + fee;
            burned_cost := burned_cost state + realized_cost;
            protocol_minted_supply := protocol_minted_supply state
          |}
        else None
      else None
    else None
  else None.

Definition replay_lollipop_continuation := settle_lollipop_continuation.

Definition execute_wallet_lollipop
  (gateway_authenticated outer_committed slot_capability_present : bool)
  (certified_bound realized_cost fee : nat)
  (state : wallet_lollipop_state)
  : option wallet_lollipop_state :=
  if gateway_authenticated then
    settle_lollipop_continuation
      outer_committed slot_capability_present
      certified_bound realized_cost fee state
  else None.

Definition replay_wallet_lollipop := execute_wallet_lollipop.

Definition address_only_authorizes_draw
  (_address_published slot_capability_present : bool)
  : bool := slot_capability_present.

Definition slot_address (slot_unforgeable : nat) : nat := slot_unforgeable.

Theorem slot_address_is_canonical : forall slot,
  slot_address slot = slot.
Proof.
  reflexivity.
Qed.

Theorem slot_address_is_injective : forall left right,
  slot_address left = slot_address right ->
  left = right.
Proof.
  intros left right equality.
  exact equality.
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

Theorem fund_slot_success_is_exact : forall state amount funded,
  fund_slot amount state = Some funded ->
  sponsor_balance funded + amount = sponsor_balance state /\
  slot_balance funded = slot_balance state + amount /\
  gateway_balance funded = gateway_balance state /\
  proposer_balance funded = proposer_balance state /\
  burned_cost funded = burned_cost state /\
  protocol_minted_supply funded = protocol_minted_supply state.
Proof.
  intros state amount funded result.
  unfold fund_slot in result.
  destruct (Nat.leb amount (sponsor_balance state)) eqn:sufficient;
    try discriminate.
  apply Nat.leb_le in sufficient.
  inversion result; subst; clear result.
  repeat split; simpl; lia.
Qed.

Theorem fund_slot_success_is_conserving : forall state amount funded,
  fund_slot amount state = Some funded ->
  accounted_value funded = accounted_value state.
Proof.
  intros state amount funded result.
  pose proof (fund_slot_success_is_exact state amount funded result) as
    [sponsor_exact [slot_exact [gateway_exact
      [proposer_exact [burned_exact minted_exact]]]]].
  unfold accounted_value.
  lia.
Qed.

Theorem fund_slot_is_not_minting : forall state amount funded,
  fund_slot amount state = Some funded ->
  protocol_minted_supply funded = protocol_minted_supply state.
Proof.
  intros state amount funded result.
  pose proof (fund_slot_success_is_exact state amount funded result) as
    [_ [_ [_ [_ [_ minted_exact]]]]].
  exact minted_exact.
Qed.

Theorem insufficient_sponsor_rejects_funding : forall state amount,
  sponsor_balance state < amount ->
  fund_slot amount state = None.
Proof.
  intros state amount insufficient.
  unfold fund_slot.
  apply Nat.leb_gt in insufficient.
  now rewrite insufficient.
Qed.

Theorem continuation_requires_outer_commit : forall state bound actual fee,
  settle_lollipop_continuation false true bound actual fee state = None.
Proof.
  reflexivity.
Qed.

Theorem continuation_requires_retained_slot_capability : forall state bound actual fee,
  settle_lollipop_continuation true false bound actual fee state = None.
Proof.
  reflexivity.
Qed.

Theorem unauthenticated_gateway_cannot_consume_continuation :
  forall state outer_committed slot_capability_present bound actual fee,
    execute_wallet_lollipop
      false outer_committed slot_capability_present
      bound actual fee state = None.
Proof.
  reflexivity.
Qed.

Theorem authenticated_gateway_refines_slot_settlement :
  forall state outer_committed slot_capability_present bound actual fee,
    execute_wallet_lollipop
      true outer_committed slot_capability_present
      bound actual fee state =
    settle_lollipop_continuation
      outer_committed slot_capability_present
      bound actual fee state.
Proof.
  reflexivity.
Qed.

Theorem continuation_insufficient_slot_rejects_atomically : forall state bound actual fee,
  slot_balance state < bound ->
  settle_lollipop_continuation true true bound actual fee state = None.
Proof.
  intros state bound actual fee insufficient.
  unfold settle_lollipop_continuation.
  simpl.
  apply Nat.leb_gt in insufficient.
  now rewrite insufficient.
Qed.

Theorem continuation_realized_over_bound_rejects_atomically : forall state bound actual fee,
  bound < actual ->
  settle_lollipop_continuation true true bound actual fee state = None.
Proof.
  intros state bound actual fee over_bound.
  unfold settle_lollipop_continuation.
  simpl.
  apply Nat.leb_gt in over_bound.
  destruct (Nat.leb bound (slot_balance state)).
  - now rewrite over_bound.
  - reflexivity.
Qed.

Theorem continuation_insufficient_gateway_fee_rejects_atomically :
  forall state bound actual fee,
    gateway_balance state < fee ->
    settle_lollipop_continuation true true bound actual fee state = None.
Proof.
  intros state bound actual fee insufficient.
  unfold settle_lollipop_continuation.
  simpl.
  apply Nat.leb_gt in insufficient.
  destruct (Nat.leb bound (slot_balance state)).
  - destruct (Nat.leb actual bound).
    + now rewrite insufficient.
    + reflexivity.
  - reflexivity.
Qed.

Theorem continuation_success_uses_slot_and_separates_fee :
  forall state bound actual fee settled,
    settle_lollipop_continuation true true bound actual fee state = Some settled ->
    sponsor_balance settled = sponsor_balance state /\
    slot_balance settled + actual = slot_balance state /\
    gateway_balance settled + fee = gateway_balance state /\
    proposer_balance settled = proposer_balance state + fee /\
    burned_cost settled = burned_cost state + actual /\
    protocol_minted_supply settled = protocol_minted_supply state.
Proof.
  intros state bound actual fee settled result.
  unfold settle_lollipop_continuation in result.
  simpl in result.
  destruct (Nat.leb bound (slot_balance state)) eqn:slot_funded;
    try discriminate.
  destruct (Nat.leb actual bound) eqn:within_bound;
    try discriminate.
  destruct (Nat.leb fee (gateway_balance state)) eqn:fee_funded;
    try discriminate.
  apply Nat.leb_le in slot_funded.
  apply Nat.leb_le in within_bound.
  apply Nat.leb_le in fee_funded.
  inversion result; subst; clear result.
  repeat split; simpl; lia.
Qed.

Theorem continuation_success_refunds_unused_bound :
  forall state bound actual fee settled,
    settle_lollipop_continuation true true bound actual fee state = Some settled ->
    slot_balance settled =
      slot_balance state - bound + (bound - actual).
Proof.
  intros state bound actual fee settled result.
  unfold settle_lollipop_continuation in result.
  simpl in result.
  destruct (Nat.leb bound (slot_balance state)) eqn:slot_funded;
    try discriminate.
  destruct (Nat.leb actual bound) eqn:within_bound;
    try discriminate.
  destruct (Nat.leb fee (gateway_balance state));
    try discriminate.
  apply Nat.leb_le in slot_funded.
  apply Nat.leb_le in within_bound.
  inversion result; subst; clear result.
  simpl.
  lia.
Qed.

Theorem continuation_success_is_conserving :
  forall state bound actual fee settled,
    settle_lollipop_continuation true true bound actual fee state = Some settled ->
    accounted_value settled = accounted_value state.
Proof.
  intros state bound actual fee settled result.
  pose proof
    (continuation_success_uses_slot_and_separates_fee
      state bound actual fee settled result) as
    [sponsor_exact [slot_exact [gateway_exact
      [proposer_exact [burned_exact minted_exact]]]]].
  unfold accounted_value.
  lia.
Qed.

Theorem wallet_funding_then_lollipop_is_conserving :
  forall initial funded settled amount bound actual fee,
    fund_slot amount initial = Some funded ->
    execute_wallet_lollipop true true true bound actual fee funded = Some settled ->
    accounted_value settled = accounted_value initial.
Proof.
  intros initial funded settled amount bound actual fee funding settlement.
  cbn in settlement.
  rewrite (continuation_success_is_conserving
    funded bound actual fee settled settlement).
  exact (fund_slot_success_is_conserving initial amount funded funding).
Qed.

Theorem wallet_funding_then_lollipop_is_exact :
  forall initial funded settled amount bound actual fee,
    fund_slot amount initial = Some funded ->
    execute_wallet_lollipop true true true bound actual fee funded = Some settled ->
    sponsor_balance settled + amount = sponsor_balance initial /\
    slot_balance settled + actual = slot_balance initial + amount /\
    gateway_balance settled + fee = gateway_balance initial /\
    proposer_balance settled = proposer_balance initial + fee /\
    burned_cost settled = burned_cost initial + actual /\
    protocol_minted_supply settled = protocol_minted_supply initial.
Proof.
  intros initial funded settled amount bound actual fee funding settlement.
  cbn in settlement.
  pose proof (fund_slot_success_is_exact initial amount funded funding) as
    [fund_sponsor [fund_slot_balance [fund_gateway
      [fund_proposer [fund_burned fund_minted]]]]].
  pose proof
    (continuation_success_uses_slot_and_separates_fee
      funded bound actual fee settled settlement) as
    [settled_sponsor [settled_slot [settled_gateway
      [settled_proposer [settled_burned settled_minted]]]]].
  repeat split; lia.
Qed.

Theorem replay_uses_identical_authenticated_settlement :
  forall state gateway_authenticated outer_committed slot_capability_present
    bound actual fee,
    replay_wallet_lollipop
      gateway_authenticated outer_committed slot_capability_present
      bound actual fee state =
    execute_wallet_lollipop
      gateway_authenticated outer_committed slot_capability_present
      bound actual fee state.
Proof.
  reflexivity.
Qed.

Print Assumptions slot_address_is_canonical.
Print Assumptions slot_address_is_injective.
Print Assumptions public_address_alone_never_authorizes_draw.
Print Assumptions retained_slot_capability_authorizes_draw.
Print Assumptions fund_slot_success_is_exact.
Print Assumptions fund_slot_success_is_conserving.
Print Assumptions fund_slot_is_not_minting.
Print Assumptions insufficient_sponsor_rejects_funding.
Print Assumptions continuation_requires_outer_commit.
Print Assumptions continuation_requires_retained_slot_capability.
Print Assumptions unauthenticated_gateway_cannot_consume_continuation.
Print Assumptions authenticated_gateway_refines_slot_settlement.
Print Assumptions continuation_insufficient_slot_rejects_atomically.
Print Assumptions continuation_realized_over_bound_rejects_atomically.
Print Assumptions continuation_insufficient_gateway_fee_rejects_atomically.
Print Assumptions continuation_success_uses_slot_and_separates_fee.
Print Assumptions continuation_success_refunds_unused_bound.
Print Assumptions continuation_success_is_conserving.
Print Assumptions wallet_funding_then_lollipop_is_conserving.
Print Assumptions wallet_funding_then_lollipop_is_exact.
Print Assumptions replay_uses_identical_authenticated_settlement.
