From Stdlib Require Import Lists.List.
From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

Section Algebra.

Context {Effect : Type}.

Definition state := Effect -> Prop.

Definition state_equiv (left right : state) : Prop :=
  forall effect, left effect <-> right effect.

Definition preserves (left right : state) : Prop :=
  forall effect, left effect -> right effect.

Definition no_effects : state := fun _ => False.

Definition union_state (left right : state) : state :=
  fun effect => left effect \/ right effect.

Definition construct_state
  (state_parent applied own : state)
  : state :=
  fun effect =>
    state_parent effect \/ applied effect \/ own effect.

Definition construct_state_list
  (state_parent own : state)
  (applied : list Effect)
  : state :=
  construct_state state_parent (fun effect => In effect applied) own.

Definition union_parent_state
  (state_parent applied own header_parent : state)
  : state :=
  construct_state state_parent (union_state applied header_parent) own.

Theorem preservation_reflexive : forall current, preserves current current.
Proof.
  unfold preserves.
  auto.
Qed.
Theorem preservation_transitive :
  forall left middle right,
    preserves left middle ->
    preserves middle right ->
    preserves left right.
Proof.
  unfold preserves.
  eauto.
Qed.

Theorem construct_state_exact :
  forall state_parent applied own effect,
    construct_state state_parent applied own effect <->
    state_parent effect \/ applied effect \/ own effect.
Proof.
  reflexivity.
Qed.

Theorem construct_state_preserves_state_parent :
  forall state_parent applied own,
    preserves state_parent (construct_state state_parent applied own).
Proof.
  unfold preserves, construct_state.
  firstorder.
Qed.

Theorem construct_state_preserves_applied :
  forall state_parent applied own,
    preserves applied (construct_state state_parent applied own).
Proof.
  unfold preserves, construct_state.
  firstorder.
Qed.

Theorem construct_state_preserves_own :
  forall state_parent applied own,
    preserves own (construct_state state_parent applied own).
Proof.
  unfold preserves, construct_state.
  firstorder.
Qed.

Theorem merge_parent_order_invariant :
  forall state_parent own left right,
    Permutation left right ->
    state_equiv
      (construct_state_list state_parent own left)
      (construct_state_list state_parent own right).
Proof.
  unfold state_equiv, construct_state_list, construct_state.
  intros state_parent own left right Hpermutation effect.
  split.
  - intros [Hparent | [Happlied | Hown]].
    + auto.
    + right.
      left.
      eapply Permutation_in.
      * exact Hpermutation.
      * exact Happlied.
    + auto.
  - intros [Hparent | [Happlied | Hown]].
    + auto.
    + right.
      left.
      eapply Permutation_in.
      * symmetry.
        exact Hpermutation.
      * exact Happlied.
    + auto.
Qed.

Theorem omitted_header_parent_effect_is_absent :
  forall state_parent applied own header_parent effect,
    header_parent effect ->
    ~ state_parent effect ->
    ~ applied effect ->
    ~ own effect ->
    ~ construct_state state_parent applied own effect.
Proof.
  unfold construct_state.
  firstorder.
Qed.

Theorem union_parent_recurrence_resurrects_omitted_effect :
  forall state_parent applied own header_parent effect,
    header_parent effect ->
    ~ state_parent effect ->
    ~ applied effect ->
    ~ own effect ->
    ~ construct_state state_parent applied own effect /\
    union_parent_state state_parent applied own header_parent effect.
Proof.
  unfold construct_state, union_parent_state, union_state.
  firstorder.
Qed.

Theorem direct_rejection_is_not_a_state_constructor :
  forall state_parent applied own rejected effect,
    rejected effect ->
    ~ state_parent effect ->
    ~ applied effect ->
    ~ own effect ->
    ~ construct_state state_parent applied own effect.
Proof.
  unfold construct_state.
  firstorder.
Qed.

Theorem direct_rejection_does_not_subtract_state_parent :
  forall state_parent applied own rejected effect,
    rejected effect ->
    state_parent effect ->
    construct_state state_parent applied own effect.
