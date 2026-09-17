From Stdlib Require Import Lists.List Arith.PeanoNat Bool.Bool Lia.
From CostAccountedRho Require Import SignedPhloControls SignedPhloCapture SignedPhloFunding ContributionCursor
  FeeCursorTransition FundingFamilyCursor EligibleFundingAssignment.
Import ListNotations.

Record family_scoped_cursor_snapshot := {
  family_snapshot_scope : nat;
  family_snapshot_cursor : fee_cursor
}.

Record family_cursor_snapshot_pair := {
  family_snapshot_custodies : list nat;
  family_snapshot_resource : family_scoped_cursor_snapshot;
  family_snapshot_fee : family_scoped_cursor_snapshot
}.

Definition family_bind_cursor maximum count snapshot successor :=
  match successor with
  | None => Some None
  | Some position =>
      let current := family_snapshot_cursor snapshot in
      let plan := {| cursor_scope := family_snapshot_scope snapshot;
                     cursor_expected := current;
                     cursor_next := {| cursor_revision := S (cursor_revision current);
                                       cursor_position := position |} |} in
      match check_fee_cursor maximum count (family_snapshot_scope snapshot) current plan with
      | Some _ => Some (Some plan)
      | None => None
      end
  end.

Theorem family_bound_cursor_keeps_captured_scope_and_revision :
  forall maximum count snapshot successor plan,
  family_bind_cursor maximum count snapshot successor = Some (Some plan) ->
  cursor_scope plan = family_snapshot_scope snapshot /\
  cursor_expected plan = family_snapshot_cursor snapshot /\
  cursor_revision (cursor_next plan) = S (cursor_revision (family_snapshot_cursor snapshot)) /\
  cursor_position (cursor_next plan) < count /\
  cursor_revision (cursor_next plan) <= maximum.
Proof.
  intros maximum count snapshot [position|] plan bound; [|discriminate].
  unfold family_bind_cursor in bound.
  destruct (check_fee_cursor _ _ _ _ _) as [result|] eqn:checked; [|discriminate].
  inversion bound; subst plan. simpl.
  pose proof (checked_fee_cursor_is_scoped_bounded_successor _ _ _ _ _ _ checked)
    as [_ [_ [_ [_ [position_bound [_ [revision_bound same]]]]]]].
  rewrite same in position_bound, revision_bound. cbn in position_bound, revision_bound.
  repeat split; auto.
Qed.

Theorem family_absent_cursor_does_not_need_revision_headroom : forall maximum count snapshot,
  family_bind_cursor maximum count snapshot None = Some None.
Proof. reflexivity. Qed.

Theorem family_positive_cursor_rejects_exhausted_capture : forall maximum count snapshot position,
  maximum <= cursor_revision (family_snapshot_cursor snapshot) ->
  family_bind_cursor maximum count snapshot (Some position) = None.
Proof.
  intros maximum count snapshot position exhausted. unfold family_bind_cursor.
  rewrite fee_cursor_rejects_exhausted_revision by exact exhausted. reflexivity.
Qed.

Theorem family_zero_cursor_matches_contribution_check : forall maximum count snapshot,
  cursor_revision (family_snapshot_cursor snapshot) <= maximum ->
  cursor_position (family_snapshot_cursor snapshot) < count ->
  check_contribution_cursor maximum count (family_snapshot_scope snapshot) 0
    (family_snapshot_cursor snapshot) None = Some (family_snapshot_cursor snapshot).
Proof. intros. now apply zero_contribution_accepts_every_valid_revision. Qed.

Theorem family_positive_cursor_matches_contribution_check :
  forall maximum count snapshot position amount plan,
  0 < amount ->
  family_bind_cursor maximum count snapshot (Some position) = Some (Some plan) ->
  check_contribution_cursor maximum count (family_snapshot_scope snapshot) amount
    (family_snapshot_cursor snapshot) (Some plan) = Some (cursor_next plan).
