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

Definition segment_lt (left right : operation_segment) : Prop :=
  fst left < fst right \/
  (fst left = fst right /\ snd left < snd right).

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
  completed_mutations : nat
}.

Definition root_with_detached_child : evaluation_epoch :=
  {| active_participants := 2;
     completed_mutations := 0 |}.

Definition cancel_root (epoch : evaluation_epoch) : evaluation_epoch :=
  {| active_participants := Nat.pred epoch.(active_participants);
     completed_mutations := epoch.(completed_mutations) |}.

Definition complete_child (epoch : evaluation_epoch) : evaluation_epoch :=
  {| active_participants := Nat.pred epoch.(active_participants);
     completed_mutations := S epoch.(completed_mutations) |}.

Definition checkpoint_allowed (epoch : evaluation_epoch) : Prop :=
  epoch.(active_participants) = 0.

Theorem cancelled_root_retains_child_checkpoint_exclusion :
  ~ checkpoint_allowed (cancel_root root_with_detached_child).
Proof. discriminate. Qed.

Theorem detached_child_completion_opens_complete_checkpoint :
  checkpoint_allowed
    (complete_child (cancel_root root_with_detached_child)) /\
  completed_mutations
    (complete_child (cancel_root root_with_detached_child)) = 1.
Proof. split; reflexivity. Qed.

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
  ~ checkpoint_allowed (cancel_root root_with_detached_child) /\
  checkpoint_allowed
    (complete_child (cancel_root root_with_detached_child)) /\
  completed_mutations
    (complete_child (cancel_root root_with_detached_child)) = 1.

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
  exact detached_child_completion_opens_complete_checkpoint.
Qed.

Print Assumptions deterministic_parallel_reduction_end_to_end.
