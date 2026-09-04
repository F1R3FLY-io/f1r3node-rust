From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Lists.ListSet.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

From FinalizedFloor Require Import Foundation.
From FinalizedFloor Require Import CertifiedObjectiveEquivocation.

Record CausalObjectiveEvidence : Type := mkCausalObjectiveEvidence {
  causal_evidence_validator : Validator;
  causal_evidence_generation : BondGeneration;
  causal_evidence_sequence : nat;
  causal_evidence_left_hash : BlockHash;
  causal_evidence_right_hash : BlockHash
}.

Scheme Equality for CausalObjectiveEvidence.

Definition CausalEvidenceContext := list CausalObjectiveEvidence.

Record ParentAuthorityEntry : Type := mkParentAuthorityEntry {
  parent_authority_validator : Validator;
  parent_authority_generation : BondGeneration;
  parent_authority_stake : nat
}.

Record CertifiedLatestMessage : Type := mkCertifiedLatestMessage {
  certified_latest_validator : Validator;
  certified_latest_hash : BlockHash;
  certified_latest_sender : Validator;
  certified_latest_generation : option BondGeneration;
  certified_latest_admission_accepted : bool;
  certified_latest_descends_from_floor : bool
}.

Fixpoint lookup_parent_authority
  (validator : Validator) (authority : list ParentAuthorityEntry)
  : option ParentAuthorityEntry :=
  match authority with
  | [] => None
  | entry :: rest =>
      if Nat.eqb validator (parent_authority_validator entry)
      then Some entry
      else lookup_parent_authority validator rest
  end.

Fixpoint lookup_certified_latest
  (validator : Validator) (latest : list CertifiedLatestMessage)
  : option CertifiedLatestMessage :=
  match latest with
  | [] => None
  | entry :: rest =>
      if Nat.eqb validator (certified_latest_validator entry)
      then Some entry
      else lookup_certified_latest validator rest
  end.

Definition evidence_targets_incarnation
  (validator : Validator) (generation : BondGeneration)
  (evidence : CausalObjectiveEvidence) : bool :=
  Nat.eqb validator (causal_evidence_validator evidence) &&
  Nat.eqb generation (causal_evidence_generation evidence).

Definition has_causal_objective_evidence
  (validator : Validator) (generation : BondGeneration)
  (evidence : CausalEvidenceContext) : bool :=
  existsb (evidence_targets_incarnation validator generation) evidence.

Definition causal_parent_is_eligible
  (authority : list ParentAuthorityEntry)
  (latest : list CertifiedLatestMessage)
  (incoming : CausalEvidenceContext)
  (justification : Validator * BlockHash) : bool :=
  let validator := fst justification in
  match lookup_parent_authority validator authority,
        lookup_certified_latest validator latest with
  | Some authority_entry, Some latest_entry =>
      Nat.ltb 0 (parent_authority_stake authority_entry) &&
      Nat.eqb (snd justification) (certified_latest_hash latest_entry) &&
      Nat.eqb validator (certified_latest_sender latest_entry) &&
      certified_latest_admission_accepted latest_entry &&
      match certified_latest_generation latest_entry with
      | Some generation =>
          Nat.eqb generation (parent_authority_generation authority_entry) &&
          negb
            (has_causal_objective_evidence
              validator
              (parent_authority_generation authority_entry)
              incoming)
      | None => false
      end
  | _, _ => false
  end.

Definition vote_is_eligible
  (authority : list ParentAuthorityEntry)
  (latest : list CertifiedLatestMessage)
  (incoming : CausalEvidenceContext)
  (justification : Validator * BlockHash) : bool :=
  causal_parent_is_eligible authority latest incoming justification &&
  match lookup_certified_latest (fst justification) latest with
  | Some latest_entry => certified_latest_descends_from_floor latest_entry
  | None => false
  end.

Definition derive_causal_parent_projection
  (authority : list ParentAuthorityEntry)
  (latest : list CertifiedLatestMessage)
  (incoming : CausalEvidenceContext)
  (exact_justifications : list (Validator * BlockHash))
  : list (Validator * BlockHash) :=
  filter (causal_parent_is_eligible authority latest incoming) exact_justifications.

Definition derive_finality_vote_projection
  (authority : list ParentAuthorityEntry)
  (latest : list CertifiedLatestMessage)
  (incoming : CausalEvidenceContext)
  (exact_justifications : list (Validator * BlockHash))
  : list (Validator * BlockHash) :=
  filter (vote_is_eligible authority latest incoming) exact_justifications.