Proof.
  intros maximum count snapshot position amount plan positive bound.
  rewrite positive_contribution_uses_existing_cursor_check by exact positive.
  unfold family_bind_cursor in bound.
  destruct (check_fee_cursor _ _ _ _ _) as [result|] eqn:checked; [|discriminate].
  inversion bound; subst plan.
  pose proof (checked_fee_cursor_is_scoped_bounded_successor _ _ _ _ _ _ checked) as facts.
  decompose [and] facts. congruence.
Qed.

Definition family_cursor_base_valid maximum count snapshot :=
  (cursor_revision (family_snapshot_cursor snapshot) <=? maximum) &&
  (cursor_position (family_snapshot_cursor snapshot) <? count).

Definition family_cursor_pair_valid maximum count expected pair :=
  if list_eq_dec Nat.eq_dec (family_snapshot_custodies pair) expected then
    (length expected =? count) &&
    family_cursor_base_valid maximum count (family_snapshot_resource pair) &&
    family_cursor_base_valid maximum count (family_snapshot_fee pair)
  else false.

Definition family_bindable_case maximum count pair transition :=
  match family_bind_cursor maximum count (family_snapshot_resource pair)
      (family_resource_successor transition),
    family_bind_cursor maximum count (family_snapshot_fee pair)
      (family_fee_successor transition) with
  | Some _, Some _ => true
  | _, _ => false
  end.

Definition family_capture_preflight maximum count pair transitions :=
  forallb (family_bindable_case maximum count pair) transitions.

Theorem family_preflight_covers_every_outcome : forall maximum count pair transitions transition,
  family_capture_preflight maximum count pair transitions = true ->
  In transition transitions ->
  exists resource fee,
    family_bind_cursor maximum count (family_snapshot_resource pair)
      (family_resource_successor transition) = Some resource /\
    family_bind_cursor maximum count (family_snapshot_fee pair)
      (family_fee_successor transition) = Some fee.
Proof.
  intros maximum count pair transitions transition checked member.
  unfold family_capture_preflight in checked. rewrite forallb_forall in checked.
  specialize (checked transition member). unfold family_bindable_case in checked.
  destruct (family_bind_cursor _ _ (family_snapshot_resource pair) _) as [resource|] eqn:r;
    destruct (family_bind_cursor _ _ (family_snapshot_fee pair) _) as [fee|] eqn:f;
    try discriminate.
  exists resource, fee. auto.
Qed.

Theorem family_preflight_rejects_any_exhausted_resource_successor :
  forall maximum count pair transitions transition position,
  In transition transitions ->
  family_resource_successor transition = Some position ->
  maximum <= cursor_revision (family_snapshot_cursor (family_snapshot_resource pair)) ->
  family_capture_preflight maximum count pair transitions = false.
Proof.
  intros maximum count pair transitions transition position member successor exhausted.
  destruct (family_capture_preflight maximum count pair transitions) eqn:checked; [|reflexivity].
  destruct (family_preflight_covers_every_outcome _ _ _ _ _ checked member) as [resource [fee [bound _]]].
  rewrite successor, family_positive_cursor_rejects_exhausted_capture in bound by exact exhausted. discriminate.
Qed.

Theorem family_preflight_rejects_any_exhausted_fee_successor :
  forall maximum count pair transitions transition position,
  In transition transitions ->
  family_fee_successor transition = Some position ->
  maximum <= cursor_revision (family_snapshot_cursor (family_snapshot_fee pair)) ->
  family_capture_preflight maximum count pair transitions = false.
Proof.
  intros maximum count pair transitions transition position member successor exhausted.
  destruct (family_capture_preflight maximum count pair transitions) eqn:checked; [|reflexivity].
  destruct (family_preflight_covers_every_outcome _ _ _ _ _ checked member) as [resource [fee [_ bound]]].
  rewrite successor, family_positive_cursor_rejects_exhausted_capture in bound by exact exhausted. discriminate.
Qed.

Theorem family_all_zero_transitions_pass_preflight_at_any_revision :
  forall maximum count pair transitions,
  (forall transition, In transition transitions ->
    family_resource_successor transition = None /\ family_fee_successor transition = None) ->
  family_capture_preflight maximum count pair transitions = true.