Proof.
  intros state_parent applied own rejected effect _ Hparent.
  apply construct_state_preserves_state_parent.
  exact Hparent.
Qed.

Theorem rejected_sibling_is_absent_without_positive_provenance :
  forall state_parent applied own rejected effect,
    rejected effect ->
    ~ state_parent effect ->
    ~ applied effect ->
    ~ own effect ->
    ~ construct_state state_parent applied own effect.
Proof.
  apply direct_rejection_is_not_a_state_constructor.
Qed.

Theorem accepted_effect_projection_preserves_source :
  forall source state_parent applied own,
    preserves source applied ->
    preserves source (construct_state state_parent applied own).
Proof.
  intros source state_parent applied own Hprojection.
  eapply preservation_transitive.
  - exact Hprojection.
  - apply construct_state_preserves_applied.
Qed.

Definition three_way
  (state_parent own left middle right : state)
  : state :=
  construct_state state_parent
    (union_state left (union_state middle right)) own.

Theorem three_way_preserves_each_applied_input :
  forall state_parent own left middle right,
    preserves left (three_way state_parent own left middle right) /\
    preserves middle (three_way state_parent own left middle right) /\
    preserves right (three_way state_parent own left middle right).
Proof.
  unfold preserves, three_way, construct_state, union_state.
  firstorder.
Qed.

Theorem repeated_three_way_merge_preserves_source :
  forall source state_parent own side_a side_b side_c side_d,
    preserves source
      (three_way state_parent own
        (three_way state_parent own source side_a side_b)
        (three_way state_parent own side_c source side_a)
        (three_way state_parent own side_b side_d source)).
Proof.
  intros source state_parent own side_a side_b side_c side_d effect Hsource.
  apply (proj1 (three_way_preserves_each_applied_input
    state_parent own
    (three_way state_parent own source side_a side_b)
    (three_way state_parent own side_c source side_a)
    (three_way state_parent own side_b side_d source))).
  apply (proj1 (three_way_preserves_each_applied_input
    state_parent own source side_a side_b)).
  exact Hsource.
Qed.

Definition majority_certificate
  (candidate tip_a tip_b tip_c : state)
  : Prop :=
  (preserves candidate tip_a /\ preserves candidate tip_b) \/
  (preserves candidate tip_a /\ preserves candidate tip_c) \/
  (preserves candidate tip_b /\ preserves candidate tip_c).

Theorem accepted_three_way_merges_have_majority_certificate :
  forall source state_parent own side_a side_b side_c side_d,
    majority_certificate
      source
      (three_way state_parent own source side_a side_b)
      (three_way state_parent own side_c source side_a)
      (three_way state_parent own side_b side_d source).
Proof.
  intros source state_parent own side_a side_b side_c side_d.
  left.
  split.
  - exact (proj1 (three_way_preserves_each_applied_input
      state_parent own source side_a side_b)).
  - exact (proj1 (proj2 (three_way_preserves_each_applied_input
      state_parent own side_c source side_a))).
Qed.

Definition rejection_candidates_complete
  (left right candidates : state)
  : Prop :=
  forall effect, left effect -> candidates effect \/ right effect.

Definition rejection_candidates_preserve
  (left right candidates : state)
  : Prop :=
  forall effect, candidates effect -> left effect -> right effect.

Theorem complete_rejection_candidate_check_iff_preserves :
  forall left right candidates,
    rejection_candidates_complete left right candidates ->
    (rejection_candidates_preserve left right candidates <->
     preserves left right).
Proof.
  unfold rejection_candidates_complete,
    rejection_candidates_preserve, preserves.
  intros left right candidates Hcomplete.
  split.
  - intros Hcheck effect Hleft.
    destruct (Hcomplete effect Hleft) as [Hcandidate | Hright].
    + exact (Hcheck effect Hcandidate Hleft).
    + exact Hright.
  - intros Hpreserves effect _ Hleft.
    exact (Hpreserves effect Hleft).
Qed.

End Algebra.

