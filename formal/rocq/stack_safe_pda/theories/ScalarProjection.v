From Stdlib Require Import List Bool Arith Lia.

Import ListNotations.
Set Implicit Arguments.

Module ScalarProjection.

Section Endpoints.

Context {Payload : Type}.

Record endpoint := Endpoint {
  endpoint_payload : Payload;
  endpoint_multiplicity : nat
}.

Variable in_family : Payload -> bool.

Definition selected (entry : endpoint) := in_family (endpoint_payload entry).
Definition family_entries (entries : list endpoint) := filter selected entries.
Definition family_pure (entries : list endpoint) :=
  Nat.eqb (length entries) (length (family_entries entries)).
Definition positive_multiplicities (entries : list endpoint) :=
  Forall (fun entry => 0 < endpoint_multiplicity entry) entries.

Fixpoint exact_mass (entries : list endpoint) : nat :=
  match entries with
  | [] => 0
  | entry :: rest => endpoint_multiplicity entry + exact_mass rest
  end.

Definition projected_family_mass cap entries := Nat.min cap (exact_mass (family_entries entries)).

Lemma all_selected_entries_are_the_original_list : forall entries,
  Forall (fun entry => selected entry = true) entries -> family_entries entries = entries.
Proof.
  intros entries all. induction all; [reflexivity|].
  unfold family_entries in *. simpl. now rewrite H, IHall.
Qed.

Theorem exact_distinct_family_purity : forall entries,
  family_pure entries = true <-> Forall (fun entry => selected entry = true) entries.
Proof.
  intro entries. unfold family_pure. rewrite Nat.eqb_eq. split.
  - intro equal. apply Forall_forall. intros entry present.
    symmetry in equal. apply filter_length_forallb in equal.
    now apply (proj1 (forallb_forall selected entries) equal).
  - intro all. now rewrite (all_selected_entries_are_the_original_list all).
Qed.

Theorem saturated_singleton_count_is_exact : forall cap mass,
  1 < cap -> (Nat.min cap mass = 1 <-> mass = 1).
Proof.
  intros cap mass cap_large. split; intro count.
  - destruct (Nat.le_ge_cases cap mass).
    + rewrite Nat.min_l in count by assumption. lia.
    + now rewrite Nat.min_r in count by assumption.
  - subst. apply Nat.min_r. lia.
Qed.

Lemma positive_zero_mass_is_empty : forall entries,
  positive_multiplicities entries -> exact_mass entries = 0 -> entries = [].
Proof.
  intros entries positive zero. destruct entries; [reflexivity|].
  inversion positive; subst. simpl in zero. lia.
Qed.

Lemma positive_unit_mass_is_one_endpoint : forall entries,
  positive_multiplicities entries -> exact_mass entries = 1 ->
  exists payload, entries = [Endpoint payload 1].
Proof.
  intros entries positive one. destruct entries as [|[payload count] rest]; [discriminate|].
  inversion positive as [|entry tail head_positive tail_positive]; subst.
  simpl in *. assert (count = 1) by lia. subst count.
  assert (zero : exact_mass rest = 0) by lia.
  pose proof (positive_zero_mass_is_empty tail_positive zero) as empty. subst rest.
  exists payload; reflexivity.
Qed.

Theorem pure_unit_mass_is_exactly_one_selected_endpoint : forall cap entries,
  1 < cap -> positive_multiplicities entries ->
  (family_pure entries = true /\ projected_family_mass cap entries = 1 <->
   exists payload, entries = [Endpoint payload 1] /\ in_family payload = true).
Proof.
  intros cap entries large positive. split.
  - intros [pure count]. apply exact_distinct_family_purity in pure.
    unfold projected_family_mass in count.
    rewrite (all_selected_entries_are_the_original_list pure) in count.
    apply saturated_singleton_count_is_exact in count; [|assumption].
    destruct (positive_unit_mass_is_one_endpoint positive count) as [payload same].
    exists payload; split; [assumption|]. subst entries. inversion pure; assumption.
  - intros [payload [same belongs]]. subst entries.
    unfold family_pure, projected_family_mass, family_entries, selected. simpl.
    rewrite belongs. simpl. split; [reflexivity|].
    apply Nat.min_r; lia.
