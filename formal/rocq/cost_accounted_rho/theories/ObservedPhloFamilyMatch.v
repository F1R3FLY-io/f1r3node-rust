From Stdlib Require Import Lists.List Bool.Bool Arith.PeanoNat Lia Sorting.Permutation.
From CostAccountedRho Require Import SignedPhloCapture SignedPhloFunding SignedPhloControls
  PrepaidResourceDischarge EconomicFailureSummary CostAccountedSyntax FeeCursorTransition.
Import ListNotations.

Definition economic_summary_eq_dec : forall (left right : economic_failure_summary),
  {left = right} + {left <> right}.
Proof. decide equality; apply Bool.bool_dec. Defined.

Inductive observed_outcome_key :=
| ObservedRejected
| ObservedAccepted (summary : economic_failure_summary).

Definition observed_outcome_eq_dec : forall (left right : observed_outcome_key),
  {left = right} + {left <> right}.
Proof. decide equality; apply economic_summary_eq_dec. Defined.

Definition outcome_observation outcome :=
  match outcome with
  | PhloAdmissionRejected => ObservedRejected
  | PhloAccepted failures => ObservedAccepted (economic_summary failures)
  end.

Definition observed_equal {A} (decide : forall (left right : A), {left = right} + {left <> right}) left right :=
  if decide left right then true else false.

Theorem observed_equal_exact : forall A decide (left right : A),
  observed_equal decide left right = true <-> left = right.
Proof. intros. unfold observed_equal. destruct (decide left right); split; auto; discriminate || contradiction. Qed.

Definition execution_case_match expected actual :=
  same_resources (case_available expected) (case_available actual) &&
  same_resources (case_required expected) (case_required actual) &&
  same_resources (case_used expected) (case_used actual) &&
  same_resources (case_unused expected) (case_unused actual) &&
  same_resources (case_fresh expected) (case_fresh actual) &&
  observed_equal observed_outcome_eq_dec
    (outcome_observation (case_outcome expected)) (outcome_observation (case_outcome actual)).

Definition execution_case_equivalent expected actual :=
  Permutation (case_available expected) (case_available actual) /\
  Permutation (case_required expected) (case_required actual) /\
  Permutation (case_used expected) (case_used actual) /\
  Permutation (case_unused expected) (case_unused actual) /\
  Permutation (case_fresh expected) (case_fresh actual) /\
  outcome_observation (case_outcome expected) = outcome_observation (case_outcome actual).

Theorem execution_case_match_exact : forall expected actual,
  execution_case_match expected actual = true <-> execution_case_equivalent expected actual.
Proof.
  intros. unfold execution_case_match, execution_case_equivalent.
  rewrite !andb_true_iff, !same_resources_exact, observed_equal_exact. tauto.
Qed.

Theorem outcome_failure_order_does_not_change_observation : forall left right,
  Permutation left right -> outcome_observation (PhloAccepted left) = outcome_observation (PhloAccepted right).
Proof. intros. simpl. f_equal. now apply economic_summary_permutation_invariant. Qed.

Theorem duplicate_failure_report_does_not_change_observation : forall failure rest,
  outcome_observation (PhloAccepted (failure :: failure :: rest)) =
  outcome_observation (PhloAccepted (failure :: rest)).
Proof.
  intros. simpl. rewrite economic_join_associative, economic_join_idempotent. reflexivity.
Qed.

Theorem matching_resources_preserve_exact_multiplicity : forall expected actual,
  execution_case_match expected actual = true ->
  forall resource,
  resource_count (case_available expected) resource = resource_count (case_available actual) resource /\
  resource_count (case_required expected) resource = resource_count (case_required actual) resource /\
  resource_count (case_used expected) resource = resource_count (case_used actual) resource /\
  resource_count (case_unused expected) resource = resource_count (case_unused actual) resource /\
  resource_count (case_fresh expected) resource = resource_count (case_fresh actual) resource.
Proof.
  intros expected actual matched resource. apply execution_case_match_exact in matched.
  unfold execution_case_equivalent in matched. destruct matched as [a [b [c [d [e _]]]]].
  unfold resource_count. repeat split;
    apply (proj1 (Permutation_count_occ prepaid_key_eq_dec _ _)); assumption.
Qed.

Definition observed_case_at snapshot actual position :=
  match nth_error (snapshot_branches snapshot) position with
  | None => false
  | Some branch => execution_case_match (snapshot_cases snapshot branch) actual
  end.

