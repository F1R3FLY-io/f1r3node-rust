From Stdlib Require Import Arith.PeanoNat Lia Lists.List Sorting.Permutation.

Import ListNotations.

Inductive demand_certificate : Type :=
  | ExactDemand (amount : nat)
  | FiniteUpperBound (amount proof_id : nat)
  | UnprovableDemand.

Definition certified_reservation (certificate : demand_certificate) : option nat :=
  match certificate with
  | ExactDemand amount => Some amount
  | FiniteUpperBound amount proof_id =>
      if proof_id =? 0 then None else Some amount
  | UnprovableDemand => None
  end.

Definition admitted
  (supply : nat)
  (certificate : demand_certificate)
  (fee : nat)
  : Prop :=
  exists reservation,
    certified_reservation certificate = Some reservation /\
    reservation + fee <= supply.

Definition valid_witness
  (certificate : demand_certificate)
  (realized : nat)
  : Prop :=
  exists reservation,
    certified_reservation certificate = Some reservation /\
    realized <= reservation.

Definition branch_reservation (left right : nat) : nat := Nat.max left right.

Inductive deployment_kind : Type :=
  | ClientDeploy
  | ValidatorHeartbeatDeploy
  | ValidatorDummyDeploy.

Definition admitted_deployment
  (_kind : deployment_kind)
  (supply : nat)
  (certificate : demand_certificate)
  (fee : nat)
  : Prop := admitted supply certificate fee.

Record certificate_context : Type := {
  context_protocol_version : nat;
  context_program_hash : nat;
  context_pre_state_root : nat;
  context_reservation_id : nat
}.

Definition context_matches
  (expected actual : certificate_context)
  : bool :=
  (context_pre_state_root expected =? context_pre_state_root actual) &&
  (context_protocol_version expected =? context_protocol_version actual) &&
  (context_program_hash expected =? context_program_hash actual) &&
  (context_reservation_id expected =? context_reservation_id actual).

Theorem zero_supply_admission_is_zero_demand_and_fee :
  forall certificate fee,
    admitted 0 certificate fee ->
    exists reservation,
      certified_reservation certificate = Some reservation /\
      reservation = 0 /\ fee = 0.
Proof.
  intros certificate fee [reservation [Hcertificate Hfunded]].
  exists reservation.
  split; [exact Hcertificate|].
  lia.
Qed.

Theorem positive_exact_demand_cannot_use_absent_supply :
  forall amount fee,
    ~ admitted 0 (ExactDemand (S amount)) fee.
Proof.
  intros amount fee [reservation [Hcertificate Hfunded]].
  simpl in Hcertificate.
  inversion Hcertificate.
  lia.
Qed.

Theorem deployment_kind_never_exempts_funding :
  forall kind supply certificate fee,
    admitted_deployment kind supply certificate fee <->
    admitted supply certificate fee.
Proof.
  reflexivity.
Qed.

