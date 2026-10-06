From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import CostAccountedSyntax PrepaidResourceDischarge SignedPhloControls
  EligibleFundingAssignment.
Import ListNotations.

Record phlo_execution_case := {
  case_available : list prepaid_resource_key;
  case_required : list prepaid_resource_key;
  case_used : list prepaid_resource_key;
  case_unused : list prepaid_resource_key;
  case_fresh : list prepaid_resource_key;
  case_outcome : phlo_outcome
}.

Definition case_charge schedule candidate :=
  retained_phlo_charge schedule (case_fresh candidate) (case_outcome candidate).

Definition check_case_controls environment machine_max terms schedule bound candidate :=
  check_phlo_execution environment machine_max terms schedule bound
    (case_available candidate) (case_required candidate) (case_used candidate)
    (case_unused candidate) (case_fresh candidate).

Inductive phlo_obligation_key :=
| PhloFeeObligation
| PhloResourceObligation (resource : prepaid_resource_key).

Definition case_obligation_keys candidate :=
  PhloFeeObligation :: map PhloResourceObligation (case_fresh candidate).

Definition case_is_billable candidate :=
  match case_outcome candidate with
  | PhloAdmissionRejected => false
  | PhloAccepted failures => forallb billable_failure failures
  end.

Definition phlo_obligation_value schedule candidate obligation :=
  if case_is_billable candidate then
    match obligation with
    | PhloFeeObligation => 1
    | PhloResourceObligation resource => resource_phlo schedule resource * phlo_actual_price schedule
    end
  else 0.

Definition case_obligation_amounts schedule candidate :=
  map (phlo_obligation_value schedule candidate) (case_obligation_keys candidate).

Definition case_obligation_demand schedule candidate slot :=
  nth slot (case_obligation_amounts schedule candidate) 0.

Definition check_obligation_projection schedule candidate slots supplied :=
  (length (case_obligation_keys candidate) <=? slots) &&
  forallb (fun slot => supplied slot =? case_obligation_demand schedule candidate slot) (seq 0 slots).

Theorem obligation_projection_check_exact : forall schedule candidate slots supplied,
  check_obligation_projection schedule candidate slots supplied = true <->
  length (case_obligation_keys candidate) <= slots /\
  forall slot, slot < slots -> supplied slot = case_obligation_demand schedule candidate slot.
Proof.
  intros. unfold check_obligation_projection.
  rewrite andb_true_iff, Nat.leb_le, funding_forallb_range.
  split; intros [size entries]; split; [exact size| |exact size|].
  - intros. apply Nat.eqb_eq. now apply entries.
  - intros. apply Nat.eqb_eq. now apply entries.
Qed.

Theorem obligation_projection_preserves_every_occurrence : forall candidate,
  length (case_obligation_keys candidate) = S (length (case_fresh candidate)).
Proof. intros. unfold case_obligation_keys. simpl. now rewrite length_map. Qed.

Theorem obligation_projection_retains_resource_identity : forall candidate slot resource,
  nth_error (case_fresh candidate) slot = Some resource ->
  nth_error (case_obligation_keys candidate) (S slot) = Some (PhloResourceObligation resource).
Proof.
  intros. unfold case_obligation_keys. simpl. rewrite nth_error_map, H. reflexivity.
Qed.

Theorem obligation_value_matches_its_key : forall schedule candidate slot key,
  nth_error (case_obligation_keys candidate) slot = Some key ->
  case_obligation_demand schedule candidate slot = phlo_obligation_value schedule candidate key.
Proof.
  intros. unfold case_obligation_demand, case_obligation_amounts.
  apply nth_error_nth. rewrite nth_error_map, H. reflexivity.
Qed.

Lemma sum_zero_mapped : forall (A : Type) (items : list A),
  fold_right Nat.add 0 (map (fun _ => 0) items) = 0.
Proof. intros A items. induction items; simpl; assumption || reflexivity. Qed.

Lemma sum_priced_resource_occurrences : forall schedule resources,
  fold_right Nat.add 0
    (map (fun resource => resource_phlo schedule resource * phlo_actual_price schedule) resources) =
  execution_phlo schedule resources * phlo_actual_price schedule.
Proof.
  intros schedule resources. induction resources; unfold execution_phlo, acquisition_value in *; simpl in *; nia.
Qed.

Theorem projected_obligations_sum_to_retained_charge : forall schedule candidate,
  fold_right Nat.add 0 (case_obligation_amounts schedule candidate) = case_charge schedule candidate.
