From Stdlib Require Import Lists.List Arith.PeanoNat Bool.Bool.
From CostAccountedRho Require Import StateBoundCandidateFunding.
Import ListNotations.

Section WitnessReplay.
  Variables State Choice Observation : Type.
  Variable enabled : State -> Choice -> bool.
  Variable step : State -> Choice -> option State.
  Variable cut_is_complete : State -> bool.
  Variable observe : State -> Observation.

  Fixpoint replay (state : State) (choices : list Choice) : option State :=
    match choices with
    | [] => Some state
    | choice :: rest =>
        if cut_is_complete state then None else
        if enabled state choice then
          match step state choice with
          | Some next => replay next rest
          | None => None
          end
        else None
    end.

  Definition certified_selected_case
    (initial : State) (choices : list Choice) (result : Observation) : Prop :=
    exists final,
      replay initial choices = Some final /\
      cut_is_complete final = true /\
      observe final = result.

  Theorem fixed_complete_witness_has_one_observation :
    forall initial choices left right,
      certified_selected_case initial choices left ->
      certified_selected_case initial choices right ->
      left = right.
  Proof.
    intros initial choices left right [left_final [left_run [_ left_result]]]
      [right_final [right_run [_ right_result]]].
    rewrite left_run in right_run.
    injection right_run as same_final.
    subst right_final.
    now rewrite <- left_result, <- right_result.
  Qed.

  Theorem incomplete_witness_cannot_be_certified :
    forall initial choices final result,
      replay initial choices = Some final ->
      cut_is_complete final = false ->
      ~ certified_selected_case initial choices result.
  Proof.
    intros initial choices final result run incomplete [other [other_run [complete _]]].
    rewrite run in other_run.
    injection other_run as same_final.
    subst other.
    congruence.
  Qed.

  Theorem rejected_witness_has_no_selected_case :
    forall initial choices result,
      replay initial choices = None ->
      ~ certified_selected_case initial choices result.
  Proof.
    intros initial choices result rejected [final [accepted _]].
    congruence.
  Qed.

  Theorem any_operation_after_complete_cut_is_rejected :
    forall state choice rest,
      cut_is_complete state = true ->
      replay state (choice :: rest) = None.
  Proof.
    intros state choice rest complete.
    simpl. now rewrite complete.
  Qed.
End WitnessReplay.

Definition example_enabled (state : nat) (_ : bool) := Nat.ltb state 3.

Definition example_step (state : nat) (choice : bool) : option nat :=
  match state with
  | 0 => Some (if choice then 2 else 1)
  | 1 => Some 3
  | 2 => Some 4
  | _ => None
  end.

Definition example_cut (state : nat) :=
  Nat.eqb state 3 || Nat.eqb state 4.

Example one_prefix_is_not_a_complete_witness :
  ~ certified_selected_case nat bool nat example_enabled example_step
      example_cut (fun state => state) 0 [false] 1.
Proof.
  eapply incomplete_witness_cannot_be_certified with (final := 1).
  - reflexivity.
  - reflexivity.
Qed.

Example distinct_complete_alternatives_can_both_be_valid :
  certified_selected_case nat bool nat example_enabled example_step
    example_cut (fun state => state) 0 [false; false] 3 /\
  certified_selected_case nat bool nat example_enabled example_step
    example_cut (fun state => state) 0 [true; false] 4 /\
  3 <> 4.
Proof.
  repeat split.
  - exists 3. repeat split; reflexivity.
  - exists 4. repeat split; reflexivity.
  - discriminate.
Qed.

Example operation_after_complete_cut_is_rejected :
  ~ certified_selected_case nat bool nat example_enabled example_step
      example_cut (fun state => state) 0 [false; false; false] 3.
Proof.
  eapply rejected_witness_has_no_selected_case.
  reflexivity.
Qed.

Record selected_witness_candidate (Choice : Type) := {
  selected_id : nat;
  selected_root : nat;
  selected_input : nat;
  selected_choices : list Choice;
  selected_observation : funded_observation
}.

Definition selected_charge {Choice} (candidate : selected_witness_candidate Choice) :=
  observed_resource_cost (selected_observation _ candidate) +
  observed_fee_cost (selected_observation _ candidate).

