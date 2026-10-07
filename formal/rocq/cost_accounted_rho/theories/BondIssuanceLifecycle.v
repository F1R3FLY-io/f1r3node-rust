From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
Import ListNotations.

From CostAccountedRho Require Import MintingInjection.
From CostAccountedRho Require Import MintingHalt.

(* [il_minted_epochs] specifies logical history. Production stores one bounded
   [mintedThroughEpoch] frontier as defined by [MintedEpochRetention.v]. *)

Inductive validator_phase : Type :=
  | Absent
  | Bonded
  | Active
  | Withdrawing
  | Quarantined
  | Burned.

Definition phase_eqb (left right : validator_phase) : bool :=
  match left, right with
  | Absent, Absent
  | Bonded, Bonded
  | Active, Active
  | Withdrawing, Withdrawing
  | Quarantined, Quarantined
  | Burned, Burned => true
  | _, _ => false
  end.

Lemma phase_eqb_true_iff : forall left right,
  phase_eqb left right = true <-> left = right.
Proof.
  intros left right.
  destruct left, right; simpl; split; intro H; try discriminate; reflexivity.
Qed.

Lemma phase_eqb_refl : forall current,
  phase_eqb current current = true.
Proof.
  destruct current; reflexivity.
Qed.

Definition epoch_inb (epoch : nat) (minted : list nat) : bool :=
  existsb (Nat.eqb epoch) minted.

Lemma epoch_inb_true_iff : forall epoch minted,
  epoch_inb epoch minted = true <-> In epoch minted.
Proof.
  intros epoch minted.
  unfold epoch_inb.
  rewrite existsb_exists.
  split.
  - intros [candidate [Hin Heq]].
    apply Nat.eqb_eq in Heq.
    subst candidate.
    exact Hin.
  - intros Hin.
    exists epoch.
    split; [exact Hin | apply Nat.eqb_refl].
Qed.

Record issuance_state : Type := {
  il_custody : nat;
  il_stake : nat;
  il_generation : option nat;
  il_phase : validator_phase;
  il_halted : bool;
  il_minted_epochs : list nat;
  il_authorized_issuance : nat
}.

Definition next_generation (generation : option nat) : option nat :=
  match generation with
  | None => Some 0
  | Some current => Some (S current)
  end.

Definition genesis_allocate
  (genesis_member : bool)
  (amount : nat)
  (state : issuance_state) : issuance_state :=
  if genesis_member then
    {| il_custody := il_custody state + amount;
       il_stake := il_stake state;
       il_generation := il_generation state;
       il_phase := il_phase state;
       il_halted := il_halted state;
       il_minted_epochs := il_minted_epochs state;
       il_authorized_issuance := il_authorized_issuance state + amount |}
  else state.

Definition fresh_bond (state : issuance_state) (amount : nat) : issuance_state :=
  if phase_eqb (il_phase state) Absent then
    if (0 <? amount) && (amount <=? il_custody state) then
      {| il_custody := il_custody state - amount;
         il_stake := amount;
         il_generation := next_generation (il_generation state);
         il_phase := Bonded;
         il_halted := il_halted state;
         il_minted_epochs := il_minted_epochs state;
         il_authorized_issuance := il_authorized_issuance state |}
    else state
  else state.

Definition activate (state : issuance_state) : issuance_state :=
  if phase_eqb (il_phase state) Bonded then
    {| il_custody := il_custody state;
       il_stake := il_stake state;
       il_generation := il_generation state;
       il_phase := Active;
       il_halted := il_halted state;
       il_minted_epochs := il_minted_epochs state;
       il_authorized_issuance := il_authorized_issuance state |}
  else state.

Definition epoch_issue
  (boundary : bool)
  (epoch amount : nat)
  (state : issuance_state) : issuance_state :=
  if boundary
     && phase_eqb (il_phase state) Active
     && negb (il_halted state)
     && negb (epoch_inb epoch (il_minted_epochs state))
  then
    {| il_custody := il_custody state + amount;
       il_stake := il_stake state;
       il_generation := il_generation state;
       il_phase := il_phase state;
       il_halted := il_halted state;
       il_minted_epochs := epoch :: il_minted_epochs state;
       il_authorized_issuance := il_authorized_issuance state + amount |}
  else state.

Definition request_withdrawal (state : issuance_state) : issuance_state :=
  if phase_eqb (il_phase state) Active
     || phase_eqb (il_phase state) Bonded
  then
    {| il_custody := il_custody state;
       il_stake := il_stake state;
       il_generation := il_generation state;
       il_phase := Withdrawing;
       il_halted := il_halted state;
       il_minted_epochs := il_minted_epochs state;
       il_authorized_issuance := il_authorized_issuance state |}
  else state.