Proof.
  intros maximum count pair transitions absent. unfold family_capture_preflight.
  rewrite forallb_forall. intros transition member. destruct (absent transition member) as [r f].
  unfold family_bindable_case. rewrite r, f. reflexivity.
Qed.

Record family_capture_policy := {
  family_policy_snapshot : phlo_snapshot;
  family_policy_cursors : family_cursor_snapshot_pair;
  family_policy_transitions : list family_cursor_transition
}.

Definition prepare_family_capture_policy maximum expected intent snapshot pair transitions :=
  let count := funding_sources (snapshot_domain snapshot) in
  if check_phlo_family_intent intent snapshot &&
     check_phlo_snapshot snapshot &&
     family_cursor_pair_valid maximum count expected pair &&
     (length transitions =? length (snapshot_branches snapshot)) &&
     family_capture_preflight maximum count pair transitions then
    Some {| family_policy_snapshot := snapshot; family_policy_cursors := pair;
            family_policy_transitions := transitions |}
  else None.

Record family_case_capture := {
  family_capture_snapshot : phlo_snapshot;
  family_capture_index : nat;
  family_capture_branch : nat;
  family_capture_resource_plan : option fee_cursor_plan;
  family_capture_fee_plan : option fee_cursor_plan
}.

Definition capture_family_case maximum policy index :=
  let snapshot := family_policy_snapshot policy in
  let pair := family_policy_cursors policy in
  let count := funding_sources (snapshot_domain snapshot) in
  match nth_error (snapshot_branches snapshot) index,
    nth_error (family_policy_transitions policy) index with
  | Some branch, Some transition =>
      match family_bind_cursor maximum count (family_snapshot_resource pair)
          (family_resource_successor transition),
        family_bind_cursor maximum count (family_snapshot_fee pair)
          (family_fee_successor transition) with
      | Some resource, Some fee =>
          Some {| family_capture_snapshot := snapshot; family_capture_index := index;
                  family_capture_branch := branch; family_capture_resource_plan := resource;
                  family_capture_fee_plan := fee |}
      | _, _ => None
      end
  | _, _ => None
  end.

Theorem family_capture_uses_one_index_for_amounts_and_cursors : forall maximum policy index capture,
  capture_family_case maximum policy index = Some capture ->
  family_capture_index capture = index /\
  family_capture_snapshot capture = family_policy_snapshot policy /\
  exists branch transition,
    nth_error (snapshot_branches (family_policy_snapshot policy)) index = Some branch /\
    nth_error (family_policy_transitions policy) index = Some transition /\
    family_capture_branch capture = branch /\
    family_bind_cursor maximum (funding_sources (snapshot_domain (family_policy_snapshot policy)))
      (family_snapshot_resource (family_policy_cursors policy))
      (family_resource_successor transition) = Some (family_capture_resource_plan capture) /\
    family_bind_cursor maximum (funding_sources (snapshot_domain (family_policy_snapshot policy)))
      (family_snapshot_fee (family_policy_cursors policy))
      (family_fee_successor transition) = Some (family_capture_fee_plan capture).
Proof.
  intros maximum policy index capture checked. unfold capture_family_case in checked.
  destruct (nth_error (snapshot_branches _) index) as [branch|] eqn:b; [|discriminate].
  destruct (nth_error (family_policy_transitions _) index) as [transition|] eqn:t; [|discriminate].
  destruct (family_bind_cursor _ _ (family_snapshot_resource _) _) as [resource|] eqn:r; [|discriminate].
  destruct (family_bind_cursor _ _ (family_snapshot_fee _) _) as [fee|] eqn:f; [|discriminate].
  inversion checked; subst capture. simpl. split; [reflexivity|]. split; [reflexivity|].
  exists branch, transition. auto.
Qed.

Theorem family_capture_cannot_mix_distinct_indices : forall maximum policy first second capture,
  capture_family_case maximum policy first = Some capture ->
  capture_family_case maximum policy second = Some capture -> first = second.
Proof.
  intros maximum policy first second capture a b.
  apply family_capture_uses_one_index_for_amounts_and_cursors in a.
  apply family_capture_uses_one_index_for_amounts_and_cursors in b.
  destruct a as [a _]. destruct b as [b _]. congruence.