Section SelectedPublication.
  Variables State Choice : Type.
  Variable initial : nat -> nat -> State.
  Variable enabled : State -> Choice -> bool.
  Variable step : State -> Choice -> option State.
  Variable cut_is_complete : State -> bool.
  Variable observe : State -> funded_observation.
  Variable signed_authorized : nat -> nat -> list Choice -> funded_observation -> bool.

  Definition complete_exact_case (candidate : selected_witness_candidate Choice) : bool :=
    match replay State Choice enabled step cut_is_complete
      (initial (selected_root _ candidate) (selected_input _ candidate))
      (selected_choices _ candidate) with
    | Some final =>
        cut_is_complete final &&
        if observation_eq_dec (observe final) (selected_observation _ candidate)
        then true else false
    | None => false
    end.

  Definition selected_admitted
    (ledger : funded_state) (candidate : selected_witness_candidate Choice) : bool :=
    Nat.eqb (selected_root _ candidate) (state_root ledger) &&
    negb (existsb (Nat.eqb (selected_id _ candidate)) (state_published ledger)) &&
    signed_authorized (selected_root _ candidate) (selected_input _ candidate)
      (selected_choices _ candidate) (selected_observation _ candidate) &&
    Nat.leb (selected_charge candidate) (state_supply ledger) &&
    complete_exact_case candidate.

  Definition publish_selected
    (ledger : funded_state) (candidate : selected_witness_candidate Choice) : funded_state :=
    if selected_admitted ledger candidate then
      {| state_root := observed_post_root (selected_observation _ candidate);
         state_supply := state_supply ledger - selected_charge candidate;
         state_effects := state_effects ledger ++
           observed_effects (selected_observation _ candidate);
         state_charges := state_charges ledger ++ [selected_charge candidate];
         state_published := selected_id _ candidate :: state_published ledger |}
    else ledger.

  Theorem complete_exact_case_has_a_complete_enabled_replay :
    forall candidate,
      complete_exact_case candidate = true ->
      certified_selected_case State Choice funded_observation enabled step
        cut_is_complete observe
        (initial (selected_root _ candidate) (selected_input _ candidate))
        (selected_choices _ candidate) (selected_observation _ candidate).
  Proof.
    intros candidate complete.
    unfold complete_exact_case in complete.
    destruct (replay State Choice enabled step cut_is_complete
      (initial (selected_root _ candidate) (selected_input _ candidate))
      (selected_choices _ candidate)) as [final|] eqn:run; [|discriminate].
    apply andb_true_iff in complete.
    destruct complete as [cut equal].
    destruct (observation_eq_dec (observe final) (selected_observation _ candidate))
      as [same|different]; [|discriminate].
    exists final. repeat split; assumption.
  Qed.

  Theorem admitted_selected_witness_is_authorized_complete_and_funded :
    forall ledger candidate,
      selected_admitted ledger candidate = true ->
      selected_root _ candidate = state_root ledger /\
      signed_authorized (selected_root _ candidate) (selected_input _ candidate)
        (selected_choices _ candidate) (selected_observation _ candidate) = true /\
      selected_charge candidate <= state_supply ledger /\
      certified_selected_case State Choice funded_observation enabled step
        cut_is_complete observe
        (initial (selected_root _ candidate) (selected_input _ candidate))
        (selected_choices _ candidate) (selected_observation _ candidate) /\
      ~ In (selected_id _ candidate) (state_published ledger).
  Proof.
    intros ledger candidate admitted.
    unfold selected_admitted in admitted.
    repeat rewrite andb_true_iff in admitted.
    destruct admitted as [[[[root fresh] authorized] funded] complete].
    apply Nat.eqb_eq in root.
    apply Nat.leb_le in funded.
    apply negb_true_iff in fresh.
    repeat split; try assumption.
    - now apply complete_exact_case_has_a_complete_enabled_replay.
    - intro member.
      assert (existsb (Nat.eqb (selected_id _ candidate))
        (state_published ledger) = true) as found.
      { apply existsb_exists.
        exists (selected_id _ candidate).
        split; [exact member|apply Nat.eqb_refl]. }
      congruence.
  Qed.

  Theorem rejected_selected_witness_has_no_effect :
    forall ledger candidate,
      selected_admitted ledger candidate = false ->
      publish_selected ledger candidate = ledger.
  Proof.
    intros ledger candidate rejected.
    unfold publish_selected. now rewrite rejected.
  Qed.

  Theorem stale_selected_witness_has_no_effect :
    forall ledger candidate,
      selected_root _ candidate <> state_root ledger ->
      publish_selected ledger candidate = ledger.
  Proof.
    intros ledger candidate stale.
    apply rejected_selected_witness_has_no_effect.
    unfold selected_admitted.
    apply Nat.eqb_neq in stale.
    now rewrite stale.
  Qed.

  Theorem unauthorized_selected_witness_has_no_effect :
    forall ledger candidate,
      signed_authorized (selected_root _ candidate) (selected_input _ candidate)
        (selected_choices _ candidate) (selected_observation _ candidate) = false ->
      publish_selected ledger candidate = ledger.
  Proof.
    intros ledger candidate unauthorized.
    apply rejected_selected_witness_has_no_effect.
    unfold selected_admitted.
    now rewrite unauthorized, andb_false_r.
  Qed.

  Theorem incomplete_selected_witness_has_no_effect :
    forall ledger candidate,
      complete_exact_case candidate = false ->
      publish_selected ledger candidate = ledger.
  Proof.
    intros ledger candidate incomplete.
    apply rejected_selected_witness_has_no_effect.
    unfold selected_admitted.
    now rewrite incomplete, andb_false_r.
  Qed.