Definition observed_matching_positions snapshot actual :=
  filter (observed_case_at snapshot actual) (seq 0 (length (snapshot_branches snapshot))).

Theorem observed_matching_position_exact : forall snapshot actual position,
  In position (observed_matching_positions snapshot actual) <->
  exists branch, nth_error (snapshot_branches snapshot) position = Some branch /\
    execution_case_equivalent (snapshot_cases snapshot branch) actual.
Proof.
  intros snapshot actual position. unfold observed_matching_positions. rewrite filter_In.
  unfold observed_case_at. destruct (nth_error (snapshot_branches snapshot) position) as [branch|] eqn:indexed.
  - rewrite execution_case_match_exact. split.
    + intros [_ matched]. exists branch. auto.
    + intros [found [same matched]]. inversion same; subst found. split; [|exact matched].
      apply in_seq. split; [lia|]. apply nth_error_Some. rewrite indexed. discriminate.
  - split; [intros [_ impossible]; discriminate|]. intros [branch [impossible _]]. discriminate.
Qed.

Definition observed_context_match snapshot controls schedule :=
  observed_equal phlo_controls_eq_dec (snapshot_controls snapshot) controls &&
  observed_equal phlo_schedule_eq_dec (snapshot_schedule snapshot) schedule.

Theorem observed_context_match_exact : forall snapshot controls schedule,
  observed_context_match snapshot controls schedule = true <->
  snapshot_controls snapshot = controls /\ snapshot_schedule snapshot = schedule.
Proof. intros. unfold observed_context_match. rewrite andb_true_iff, !observed_equal_exact. reflexivity. Qed.

Definition observed_location_case location :=
  let resource := {| prepaid_location := location; prepaid_class := 0;
    prepaid_terms := 0; prepaid_authority := SUnit |} in
  {| case_available := []; case_required := [resource]; case_used := [];
     case_unused := []; case_fresh := [resource]; case_outcome := PhloAccepted [] |}.

Theorem scalar_charge_does_not_identify_execution : forall schedule,
  case_charge schedule (observed_location_case 0) = case_charge schedule (observed_location_case 1) /\
  execution_case_match (observed_location_case 0) (observed_location_case 1) = false.
Proof. intros. split; reflexivity. Qed.

Record observed_scoped_transition := {
  observed_transition_scope : nat;
  observed_transition_expected : option fee_cursor;
  observed_transition_next : fee_cursor
}.

Record observed_settlement_record := {
  observed_sources : list nat;
  observed_holds : list nat;
  observed_debits : list nat;
  observed_fees : list nat;
  observed_refunds : list nat;
  observed_obligations : list phlo_obligation_key;
  observed_amounts : list nat;
  observed_eligibility : list (list bool);
  observed_assignment : list (list nat);
  observed_resource_transition : option observed_scoped_transition;
  observed_fee_transition : option observed_scoped_transition
}.

Definition observed_cursor_eq_dec : forall (left right : fee_cursor), {left = right} + {left <> right}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition observed_transition_eq_dec : forall (left right : observed_scoped_transition),
  {left = right} + {left <> right}.
Proof. decide equality; try apply Nat.eq_dec; try apply observed_cursor_eq_dec.
  decide equality; apply observed_cursor_eq_dec. Defined.

Definition observed_obligation_eq_dec : forall (left right : phlo_obligation_key),
  {left = right} + {left <> right}.
Proof. decide equality; apply prepaid_key_eq_dec. Defined.

Definition observed_settlement_eq_dec : forall (left right : observed_settlement_record),
  {left = right} + {left <> right}.
Proof.
  decide equality; try (apply list_eq_dec; apply Nat.eq_dec);
    try (apply list_eq_dec; apply observed_obligation_eq_dec);
    try (apply list_eq_dec; apply list_eq_dec; apply Nat.eq_dec);
    try (apply list_eq_dec; apply list_eq_dec; apply Bool.bool_dec);
    decide equality; apply observed_transition_eq_dec.
Defined.

Definition select_equivalent_observed_positions (selections : nat -> observed_settlement_record) positions :=
  match positions with
  | [] => None
  | first :: rest =>
      if forallb (fun position => observed_equal observed_settlement_eq_dec
        (selections position) (selections first)) rest then Some first else None
  end.

Theorem selected_observed_position_has_one_complete_settlement : forall selections positions selected,
  select_equivalent_observed_positions selections positions = Some selected ->
  In selected positions /\ forall position, In position positions -> selections position = selections selected.