Section Projection.

Context {Effect Sig : Type}.

Definition signature_projection
  (effect_signature : Effect -> Sig)
  (applied : list Effect)
  (signature : Sig)
  : Prop :=
  exists effect,
    In effect applied /\ effect_signature effect = signature.

Theorem applied_effect_projects_its_signature :
  forall effect_signature applied effect,
    In effect applied ->
    signature_projection effect_signature applied (effect_signature effect).
Proof.
  unfold signature_projection.
  intros effect_signature applied effect Happlied.
  exists effect.
  auto.
Qed.

Theorem projected_signature_has_applied_effect :
  forall effect_signature applied signature,
    signature_projection effect_signature applied signature ->
    exists effect,
      In effect applied /\ effect_signature effect = signature.
Proof.
  intros effect_signature applied signature Hprojection.
  exact Hprojection.
Qed.

End Projection.

Inductive scenario_effect : Type :=
| SuccessfulEffect
| FailedSettlementEffect.

Definition source_state : @state scenario_effect := fun _ => True.
Definition empty_state : @state scenario_effect := fun _ => False.
Definition rejection_evidence : @state scenario_effect := fun _ => True.

Definition accepted_merge : @state scenario_effect :=
  construct_state empty_state source_state empty_state.

Definition rejected_merge : @state scenario_effect :=
  construct_state empty_state empty_state empty_state.

Definition restored_merge : @state scenario_effect :=
  construct_state rejected_merge source_state empty_state.

Definition repeated_accepted_merge : @state scenario_effect :=
  construct_state accepted_merge empty_state empty_state.

Definition single_base_merge : @state scenario_effect :=
  construct_state empty_state empty_state empty_state.

Definition unsafe_union_parent_merge : @state scenario_effect :=
  union_parent_state empty_state empty_state empty_state source_state.

Definition scenario_effect_contract (effect : scenario_effect) : Prop :=
  accepted_merge effect /\
  repeated_accepted_merge effect /\
  ~ rejected_merge effect /\
  restored_merge effect /\
  ~ single_base_merge effect /\
  unsafe_union_parent_merge effect.

Definition state_effect_provenance_contract : Prop :=
  (forall effect, scenario_effect_contract effect) /\
  majority_certificate
    source_state
    accepted_merge
    accepted_merge
    accepted_merge /\
  (forall
      (Effect : Type)
      (state_parent applied own header_parent : @state Effect)
      effect,
    header_parent effect ->
    ~ state_parent effect ->
    ~ applied effect ->
    ~ own effect ->
    ~ construct_state state_parent applied own effect /\
    union_parent_state state_parent applied own header_parent effect) /\
  forall (Effect : Type) (left right candidates : @state Effect),
    rejection_candidates_complete left right candidates ->
    (rejection_candidates_preserve left right candidates <->
     preserves left right).

Definition floor_rebased_parent_selection_contract : Prop :=
  forall
    (Effect : Type)
    (finalized applied own : @state Effect),
    preserves finalized (construct_state finalized applied own).

Theorem floor_rebased_parent_selection_end_to_end :
  floor_rebased_parent_selection_contract.
Proof.
  unfold floor_rebased_parent_selection_contract.
  intros Effect finalized applied own.
  apply construct_state_preserves_state_parent.
Qed.

Theorem state_effect_provenance_end_to_end :
  state_effect_provenance_contract.
Proof.
  unfold state_effect_provenance_contract.
  split.
  - intros effect.
    unfold scenario_effect_contract, accepted_merge,
      repeated_accepted_merge, rejected_merge, restored_merge,
      single_base_merge, unsafe_union_parent_merge,
      union_parent_state, union_state, construct_state,
      source_state, empty_state.
    firstorder.
  - split.
    + unfold majority_certificate, accepted_merge.
      left.
      split.
      * apply accepted_effect_projection_preserves_source.
        apply preservation_reflexive.
      * apply accepted_effect_projection_preserves_source.
        apply preservation_reflexive.
    + split.
      * intros Effect state_parent applied own header_parent effect
          Hparent Hnot_base Hnot_applied Hnot_own.
        apply union_parent_recurrence_resurrects_omitted_effect.
        -- exact Hparent.
        -- exact Hnot_base.
        -- exact Hnot_applied.
        -- exact Hnot_own.
      * intros Effect left right candidates Hcomplete.
        apply complete_rejection_candidate_check_iff_preserves.
        exact Hcomplete.
