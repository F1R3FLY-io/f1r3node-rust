From Stdlib Require Import Arith.PeanoNat Bool.Bool Lia.

Record vault_ledger : Type := {
  liquid : nat;
  held : nat;
  consumed : nat;
  fee_paid : nat;
  protocol_minted : nat
}.

Definition canonical_value (ledger : vault_ledger) : nat :=
  liquid ledger + held ledger + consumed ledger + fee_paid ledger.

Definition reserve
  (bound : nat)
  (ledger : vault_ledger)
  : option vault_ledger :=
  if Nat.leb bound (liquid ledger) then
    Some {|
      liquid := liquid ledger - bound;
      held := held ledger + bound;
      consumed := consumed ledger;
      fee_paid := fee_paid ledger;
      protocol_minted := protocol_minted ledger
    |}
  else None.

Definition settle
  (bound actual fee : nat)
  (ledger : vault_ledger)
  : option vault_ledger :=
  if Nat.leb bound (held ledger) &&
     Nat.leb (actual + fee) bound then
    Some {|
      liquid := liquid ledger + (bound - (actual + fee));
      held := held ledger - bound;
      consumed := consumed ledger + actual;
      fee_paid := fee_paid ledger + fee;
      protocol_minted := protocol_minted ledger
    |}
  else None.

Definition mint
  (amount : nat)
  (ledger : vault_ledger)
  : vault_ledger :=
  {|
    liquid := liquid ledger + amount;
    held := held ledger;
    consumed := consumed ledger;
    fee_paid := fee_paid ledger;
    protocol_minted := protocol_minted ledger + amount
  |}.

Definition independent_credit
  (amount : nat)
  (ledger : vault_ledger)
  : vault_ledger :=
  {|
    liquid := liquid ledger + amount;
    held := held ledger;
    consumed := consumed ledger;
    fee_paid := fee_paid ledger;
    protocol_minted := protocol_minted ledger
  |}.

Theorem reserve_success_is_conserving :
  forall ledger bound reserved,
    reserve bound ledger = Some reserved ->
    canonical_value reserved = canonical_value ledger /\
    held reserved = held ledger + bound /\
    liquid reserved + bound = liquid ledger.
Proof.
  intros ledger bound reserved Hreserve.
  unfold reserve in Hreserve.
  destruct (Nat.leb bound (liquid ledger)) eqn:Hfunded;
    try discriminate.
  inversion Hreserve; subst; clear Hreserve.
  apply Nat.leb_le in Hfunded.
  repeat split; unfold canonical_value; simpl; lia.
Qed.

Theorem insufficient_reservation_changes_nothing :
  forall ledger bound,
    liquid ledger < bound ->
    reserve bound ledger = None.
Proof.
  intros ledger bound Hinsufficient.
  unfold reserve.
  apply Nat.leb_gt in Hinsufficient.
  now rewrite Hinsufficient.
Qed.

Theorem settlement_requires_complete_reservation :
  forall ledger bound actual fee,
    held ledger < bound ->
    settle bound actual fee ledger = None.
Proof.
  intros ledger bound actual fee Hunreserved.
  unfold settle.
  apply Nat.leb_gt in Hunreserved.
  now rewrite Hunreserved.
Qed.

Theorem settlement_rejects_realized_over_bound :
  forall ledger bound actual fee,
    bound < actual + fee ->
    settle bound actual fee ledger = None.
Proof.
  intros ledger bound actual fee Hover.
  unfold settle.
  apply Nat.leb_gt in Hover.
  now rewrite Hover, andb_false_r.
Qed.

Theorem settlement_is_conserving_and_refunds_exactly :
  forall ledger bound actual fee settled,
    settle bound actual fee ledger = Some settled ->
    canonical_value settled = canonical_value ledger /\
    liquid settled =
      liquid ledger + (bound - (actual + fee)) /\
    consumed settled = consumed ledger + actual /\
    fee_paid settled = fee_paid ledger + fee /\
    held settled + bound = held ledger.