Proof.
  intros selections [|first rest] selected chosen; [discriminate|].
  unfold select_equivalent_observed_positions in chosen.
  destruct (forallb _ _) eqn:uniform; [|discriminate]. inversion chosen; subst selected.
  split; [now left|]. intros position [same|included]; [now subst|].
  rewrite forallb_forall in uniform.
  apply (proj1 (observed_equal_exact _ observed_settlement_eq_dec _ _)), uniform, included.
Qed.

Theorem equivalent_observed_aliases_are_accepted : forall selections first rest,
  (forall position, In position rest -> selections position = selections first) ->
  select_equivalent_observed_positions selections (first :: rest) = Some first.
Proof.
  intros selections first rest uniform. unfold select_equivalent_observed_positions.
  assert (forallb (fun position => observed_equal observed_settlement_eq_dec
    (selections position) (selections first)) rest = true).
  { apply forallb_forall. intros position included. apply observed_equal_exact. now apply uniform. }
  now rewrite H.
Qed.

Theorem conflicting_observed_aliases_are_rejected : forall selections positions first second,
  In first positions -> In second positions -> selections first <> selections second ->
  select_equivalent_observed_positions selections positions = None.
Proof.
  intros selections positions first second one two different.
  destruct (select_equivalent_observed_positions selections positions) as [selected|] eqn:chosen;
    [|reflexivity].
  apply selected_observed_position_has_one_complete_settlement in chosen.
  destruct chosen as [_ uniform]. exfalso. apply different. rewrite (uniform first one), (uniform second two). reflexivity.
Qed.

Theorem observed_alias_order_preserves_complete_capture : forall selections left right first second,
  Permutation left right ->
  select_equivalent_observed_positions selections left = Some first ->
  select_equivalent_observed_positions selections right = Some second ->
  selections first = selections second.
Proof.
  intros selections left right first second order one two.
  apply selected_observed_position_has_one_complete_settlement in one, two.
  destruct one as [included _]. destruct two as [_ uniform]. apply uniform.
  eapply Permutation_in; eauto.
Qed.

Definition match_observed_phlo_family snapshot controls schedule actual selections :=
  if observed_context_match snapshot controls schedule then
    select_equivalent_observed_positions selections (observed_matching_positions snapshot actual)
  else None.

Theorem matched_observation_binds_context_case_and_settlement : forall snapshot controls schedule actual selections selected,
  match_observed_phlo_family snapshot controls schedule actual selections = Some selected ->
  snapshot_controls snapshot = controls /\ snapshot_schedule snapshot = schedule /\
  (exists branch, nth_error (snapshot_branches snapshot) selected = Some branch /\
     execution_case_equivalent (snapshot_cases snapshot branch) actual) /\
  forall position, In position (observed_matching_positions snapshot actual) ->
    selections position = selections selected.
Proof.
  intros snapshot controls schedule actual selections selected matched.
  unfold match_observed_phlo_family in matched.
  destruct (observed_context_match snapshot controls schedule) eqn:context; [|discriminate].
  apply observed_context_match_exact in context.
  apply selected_observed_position_has_one_complete_settlement in matched.
  destruct matched as [included uniform]. apply observed_matching_position_exact in included. tauto.
Qed.

Theorem matching_case_with_nonuser_failure_retains_no_charge : forall expected actual failures bad schedule,
  execution_case_equivalent expected actual -> case_outcome actual = PhloAccepted failures ->
  In bad failures -> billable_failure bad = false -> case_charge schedule expected = 0.
Proof.
  intros expected actual failures bad schedule matched outcome included unsafe.
  destruct matched as [_ [_ [_ [_ [_ same]]]]]. rewrite outcome in same.
  unfold case_charge. destruct (case_outcome expected) as [|expected_failures] eqn:expected_outcome;
    [discriminate|]. simpl in same. inversion same as [summary].
  rewrite <- economic_summary_refines_retained_charge, summary.
  eapply economic_nonuser_veto_survives_all_siblings; eauto.
Qed.

Theorem matched_family_keeps_original_source_permissions : forall snapshot controls schedule actual selections selected branch source slot key,
  check_phlo_snapshot snapshot = true ->
  match_observed_phlo_family snapshot controls schedule actual selections = Some selected ->
  nth_error (snapshot_branches snapshot) selected = Some branch ->
  source < funding_sources (snapshot_domain snapshot) ->
  slot < funding_obligations (snapshot_domain snapshot) ->
  nth_error (case_obligation_keys (snapshot_cases snapshot branch)) slot = Some key ->
  funding_permission (snapshot_domain snapshot) branch source key = false ->
  snapshot_plans snapshot branch source slot = 0.
