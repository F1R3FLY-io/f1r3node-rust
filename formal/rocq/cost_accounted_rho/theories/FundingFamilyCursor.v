From Stdlib Require Import Lists.List Arith.PeanoNat Bool.Bool Lia.
From CostAccountedRho Require Import LexicographicMinimax FundingFamilyPriority EligibleFundingAssignment.
Import ListNotations.

Lemma family_cursor_lex_prefix : forall prefix left right,
  funding_lex_le (prefix ++ left) (prefix ++ right) <-> funding_lex_le left right.
Proof.
  induction prefix; intros; simpl; [tauto|]. rewrite IHprefix.
  split; [intros [impossible|[_ ordered]]; [lia|exact ordered]|].
  intro ordered. right. auto.
Qed.

Lemma family_cursor_lex_block : forall left right left_tail right_tail,
  length left = length right ->
  funding_lex_le (left ++ left_tail) (right ++ right_tail) -> funding_lex_le left right.
Proof.
  induction left as [|head tail IH]; intros [|other rest] left_tail right_tail sizes ordered;
    simpl in *; try discriminate; auto.
  destruct ordered as [less|[same later]]; [now left|].
  right. split; [exact same|]. eapply IH; eauto.
Qed.

Definition family_cursor_row_key row cursor :=
  {| family_rank_sequence := burden_rank row;
     family_tie_sequence := skipn cursor row ++ firstn cursor row |}.

