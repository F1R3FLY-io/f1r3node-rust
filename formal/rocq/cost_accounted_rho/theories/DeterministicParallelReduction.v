From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lists.List.
Import ListNotations.

Inductive reduction_channel := X | Y.

Inductive reduction_action :=
| Produce (value : nat)
| Consume.

Record reduction_intent := {
  intent_order : nat;
  intent_channel : reduction_channel;
  intent_action : reduction_action;
  intent_authority_regions : list nat
}.

Record reduction_state := {
  x_data : list nat;
  y_data : list nat;
  x_output : option nat;
  y_output : option nat
}.

Definition empty_reduction_state : reduction_state :=
  {| x_data := [];
     y_data := [];
     x_output := None;
     y_output := None |}.

Fixpoint insert_sorted (value : nat) (values : list nat) : list nat :=
  match values with
  | [] => [value]
  | head :: tail =>
      if Nat.leb value head
      then value :: values
      else head :: insert_sorted value tail
  end.

Definition reduce_x
  (action : reduction_action)
  (state : reduction_state) : reduction_state :=
  match action with
  | Produce value =>
      {| x_data := insert_sorted value state.(x_data);
         y_data := state.(y_data);
         x_output := state.(x_output);
         y_output := state.(y_output) |}
  | Consume =>
      match state.(x_data) with
      | [] => state
      | value :: tail =>
          {| x_data := tail;
             y_data := state.(y_data);
             x_output := Some value;
             y_output := state.(y_output) |}
      end
  end.

Definition reduce_y
  (action : reduction_action)
  (state : reduction_state) : reduction_state :=
  match action with
  | Produce value =>
      {| x_data := state.(x_data);
         y_data := insert_sorted value state.(y_data);
         x_output := state.(x_output);
         y_output := state.(y_output) |}
  | Consume =>
      match state.(y_data) with
      | [] => state
      | value :: tail =>
          {| x_data := state.(x_data);
             y_data := tail;
             x_output := state.(x_output);
             y_output := Some value |}
      end
  end.

Definition commit_intent
  (intent : reduction_intent)
  (state : reduction_state) : reduction_state :=
  match intent.(intent_channel) with
  | X => reduce_x intent.(intent_action) state
  | Y => reduce_y intent.(intent_action) state
  end.

Fixpoint commit_frontier
  (frontier : list reduction_intent)
  (state : reduction_state) : reduction_state :=
  match frontier with
  | [] => state
  | intent :: remaining =>
      commit_frontier remaining (commit_intent intent state)
  end.

Definition intents_conflict
  (left right : reduction_intent) : Prop :=
  left.(intent_channel) = right.(intent_channel) \/
  exists region,
    In region left.(intent_authority_regions) /\
    In region right.(intent_authority_regions).

Theorem intents_conflict_symmetric : forall left right,
  intents_conflict left right -> intents_conflict right left.
Proof.
  intros left right H.
  unfold intents_conflict in *.
  destruct H as [Hchannel | [region [Hleft Hright]]].
  - left. symmetry. exact Hchannel.
  - right. exists region. auto.
Qed.

Theorem disjoint_intents_commute : forall left right state,
  ~ intents_conflict left right ->
  commit_intent left (commit_intent right state) =
  commit_intent right (commit_intent left state).
Proof.
  intros [left_order left_channel left_action left_regions]
         [right_order right_channel right_action right_regions]
         [xs ys xo yo] Hdisjoint.
  unfold intents_conflict in Hdisjoint.
  simpl in Hdisjoint.
  assert (Hchannel : left_channel <> right_channel).
  { intro Hequal. apply Hdisjoint. left. exact Hequal. }
  destruct left_channel, right_channel;
    try (exfalso; apply Hchannel; reflexivity);
    destruct left_action, right_action;
    destruct xs, ys;
    reflexivity.
Qed.

Definition produce_one : reduction_intent :=
  {| intent_order := 1;
     intent_channel := X;
     intent_action := Produce 1;
     intent_authority_regions := [1; 2] |}.