Qed.

Theorem saturated_cardinality_equality_does_not_prove_purity : forall cap matching foreign,
  1 < cap -> in_family matching = true -> in_family foreign = false ->
  let entries := [Endpoint matching cap; Endpoint foreign 1] in
  Nat.min cap (exact_mass entries) = projected_family_mass cap entries /\
  family_pure entries = false.
Proof.
  intros cap matching foreign large yes no entries.
  unfold entries, projected_family_mass, family_pure, family_entries, selected.
  simpl. rewrite yes, no. simpl. split; [|reflexivity].
  rewrite Nat.add_0_r, Nat.min_id. apply Nat.min_l. lia.
Qed.

End Endpoints.

Section Dispatch.

Context {Payload Decoded Value Error State Event Plan : Type}.
Variable in_family : Payload -> bool.
Variable cap : nat.
Variable foreign_error arity_error : Error.
Variable decode : Payload -> Decoded.

Inductive evaluation_result := Success (value : Value) | Failure (error : Error).

Record observation := Observation {
  observed_result : evaluation_result;
  observed_state : State;
  observed_trace : list Event
}.

Record projection_run := ProjectionRun {
  run_observation : observation;
  decoded_payloads : list Payload;
  delegated_values : list Decoded
}.

Variable delegate : Decoded -> State -> observation.

Definition refuse error state := ProjectionRun (Observation (Failure error) state []) [] [].

Definition scalar_projection (entries : list (@endpoint Payload)) state : projection_run :=
  if family_pure in_family entries then
    if Nat.eqb (projected_family_mass in_family cap entries) 1 then
      match entries with
      | entry :: _ =>
          let decoded := decode (endpoint_payload entry) in
          ProjectionRun (delegate decoded state) [endpoint_payload entry] [decoded]
      | [] => refuse arity_error state
      end
    else refuse arity_error state
  else refuse foreign_error state.

Theorem foreign_refusal_precedes_arity_without_effects : forall entries state,
  family_pure in_family entries = false ->
  scalar_projection entries state =
    ProjectionRun (Observation (Failure foreign_error) state []) [] [].
Proof. intros entries state foreign. unfold scalar_projection. now rewrite foreign. Qed.

Theorem arity_refusal_has_no_decode_or_child_effects : forall entries state,
  family_pure in_family entries = true -> projected_family_mass in_family cap entries <> 1 ->
  scalar_projection entries state =
    ProjectionRun (Observation (Failure arity_error) state []) [] [].
Proof.
  intros entries state pure arity. unfold scalar_projection. rewrite pure.
  apply Nat.eqb_neq in arity. now rewrite arity.
Qed.

Theorem singleton_delegation_preserves_the_complete_observation : forall entries state,
  1 < cap -> positive_multiplicities entries ->
  family_pure in_family entries = true -> projected_family_mass in_family cap entries = 1 ->
  exists payload,
    entries = [Endpoint payload 1] /\ in_family payload = true /\
    scalar_projection entries state =
      ProjectionRun (delegate (decode payload) state) [payload] [decode payload].
Proof.
  intros entries state large positive pure count.
  destruct (proj1 (pure_unit_mass_is_exactly_one_selected_endpoint in_family large positive)
    (conj pure count)) as [payload [same belongs]].
  exists payload. split; [assumption|]. split; [assumption|].
  unfold scalar_projection. rewrite pure, count, Nat.eqb_refl, same. reflexivity.
Qed.

Variable recognize : list (@endpoint Payload) -> option Plan.
Variable execute : Plan -> State -> projection_run.

Definition fused_dispatch entries state :=
  match recognize entries with
  | None => scalar_projection entries state
  | Some plan => execute plan state
  end.

Theorem fusion_decline_preserves_fallback_and_accepted_execution :
  (forall entries state, recognize entries = None ->
    fused_dispatch entries state = scalar_projection entries state) /\
  (forall entries state plan, recognize entries = Some plan ->
    fused_dispatch entries state = execute plan state).
Proof. split; intros; unfold fused_dispatch; now rewrite H. Qed.

End Dispatch.

Definition fixture_family := Nat.eqb 7.
Definition fixture_delegate value state : @observation nat nat nat nat :=
  Observation (Success value) (S state) [3; 4].