Qed.

Definition family_case_native_amount maximum capture source :=
  let snapshot := family_capture_snapshot capture in
  let domain := snapshot_domain snapshot in
  let branch := family_capture_branch capture in
  lower_phlo_amounts maximum
    (phlo_source_holds domain (snapshot_branches snapshot) (snapshot_plans snapshot) source)
    (source_draw (funding_obligations domain) (snapshot_plans snapshot branch) source)
    (snapshot_plans snapshot branch source 0).

Theorem family_prepared_capture_retains_checked_inputs :
  forall maximum expected intent snapshot pair transitions policy,
  prepare_family_capture_policy maximum expected intent snapshot pair transitions = Some policy ->
  family_policy_snapshot policy = snapshot /\
  family_policy_cursors policy = pair /\
  family_policy_transitions policy = transitions /\
  check_phlo_snapshot snapshot = true /\
  check_phlo_family_intent intent snapshot = true /\
  family_snapshot_custodies pair = expected /\
  family_capture_preflight maximum (funding_sources (snapshot_domain snapshot)) pair transitions = true.
Proof.
  intros maximum expected intent snapshot pair transitions policy checked.
  unfold prepare_family_capture_policy in checked.
  destruct (_ && _) eqn:valid; [|discriminate]. inversion checked; subst policy. simpl.
  rewrite !andb_true_iff in valid.
  destruct valid as [[[[intent_valid snapshot_valid] pair_valid] _] preflight].
  unfold family_cursor_pair_valid in pair_valid.
  destruct (list_eq_dec Nat.eq_dec (family_snapshot_custodies pair) expected); [|discriminate].
  repeat split; assumption || reflexivity.
Qed.

Theorem family_checked_capture_native_amounts_conserve :
  forall maximum expected intent snapshot pair transitions policy index capture source,
  prepare_family_capture_policy maximum expected intent snapshot pair transitions = Some policy ->
  capture_family_case maximum policy index = Some capture ->
  source < funding_sources (snapshot_domain snapshot) ->
  phlo_source_holds (snapshot_domain snapshot) (snapshot_branches snapshot) (snapshot_plans snapshot) source <= maximum ->
  exists amounts, family_case_native_amount maximum capture source = Some amounts /\
    native_acquisition amounts + native_fee amounts + native_refund amounts = native_hold amounts /\
    native_hold amounts <= maximum.
Proof.
  intros maximum expected intent snapshot pair transitions policy index capture source prepared captured inside bounded.
  pose proof (family_prepared_capture_retains_checked_inputs _ _ _ _ _ _ _ prepared)
    as [same_snapshot [_ [_ [checked _]]]].
  pose proof (family_capture_uses_one_index_for_amounts_and_cursors _ _ _ _ captured)
    as [_ [same_capture [branch [transition [indexed [_ [same_branch _]]]]]]].
  rewrite same_snapshot in same_capture, indexed.
  assert (member : In branch (snapshot_branches snapshot)) by (eapply nth_error_In; exact indexed).
  unfold check_phlo_snapshot in checked.
  destruct (checked_family_has_native_amounts _ _ _ _ _ _ _ _ _ _ branch source maximum
    checked member inside bounded) as [amounts lowered].
  exists amounts. split.
  - unfold family_case_native_amount. now rewrite same_capture, same_branch.
  - pose proof (lowered_phlo_amounts_conserve_and_fit _ _ _ _ _ lowered) as facts.
    decompose [and] facts. split; congruence.
Qed.

Definition family_branch_resource_amount snapshot branch :=
  funding_sum (funding_sources (snapshot_domain snapshot))
    (fun source => source_draw (funding_obligations (snapshot_domain snapshot))
      (snapshot_plans snapshot branch) source - snapshot_plans snapshot branch source 0).

Definition family_branch_fee_amount snapshot branch :=
  funding_sum (funding_sources (snapshot_domain snapshot))
    (fun source => snapshot_plans snapshot branch source 0).

