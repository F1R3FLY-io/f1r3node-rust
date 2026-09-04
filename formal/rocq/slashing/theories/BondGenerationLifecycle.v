From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Slashing Require Import ValidatorLifetime.

Set Implicit Arguments.

Inductive BondLifecyclePhase : Type :=
| NeverBonded
| BondedPhase
| PendingWithdrawPhase
| WithdrawingPhase
| WithdrawnPhase
| QuarantinedPhase
| BurnedPhase.

Record ValidatorBondLifecycle : Type := mkValidatorBondLifecycle {
  lifecycle_generation : option BondGeneration;
  lifecycle_successful_bonds : nat;
  lifecycle_phase : BondLifecyclePhase
}.

Definition generation_matches_successful_bonds
  (state : ValidatorBondLifecycle) : Prop :=
  lifecycle_generation state =
    match lifecycle_successful_bonds state with
    | 0 => None
    | S previous => Some previous
    end.

Definition next_lifecycle_generation
  (state : ValidatorBondLifecycle) : option BondGeneration :=
  match lifecycle_generation state with
  | None => Some 0
  | Some generation => checked_next_generation generation
  end.

Definition fresh_bond
  (state : ValidatorBondLifecycle) : option ValidatorBondLifecycle :=
  match lifecycle_phase state with
  | NeverBonded | WithdrawnPhase =>
      match next_lifecycle_generation state with
      | Some generation =>
          Some (mkValidatorBondLifecycle
            (Some generation)
            (S (lifecycle_successful_bonds state))
            BondedPhase)
      | None => None
      end
  | _ => None
  end.

Definition request_withdraw
  (state : ValidatorBondLifecycle) : ValidatorBondLifecycle :=
  match lifecycle_phase state with
  | BondedPhase =>
      mkValidatorBondLifecycle
        (lifecycle_generation state)
        (lifecycle_successful_bonds state)
        PendingWithdrawPhase
  | _ => state
  end.

Definition begin_withdraw
  (state : ValidatorBondLifecycle) : ValidatorBondLifecycle :=
  match lifecycle_phase state with
  | PendingWithdrawPhase =>
      mkValidatorBondLifecycle
        (lifecycle_generation state)
        (lifecycle_successful_bonds state)
        WithdrawingPhase
  | _ => state
  end.

Definition complete_withdraw
  (state : ValidatorBondLifecycle) : ValidatorBondLifecycle :=
  match lifecycle_phase state with
  | WithdrawingPhase =>
      mkValidatorBondLifecycle
        (lifecycle_generation state)
        (lifecycle_successful_bonds state)
        WithdrawnPhase
  | _ => state
  end.

Definition slash_lifecycle
  (state : ValidatorBondLifecycle)
  (target_generation : BondGeneration)
  : ValidatorBondLifecycle * bool :=
  match lifecycle_generation state with
  | Some current_generation =>
      if Nat.eq_dec target_generation current_generation then
        match lifecycle_phase state with
        | BondedPhase | PendingWithdrawPhase | WithdrawingPhase =>
            (mkValidatorBondLifecycle
              (lifecycle_generation state)
              (lifecycle_successful_bonds state)
              QuarantinedPhase,
             true)
        | QuarantinedPhase => (state, true)
        | _ => (state, false)
        end
      else (state, false)
  | None => (state, false)
  end.

Definition advance_epoch
  (state : ValidatorBondLifecycle) : ValidatorBondLifecycle := state.

Theorem initial_lifecycle_is_well_formed :
  generation_matches_successful_bonds
    (mkValidatorBondLifecycle None 0 NeverBonded).
Proof.
  reflexivity.
Qed.

Theorem first_fresh_bond_uses_generation_zero :
  fresh_bond (mkValidatorBondLifecycle None 0 NeverBonded) =
    Some (mkValidatorBondLifecycle (Some 0) 1 BondedPhase).
Proof.
  reflexivity.
Qed.

Theorem fresh_bond_preserves_generation_count_relation :
  forall state next,
    generation_matches_successful_bonds state ->
    fresh_bond state = Some next ->
    generation_matches_successful_bonds next.