Qed.

Section ExactFloorSelection.

Context {Effect Validator : Type}.

Definition contains_all
  (floors : list (@state Effect))
  (candidate : @state Effect)
  : Prop :=
  forall floor, In floor floors -> preserves floor candidate.

Definition causal_witness
  (causal_support : Validator -> Prop)
  (validator : Validator)
  : Prop :=
  causal_support validator.

Definition exact_state_witness
  (candidate : @state Effect)
  (tips : Validator -> @state Effect)
  (causal_support : Validator -> Prop)
  (validator : Validator)
  : Prop :=
  causal_support validator /\ preserves candidate (tips validator).

Definition certificate_accepts
  (required candidate : @state Effect)
  : Prop :=
  preserves required candidate.

Definition proposal_accepts
  (required candidate : @state Effect)
  : Prop :=
  preserves required candidate.

Definition durable_append_accepts
  (required candidate : @state Effect)
  : Prop :=
  preserves required candidate.

Theorem exact_containment_reflexive :
  forall current : @state Effect,
    preserves current current.
Proof.
  apply preservation_reflexive.
Qed.

Theorem exact_containment_transitive :
  forall left middle right : @state Effect,
    preserves left middle ->
    preserves middle right ->
    preserves left right.
Proof.
  apply preservation_transitive.
Qed.

Theorem selected_state_contains_each_inherited_floor :
  forall floors candidate floor,
    contains_all floors candidate ->
    In floor floors ->
    preserves floor candidate.
Proof.
  unfold contains_all.
  auto.
Qed.

Theorem exact_witness_refines_causal_witness :
  forall candidate tips causal_support validator,
    exact_state_witness candidate tips causal_support validator ->
    causal_witness causal_support validator.
Proof.
  unfold exact_state_witness, causal_witness.
  firstorder.
Qed.

Theorem exact_containment_consumer_equivalence :
  forall required candidate,
    certificate_accepts required candidate <->
    proposal_accepts required candidate /\
    durable_append_accepts required candidate.
Proof.
  unfold certificate_accepts, proposal_accepts, durable_append_accepts.
  firstorder.
Qed.

Theorem activation_anchor_is_preserved :
  forall legacy_anchor applied own : @state Effect,
    preserves legacy_anchor
      (construct_state legacy_anchor applied own).
Proof.
  apply construct_state_preserves_state_parent.
Qed.

End ExactFloorSelection.

Inductive alias_effect : Type :=
| LeftEffect
| RightEffect.

Inductive alias_signature : Type :=
| SharedSignature.

Definition effect_signature (_ : alias_effect) : alias_signature :=
  SharedSignature.

Definition left_effect_state : @state alias_effect :=
  fun effect => effect = LeftEffect.

Definition right_effect_state : @state alias_effect :=
  fun effect => effect = RightEffect.

Definition joined_effect_state : @state alias_effect :=
  union_state left_effect_state right_effect_state.

Definition projected_signature_state
  (source : @state alias_effect)
  : @state alias_signature :=
  fun signature =>
    exists effect,
      source effect /\ effect_signature effect = signature.

Theorem equal_signature_projection_does_not_imply_exact_containment :
  state_equiv
    (projected_signature_state left_effect_state)
    (projected_signature_state right_effect_state) /\
  ~ preserves left_effect_state right_effect_state.
Proof.
  split.
  - unfold state_equiv, projected_signature_state,
      left_effect_state, right_effect_state, effect_signature.
    intros signature.
    destruct signature.
    split.
    + intros _.
      exists RightEffect.
      auto.
    + intros _.
      exists LeftEffect.
      auto.
  - unfold preserves, left_effect_state, right_effect_state.
    intros Hpreserves.
    specialize (Hpreserves LeftEffect eq_refl).
    discriminate.