Definition produce_two : reduction_intent :=
  {| intent_order := 2;
     intent_channel := X;
     intent_action := Produce 2;
     intent_authority_regions := [2; 3] |}.

Definition consume_x : reduction_intent :=
  {| intent_order := 3;
     intent_channel := X;
     intent_action := Consume;
     intent_authority_regions := [2] |}.

Definition produce_y : reduction_intent :=
  {| intent_order := 4;
     intent_channel := Y;
     intent_action := Produce 9;
     intent_authority_regions := [4] |}.

Definition shared_authority_y : reduction_intent :=
  {| intent_order := 4;
     intent_channel := Y;
     intent_action := Produce 9;
     intent_authority_regions := [2; 5] |}.

Theorem compound_authority_overlap_is_a_conflict :
  intents_conflict produce_one shared_authority_y.
Proof.
  right.
  exists 2.
  simpl.
  auto.
Qed.

Theorem compound_authority_order_does_not_hide_overlap :
  intents_conflict
    {| intent_order := 5;
       intent_channel := Y;
       intent_action := Produce 8;
       intent_authority_regions := [5; 2] |}
    produce_two.
Proof.
  right.
  exists 2.
  simpl.
  auto.
Qed.

Definition canonical_frontier : list reduction_intent :=
  [produce_one; produce_two; consume_x; produce_y].

Definition parallel_component_schedule : list reduction_intent :=
  [produce_one; produce_y; produce_two; consume_x].

Definition arrival_order_counterexample : list reduction_intent :=
  [produce_two; produce_y; consume_x; produce_one].

Theorem canonical_frontier_result :
  commit_frontier canonical_frontier empty_reduction_state =
  {| x_data := [2];
     y_data := [9];
     x_output := Some 1;
     y_output := None |}.
Proof. reflexivity. Qed.

Theorem disjoint_parallel_schedule_refines_canonical :
  commit_frontier parallel_component_schedule empty_reduction_state =
  commit_frontier canonical_frontier empty_reduction_state.
Proof. reflexivity. Qed.

Theorem arrival_order_commit_is_not_canonical :
  commit_frontier arrival_order_counterexample empty_reduction_state <>
  commit_frontier canonical_frontier empty_reduction_state.
Proof. discriminate. Qed.

Definition operation_segment := (nat * nat)%type.

Inductive persistent_operation_path :=
| PersistentPathRoot
| PersistentPathAppend
    (prefix : persistent_operation_path)
    (segment : operation_segment).

Fixpoint persistent_path_projection
  (path : persistent_operation_path) : list operation_segment :=
  match path with
  | PersistentPathRoot => []
  | PersistentPathAppend prefix segment =>
      persistent_path_projection prefix ++ [segment]
  end.

Definition persistent_path_append
  (path : persistent_operation_path)
  (segment : operation_segment) : persistent_operation_path :=
  PersistentPathAppend path segment.

Definition persistent_child_path
  (path : persistent_operation_path)
  (split_step child_index : nat) : persistent_operation_path :=
  persistent_path_append
    (persistent_path_append path (split_step, 1))
    (child_index, 0).

Fixpoint persistent_path_nodes
  (path : persistent_operation_path) : nat :=
  match path with
  | PersistentPathRoot => 0
  | PersistentPathAppend prefix _ => S (persistent_path_nodes prefix)
  end.

Theorem persistent_path_append_projection : forall path segment,
  persistent_path_projection (persistent_path_append path segment) =
  persistent_path_projection path ++ [segment].
Proof. reflexivity. Qed.

Theorem persistent_child_path_projection : forall path split_step child_index,
  persistent_path_projection
    (persistent_child_path path split_step child_index) =
  persistent_path_projection path ++ [(split_step, 1); (child_index, 0)].
Proof.
  intros.
  unfold persistent_child_path, persistent_path_append.
  simpl.
  now rewrite <- app_assoc.
Qed.

