From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Lia.
Import ListNotations.

From FinalizedFloor Require Import Foundation.
From FinalizedFloor Require Import CertifiedObjectiveEquivocation.

Definition ValidatorIncarnation := (Validator * BondGeneration)%type.
Definition EvidenceRank := nat.
Definition CanonicalEvidenceContext := ValidatorIncarnation -> option EvidenceRank.

Definition incarnation_eq_dec :
  forall left right : ValidatorIncarnation, {left = right} + {left <> right}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition option_min (left right : option EvidenceRank) : option EvidenceRank :=
  match left, right with
  | None, other => other
  | other, None => other
  | Some left_rank, Some right_rank => Some (Nat.min left_rank right_rank)
  end.

Definition context_join
  (left right : CanonicalEvidenceContext) : CanonicalEvidenceContext :=
  fun incarnation => option_min (left incarnation) (right incarnation).

Definition context_insert
  (context : CanonicalEvidenceContext)
  (incarnation : ValidatorIncarnation)
  (rank : EvidenceRank) : CanonicalEvidenceContext :=
  fun queried =>
    if incarnation_eq_dec queried incarnation
    then option_min (context queried) (Some rank)
    else context queried.

Lemma option_min_commutative :
  forall left right,
    option_min left right = option_min right left.
Proof.
  intros [left |] [right |]; simpl; try reflexivity.
  rewrite Nat.min_comm. reflexivity.
Qed.

Lemma option_min_associative :
  forall left middle right,
    option_min (option_min left middle) right =
    option_min left (option_min middle right).
Proof.
  intros [left |] [middle |] [right |]; simpl; try reflexivity.
  rewrite Nat.min_assoc. reflexivity.
Qed.

Lemma option_min_idempotent :
  forall value, option_min value value = value.
Proof.
  intros [value |]; simpl; try reflexivity.
  rewrite Nat.min_id. reflexivity.
Qed.

Theorem canonical_context_join_commutative :
  forall left right incarnation,
    context_join left right incarnation = context_join right left incarnation.
Proof.
  intros left right incarnation.
  unfold context_join. apply option_min_commutative.
Qed.

Theorem canonical_context_join_associative :
  forall left middle right incarnation,
    context_join (context_join left middle) right incarnation =
    context_join left (context_join middle right) incarnation.
Proof.
  intros left middle right incarnation.
  unfold context_join. apply option_min_associative.
Qed.

Theorem canonical_context_join_idempotent :
  forall context incarnation,
    context_join context context incarnation = context incarnation.
Proof.
  intros context incarnation.
  unfold context_join. apply option_min_idempotent.
Qed.

Theorem canonical_context_has_one_proof_per_incarnation :
  forall (context : CanonicalEvidenceContext)
    (incarnation : ValidatorIncarnation) (left right : EvidenceRank),
    context incarnation = Some left ->
    context incarnation = Some right ->
    left = right.
Proof. intros context incarnation left right Hleft Hright; congruence. Qed.

Theorem context_insert_is_order_independent :
  forall context incarnation left right queried,
    context_insert
      (context_insert context incarnation left)
      incarnation right queried =
    context_insert
      (context_insert context incarnation right)
      incarnation left queried.
Proof.
  intros context incarnation left right queried.
  unfold context_insert.
  destruct (incarnation_eq_dec queried incarnation) as [Heq | Hneq].
  - subst queried.
    destruct (incarnation_eq_dec incarnation incarnation) as [_ | Hbad].
    + repeat rewrite option_min_associative.
      rewrite (option_min_commutative (Some left) (Some right)).
      reflexivity.
    + contradiction.
  - destruct (incarnation_eq_dec queried incarnation); congruence.
Qed.

Inductive CertifiedAdmissionDecision : Type :=
| CertifiedAccepted
| CertifiedRejected.

Record CausalEvidenceNode : Type := mkCausalEvidenceNode {
  causal_node_hash : BlockHash;
  causal_node_parents : list BlockHash;
  causal_node_justifications : list BlockHash;
  causal_node_decision : CertifiedAdmissionDecision;
  causal_node_delta : CanonicalEvidenceContext
}.

Definition empty_context : CanonicalEvidenceContext := fun _ => None.