Section ResourceProjection.
  Context {Plan : Type}.
  Variable feasible : Plan -> Prop.
  Variable draws : Plan -> nat -> list nat.
  Variable before after : list nat.
  Variable target cursor : nat.

  Definition family_cursor_groups := before ++ target :: after.

  Definition family_cursor_resource_key plan :=
    canonical_family_resource_key (map (draws plan) family_cursor_groups) cursor.

  Definition family_cursor_resource_slice selected candidate :=
    feasible candidate /\
    (forall group, In group (before ++ after) ->
      burden_rank (draws candidate group) = burden_rank (draws selected group)) /\
    (forall group, In group before -> draws candidate group = draws selected group).

  Definition family_cursor_resource_projection selected value :=
    exists candidate, family_cursor_resource_slice selected candidate /\ draws candidate target = value.

  Lemma family_cursor_rank_blocks : forall plan,
    family_rank_sequence (family_cursor_resource_key plan) =
    concat (map (fun group => burden_rank (draws plan group)) before) ++
    burden_rank (draws plan target) ++
    concat (map (fun group => burden_rank (draws plan group)) after).
  Proof.
    intro plan. unfold family_cursor_resource_key, canonical_family_resource_key,
      family_cursor_groups. simpl. rewrite map_map, map_app, concat_app. reflexivity.
  Qed.

  Lemma family_cursor_tie_blocks : forall plan,
    family_tie_sequence (family_cursor_resource_key plan) =
    concat (map (fun group => skipn cursor (draws plan group) ++ firstn cursor (draws plan group)) before) ++
    (skipn cursor (draws plan target) ++ firstn cursor (draws plan target)) ++
    concat (map (fun group => skipn cursor (draws plan group) ++ firstn cursor (draws plan group)) after).
  Proof.
    intro plan. unfold family_cursor_resource_key, canonical_family_resource_key,
      family_cursor_groups. simpl. rewrite map_map, map_app, concat_app. reflexivity.
  Qed.

  Theorem family_staged_optimum_is_slice_optimum : forall selected candidate,
    (forall possible, feasible possible ->
      family_priority_le (family_cursor_resource_key selected) (family_cursor_resource_key possible)) ->
    length (draws selected target) = length (draws candidate target) ->
    family_cursor_resource_slice selected candidate ->
    family_priority_le (family_cursor_row_key (draws selected target) cursor)
      (family_cursor_row_key (draws candidate target) cursor).
  Proof.
    intros selected candidate optimal widths [possible [other_ranks preceding]].
    destruct (optimal candidate possible) as [ranks ties].
    assert (rank_before :
      map (fun group => burden_rank (draws candidate group)) before =
      map (fun group => burden_rank (draws selected group)) before).
    { apply map_ext_in. intros group member. apply other_ranks. apply in_or_app. now left. }
    assert (rank_after :
      map (fun group => burden_rank (draws candidate group)) after =
      map (fun group => burden_rank (draws selected group)) after).
    { apply map_ext_in. intros group member. apply other_ranks. apply in_or_app. now right. }
    assert (tie_before :
      map (fun group => skipn cursor (draws candidate group) ++ firstn cursor (draws candidate group)) before =
      map (fun group => skipn cursor (draws selected group) ++ firstn cursor (draws selected group)) before).
    { apply map_ext_in. intros group member. now rewrite preceding. }
    unfold family_priority_le, family_cursor_row_key. simpl. split.
    - rewrite !family_cursor_rank_blocks, rank_before in ranks.
      apply family_cursor_lex_prefix in ranks. eapply family_cursor_lex_block; [|exact ranks].
      now rewrite !burden_rank_preserves_source_count.
    - intro target_rank_same.
      assert (all_ranks_same : family_rank_sequence (family_cursor_resource_key selected) =
        family_rank_sequence (family_cursor_resource_key candidate)).
      { rewrite !family_cursor_rank_blocks, rank_before, rank_after, target_rank_same. reflexivity. }
      specialize (ties all_ranks_same).
      rewrite !family_cursor_tie_blocks, tie_before in ties.
      apply family_cursor_lex_prefix in ties. eapply family_cursor_lex_block; [|exact ties].
      rewrite !length_app, !length_skipn, !length_firstn, widths. reflexivity.
  Qed.

  Theorem family_unrestricted_slice_selects_capped_preference : forall selected capped_domain capped,
    feasible selected ->
    (forall possible, feasible possible ->
      length (draws selected target) = length (draws possible target)) ->
    (forall possible, feasible possible ->
      family_priority_le (family_cursor_resource_key selected) (family_cursor_resource_key possible)) ->
    (forall value, family_cursor_resource_projection selected value <-> capped_domain value) ->
    capped_domain capped ->
    (forall value, capped_domain value ->
      family_priority_le (family_cursor_row_key value cursor) (family_cursor_row_key capped cursor) ->
      value = capped) ->
    draws selected target = capped.
  Proof.
    intros selected capped_domain capped sound widths optimal unrestricted admitted unique.
    assert (selected_member : capped_domain (draws selected target)).
    { apply unrestricted. exists selected. split; [|reflexivity]. repeat split; auto. }
    apply unrestricted in admitted. destruct admitted as [candidate [slice same]].
    apply unique; [exact selected_member|]. rewrite <- same.
    apply family_staged_optimum_is_slice_optimum; auto. apply widths. exact (proj1 slice).
  Qed.

  Theorem family_single_group_projection_contract : forall selected value,
    before = [] -> after = [] ->
    (family_cursor_resource_projection selected value <->
      exists candidate, feasible candidate /\ draws candidate target = value).
  Proof.
    intros selected value empty_before empty_after. unfold family_cursor_resource_projection,
      family_cursor_resource_slice. rewrite empty_before, empty_after. simpl.
    split.
    - intros [candidate [[possible _] same]]. now exists candidate.
    - intros [candidate [possible same]]. exists candidate. repeat split; auto; contradiction.
  Qed.
End ResourceProjection.

Definition family_cursor_resource_next count captured amount (unrestricted : bool) capped_next :=
  if Nat.eqb amount 0 then None
  else Some (if unrestricted then capped_next else (S captured) mod count).

Theorem family_zero_resource_has_no_cursor_transition : forall count captured unrestricted capped_next,
  family_cursor_resource_next count captured 0 unrestricted capped_next = None.
Proof. reflexivity. Qed.

Theorem family_restricted_resource_rotates_once : forall count captured amount capped_next,
  amount <> 0 ->
  family_cursor_resource_next count captured amount false capped_next = Some ((S captured) mod count).
Proof.
  intros. unfold family_cursor_resource_next. destruct (Nat.eqb_spec amount 0); congruence.
Qed.

Theorem family_resource_transition_in_cohort : forall count captured amount unrestricted capped_next next,
  0 < count -> capped_next < count ->
  family_cursor_resource_next count captured amount unrestricted capped_next = Some next -> next < count.