Definition consensus_evidence_roots
  (floor : BlockHash)
  (exact_justifications : list (Validator * BlockHash)) : list BlockHash :=
  floor :: map snd exact_justifications.

Definition certified_evidence_closure
  (root_evidence : BlockHash -> CausalEvidenceContext)
  (floor : BlockHash)
  (exact_justifications : list (Validator * BlockHash)) : CausalEvidenceContext :=
  fold_right
    (set_union CausalObjectiveEvidence_eq_dec)
    []
    (map root_evidence (consensus_evidence_roots floor exact_justifications)).

Lemma causal_parent_predicate_false_excludes :
  forall authority latest incoming exact justification,
    causal_parent_is_eligible authority latest incoming justification = false ->
    ~ In justification
      (derive_causal_parent_projection authority latest incoming exact).
Proof.
  intros authority latest incoming exact justification Hfalse Hmember.
  apply filter_In in Hmember. destruct Hmember as [_ Htrue].
  rewrite Hfalse in Htrue. discriminate.
Qed.

Lemma filter_preserves_permutation :
  forall (predicate : (Validator * BlockHash) -> bool) left right,
    Permutation left right ->
    Permutation (filter predicate left) (filter predicate right).
Proof.
  intros predicate left right Hperm. induction Hperm; simpl.
  - constructor.
  - destruct (predicate x); auto using perm_skip.
  - destruct (predicate x), (predicate y); simpl; try reflexivity.
    apply perm_swap.
  - eapply perm_trans; eauto.
Qed.

Definition floor_from_projection
  (base promoted : BlockHash)
  (projection : list (Validator * BlockHash)) : BlockHash :=
  if Nat.leb 2 (length projection) then promoted else base.

Definition causal_context_union
  (incoming delta : CausalEvidenceContext) : CausalEvidenceContext :=
  set_union CausalObjectiveEvidence_eq_dec incoming delta.

Definition causal_evidence_dependencies_present
  (dependencies : list BlockHash)
  (evidence : CausalObjectiveEvidence) : Prop :=
  In (causal_evidence_left_hash evidence) dependencies /\
  In (causal_evidence_right_hash evidence) dependencies /\
  causal_evidence_left_hash evidence <> causal_evidence_right_hash evidence.

Definition causal_delta_dependencies_complete
  (dependencies : list BlockHash)
  (delta : CausalEvidenceContext) : Prop :=
  Forall (causal_evidence_dependencies_present dependencies) delta.

Fixpoint first_missing_dependency
  (held : BlockHash -> bool) (dependencies : list BlockHash)
  : option BlockHash :=
  match dependencies with
  | [] => None
  | dependency :: rest =>
      if held dependency
      then first_missing_dependency held rest
      else Some dependency
  end.

Record ReceiverCausalView : Type := mkReceiverCausalView {
  receiver_parent_evidence : CausalEvidenceContext;
  receiver_ambient_evidence : CausalEvidenceContext
}.

Record FinalityConsensusCertificate : Type := mkFinalityConsensusCertificate {
  consensus_exact_justifications : list (Validator * BlockHash);
  consensus_exact_max_sequences : list (Validator * nat);
  consensus_incoming_evidence : CausalEvidenceContext;
  consensus_outgoing_evidence : CausalEvidenceContext;
  consensus_finality_projection : list (Validator * BlockHash);
  consensus_floor : BlockHash;
  consensus_pre_state_floor : BlockHash
}.

Inductive FinalityProjectionCapture : Type :=
| CompleteFinalityProjection (projection : list (Validator * BlockHash))
| MissingFinalityDependency (missing : BlockHash)
| InvalidFinalityClosure.

Definition capture_finality_projection
  (held : BlockHash -> bool)
  (dependencies : list BlockHash)
  (closure_invalid : bool)
  (authority : list ParentAuthorityEntry)
  (latest : list CertifiedLatestMessage)
  (incoming : CausalEvidenceContext)
  (exact_justifications : list (Validator * BlockHash))
  : FinalityProjectionCapture :=
  match first_missing_dependency held dependencies with
  | Some missing => MissingFinalityDependency missing
  | None =>
      if closure_invalid
      then InvalidFinalityClosure
      else CompleteFinalityProjection
        (derive_finality_vote_projection
          authority latest incoming exact_justifications)
  end.