Definition causal_node_predecessors
  (node : CausalEvidenceNode) : list BlockHash :=
  causal_node_parents node ++ causal_node_justifications node.

Definition propagated_node_delta
  (node : CausalEvidenceNode) : CanonicalEvidenceContext :=
  match causal_node_decision node with
  | CertifiedAccepted => causal_node_delta node
  | CertifiedRejected => empty_context
  end.

Definition with_decision
  (node : CausalEvidenceNode)
  (decision : CertifiedAdmissionDecision) : CausalEvidenceNode :=
  mkCausalEvidenceNode
    (causal_node_hash node)
    (causal_node_parents node)
    (causal_node_justifications node)
    decision
    (causal_node_delta node).

Theorem rejected_wrapper_does_not_stop_structural_traversal :
  forall node,
    causal_node_predecessors (with_decision node CertifiedRejected) =
    causal_node_predecessors (with_decision node CertifiedAccepted).
Proof. intros node; destruct node; reflexivity. Qed.

Theorem rejected_delta_does_not_propagate :
  forall node incarnation,
    propagated_node_delta (with_decision node CertifiedRejected) incarnation = None.
Proof. intros node incarnation; destruct node; reflexivity. Qed.

Theorem accepted_delta_propagates_exactly :
  forall node incarnation,
    propagated_node_delta (with_decision node CertifiedAccepted) incarnation =
    causal_node_delta node incarnation.
Proof. intros node incarnation; destruct node; reflexivity. Qed.

Record ObjectiveEvidenceFact : Type := mkObjectiveEvidenceFact {
  fact_incarnation : ValidatorIncarnation;
  fact_rank : EvidenceRank;
  fact_first_hash : BlockHash;
  fact_second_hash : BlockHash
}.

Definition proof_dependencies_present
  (known : list BlockHash)
  (fact : ObjectiveEvidenceFact) : Prop :=
  In (fact_first_hash fact) known /\
  In (fact_second_hash fact) known /\
  fact_first_hash fact <> fact_second_hash fact.

Definition import_proof_fact
  (context : CanonicalEvidenceContext)
  (fact : ObjectiveEvidenceFact) : CanonicalEvidenceContext :=
  context_insert context (fact_incarnation fact) (fact_rank fact).

Definition proof_fact_step
  (pending : list BlockHash)
  (context : CanonicalEvidenceContext)
  (fact : ObjectiveEvidenceFact)
  : list BlockHash * CanonicalEvidenceContext :=
  (pending, import_proof_fact context fact).

Theorem proof_roots_are_leaf_facts :
  forall (pending known : list BlockHash)
    (context : CanonicalEvidenceContext) (fact : ObjectiveEvidenceFact),
    proof_dependencies_present known fact ->
    fst (proof_fact_step pending context fact) = pending /\
    forall incarnation,
      snd (proof_fact_step pending context fact) incarnation =
      import_proof_fact context fact incarnation.
Proof. intros; split; [reflexivity | intros; reflexivity]. Qed.

Theorem proof_fact_changes_only_its_incarnation :
  forall context fact other,
    other <> fact_incarnation fact ->
    import_proof_fact context fact other = context other.
Proof.
  intros context fact other Hneq.
  unfold import_proof_fact, context_insert.
  destruct (incarnation_eq_dec other (fact_incarnation fact));
    congruence.
Qed.

Definition certified_effective_context
  (inherited structural ambient : CanonicalEvidenceContext)
  : CanonicalEvidenceContext :=
  context_join inherited structural.

Definition required_context_delta
  (inherited effective : CanonicalEvidenceContext)
  : CanonicalEvidenceContext :=
  fun incarnation =>
    match inherited incarnation, effective incarnation with
    | Some inherited_rank, Some effective_rank =>
        if Nat.eqb inherited_rank effective_rank then None else Some effective_rank
    | None, result => result
    | Some _, None => None
    end.

Theorem ambient_tracker_cannot_change_certified_context :
  forall inherited structural ambient_left ambient_right incarnation,
    certified_effective_context inherited structural ambient_left incarnation =
    certified_effective_context inherited structural ambient_right incarnation.
Proof. reflexivity. Qed.