Proof.
  intros [generation successful_bonds phase] next Hrelation Hbond.
  unfold generation_matches_successful_bonds in *.
  unfold fresh_bond, next_lifecycle_generation in Hbond.
  destruct phase;
  destruct generation as [generation |];
  destruct successful_bonds as [| previous];
  simpl in *;
  try discriminate.
  all: try solve [inversion Hbond; subst; reflexivity].
  all: inversion Hrelation; subst generation.
  all: unfold checked_next_generation in Hbond.
  all: destruct (Nat.ltb previous max_bond_generation);
    inversion Hbond; subst; reflexivity.
Qed.

Theorem successful_rebond_strictly_increases_generation :
  forall generation successful_bonds next,
    fresh_bond
      (mkValidatorBondLifecycle
        (Some generation) successful_bonds WithdrawnPhase) = Some next ->
    generation <
      match lifecycle_generation next with
      | Some next_generation => next_generation
      | None => 0
      end.
Proof.
  intros generation successful_bonds next Hbond.
  unfold fresh_bond, next_lifecycle_generation in Hbond. simpl in Hbond.
  destruct (checked_next_generation generation) as [next_generation |]
    eqn:Hnext; [|discriminate].
  inversion Hbond; subst; simpl.
  apply fresh_bond_generation_strictly_increases with (next := next_generation).
  assumption.
Qed.

Theorem generation_changes_only_on_successful_fresh_bond :
  forall state,
    lifecycle_generation (request_withdraw state) =
      lifecycle_generation state /\
    lifecycle_generation (begin_withdraw state) =
      lifecycle_generation state /\
    lifecycle_generation (complete_withdraw state) =
      lifecycle_generation state /\
    lifecycle_generation (advance_epoch state) =
      lifecycle_generation state.
Proof.
  intros [generation successful_bonds phase].
  destruct phase; repeat split; reflexivity.
Qed.

Theorem epoch_advance_preserves_bond_generation :
  forall state,
    lifecycle_generation (advance_epoch state) = lifecycle_generation state.
Proof.
  reflexivity.
Qed.

Theorem stale_generation_slash_is_noninterfering :
  forall state current_generation target_generation,
    lifecycle_generation state = Some current_generation ->
    target_generation <> current_generation ->
    slash_lifecycle state target_generation = (state, false).
Proof.
  intros [generation successful_bonds phase]
         current_generation target_generation Hcurrent Hstale.
  simpl in Hcurrent. subst generation.
  unfold slash_lifecycle. simpl.
  destruct (Nat.eq_dec target_generation current_generation);
    [contradiction | reflexivity].
Qed.

Theorem current_generation_locked_slash_quarantines :
  forall generation successful_bonds phase,
    phase = BondedPhase \/
    phase = PendingWithdrawPhase \/
    phase = WithdrawingPhase ->
    slash_lifecycle
      (mkValidatorBondLifecycle
        (Some generation) successful_bonds phase)
      generation =
    (mkValidatorBondLifecycle
      (Some generation) successful_bonds QuarantinedPhase,
     true).
Proof.
  intros generation successful_bonds phase Hphase.
  destruct Hphase as [Hphase | [Hphase | Hphase]];
    subst phase.
  all: unfold slash_lifecycle; simpl.
  all: destruct (Nat.eq_dec generation generation) as [_ | Hbad].
  all: try reflexivity.
  all: contradiction.
Qed.

Theorem unavailable_next_generation_rejects_rebond :
  forall generation successful_bonds,
    checked_next_generation generation = None ->
    fresh_bond
      (mkValidatorBondLifecycle
        (Some generation) successful_bonds WithdrawnPhase) = None.
Proof.
  intros generation successful_bonds Hexhausted.
  unfold checked_next_generation in Hexhausted.
  destruct (Nat.ltb generation max_bond_generation) eqn:Hbounded.
  - discriminate.
  - assert (Hnext :
      next_lifecycle_generation
        (mkValidatorBondLifecycle
          (Some generation) successful_bonds WithdrawnPhase) = None).
    {
      unfold next_lifecycle_generation.
      cbn [lifecycle_generation].
      unfold checked_next_generation.
      rewrite Hbounded. reflexivity.
    }
    unfold fresh_bond.
    cbn [lifecycle_phase].
    rewrite Hnext. reflexivity.
Qed.