Proof.
  intros snapshot controls schedule actual selections selected branch source slot key checked
    matched indexed source_bound slot_bound found denied.
  assert (included : In branch (snapshot_branches snapshot)) by (eapply nth_error_In; exact indexed).
  unfold check_phlo_snapshot in checked. apply accepted_phlo_family_exact in checked.
  destruct checked as [_ [_ [_ branches]]]. specialize (branches branch included).
  eapply accepted_flow_requires_permission_for_exact_resource; eauto.
Qed.

Definition bounded_match_observed_phlo_family fuel snapshot controls schedule actual selections :=
  if length (snapshot_branches snapshot) <=? fuel then
    match_observed_phlo_family snapshot controls schedule actual selections
  else None.

Theorem incomplete_observed_family_scan_is_rejected : forall fuel snapshot controls schedule actual selections,
  fuel < length (snapshot_branches snapshot) ->
  bounded_match_observed_phlo_family fuel snapshot controls schedule actual selections = None.
Proof.
  intros. unfold bounded_match_observed_phlo_family.
  assert (length (snapshot_branches snapshot) <=? fuel = false) by (apply Nat.leb_gt; assumption).
  now rewrite H0.
Qed.

Theorem late_conflicting_observed_alias_is_rejected : forall selections first prefix late suffix,
  selections first <> selections late ->
  select_equivalent_observed_positions selections (first :: prefix ++ late :: suffix) = None.
Proof.
  intros. eapply conflicting_observed_aliases_are_rejected; [now left| |exact H].
  right. apply in_or_app. right. now left.
Qed.

Theorem duplicate_equivalent_alias_keeps_selected_capture : forall selections first rest,
  select_equivalent_observed_positions selections (first :: rest) = Some first ->
  select_equivalent_observed_positions selections (first :: first :: rest) = Some first.
Proof.
  intros selections first rest accepted. apply selected_observed_position_has_one_complete_settlement in accepted.
  destruct accepted as [_ uniform]. apply equivalent_observed_aliases_are_accepted. exact uniform.
Qed.

Theorem execution_case_equivalence_symmetric : forall left right,
  execution_case_equivalent left right -> execution_case_equivalent right left.
Proof.
  intros left right [a [b [c [d [e f]]]]]. repeat split; try (apply Permutation_sym; assumption).
  symmetry. assumption.
Qed.

Theorem execution_case_equivalence_transitive : forall left middle right,
  execution_case_equivalent left middle -> execution_case_equivalent middle right ->
  execution_case_equivalent left right.
Proof.
  intros left middle right [a [b [c [d [e f]]]]] [g [h [i [j [k l]]]]].
  repeat split; try (eapply Permutation_trans; eassumption). congruence.
Qed.

Theorem observed_report_reordering_preserves_case_match : forall expected left right,
  execution_case_equivalent left right -> execution_case_match expected left = execution_case_match expected right.
Proof.
  intros expected left right same.
  destruct (execution_case_match expected left) eqn:first,
    (execution_case_match expected right) eqn:second; try reflexivity.
  - apply execution_case_match_exact in first.
    assert (execution_case_match expected right = true).
    { apply execution_case_match_exact. eapply execution_case_equivalence_transitive; eauto. }
    congruence.
  - apply execution_case_match_exact in second.
    assert (execution_case_match expected left = true).
    { apply execution_case_match_exact. eapply execution_case_equivalence_transitive; [exact second|].
      now apply execution_case_equivalence_symmetric. }
    congruence.
Qed.

Theorem observed_report_reordering_preserves_family_match : forall snapshot controls schedule left right selections,
  execution_case_equivalent left right ->
  match_observed_phlo_family snapshot controls schedule left selections =
  match_observed_phlo_family snapshot controls schedule right selections.
Proof.
  intros snapshot controls schedule left right selections same.
  unfold match_observed_phlo_family, observed_matching_positions.
  assert (filter (observed_case_at snapshot left) (seq 0 (length (snapshot_branches snapshot))) =
    filter (observed_case_at snapshot right) (seq 0 (length (snapshot_branches snapshot)))).
  { apply filter_ext. intros position. unfold observed_case_at.
    destruct (nth_error (snapshot_branches snapshot) position); [|reflexivity].
    now apply observed_report_reordering_preserves_case_match. }
  now rewrite H.