Proof.
  intros count captured amount unrestricted capped_next next positive bounded chosen.
  unfold family_cursor_resource_next in chosen. destruct (Nat.eqb amount 0); [discriminate|].
  destruct unrestricted; inversion chosen; subst; [exact bounded|apply Nat.mod_upper_bound; lia].
Qed.

Section FeeProjection.
  Context {Plan : Type}.
  Variable feasible : Plan -> Prop.
  Variable draws : Plan -> nat -> list nat.
  Variable payer_position : Plan -> nat -> nat.
  Variable before after : list nat.
  Variable target : nat.

  Definition family_fee_groups := before ++ target :: after.

  Definition family_cursor_fee_slice selected candidate :=
    feasible candidate /\
    (forall group, In group family_fee_groups -> draws candidate group = draws selected group) /\
    (forall group, In group before -> payer_position candidate group = payer_position selected group).

  Definition family_cursor_fee_possible selected position :=
    exists candidate, family_cursor_fee_slice selected candidate /\ payer_position candidate target = position.

  Theorem family_fee_staged_optimum_selects_first_possible : forall selected position,
    (forall candidate,
      feasible candidate ->
      (forall group, In group family_fee_groups -> draws candidate group = draws selected group) ->
      funding_lex_le (map (payer_position selected) family_fee_groups)
        (map (payer_position candidate) family_fee_groups)) ->
    family_cursor_fee_possible selected position -> payer_position selected target <= position.
  Proof.
    intros selected position optimal [candidate [[possible [same_resources previous]] chosen]].
    pose proof (optimal candidate possible same_resources) as preferred.
    assert (same_prefix : map (payer_position candidate) before = map (payer_position selected) before).
    { apply map_ext_in. exact previous. }
    unfold family_fee_groups in preferred. rewrite !map_app, same_prefix in preferred.
    apply family_cursor_lex_prefix in preferred. simpl in preferred.
    rewrite chosen in preferred. destruct preferred as [less|[same _]]; lia.
  Qed.

  Theorem family_fee_mask_exact_completion : forall selected mask,
    (forall position, mask position = true <-> family_cursor_fee_possible selected position) ->
    forall position, mask position = true <->
      exists candidate, feasible candidate /\
        (forall group, In group family_fee_groups -> draws candidate group = draws selected group) /\
        (forall group, In group before -> payer_position candidate group = payer_position selected group) /\
        payer_position candidate target = position.
  Proof.
    intros selected mask exact position. rewrite exact.
    unfold family_cursor_fee_possible, family_cursor_fee_slice. split;
      intros [candidate hypotheses]; exists candidate; tauto.
  Qed.

  Theorem family_single_group_fee_projection_contract : forall selected position,
    before = [] -> after = [] ->
    (family_cursor_fee_possible selected position <->
      exists candidate, feasible candidate /\ draws candidate target = draws selected target /\
        payer_position candidate target = position).
  Proof.
    intros selected position empty_before empty_after.
    unfold family_cursor_fee_possible, family_cursor_fee_slice, family_fee_groups.
    rewrite empty_before, empty_after. simpl. split.
    - intros [candidate [[possible [resources _]] chosen]]. exists candidate.
      repeat split; auto.
    - intros [candidate [possible [resources chosen]]]. exists candidate.
      split; [|exact chosen]. split; [exact possible|]. split.
      + intros group [same|absent]; [now subst|contradiction].
      + intros group absent. contradiction.
  Qed.
End FeeProjection.

Definition family_cursor_fee_support count mask := filter mask (seq 0 count).

Theorem family_cursor_fee_support_has_no_duplicates : forall count mask,
  NoDup (family_cursor_fee_support count mask).
Proof. intros. apply NoDup_filter. apply seq_NoDup. Qed.

Theorem family_cursor_fee_support_exact : forall count mask payer,
  In payer (family_cursor_fee_support count mask) <-> payer < count /\ mask payer = true.
Proof.
  intros. unfold family_cursor_fee_support. rewrite filter_In, in_seq. split.
  - intros [[_ bounded] permitted]. auto.
  - intros [bounded permitted]. split; [lia|exact permitted].
Qed.

Definition family_cursor_fee_position_to_source captured count position := (captured + position) mod count.

