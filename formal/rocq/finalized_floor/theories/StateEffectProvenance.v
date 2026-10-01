From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

Section Algebra.

Context {Effect : Type}.

Definition state := Effect -> Prop.

Definition state_equiv (left right : state) : Prop :=
  forall effect, left effect <-> right effect.

Definition preserves (left right : state) : Prop :=
  forall effect, left effect -> right effect.

Definition merge_state
  (own rejected : state)
  (inputs : list state)
  : state :=
  fun effect =>
    ~ rejected effect /\
    (own effect \/ exists input, In input inputs /\ input effect).

Definition no_effects : state := fun _ => False.

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

Theorem merge_rejects_named_effect :
  forall own rejected inputs effect,
    rejected effect ->
    ~ merge_state own rejected inputs effect.
Proof.
  unfold merge_state.
  firstorder.
Qed.

Theorem merge_preserves_input :
  forall own rejected inputs input,
    In input inputs ->
    (forall effect, input effect -> ~ rejected effect) ->
    preserves input (merge_state own rejected inputs).
Proof.
  unfold preserves, merge_state.
  intros own rejected inputs input Hin Haccepted effect Hactive.
  split.
  - apply Haccepted.
    exact Hactive.
  - right.
    exists input.
    auto.
Qed.

Theorem finalized_floor_restores_effect :
  forall own rejected parents floor effect,
    floor effect ->
    ~ rejected effect ->
    merge_state own rejected (floor :: parents) effect.
Proof.
  unfold merge_state.
  intros own rejected parents floor effect Hfloor Haccepted.
  split.
  - exact Haccepted.
  - right.
    exists floor.
    split.
    + left.
      reflexivity.
    + exact Hfloor.
Qed.

Theorem merge_parent_order_invariant :
  forall own rejected left right,
    Permutation left right ->
    state_equiv
      (merge_state own rejected left)
      (merge_state own rejected right).
Proof.
  unfold state_equiv, merge_state.
  intros own rejected left right Hpermutation effect.
  split.
  - intros [Haccepted [Hown | [input [Hin Hactive]]]].
    + auto.
    + split.
      * exact Haccepted.
      * right.
        exists input.
        split.
        -- eapply Permutation_in.
           ++ exact Hpermutation.
           ++ exact Hin.
        -- exact Hactive.
  - intros [Haccepted [Hown | [input [Hin Hactive]]]].
    + auto.
    + split.
      * exact Haccepted.
      * right.
        exists input.
        split.
        -- eapply Permutation_in.
           ++ symmetry.
              exact Hpermutation.
           ++ exact Hin.
        -- exact Hactive.
Qed.

Theorem covered_parent_elimination :
  forall own rejected redundant covering tail,
    preserves redundant covering ->
    state_equiv
      (merge_state own rejected (redundant :: covering :: tail))
      (merge_state own rejected (covering :: tail)).
Proof.
  unfold preserves, state_equiv, merge_state.
  intros own rejected redundant covering tail Hcovered effect.
  split.
  - intros [Haccepted [Hown | [input [Hin Hactive]]]].
    + auto.
    + split.
      * exact Haccepted.
      * right.
        simpl in Hin.
        destruct Hin as [Heq | [Heq | Htail]].
        -- subst input.
           exists covering.
           split.
           ++ simpl; auto.
           ++ apply Hcovered.
              exact Hactive.
        -- subst input.
           exists covering.
           split.
           ++ left.
              reflexivity.
           ++ exact Hactive.
        -- exists input.
           split.
           ++ right.
              exact Htail.
           ++ exact Hactive.
  - intros [Haccepted [Hown | [input [Hin Hactive]]]].
    + auto.
    + split.
      * exact Haccepted.
      * right.
        exists input.
        split.
        -- simpl in Hin.
           simpl.
           tauto.
        -- exact Hactive.
Qed.