End SelectedPublication.

Record selected_private_resource_key := {
  private_resource_location : list nat;
  private_resource_class : nat;
  private_resource_acquisition_terms : list nat;
  private_resource_authority : list nat
}.

Section PrivatePurseSelectedWitness.
  Variables State Choice : Type.
  Variable initial : nat -> nat -> State.
  Variable enabled : State -> Choice -> bool.
  Variable step : State -> Choice -> option State.
  Variable cut_is_complete : State -> bool.
  Variable observe : State -> funded_observation.
  Variable signed_authorized : nat -> nat -> list Choice -> funded_observation -> bool.
  Variable private_purse_assignment :
    selected_witness_candidate Choice -> selected_private_resource_key -> Prop.
  Variable typed_name_event : State -> selected_private_resource_key -> Prop.

  Hypothesis private_assignment_capture_sound :
    forall candidate key final,
      private_purse_assignment candidate key ->
      replay State Choice enabled step cut_is_complete
        (initial (selected_root _ candidate) (selected_input _ candidate))
        (selected_choices _ candidate) = Some final ->
      typed_name_event final key.

  Theorem admitted_private_assignment_has_exact_typed_name_event :
    forall ledger candidate key,
      selected_admitted State Choice initial enabled step cut_is_complete
        observe signed_authorized ledger candidate = true ->
      private_purse_assignment candidate key ->
      exists final,
        replay State Choice enabled step cut_is_complete
          (initial (selected_root _ candidate) (selected_input _ candidate))
          (selected_choices _ candidate) = Some final /\
        cut_is_complete final = true /\
        observe final = selected_observation _ candidate /\
        typed_name_event final key.
  Proof.
    intros ledger candidate key admitted assigned.
    pose proof
      (admitted_selected_witness_is_authorized_complete_and_funded
        State Choice initial enabled step cut_is_complete observe
        signed_authorized ledger candidate admitted) as
      [_ [_ [_ [[final [run [complete observed]]] _]]]].
    exists final.
    repeat split; try assumption.
    eapply private_assignment_capture_sound; eassumption.
  Qed.

  Theorem forged_ground_alias_without_typed_name_is_rejected :
    forall ledger candidate key,
      private_purse_assignment candidate key ->
      (forall final,
        replay State Choice enabled step cut_is_complete
          (initial (selected_root _ candidate) (selected_input _ candidate))
          (selected_choices _ candidate) = Some final ->
        ~ typed_name_event final key) ->
      selected_admitted State Choice initial enabled step cut_is_complete
        observe signed_authorized ledger candidate = false.
  Proof.
    intros ledger candidate key assigned absent.
    destruct (selected_admitted State Choice initial enabled step
      cut_is_complete observe signed_authorized ledger candidate) eqn:admitted;
      [|reflexivity].
    destruct (admitted_private_assignment_has_exact_typed_name_event
      ledger candidate key admitted assigned) as [final [run [_ [_ name]]]].
    exfalso.
    apply (absent final run).
    exact name.
  Qed.
End PrivatePurseSelectedWitness.

Section GuidedReplayRefinement.
  Variables State Choice : Type.
  Variable initial : nat -> nat -> State.
  Variable enabled : State -> Choice -> bool.
  Variable step : State -> Choice -> option State.
  Variable cut_is_complete : State -> bool.
  Variable producer_run validator_run : nat -> nat -> list Choice -> State -> Prop.

  Hypothesis producer_follows_selected_witness : forall root input choices final,
    producer_run root input choices final ->
    replay State Choice enabled step cut_is_complete
      (initial root input) choices = Some final.
  Hypothesis validator_follows_selected_witness : forall root input choices final,
    validator_run root input choices final ->
    replay State Choice enabled step cut_is_complete
      (initial root input) choices = Some final.

  Theorem complete_guided_replay_agrees_on_final_state :
    forall root input choices produced validated,
      producer_run root input choices produced ->
      validator_run root input choices validated ->
      cut_is_complete produced = true ->
      cut_is_complete validated = true ->
      produced = validated.
  Proof.
    intros root input choices produced validated producer validator _ _.
    pose proof (producer_follows_selected_witness _ _ _ _ producer) as left.
    pose proof (validator_follows_selected_witness _ _ _ _ validator) as right.
    now rewrite left in right; injection right.
  Qed.
End GuidedReplayRefinement.

Print Assumptions fixed_complete_witness_has_one_observation.
Print Assumptions incomplete_witness_cannot_be_certified.
Print Assumptions rejected_witness_has_no_selected_case.
Print Assumptions any_operation_after_complete_cut_is_rejected.
Print Assumptions admitted_selected_witness_is_authorized_complete_and_funded.
Print Assumptions rejected_selected_witness_has_no_effect.
Print Assumptions admitted_private_assignment_has_exact_typed_name_event.
Print Assumptions forged_ground_alias_without_typed_name_is_rejected.
Print Assumptions complete_guided_replay_agrees_on_final_state.