Proof.
  intros schedule candidate.
  unfold case_obligation_amounts, case_obligation_keys, phlo_obligation_value,
    case_is_billable, case_charge, retained_phlo_charge.
  destruct (case_outcome candidate) as [|failures]; simpl.
  - rewrite sum_zero_mapped. reflexivity.
  - destruct (forallb billable_failure failures); simpl.
    + rewrite map_map. change (1 + fold_right Nat.add 0
        (map (fun resource => resource_phlo schedule resource * phlo_actual_price schedule) (case_fresh candidate)) =
        newly_required_charge schedule (case_fresh candidate)).
      rewrite sum_priced_resource_occurrences. unfold newly_required_charge. lia.
    + rewrite sum_zero_mapped. reflexivity.
Qed.

Record phlo_funding_domain := {
  funding_sources : nat;
  funding_obligations : nat;
  funding_custody : nat -> nat;
  funding_capacity : nat -> nat;
  signed_source_exposure : nat -> nat;
  signed_source_debit : nat -> nat;
  signed_total_exposure : nat;
  funding_permission : nat -> nat -> phlo_obligation_key -> bool
}.

Definition case_funding_eligibility domain cases branch source slot :=
  match nth_error (case_obligation_keys (cases branch)) slot with
  | None => false
  | Some key => funding_permission domain branch source key
  end.

Definition funding_custody_ids domain :=
  map (funding_custody domain) (seq 0 (funding_sources domain)).

Theorem equal_obligation_keys_have_equal_permission : forall domain cases branch source
  first_slot second_slot key,
  nth_error (case_obligation_keys (cases branch)) first_slot = Some key ->
  nth_error (case_obligation_keys (cases branch)) second_slot = Some key ->
  case_funding_eligibility domain cases branch source first_slot =
  case_funding_eligibility domain cases branch source second_slot.
Proof.
  intros domain cases branch source first_slot second_slot key first_key second_key.
  unfold case_funding_eligibility. now rewrite first_key, second_key.
Qed.

Print Assumptions equal_obligation_keys_have_equal_permission.

Definition check_custody_unique (ids : list nat) : bool :=
  if list_eq_dec Nat.eq_dec ids (nodup Nat.eq_dec ids) then true else false.

Theorem custody_unique_check_exact : forall ids,
  check_custody_unique ids = true <-> NoDup ids.
Proof.
  intros ids. unfold check_custody_unique.
  destruct (list_eq_dec Nat.eq_dec ids (nodup Nat.eq_dec ids)) as [same|different].
  - split; [intros _; rewrite same; apply NoDup_nodup|reflexivity].
  - split; [discriminate|]. intro unique. exfalso. apply different.
    symmetry. now apply nodup_fixed_point.
Qed.

Definition phlo_source_holds domain branches plans :=
  branch_reservation branches (funding_obligations domain) plans.

Definition phlo_settlement_capacity domain branches plans source :=
  Nat.min (phlo_source_holds domain branches plans source) (signed_source_debit domain source).

Definition check_phlo_source_holds domain branches plans : bool :=
  forallb (fun source =>
    (phlo_source_holds domain branches plans source <=? funding_capacity domain source) &&
    (phlo_source_holds domain branches plans source <=? signed_source_exposure domain source))
    (seq 0 (funding_sources domain)) &&
  (funding_sum (funding_sources domain) (phlo_source_holds domain branches plans)
    <=? signed_total_exposure domain).

Definition check_phlo_branch environment machine_max terms schedule bound domain branches
  cases demands plans branch : bool :=
  check_case_controls environment machine_max terms schedule bound (cases branch) &&
  check_obligation_projection schedule (cases branch) (funding_obligations domain) (demands branch) &&
  assignment_check (funding_sources domain) (funding_obligations domain)
    (case_funding_eligibility domain cases branch) (phlo_settlement_capacity domain branches plans)
    (demands branch) (plans branch) &&
  (funding_sum (funding_obligations domain) (demands branch) =?
    case_charge schedule (cases branch)).

Definition check_phlo_family environment machine_max terms schedule bound domain branches
  cases demands plans : bool :=
  if check_custody_unique (funding_custody_ids domain) then
    match branches with
    | [] => false
    | _ :: _ => check_phlo_source_holds domain branches plans &&
        forallb (check_phlo_branch environment machine_max terms schedule bound domain branches
          cases demands plans) branches
    end
  else false.

