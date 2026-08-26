From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.

Inductive BondPhase : Type :=
| PhaseUnseen
| PhaseBonded
| PhasePendingWithdraw
| PhaseWithdrawing
| PhaseQuarantined
| PhaseWithdrawn
| PhaseBurned.

Record BondLifecycle : Type := mkBondLifecycle {
  lifecycle_phase : BondPhase;
  lifecycle_generation : option nat;
  lifecycle_live_generation : option nat;
  lifecycle_stake : nat;
  lifecycle_wallet : nat;
  lifecycle_cooperative : nat;
  lifecycle_burned : nat;
  lifecycle_successful_bonds : nat;
  lifecycle_quarantine_origin : option BondPhase;
  lifecycle_minting_halted : bool
}.

Inductive BondEvent : Type :=
| BondCommitted (generation : nat)
| BondDepositFailed
| WithdrawRequested
| WithdrawalStarted
| WithdrawalPayoutFailed
| WithdrawalPayoutCommitted
| SlashAttempted (target_generation : nat)
| SlashVindicated
| SlashGuilty (penalty : nat)
| SlashBurned.

Definition locked_phase (phase : BondPhase) : Prop :=
  phase = PhaseBonded \/
  phase = PhasePendingWithdraw \/
  phase = PhaseWithdrawing.

Definition fresh_bond_phase (phase : BondPhase) : Prop :=
  phase = PhaseUnseen \/ phase = PhaseWithdrawn.

Definition next_generation (generation : option nat) : nat :=
  match generation with
  | None => 0
  | Some current => S current
  end.

Definition max_bond_generation : nat := 9223372036854775807.

Definition fresh_bond_state
  (state : BondLifecycle) (amount generation : nat) : BondLifecycle :=
  mkBondLifecycle
    PhaseBonded
    (Some generation)
    (Some generation)
    (lifecycle_stake state + amount)
    (lifecycle_wallet state - amount)
    (lifecycle_cooperative state)
    (lifecycle_burned state)
    (S (lifecycle_successful_bonds state))
    None
    false.

Definition set_phase
  (state : BondLifecycle) (phase : BondPhase) : BondLifecycle :=
  mkBondLifecycle
    phase
    (lifecycle_generation state)
    (lifecycle_live_generation state)
    (lifecycle_stake state)
    (lifecycle_wallet state)
    (lifecycle_cooperative state)
    (lifecycle_burned state)
    (lifecycle_successful_bonds state)
    (lifecycle_quarantine_origin state)
    (lifecycle_minting_halted state).

Definition quarantine_state (state : BondLifecycle) : BondLifecycle :=
  mkBondLifecycle
    PhaseQuarantined
    (lifecycle_generation state)
    (lifecycle_live_generation state)
    (lifecycle_stake state)
    (lifecycle_wallet state)
    (lifecycle_cooperative state)
    (lifecycle_burned state)
    (lifecycle_successful_bonds state)
    (Some (lifecycle_phase state))
    true.

Definition vindicated_state
  (state : BondLifecycle) (origin : BondPhase) : BondLifecycle :=
  mkBondLifecycle
    origin
    (lifecycle_generation state)
    (lifecycle_live_generation state)
    (lifecycle_stake state)
    (lifecycle_wallet state)
    (lifecycle_cooperative state)
    (lifecycle_burned state)
    (lifecycle_successful_bonds state)
    None
    false.

Definition payout_state (state : BondLifecycle) : BondLifecycle :=
  mkBondLifecycle
    PhaseWithdrawn
    (lifecycle_generation state)
    None
    0
    (lifecycle_wallet state + lifecycle_stake state)
    (lifecycle_cooperative state)
    (lifecycle_burned state)
    (lifecycle_successful_bonds state)
    None
    false.

Definition guilty_state
  (state : BondLifecycle) (origin : BondPhase) (penalty : nat) : BondLifecycle :=
  let remainder := lifecycle_stake state - penalty in
  mkBondLifecycle
    origin
    (lifecycle_generation state)
    (lifecycle_live_generation state)
    remainder
    (lifecycle_wallet state)
    (lifecycle_cooperative state + penalty)
    (lifecycle_burned state)
    (lifecycle_successful_bonds state)
    None
    false.