Theorem family_cursor_fee_position_stays_in_cohort : forall captured count position,
  0 < count -> family_cursor_fee_position_to_source captured count position < count.
Proof. intros. apply Nat.mod_upper_bound. lia. Qed.

Definition family_cursor_fee_next captured count fee payer (possible_payers : list nat) :=
  if Nat.eqb fee 0 then None
  else Some (if Nat.eqb (length possible_payers) 1 then captured else (S payer) mod count).

Theorem family_forced_unit_fee_keeps_cursor : forall captured count payer,
  family_cursor_fee_next captured count 1 payer [payer] = Some captured.
Proof. reflexivity. Qed.

Theorem family_multiple_fee_payers_advance_cursor : forall captured count payer possible_payers,
  1 < length possible_payers ->
  family_cursor_fee_next captured count 1 payer possible_payers = Some ((S payer) mod count).
Proof.
  intros. unfold family_cursor_fee_next. simpl. destruct (Nat.eqb_spec (length possible_payers) 1); congruence || lia.
Qed.

Theorem family_fee_transition_in_cohort : forall count captured fee payer possible_payers next,
  0 < count -> captured < count ->
  family_cursor_fee_next captured count fee payer possible_payers = Some next -> next < count.
Proof.
  intros count captured fee payer possible_payers next positive bounded selected.
  unfold family_cursor_fee_next in selected. destruct (Nat.eqb fee 0); [discriminate|].
  destruct (Nat.eqb (length possible_payers) 1); inversion selected; subst;
    [exact bounded|apply Nat.mod_upper_bound; lia].
Qed.

Record family_cursor_state := {
  family_resource_cursor : nat;
  family_fee_cursor : nat
}.

Record family_cursor_transition := {
  family_resource_successor : option nat;
  family_fee_successor : option nat
}.

Definition family_apply_cursor_transition state transition :=
  {| family_resource_cursor :=
       match family_resource_successor transition with
       | Some next => next | None => family_resource_cursor state end;
     family_fee_cursor :=
       match family_fee_successor transition with
       | Some next => next | None => family_fee_cursor state end |}.

Definition family_publish_realized_cursor state (group_of : nat -> nat) transitions outcome :=
  family_apply_cursor_transition state (transitions (group_of outcome)).

Theorem family_duplicate_outcomes_share_cursor_transition : forall state group_of transitions left right,
  group_of left = group_of right ->
  family_publish_realized_cursor state group_of transitions left =
  family_publish_realized_cursor state group_of transitions right.
Proof. intros. unfold family_publish_realized_cursor. now rewrite H. Qed.

Theorem family_unrealized_transitions_do_not_publish : forall state group_of first second outcome,
  first (group_of outcome) = second (group_of outcome) ->
  family_publish_realized_cursor state group_of first outcome =
  family_publish_realized_cursor state group_of second outcome.
Proof. intros. unfold family_publish_realized_cursor. now rewrite H. Qed.

Definition family_commit_realized_cursor state (already_committed : bool) group_of transitions outcome :=
  if already_committed then (state, true)
  else (family_publish_realized_cursor state group_of transitions outcome, true).

Theorem family_cursor_receipt_prevents_repeat_transition : forall state seen group_of transitions outcome,
  let result := family_commit_realized_cursor state seen group_of transitions outcome in
  family_commit_realized_cursor (fst result) (snd result) group_of transitions outcome = result.
Proof. intros. destruct seen; reflexivity. Qed.

Fixpoint family_cursor_vectors capacities :=
  match capacities with
  | [] => [[]]
  | cap :: rest => flat_map
      (fun amount => map (cons amount) (family_cursor_vectors rest)) (seq 0 (S cap))
  end.

Theorem family_cursor_vectors_complete : forall capacities values,
  In values (family_cursor_vectors capacities) <-> Forall2 Nat.le values capacities.
Proof.
  induction capacities as [|cap rest IH]; intros values; cbn [family_cursor_vectors].
  - simpl. split.
    + intros [same|absent]; [subst; constructor|contradiction].
    + intro empty. inversion empty. now left.
  - rewrite in_flat_map. split.
    + intros [amount [bounded member]]. apply in_map_iff in member.
      destruct member as [tail [same member]]. subst values.
      apply in_seq in bounded. constructor; [lia|now apply IH].
    + intro bounded. inversion bounded as [|amount upper tail limits head_bound tail_bound]; subst.
      exists amount. split; [apply in_seq; lia|]. apply in_map_iff.
      exists tail. split; [reflexivity|now apply IH].