Definition projection_from_capture
  (capture : FinalityProjectionCapture)
  : option (list (Validator * BlockHash)) :=
  match capture with
  | CompleteFinalityProjection projection => Some projection
  | MissingFinalityDependency _ => None
  | InvalidFinalityClosure => None
  end.

Definition certify_frozen_projection_context
  (base promoted : BlockHash)
  (exact_justifications : list (Validator * BlockHash))
  (exact_max_sequences : list (Validator * nat))
  (incoming delta : CausalEvidenceContext)
  (projection : list (Validator * BlockHash))
  : FinalityConsensusCertificate :=
  let floor := floor_from_projection base promoted projection in
  mkFinalityConsensusCertificate
    exact_justifications
    exact_max_sequences
    incoming
    (causal_context_union incoming delta)
    projection
    floor
    floor.

Definition certify_finality_context
  (base promoted : BlockHash)
  (authority : list ParentAuthorityEntry)
  (latest : list CertifiedLatestMessage)
  (exact_justifications : list (Validator * BlockHash))
  (exact_max_sequences : list (Validator * nat))
  (view : ReceiverCausalView)
  (delta : CausalEvidenceContext)
  : FinalityConsensusCertificate :=
  let incoming := receiver_parent_evidence view in
  let projection :=
    derive_finality_vote_projection
      authority latest incoming exact_justifications in
  certify_frozen_projection_context
    base
    promoted
    exact_justifications
    exact_max_sequences
    incoming
    delta
    projection.

Definition certificate_from_projection_capture
  (base promoted : BlockHash)
  (exact_justifications : list (Validator * BlockHash))
  (exact_max_sequences : list (Validator * nat))
  (incoming delta : CausalEvidenceContext)
  (capture : FinalityProjectionCapture)
  : option FinalityConsensusCertificate :=
  match capture with
  | CompleteFinalityProjection projection =>
      Some
        (certify_frozen_projection_context
          base promoted exact_justifications exact_max_sequences
          incoming delta projection)
  | MissingFinalityDependency _ => None
  | InvalidFinalityClosure => None
  end.

Definition evidence_is_slash_authorized
  (evidence : CausalObjectiveEvidence)
  (certificate : FinalityConsensusCertificate) : bool :=
  existsb
    (CausalObjectiveEvidence_beq evidence)
    (consensus_outgoing_evidence certificate).

Inductive ConsensusContextAdmission : Type :=
| AcceptedConsensusContext (certificate : FinalityConsensusCertificate)
| InvalidConsensusContext.

Definition propagated_consensus_context
  (admission : ConsensusContextAdmission) : CausalEvidenceContext :=
  match admission with
  | AcceptedConsensusContext certificate =>
      consensus_outgoing_evidence certificate
  | InvalidConsensusContext => []
  end.

Theorem finality_projection_is_subset_of_exact_justifications :
  forall authority latest incoming exact_justifications vote,
    In vote
      (derive_finality_vote_projection
        authority latest incoming exact_justifications) ->
    In vote exact_justifications.
Proof.
  intros authority latest incoming exact_justifications vote Hvote.
  apply filter_In in Hvote. exact (proj1 Hvote).
Qed.

Theorem first_missing_dependency_names_unheld :
  forall held dependencies missing,
    first_missing_dependency held dependencies = Some missing ->
    In missing dependencies /\ held missing = false.
Proof.
  intros held dependencies. induction dependencies as [|dependency rest IH]; simpl.
  - intros missing Hmissing. discriminate.
  - destruct (held dependency) eqn:Hheld.
    + intros missing Hmissing. apply IH in Hmissing.
      destruct Hmissing as [Hin Hunheld]. split; auto.
    + intros missing Hmissing. inversion Hmissing; subst. split; auto.
Qed.

Theorem complete_dependency_set_has_no_missing_member :
  forall held dependencies,
    Forall (fun dependency => held dependency = true) dependencies ->
    first_missing_dependency held dependencies = None.
Proof.
  intros held dependencies Hcomplete. induction Hcomplete; simpl.
  - reflexivity.
  - rewrite H. exact IHHcomplete.
Qed.

Theorem missing_capture_names_exact_unheld_dependency :
  forall held dependencies closure_invalid authority latest incoming exact missing,
    capture_finality_projection
      held dependencies closure_invalid authority latest incoming exact =
      MissingFinalityDependency missing ->
    In missing dependencies /\ held missing = false.