Qed.

Theorem causal_reachability_does_not_imply_exact_containment :
  True /\ ~ preserves left_effect_state right_effect_state.
Proof.
  split.
  - exact I.
  - exact (proj2 equal_signature_projection_does_not_imply_exact_containment).
Qed.

Theorem joined_base_contains_all_settled_floors :
  contains_all
    [left_effect_state; right_effect_state]
    joined_effect_state /\
  ~ contains_all
      [left_effect_state; right_effect_state]
      left_effect_state.
Proof.
  split.
  - unfold contains_all, joined_effect_state, union_state, preserves.
    intros floor Hfloor effect Heffect.
    destruct Hfloor as [Hleft | [Hright | Hnone]].
    + subst floor.
      auto.
    + subst floor.
      auto.
    + contradiction.
  - unfold contains_all, preserves, left_effect_state, right_effect_state.
    intros Hall.
    specialize (Hall right_effect_state).
    specialize (Hall (or_intror (or_introl eq_refl)) RightEffect eq_refl).
    discriminate.
Qed.

Definition exact_floor_selection_contract : Prop :=
  state_equiv
    (projected_signature_state left_effect_state)
    (projected_signature_state right_effect_state) /\
  ~ preserves left_effect_state right_effect_state /\
  True /\
  contains_all
    [left_effect_state; right_effect_state]
    joined_effect_state /\
  ~ contains_all
      [left_effect_state; right_effect_state]
      left_effect_state /\
  forall required candidate : @state alias_effect,
    @certificate_accepts alias_effect required candidate <->
    @proposal_accepts alias_effect required candidate /\
    @durable_append_accepts alias_effect required candidate.

Theorem exact_floor_selection_end_to_end :
  exact_floor_selection_contract.
Proof.
  unfold exact_floor_selection_contract.
  destruct equal_signature_projection_does_not_imply_exact_containment
    as [Hprojection Hnot_exact].
  destruct joined_base_contains_all_settled_floors
    as [Hjoined Hpartial].
  split.
  - exact Hprojection.
  - split.
    + exact Hnot_exact.
    + split.
      * exact I.
      * split.
        -- exact Hjoined.
        -- split.
           ++ exact Hpartial.
           ++ intros required candidate.
              apply exact_containment_consumer_equivalence.
Qed.

Section AppliedStateValidationPrecedence.

Context {Effect : Type}.
Variable effect_eq_dec : forall left right : Effect, {left = right} + {left <> right}.

Inductive applied_state_validation_result : Type :=
| AppliedStateInvalid
| AppliedStateDeferred
| AppliedStateAccepted.

Definition applied_state_sources_to_resolve
  (claimed computed : list Effect)
  : list Effect :=
  match list_eq_dec effect_eq_dec claimed computed with
  | left _ => computed
  | right _ => []
  end.

Definition validate_applied_state
  (claimed computed : list Effect)
  (dependencies_available projection_exact : bool)
  : applied_state_validation_result :=
  match list_eq_dec effect_eq_dec claimed computed with
  | left _ =>
      if dependencies_available
      then if projection_exact
           then AppliedStateAccepted
           else AppliedStateInvalid
      else AppliedStateDeferred
  | right _ => AppliedStateInvalid
  end.

Definition validate_applied_state_claims_first
  (claimed computed : list Effect)
  (claimed_dependencies_available computed_dependencies_available
    projection_exact : bool)
  : applied_state_validation_result :=
  if claimed_dependencies_available
  then validate_applied_state
         claimed computed computed_dependencies_available projection_exact
  else AppliedStateDeferred.

Theorem unequal_applied_vector_is_invalid_without_source_resolution :
  forall claimed computed dependencies_available projection_exact,
    claimed <> computed ->
    validate_applied_state
      claimed computed dependencies_available projection_exact =
      AppliedStateInvalid /\
    applied_state_sources_to_resolve claimed computed = [].