Theorem persistent_path_append_allocates_one_node : forall path segment,
  persistent_path_nodes (persistent_path_append path segment) =
  S (persistent_path_nodes path).
Proof. reflexivity. Qed.

Definition segment_lt (left right : operation_segment) : Prop :=
  fst left < fst right \/
  (fst left = fst right /\ snd left < snd right).

Inductive sequence_path_lt :
  list operation_segment -> list operation_segment -> Prop :=
| sequence_path_lt_prefix : forall head tail,
    sequence_path_lt [] (head :: tail)
| sequence_path_lt_head : forall left right left_tail right_tail,
    segment_lt left right ->
    sequence_path_lt (left :: left_tail) (right :: right_tail)
| sequence_path_lt_tail : forall head left_tail right_tail,
    sequence_path_lt left_tail right_tail ->
    sequence_path_lt (head :: left_tail) (head :: right_tail).

Definition persistent_path_lt
  (left right : persistent_operation_path) : Prop :=
  sequence_path_lt
    (persistent_path_projection left)
    (persistent_path_projection right).

Theorem persistent_path_order_refines_sequence_order : forall left right,
  persistent_path_lt left right <->
  sequence_path_lt
    (persistent_path_projection left)
    (persistent_path_projection right).
Proof. reflexivity. Qed.

Theorem equal_path_projections_preserve_all_comparisons : forall left right other,
  persistent_path_projection left = persistent_path_projection right ->
  (persistent_path_lt left other <-> persistent_path_lt right other) /\
  (persistent_path_lt other left <-> persistent_path_lt other right).
Proof.
  intros left right other Hequal.
  unfold persistent_path_lt.
  now rewrite Hequal.
Qed.

Definition before_split : operation_segment := (0, 0).
Definition split_child : operation_segment := (1, 1).
Definition after_join : operation_segment := (2, 0).

Theorem causal_operation_segments_are_monotone :
  segment_lt before_split split_child /\
  segment_lt split_child after_join.
Proof.
  unfold segment_lt, before_split, split_child, after_join.
  simpl.
  auto with arith.
Qed.

Record evaluation_epoch := {
  active_participants : nat;
  cancellation_requests : nat;
  completed_mutations : nat
}.

Definition root_with_detached_child : evaluation_epoch :=
  {| active_participants := 2;
     cancellation_requests := 0;
     completed_mutations := 0 |}.

Definition cancel_root_structured (epoch : evaluation_epoch) : evaluation_epoch :=
  {| active_participants := Nat.pred epoch.(active_participants);
     cancellation_requests := Nat.pred epoch.(active_participants);
     completed_mutations := epoch.(completed_mutations) |}.

Definition abort_requested_children (epoch : evaluation_epoch) : evaluation_epoch :=
  {| active_participants :=
       epoch.(active_participants) - epoch.(cancellation_requests);
     cancellation_requests := 0;
     completed_mutations := epoch.(completed_mutations) |}.

Definition complete_child_before_cancellation
  (epoch : evaluation_epoch) : evaluation_epoch :=
  {| active_participants := Nat.pred epoch.(active_participants);
     cancellation_requests := epoch.(cancellation_requests);
     completed_mutations := S epoch.(completed_mutations) |}.

Definition checkpoint_allowed (epoch : evaluation_epoch) : Prop :=
  epoch.(active_participants) = 0.

Theorem cancelled_root_retains_child_checkpoint_exclusion :
  ~ checkpoint_allowed (cancel_root_structured root_with_detached_child).
Proof. discriminate. Qed.

Theorem cancelled_root_owns_every_remaining_child :
  cancellation_requests (cancel_root_structured root_with_detached_child) =
  active_participants (cancel_root_structured root_with_detached_child).
Proof. reflexivity. Qed.

Theorem structured_child_abort_opens_checkpoint_without_mutation :
  checkpoint_allowed
    (abort_requested_children
      (cancel_root_structured root_with_detached_child)) /\
  completed_mutations
    (abort_requested_children
      (cancel_root_structured root_with_detached_child)) = 0.