Theorem equal_causal_closures_have_equal_required_delta :
  forall inherited_left inherited_right effective_left effective_right,
    (forall incarnation,
      inherited_left incarnation = inherited_right incarnation) ->
    (forall incarnation,
      effective_left incarnation = effective_right incarnation) ->
    forall incarnation,
      required_context_delta inherited_left effective_left incarnation =
      required_context_delta inherited_right effective_right incarnation.
Proof.
  intros inherited_left inherited_right effective_left effective_right
    Hinherited Heffective incarnation.
  unfold required_context_delta.
  rewrite Hinherited, Heffective. reflexivity.
Qed.

Record CertifiedAdmissionOutcome : Type := mkCertifiedAdmissionOutcome {
  outcome_block_hash : BlockHash;
  outcome_protocol_version : nat;
  outcome_schema_version : nat;
  outcome_ruleset_digest : nat;
  outcome_context_digest : nat;
  outcome_authority_digest : nat;
  outcome_decision : CertifiedAdmissionDecision
}.

Definition outcome_matches
  (block_hash protocol_version schema_version ruleset_digest
   context_digest authority_digest : nat)
  (outcome : CertifiedAdmissionOutcome) : Prop :=
  outcome_block_hash outcome = block_hash /\
  outcome_protocol_version outcome = protocol_version /\
  outcome_schema_version outcome = schema_version /\
  outcome_ruleset_digest outcome = ruleset_digest /\
  outcome_context_digest outcome = context_digest /\
  outcome_authority_digest outcome = authority_digest.

Theorem certified_outcome_rejects_any_identity_tamper :
  forall block protocol schema ruleset context authority outcome,
    outcome_matches block protocol schema ruleset context authority outcome ->
    forall block' protocol' schema' ruleset' context' authority',
      block' <> block \/ protocol' <> protocol \/ schema' <> schema \/
      ruleset' <> ruleset \/ context' <> context \/ authority' <> authority ->
      ~ outcome_matches
          block' protocol' schema' ruleset' context' authority' outcome.
