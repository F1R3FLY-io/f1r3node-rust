From Stdlib Require Import List.
From Stdlib Require Import ZArith.
Import ListNotations.
Open Scope Z_scope.

From CostAccountedRho Require Import ValidatorEconomicsLifecycle.
From CostAccountedRho Require Import WalletNaming.
From CostAccountedRho Require Import RhoSyntax.

Inductive custody_role : Type :=
  | GeneralCustody
  | ValidatorFuelCustody.

Scheme Equality for custody_role.

Definition custody_location : Type := name * custody_role.

Definition location_of
  (key : pubkey)
  (role : custody_role) : custody_location :=
  (system_vault_name key, role).

Theorem custody_location_injective : forall key_a key_b role_a role_b,
  location_of key_a role_a = location_of key_b role_b ->
  key_a = key_b /\ role_a = role_b.
Proof.
  intros key_a key_b role_a role_b Heq.
  unfold location_of in Heq. injection Heq as Hname Hrole.
  split.
  - apply encode_bits_injective. exact Hname.
  - exact Hrole.
Qed.

Theorem custody_roles_are_disjoint : forall key_a key_b,
  location_of key_a GeneralCustody <>
  location_of key_b ValidatorFuelCustody.
Proof.
  intros key_a key_b Heq.
  apply custody_location_injective in Heq as [_ Hrole].
  discriminate.
Qed.

Inductive abstract_economic_event : Type :=
  | AbstractClientFee
  | AbstractFuelTopUp
  | AbstractValidatorHandler
  | AbstractBond
  | AbstractEpochIssue
  | AbstractWithdraw
  | AbstractSlash
  | AbstractRedeem.

Inductive native_economic_event : Type :=
  | NativeReserve (role : custody_role) (amount : Z)
  | NativeRefund (role : custody_role) (amount : Z)
  | NativeBurn (role : custody_role) (amount : Z)
  | NativeTransfer (source target : custody_role) (amount : Z)
  | NativeIssue (role : custody_role) (amount : Z)
  | NativeLifecycle (event : abstract_economic_event)
  | NativeCommit.

Definition visible_event
  (event : native_economic_event) : option abstract_economic_event :=
  match event with
  | NativeBurn ValidatorFuelCustody amount =>
      if Z.eqb amount handler_cost
      then Some AbstractValidatorHandler
      else None
  | NativeTransfer GeneralCustody GeneralCustody 1 =>
      Some AbstractClientFee
  | NativeTransfer GeneralCustody ValidatorFuelCustody amount =>
      if 0 <? amount then Some AbstractFuelTopUp else None
  | NativeIssue ValidatorFuelCustody amount =>
      if 0 <=? amount then Some AbstractEpochIssue else None
  | NativeLifecycle event => Some event
  | _ => None
  end.

Fixpoint hide_administration
  (trace : list native_economic_event) : list abstract_economic_event :=
  match trace with
  | [] => []
  | event :: tail =>
      match visible_event event with
      | Some visible => visible :: hide_administration tail
      | None => hide_administration tail
      end
  end.

Definition admitted_handler_trace : list native_economic_event :=
  [ NativeReserve ValidatorFuelCustody handler_cost;
    NativeBurn ValidatorFuelCustody handler_cost;
    NativeCommit ].

Definition rejected_handler_trace : list native_economic_event :=
  [ NativeReserve ValidatorFuelCustody handler_cost;
    NativeRefund ValidatorFuelCustody handler_cost ].

Definition deferred_handler_trace : list native_economic_event := [].

Theorem one_forcing_has_one_native_handler_settlement :
  hide_administration admitted_handler_trace =
  [AbstractValidatorHandler].
Proof.
  reflexivity.
Qed.

Theorem native_handler_settlement_has_exact_paper_grade : forall amount,
  visible_event (NativeBurn ValidatorFuelCustody amount) =
    Some AbstractValidatorHandler ->
  amount = handler_cost.
Proof.
  intros amount Hvisible. simpl in Hvisible.
  destruct (amount =? handler_cost) eqn:Heq; [| discriminate].
  apply Z.eqb_eq. exact Heq.
