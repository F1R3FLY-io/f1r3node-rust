From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.

From FinalizedFloor Require Import CertifiedFloorPromotion.

Section General.

Context {Block Validator : Type}.

Variable block_eq_dec : forall left right : Block, {left = right} + {left <> right}.
Variable parent_edge : Block -> Block -> Prop.
Variable latest : Validator -> Block.
Variable causal_certified state_certified : Block -> Prop.
Variable preserves : Block -> Block -> Prop.
Variable current_floor : Block.

Record finalization_evidence : Type := {
  evidence_target : Block;
  evidence_causal_certificate : causal_certified evidence_target;
  evidence_state_certificate : state_certified evidence_target;
  evidence_preserves_current_floor : preserves current_floor evidence_target
}.

Definition validate_requested_target
  (requested : Block)
  (evidence : finalization_evidence)
  : option Block :=
  if block_eq_dec requested (evidence_target evidence)
  then Some (evidence_target evidence)
  else None.

Theorem validated_materialization_is_exact_and_dual_certified :
  forall requested evidence committed,
    validate_requested_target requested evidence = Some committed ->
    committed = requested /\
    causal_certified committed /\
    state_certified committed /\
    preserves current_floor committed.
Proof.
  intros requested evidence committed Hvalidated.
  unfold validate_requested_target in Hvalidated.
  destruct (block_eq_dec requested (evidence_target evidence)) as [Heq | Hneq].
  - inversion Hvalidated; subst committed.
    repeat split.
    + symmetry. exact Heq.
    + exact (evidence_causal_certificate evidence).
    + exact (evidence_state_certificate evidence).
    + exact (evidence_preserves_current_floor evidence).
  - discriminate.
Qed.

Theorem target_substitution_is_rejected :
  forall requested evidence,
    requested <> evidence_target evidence ->
    validate_requested_target requested evidence = None.
Proof.
  intros requested evidence Hneq.
  unfold validate_requested_target.
  destruct (block_eq_dec requested (evidence_target evidence)); congruence.
Qed.

Variable decide : (Validator -> Prop) -> Prop.

Hypothesis decide_extensional :
  forall left right,
    (forall validator, left validator <-> right validator) ->
    (decide left <-> decide right).

Theorem finalizer_discovery_matches_pairwise_certificate :
  forall candidate,
    decide
      (fun validator =>
        propagated_coverage parent_edge latest candidate validator) <->
    decide
      (fun validator =>
        pairwise_support parent_edge latest candidate validator).
Proof.
  intros candidate.
  apply coverage_decision_transparent.
  exact decide_extensional.
Qed.

Variable key : Block -> nat.

Definition exact_eligible (candidate : Block) : Prop :=
  causal_certified candidate /\
  state_certified candidate /\
  preserves current_floor candidate.

Definition highest_exact (candidate : Block) : Prop :=
  exact_eligible candidate /\
  forall other, exact_eligible other -> key other <= key candidate.

Hypothesis eligible_key_injective :
  forall left right,
    exact_eligible left ->
    exact_eligible right ->
    key left = key right ->
    left = right.

Theorem highest_exact_candidate_is_unique :
  forall left right,
    highest_exact left ->
    highest_exact right ->
    left = right.
Proof.
  intros left right [Hleft Hleft_highest] [Hright Hright_highest].
  apply eligible_key_injective; try assumption.
  apply Nat.le_antisymm.
  - apply Hright_highest. exact Hleft.
  - apply Hleft_highest. exact Hright.
Qed.

End General.

Inductive trace_block : Type :=
| TraceGenesis
| TraceSibling1
| TraceSibling2
| TraceSibling3
| TraceMerge.

Inductive trace_validator : Type :=
| TraceValidator1
| TraceValidator2
| TraceValidator3
| TraceValidator4.

Inductive trace_full_parent : trace_block -> trace_block -> Prop :=
| TraceGenesisSibling1 : trace_full_parent TraceGenesis TraceSibling1
| TraceGenesisSibling2 : trace_full_parent TraceGenesis TraceSibling2
| TraceGenesisSibling3 : trace_full_parent TraceGenesis TraceSibling3
| TraceSibling1Merge : trace_full_parent TraceSibling1 TraceMerge
| TraceSibling2Merge : trace_full_parent TraceSibling2 TraceMerge
| TraceSibling3Merge : trace_full_parent TraceSibling3 TraceMerge.

Inductive trace_main_parent : trace_block -> trace_block -> Prop :=
| TraceMainGenesisSibling1 : trace_main_parent TraceGenesis TraceSibling1
| TraceMainGenesisSibling2 : trace_main_parent TraceGenesis TraceSibling2
| TraceMainGenesisSibling3 : trace_main_parent TraceGenesis TraceSibling3
| TraceMainSibling1Merge : trace_main_parent TraceSibling1 TraceMerge.

Definition trace_latest (validator : trace_validator) : trace_block :=
  match validator with
  | TraceValidator1 => TraceSibling1
  | TraceValidator2 => TraceSibling2
  | TraceValidator3 => TraceSibling3
  | TraceValidator4 => TraceMerge
  end.

Definition trace_state_preserves
  (candidate latest_block : trace_block) : Prop :=
  match candidate, latest_block with
  | TraceSibling2, TraceMerge => False
  | _, _ => True
  end.

Theorem secondary_parent_is_discovered_by_complete_coverage :
  pairwise_support trace_full_parent trace_latest
    TraceSibling3 TraceValidator4.
Proof.
  unfold pairwise_support, trace_latest.
  apply (dag_reaches_step
    trace_full_parent TraceSibling3 TraceMerge TraceMerge).
  - apply TraceSibling3Merge.
  - apply dag_reaches_refl.
Qed.

Theorem main_parent_only_discovery_misses_secondary_parent :
  ~ pairwise_support trace_main_parent trace_latest
      TraceSibling3 TraceValidator4.
Proof.
  unfold pairwise_support, trace_latest.
  intros Hreaches.
  inversion Hreaches; subst.
  inversion H.
Qed.

Theorem rejected_and_preserving_secondary_targets_are_distinct :
  ~ trace_state_preserves TraceSibling2 TraceMerge /\
  trace_state_preserves TraceSibling3 TraceMerge.
Proof.
  split; simpl; tauto.
Qed.

Theorem strict_half_boundary_and_secondary_majority :
  2 * 8 <= 16 /\ 12 > 8.
Proof.
  lia.
Qed.

Theorem finalizer_floor_materialization_trace_correct :
  pairwise_support trace_full_parent trace_latest
    TraceSibling3 TraceValidator4 /\
  ~ pairwise_support trace_main_parent trace_latest
      TraceSibling3 TraceValidator4 /\
  ~ trace_state_preserves TraceSibling2 TraceMerge /\
  trace_state_preserves TraceSibling3 TraceMerge /\
  2 * 8 <= 16 /\ 12 > 8.
Proof.
  exact
    (conj secondary_parent_is_discovered_by_complete_coverage
      (conj main_parent_only_discovery_misses_secondary_parent
        (conj (proj1 rejected_and_preserving_secondary_targets_are_distinct)
          (conj (proj2 rejected_and_preserving_secondary_targets_are_distinct)
                strict_half_boundary_and_secondary_majority)))).
Qed.

Print Assumptions validated_materialization_is_exact_and_dual_certified.
Print Assumptions finalizer_discovery_matches_pairwise_certificate.
Print Assumptions highest_exact_candidate_is_unique.
Print Assumptions finalizer_floor_materialization_trace_correct.