Qed.

Definition family_cursor_simplex capacities total :=
  filter (fun row => Nat.eqb (fold_right Nat.add 0 row) total) (family_cursor_vectors capacities).

Theorem family_cursor_simplex_complete : forall capacities total values,
  In values (family_cursor_simplex capacities total) <->
  Forall2 Nat.le values capacities /\ fold_right Nat.add 0 values = total.
Proof.
  intros. unfold family_cursor_simplex. rewrite filter_In, Nat.eqb_eq,
    family_cursor_vectors_complete. tauto.
Qed.

Theorem family_cursor_full_projection_check_exact : forall capacities total completion projection,
  (forall values, completion values = true <-> projection values) ->
  (forallb completion (family_cursor_simplex capacities total) = true <->
    forall values, Forall2 Nat.le values capacities -> fold_right Nat.add 0 values = total -> projection values).
Proof.
  intros capacities total completion projection exact. rewrite forallb_forall. split.
  - intros checked values bounded sum. apply exact. apply checked.
    apply family_cursor_simplex_complete. auto.
  - intros covers values member. apply exact. apply family_cursor_simplex_complete in member.
    destruct member as [bounded sum]. now apply covers.
Qed.

Theorem family_cursor_restriction_has_counterexample : forall capacities total completion projection,
  (forall values, completion values = true <-> projection values) ->
  forallb completion (family_cursor_simplex capacities total) = false ->
  exists values, Forall2 Nat.le values capacities /\ fold_right Nat.add 0 values = total /\ ~ projection values.
Proof.
  intros capacities total completion projection exact failed.
  assert (find : forall values_list,
    forallb completion values_list = false -> exists values, In values values_list /\ completion values = false).
  { induction values_list as [|value rest IH]; simpl; intro rejected; [discriminate|].
    destruct (completion value) eqn:checked.
    - destruct (IH rejected) as [found [member no]]. exists found. split; [now right|exact no].
    - exists value. auto. }
  destruct (find _ failed) as [values [member no]].
  apply family_cursor_simplex_complete in member. destruct member as [bounded sum].
  exists values. repeat split; auto. intro feasible. apply exact in feasible. congruence.
Qed.

Section CursorCoverageCertificates.
  Context {Cell Value : Type}.
  Variable inside : Cell -> Value -> Prop.
  Variable projection : Value -> Prop.

  Inductive family_cursor_coverage_certificate : Cell -> Prop :=
  | family_cursor_cell_empty : forall cell,
      (forall value, ~ inside cell value) -> family_cursor_coverage_certificate cell
  | family_cursor_cell_covered : forall cell,
      (forall value, inside cell value -> projection value) -> family_cursor_coverage_certificate cell
  | family_cursor_cell_partition : forall cell left right,
      (forall value, inside cell value -> inside left value \/ inside right value) ->
      family_cursor_coverage_certificate left -> family_cursor_coverage_certificate right ->
      family_cursor_coverage_certificate cell.

  Theorem family_cursor_coverage_certificate_sound : forall cell,
    family_cursor_coverage_certificate cell -> forall value, inside cell value -> projection value.
  Proof.
    intros cell certificate. induction certificate; intros value member.
    - exfalso. now apply (H value).
    - now apply H.
    - destruct (H value member); [now apply IHcertificate1|now apply IHcertificate2].
  Qed.

  Theorem family_cursor_complete_finite_cell_has_certificate : forall cell enumeration,
    (forall value, inside cell value -> In value enumeration) ->
    Forall projection enumeration -> family_cursor_coverage_certificate cell.
  Proof.
    intros cell enumeration complete checked. apply family_cursor_cell_covered.
    rewrite Forall_forall in checked. intros value member. apply checked. now apply complete.
  Qed.
End CursorCoverageCertificates.

Theorem family_cursor_midpoint_partition_exact : forall lower upper middle value,
  lower <= middle -> middle < upper ->
  (lower <= value /\ value <= upper <->
    (lower <= value /\ value <= middle) \/ (S middle <= value /\ value <= upper)).