Qed.

Theorem every_handler_forcing_has_native_realization :
  exists native_trace,
    hide_administration native_trace = [AbstractValidatorHandler].
Proof.
  exists admitted_handler_trace.
  apply one_forcing_has_one_native_handler_settlement.
Qed.

Theorem rejected_deploy_has_no_handler_transition :
  hide_administration rejected_handler_trace = [].
Proof.
  reflexivity.
Qed.

Theorem deferred_deploy_has_no_handler_transition :
  hide_administration deferred_handler_trace = [].
Proof.
  reflexivity.
Qed.

Theorem administrative_prefix_is_weakly_hidden : forall role amount tail,
  hide_administration
    (NativeReserve role amount :: NativeRefund role amount :: tail) =
  hide_administration tail.
Proof.
  intros role amount tail. destruct role; reflexivity.
Qed.

Theorem commit_is_weakly_hidden : forall trace,
  hide_administration (trace ++ [NativeCommit]) =
  hide_administration trace.
Proof.
  induction trace as [| event tail IH].
  - reflexivity.
  - simpl. destruct (visible_event event); simpl; rewrite IH; reflexivity.
Qed.

Theorem general_fee_cannot_be_handler_fuel : forall amount,
  visible_event (NativeBurn GeneralCustody amount) <>
  Some AbstractValidatorHandler.
Proof.
  intros amount. simpl. discriminate.
Qed.

Theorem general_issue_cannot_refine_epoch_fuel : forall amount,
  visible_event (NativeIssue GeneralCustody amount) <>
  Some AbstractEpochIssue.
Proof.
  intros amount. simpl. discriminate.
Qed.

Theorem reverse_fuel_transfer_is_not_a_top_up : forall amount,
  visible_event
    (NativeTransfer ValidatorFuelCustody GeneralCustody amount) <>
  Some AbstractFuelTopUp.
Proof.
  intros amount. simpl. discriminate.
Qed.

Definition state_refines
  (abstract native : validator_economics) : Prop :=
  ve_general abstract = ve_general native /\
  ve_fuel abstract = ve_fuel native /\
  ve_pos abstract = ve_pos native /\
  ve_fuel_quarantine abstract = ve_fuel_quarantine native /\
  ve_cooperative abstract = ve_cooperative native /\
  ve_located abstract = ve_located native /\
  ve_burned abstract = ve_burned native /\
  ve_authorized_issuance abstract = ve_authorized_issuance native /\
  ve_stake_claim abstract = ve_stake_claim native /\
  ve_reward_claim abstract = ve_reward_claim native /\
  ve_generation abstract = ve_generation native /\
  ve_phase abstract = ve_phase native /\
  ve_halted abstract = ve_halted native.

Theorem state_refinement_is_functional : forall abstract left right,
  state_refines abstract left ->
  state_refines abstract right ->
  left = right.
Proof.
  intros abstract
    [lg lf lp lq lc ll lb li ls lr lgen lphase lhalt]
    [rg rf rp rq rc rl rb ri rs rr rgen rphase rhalt]
    Hleft Hright.
  unfold state_refines in Hleft, Hright. simpl in *.
  destruct Hleft as
    [Hlg [Hlf [Hlp [Hlq [Hlc [Hll [Hlb [Hli [Hls [Hlr [Hlgen [Hlphase Hlhalt]]]]]]]]]]]].
  destruct Hright as
    [Hrg [Hrf [Hrp [Hrq [Hrc [Hrl [Hrb [Hri [Hrs [Hrr [Hrgen [Hrphase Hrhalt]]]]]]]]]]]].
  f_equal; congruence.
Qed.

Theorem play_replay_role_substitution_is_impossible : forall expected play replay,
  state_refines expected play ->
  state_refines expected replay ->
  play = replay.
Proof.
  intros expected play replay Hplay Hreplay.
  eapply state_refinement_is_functional; eauto.
Qed.

Print Assumptions custody_location_injective.
Print Assumptions one_forcing_has_one_native_handler_settlement.
Print Assumptions play_replay_role_substitution_is_impossible.