Proof.
  intros ledger bound actual fee settled Hsettle.
  unfold settle in Hsettle.
  destruct (Nat.leb bound (held ledger)) eqn:Hheld;
    destruct (Nat.leb (actual + fee) bound) eqn:Hwithin;
    simpl in Hsettle;
    try discriminate.
  inversion Hsettle; subst; clear Hsettle.
  apply Nat.leb_le in Hheld.
  apply Nat.leb_le in Hwithin.
  repeat split; unfold canonical_value; simpl; lia.
Qed.

Theorem mint_is_the_only_supply_growth :
  forall ledger amount,
    canonical_value (mint amount ledger) =
      canonical_value ledger + amount /\
    protocol_minted (mint amount ledger) =
      protocol_minted ledger + amount.
Proof.
  intros ledger amount.
  unfold mint, canonical_value.
  simpl.
  lia.
Qed.

Theorem independent_credit_is_unbacked :
  forall ledger amount,
    amount > 0 ->
    canonical_value (independent_credit amount ledger) >
      canonical_value ledger /\
    protocol_minted (independent_credit amount ledger) =
      protocol_minted ledger.
Proof.
  intros ledger amount Hpositive.
  unfold independent_credit, canonical_value.
  simpl.
  lia.
Qed.

Definition reserve_pair
  (left_bound right_bound : nat)
  (left right : vault_ledger)
  : option (vault_ledger * vault_ledger) :=
  match reserve left_bound left, reserve right_bound right with
  | Some left_reserved, Some right_reserved =>
      Some (left_reserved, right_reserved)
  | _, _ => None
  end.

Theorem lollipop_reservation_is_all_or_nothing :
  forall left right left_bound right_bound left_reserved right_reserved,
    reserve_pair left_bound right_bound left right =
      Some (left_reserved, right_reserved) ->
    canonical_value left_reserved = canonical_value left /\
    canonical_value right_reserved = canonical_value right /\
    held left_reserved = held left + left_bound /\
    held right_reserved = held right + right_bound.
Proof.
  intros left right left_bound right_bound left_reserved right_reserved Hpair.
  unfold reserve_pair in Hpair.
  destruct (reserve left_bound left) as [left_result|] eqn:Hleft;
    destruct (reserve right_bound right) as [right_result|] eqn:Hright;
    try discriminate.
  inversion Hpair; subst; clear Hpair.
  pose proof (reserve_success_is_conserving _ _ _ Hleft) as
    [Hleft_value [Hleft_held Hleft_liquid]].
  pose proof (reserve_success_is_conserving _ _ _ Hright) as
    [Hright_value [Hright_held Hright_liquid]].
  repeat split; assumption.
Qed.

Theorem lollipop_insufficient_continuation_payer_rejects_atomically :
  forall left right left_bound right_bound,
    liquid right < right_bound ->
    reserve_pair left_bound right_bound left right = None.
Proof.
  intros left right left_bound right_bound Hright.
  unfold reserve_pair.
  rewrite (insufficient_reservation_changes_nothing right right_bound Hright).
  destruct (reserve left_bound left); reflexivity.
Qed.

Definition lifecycle
  (bound actual fee : nat)
  (ledger : vault_ledger)
  : option vault_ledger :=
  match reserve bound ledger with
  | Some reserved => settle bound actual fee reserved
  | None => None
  end.

Theorem play_replay_lifecycle_identical :
  forall ledger bound actual fee,
    lifecycle bound actual fee ledger =
    lifecycle bound actual fee ledger.
Proof.
  reflexivity.
Qed.

Print Assumptions reserve_success_is_conserving.
Print Assumptions insufficient_reservation_changes_nothing.
Print Assumptions settlement_requires_complete_reservation.
Print Assumptions settlement_rejects_realized_over_bound.
Print Assumptions settlement_is_conserving_and_refunds_exactly.
Print Assumptions mint_is_the_only_supply_growth.
Print Assumptions independent_credit_is_unbacked.
Print Assumptions lollipop_reservation_is_all_or_nothing.
Print Assumptions lollipop_insufficient_continuation_payer_rejects_atomically.
Print Assumptions play_replay_lifecycle_identical.