Theorem accepted_phlo_family_exact : forall environment machine_max terms schedule bound domain
  branches cases demands plans,
  check_phlo_family environment machine_max terms schedule bound domain branches cases demands plans = true <->
  NoDup (funding_custody_ids domain) /\ branches <> [] /\
  check_phlo_source_holds domain branches plans = true /\
  forall branch, In branch branches ->
    check_phlo_branch environment machine_max terms schedule bound domain branches cases demands plans branch = true.
Proof.
  intros. unfold check_phlo_family.
  destruct (check_custody_unique (funding_custody_ids domain)) eqn:uniqueness.
  - apply custody_unique_check_exact in uniqueness.
    destruct branches as [|head rest]; [split; [discriminate|tauto]|].
    rewrite andb_true_iff, forallb_forall.
    assert (head :: rest <> []) by discriminate. tauto.
  - split; [discriminate|]. intros [unique _].
    apply custody_unique_check_exact in unique. congruence.
Qed.

Theorem accepted_branch_assignment_and_charge : forall environment machine_max terms schedule bound
  domain branches cases demands plans branch,
  check_phlo_branch environment machine_max terms schedule bound domain branches cases demands plans branch = true ->
  check_case_controls environment machine_max terms schedule bound (cases branch) = true /\
  assignment_valid (funding_sources domain) (funding_obligations domain)
    (case_funding_eligibility domain cases branch) (phlo_settlement_capacity domain branches plans)
    (demands branch) (plans branch) /\
  funding_sum (funding_sources domain) (source_draw (funding_obligations domain) (plans branch)) =
    case_charge schedule (cases branch).
Proof.
  intros environment machine_max terms schedule bound domain branches cases demands plans branch checked.
  unfold check_phlo_branch in checked. rewrite !andb_true_iff, Nat.eqb_eq in checked.
  destruct checked as [[[controls projection] assignment] charge]. apply assignment_check_exact in assignment.
  split; [exact controls|]. split; [exact assignment|].
  rewrite (accepted_assignment_conserves_obligation _ _ _ _ _ _ assignment). exact charge.
Qed.

Theorem accepted_branch_binds_each_obligation : forall environment machine_max terms schedule bound
  domain branches cases demands plans branch slot key,
  check_phlo_branch environment machine_max terms schedule bound domain branches cases demands plans branch = true ->
  nth_error (case_obligation_keys (cases branch)) slot = Some key ->
  demands branch slot = phlo_obligation_value schedule (cases branch) key.
Proof.
  intros environment machine_max terms schedule bound domain branches cases demands plans branch slot key checked found.
  unfold check_phlo_branch in checked. rewrite !andb_true_iff in checked.
  destruct checked as [[[controls projection] assignment] total].
  apply obligation_projection_check_exact in projection. destruct projection as [size entries].
  assert (inside : slot < length (case_obligation_keys (cases branch))).
  { apply nth_error_Some. rewrite found. discriminate. }
  rewrite entries by lia. now apply obligation_value_matches_its_key.
Qed.

Theorem accepted_flow_requires_permission_for_exact_resource : forall environment machine_max terms schedule bound
  domain branches cases demands plans branch source slot key,
  check_phlo_branch environment machine_max terms schedule bound domain branches cases demands plans branch = true ->
  source < funding_sources domain -> slot < funding_obligations domain ->
  nth_error (case_obligation_keys (cases branch)) slot = Some key ->
  funding_permission domain branch source key = false -> plans branch source slot = 0.
Proof.
  intros environment machine_max terms schedule bound domain branches cases demands plans branch source slot key checked source_bound slot_bound found denied.
  apply accepted_branch_assignment_and_charge in checked. destruct checked as [_ [[sources _] _]].
  specialize (sources source source_bound). destruct sources as [_ edges].
  apply edges; [exact slot_bound|]. unfold case_funding_eligibility. now rewrite found.
Qed.

Theorem accepted_flow_cannot_use_an_unknown_obligation : forall environment machine_max terms schedule bound
  domain branches cases demands plans branch source slot,
  check_phlo_branch environment machine_max terms schedule bound domain branches cases demands plans branch = true ->
  source < funding_sources domain -> slot < funding_obligations domain ->
  nth_error (case_obligation_keys (cases branch)) slot = None -> plans branch source slot = 0.