Definition burned_state (state : BondLifecycle) : BondLifecycle :=
  mkBondLifecycle
    PhaseBurned
    (lifecycle_generation state)
    None
    0
    (lifecycle_wallet state)
    (lifecycle_cooperative state)
    (lifecycle_burned state + lifecycle_stake state)
    (lifecycle_successful_bonds state)
    None
    true.

Inductive lifecycle_step (amount : nat) :
  BondLifecycle -> BondEvent -> BondLifecycle -> Prop :=
| StepFreshBond : forall state generation,
    fresh_bond_phase (lifecycle_phase state) ->
    lifecycle_live_generation state = None ->
    amount > 0 ->
    amount <= lifecycle_wallet state ->
    generation = next_generation (lifecycle_generation state) ->
    generation <= max_bond_generation ->
    lifecycle_step amount state (BondCommitted generation)
      (fresh_bond_state state amount generation)
| StepDepositFailure : forall state,
    lifecycle_step amount state BondDepositFailed state
| StepRequestWithdraw : forall state,
    lifecycle_phase state = PhaseBonded ->
    lifecycle_step amount state WithdrawRequested
      (set_phase state PhasePendingWithdraw)
| StepBeginWithdrawal : forall state,
    lifecycle_phase state = PhasePendingWithdraw ->
    lifecycle_step amount state WithdrawalStarted
      (set_phase state PhaseWithdrawing)
| StepPayoutFailure : forall state,
    lifecycle_phase state = PhaseWithdrawing ->
    lifecycle_step amount state WithdrawalPayoutFailed state
| StepPayoutSuccess : forall state,
    lifecycle_phase state = PhaseWithdrawing ->
    lifecycle_step amount state WithdrawalPayoutCommitted (payout_state state)
| StepSlashCurrent : forall state target,
    lifecycle_generation state = Some target ->
    locked_phase (lifecycle_phase state) ->
    lifecycle_step amount state (SlashAttempted target) (quarantine_state state)
| StepSlashRetry : forall state target,
    lifecycle_generation state = Some target ->
    lifecycle_phase state = PhaseQuarantined ->
    lifecycle_step amount state (SlashAttempted target) state
| StepSlashStale : forall state target,
    lifecycle_generation state <> Some target ->
    lifecycle_step amount state (SlashAttempted target) state
| StepVindicated : forall state origin,
    lifecycle_phase state = PhaseQuarantined ->
    lifecycle_quarantine_origin state = Some origin ->
    locked_phase origin ->
    lifecycle_step amount state SlashVindicated (vindicated_state state origin)
| StepGuilty : forall state origin penalty,
    lifecycle_phase state = PhaseQuarantined ->
    lifecycle_quarantine_origin state = Some origin ->
    locked_phase origin ->
    penalty < lifecycle_stake state ->
    lifecycle_step amount state (SlashGuilty penalty) (guilty_state state origin penalty)
| StepBurned : forall state,
    lifecycle_phase state = PhaseQuarantined ->
    lifecycle_step amount state SlashBurned (burned_state state).

Definition generation_le
  (left right : option nat) : Prop :=
  match left, right with
  | None, _ => True
  | Some _, None => False
  | Some left_generation, Some right_generation =>
      left_generation <= right_generation
  end.

Definition lifecycle_total (state : BondLifecycle) : nat :=
  lifecycle_wallet state +
  lifecycle_stake state +
  lifecycle_cooperative state +
  lifecycle_burned state.

Definition lifecycle_well_formed (state : BondLifecycle) : Prop :=
  match lifecycle_phase state with
  | PhaseUnseen =>
      lifecycle_generation state = None /\
      lifecycle_live_generation state = None /\
      lifecycle_stake state = 0 /\
      lifecycle_quarantine_origin state = None
  | PhaseWithdrawn | PhaseBurned =>
      lifecycle_live_generation state = None /\
      lifecycle_stake state = 0 /\
      lifecycle_quarantine_origin state = None
  | PhaseBonded | PhasePendingWithdraw | PhaseWithdrawing =>
      lifecycle_live_generation state = lifecycle_generation state /\
      lifecycle_generation state <> None /\
      lifecycle_stake state > 0 /\
      lifecycle_quarantine_origin state = None
  | PhaseQuarantined =>
      lifecycle_live_generation state = lifecycle_generation state /\
      lifecycle_generation state <> None /\
      lifecycle_stake state > 0 /\
      exists origin,
        lifecycle_quarantine_origin state = Some origin /\
        locked_phase origin
  end.

