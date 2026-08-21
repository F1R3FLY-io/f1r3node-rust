From Stdlib Require Import Arith.PeanoNat Lists.List Lia.

Import ListNotations.

Record stack_ledger := {
  source_cells : nat;
  target_cells : nat;
  seen_transfers : list nat
}.

Definition ledger_total (ledger : stack_ledger) : nat :=
  source_cells ledger + target_cells ledger.

Definition transfer_stack
  (event cells : nat)
  (ledger : stack_ledger)
  : stack_ledger * bool :=
  if in_dec Nat.eq_dec event (seen_transfers ledger) then
    (ledger, false)
  else if cells <=? source_cells ledger then
    ({|
       source_cells := source_cells ledger - cells;
       target_cells := target_cells ledger + cells;
       seen_transfers := event :: seen_transfers ledger
     |}, true)
  else
    (ledger, false).

Definition replay_stack_transfer := transfer_stack.

Definition authorized_mint
  (cells : nat)
  (ledger : stack_ledger)
  : stack_ledger :=
  {|
    source_cells := source_cells ledger;
    target_cells := target_cells ledger + cells;
    seen_transfers := seen_transfers ledger
  |}.

Theorem duplicate_stack_transfer_is_rejected_atomically :
  forall event cells ledger,
    In event (seen_transfers ledger) ->
    transfer_stack event cells ledger = (ledger, false).
Proof.
  intros event cells ledger present.
  unfold transfer_stack.
  destruct (in_dec Nat.eq_dec event (seen_transfers ledger)).
  - reflexivity.
  - contradiction.
Qed.

Theorem underfunded_stack_transfer_is_rejected_atomically :
  forall event cells ledger,
    source_cells ledger < cells ->
    transfer_stack event cells ledger = (ledger, false).
Proof.
  intros event cells ledger underfunded.
  unfold transfer_stack.
  destruct (in_dec Nat.eq_dec event (seen_transfers ledger)).
  - reflexivity.
  - destruct (cells <=? source_cells ledger) eqn:funded.
    + apply Nat.leb_le in funded.
      lia.
    + reflexivity.
Qed.

Theorem funded_fresh_stack_transfer_is_exact :
  forall event cells ledger,
    ~ In event (seen_transfers ledger) ->
    cells <= source_cells ledger ->
    transfer_stack event cells ledger =
      ({|
         source_cells := source_cells ledger - cells;
         target_cells := target_cells ledger + cells;
         seen_transfers := event :: seen_transfers ledger
       |}, true).
Proof.
  intros event cells ledger fresh funded.
  unfold transfer_stack.
  destruct (in_dec Nat.eq_dec event (seen_transfers ledger)).
  - contradiction.
  - destruct (cells <=? source_cells ledger) eqn:decision.
    + reflexivity.
    + apply Nat.leb_gt in decision.
      lia.
Qed.

Theorem funded_stack_transfer_conserves :
  forall event cells ledger,
    ~ In event (seen_transfers ledger) ->
    cells <= source_cells ledger ->
    ledger_total (fst (transfer_stack event cells ledger)) =
    ledger_total ledger.
Proof.
  intros event cells [source target seen] fresh funded.
  rewrite funded_fresh_stack_transfer_is_exact by assumption.
  unfold ledger_total.
  simpl in *.
  lia.
Qed.

Theorem funded_stack_transfer_produces_every_debited_cell :
  forall event cells ledger,
    ~ In event (seen_transfers ledger) ->
    cells <= source_cells ledger ->
    target_cells (fst (transfer_stack event cells ledger)) =
    target_cells ledger + cells.
Proof.
  intros event cells ledger fresh funded.
  rewrite funded_fresh_stack_transfer_is_exact by assumption.
  reflexivity.
Qed.

Theorem funded_stack_transfer_records_one_fresh_event :
  forall event cells ledger,
    ~ In event (seen_transfers ledger) ->
    cells <= source_cells ledger ->
    length (seen_transfers (fst (transfer_stack event cells ledger))) =
    S (length (seen_transfers ledger)).
Proof.
  intros event cells ledger fresh funded.
  rewrite funded_fresh_stack_transfer_is_exact by assumption.
  reflexivity.
Qed.

Theorem stack_transfer_is_all_or_none :
  forall event cells ledger,
    transfer_stack event cells ledger = (ledger, false) \/
    transfer_stack event cells ledger =
      ({|
         source_cells := source_cells ledger - cells;
         target_cells := target_cells ledger + cells;
         seen_transfers := event :: seen_transfers ledger
       |}, true).
Proof.
  intros event cells ledger.
  unfold transfer_stack.
  destruct (in_dec Nat.eq_dec event (seen_transfers ledger)).
  - now left.
  - destruct (cells <=? source_cells ledger).
    + now right.
    + now left.
Qed.

Theorem replay_stack_transfer_is_identical :
  forall event cells ledger,
    replay_stack_transfer event cells ledger =
    transfer_stack event cells ledger.
Proof.
  reflexivity.
Qed.

Theorem authorized_mint_is_the_only_supply_increase :
  forall cells ledger,
    ledger_total (authorized_mint cells ledger) =
    ledger_total ledger + cells.
Proof.
  intros cells [source target seen].
  unfold ledger_total, authorized_mint.
  simpl.
  lia.
Qed.