Qed.

Print Assumptions incomplete_observed_family_scan_is_rejected.
Print Assumptions late_conflicting_observed_alias_is_rejected.
Print Assumptions duplicate_equivalent_alias_keeps_selected_capture.
Print Assumptions observed_report_reordering_preserves_family_match.
Print Assumptions selected_observed_position_has_one_complete_settlement.
Print Assumptions equivalent_observed_aliases_are_accepted.
Print Assumptions conflicting_observed_aliases_are_rejected.
Print Assumptions observed_alias_order_preserves_complete_capture.
Print Assumptions matched_observation_binds_context_case_and_settlement.
Print Assumptions matching_case_with_nonuser_failure_retains_no_charge.
Print Assumptions matched_family_keeps_original_source_permissions.
Print Assumptions execution_case_match_exact.
Print Assumptions outcome_failure_order_does_not_change_observation.
Print Assumptions duplicate_failure_report_does_not_change_observation.
Print Assumptions matching_resources_preserve_exact_multiplicity.
Print Assumptions observed_matching_position_exact.
Print Assumptions observed_context_match_exact.
Print Assumptions scalar_charge_does_not_identify_execution.

Definition retained_execution_case_match expected actual expected_output actual_output :=
  execution_case_match expected actual && same_resources expected_output actual_output.

Theorem retained_execution_case_match_exact : forall expected actual expected_output actual_output,
  retained_execution_case_match expected actual expected_output actual_output = true <->
  execution_case_equivalent expected actual /\ Permutation expected_output actual_output.
Proof.
  intros. unfold retained_execution_case_match.
  rewrite andb_true_iff, execution_case_match_exact, same_resources_exact. reflexivity.
Qed.

Theorem retained_matching_preserves_every_output_quantity : forall expected actual expected_output actual_output resource,
  retained_execution_case_match expected actual expected_output actual_output = true ->
  resource_count expected_output resource = resource_count actual_output resource.
Proof.
  intros. apply retained_execution_case_match_exact in H. destruct H as [_ same].
  unfold resource_count. now apply (proj1 (Permutation_count_occ prepaid_key_eq_dec _ _)).
Qed.

Theorem empty_retained_output_preserves_existing_match : forall expected actual,
  retained_execution_case_match expected actual [] [] = execution_case_match expected actual.
Proof. intros. unfold retained_execution_case_match. simpl. apply andb_true_r. Qed.

Theorem consumption_match_cannot_hide_an_extra_retained_output : forall expected actual resource,
  retained_execution_case_match expected actual [] [resource] = false.
Proof.
  intros. destruct (retained_execution_case_match expected actual [] [resource]) eqn:matched; auto.
  apply retained_execution_case_match_exact in matched. destruct matched as [_ impossible].
  apply Permutation_length in impossible. discriminate.
Qed.

Definition retained_outputs_allowed candidate (outputs : list prepaid_resource_key) :=
  match outputs, case_outcome candidate with
  | [], _ => true
  | _, PhloAccepted [] => true
  | _, _ => false
  end.

Theorem failed_execution_cannot_retain_new_acquisition : forall candidate outputs,
  case_outcome candidate <> PhloAccepted [] ->
  retained_outputs_allowed candidate outputs = true -> outputs = [].
Proof.
  intros candidate [|output rest] failed accepted; auto.
  unfold retained_outputs_allowed in accepted.
  destruct (case_outcome candidate) as [|[|failure failures]] eqn:outcome; try discriminate.
  contradiction.
Qed.

Definition retained_case_obligation_keys candidate outputs :=
  map (fun key => (false, key)) (case_obligation_keys candidate) ++
  map (fun resource => (true, PhloResourceObligation resource)) outputs.

Definition retained_case_amounts schedule candidate outputs :=
  case_obligation_amounts schedule candidate ++
  map (fun resource => resource_phlo schedule resource * phlo_actual_price schedule) outputs.

Theorem retained_obligation_purposes_are_distinct : forall left right,
  (false, PhloResourceObligation left) <> (true, PhloResourceObligation right).
Proof. intros left right same. discriminate. Qed.

Theorem retained_projection_keeps_complete_key_count : forall candidate outputs,
  length (retained_case_obligation_keys candidate outputs) =
  length (case_obligation_keys candidate) + length outputs.