Section GenesisBootstrap.

  Context {authority : Type}.
  Variable authority_eq_dec : forall left right : authority, {left = right} + {left <> right}.

  Definition genesis_allocation := (authority * nat)%type.

  Fixpoint genesis_allocation_total
    (key : authority)
    (allocations : list genesis_allocation)
    : nat :=
    match allocations with
    | [] => 0
    | (candidate, amount) :: tail =>
        (if authority_eq_dec key candidate then amount else 0) +
        genesis_allocation_total key tail
    end.

  Theorem genesis_allocation_total_permutation :
    forall key left right,
      Permutation left right ->
      genesis_allocation_total key left = genesis_allocation_total key right.
  Proof.
    intros key left right Hpermutation.
    induction Hpermutation.
    - reflexivity.
    - destruct x as [candidate amount].
      simpl.
      destruct (authority_eq_dec key candidate); now rewrite IHHpermutation.
    - destruct x as [left_key left_amount].
      destruct y as [right_key right_amount].
      simpl.
      destruct (authority_eq_dec key left_key);
        destruct (authority_eq_dec key right_key); lia.
    - now rewrite IHHpermutation1, IHHpermutation2.
  Qed.

  Definition genesis_replay_agrees
    (committed replayed : list genesis_allocation)
    : Prop :=
    forall key,
      genesis_allocation_total key committed =
      genesis_allocation_total key replayed.

  Theorem permutation_genesis_replay_agrees :
    forall committed replayed,
      Permutation committed replayed ->
      genesis_replay_agrees committed replayed.
  Proof.
    intros committed replayed Hpermutation key.
    now apply genesis_allocation_total_permutation.
  Qed.

  Theorem genesis_replay_agreement_preserves_admission :
    forall committed replayed key certificate fee,
      genesis_replay_agrees committed replayed ->
      (admitted (genesis_allocation_total key committed) certificate fee <->
       admitted (genesis_allocation_total key replayed) certificate fee).
  Proof.
    intros committed replayed key certificate fee Hagreement.
    specialize (Hagreement key).
    now rewrite Hagreement.
  Qed.

  Theorem duplicate_genesis_allocations_combine :
    forall key first second tail,
      genesis_allocation_total key ((key, first) :: (key, second) :: tail) =
      first + second + genesis_allocation_total key tail.
  Proof.
    intros key first second tail.
    simpl.
    destruct (authority_eq_dec key key); [lia|contradiction].
  Qed.

End GenesisBootstrap.

Record genesis_vault_state : Type := {
  genesis_vault_balance : nat;
  genesis_funding_committed : bool
}.

Definition commit_genesis_system_vault_funding
  (state : genesis_vault_state)
  (amount : nat)
  : genesis_vault_state :=
  if genesis_funding_committed state
  then state
  else {| genesis_vault_balance := genesis_vault_balance state + amount;
          genesis_funding_committed := true |}.

Theorem genesis_system_vault_funding_is_exact :
  forall balance amount,
    genesis_vault_balance
      (commit_genesis_system_vault_funding
        {| genesis_vault_balance := balance;
           genesis_funding_committed := false |}
        amount) =
    balance + amount.
Proof.
  reflexivity.
Qed.

Theorem committed_genesis_system_vault_funding_is_idempotent :
  forall state amount,
    genesis_funding_committed state = true ->
    commit_genesis_system_vault_funding state amount = state.
Proof.
  intros state amount Hcommitted.
  unfold commit_genesis_system_vault_funding.
  now rewrite Hcommitted.
Qed.

Theorem genesis_system_vault_replay_agrees :
  forall played_pre replay_pre amount,
    played_pre = replay_pre ->
    commit_genesis_system_vault_funding played_pre amount =
    commit_genesis_system_vault_funding replay_pre amount.
Proof.
  intros played_pre replay_pre amount Hagreement.
  now rewrite Hagreement.
Qed.

Inductive bootstrap_phase : Type :=
  | GenesisUncommitted
  | GenesisCommitted
  | GenesisReplayVerified
  | AdmissionOpen.

Inductive genesis_authority_mode : Type :=
  | GenesisUnitAuthority
  | GenesisFunderAuthority.

Definition genesis_authority_agrees
  (executed replayed : genesis_authority_mode)
  : bool :=
  match executed, replayed with
  | GenesisUnitAuthority, GenesisUnitAuthority
  | GenesisFunderAuthority, GenesisFunderAuthority => true
  | _, _ => false
  end.

Theorem genesis_unit_execution_replay_agrees :
  genesis_authority_agrees GenesisUnitAuthority GenesisUnitAuthority = true.
Proof.
  reflexivity.
Qed.

Theorem genesis_unit_execution_rejects_funder_replay :
  genesis_authority_agrees GenesisUnitAuthority GenesisFunderAuthority = false.
Proof.
  reflexivity.
Qed.

Definition admission_enabled (phase : bootstrap_phase) : bool :=
  match phase with
  | GenesisReplayVerified | AdmissionOpen => true
  | GenesisUncommitted | GenesisCommitted => false
  end.

Theorem admission_requires_verified_genesis :
  forall phase,
    admission_enabled phase = true ->
    phase = GenesisReplayVerified \/ phase = AdmissionOpen.