Proof.
  intros held dependencies closure_invalid authority latest incoming exact missing Hcapture.
  unfold capture_finality_projection in Hcapture.
  destruct (first_missing_dependency held dependencies) as [found|] eqn:Hmissing.
  - inversion Hcapture; subst.
    eapply first_missing_dependency_names_unheld. exact Hmissing.
  - destruct closure_invalid; discriminate.
Qed.

Theorem incomplete_closure_has_no_projection :
  forall held dependencies closure_invalid authority latest incoming exact missing,
    capture_finality_projection
      held dependencies closure_invalid authority latest incoming exact =
      MissingFinalityDependency missing ->
    projection_from_capture
      (capture_finality_projection
        held dependencies closure_invalid authority latest incoming exact) = None.
Proof.
  intros. rewrite H. reflexivity.
Qed.

Theorem incomplete_closure_has_no_certificate :
  forall base promoted exact max_sequences incoming delta capture missing,
    capture = MissingFinalityDependency missing ->
    certificate_from_projection_capture
      base promoted exact max_sequences incoming delta capture = None.
Proof.
  intros. subst. reflexivity.
Qed.

Theorem invalid_closure_has_no_certificate :
  forall base promoted exact max_sequences incoming delta capture,
    capture = InvalidFinalityClosure ->
    certificate_from_projection_capture
      base promoted exact max_sequences incoming delta capture = None.
Proof.
  intros. subst. reflexivity.
Qed.

Theorem full_restoration_reproduces_complete_projection :
  forall held dependencies authority latest incoming exact,
    Forall (fun dependency => held dependency = true) dependencies ->
    capture_finality_projection
      held dependencies false authority latest incoming exact =
      CompleteFinalityProjection
        (derive_finality_vote_projection authority latest incoming exact).
Proof.
  intros held dependencies authority latest incoming exact Hcomplete.
  unfold capture_finality_projection.
  rewrite (complete_dependency_set_has_no_missing_member held dependencies Hcomplete).
  reflexivity.
Qed.

Theorem complete_capture_certifies_the_same_projection :
  forall base promoted exact max_sequences incoming delta capture projection,
    capture = CompleteFinalityProjection projection ->
    exists certificate,
      certificate_from_projection_capture
        base promoted exact max_sequences incoming delta capture = Some certificate /\
      consensus_finality_projection certificate = projection.
Proof.
  intros. subst.
  eexists. split; reflexivity.
Qed.

Theorem causal_parent_projection_is_subset_of_exact_justifications :
  forall authority latest incoming exact_justifications parent,
    In parent
      (derive_causal_parent_projection
        authority latest incoming exact_justifications) ->
    In parent exact_justifications.
Proof.
  intros authority latest incoming exact_justifications parent Hparent.
  apply filter_In in Hparent. exact (proj1 Hparent).
Qed.

Theorem finality_projection_is_subset_of_causal_parent_projection :
  forall authority latest incoming exact_justifications vote,
    In vote
      (derive_finality_vote_projection
        authority latest incoming exact_justifications) ->
    In vote
      (derive_causal_parent_projection
        authority latest incoming exact_justifications).
Proof.
  intros authority latest incoming exact_justifications vote Hvote.
  apply filter_In in Hvote. destruct Hvote as [Hexact Heligible].
  apply filter_In. split; auto.
  unfold vote_is_eligible in Heligible.
  apply andb_true_iff in Heligible. exact (proj1 Heligible).
Qed.

Theorem accepted_stale_latest_is_causal_but_cannot_vote :
  forall authority latest incoming exact validator hash latest_entry,
    In (validator, hash) exact ->
    causal_parent_is_eligible authority latest incoming (validator, hash) = true ->
    lookup_certified_latest validator latest = Some latest_entry ->
    certified_latest_descends_from_floor latest_entry = false ->
    In (validator, hash)
      (derive_causal_parent_projection authority latest incoming exact) /\
    ~ In (validator, hash)
      (derive_finality_vote_projection authority latest incoming exact).
Proof.
  intros authority latest incoming exact validator hash latest_entry
    Hexact Hcausal Hlatest Hstale.
  split.
  - apply filter_In. split; auto.
  - intro Hvote. apply filter_In in Hvote. destruct Hvote as [_ Heligible].
    unfold vote_is_eligible in Heligible; simpl in Heligible.
    rewrite Hlatest, Hcausal, Hstale in Heligible. discriminate.