Proof. intros. lia. Qed.

Theorem family_cursor_duplicate_group_constraints_invariant : forall (groups reordered : list nat) predicate,
  (forall group, In group groups <-> In group reordered) ->
  ((forall group, In group groups -> predicate group) <->
   (forall group, In group reordered -> predicate group)).
Proof.
  intros groups reordered predicate same. split; intros valid group member;
    apply valid; now apply same.
Qed.

Lemma family_cursor_sum_monotone : forall count left right,
  (forall source, source < count -> left source <= right source) ->
  funding_sum count left <= funding_sum count right.
Proof.
  induction count as [|count IH]; intros left right bounded; simpl; [lia|].
  assert (tail : funding_sum count left <= funding_sum count right).
  { apply IH. intros source inside. apply bounded. lia. }
  specialize (bounded count ltac:(lia)). lia.
Qed.

Lemma family_cursor_sum_max_bound : forall count fixed actual fee,
  funding_sum count (fun source => Nat.max (fixed source) (actual source + fee source)) <=
  funding_sum count fixed + funding_sum count actual + funding_sum count fee.
Proof.
  induction count as [|count IH]; intros fixed actual fee; simpl; [lia|].
  specialize (IH fixed actual fee). lia.
Qed.

Theorem family_cursor_cell_exposure_coverage_bound :
  forall count fixed actual upper fee resource_total fee_total exposure,
  (forall source, source < count -> actual source <= upper source) ->
  funding_sum count actual = resource_total -> funding_sum count fee = fee_total ->
  Nat.min
    (funding_sum count (fun source => Nat.max (fixed source) (upper source + fee source)))
    (funding_sum count fixed + resource_total + fee_total) <= exposure ->
  funding_sum count (fun source => Nat.max (fixed source) (actual source + fee source)) <= exposure.
Proof.
  intros count fixed actual upper fee resource_total fee_total exposure bounded actual_total fees_total cap.
  assert (pointwise :
    funding_sum count (fun source => Nat.max (fixed source) (actual source + fee source)) <=
    funding_sum count (fun source => Nat.max (fixed source) (upper source + fee source))).
  { apply family_cursor_sum_monotone. intros source inside. specialize (bounded source inside). lia. }
  pose proof (family_cursor_sum_max_bound count fixed actual fee) as aggregate.
  rewrite actual_total, fees_total in aggregate. lia.
Qed.

Theorem family_cursor_cell_source_coverage_bound : forall count fixed actual upper fee capacity,
  (forall source, source < count ->
    actual source <= upper source /\ fixed source <= capacity source /\
    upper source + fee source <= capacity source) ->
  forall source, source < count ->
    Nat.max (fixed source) (actual source + fee source) <= capacity source.
Proof. intros count fixed actual upper fee capacity bounded source inside. specialize (bounded source inside). lia. Qed.

Print Assumptions family_staged_optimum_is_slice_optimum.
Print Assumptions family_unrestricted_slice_selects_capped_preference.
Print Assumptions family_single_group_projection_contract.
Print Assumptions family_fee_staged_optimum_selects_first_possible.
Print Assumptions family_fee_mask_exact_completion.
Print Assumptions family_cursor_fee_support_exact.
Print Assumptions family_cursor_fee_support_has_no_duplicates.
Print Assumptions family_fee_transition_in_cohort.
Print Assumptions family_single_group_fee_projection_contract.
Print Assumptions family_resource_transition_in_cohort.
Print Assumptions family_duplicate_outcomes_share_cursor_transition.
Print Assumptions family_unrealized_transitions_do_not_publish.
Print Assumptions family_cursor_receipt_prevents_repeat_transition.
Print Assumptions family_cursor_full_projection_check_exact.
Print Assumptions family_cursor_restriction_has_counterexample.
Print Assumptions family_cursor_coverage_certificate_sound.
Print Assumptions family_cursor_midpoint_partition_exact.
Print Assumptions family_cursor_duplicate_group_constraints_invariant.
Print Assumptions family_cursor_cell_exposure_coverage_bound.
Print Assumptions family_cursor_cell_source_coverage_bound.
