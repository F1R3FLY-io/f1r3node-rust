(* ═══════════════════════════════════════════════════════════════════════════
   SlashingComposition.v — Cost Accounting and Slashing Boundary
   ═══════════════════════════════════════════════════════════════════════════

   The slashing protocol is verified in the f1r3node-rust
   analysis/slashing branch. This module does not duplicate those proofs.
   Instead it adopts the small interface that the cost-accounting model
   needs: slash system deploys may change PoS slashing state, but they do
   not change a user deploy's evaluated fuel, settlement inputs, or
   post-evaluation settlement arithmetic.

   The model is intentionally shallow. Slashing authorization, two-level
   closure, validator lifetime, and Rust/Scala slashing bisimilarity remain
   obligations of the slashing proof suite. The cost-accounting proof suite
   only needs the composition fact that a slash effect is a system effect
   over PoS state, not an in-flight mutation of user-evaluation fuel.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.PeanoNat Lia.

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CostAccountedReduction.
From CostAccountedRho Require Import Settlement.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Boundary State
   ═══════════════════════════════════════════════════════════════════════════ *)

Record cost_boundary := {
  boundary_settlement : fee_settlement;
  boundary_user_system : system
}.

Record slashing_ledger := {
  slashing_bonded_stake : nat;
  slashing_coop_vault_balance : nat;
  slashing_active_validator_count : nat;
  slashing_slashed_validator_count : nat
}.

Record composed_state := {
  composed_cost_boundary : cost_boundary;
  composed_slashing_ledger : slashing_ledger
}.

Record slash_system_effect := {
  slash_effect_before : slashing_ledger;
  slash_effect_after : slashing_ledger;
  slash_effect_evidence_epoch : nat
}.

Definition apply_slash_system_effect
  (C : composed_state)
  (E : slash_system_effect)
  : composed_state :=
  {|
    composed_cost_boundary := composed_cost_boundary C;
    composed_slashing_ledger := slash_effect_after E
  |}.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Cost-Invalid Evidence as Slashing Input
   ═══════════════════════════════════════════════════════════════════════════ *)

Inductive cost_invalid_block_evidence : Type :=
  | EvidenceReplayCostMismatch : nat -> nat -> cost_invalid_block_evidence
  | EvidenceLowDeployPrice : nat -> nat -> cost_invalid_block_evidence
  | EvidenceUnauthorizedFeeSettlement : cost_invalid_block_evidence
  | EvidenceUnauthorizedBudgetMutation : cost_invalid_block_evidence.

Record cost_invalid_block := {
  cost_invalid_evidence : cost_invalid_block_evidence;
  cost_invalid_boundary : cost_boundary
}.

Definition record_cost_invalid_block
  (evidence : cost_invalid_block_evidence)
  (boundary : cost_boundary)
  : cost_invalid_block :=
  {|
    cost_invalid_evidence := evidence;
    cost_invalid_boundary := boundary
  |}.

Definition replay_cost_mismatch
  (recorded observed : nat)
  : bool :=
  negb (Nat.eqb recorded observed).

Definition low_deploy_price_violation
  (offered minimum : nat)
  : bool :=
  offered <? minimum.

Inductive settlement_actor : Type :=
  | UserDeployActor
  | SystemDeployActor.

Definition fee_settlement_authorized
  (actor : settlement_actor)
  : bool :=
  match actor with
  | UserDeployActor => false
  | SystemDeployActor => true
  end.

Definition unauthorized_fee_settlement
  (actor : settlement_actor)
  : bool :=
  negb (fee_settlement_authorized actor).

Inductive budget_mutation_phase : Type :=
  | DuringUserEvaluation
  | PostEvaluationSettlement.

Definition budget_mutation_authorized
  (phase : budget_mutation_phase)
  (actor : settlement_actor)
  : bool :=
  match phase, actor with
  | PostEvaluationSettlement, SystemDeployActor => true
  | _, _ => false
  end.

Definition unauthorized_budget_mutation
  (phase : budget_mutation_phase)
  (actor : settlement_actor)
  : bool :=
  negb (budget_mutation_authorized phase actor).

Definition stale_cost_evidence
  (current_epoch evidence_epoch horizon : nat)
  : bool :=
  negb (current_epoch <=? evidence_epoch + horizon).