Proof.
  intros environment machine_max terms schedule bound domain branches cases demands plans branch source slot checked source_bound slot_bound absent.
  apply accepted_branch_assignment_and_charge in checked. destruct checked as [_ [[sources _] _]].
  specialize (sources source source_bound). destruct sources as [_ edges].
  apply edges; [exact slot_bound|]. unfold case_funding_eligibility. now rewrite absent.
Qed.

Theorem accepted_family_bounds_each_hold : forall environment machine_max terms schedule bound domain
  branches cases demands plans source,
  check_phlo_family environment machine_max terms schedule bound domain branches cases demands plans = true ->
  source < funding_sources domain ->
  phlo_source_holds domain branches plans source <= funding_capacity domain source /\
  phlo_source_holds domain branches plans source <= signed_source_exposure domain source.
Proof.
  intros environment machine_max terms schedule bound domain branches cases demands plans source checked inside.
  apply accepted_phlo_family_exact in checked. destruct checked as [_ [_ [holds _]]].
  unfold check_phlo_source_holds in holds. rewrite andb_true_iff in holds.
  destruct holds as [rows _]. apply funding_forallb_range with (index := source) in rows; [|exact inside].
  now rewrite andb_true_iff, !Nat.leb_le in rows.
Qed.

Theorem accepted_family_bounds_total_exposure : forall environment machine_max terms schedule bound domain
  branches cases demands plans,
  check_phlo_family environment machine_max terms schedule bound domain branches cases demands plans = true ->
  funding_sum (funding_sources domain) (phlo_source_holds domain branches plans) <= signed_total_exposure domain.
Proof.
  intros. apply accepted_phlo_family_exact in H. destruct H as [_ [_ [holds _]]].
  unfold check_phlo_source_holds in holds. apply andb_true_iff in holds.
  destruct holds as [_ total]. now apply Nat.leb_le in total.
Qed.

Theorem accepted_family_bounds_each_debit : forall environment machine_max terms schedule bound domain
  branches cases demands plans branch source,
  check_phlo_family environment machine_max terms schedule bound domain branches cases demands plans = true ->
  In branch branches -> source < funding_sources domain ->
  source_draw (funding_obligations domain) (plans branch) source <= phlo_source_holds domain branches plans source /\
  source_draw (funding_obligations domain) (plans branch) source <= signed_source_debit domain source.
Proof.
  intros environment machine_max terms schedule bound domain branches cases demands plans branch source checked included inside.
  apply accepted_phlo_family_exact in checked. destruct checked as [_ [_ [_ family]]].
  specialize (family branch included). apply accepted_branch_assignment_and_charge in family.
  destruct family as [_ [[rows _] _]]. specialize (rows source inside).
  unfold source_valid, phlo_settlement_capacity in rows. destruct rows as [caps _].
  now apply Nat.min_glb_iff in caps.
Qed.

Theorem accepted_family_bounds_each_hold_by_debit_cap : forall environment machine_max terms schedule bound domain
  branches cases demands plans source,
  check_phlo_family environment machine_max terms schedule bound domain branches cases demands plans = true ->
  source < funding_sources domain ->
  phlo_source_holds domain branches plans source <= signed_source_debit domain source.
Proof.
  intros environment machine_max terms schedule bound domain branches cases demands plans source checked inside.
  unfold phlo_source_holds. apply branch_reservation_within_capacity.
  intros branch included.
  exact (proj2 (accepted_family_bounds_each_debit environment machine_max terms schedule bound domain
    branches cases demands plans branch source checked included inside)).
Qed.

Print Assumptions accepted_family_bounds_each_hold_by_debit_cap.

Theorem accepted_family_total_debit_respects_phlo : forall environment machine_max terms schedule bound domain
  branches cases demands plans branch,
  check_phlo_family environment machine_max terms schedule bound domain branches cases demands plans = true ->
  In branch branches ->
  funding_sum (funding_sources domain) (source_draw (funding_obligations domain) (plans branch)) <=
    signed_charge_ceiling terms /\
  funding_sum (funding_sources domain) (source_draw (funding_obligations domain) (plans branch)) <= machine_max.
Proof.
  intros environment machine_max terms schedule bound domain branches cases demands plans branch checked included.
  apply accepted_phlo_family_exact in checked. destruct checked as [_ [_ [_ family]]].
  specialize (family branch included). apply accepted_branch_assignment_and_charge in family.
  destruct family as [controls [_ charge]]. rewrite charge.
  unfold case_charge, check_case_controls in *.
  eapply checked_retained_charge_bounded. exact controls.