Proof.
  intros phase Henabled.
  destruct phase; simpl in Henabled; try discriminate; auto.
Qed.

Theorem pre_state_mismatch_rejects_context :
  forall expected actual,
    context_pre_state_root expected <> context_pre_state_root actual ->
    context_matches expected actual = false.
Proof.
  intros expected actual Hdifferent.
  unfold context_matches.
  apply Nat.eqb_neq in Hdifferent.
  rewrite Hdifferent.
  reflexivity.
Qed.

Theorem left_branch_fits_reservation :
  forall left right,
    left <= branch_reservation left right.
Proof.
  intros left right.
  unfold branch_reservation.
  apply Nat.le_max_l.
Qed.

Theorem right_branch_fits_reservation :
  forall left right,
    right <= branch_reservation left right.
Proof.
  intros left right.
  unfold branch_reservation.
  apply Nat.le_max_r.
Qed.

Definition authority_multiset (authority : Type) : Type := authority -> nat.

Definition dominates {authority : Type}
  (available reservation : authority_multiset authority)
  : Prop :=
  forall key, reservation key <= available key.

Definition multiset_add {authority : Type}
  (left right : authority_multiset authority)
  : authority_multiset authority :=
  fun key => left key + right key.

Definition multiset_max {authority : Type}
  (left right : authority_multiset authority)
  : authority_multiset authority :=
  fun key => Nat.max (left key) (right key).

Definition settlement {authority : Type}
  (available realized fee : authority_multiset authority)
  : authority_multiset authority :=
  fun key => available key - (realized key + fee key).

Definition refund {authority : Type}
  (reservation realized : authority_multiset authority)
  : authority_multiset authority :=
  fun key => reservation key - realized key.

Theorem pointwise_funded_realized_is_funded :
  forall (authority : Type)
         (available reservation realized : authority_multiset authority),
    dominates available reservation ->
    dominates reservation realized ->
    dominates available realized.
Proof.
  intros authority available reservation realized Havailable Hrealized key.
  specialize (Havailable key).
  specialize (Hrealized key).
  lia.
Qed.

Theorem parallel_reservations_add :
  forall (authority : Type)
         (first second : authority_multiset authority)
         key,
    multiset_add first second key = first key + second key.
Proof.
  reflexivity.
Qed.

Theorem left_branch_fits_pointwise_max :
  forall (authority : Type)
         (left right : authority_multiset authority),
    dominates (multiset_max left right) left.
Proof.
  intros authority left right key.
  unfold multiset_max.
  apply Nat.le_max_l.
Qed.

Theorem right_branch_fits_pointwise_max :
  forall (authority : Type)
         (left right : authority_multiset authority),
    dominates (multiset_max left right) right.
Proof.
  intros authority left right key.
  unfold multiset_max.
  apply Nat.le_max_r.
Qed.

Theorem pointwise_settlement_conserves :
  forall (authority : Type)
         (available realized fee : authority_multiset authority),
    dominates available (multiset_add realized fee) ->
    forall key,
      settlement available realized fee key + realized key + fee key =
      available key.
Proof.
  intros authority available realized fee Hfunded key.
  specialize (Hfunded key).
  unfold settlement, multiset_add in *.
  lia.
Qed.

Theorem pointwise_refund_is_unused_reservation :
  forall (authority : Type)
         (reservation realized : authority_multiset authority),
    dominates reservation realized ->
    forall key,
      refund reservation realized key + realized key = reservation key.
Proof.
  intros authority reservation realized Hbounded key.
  specialize (Hbounded key).
  unfold refund.
  lia.
Qed.

Theorem admitted_witness_is_funded :
  forall supply certificate realized fee,
    admitted supply certificate fee ->
    valid_witness certificate realized ->
    realized + fee <= supply.
Proof.
  intros supply certificate realized fee
    [reservation [Hcertificate Hfunded]]
    [witness_reservation [Hwitness Hrealized]].
  rewrite Hcertificate in Hwitness.
  inversion Hwitness.
  lia.
Qed.

Theorem exact_settlement_conserves :
  forall supply certificate realized fee,
    admitted supply certificate fee ->
    valid_witness certificate realized ->
    supply - (realized + fee) + realized + fee = supply.