Qed.

Theorem causal_parent_minus_vote_is_exactly_floor_stale :
  forall authority latest incoming exact validator hash latest_entry,
    lookup_certified_latest validator latest = Some latest_entry ->
    In (validator, hash)
      (derive_causal_parent_projection authority latest incoming exact) ->
    (~ In (validator, hash)
        (derive_finality_vote_projection authority latest incoming exact) <->
     certified_latest_descends_from_floor latest_entry = false).
Proof.
  intros authority latest incoming exact validator hash latest_entry Hlatest Hcausal.
  apply filter_In in Hcausal. destruct Hcausal as [Hexact Hbase].
  split.
  - intro HnotVote.
    destruct (certified_latest_descends_from_floor latest_entry) eqn:Hdescends.
    + exfalso. apply HnotVote. apply filter_In. split; auto.
      unfold vote_is_eligible; simpl. rewrite Hbase, Hlatest, Hdescends. reflexivity.
    + reflexivity.
  - intros Hdescends Hvote.
    apply filter_In in Hvote. destruct Hvote as [_ Heligible].
    unfold vote_is_eligible in Heligible; simpl in Heligible.
    rewrite Hlatest, Hdescends in Heligible.
    rewrite andb_false_r in Heligible. discriminate.
Qed.

Theorem causal_parent_projection_is_permutation_invariant :
  forall authority latest incoming left right,
    Permutation left right ->
    Permutation
      (derive_causal_parent_projection authority latest incoming left)
      (derive_causal_parent_projection authority latest incoming right).
Proof.
  intros authority latest incoming left right Hperm.
  unfold derive_causal_parent_projection. apply filter_preserves_permutation. exact Hperm.
Qed.

Theorem finality_vote_projection_is_permutation_invariant :
  forall authority latest incoming left right,
    Permutation left right ->
    Permutation
      (derive_finality_vote_projection authority latest incoming left)
      (derive_finality_vote_projection authority latest incoming right).
Proof.
  intros authority latest incoming left right Hperm.
  unfold derive_finality_vote_projection. apply filter_preserves_permutation. exact Hperm.
Qed.

Theorem absent_authority_cannot_be_causal_parent :
  forall authority latest incoming exact validator hash,
    lookup_parent_authority validator authority = None ->
    ~ In (validator, hash)
      (derive_causal_parent_projection authority latest incoming exact).
Proof.
  intros authority latest incoming exact validator hash Hauthority Hparent.
  apply filter_In in Hparent. destruct Hparent as [_ Heligible].
  unfold causal_parent_is_eligible in Heligible; simpl in Heligible.
  rewrite Hauthority in Heligible. discriminate.
Qed.

Theorem absent_authority_cannot_vote :
  forall authority latest incoming exact validator hash,
    lookup_parent_authority validator authority = None ->
    ~ In (validator, hash)
      (derive_finality_vote_projection authority latest incoming exact).
Proof.
  intros authority latest incoming exact validator hash Hauthority Hvote.
  apply finality_projection_is_subset_of_causal_parent_projection in Hvote.
  eapply absent_authority_cannot_be_causal_parent; eauto.
Qed.

Theorem wrong_generation_cannot_be_causal_parent :
  forall authority latest incoming exact validator hash authority_entry latest_entry
      latest_generation,
    lookup_parent_authority validator authority = Some authority_entry ->
    lookup_certified_latest validator latest = Some latest_entry ->
    certified_latest_generation latest_entry = Some latest_generation ->
    latest_generation <> parent_authority_generation authority_entry ->
    ~ In (validator, hash)
      (derive_causal_parent_projection authority latest incoming exact).
Proof.
  intros authority latest incoming exact validator hash authority_entry latest_entry
    latest_generation Hauthority Hlatest Hgeneration Hwrong.
  eapply causal_parent_predicate_false_excludes.
  unfold causal_parent_is_eligible; simpl.
  rewrite Hauthority, Hlatest, Hgeneration.
  apply Nat.eqb_neq in Hwrong. rewrite Hwrong. simpl.
  repeat rewrite andb_false_r. reflexivity.
Qed.

Theorem missing_generation_cannot_be_causal_parent :
  forall authority latest incoming exact validator hash authority_entry latest_entry,
    lookup_parent_authority validator authority = Some authority_entry ->
    lookup_certified_latest validator latest = Some latest_entry ->
    certified_latest_generation latest_entry = None ->
    ~ In (validator, hash)
      (derive_causal_parent_projection authority latest incoming exact).