Lemma locked_phase_well_formed_components :
  forall state,
    lifecycle_well_formed state ->
    locked_phase (lifecycle_phase state) ->
    lifecycle_live_generation state = lifecycle_generation state /\
    lifecycle_generation state <> None /\
    lifecycle_stake state > 0 /\
    lifecycle_quarantine_origin state = None.
Proof.
  intros state Hwf Hlocked.
  unfold lifecycle_well_formed in Hwf.
  destruct (lifecycle_phase state); try exact Hwf;
    unfold locked_phase in Hlocked;
    destruct Hlocked as [Hlocked | [Hlocked | Hlocked]];
    discriminate.
Qed.

Theorem lifecycle_generation_monotone :
  forall amount state event next,
    lifecycle_step amount state event next ->
    generation_le
      (lifecycle_generation state)
      (lifecycle_generation next).
Proof.
  intros amount state event next Hstep.
  destruct Hstep; unfold generation_le; simpl;
    unfold next_generation in *;
    destruct (lifecycle_generation state); simpl in *; auto; lia.
Qed.

Theorem generation_changes_only_on_fresh_bond :
  forall amount state event next,
    lifecycle_step amount state event next ->
    (lifecycle_generation state <> lifecycle_generation next ->
      exists generation, event = BondCommitted generation).
Proof.
  intros amount state event next Hstep Hchanged.
  inversion Hstep; subst; simpl in Hchanged; try contradiction.
  eexists; reflexivity.
Qed.

Theorem fresh_bond_increments_exactly_once :
  forall amount state generation next,
    lifecycle_step amount state (BondCommitted generation) next ->
    generation = next_generation (lifecycle_generation state) /\
    lifecycle_generation next = Some generation /\
    lifecycle_successful_bonds next =
      S (lifecycle_successful_bonds state).
Proof.
  intros amount state generation next Hstep.
  inversion Hstep; subst; simpl; auto.
Qed.

Theorem fresh_bond_requires_no_live_generation :
  forall amount state generation next,
    lifecycle_step amount state (BondCommitted generation) next ->
    lifecycle_live_generation state = None /\
    lifecycle_phase state <> PhaseBurned.
Proof.
  intros amount state generation next Hstep.
  inversion Hstep; subst.
  split; auto.
  unfold fresh_bond_phase in H0. destruct H0; congruence.
Qed.

Theorem exhausted_generation_rejects_fresh_bond :
  forall amount state generation next,
    lifecycle_generation state = Some max_bond_generation ->
    lifecycle_step amount state (BondCommitted generation) next ->
    False.
Proof.
  intros amount state generation next Hexhausted Hstep.
  inversion Hstep; subst.
  unfold next_generation in *.
  rewrite Hexhausted in *.
  unfold max_bond_generation in *.
  lia.
Qed.

Theorem stale_slash_is_noninterfering :
  forall amount state target next,
    lifecycle_generation state <> Some target ->
    lifecycle_step amount state (SlashAttempted target) next ->
    next = state.
Proof.
  intros amount state target next Hstale Hstep.
  inversion Hstep; subst; try reflexivity; congruence.
Qed.

Theorem current_locked_slash_enters_quarantine :
  forall amount state target next,
    lifecycle_generation state = Some target ->
    locked_phase (lifecycle_phase state) ->
    lifecycle_step amount state (SlashAttempted target) next ->
    next = quarantine_state state.
Proof.
  intros amount state target next Hgeneration Hlocked Hstep.
  inversion Hstep; subst; try reflexivity.
  - unfold locked_phase in Hlocked.
    destruct Hlocked as [Hlocked | [Hlocked | Hlocked]]; congruence.
  - congruence.
Qed.

Theorem same_generation_quarantine_slash_is_idempotent :
  forall amount state target next,
    lifecycle_phase state = PhaseQuarantined ->
    lifecycle_generation state = Some target ->
    lifecycle_step amount state (SlashAttempted target) next ->
    next = state.