Proof. split; reflexivity. Qed.

Theorem child_completion_before_cancellation_is_preserved :
  checkpoint_allowed
    (cancel_root_structured
      (complete_child_before_cancellation root_with_detached_child)) /\
  completed_mutations
    (cancel_root_structured
      (complete_child_before_cancellation root_with_detached_child)) = 1.
Proof. split; reflexivity. Qed.

Theorem structured_cancellation_does_not_fabricate_mutation : forall epoch,
  completed_mutations
    (abort_requested_children (cancel_root_structured epoch)) =
  completed_mutations epoch.
Proof. reflexivity. Qed.

Inductive driver_location :=
| InlineDriver
| SpawnedDriver.

Inductive execution_layer :=
| ParticipantSubmission
| InternalCommit.

Definition submits_reduction_intent (layer : execution_layer) : bool :=
  match layer with
  | ParticipantSubmission => true
  | InternalCommit => false
  end.

Record driver_lifecycle := {
  queued_intents : nat;
  in_flight_intents : nat;
  live_participants : nat;
  waiting_participants : nat;
  driver_active : bool
}.

Definition driver_readyb (lifecycle : driver_lifecycle) : bool :=
  negb lifecycle.(driver_active) &&
  Nat.ltb 0 lifecycle.(queued_intents) &&
  Nat.eqb lifecycle.(live_participants) lifecycle.(waiting_participants).

Definition claim_driver (lifecycle : driver_lifecycle) : driver_lifecycle :=
  if driver_readyb lifecycle
  then
    {| queued_intents := 0;
       in_flight_intents := lifecycle.(queued_intents);
       live_participants := lifecycle.(live_participants);
       waiting_participants := lifecycle.(waiting_participants);
       driver_active := true |}
  else lifecycle.

Definition submit_intent (lifecycle : driver_lifecycle) : driver_lifecycle :=
  {| queued_intents := S lifecycle.(queued_intents);
     in_flight_intents := lifecycle.(in_flight_intents);
     live_participants := lifecycle.(live_participants);
     waiting_participants := S lifecycle.(waiting_participants);
     driver_active := lifecycle.(driver_active) |}.

Definition submit_and_claim (lifecycle : driver_lifecycle) : driver_lifecycle :=
  claim_driver (submit_intent lifecycle).

Record located_driver := {
  located_lifecycle : driver_lifecycle;
  located_driver_location : driver_location
}.

Definition transfer_pending_driver (driver : located_driver) : located_driver :=
  {| located_lifecycle := driver.(located_lifecycle);
     located_driver_location := SpawnedDriver |}.

Definition execute_frontier_at
  (_ : driver_location)
  (frontier : list reduction_intent)
  (state : reduction_state) : reduction_state :=
  commit_frontier frontier state.

Definition execute_frontier_in_layer
  (_ : execution_layer)
  (location : driver_location)
  (frontier : list reduction_intent)
  (state : reduction_state) : reduction_state :=
  execute_frontier_at location frontier state.

Theorem ready_frontier_claims_exactly_one_driver : forall lifecycle,
  driver_readyb lifecycle = true ->
  driver_active (claim_driver lifecycle) = true /\
  queued_intents (claim_driver lifecycle) = 0 /\
  in_flight_intents (claim_driver lifecycle) = queued_intents lifecycle.
Proof.
  intros lifecycle Hready.
  unfold claim_driver.
  now rewrite Hready.
Qed.

Theorem driver_claim_is_idempotent : forall lifecycle,
  claim_driver (claim_driver lifecycle) = claim_driver lifecycle.
Proof.
  intros lifecycle.
  destruct (driver_readyb lifecycle) eqn:Hready.
  - unfold claim_driver.
    rewrite Hready.
    unfold driver_readyb.
    simpl.
    reflexivity.
  - unfold claim_driver.
    repeat rewrite Hready.
    reflexivity.
Qed.