Proof.
  intros block protocol schema ruleset context authority outcome Hmatches
    block' protocol' schema' ruleset' context' authority' Hchanged Hmatches'.
  unfold outcome_matches in Hmatches, Hmatches'.
  destruct Hmatches as [Hb [Hp [Hs [Hr [Hc Ha]]]]].
  destruct Hmatches' as [Hb' [Hp' [Hs' [Hr' [Hc' Ha']]]]].
  repeat match goal with
  | H : _ \/ _ |- _ => destruct H as [H | H]
  end; congruence.
Qed.

Inductive EvidenceDeltaVerdict : Type :=
| DeltaValid
| DeltaNeglected
| DeltaInvalid.

Inductive evidence_delta_classification
  (actual required : list EvidenceRank) : EvidenceDeltaVerdict -> Prop :=
| ClassifyExact :
    actual = required ->
    evidence_delta_classification actual required DeltaValid
| ClassifyCanonicalSubset :
    actual <> required ->
    NoDup actual ->
    incl actual required ->
    evidence_delta_classification actual required DeltaNeglected
| ClassifyMalformed :
    (~ NoDup actual \/ ~ incl actual required) ->
    evidence_delta_classification actual required DeltaInvalid.

Theorem exact_delta_is_valid :
  forall required,
    evidence_delta_classification required required DeltaValid.
Proof. intros required; apply ClassifyExact; reflexivity. Qed.

Theorem canonical_omission_is_neglected :
  forall actual required,
    actual <> required ->
    NoDup actual ->
    incl actual required ->
    evidence_delta_classification actual required DeltaNeglected.
Proof. intros; apply ClassifyCanonicalSubset; assumption. Qed.

Theorem extra_or_duplicate_delta_is_invalid :
  forall actual required,
    (~ NoDup actual \/ ~ incl actual required) ->
    evidence_delta_classification actual required DeltaInvalid.
Proof. intros; apply ClassifyMalformed; assumption. Qed.

Theorem certified_causal_admission_correct :
  (forall left right incarnation,
    context_join left right incarnation = context_join right left incarnation)
  /\
  (forall left middle right incarnation,
    context_join (context_join left middle) right incarnation =
    context_join left (context_join middle right) incarnation)
  /\
  (forall context incarnation,
    context_join context context incarnation = context incarnation)
  /\
  (forall node,
    causal_node_predecessors (with_decision node CertifiedRejected) =
    causal_node_predecessors (with_decision node CertifiedAccepted))
  /\
  (forall node incarnation,
    propagated_node_delta (with_decision node CertifiedRejected) incarnation = None)
  /\
  (forall inherited structural ambient_left ambient_right incarnation,
    certified_effective_context inherited structural ambient_left incarnation =
    certified_effective_context inherited structural ambient_right incarnation).
Proof.
  exact
    (conj canonical_context_join_commutative
      (conj canonical_context_join_associative
        (conj canonical_context_join_idempotent
          (conj rejected_wrapper_does_not_stop_structural_traversal
            (conj rejected_delta_does_not_propagate
                  ambient_tracker_cannot_change_certified_context))))).
Qed.

Inductive AdmissionCause : Type :=
| CandidateValid
| AuthenticatedObjectiveInvalid
| DeclaredHashMismatch
| NodeLocalValidationFault.

Inductive AdmissionResultKind : Type :=
| AdmissionAccepted
| AdmissionObjectiveRejected
| AdmissionUnattributableRejected
| AdmissionLocalFault.

Definition classify_admission_cause (cause : AdmissionCause) : AdmissionResultKind :=
  match cause with
  | CandidateValid => AdmissionAccepted
  | AuthenticatedObjectiveInvalid => AdmissionObjectiveRejected
  | DeclaredHashMismatch => AdmissionUnattributableRejected
  | NodeLocalValidationFault => AdmissionLocalFault
  end.

Definition carries_sender_authority (result : AdmissionResultKind) : bool :=
  match result with
  | AdmissionAccepted => true
  | AdmissionObjectiveRejected => true
  | AdmissionUnattributableRejected => false
  | AdmissionLocalFault => false
  end.

Definition carries_admission_outcome (result : AdmissionResultKind) : bool :=
  carries_sender_authority result.

Definition creates_slash_evidence (result : AdmissionResultKind) : bool :=
  match result with
  | AdmissionObjectiveRejected => true
  | _ => false
  end.

Theorem authenticated_objective_invalidity_is_certified :
  carries_sender_authority
    (classify_admission_cause AuthenticatedObjectiveInvalid) = true /\
  carries_admission_outcome
    (classify_admission_cause AuthenticatedObjectiveInvalid) = true /\
  creates_slash_evidence
    (classify_admission_cause AuthenticatedObjectiveInvalid) = true.
Proof. repeat split. Qed.

Theorem declared_hash_mismatch_cannot_frame_signer :
  carries_sender_authority
    (classify_admission_cause DeclaredHashMismatch) = false /\
  carries_admission_outcome
    (classify_admission_cause DeclaredHashMismatch) = false /\
  creates_slash_evidence
    (classify_admission_cause DeclaredHashMismatch) = false.
Proof. repeat split. Qed.

Theorem local_validation_fault_has_no_consensus_effect :
  carries_sender_authority
    (classify_admission_cause NodeLocalValidationFault) = false /\
  carries_admission_outcome
    (classify_admission_cause NodeLocalValidationFault) = false /\
  creates_slash_evidence
    (classify_admission_cause NodeLocalValidationFault) = false.
Proof. repeat split. Qed.

Theorem typed_admission_classification_total :
  forall cause,
    classify_admission_cause cause = AdmissionAccepted \/
    classify_admission_cause cause = AdmissionObjectiveRejected \/
    classify_admission_cause cause = AdmissionUnattributableRejected \/
    classify_admission_cause cause = AdmissionLocalFault.
Proof. intros []; intuition. Qed.

Theorem typed_admission_evidence_requires_certified_objective_invalidity :
  forall cause,
    creates_slash_evidence (classify_admission_cause cause) = true ->
    classify_admission_cause cause = AdmissionObjectiveRejected /\
    carries_sender_authority (classify_admission_cause cause) = true /\
    carries_admission_outcome (classify_admission_cause cause) = true.
Proof. intros []; simpl; intros; try discriminate; repeat split. Qed.

Print Assumptions certified_causal_admission_correct.
Print Assumptions certified_outcome_rejects_any_identity_tamper.
Print Assumptions typed_admission_classification_total.
Print Assumptions typed_admission_evidence_requires_certified_objective_invalidity.