Definition three_way
  (left middle right : state)
  : state :=
  merge_state no_effects no_effects [left; middle; right].

Theorem three_way_preserves_each_input :
  forall left middle right,
    preserves left (three_way left middle right) /\
    preserves middle (three_way left middle right) /\
    preserves right (three_way left middle right).
Proof.
  unfold preserves, three_way, merge_state, no_effects.
  firstorder.
Qed.

Theorem repeated_three_way_merge_preserves_source :
  forall source side_a side_b side_c side_d,
    preserves
      source
      (three_way
        (three_way source side_a side_b)
        (three_way side_c source side_a)
        (three_way side_b side_d source)).
Proof.
  intros source side_a side_b side_c side_d effect Hsource.
  apply (proj1 (three_way_preserves_each_input
    (three_way source side_a side_b)
    (three_way side_c source side_a)
    (three_way side_b side_d source))).
  apply (proj1 (three_way_preserves_each_input source side_a side_b)).
  exact Hsource.
Qed.

Definition majority_certificate
  (candidate tip_a tip_b tip_c : state)
  : Prop :=
  (preserves candidate tip_a /\ preserves candidate tip_b) \/
  (preserves candidate tip_a /\ preserves candidate tip_c) \/
  (preserves candidate tip_b /\ preserves candidate tip_c).

Theorem accepted_three_way_merges_have_majority_certificate :
  forall source side_a side_b side_c side_d,
    majority_certificate
      source
      (three_way source side_a side_b)
      (three_way side_c source side_a)
      (three_way side_b side_d source).
Proof.
  intros source side_a side_b side_c side_d.
  left.
  split.
  - exact (proj1 (three_way_preserves_each_input source side_a side_b)).
  - exact (proj1 (proj2
      (three_way_preserves_each_input side_c source side_a))).
Qed.

Definition selected_parent_states
  (finalized : state)
  (latest : list state)
  : list state :=
  match latest with
  | [] => [finalized]
  | parents => parents
  end.

Definition rebased_inputs
  (finalized : state)
  (latest : list state)
  : list state :=
  finalized :: selected_parent_states finalized latest.

Theorem selected_parent_states_nonempty :
  forall finalized latest,
    exists candidate,
      In candidate (selected_parent_states finalized latest).
Proof.
  unfold selected_parent_states.
  intros finalized latest.
  destruct latest as [|candidate tail].
  - exists finalized.
    simpl.
    auto.
  - exists candidate.
    simpl.
    auto.
Qed.

Theorem selected_parent_states_retain_every_valid_latest :
  forall finalized head tail candidate,
    (In candidate (selected_parent_states finalized (head :: tail)) <->
     In candidate (head :: tail)).
Proof.
  intros finalized head tail candidate.
  reflexivity.
Qed.

Theorem selected_parent_states_fallback_to_finalized :
  forall finalized,
    selected_parent_states finalized [] = [finalized].
Proof.
  reflexivity.
Qed.

Theorem rebased_merge_preserves_finalized :
  forall finalized latest own rejected,
    (forall effect, finalized effect -> ~ rejected effect) ->
    preserves finalized
      (merge_state own rejected
        (rebased_inputs finalized latest)).
Proof.
  unfold preserves, merge_state, rebased_inputs.
  intros finalized latest own rejected Haccepted effect Hfinalized.
  split.
  - exact (Haccepted effect Hfinalized).
  - right.
    exists finalized.
    split.
    + left.
      reflexivity.
    + exact Hfinalized.
Qed.

End Algebra.

Inductive scenario_effect : Type := CertifiedEffect.

Definition source_state : @state scenario_effect := fun _ => True.
Definition empty_state : @state scenario_effect := fun _ => False.
Definition reject_certified_effect : @state scenario_effect := fun _ => True.

Definition accepted_merge : @state scenario_effect :=
  three_way source_state empty_state empty_state.