Definition family_selection_amount_correspondence snapshot transitions :=
  forall index branch transition,
    nth_error (snapshot_branches snapshot) index = Some branch ->
    nth_error transitions index = Some transition ->
    (family_branch_resource_amount snapshot branch = 0 <->
      family_resource_successor transition = None) /\
    (family_branch_fee_amount snapshot branch = 0 <->
      family_fee_successor transition = None).

Theorem family_bound_cursor_preserves_amount_correspondence :
  forall maximum count snapshot successor amount plan,
  family_cursor_base_valid maximum count snapshot = true ->
  (amount = 0 <-> successor = None) ->
  family_bind_cursor maximum count snapshot successor = Some plan ->
  exists result, check_contribution_cursor maximum count (family_snapshot_scope snapshot)
    amount (family_snapshot_cursor snapshot) plan = Some result.
Proof.
  intros maximum count snapshot successor amount plan valid correspondence bound.
  unfold family_cursor_base_valid in valid.
  rewrite andb_true_iff, Nat.leb_le, Nat.ltb_lt in valid.
  destruct valid as [revision position]. destruct amount as [|amount].
  - assert (absent : successor = None) by (apply correspondence; reflexivity).
    subst successor. simpl in bound. inversion bound; subst plan.
    exists (family_snapshot_cursor snapshot). now apply zero_contribution_accepts_every_valid_revision.
  - destruct successor as [next|].
    + destruct plan as [actual|].
      * exists (cursor_next actual). eapply family_positive_cursor_matches_contribution_check; eauto. lia.
      * unfold family_bind_cursor in bound. destruct (check_fee_cursor _ _ _ _ _); discriminate.
    + assert (S amount = 0) by (apply correspondence; reflexivity). lia.
Qed.

Theorem family_positive_resource_amount_needs_family_headroom :
  forall maximum count snapshot pair transitions index branch transition,
  family_selection_amount_correspondence snapshot transitions ->
  nth_error (snapshot_branches snapshot) index = Some branch ->
  nth_error transitions index = Some transition ->
  0 < family_branch_resource_amount snapshot branch ->
  maximum <= cursor_revision (family_snapshot_cursor (family_snapshot_resource pair)) ->
  family_capture_preflight maximum count pair transitions = false.
Proof.
  intros maximum count snapshot pair transitions index branch transition correspondence b t positive exhausted.
  destruct (correspondence index branch transition b t) as [resource _].
  destruct (family_resource_successor transition) as [position|] eqn:successor.
  - eapply family_preflight_rejects_any_exhausted_resource_successor; eauto.
    eapply nth_error_In; exact t.
  - assert (family_branch_resource_amount snapshot branch = 0) by (apply resource; reflexivity). lia.
Qed.

Theorem family_positive_fee_amount_needs_family_headroom :
  forall maximum count snapshot pair transitions index branch transition,
  family_selection_amount_correspondence snapshot transitions ->
  nth_error (snapshot_branches snapshot) index = Some branch ->
  nth_error transitions index = Some transition ->
  0 < family_branch_fee_amount snapshot branch ->
  maximum <= cursor_revision (family_snapshot_cursor (family_snapshot_fee pair)) ->
  family_capture_preflight maximum count pair transitions = false.
Proof.
  intros maximum count snapshot pair transitions index branch transition correspondence b t positive exhausted.
  destruct (correspondence index branch transition b t) as [_ fee].
  destruct (family_fee_successor transition) as [position|] eqn:successor.
  - eapply family_preflight_rejects_any_exhausted_fee_successor; eauto.
    eapply nth_error_In; exact t.
  - assert (family_branch_fee_amount snapshot branch = 0) by (apply fee; reflexivity). lia.
Qed.