Proof. intros. unfold retained_case_obligation_keys. rewrite length_app, !length_map. reflexivity. Qed.

Theorem retained_projection_sums_to_separate_backing : forall schedule candidate outputs,
  fold_right Nat.add 0 (retained_case_amounts schedule candidate outputs) =
    case_charge schedule candidate + execution_phlo schedule outputs * phlo_actual_price schedule.
Proof.
  intros schedule candidate outputs. unfold retained_case_amounts.
  rewrite fold_right_app.
  rewrite sum_priced_resource_occurrences.
  rewrite <- projected_obligations_sum_to_retained_charge.
  generalize (case_obligation_amounts schedule candidate). intro amounts.
  induction amounts; simpl; lia.
Qed.

Definition retained_matching_positions snapshot outputs actual actual_output :=
  filter (fun position =>
    match nth_error (snapshot_branches snapshot) position with
    | None => false
    | Some branch => retained_execution_case_match
        (snapshot_cases snapshot branch) actual (outputs branch) actual_output
    end) (seq 0 (length (snapshot_branches snapshot))).

Theorem retained_selected_position_has_matching_output : forall snapshot outputs actual actual_output selections selected,
  select_equivalent_observed_positions selections
    (retained_matching_positions snapshot outputs actual actual_output) = Some selected ->
  exists branch, nth_error (snapshot_branches snapshot) selected = Some branch /\
    execution_case_equivalent (snapshot_cases snapshot branch) actual /\
    Permutation (outputs branch) actual_output.
Proof.
  intros snapshot outputs actual actual_output selections selected accepted.
  apply selected_observed_position_has_one_complete_settlement in accepted.
  destruct accepted as [member _]. unfold retained_matching_positions in member.
  apply filter_In in member. destruct member as [_ matched].
  destruct (nth_error (snapshot_branches snapshot) selected) as [branch|] eqn:found; try discriminate.
  apply retained_execution_case_match_exact in matched.
  exists branch. tauto.
Qed.

Print Assumptions retained_execution_case_match_exact.
Print Assumptions retained_matching_preserves_every_output_quantity.
Print Assumptions empty_retained_output_preserves_existing_match.
Print Assumptions consumption_match_cannot_hide_an_extra_retained_output.
Print Assumptions failed_execution_cannot_retain_new_acquisition.
Print Assumptions retained_obligation_purposes_are_distinct.
Print Assumptions retained_projection_keeps_complete_key_count.
Print Assumptions retained_projection_sums_to_separate_backing.

Definition match_rooted_retained_phlo_family (wallet_root prepaid_root : nat)
  snapshot controls schedule outputs actual actual_output selections :=
  if Nat.eqb wallet_root prepaid_root && observed_context_match snapshot controls schedule then
    select_equivalent_observed_positions selections
      (retained_matching_positions snapshot outputs actual actual_output)
  else None.

Theorem rooted_retained_match_rejects_cross_root : forall wallet_root prepaid_root
  snapshot controls schedule outputs actual actual_output selections,
  wallet_root <> prepaid_root ->
  match_rooted_retained_phlo_family wallet_root prepaid_root snapshot controls schedule
    outputs actual actual_output selections = None.
Proof.
  intros. unfold match_rooted_retained_phlo_family.
  apply Nat.eqb_neq in H. rewrite H. reflexivity.
Qed.

Theorem rooted_retained_match_preserves_context_resources_and_settlement :
  forall wallet_root prepaid_root snapshot controls schedule outputs actual actual_output selections selected,
  match_rooted_retained_phlo_family wallet_root prepaid_root snapshot controls schedule
    outputs actual actual_output selections = Some selected ->
  wallet_root = prepaid_root /\
  snapshot_controls snapshot = controls /\ snapshot_schedule snapshot = schedule /\
  (exists branch, nth_error (snapshot_branches snapshot) selected = Some branch /\
    execution_case_equivalent (snapshot_cases snapshot branch) actual /\
    Permutation (outputs branch) actual_output) /\
  forall position, In position (retained_matching_positions snapshot outputs actual actual_output) ->
    selections position = selections selected.
Proof.
  intros wallet_root prepaid_root snapshot controls schedule outputs actual actual_output selections selected accepted.
  unfold match_rooted_retained_phlo_family in accepted.
  destruct (Nat.eqb wallet_root prepaid_root && observed_context_match snapshot controls schedule) eqn:context;
    [|discriminate].
  apply andb_true_iff in context. destruct context as [roots context].
  apply Nat.eqb_eq in roots. apply observed_context_match_exact in context.
  pose proof (retained_selected_position_has_matching_output _ _ _ _ _ _ accepted) as matched.
  apply selected_observed_position_has_one_complete_settlement in accepted.
  tauto.