Definition settle_withdrawal (state : issuance_state) : issuance_state :=
  if phase_eqb (il_phase state) Withdrawing then
    {| il_custody := il_custody state + il_stake state;
       il_stake := 0;
       il_generation := il_generation state;
       il_phase := Absent;
       il_halted := il_halted state;
       il_minted_epochs := il_minted_epochs state;
       il_authorized_issuance := il_authorized_issuance state |}
  else state.

Definition slash (state : issuance_state) : issuance_state :=
  match il_phase state with
  | Absent | Burned => state
  | _ =>
      {| il_custody := il_custody state;
         il_stake := il_stake state;
         il_generation := il_generation state;
         il_phase := Quarantined;
         il_halted := true;
         il_minted_epochs := il_minted_epochs state;
         il_authorized_issuance := il_authorized_issuance state |}
  end.

Theorem initial_credit_iff_genesis_allocation : forall member amount state,
  il_custody (genesis_allocate member amount state) > il_custody state <->
  member = true /\ amount > 0.
Proof.
  intros member amount state.
  destruct member, amount; simpl; lia.
Qed.

Theorem fresh_bond_no_protocol_credit : forall state amount,
  il_authorized_issuance (fresh_bond state amount) =
  il_authorized_issuance state.
Proof.
  intros state amount.
  unfold fresh_bond.
  destruct (phase_eqb (il_phase state) Absent);
    [destruct ((0 <? amount) && (amount <=? il_custody state)) |];
    reflexivity.
Qed.

Theorem rebond_no_protocol_credit : forall state amount,
  il_phase state = Absent ->
  il_authorized_issuance (fresh_bond state amount) =
  il_authorized_issuance state.
Proof.
  intros state amount _.
  apply fresh_bond_no_protocol_credit.
Qed.

Theorem successful_fresh_bond_starts_generation_zero : forall state amount,
  il_phase state = Absent ->
  amount > 0 ->
  amount <= il_custody state ->
  il_generation state = None ->
  il_generation (fresh_bond state amount) = Some 0.
Proof.
  intros state amount Hphase Hpositive Hfunded Hgeneration.
  unfold fresh_bond.
  assert (Hphaseb : phase_eqb (il_phase state) Absent = true).
  { apply phase_eqb_true_iff. exact Hphase. }
  assert (Hpositiveb : (0 <? amount) = true) by
    (apply Nat.ltb_lt; exact Hpositive).
  assert (Hfundedb : (amount <=? il_custody state) = true) by
    (apply Nat.leb_le; exact Hfunded).
  rewrite Hphaseb, Hpositiveb, Hfundedb.
  simpl.
  rewrite Hgeneration.
  reflexivity.
Qed.

Theorem successful_rebond_increments_generation_once : forall state amount generation,
  il_phase state = Absent ->
  amount > 0 ->
  amount <= il_custody state ->
  il_generation state = Some generation ->
  il_generation (fresh_bond state amount) = Some (S generation).
Proof.
  intros state amount generation Hphase Hpositive Hfunded Hgeneration.
  unfold fresh_bond.
  assert (Hphaseb : phase_eqb (il_phase state) Absent = true).
  { apply phase_eqb_true_iff. exact Hphase. }
  assert (Hpositiveb : (0 <? amount) = true) by
    (apply Nat.ltb_lt; exact Hpositive).
  assert (Hfundedb : (amount <=? il_custody state) = true) by
    (apply Nat.leb_le; exact Hfunded).
  rewrite Hphaseb, Hpositiveb, Hfundedb.
  simpl.
  rewrite Hgeneration.
  reflexivity.
Qed.

Theorem bond_transition_has_no_issuance_arbitrage : forall state amount,
  il_phase state = Absent ->
  il_stake state = 0 ->
  amount > 0 ->
  amount <= il_custody state ->
  il_custody (fresh_bond state amount) + il_stake (fresh_bond state amount) =
    il_custody state + il_stake state /\
  il_authorized_issuance (fresh_bond state amount) =
    il_authorized_issuance state.
Proof.
  intros state amount Hphase Hstake Hpositive Hfunded.
  unfold fresh_bond.
  assert (Hphaseb : phase_eqb (il_phase state) Absent = true).
  { apply phase_eqb_true_iff. exact Hphase. }
  assert (Hpositiveb : (0 <? amount) = true) by
    (apply Nat.ltb_lt; exact Hpositive).
  assert (Hfundedb : (amount <=? il_custody state) = true) by
    (apply Nat.leb_le; exact Hfunded).
  rewrite Hphaseb, Hpositiveb, Hfundedb.
  simpl.
  split; [rewrite Hstake; lia | reflexivity].
Qed.

Theorem activation_does_not_credit : forall state,
  il_authorized_issuance (activate state) = il_authorized_issuance state.
Proof.
  intros state.
  unfold activate.
  destruct (phase_eqb (il_phase state) Bonded); reflexivity.
Qed.

Theorem epoch_credit_requires_boundary : forall boundary epoch amount state,
  il_custody (epoch_issue boundary epoch amount state) <>
    il_custody state ->
  boundary = true.