Theorem family_captured_branch_checks_both_contribution_dimensions :
  forall maximum policy index capture,
  family_selection_amount_correspondence (family_policy_snapshot policy) (family_policy_transitions policy) ->
  family_cursor_base_valid maximum (funding_sources (snapshot_domain (family_policy_snapshot policy)))
    (family_snapshot_resource (family_policy_cursors policy)) = true ->
  family_cursor_base_valid maximum (funding_sources (snapshot_domain (family_policy_snapshot policy)))
    (family_snapshot_fee (family_policy_cursors policy)) = true ->
  capture_family_case maximum policy index = Some capture ->
  exists resource_after fee_after,
    check_contribution_cursor maximum (funding_sources (snapshot_domain (family_policy_snapshot policy)))
      (family_snapshot_scope (family_snapshot_resource (family_policy_cursors policy)))
      (family_branch_resource_amount (family_policy_snapshot policy) (family_capture_branch capture))
      (family_snapshot_cursor (family_snapshot_resource (family_policy_cursors policy)))
      (family_capture_resource_plan capture) = Some resource_after /\
    check_contribution_cursor maximum (funding_sources (snapshot_domain (family_policy_snapshot policy)))
      (family_snapshot_scope (family_snapshot_fee (family_policy_cursors policy)))
      (family_branch_fee_amount (family_policy_snapshot policy) (family_capture_branch capture))
      (family_snapshot_cursor (family_snapshot_fee (family_policy_cursors policy)))
      (family_capture_fee_plan capture) = Some fee_after.
Proof.
  intros maximum policy index capture correspondence resource_valid fee_valid checked.
  destruct (family_capture_uses_one_index_for_amounts_and_cursors _ _ _ _ checked)
    as [_ [_ [branch [transition [b [t [selected [r f]]]]]]]].
  destruct (correspondence index branch transition b t) as [resource_correspondence fee_correspondence].
  rewrite selected.
  destruct (family_bound_cursor_preserves_amount_correspondence _ _ _ _ _ _
    resource_valid resource_correspondence r) as [resource_after resource_checked].
  destruct (family_bound_cursor_preserves_amount_correspondence _ _ _ _ _ _
    fee_valid fee_correspondence f) as [fee_after fee_checked].
  exists resource_after, fee_after. auto.
Qed.

Definition family_native_hold_preflight maximum snapshot :=
  forallb (fun source =>
    phlo_source_holds (snapshot_domain snapshot) (snapshot_branches snapshot)
      (snapshot_plans snapshot) source <=? maximum)
    (seq 0 (funding_sources (snapshot_domain snapshot))).

Theorem family_native_hold_preflight_exact : forall maximum snapshot,
  family_native_hold_preflight maximum snapshot = true <->
  forall source, source < funding_sources (snapshot_domain snapshot) ->
    phlo_source_holds (snapshot_domain snapshot) (snapshot_branches snapshot)
      (snapshot_plans snapshot) source <= maximum.
Proof.
  intros maximum snapshot. unfold family_native_hold_preflight.
  rewrite forallb_forall. split.
  - intros checked source inside. apply Nat.leb_le, checked, in_seq. lia.
  - intros bounded source included. apply Nat.leb_le, bounded.
    apply in_seq in included. lia.
Qed.

Theorem family_native_preflight_covers_every_branch_and_source : forall maximum snapshot,
  check_phlo_snapshot snapshot = true ->
  family_native_hold_preflight maximum snapshot = true ->
  forall branch source,
  In branch (snapshot_branches snapshot) ->
  source < funding_sources (snapshot_domain snapshot) ->
  exists amounts,
    lower_phlo_amounts maximum
      (phlo_source_holds (snapshot_domain snapshot) (snapshot_branches snapshot)
        (snapshot_plans snapshot) source)
      (source_draw (funding_obligations (snapshot_domain snapshot))
        (snapshot_plans snapshot branch) source)
      (snapshot_plans snapshot branch source 0) = Some amounts /\
    native_hold amounts = phlo_source_holds (snapshot_domain snapshot)
      (snapshot_branches snapshot) (snapshot_plans snapshot) source /\
    native_fee amounts = snapshot_plans snapshot branch source 0 /\
    native_acquisition amounts + native_fee amounts =
      source_draw (funding_obligations (snapshot_domain snapshot))
        (snapshot_plans snapshot branch) source /\
    native_acquisition amounts + native_fee amounts + native_refund amounts =
      native_hold amounts /\
    native_hold amounts <= maximum /\ native_acquisition amounts <= maximum /\
    native_fee amounts <= maximum /\ native_refund amounts <= maximum.