Theorem replay_cost_mismatch_sound_for_evidence : forall recorded observed,
  recorded <> observed ->
  replay_cost_mismatch recorded observed = true.
Proof.
  intros recorded observed Hneq.
  unfold replay_cost_mismatch.
  destruct (Nat.eqb_spec recorded observed) as [Heq | Hneq'].
  - contradiction.
  - reflexivity.
Qed.

Theorem replay_cost_mismatch_complete_for_evidence : forall recorded observed,
  replay_cost_mismatch recorded observed = true ->
  recorded <> observed.
Proof.
  intros recorded observed Hmismatch.
  unfold replay_cost_mismatch in Hmismatch.
  destruct (Nat.eqb_spec recorded observed) as [Heq | Hneq].
  - discriminate.
  - exact Hneq.
Qed.

Theorem low_deploy_price_violation_sound : forall offered minimum,
  offered < minimum ->
  low_deploy_price_violation offered minimum = true.
Proof.
  intros offered minimum Hlt.
  unfold low_deploy_price_violation.
  apply Nat.ltb_lt. exact Hlt.
Qed.

Theorem low_deploy_price_violation_complete : forall offered minimum,
  low_deploy_price_violation offered minimum = true ->
  offered < minimum.
Proof.
  intros offered minimum Hviolation.
  unfold low_deploy_price_violation in Hviolation.
  apply Nat.ltb_lt. exact Hviolation.
Qed.

Theorem unauthorized_fee_settlement_sound : forall actor,
  unauthorized_fee_settlement actor = true ->
  actor = UserDeployActor.
Proof.
  intros actor Hunauthorized.
  destruct actor; reflexivity || discriminate.
Qed.

Theorem unauthorized_fee_settlement_complete :
  unauthorized_fee_settlement UserDeployActor = true.
Proof.
  reflexivity.
Qed.

Theorem unauthorized_budget_mutation_sound : forall phase actor,
  unauthorized_budget_mutation phase actor = true ->
  phase <> PostEvaluationSettlement \/ actor <> SystemDeployActor.
Proof.
  intros phase actor Hunauthorized.
  destruct phase, actor; simpl in Hunauthorized; try discriminate.
  - left. discriminate.
  - left. discriminate.
  - right. discriminate.
Qed.

Theorem unauthorized_runtime_budget_mutation_during_evaluation : forall actor,
  unauthorized_budget_mutation DuringUserEvaluation actor = true.
Proof.
  intros actor. destruct actor; reflexivity.
Qed.

Theorem authorized_system_settlement_budget_mutation :
  unauthorized_budget_mutation PostEvaluationSettlement SystemDeployActor =
  false.
Proof.
  reflexivity.
Qed.

Theorem stale_cost_evidence_sound : forall current evidence_epoch horizon,
  stale_cost_evidence current evidence_epoch horizon = true ->
  evidence_epoch + horizon < current.
Proof.
  intros current evidence_epoch horizon Hstale.
  unfold stale_cost_evidence in Hstale.
  destruct (current <=? evidence_epoch + horizon) eqn:Hfresh.
  - discriminate.
  - apply Nat.leb_gt in Hfresh. exact Hfresh.
Qed.

Theorem stale_cost_evidence_complete : forall current evidence_epoch horizon,
  evidence_epoch + horizon < current ->
  stale_cost_evidence current evidence_epoch horizon = true.
Proof.
  intros current evidence_epoch horizon Hstale.
  unfold stale_cost_evidence.
  apply Nat.leb_gt in Hstale.
  rewrite Hstale. reflexivity.
Qed.

Theorem cost_invalid_block_evidence_does_not_change_user_cost :
  forall evidence boundary,
    settlement_token_cost
      (boundary_settlement
        (cost_invalid_boundary
          (record_cost_invalid_block evidence boundary))) =
    settlement_token_cost (boundary_settlement boundary).
Proof.
  reflexivity.
Qed.

Theorem cost_invalid_block_evidence_preserves_settlement_inputs :
  forall evidence boundary,
    let recorded := record_cost_invalid_block evidence boundary in
    settlement_limit
      (boundary_settlement (cost_invalid_boundary recorded)) =
      settlement_limit (boundary_settlement boundary) /\
    settlement_price
      (boundary_settlement (cost_invalid_boundary recorded)) =
      settlement_price (boundary_settlement boundary) /\
    settlement_token_cost
      (boundary_settlement (cost_invalid_boundary recorded)) =
      settlement_token_cost (boundary_settlement boundary).
Proof.
  intros evidence boundary.
  repeat split; reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Slash Effects Preserve Cost-Accounting Observables
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem slash_preserves_fee_settlement_inputs :
  forall C E,
    let C' := apply_slash_system_effect C E in
    settlement_limit
      (boundary_settlement (composed_cost_boundary C')) =
      settlement_limit
        (boundary_settlement (composed_cost_boundary C)) /\
    settlement_price
      (boundary_settlement (composed_cost_boundary C')) =
      settlement_price
        (boundary_settlement (composed_cost_boundary C)) /\
    settlement_token_cost
      (boundary_settlement (composed_cost_boundary C')) =
      settlement_token_cost
        (boundary_settlement (composed_cost_boundary C)).
Proof.
  intros C E.
  repeat split; reflexivity.
Qed.

Theorem slash_preserves_settled_amount :
  forall C E,
    let C' := apply_slash_system_effect C E in
    settled_amount
      (boundary_settlement (composed_cost_boundary C')) =
    settled_amount
      (boundary_settlement (composed_cost_boundary C)).
Proof.
  reflexivity.
Qed.

Theorem slash_preserves_settlement_accounting :
  forall C E,
    let C' := apply_slash_system_effect C E in
    escrowed_amount
      (boundary_settlement (composed_cost_boundary C')) =
      escrowed_amount
        (boundary_settlement (composed_cost_boundary C)) /\
    charged_amount
      (boundary_settlement (composed_cost_boundary C')) =
      charged_amount
        (boundary_settlement (composed_cost_boundary C)) /\
    refund_amount
      (boundary_settlement (composed_cost_boundary C')) =
      refund_amount
        (boundary_settlement (composed_cost_boundary C)) /\
    settled_amount
      (boundary_settlement (composed_cost_boundary C')) =
      settled_amount
        (boundary_settlement (composed_cost_boundary C)).
Proof.
  intros C E.
  repeat split; reflexivity.
Qed.

Theorem slash_system_effect_is_unmetered_for_user_budget :
  forall C E,
    system_token_count
      (boundary_user_system
        (composed_cost_boundary
          (apply_slash_system_effect C E))) =
    system_token_count
      (boundary_user_system (composed_cost_boundary C)).
Proof.
  reflexivity.
Qed.

Theorem slash_after_evaluation_preserves_final_fuel :
  forall C E,
    boundary_user_system
      (composed_cost_boundary
        (apply_slash_system_effect C E)) =
    boundary_user_system (composed_cost_boundary C).
Proof.
  reflexivity.
Qed.

Theorem slash_after_evaluation_cannot_add_fuel :
  forall S S' C E,
    ca_reachable S S' ->
    boundary_user_system (composed_cost_boundary C) = S' ->
    system_token_count
      (boundary_user_system
        (composed_cost_boundary
          (apply_slash_system_effect C E))) <=
    system_token_count S.
Proof.
  intros S S' C E Hreach Hboundary.
  rewrite slash_after_evaluation_preserves_final_fuel.
  rewrite Hboundary.
  exact (evaluation_cannot_receive_refund_fuel S S' Hreach).
Qed.

Theorem slash_after_evaluation_preserves_settlement_conservation :
  forall C E,
    settlement_token_cost
      (boundary_settlement (composed_cost_boundary C)) <=
    settlement_limit
      (boundary_settlement (composed_cost_boundary C)) ->
    settled_amount
      (boundary_settlement
        (composed_cost_boundary
          (apply_slash_system_effect C E))) =
    escrowed_amount
      (boundary_settlement
        (composed_cost_boundary
          (apply_slash_system_effect C E))).
Proof.
  intros C E Hbounded.
  rewrite slash_preserves_settled_amount.
  replace
    (escrowed_amount
      (boundary_settlement
        (composed_cost_boundary
          (apply_slash_system_effect C E))))
    with
      (escrowed_amount
        (boundary_settlement (composed_cost_boundary C)))
    by reflexivity.
  exact (charged_plus_refund_eq_escrow
    (boundary_settlement (composed_cost_boundary C))
    Hbounded).
Qed.