Theorem last_waiter_claims_driver : forall queued live waiting,
  S waiting = live ->
  driver_active
    (submit_and_claim
      {| queued_intents := queued;
         in_flight_intents := 0;
         live_participants := live;
         waiting_participants := waiting;
         driver_active := false |}) = true.
Proof.
  intros queued live waiting Hcomplete.
  unfold submit_and_claim, submit_intent, claim_driver, driver_readyb.
  simpl.
  now rewrite Hcomplete, Nat.eqb_refl.
Qed.

Theorem pending_transfer_preserves_consensus_state : forall driver,
  located_lifecycle (transfer_pending_driver driver) =
  located_lifecycle driver.
Proof. reflexivity. Qed.

Theorem driver_location_does_not_change_frontier_result :
  forall location frontier state,
    execute_frontier_at location frontier state =
    commit_frontier frontier state.
Proof. reflexivity. Qed.

Theorem internal_commit_bypasses_intent_submission :
  submits_reduction_intent InternalCommit = false.
Proof. reflexivity. Qed.

Theorem execution_layer_does_not_change_frontier_result :
  forall layer location frontier state,
    execute_frontier_in_layer layer location frontier state =
    commit_frontier frontier state.
Proof. reflexivity. Qed.

Inductive comm_trigger_side :=
| ProduceTriggered
| ConsumeTriggered.

Definition execute_comm_continuation
  (_ : comm_trigger_side)
  (continuation : reduction_intent)
  (state : reduction_state) : reduction_state :=
  commit_intent continuation state.

Theorem comm_trigger_side_does_not_change_continuation_result :
  forall left right continuation state,
    execute_comm_continuation left continuation state =
    execute_comm_continuation right continuation state.
Proof. reflexivity. Qed.

Inductive single_participant_mode :=
| ScheduledSingleton
| DirectSingleton.

Definition execute_single_participant
  (_ : single_participant_mode)
  (intent : reduction_intent)
  (state : reduction_state) : reduction_state :=
  commit_intent intent state.

Theorem direct_single_participant_refines_scheduled_execution :
  forall intent state,
    execute_single_participant DirectSingleton intent state =
    execute_single_participant ScheduledSingleton intent state.
Proof. reflexivity. Qed.

Definition deterministic_parallel_reduction_contract : Prop :=
  (forall left right state,
      ~ intents_conflict left right ->
      commit_intent left (commit_intent right state) =
      commit_intent right (commit_intent left state)) /\
  intents_conflict produce_one shared_authority_y /\
  intents_conflict
    {| intent_order := 5;
       intent_channel := Y;
       intent_action := Produce 8;
       intent_authority_regions := [5; 2] |}
    produce_two /\
  commit_frontier parallel_component_schedule empty_reduction_state =
    commit_frontier canonical_frontier empty_reduction_state /\
  commit_frontier canonical_frontier empty_reduction_state =
    {| x_data := [2];
       y_data := [9];
       x_output := Some 1;
       y_output := None |} /\
  commit_frontier arrival_order_counterexample empty_reduction_state <>
    commit_frontier canonical_frontier empty_reduction_state /\
  segment_lt before_split split_child /\
  segment_lt split_child after_join /\
  ~ checkpoint_allowed
    (cancel_root_structured root_with_detached_child) /\
  cancellation_requests
    (cancel_root_structured root_with_detached_child) =
  active_participants
    (cancel_root_structured root_with_detached_child) /\
  checkpoint_allowed
    (abort_requested_children
      (cancel_root_structured root_with_detached_child)) /\
  completed_mutations
    (abort_requested_children
      (cancel_root_structured root_with_detached_child)) = 0 /\
  checkpoint_allowed
    (cancel_root_structured
      (complete_child_before_cancellation root_with_detached_child)) /\
  completed_mutations
    (cancel_root_structured
      (complete_child_before_cancellation root_with_detached_child)) = 1 /\
  (forall epoch,
      completed_mutations
        (abort_requested_children (cancel_root_structured epoch)) =
      completed_mutations epoch) /\
  (forall lifecycle,
      driver_readyb lifecycle = true ->
      driver_active (claim_driver lifecycle) = true /\
      queued_intents (claim_driver lifecycle) = 0 /\
      in_flight_intents (claim_driver lifecycle) = queued_intents lifecycle) /\
  (forall lifecycle,
      claim_driver (claim_driver lifecycle) = claim_driver lifecycle) /\
  (forall driver,
      located_lifecycle (transfer_pending_driver driver) =
      located_lifecycle driver) /\
  (forall location frontier state,
      execute_frontier_at location frontier state =
      commit_frontier frontier state) /\
  submits_reduction_intent InternalCommit = false /\
  (forall layer location frontier state,
      execute_frontier_in_layer layer location frontier state =
      commit_frontier frontier state) /\
  (forall left right continuation state,
      execute_comm_continuation left continuation state =
      execute_comm_continuation right continuation state) /\
  (forall intent state,
      execute_single_participant DirectSingleton intent state =
      execute_single_participant ScheduledSingleton intent state).