Proof.
  intros maximum snapshot checked preflight branch source included inside.
  pose proof (proj1 (family_native_hold_preflight_exact maximum snapshot)
    preflight source inside) as bounded. unfold check_phlo_snapshot in checked.
  destruct (checked_family_has_native_amounts _ _ _ _ _ _ _ _ _ _ branch source maximum
    checked included inside bounded) as [amounts lowered].
  exists amounts. split; [exact lowered|].
  pose proof (lowered_phlo_amounts_conserve_and_fit _ _ _ _ _ lowered) as facts.
  destruct facts as [hold [fee [debit [conservation fits]]]].
  repeat split; try tauto. congruence.
Qed.

Theorem family_native_preflight_covers_captured_case :
  forall maximum expected intent snapshot pair transitions policy index capture source,
  prepare_family_capture_policy maximum expected intent snapshot pair transitions = Some policy ->
  family_native_hold_preflight maximum snapshot = true ->
  capture_family_case maximum policy index = Some capture ->
  source < funding_sources (snapshot_domain snapshot) ->
  exists amounts, family_case_native_amount maximum capture source = Some amounts /\
    native_acquisition amounts + native_fee amounts + native_refund amounts = native_hold amounts /\
    native_hold amounts <= maximum /\ native_acquisition amounts <= maximum /\
    native_fee amounts <= maximum /\ native_refund amounts <= maximum.
Proof.
  intros maximum expected intent snapshot pair transitions policy index capture source
    prepared preflight captured inside.
  pose proof (family_prepared_capture_retains_checked_inputs _ _ _ _ _ _ _ prepared)
    as [same_snapshot [_ [_ [checked _]]]].
  pose proof (family_capture_uses_one_index_for_amounts_and_cursors _ _ _ _ captured)
    as [_ [same_capture [branch [transition [indexed [_ [same_branch _]]]]]]].
  rewrite same_snapshot in same_capture, indexed.
  assert (included : In branch (snapshot_branches snapshot)) by
    (eapply nth_error_In; exact indexed).
  destruct (family_native_preflight_covers_every_branch_and_source maximum snapshot
    checked preflight branch source included inside) as [amounts [lowered facts]].
  exists amounts. split.
  - unfold family_case_native_amount. now rewrite same_capture, same_branch.
  - tauto.
Qed.

Theorem family_native_bounds_do_not_require_aggregate_native_bound : forall maximum,
  0 < maximum ->
  maximum < funding_sum 2 (fun _ => maximum) /\
  forall source, source < 2 ->
    exists amounts, lower_phlo_amounts maximum maximum maximum 1 = Some amounts /\
      native_acquisition amounts + native_fee amounts + native_refund amounts = maximum /\
      native_hold amounts <= maximum /\ native_acquisition amounts <= maximum /\
      native_fee amounts <= maximum /\ native_refund amounts <= maximum.
Proof.
  intros maximum positive. split; [simpl; lia|]. intros source inside.
  assert (bounded : maximum <= maximum /\ maximum <= maximum /\ 1 <= maximum) by lia.
  apply lower_phlo_amounts_acceptance_exact in bounded.
  destruct bounded as [amounts lowered]. exists amounts. split; [exact lowered|].
  pose proof (lowered_phlo_amounts_conserve_and_fit _ _ _ _ _ lowered). tauto.
Qed.

Print Assumptions family_native_hold_preflight_exact.
Print Assumptions family_native_preflight_covers_every_branch_and_source.
Print Assumptions family_native_preflight_covers_captured_case.
Print Assumptions family_native_bounds_do_not_require_aggregate_native_bound.
Print Assumptions family_bound_cursor_keeps_captured_scope_and_revision.
Print Assumptions family_positive_cursor_rejects_exhausted_capture.
Print Assumptions family_positive_cursor_matches_contribution_check.
Print Assumptions family_preflight_covers_every_outcome.
Print Assumptions family_preflight_rejects_any_exhausted_resource_successor.
Print Assumptions family_preflight_rejects_any_exhausted_fee_successor.
Print Assumptions family_all_zero_transitions_pass_preflight_at_any_revision.
Print Assumptions family_capture_uses_one_index_for_amounts_and_cursors.
Print Assumptions family_capture_cannot_mix_distinct_indices.
Print Assumptions family_prepared_capture_retains_checked_inputs.
Print Assumptions family_checked_capture_native_amounts_conserve.
Print Assumptions family_bound_cursor_preserves_amount_correspondence.
Print Assumptions family_positive_resource_amount_needs_family_headroom.
Print Assumptions family_positive_fee_amount_needs_family_headroom.
Print Assumptions family_captured_branch_checks_both_contribution_dimensions.