Qed.

Definition captured_phlo_refunds domain branches plans branch : list (nat * nat) :=
  map (fun source => (funding_custody domain source,
    selected_branch_refund branches (funding_obligations domain) plans branch source))
    (seq 0 (funding_sources domain)).

Theorem refund_destinations_are_original_custody : forall domain branches plans branch,
  map fst (captured_phlo_refunds domain branches plans branch) = funding_custody_ids domain.
Proof. intros. unfold captured_phlo_refunds, funding_custody_ids. rewrite map_map. reflexivity. Qed.

Theorem checked_family_refunds_do_not_alias : forall environment machine_max terms schedule bound domain
  branches cases demands plans branch,
  check_phlo_family environment machine_max terms schedule bound domain branches cases demands plans = true ->
  NoDup (map fst (captured_phlo_refunds domain branches plans branch)).
Proof.
  intros. rewrite refund_destinations_are_original_custody.
  apply accepted_phlo_family_exact in H. tauto.
Qed.

Theorem selected_phlo_case_conserves_each_hold : forall domain branches plans branch source,
  In branch branches ->
  source_draw (funding_obligations domain) (plans branch) source +
    selected_branch_refund branches (funding_obligations domain) plans branch source =
  phlo_source_holds domain branches plans source.
Proof. intros. now apply branch_settlement_conserves_each_source. Qed.

Theorem selected_phlo_case_conserves_total_hold : forall domain branches plans branch,
  In branch branches ->
  funding_sum (funding_sources domain) (source_draw (funding_obligations domain) (plans branch)) +
    funding_sum (funding_sources domain)
      (selected_branch_refund branches (funding_obligations domain) plans branch) =
  funding_sum (funding_sources domain) (phlo_source_holds domain branches plans).
Proof. intros. now apply branch_settlement_conserves_total. Qed.

Theorem covered_realized_case_is_checked : forall environment machine_max terms schedule bound domain
  branches cases demands plans covered_inputs branch,
  check_phlo_family environment machine_max terms schedule bound domain branches cases demands plans = true ->
  (forall realized, covered_inputs realized -> In realized branches) ->
  covered_inputs branch ->
  check_phlo_branch environment machine_max terms schedule bound domain branches cases demands plans branch = true.
Proof.
  intros environment machine_max terms schedule bound domain branches cases demands plans covered_inputs branch checked cover realized.
  apply accepted_phlo_family_exact in checked. destruct checked as [_ [_ [_ family]]].
  apply family. now apply cover.
Qed.

Lemma funding_component_bounded_by_total : forall count amount source,
  source < count -> amount source <= funding_sum count amount.
Proof.
  induction count; intros amount source inside; [lia|]. simpl.
  destruct (Nat.eq_dec source count); [subst; lia|].
  assert (source < count) by lia. specialize (IHcount amount source H). lia.
Qed.

Record phlo_native_amounts := {
  native_hold : nat;
  native_acquisition : nat;
  native_fee : nat;
  native_refund : nat
}.

Definition lower_phlo_amounts maximum hold debit fee : option phlo_native_amounts :=
  if (hold <=? maximum) && (debit <=? hold) && (fee <=? debit) then
    Some {| native_hold := hold; native_acquisition := debit - fee;
            native_fee := fee; native_refund := hold - debit |}
  else None.

Theorem lower_phlo_amounts_acceptance_exact : forall maximum hold debit fee,
  (exists result, lower_phlo_amounts maximum hold debit fee = Some result) <->
  hold <= maximum /\ debit <= hold /\ fee <= debit.
Proof.
  intros. unfold lower_phlo_amounts.
  destruct ((hold <=? maximum) && (debit <=? hold) && (fee <=? debit)) eqn:checked.
  - rewrite !andb_true_iff, !Nat.leb_le in checked.
    split; [tauto|intros; eexists; reflexivity].
  - split; [intros [result impossible]; discriminate|].
    intros [hold_bound [debit_bound fee_bound]].
    assert ((hold <=? maximum) && (debit <=? hold) && (fee <=? debit) = true)
      by (rewrite !andb_true_iff, !Nat.leb_le; tauto).
    congruence.
Qed.