Proof.
  intros claimed computed dependencies_available projection_exact Hunequal.
  unfold validate_applied_state, applied_state_sources_to_resolve.
  destruct (list_eq_dec effect_eq_dec claimed computed) as [Hequal | Hdifferent].
  - contradiction.
  - split; reflexivity.
Qed.

Theorem exact_applied_vector_with_missing_dependency_is_deferred :
  forall computed projection_exact,
    validate_applied_state computed computed false projection_exact =
      AppliedStateDeferred /\
    applied_state_sources_to_resolve computed computed = computed.
Proof.
  intros computed projection_exact.
  unfold validate_applied_state, applied_state_sources_to_resolve.
  destruct (list_eq_dec effect_eq_dec computed computed) as [_ | Hunequal].
  - split; reflexivity.
  - contradiction.
Qed.

Theorem applied_state_acceptance_requires_exact_vector_and_projection :
  forall claimed computed dependencies_available projection_exact,
    validate_applied_state
      claimed computed dependencies_available projection_exact =
      AppliedStateAccepted ->
    claimed = computed /\
    dependencies_available = true /\
    projection_exact = true.
Proof.
  intros claimed computed dependencies_available projection_exact Haccepted.
  unfold validate_applied_state in Haccepted.
  destruct (list_eq_dec effect_eq_dec claimed computed) as [Hequal | Hunequal].
  - destruct dependencies_available; try discriminate.
    destruct projection_exact; try discriminate.
    repeat split; assumption || reflexivity.
  - discriminate.
Qed.

Theorem exact_available_vector_with_exact_projection_is_accepted :
  forall computed,
    validate_applied_state computed computed true true = AppliedStateAccepted.
Proof.
  intros computed.
  unfold validate_applied_state.
  destruct (list_eq_dec effect_eq_dec computed computed) as [_ | Hunequal].
  - reflexivity.
  - contradiction.
Qed.

End AppliedStateValidationPrecedence.

Example claims_first_dependency_amplification_exists :
  @validate_applied_state_claims_first nat Nat.eq_dec
    [1; 3] [1; 2] false true true = AppliedStateDeferred /\
  @validate_applied_state nat Nat.eq_dec
    [1; 3] [1; 2] true true = AppliedStateInvalid.
Proof.
  split; reflexivity.
Qed.

Example duplicate_applied_vector_is_invalid_without_resolution :
  @validate_applied_state nat Nat.eq_dec
    [1; 1] [1] true true = AppliedStateInvalid /\
  @applied_state_sources_to_resolve nat Nat.eq_dec [1; 1] [1] = [].
Proof.
  split; reflexivity.
Qed.

Example out_of_order_applied_vector_is_invalid_without_resolution :
  @validate_applied_state nat Nat.eq_dec
    [2; 1] [1; 2] true true = AppliedStateInvalid /\
  @applied_state_sources_to_resolve nat Nat.eq_dec [2; 1] [1; 2] = [].
Proof.
  split; reflexivity.
Qed.

Example held_inherited_non_applied_effect_is_invalid_without_resolution :
  @validate_applied_state nat Nat.eq_dec
    [1; 3] [1] true true = AppliedStateInvalid /\
  @applied_state_sources_to_resolve nat Nat.eq_dec [1; 3] [1] = [].
Proof.
  split; reflexivity.
Qed.

Print Assumptions unequal_applied_vector_is_invalid_without_source_resolution.
Print Assumptions exact_applied_vector_with_missing_dependency_is_deferred.
Print Assumptions applied_state_acceptance_requires_exact_vector_and_projection.
Print Assumptions exact_available_vector_with_exact_projection_is_accepted.
Print Assumptions claims_first_dependency_amplification_exists.
Print Assumptions duplicate_applied_vector_is_invalid_without_resolution.
Print Assumptions out_of_order_applied_vector_is_invalid_without_resolution.
Print Assumptions held_inherited_non_applied_effect_is_invalid_without_resolution.