Qed.

Theorem rooted_retained_match_preserves_existing_selection : forall root
  snapshot controls schedule outputs actual actual_output selections,
  snapshot_controls snapshot = controls -> snapshot_schedule snapshot = schedule ->
  match_rooted_retained_phlo_family root root snapshot controls schedule
    outputs actual actual_output selections =
  select_equivalent_observed_positions selections
    (retained_matching_positions snapshot outputs actual actual_output).
Proof.
  intros. unfold match_rooted_retained_phlo_family. rewrite Nat.eqb_refl.
  assert (observed_context_match snapshot controls schedule = true)
    by (apply observed_context_match_exact; auto).
  now rewrite H1.
Qed.

Print Assumptions rooted_retained_match_rejects_cross_root.
Print Assumptions rooted_retained_match_preserves_context_resources_and_settlement.
Print Assumptions rooted_retained_match_preserves_existing_selection.
Print Assumptions retained_selected_position_has_matching_output.

Record quantity_capture_column := {
  quantity_capture_key : bool * phlo_obligation_key;
  quantity_capture_quantity : nat;
  quantity_capture_amount : nat;
  quantity_capture_contributions : list nat
}.

Definition capture_quantity_column (key : nat -> bool * phlo_obligation_key)
    (quantity amount : nat -> nat) (contribution : nat -> nat -> nat)
    (sources : list nat) (column : nat) :=
  {| quantity_capture_key := key column;
     quantity_capture_quantity := quantity column;
     quantity_capture_amount := amount column;
     quantity_capture_contributions := map (fun source => contribution source column) sources |}.

Definition capture_quantity_columns key quantity amount contribution sources columns :=
  map (capture_quantity_column key quantity amount contribution sources) columns.

Theorem quantity_capture_preserves_column_count : forall key quantity amount contribution sources columns,
  length (capture_quantity_columns key quantity amount contribution sources columns) = length columns.
Proof. intros. apply length_map. Qed.

Theorem quantity_capture_binds_every_field : forall key quantity amount contribution sources columns position original,
  nth_error columns position = Some original ->
  nth_error (capture_quantity_columns key quantity amount contribution sources columns) position =
    Some (capture_quantity_column key quantity amount contribution sources original).
Proof. intros. unfold capture_quantity_columns. now rewrite nth_error_map, H. Qed.

Theorem quantity_capture_binds_each_source : forall key quantity amount contribution sources column position original,
  nth_error sources position = Some original ->
  nth_error (quantity_capture_contributions
    (capture_quantity_column key quantity amount contribution sources column)) position =
    Some (contribution original column).
Proof. intros. simpl. now rewrite nth_error_map, H. Qed.

Theorem quantity_capture_preserves_source_totals : forall key quantity amount contribution sources reordered column,
  Permutation sources reordered ->
  fold_right Nat.add 0 (quantity_capture_contributions
    (capture_quantity_column key quantity amount contribution sources column)) =
  fold_right Nat.add 0 (quantity_capture_contributions
    (capture_quantity_column key quantity amount contribution reordered column)).
Proof.
  intros. simpl. apply Permutation_map with (f := fun source => contribution source column) in H.
  induction H; simpl; lia.
Qed.

Theorem quantity_capture_preserves_wallet_debits : forall (contribution : nat -> nat -> nat) source columns reordered,
  Permutation columns reordered ->
  fold_right Nat.add 0 (map (fun column => contribution source column) columns) =
  fold_right Nat.add 0 (map (fun column => contribution source column) reordered).
Proof.
  intros. apply Permutation_map with (f := fun column => contribution source column) in H.
  induction H; simpl; lia.
Qed.

Theorem quantity_capture_preserves_all_columns : forall key quantity amount contribution sources columns reordered,
  Permutation columns reordered ->
  Permutation (capture_quantity_columns key quantity amount contribution sources columns)
    (capture_quantity_columns key quantity amount contribution sources reordered).
Proof. intros. now apply Permutation_map. Qed.