Theorem lowered_phlo_amounts_conserve_and_fit : forall maximum hold debit fee result,
  lower_phlo_amounts maximum hold debit fee = Some result ->
  native_hold result = hold /\ native_fee result = fee /\
  native_acquisition result + native_fee result = debit /\
  native_acquisition result + native_fee result + native_refund result = hold /\
  native_hold result <= maximum /\ native_acquisition result <= maximum /\
  native_fee result <= maximum /\ native_refund result <= maximum.
Proof.
  intros maximum hold debit fee result lowered.
  pose proof (proj1 (lower_phlo_amounts_acceptance_exact maximum hold debit fee)
    (ex_intro _ result lowered)) as bounds.
  unfold lower_phlo_amounts in lowered.
  destruct ((hold <=? maximum) && (debit <=? hold) && (fee <=? debit)); [|discriminate].
  inversion lowered; subst. simpl. destruct bounds as [hold_bound [debit_bound fee_bound]].
  repeat split; lia.
Qed.

Theorem checked_family_has_native_amounts : forall environment machine_max terms schedule bound domain
  branches cases demands plans branch source maximum,
  check_phlo_family environment machine_max terms schedule bound domain branches cases demands plans = true ->
  In branch branches -> source < funding_sources domain ->
  phlo_source_holds domain branches plans source <= maximum ->
  exists result, lower_phlo_amounts maximum (phlo_source_holds domain branches plans source)
    (source_draw (funding_obligations domain) (plans branch) source) (plans branch source 0) = Some result.
Proof.
  intros environment machine_max terms schedule bound domain branches cases demands plans branch source maximum
    checked included inside native_bound.
  apply lower_phlo_amounts_acceptance_exact. split; [exact native_bound|].
  split.
  - exact (proj1 (accepted_family_bounds_each_debit environment machine_max terms schedule bound domain
      branches cases demands plans branch source checked included inside)).
  - apply accepted_phlo_family_exact in checked.
    destruct checked as [_ [_ [_ valid]]]. specialize (valid branch included).
    unfold check_phlo_branch in valid. rewrite !andb_true_iff in valid.
    destruct valid as [[[_ projection] _] _].
    apply obligation_projection_check_exact in projection. destruct projection as [slots _].
    unfold case_obligation_keys in slots. simpl in slots.
    unfold source_draw. apply funding_component_bounded_by_total. lia.
Qed.

Print Assumptions lower_phlo_amounts_acceptance_exact.
Print Assumptions lowered_phlo_amounts_conserve_and_fit.
Print Assumptions checked_family_has_native_amounts.

Theorem zero_charge_releases_every_source_hold : forall environment machine_max terms schedule bound domain
  branches cases demands plans branch source,
  check_phlo_family environment machine_max terms schedule bound domain branches cases demands plans = true ->
  In branch branches -> source < funding_sources domain ->
  case_charge schedule (cases branch) = 0 ->
  source_draw (funding_obligations domain) (plans branch) source = 0 /\
  selected_branch_refund branches (funding_obligations domain) plans branch source =
    phlo_source_holds domain branches plans source.
Proof.
  intros environment machine_max terms schedule bound domain branches cases demands plans branch source checked included inside zero.
  apply accepted_phlo_family_exact in checked. destruct checked as [_ [_ [_ family]]].
  specialize (family branch included). apply accepted_branch_assignment_and_charge in family.
  destruct family as [_ [_ charge]].
  pose proof (funding_component_bounded_by_total _ (source_draw (funding_obligations domain) (plans branch)) _ inside).
  assert (empty : source_draw (funding_obligations domain) (plans branch) source = 0) by lia.
  split; [exact empty|]. unfold selected_branch_refund, phlo_source_holds. rewrite empty. lia.
Qed.

Definition example_phlo_case : phlo_execution_case :=
  {| case_available := []; case_required := []; case_used := []; case_unused := [];
     case_fresh := []; case_outcome := PhloAccepted [] |}.

Definition example_restricted_phlo_domain (sources exposure : nat) : phlo_funding_domain :=
  {| funding_sources := sources; funding_obligations := 1; funding_custody := fun source => source;
     funding_capacity := fun _ => 1; signed_source_exposure := fun _ => 1;
     signed_source_debit := fun _ => 1; signed_total_exposure := exposure;
     funding_permission := fun branch source _ => source =? branch |}.

Definition example_restricted_phlo_check sources exposure : bool :=
  check_phlo_family example_phlo_environment 31 (example_phlo_terms sources) (example_phlo_schedule 2) 0
    (example_restricted_phlo_domain sources exposure) (seq 0 sources)
    (fun _ => example_phlo_case) (fun _ _ => 1) branch_plan.