Definition fixture_project :=
  @scalar_projection nat nat nat nat nat nat fixture_family 9 10 20 (fun value => value)
    fixture_delegate.

Example scalar_projection_has_a_nonvacuous_accepted_fixture :
  fixture_project [Endpoint 7 1] 30 = ProjectionRun
    (Observation (Success 7) 31 [3; 4]) [7] [7].
Proof. reflexivity. Qed.

Definition mutant_arity_first entries state :=
  if Nat.eqb (projected_family_mass fixture_family 9 entries) 1
  then fixture_project entries state
  else @refuse nat nat nat nat nat nat 20 state.

Definition mutant_eager_refusal state : @projection_run nat nat nat nat nat nat :=
  ProjectionRun (Observation (Failure 10) state []) [7] [7].

Example arity_first_changes_the_foreign_error :
  mutant_arity_first [Endpoint 7 9; Endpoint 8 1] 30 <>
    fixture_project [Endpoint 7 9; Endpoint 8 1] 30.
Proof. discriminate. Qed.

Example empty_is_pure_but_not_scalar :
  family_pure fixture_family [] = true /\
  fixture_project [] 30 = ProjectionRun (Observation (Failure 20) 30 []) [] [].
Proof. split; reflexivity. Qed.

Example one_distinct_endpoint_with_repetition_is_not_scalar :
  length [Endpoint 7 2] = 1 /\
  fixture_project [Endpoint 7 2] 30 =
    ProjectionRun (Observation (Failure 20) 30 []) [] [].
Proof. split; reflexivity. Qed.

Example early_decode_and_child_calls_change_refusal_observations :
  mutant_eager_refusal 30 <> fixture_project [Endpoint 7 1; Endpoint 8 1] 30.
Proof. discriminate. Qed.

Example singleton_child_failure_keeps_its_state_and_ordered_trace :
  @scalar_projection nat nat nat nat nat nat fixture_family 9 10 20 (fun value => value)
    (fun _ state => Observation (Failure 99) (S state) [4; 3]) [Endpoint 7 1] 30 =
  ProjectionRun (Observation (Failure 99) 31 [4; 3]) [7] [7].
Proof. reflexivity. Qed.

Example declined_fusion_cannot_turn_a_fallback_error_into_success :
  @fused_dispatch nat nat nat nat nat nat unit fixture_family 9 10 20 (fun value => value)
    fixture_delegate (fun _ => None)
    (fun _ state => ProjectionRun (Observation (Success 0) state []) [] [])
    [Endpoint 7 1; Endpoint 8 1] 30 <>
  ProjectionRun (Observation (Success 0) 30 []) [] [].
Proof. discriminate. Qed.

Theorem scalar_omission_controls_are_nonvacuous :
  mutant_arity_first [Endpoint 7 9; Endpoint 8 1] 30 <>
    fixture_project [Endpoint 7 9; Endpoint 8 1] 30 /\
  family_pure fixture_family [] = true /\
  fixture_project [] 30 = ProjectionRun (Observation (Failure 20) 30 []) [] [] /\
  fixture_project [Endpoint 7 2] 30 = ProjectionRun (Observation (Failure 20) 30 []) [] [] /\
  mutant_eager_refusal 30 <> fixture_project [Endpoint 7 1; Endpoint 8 1] 30.
Proof.
  split; [apply arity_first_changes_the_foreign_error|].
  repeat split; try reflexivity. apply early_decode_and_child_calls_change_refusal_observations.
Qed.

Print Assumptions exact_distinct_family_purity.
Print Assumptions saturated_singleton_count_is_exact.
Print Assumptions pure_unit_mass_is_exactly_one_selected_endpoint.
Print Assumptions saturated_cardinality_equality_does_not_prove_purity.
Print Assumptions foreign_refusal_precedes_arity_without_effects.
Print Assumptions arity_refusal_has_no_decode_or_child_effects.
Print Assumptions singleton_delegation_preserves_the_complete_observation.
Print Assumptions fusion_decline_preserves_fallback_and_accepted_execution.
Print Assumptions scalar_projection_has_a_nonvacuous_accepted_fixture.
Print Assumptions scalar_omission_controls_are_nonvacuous.

End ScalarProjection.