Proof.
  intros authority latest incoming exact validator hash authority_entry latest_entry
    Hauthority Hlatest Hgeneration.
  eapply causal_parent_predicate_false_excludes.
  unfold causal_parent_is_eligible; simpl.
  rewrite Hauthority, Hlatest, Hgeneration. simpl.
  repeat rewrite andb_false_r. reflexivity.
Qed.

Theorem mismatched_hash_cannot_be_causal_parent :
  forall authority latest incoming exact validator hash authority_entry latest_entry,
    lookup_parent_authority validator authority = Some authority_entry ->
    lookup_certified_latest validator latest = Some latest_entry ->
    hash <> certified_latest_hash latest_entry ->
    ~ In (validator, hash)
      (derive_causal_parent_projection authority latest incoming exact).
Proof.
  intros authority latest incoming exact validator hash authority_entry latest_entry
    Hauthority Hlatest Hhash.
  eapply causal_parent_predicate_false_excludes.
  unfold causal_parent_is_eligible; simpl.
  rewrite Hauthority, Hlatest.
  apply Nat.eqb_neq in Hhash. rewrite Hhash. simpl.
  repeat rewrite andb_false_r. reflexivity.
Qed.

Theorem mismatched_sender_cannot_be_causal_parent :
  forall authority latest incoming exact validator hash authority_entry latest_entry,
    lookup_parent_authority validator authority = Some authority_entry ->
    lookup_certified_latest validator latest = Some latest_entry ->
    validator <> certified_latest_sender latest_entry ->
    ~ In (validator, hash)
      (derive_causal_parent_projection authority latest incoming exact).
Proof.
  intros authority latest incoming exact validator hash authority_entry latest_entry
    Hauthority Hlatest Hsender.
  eapply causal_parent_predicate_false_excludes.
  unfold causal_parent_is_eligible; simpl.
  rewrite Hauthority, Hlatest.
  apply Nat.eqb_neq in Hsender. rewrite Hsender. simpl.
  repeat rewrite andb_false_r. reflexivity.
Qed.

Theorem zero_stake_cannot_be_causal_parent :
  forall authority latest incoming exact validator hash authority_entry latest_entry,
    lookup_parent_authority validator authority = Some authority_entry ->
    lookup_certified_latest validator latest = Some latest_entry ->
    parent_authority_stake authority_entry = 0 ->
    ~ In (validator, hash)
      (derive_causal_parent_projection authority latest incoming exact).
Proof.
  intros authority latest incoming exact validator hash authority_entry latest_entry
    Hauthority Hlatest Hstake.
  eapply causal_parent_predicate_false_excludes.
  unfold causal_parent_is_eligible; simpl.
  rewrite Hauthority, Hlatest, Hstake. reflexivity.
Qed.

Theorem causal_exclusion_implies_vote_exclusion :
  forall authority latest incoming exact justification,
    ~ In justification
      (derive_causal_parent_projection authority latest incoming exact) ->
    ~ In justification
      (derive_finality_vote_projection authority latest incoming exact).
Proof.
  intros authority latest incoming exact justification Hcausal Hvote.
  apply Hcausal.
  eapply finality_projection_is_subset_of_causal_parent_projection. exact Hvote.
Qed.

Theorem nonaccepted_latest_message_cannot_be_causal_parent :
  forall authority latest incoming exact validator hash authority_entry latest_entry,
    lookup_parent_authority validator authority = Some authority_entry ->
    lookup_certified_latest validator latest = Some latest_entry ->
    certified_latest_admission_accepted latest_entry = false ->
    ~ In (validator, hash)
      (derive_causal_parent_projection authority latest incoming exact).
Proof.
  intros authority latest incoming exact validator hash authority_entry latest_entry
    Hauthority Hlatest Hinvalid Hparent.
  apply filter_In in Hparent. destruct Hparent as [_ Heligible].
  unfold causal_parent_is_eligible in Heligible; simpl in Heligible.
  rewrite Hauthority, Hlatest, Hinvalid in Heligible. simpl in Heligible.
  repeat rewrite andb_false_r in Heligible. discriminate.
Qed.