Theorem zero_price_cannot_identify_retained_quantity : forall key contribution sources column first second,
  first <> second ->
  quantity_capture_amount (capture_quantity_column key (fun _ => first) (fun _ => 0)
    contribution sources column) =
  quantity_capture_amount (capture_quantity_column key (fun _ => second) (fun _ => 0)
    contribution sources column) /\
  quantity_capture_quantity (capture_quantity_column key (fun _ => first) (fun _ => 0)
    contribution sources column) <>
  quantity_capture_quantity (capture_quantity_column key (fun _ => second) (fun _ => 0)
    contribution sources column).
Proof. intros. simpl. auto. Qed.

Print Assumptions quantity_capture_preserves_column_count.
Print Assumptions quantity_capture_binds_every_field.
Print Assumptions quantity_capture_binds_each_source.
Print Assumptions quantity_capture_preserves_source_totals.
Print Assumptions quantity_capture_preserves_wallet_debits.
Print Assumptions quantity_capture_preserves_all_columns.
Print Assumptions zero_price_cannot_identify_retained_quantity.

Definition match_metered_rooted_phlo_family complete meter_usage derived_usage
    wallet_root prepaid_root snapshot controls schedule outputs actual actual_output selections :=
  if complete && Nat.eqb meter_usage derived_usage then
    match_rooted_retained_phlo_family wallet_root prepaid_root snapshot controls schedule
      outputs actual actual_output selections
  else None.

Theorem metered_family_rejects_incomplete_observations : forall meter_usage derived_usage
    wallet_root prepaid_root snapshot controls schedule outputs actual actual_output selections,
  match_metered_rooted_phlo_family false meter_usage derived_usage wallet_root prepaid_root
    snapshot controls schedule outputs actual actual_output selections = None.
Proof. reflexivity. Qed.

Theorem metered_family_rejects_different_usage : forall complete meter_usage derived_usage
    wallet_root prepaid_root snapshot controls schedule outputs actual actual_output selections,
  meter_usage <> derived_usage ->
  match_metered_rooted_phlo_family complete meter_usage derived_usage wallet_root prepaid_root
    snapshot controls schedule outputs actual actual_output selections = None.
Proof.
  intros. unfold match_metered_rooted_phlo_family.
  apply Nat.eqb_neq in H. rewrite H, andb_false_r. reflexivity.
Qed.

Theorem metered_family_exact_composition : forall complete meter_usage derived_usage
    wallet_root prepaid_root snapshot controls schedule outputs actual actual_output selections selected,
  match_metered_rooted_phlo_family complete meter_usage derived_usage wallet_root prepaid_root
    snapshot controls schedule outputs actual actual_output selections = Some selected <->
  complete = true /\ meter_usage = derived_usage /\
  match_rooted_retained_phlo_family wallet_root prepaid_root snapshot controls schedule
    outputs actual actual_output selections = Some selected.
Proof.
  intros. unfold match_metered_rooted_phlo_family.
  destruct complete; simpl.
  - destruct (Nat.eqb meter_usage derived_usage) eqn:same.
    + apply Nat.eqb_eq in same. tauto.
    + apply Nat.eqb_neq in same. split; [discriminate|tauto].
  - split; [discriminate|intros [impossible _]; discriminate].
Qed.

Theorem metered_family_preserves_root_context_resources_and_settlement :
  forall complete meter_usage derived_usage wallet_root prepaid_root snapshot controls schedule
    outputs actual actual_output selections selected,
  match_metered_rooted_phlo_family complete meter_usage derived_usage wallet_root prepaid_root
    snapshot controls schedule outputs actual actual_output selections = Some selected ->
  complete = true /\ meter_usage = derived_usage /\ wallet_root = prepaid_root /\
  snapshot_controls snapshot = controls /\ snapshot_schedule snapshot = schedule /\
  (exists branch, nth_error (snapshot_branches snapshot) selected = Some branch /\
    execution_case_equivalent (snapshot_cases snapshot branch) actual /\
    Permutation (outputs branch) actual_output) /\
  forall position, In position (retained_matching_positions snapshot outputs actual actual_output) ->
    selections position = selections selected.
Proof.
  intros. apply metered_family_exact_composition in H.
  destruct H as [complete_checked [usage_checked matched]].
  apply rooted_retained_match_preserves_context_resources_and_settlement in matched.
  tauto.
Qed.

Print Assumptions metered_family_rejects_incomplete_observations.
Print Assumptions metered_family_rejects_different_usage.
Print Assumptions metered_family_exact_composition.
Print Assumptions metered_family_preserves_root_context_resources_and_settlement.