Proof.
  intros amount state target next Hphase Hgeneration Hstep.
  inversion Hstep; subst; try reflexivity.
  - match goal with
    | Hlocked : locked_phase _ |- _ =>
        unfold locked_phase in Hlocked;
        destruct Hlocked as [Hlocked | [Hlocked | Hlocked]];
        congruence
    end.
Qed.

Theorem lifecycle_step_preserves_value :
  forall amount state event next,
    lifecycle_step amount state event next ->
    lifecycle_total next = lifecycle_total state.
Proof.
  intros amount state event next Hstep.
  inversion Hstep; subst; unfold lifecycle_total;
    simpl; try lia.
Qed.

Theorem lifecycle_well_formed_preserved :
  forall amount state event next,
    lifecycle_well_formed state ->
    lifecycle_step amount state event next ->
    lifecycle_well_formed next.
Proof.
  intros amount state event next Hwf Hstep.
  destruct Hstep.
  - unfold lifecycle_well_formed, fresh_bond_state; simpl.
    repeat split; auto; try congruence; lia.
  - exact Hwf.
  - unfold lifecycle_well_formed, set_phase; simpl.
    unfold lifecycle_well_formed in Hwf. rewrite H in Hwf. exact Hwf.
  - unfold lifecycle_well_formed, set_phase; simpl.
    unfold lifecycle_well_formed in Hwf. rewrite H in Hwf. exact Hwf.
  - exact Hwf.
  - unfold lifecycle_well_formed, payout_state; simpl. auto.
  - unfold lifecycle_well_formed, quarantine_state; simpl.
    pose proof (locked_phase_well_formed_components state Hwf H0)
      as [Hlive [Hgeneration [Hstake Horigin]]].
    repeat split; auto.
    exists (lifecycle_phase state). split; auto.
  - exact Hwf.
  - exact Hwf.
  - unfold lifecycle_well_formed, vindicated_state; simpl.
    unfold lifecycle_well_formed in Hwf. rewrite H in Hwf.
    destruct Hwf as [Hlive [Hgeneration [Hstake Horigin]]].
    destruct H1 as [Hlocked | [Hlocked | Hlocked]];
      subst origin; simpl; repeat split; auto.
  - unfold lifecycle_well_formed, guilty_state; simpl.
    unfold lifecycle_well_formed in Hwf. rewrite H in Hwf.
    destruct Hwf as [Hlive [Hgeneration [Hstake Horigin]]].
    destruct H1 as [Hlocked | [Hlocked | Hlocked]];
      subst origin; simpl; repeat split; auto; lia.
  - unfold lifecycle_well_formed, burned_state; simpl. auto.
Qed.

Theorem vindication_restores_exact_pre_slash_phase :
  forall amount state next origin,
    lifecycle_quarantine_origin state = Some origin ->
    lifecycle_step amount state SlashVindicated next ->
    lifecycle_phase next = origin.
Proof.
  intros amount state next origin Horigin Hstep.
  inversion Hstep; subst; simpl; congruence.
Qed.

Theorem partial_penalty_restores_exact_pre_slash_phase :
  forall amount state next origin penalty,
    lifecycle_quarantine_origin state = Some origin ->
    penalty < lifecycle_stake state ->
    lifecycle_step amount state (SlashGuilty penalty) next ->
    lifecycle_phase next = origin.
Proof.
  intros amount state next origin penalty Horigin Hremainder Hstep.
  inversion Hstep; subst; try discriminate.
  unfold guilty_state; simpl. congruence.
Qed.

Theorem guilty_resolution_is_strictly_partial :
  forall amount state next penalty,
    lifecycle_step amount state (SlashGuilty penalty) next ->
    penalty < lifecycle_stake state /\
    lifecycle_stake next > 0.
Proof.
  intros amount state next penalty Hstep.
  inversion Hstep; subst; try discriminate.
  unfold guilty_state; simpl. split; lia.
Qed.

Print Assumptions lifecycle_generation_monotone.
Print Assumptions exhausted_generation_rejects_fresh_bond.
Print Assumptions lifecycle_step_preserves_value.
Print Assumptions lifecycle_well_formed_preserved.
Print Assumptions vindication_restores_exact_pre_slash_phase.
Print Assumptions partial_penalty_restores_exact_pre_slash_phase.
Print Assumptions guilty_resolution_is_strictly_partial.