Theorem nonaccepted_latest_message_cannot_vote :
  forall authority latest incoming exact validator hash authority_entry latest_entry,
    lookup_parent_authority validator authority = Some authority_entry ->
    lookup_certified_latest validator latest = Some latest_entry ->
    certified_latest_admission_accepted latest_entry = false ->
    ~ In (validator, hash)
      (derive_finality_vote_projection authority latest incoming exact).
Proof.
  intros authority latest incoming exact validator hash authority_entry latest_entry
    Hauthority Hlatest Hinvalid Hvote.
  apply finality_projection_is_subset_of_causal_parent_projection in Hvote.
  eapply nonaccepted_latest_message_cannot_be_causal_parent; eauto.
Qed.

Lemma evidence_member_targets_incarnation :
  forall evidence validator generation item,
    In item evidence ->
    causal_evidence_validator item = validator ->
    causal_evidence_generation item = generation ->
    has_causal_objective_evidence validator generation evidence = true.
Proof.
  intros evidence validator generation item Hitem Hvalidator Hgeneration.
  unfold has_causal_objective_evidence.
  apply existsb_exists. exists item. split; auto.
  unfold evidence_targets_incarnation.
  subst. repeat rewrite Nat.eqb_refl. reflexivity.
Qed.

Theorem causally_equivocating_incarnation_cannot_be_causal_parent :
  forall authority latest incoming exact validator hash authority_entry latest_entry evidence,
    lookup_parent_authority validator authority = Some authority_entry ->
    lookup_certified_latest validator latest = Some latest_entry ->
    certified_latest_generation latest_entry =
      Some (parent_authority_generation authority_entry) ->
    In evidence incoming ->
    causal_evidence_validator evidence = validator ->
    causal_evidence_generation evidence =
      parent_authority_generation authority_entry ->
    ~ In (validator, hash)
      (derive_causal_parent_projection authority latest incoming exact).
Proof.
  intros authority latest incoming exact validator hash authority_entry latest_entry
    evidence Hauthority Hlatest Hgeneration Hevidence Hvalidator HevidenceGeneration Hparent.
  apply filter_In in Hparent. destruct Hparent as [_ Heligible].
  unfold causal_parent_is_eligible in Heligible; simpl in Heligible.
  rewrite Hauthority, Hlatest, Hgeneration, Nat.eqb_refl in Heligible.
  pose proof
    (evidence_member_targets_incarnation
      incoming validator (parent_authority_generation authority_entry)
      evidence Hevidence Hvalidator HevidenceGeneration) as Htargeted.
  rewrite Htargeted in Heligible. simpl in Heligible.
  repeat rewrite andb_false_r in Heligible. discriminate.
Qed.

Theorem causally_equivocating_incarnation_cannot_vote :
  forall authority latest incoming exact validator hash authority_entry latest_entry evidence,
    lookup_parent_authority validator authority = Some authority_entry ->
    lookup_certified_latest validator latest = Some latest_entry ->
    certified_latest_generation latest_entry =
      Some (parent_authority_generation authority_entry) ->
    In evidence incoming ->
    causal_evidence_validator evidence = validator ->
    causal_evidence_generation evidence =
      parent_authority_generation authority_entry ->
    ~ In (validator, hash)
      (derive_finality_vote_projection authority latest incoming exact).
Proof.
  intros authority latest incoming exact validator hash authority_entry latest_entry
    evidence Hauthority Hlatest Hgeneration Hevidence Hvalidator HevidenceGeneration Hvote.
  apply finality_projection_is_subset_of_causal_parent_projection in Hvote.
  eapply causally_equivocating_incarnation_cannot_be_causal_parent; eauto.
Qed.

Theorem selected_floor_is_always_an_evidence_root :
  forall floor exact,
    In floor (consensus_evidence_roots floor exact).
Proof. intros; left; reflexivity. Qed.

Theorem exact_latest_hash_is_an_evidence_root :
  forall floor exact validator hash,
    In (validator, hash) exact ->
    In hash (consensus_evidence_roots floor exact).
Proof.
  intros floor exact validator hash Hexact. right.
  change (In (snd (validator, hash)) (map snd exact)).
  apply in_map. exact Hexact.
Qed.

Theorem selected_floor_evidence_survives_stale_latest_messages :
  forall root_evidence floor exact evidence,
    In evidence (root_evidence floor) ->
    In evidence (certified_evidence_closure root_evidence floor exact).
Proof.
  intros root_evidence floor exact evidence Hevidence.
  unfold certified_evidence_closure, consensus_evidence_roots. simpl.
  apply set_union_iff. left. exact Hevidence.