Proof.
  intros supply certificate realized fee Hadmitted Hwitness.
  pose proof
    (admitted_witness_is_funded
       supply certificate realized fee Hadmitted Hwitness).
  lia.
Qed.

Theorem refund_is_unused_reservation :
  forall reservation realized,
    realized <= reservation ->
    reservation - realized + realized = reservation.
Proof.
  intros reservation realized Hbounded.
  lia.
Qed.

Theorem replay_settlement_agrees :
  forall supply play_realized replay_realized,
    play_realized = replay_realized ->
    supply - play_realized = supply - replay_realized.
Proof.
  intros supply play_realized replay_realized Hagreement.
  now subst replay_realized.
Qed.

Inductive validation_disposition : Type :=
  | Accept
  | ObjectiveInvalid
  | MissingDependency
  | LocalFault
  | AlreadyProcessed.

Definition creates_slash_evidence
  (disposition : validation_disposition)
  : bool :=
  match disposition with
  | ObjectiveInvalid => true
  | _ => false
  end.

Theorem local_fault_never_slashes :
  creates_slash_evidence LocalFault = false.
Proof.
  reflexivity.
Qed.

Theorem missing_dependency_never_slashes :
  creates_slash_evidence MissingDependency = false.
Proof.
  reflexivity.
Qed.

Inductive validation_origin : Type :=
  | ProposerOrigin
  | PeerOrigin.

Record consensus_validation_checks : Type := {
  checks_checkpoint_replay : bool;
  checks_bonds_cache : bool
}.

Definition full_consensus_validation : consensus_validation_checks :=
  {| checks_checkpoint_replay := true;
     checks_bonds_cache := true |}.

Definition validation_checks_for_origin
  (_origin : validation_origin)
  : consensus_validation_checks :=
  full_consensus_validation.

Theorem validation_origin_independent :
  validation_checks_for_origin ProposerOrigin =
  validation_checks_for_origin PeerOrigin.
Proof.
  reflexivity.
Qed.

Theorem every_origin_replays_checkpoint_and_checks_bonds :
  forall origin,
    checks_checkpoint_replay (validation_checks_for_origin origin) = true /\
    checks_bonds_cache (validation_checks_for_origin origin) = true.
Proof.
  intros origin.
  destruct origin; auto.
Qed.

Definition finality_decision
  (dag_descends_current_lfb : bool)
  (_parent_order : list nat)
  : bool :=
  dag_descends_current_lfb.

Theorem finality_parent_permutation_invariant :
  forall dag_descends_current_lfb first_order second_order,
    finality_decision dag_descends_current_lfb first_order =
    finality_decision dag_descends_current_lfb second_order.
Proof.
  reflexivity.
Qed.

Record state_bound_certificate : Type := {
  state_bound_pre_root : nat;
  state_bound_post_root : nat;
  state_bound_cost : nat;
  state_bound_completed : bool
}.

Definition execution_capacity (supply fee : nat) : nat := supply - fee.

Definition valid_state_bound_certificate
  (expected_pre supply fee : nat)
  (certificate : state_bound_certificate)
  : Prop :=
  state_bound_pre_root certificate = expected_pre /\
  state_bound_completed certificate = true /\
  state_bound_cost certificate + fee <= supply.

Inductive state_bound_chain : nat -> list state_bound_certificate -> nat -> Prop :=
  | StateBoundChainNil : forall root, state_bound_chain root [] root
  | StateBoundChainCons : forall root tail_root certificate tail,
      state_bound_pre_root certificate = root ->
      state_bound_completed certificate = true ->
      state_bound_chain (state_bound_post_root certificate) tail tail_root ->
      state_bound_chain root (certificate :: tail) tail_root.

Definition cost_is_funded (supply fee cost : nat) : bool :=
  Nat.leb (cost + fee) supply.

Definition admitted_costs (supply fee : nat) (costs : list nat) : list nat :=
  filter (cost_is_funded supply fee) costs.