Record offered_family_capture_policy := {
  offered_policy_payload : list nat;
  offered_policy_offer : signed_phlo_offer;
  offered_policy_value : family_capture_policy
}.

Definition prepare_offered_family_policy maximum expected terms intent snapshot minimum offer payload pair transitions :=
  if check_offered_phlo_family_intent terms intent snapshot minimum offer then
    option_map (fun policy => {| offered_policy_payload := payload;
      offered_policy_offer := offer; offered_policy_value := policy |})
      (prepare_family_capture_policy maximum expected intent snapshot pair transitions)
  else None.

Record offered_family_case_capture := {
  offered_capture_policy : offered_family_capture_policy;
  offered_capture_value : family_case_capture
}.

Definition capture_offered_family_case maximum policy index :=
  option_map (fun capture => {| offered_capture_policy := policy; offered_capture_value := capture |})
    (capture_family_case maximum (offered_policy_value policy) index).

Theorem offered_policy_preserves_checked_authorization :
  forall maximum expected terms intent snapshot minimum offer payload pair transitions policy,
  prepare_offered_family_policy maximum expected terms intent snapshot minimum offer payload pair transitions = Some policy ->
  offered_policy_payload policy = payload /\ offered_policy_offer policy = offer /\
  check_offered_phlo_family_intent terms intent snapshot minimum offer = true /\
  prepare_family_capture_policy maximum expected intent snapshot pair transitions = Some (offered_policy_value policy).
Proof.
  intros maximum expected terms intent snapshot minimum offer payload pair transitions policy prepared.
  unfold prepare_offered_family_policy in prepared.
  destruct (check_offered_phlo_family_intent terms intent snapshot minimum offer) eqn:checked; [|discriminate].
  destruct (prepare_family_capture_policy maximum expected intent snapshot pair transitions) as [value|] eqn:base;
    [|discriminate].
  inversion prepared; subst policy. simpl. repeat split; assumption || reflexivity.
Qed.

Theorem offered_capture_keeps_policy_and_complete_case : forall maximum policy index capture,
  capture_offered_family_case maximum policy index = Some capture ->
  offered_capture_policy capture = policy /\
  capture_family_case maximum (offered_policy_value policy) index = Some (offered_capture_value capture).
Proof.
  intros maximum policy index capture captured. unfold capture_offered_family_case in captured.
  destruct (capture_family_case maximum (offered_policy_value policy) index) as [value|] eqn:base;
    [|discriminate].
  inversion captured; subst capture. simpl. auto.
Qed.

Theorem offered_capture_cannot_change_signed_payload_or_offer :
  forall maximum expected terms intent snapshot minimum offer payload pair transitions policy index capture,
  prepare_offered_family_policy maximum expected terms intent snapshot minimum offer payload pair transitions = Some policy ->
  capture_offered_family_case maximum policy index = Some capture ->
  offered_policy_payload (offered_capture_policy capture) = payload /\
  offered_policy_offer (offered_capture_policy capture) = offer /\
  family_capture_index (offered_capture_value capture) = index.
Proof.
  intros maximum expected terms intent snapshot minimum offer payload pair transitions policy index capture prepared captured.
  apply offered_policy_preserves_checked_authorization in prepared. destruct prepared as [bytes [price _]].
  apply offered_capture_keeps_policy_and_complete_case in captured. destruct captured as [same base].
  rewrite same. apply family_capture_uses_one_index_for_amounts_and_cursors in base. tauto.
Qed.

Print Assumptions offered_policy_preserves_checked_authorization.
Print Assumptions offered_capture_keeps_policy_and_complete_case.
Print Assumptions offered_capture_cannot_change_signed_payload_or_offer.