Qed.

Theorem exact_latest_evidence_is_in_certified_closure :
  forall root_evidence floor exact validator hash evidence,
    In (validator, hash) exact ->
    In evidence (root_evidence hash) ->
    In evidence (certified_evidence_closure root_evidence floor exact).
Proof.
  intros root_evidence floor exact validator hash evidence Hexact Hevidence.
  unfold certified_evidence_closure.
  assert (Hroot : In hash (consensus_evidence_roots floor exact)).
  { eapply exact_latest_hash_is_an_evidence_root. exact Hexact. }
  induction (consensus_evidence_roots floor exact) as [|root roots IH].
  - contradiction.
  - simpl. apply set_union_iff. destruct Hroot as [Heq | Htail].
    + left. subst. exact Hevidence.
    + right. apply IH. exact Htail.
Qed.

Theorem candidate_delta_does_not_affect_own_floor :
  forall base promoted authority latest exact max_sequences view delta,
    consensus_finality_projection
      (certify_finality_context
        base promoted authority latest exact max_sequences view delta) =
    derive_finality_vote_projection
      authority latest (receiver_parent_evidence view) exact /\
    consensus_floor
      (certify_finality_context
        base promoted authority latest exact max_sequences view delta) =
    floor_from_projection base promoted
      (derive_finality_vote_projection
        authority latest (receiver_parent_evidence view) exact).
Proof. split; reflexivity. Qed.

Theorem ambient_evidence_is_nonretroactive :
  forall base promoted authority latest exact max_sequences
      parent ambient_left ambient_right delta,
    certify_finality_context
      base promoted authority latest exact max_sequences
      (mkReceiverCausalView parent ambient_left) delta =
    certify_finality_context
      base promoted authority latest exact max_sequences
      (mkReceiverCausalView parent ambient_right) delta.
Proof. reflexivity. Qed.

Theorem exact_wire_state_is_preserved :
  forall base promoted authority latest exact max_sequences view delta,
    consensus_exact_justifications
      (certify_finality_context
        base promoted authority latest exact max_sequences view delta) = exact /\
    consensus_exact_max_sequences
      (certify_finality_context
        base promoted authority latest exact max_sequences view delta) = max_sequences.
Proof. split; reflexivity. Qed.

Theorem outgoing_context_is_incoming_union_delta :
  forall base promoted authority latest exact max_sequences view delta evidence,
    In evidence
      (consensus_outgoing_evidence
        (certify_finality_context
          base promoted authority latest exact max_sequences view delta)) <->
    In evidence (receiver_parent_evidence view) \/ In evidence delta.
Proof.
  intros base promoted authority latest exact max_sequences view delta evidence.
  simpl. unfold causal_context_union. apply set_union_iff.
Qed.

Theorem validated_delta_evidence_authorizes_slash :
  forall base promoted authority latest exact max_sequences view delta evidence,
    In evidence delta ->
    evidence_is_slash_authorized evidence
      (certify_finality_context
        base promoted authority latest exact max_sequences view delta) = true.
Proof.
  intros base promoted authority latest exact max_sequences view delta evidence Hevidence.
  unfold evidence_is_slash_authorized; simpl.
  apply existsb_exists. exists evidence. split.
  - unfold causal_context_union. apply set_union_iff. right. exact Hevidence.
  - destruct evidence; simpl. repeat rewrite Nat.eqb_refl. reflexivity.
Qed.

Theorem invalid_consensus_block_propagates_no_context :
  propagated_consensus_context InvalidConsensusContext = [].
Proof. reflexivity. Qed.

Theorem equivalent_receivers_derive_identical_consensus :
  forall base promoted authority latest exact max_sequences
      left_view right_view delta,
    receiver_parent_evidence left_view = receiver_parent_evidence right_view ->
    certify_finality_context
      base promoted authority latest exact max_sequences left_view delta =
    certify_finality_context
      base promoted authority latest exact max_sequences right_view delta.
Proof.
  intros base promoted authority latest exact max_sequences
    [left_parent left_ambient] [right_parent right_ambient] delta Hparent.
  simpl in Hparent. subst. reflexivity.
Qed.

Print Assumptions causally_equivocating_incarnation_cannot_vote.
Print Assumptions candidate_delta_does_not_affect_own_floor.
Print Assumptions equivalent_receivers_derive_identical_consensus.