Definition rejected_merge : @state scenario_effect :=
  merge_state no_effects reject_certified_effect
    [source_state; empty_state; empty_state].

Definition restored_merge : @state scenario_effect :=
  merge_state no_effects no_effects [rejected_merge; source_state].

Definition repeated_accepted_merge : @state scenario_effect :=
  three_way accepted_merge accepted_merge accepted_merge.

Definition single_base_merge : @state scenario_effect :=
  merge_state no_effects no_effects [empty_state].

Definition state_effect_provenance_contract : Prop :=
  accepted_merge CertifiedEffect /\
  repeated_accepted_merge CertifiedEffect /\
  ~ rejected_merge CertifiedEffect /\
  restored_merge CertifiedEffect /\
  ~ single_base_merge CertifiedEffect /\
  majority_certificate
    source_state
    accepted_merge
    accepted_merge
    accepted_merge /\
  forall (Effect : Type) (left right candidates : @state Effect),
    rejection_candidates_complete left right candidates ->
    (rejection_candidates_preserve left right candidates <->
     preserves left right).

Definition floor_rebased_parent_selection_contract : Prop :=
  forall
    (Effect : Type)
    (finalized : @state Effect)
    (latest : list (@state Effect))
    (own rejected : @state Effect),
    (forall effect, finalized effect -> ~ rejected effect) ->
    (exists candidate,
      In candidate (selected_parent_states finalized latest)) /\
    (forall head tail candidate,
      latest = head :: tail ->
      (In candidate (selected_parent_states finalized latest) <->
       In candidate latest)) /\
    preserves finalized
      (merge_state own rejected
        (rebased_inputs finalized latest)).

Theorem floor_rebased_parent_selection_end_to_end :
  floor_rebased_parent_selection_contract.
Proof.
  unfold floor_rebased_parent_selection_contract.
  intros Effect finalized latest own rejected Haccepted.
  split.
  - apply selected_parent_states_nonempty.
  - split.
    + intros head tail candidate Hlatest.
      subst latest.
      apply selected_parent_states_retain_every_valid_latest.
    + exact (rebased_merge_preserves_finalized
        finalized latest own rejected Haccepted).
Qed.

Theorem state_effect_provenance_end_to_end :
  state_effect_provenance_contract.
Proof.
  unfold state_effect_provenance_contract.
  split.
  - unfold accepted_merge.
    apply (proj1
      (three_way_preserves_each_input source_state empty_state empty_state)).
    exact I.
  - split.
    + unfold repeated_accepted_merge.
      apply (proj1
        (three_way_preserves_each_input
          accepted_merge accepted_merge accepted_merge)).
      unfold accepted_merge.
      apply (proj1
        (three_way_preserves_each_input source_state empty_state empty_state)).
      exact I.
    + split.
      * unfold rejected_merge.
        apply merge_rejects_named_effect.
        exact I.
      * split.
        -- unfold restored_merge, merge_state, no_effects.
           split.
           ++ auto.
           ++ right.
              exists source_state.
              split.
              ** simpl; auto.
              ** exact I.
        -- split.
           ++ unfold single_base_merge, merge_state, no_effects, empty_state.
              intros [_ [Hown | [input [Hin Hactive]]]].
              ** exact Hown.
              ** simpl in Hin.
                 destruct Hin as [Heq | Hnone].
                 --- subst input.
                     exact Hactive.
                 --- contradiction.
           ++ split.
              ** unfold majority_certificate.
                 left.
                 split.
                 --- unfold accepted_merge.
                     exact (proj1
                       (three_way_preserves_each_input
                         source_state empty_state empty_state)).
                 --- unfold accepted_merge.
                     exact (proj1
                       (three_way_preserves_each_input
                         source_state empty_state empty_state)).
              ** intros Effect left right candidates Hcomplete.
                 apply complete_rejection_candidate_check_iff_preserves.
                 exact Hcomplete.
Qed.