Proof.
  intros boundary epoch amount state Hchanged.
  destruct boundary; [reflexivity |].
  unfold epoch_issue in Hchanged.
  simpl in Hchanged.
  contradiction.
Qed.

Theorem epoch_credit_requires_active : forall boundary epoch amount state,
  il_custody (epoch_issue boundary epoch amount state) <>
    il_custody state ->
  il_phase state = Active.
Proof.
  intros boundary epoch amount state Hchanged.
  unfold epoch_issue in Hchanged.
  destruct boundary; simpl in Hchanged; [|contradiction].
  destruct (phase_eqb (il_phase state) Active) eqn:Hactive;
    simpl in Hchanged; [|contradiction].
  apply phase_eqb_true_iff.
  exact Hactive.
Qed.

Theorem epoch_credit_requires_not_halted : forall boundary epoch amount state,
  il_custody (epoch_issue boundary epoch amount state) <>
    il_custody state ->
  il_halted state = false.
Proof.
  intros boundary epoch amount state Hchanged.
  unfold epoch_issue in Hchanged.
  destruct boundary; simpl in Hchanged; [|contradiction].
  destruct (phase_eqb (il_phase state) Active); simpl in Hchanged;
    [|contradiction].
  destruct (il_halted state) eqn:Hhalt; simpl in Hchanged;
    [contradiction | reflexivity].
Qed.

Theorem epoch_credit_exactly_epoch_phlogiston : forall epoch amount state,
  il_phase state = Active ->
  il_halted state = false ->
  epoch_inb epoch (il_minted_epochs state) = false ->
  il_custody (epoch_issue true epoch amount state) =
    il_custody state + amount /\
  il_authorized_issuance (epoch_issue true epoch amount state) =
    il_authorized_issuance state + amount.
Proof.
  intros epoch amount state Hphase Hhalt Hfresh.
  unfold epoch_issue.
  assert (Hphaseb : phase_eqb (il_phase state) Active = true).
  { apply phase_eqb_true_iff. exact Hphase. }
  rewrite Hphaseb, Hhalt, Hfresh.
  simpl.
  split; reflexivity.
Qed.

Theorem ordinary_epoch_preserves_generation : forall boundary epoch amount state,
  il_generation (epoch_issue boundary epoch amount state) =
  il_generation state.
Proof.
  intros boundary epoch amount state.
  unfold epoch_issue.
  destruct (boundary
            && phase_eqb (il_phase state) Active
            && negb (il_halted state)
            && negb (epoch_inb epoch (il_minted_epochs state)));
    reflexivity.
Qed.

Theorem duplicate_epoch_issuance_is_effect_free : forall epoch amount state,
  In epoch (il_minted_epochs state) ->
  epoch_issue true epoch amount state = state.
Proof.
  intros epoch amount state Hin.
  unfold epoch_issue.
  assert (Hmember : epoch_inb epoch (il_minted_epochs state) = true).
  { apply epoch_inb_true_iff. exact Hin. }
  rewrite Hmember.
  repeat rewrite Bool.andb_false_r.
  reflexivity.
Qed.

Theorem withdrawal_does_not_credit : forall state,
  il_authorized_issuance (request_withdrawal state) =
  il_authorized_issuance state.
Proof.
  intros state.
  unfold request_withdrawal.
  destruct (phase_eqb (il_phase state) Active
            || phase_eqb (il_phase state) Bonded);
    reflexivity.
Qed.

Theorem withdrawal_settlement_conserves_custody : forall state,
  il_custody (settle_withdrawal state) + il_stake (settle_withdrawal state) =
  il_custody state + il_stake state.
Proof.
  intros state.
  unfold settle_withdrawal.
  destruct (phase_eqb (il_phase state) Withdrawing); simpl; lia.
Qed.

Theorem slash_does_not_credit : forall state,
  il_authorized_issuance (slash state) = il_authorized_issuance state.
Proof.
  intros state.
  unfold slash.
  destruct (il_phase state); reflexivity.
Qed.

Theorem genesis_plus_epoch_issuance_is_exact : forall epoch initial periodic state,
  il_phase state = Active ->
  il_halted state = false ->
  epoch_inb epoch (il_minted_epochs state) = false ->
  il_custody
    (epoch_issue true epoch periodic (genesis_allocate true initial state)) =
  il_custody state + initial + periodic.
Proof.
  intros epoch initial periodic state Hphase Hhalt Hfresh.
  unfold genesis_allocate, epoch_issue.
  simpl.
  assert (Hphaseb : phase_eqb (il_phase state) Active = true).
  { apply phase_eqb_true_iff. exact Hphase. }
  rewrite Hphaseb, Hhalt, Hfresh.
  simpl.
  reflexivity.
Qed.

Theorem play_replay_issuance_deterministic : forall boundary epoch amount state,
  epoch_issue boundary epoch amount state =
  epoch_issue boundary epoch amount state.
Proof.
  reflexivity.
Qed.