Example three_exclusive_purses_need_separate_exposure_consent :
  example_restricted_phlo_check 3 3 = true /\
  example_restricted_phlo_check 3 1 = false /\
  case_charge (example_phlo_schedule 2) example_phlo_case = 1.
Proof. vm_compute. repeat split; reflexivity. Qed.

Definition example_distinct_resource_case : phlo_execution_case :=
  let resources := [prepaid_sample 0 0 0 (SGround [true]); prepaid_sample 1 1 0 (SGround [false])] in
  {| case_available := []; case_required := resources; case_used := []; case_unused := [];
     case_fresh := resources; case_outcome := PhloAccepted [] |}.

Definition example_distinct_resource_domain : phlo_funding_domain :=
  {| funding_sources := 1; funding_obligations := 3; funding_custody := fun source => source;
     funding_capacity := fun _ => 7; signed_source_exposure := fun _ => 7;
     signed_source_debit := fun _ => 7; signed_total_exposure := 7;
     funding_permission := fun _ _ _ => true |}.

Definition example_displaced_resource_amounts (_ slot : nat) : nat :=
  if slot =? 0 then 7 else 0.

Example equal_total_cannot_move_resource_charge_to_the_fee :
  check_phlo_family example_phlo_environment 31 (example_phlo_terms 1) (example_phlo_schedule 2) 3
    example_distinct_resource_domain [0] (fun _ => example_distinct_resource_case)
    example_displaced_resource_amounts (fun branch _ slot => example_displaced_resource_amounts branch slot) = false.
Proof. vm_compute. reflexivity. Qed.

Definition example_resource_permission_domain permit_second : phlo_funding_domain :=
  {| funding_sources := 1; funding_obligations := 3; funding_custody := fun source => source;
     funding_capacity := fun _ => 7; signed_source_exposure := fun _ => 7;
     signed_source_debit := fun _ => 7; signed_total_exposure := 7;
     funding_permission := fun _ _ key =>
       match key with
       | PhloFeeObligation => true
       | PhloResourceObligation resource =>
           if prepaid_key_eq_dec resource (prepaid_sample 0 0 0 (SGround [true])) then true
           else if prepaid_key_eq_dec resource (prepaid_sample 1 1 0 (SGround [false])) then permit_second
           else false
       end |}.

Definition example_exact_resource_demands (_ : nat) :=
  case_obligation_demand (example_phlo_schedule 2) example_distinct_resource_case.

Definition example_resource_permission_check permission :=
  check_phlo_family example_phlo_environment 31 (example_phlo_terms 1) (example_phlo_schedule 2) 3
    (example_resource_permission_domain permission) [0] (fun _ => example_distinct_resource_case)
    example_exact_resource_demands (fun branch _ slot => example_exact_resource_demands branch slot).

Example resource_permission_is_required_independently_of_fee_permission :
  case_obligation_amounts (example_phlo_schedule 2) example_distinct_resource_case = [1; 2; 4] /\
  example_resource_permission_check true = true /\
  example_resource_permission_check false = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example a_zero_value_resource_occurrence_cannot_be_omitted :
  let resources := [prepaid_sample 0 0 0 SUnit] in
  let candidate := {| case_available := []; case_required := resources; case_used := []; case_unused := [];
                      case_fresh := resources; case_outcome := PhloAccepted [] |} in
  let demand := case_obligation_demand (example_phlo_schedule 2) candidate in
  case_obligation_amounts (example_phlo_schedule 2) candidate = [1; 0] /\
  check_obligation_projection (example_phlo_schedule 2) candidate 1 demand = false /\
  check_obligation_projection (example_phlo_schedule 2) candidate 2 demand = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example unused_obligation_slots_must_have_zero_demand :
  let demand := case_obligation_demand (example_phlo_schedule 2) example_distinct_resource_case in
  check_obligation_projection (example_phlo_schedule 2) example_distinct_resource_case 4 demand = true /\
  check_obligation_projection (example_phlo_schedule 2) example_distinct_resource_case 4
    (fun slot => if slot =? 3 then 1 else demand slot) = false.
Proof. vm_compute. split; reflexivity. Qed.

Example generated_restricted_cohorts_and_exposure :
  forallb (fun sources => example_restricted_phlo_check sources sources &&
    negb (example_restricted_phlo_check sources (sources - 1))) [1; 2; 3; 4; 8; 16; 65] = true.