Theorem capacity_exactly_characterizes_funding :
  forall supply fee cost,
    fee <= supply ->
    (cost <= execution_capacity supply fee <-> cost + fee <= supply).
Proof.
  intros supply fee cost Hfee.
  unfold execution_capacity.
  lia.
Qed.

Theorem exhausted_execution_cannot_be_certified :
  forall expected_pre supply fee certificate,
    fee <= supply ->
    execution_capacity supply fee < state_bound_cost certificate ->
    ~ valid_state_bound_certificate expected_pre supply fee certificate.
Proof.
  intros expected_pre supply fee certificate Hfee Hexhausted
    [_ [_ Hfunded]].
  apply capacity_exactly_characterizes_funding in Hfunded; lia.
Qed.

Theorem state_bound_certificate_funds_committed_cost :
  forall expected_pre supply fee certificate committed replayed,
    valid_state_bound_certificate expected_pre supply fee certificate ->
    committed = state_bound_cost certificate ->
    replayed = committed ->
    committed + fee <= supply /\ replayed = state_bound_cost certificate.
Proof.
  intros expected_pre supply fee certificate committed replayed
    [_ [_ Hfunded]] Hcommit Hreplay.
  subst committed; subst replayed.
  auto.
Qed.

Theorem state_bound_chain_preserves_adjacent_roots :
  forall initial certificate tail final,
    state_bound_chain initial (certificate :: tail) final ->
    state_bound_pre_root certificate = initial /\
    state_bound_chain (state_bound_post_root certificate) tail final.
Proof.
  intros initial certificate tail final Hchain.
  inversion Hchain; subst.
  auto.
Qed.

Theorem admitted_costs_are_funded :
  forall supply fee costs cost,
    In cost (admitted_costs supply fee costs) ->
    cost + fee <= supply.
Proof.
  intros supply fee costs cost Hin.
  unfold admitted_costs in Hin.
  apply filter_In in Hin.
  destruct Hin as [_ Hfunded].
  unfold cost_is_funded in Hfunded.
  now apply Nat.leb_le.
Qed.

Theorem state_bound_exact_settlement_conserves :
  forall expected_pre supply fee certificate,
    valid_state_bound_certificate expected_pre supply fee certificate ->
    supply - (state_bound_cost certificate + fee) +
      state_bound_cost certificate + fee = supply.
Proof.
  intros expected_pre supply fee certificate [_ [_ Hfunded]].
  lia.
Qed.

Print Assumptions admitted_witness_is_funded.
Print Assumptions left_branch_fits_reservation.
Print Assumptions pointwise_funded_realized_is_funded.
Print Assumptions left_branch_fits_pointwise_max.
Print Assumptions pointwise_settlement_conserves.
Print Assumptions pointwise_refund_is_unused_reservation.
Print Assumptions exact_settlement_conserves.
Print Assumptions local_fault_never_slashes.
Print Assumptions validation_origin_independent.
Print Assumptions every_origin_replays_checkpoint_and_checks_bonds.
Print Assumptions finality_parent_permutation_invariant.
Print Assumptions zero_supply_admission_is_zero_demand_and_fee.
Print Assumptions positive_exact_demand_cannot_use_absent_supply.
Print Assumptions deployment_kind_never_exempts_funding.
Print Assumptions genesis_allocation_total_permutation.
Print Assumptions permutation_genesis_replay_agrees.
Print Assumptions genesis_replay_agreement_preserves_admission.
Print Assumptions duplicate_genesis_allocations_combine.
Print Assumptions genesis_system_vault_funding_is_exact.
Print Assumptions committed_genesis_system_vault_funding_is_idempotent.
Print Assumptions genesis_system_vault_replay_agrees.
Print Assumptions admission_requires_verified_genesis.
Print Assumptions pre_state_mismatch_rejects_context.
Print Assumptions capacity_exactly_characterizes_funding.
Print Assumptions exhausted_execution_cannot_be_certified.
Print Assumptions state_bound_certificate_funds_committed_cost.
Print Assumptions state_bound_chain_preserves_adjacent_roots.
Print Assumptions admitted_costs_are_funded.
Print Assumptions state_bound_exact_settlement_conserves.