Theorem deterministic_parallel_reduction_end_to_end :
  deterministic_parallel_reduction_contract.
Proof.
  unfold deterministic_parallel_reduction_contract.
  refine (conj disjoint_intents_commute _).
  refine (conj compound_authority_overlap_is_a_conflict _).
  refine (conj compound_authority_order_does_not_hide_overlap _).
  refine (conj disjoint_parallel_schedule_refines_canonical _).
  refine (conj canonical_frontier_result _).
  refine (conj arrival_order_commit_is_not_canonical _).
  refine (conj (proj1 causal_operation_segments_are_monotone) _).
  refine (conj (proj2 causal_operation_segments_are_monotone) _).
  refine (conj cancelled_root_retains_child_checkpoint_exclusion _).
  refine (conj cancelled_root_owns_every_remaining_child _).
  refine (conj (proj1 structured_child_abort_opens_checkpoint_without_mutation) _).
  refine (conj (proj2 structured_child_abort_opens_checkpoint_without_mutation) _).
  refine (conj (proj1 child_completion_before_cancellation_is_preserved) _).
  refine (conj (proj2 child_completion_before_cancellation_is_preserved) _).
  refine (conj structured_cancellation_does_not_fabricate_mutation _).
  refine (conj ready_frontier_claims_exactly_one_driver _).
  refine (conj driver_claim_is_idempotent _).
  refine (conj pending_transfer_preserves_consensus_state _).
  refine (conj driver_location_does_not_change_frontier_result _).
  refine (conj internal_commit_bypasses_intent_submission _).
  refine (conj execution_layer_does_not_change_frontier_result _).
  refine (conj comm_trigger_side_does_not_change_continuation_result _).
  exact direct_single_participant_refines_scheduled_execution.
Qed.

Print Assumptions deterministic_parallel_reduction_end_to_end.
Print Assumptions persistent_path_append_projection.
Print Assumptions persistent_child_path_projection.
Print Assumptions persistent_path_append_allocates_one_node.
Print Assumptions persistent_path_order_refines_sequence_order.
Print Assumptions equal_path_projections_preserve_all_comparisons.
Print Assumptions ready_frontier_claims_exactly_one_driver.
Print Assumptions driver_claim_is_idempotent.
Print Assumptions last_waiter_claims_driver.
Print Assumptions pending_transfer_preserves_consensus_state.
Print Assumptions driver_location_does_not_change_frontier_result.
Print Assumptions internal_commit_bypasses_intent_submission.
Print Assumptions execution_layer_does_not_change_frontier_result.
Print Assumptions cancelled_root_owns_every_remaining_child.
Print Assumptions structured_child_abort_opens_checkpoint_without_mutation.
Print Assumptions child_completion_before_cancellation_is_preserved.
Print Assumptions structured_cancellation_does_not_fabricate_mutation.
Print Assumptions comm_trigger_side_does_not_change_continuation_result.
Print Assumptions direct_single_participant_refines_scheduled_execution.