Proof. vm_compute. reflexivity. Qed.

Definition example_phlo_domain_with_caps custody capacity exposure debit : phlo_funding_domain :=
  {| funding_sources := 3; funding_obligations := 1; funding_custody := custody;
     funding_capacity := capacity; signed_source_exposure := exposure;
     signed_source_debit := debit; signed_total_exposure := 3;
     funding_permission := fun branch source _ => source =? branch |}.

Definition example_phlo_check_domain domain plans : bool :=
  check_phlo_family example_phlo_environment 31 (example_phlo_terms 3) (example_phlo_schedule 2) 0
    domain [0; 1; 2] (fun _ => example_phlo_case) (fun _ _ => 1) plans.

Example duplicate_custody_does_not_multiply_backing :
  example_phlo_check_domain
    (example_phlo_domain_with_caps (fun _ => 0) (fun _ => 1) (fun _ => 1) (fun _ => 1)) branch_plan = false.
Proof. vm_compute. reflexivity. Qed.

Example every_source_bound_is_required :
  let identity := fun source : nat => source in
  let full := fun _ : nat => 1 in
  let missing := fun source => if source =? 2 then 0 else 1 in
  example_phlo_check_domain (example_phlo_domain_with_caps identity missing full full) branch_plan = false /\
  example_phlo_check_domain (example_phlo_domain_with_caps identity full missing full) branch_plan = false /\
  example_phlo_check_domain (example_phlo_domain_with_caps identity full full missing) branch_plan = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example balanced_but_ineligible_sources_cannot_pay :
  example_phlo_check_domain (example_restricted_phlo_domain 3 3)
    (fun _ source obligation => if (source =? 0) && (obligation =? 0) then 1 else 0) = false.
Proof. vm_compute. reflexivity. Qed.

Example supplied_obligation_cannot_replace_the_metered_charge :
  check_phlo_family example_phlo_environment 31 (example_phlo_terms 3) (example_phlo_schedule 2) 0
    (example_restricted_phlo_domain 3 3) [0; 1; 2] (fun _ => example_phlo_case)
    (fun _ _ => 0) (fun _ _ _ => 0) = false.
Proof. vm_compute. reflexivity. Qed.

Definition example_partially_prepaid_case branch : phlo_execution_case :=
  let resource := prepaid_sample 0 0 0 (SGround [true]) in
  {| case_available := [resource]; case_required := repeat resource 3;
     case_used := [resource]; case_unused := []; case_fresh := repeat resource 2;
     case_outcome := if branch =? 0 then PhloAccepted []
                     else PhloAccepted [PhloUserFailure; PhloPlatformFailure] |}.

Definition example_resource_and_fee_domain : phlo_funding_domain :=
  {| funding_sources := 2; funding_obligations := 3; funding_custody := fun source => source;
     funding_capacity := fun _ => 3; signed_source_exposure := fun _ => 3;
     signed_source_debit := fun _ => 3; signed_total_exposure := 5;
     funding_permission := fun _ _ _ => true |}.

Definition example_resource_and_fee_demands branch obligation :=
  if branch =? 0 then (if obligation =? 0 then 1 else 2) else 0.

Definition example_resource_and_fee_plans branch source obligation :=
  if branch =? 0 then
    if obligation =? 0 then (if source =? 0 then 1 else 0)
    else if obligation =? S source then 2 else 0
  else 0.

Example resource_charges_compose_with_prepaid_credit_and_failure_release :
  check_phlo_family example_phlo_environment 31 (example_phlo_terms 2) (example_phlo_schedule 2) 3
    example_resource_and_fee_domain [0; 1] example_partially_prepaid_case
    example_resource_and_fee_demands example_resource_and_fee_plans = true /\
  case_charge (example_phlo_schedule 2) (example_partially_prepaid_case 0) = 5 /\
  case_charge (example_phlo_schedule 2) (example_partially_prepaid_case 1) = 0 /\
  captured_phlo_refunds example_resource_and_fee_domain [0; 1] example_resource_and_fee_plans 0 =
    [(0, 0); (1, 0)] /\
  captured_phlo_refunds example_resource_and_fee_domain [0; 1] example_resource_and_fee_plans 1 =
    [(0, 3); (1, 2)].
Proof. vm_compute. repeat split; reflexivity. Qed.
