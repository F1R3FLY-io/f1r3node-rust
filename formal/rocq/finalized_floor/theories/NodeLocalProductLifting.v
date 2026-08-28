From Stdlib Require Import Lists.List.
Import ListNotations.

Section NodeLocalProductLifting.

Context {Node LocalState Action : Type}.
Variable node_eq_dec : forall left right : Node, {left = right} + {left <> right}.
Variable local_step : Action -> LocalState -> LocalState.
Variable local_invariant : LocalState -> Prop.
Variable local_goal : LocalState -> Prop.
Variable local_enabled : Action -> LocalState -> Prop.

Definition world := Node -> LocalState.

Definition update_world
  (current : world)
  (target : Node)
  (action : Action)
  : world :=
  fun observer =>
    if node_eq_dec observer target
    then local_step action (current target)
    else current observer.

Definition global_invariant (current : world) : Prop :=
  forall node, local_invariant (current node).

Definition world_equivalent (left right : world) : Prop :=
  forall node, left node = right node.

Fixpoint run_schedule
  (current : world)
  (schedule : list (Node * Action))
  : world :=
  match schedule with
  | [] => current
  | (node, action) :: remaining =>
      run_schedule (update_world current node action) remaining
  end.

Theorem update_world_at_target :
  forall current target action,
    update_world current target action target =
      local_step action (current target).
Proof.
  intros current target action.
  unfold update_world.
  destruct (node_eq_dec target target); congruence.
Qed.

Theorem update_world_frames_other :
  forall current target action observer,
    observer <> target ->
    update_world current target action observer = current observer.
Proof.
  intros current target action observer distinct.
  unfold update_world.
  destruct (node_eq_dec observer target); congruence.
Qed.

Theorem local_preservation_lifts_to_arbitrary_nodes :
  (forall action state,
    local_invariant state ->
    local_invariant (local_step action state)) ->
  forall current target action,
    global_invariant current ->
    global_invariant (update_world current target action).
Proof.
  intros preserves current target action invariant observer.
  destruct (node_eq_dec observer target) as [same | distinct].
  - subst observer.
    rewrite update_world_at_target.
    apply preserves.
    apply invariant.
  - rewrite update_world_frames_other by exact distinct.
    apply invariant.
Qed.

Theorem finite_schedule_preserves_global_invariant :
  (forall action state,
    local_invariant state ->
    local_invariant (local_step action state)) ->
  forall schedule current,
    global_invariant current ->
    global_invariant (run_schedule current schedule).
Proof.
  intros preserves schedule.
  induction schedule as [| [target action] remaining induction_hypothesis].
  - intros current invariant.
    exact invariant.
  - intros current invariant.
    simpl.
    apply induction_hypothesis.
    apply local_preservation_lifts_to_arbitrary_nodes.
    + exact preserves.
    + exact invariant.
Qed.

Theorem update_world_preserves_world_equivalence :
  forall left right,
    world_equivalent left right ->
    forall target action,
      world_equivalent
        (update_world left target action)
        (update_world right target action).
Proof.
  intros left right equivalent target action observer.
  destruct (node_eq_dec observer target) as [same | distinct].
  - subst observer.
    repeat rewrite update_world_at_target.
    rewrite equivalent.
    reflexivity.
  - repeat rewrite update_world_frames_other by exact distinct.
    apply equivalent.
Qed.

Theorem run_schedule_preserves_world_equivalence :
  forall schedule left right,
    world_equivalent left right ->
    world_equivalent
      (run_schedule left schedule)
      (run_schedule right schedule).
Proof.
  intros schedule.
  induction schedule as [| [target action] remaining induction_hypothesis].
  - intros left right equivalent.
    exact equivalent.
  - intros left right equivalent.
    simpl.
    apply induction_hypothesis.
    apply update_world_preserves_world_equivalence.
    exact equivalent.
Qed.

Theorem distinct_node_updates_commute :
  forall current left right left_action right_action,
    left <> right ->
    world_equivalent
      (update_world
        (update_world current left left_action)
        right right_action)
      (update_world
        (update_world current right right_action)
        left left_action).
Proof.
  intros current left right left_action right_action distinct.
  intros observer.
  destruct (node_eq_dec observer left) as [observer_left | observer_not_left].
  - subst observer.
    rewrite (update_world_frames_other
      (update_world current left left_action) right right_action left distinct).
    rewrite (update_world_at_target current left left_action).
    rewrite (update_world_at_target
      (update_world current right right_action) left left_action).
    rewrite (update_world_frames_other current right right_action left distinct).
    reflexivity.
  - destruct (node_eq_dec observer right) as [observer_right | observer_not_right].
    + subst observer.
      assert (right <> left) as reverse_distinct by congruence.
      rewrite (update_world_at_target
        (update_world current left left_action) right right_action).
      rewrite (update_world_frames_other
        current left left_action right reverse_distinct).
      rewrite (update_world_frames_other
        (update_world current right right_action)
        left left_action right reverse_distinct).
      rewrite (update_world_at_target current right right_action).
      reflexivity.
    + repeat rewrite update_world_frames_other by assumption.
      reflexivity.
Qed.

Theorem adjacent_independent_schedule_steps_commute :
  forall current left right left_action right_action remaining,
    left <> right ->
    world_equivalent
      (run_schedule current
        ((left, left_action) :: (right, right_action) :: remaining))
      (run_schedule current
        ((right, right_action) :: (left, left_action) :: remaining)).
Proof.
  intros current left right left_action right_action remaining distinct.
  simpl.
  apply run_schedule_preserves_world_equivalence.
  apply distinct_node_updates_commute.
  exact distinct.
Qed.

Theorem distinct_node_update_preserves_enablement :
  forall current target observer target_action observer_action,
    observer <> target ->
    (local_enabled observer_action
      (update_world current target target_action observer) <->
     local_enabled observer_action (current observer)).
Proof.
  intros current target observer target_action observer_action distinct.
  rewrite update_world_frames_other by exact distinct.
  reflexivity.
Qed.

Theorem distinct_node_update_preserves_goal :
  forall current target observer action,
    observer <> target ->
    local_goal (current observer) ->
    local_goal (update_world current target action observer).
Proof.
  intros current target observer action distinct goal.
  rewrite update_world_frames_other by exact distinct.
  exact goal.
Qed.

Definition node_local_product_contract : Prop :=
  (forall action state,
    local_invariant state ->
    local_invariant (local_step action state)) ->
  (forall schedule current,
    global_invariant current ->
    global_invariant (run_schedule current schedule)) /\
  (forall current left right left_action right_action,
    left <> right ->
    world_equivalent
      (update_world
        (update_world current left left_action)
        right right_action)
      (update_world
        (update_world current right right_action)
        left left_action)) /\
  (forall current left right left_action right_action remaining,
    left <> right ->
    world_equivalent
      (run_schedule current
        ((left, left_action) :: (right, right_action) :: remaining))
      (run_schedule current
        ((right, right_action) :: (left, left_action) :: remaining))) /\
  (forall current target observer target_action observer_action,
    observer <> target ->
    (local_enabled observer_action
      (update_world current target target_action observer) <->
     local_enabled observer_action (current observer))) /\
  (forall current target observer action,
    observer <> target ->
    local_goal (current observer) ->
    local_goal (update_world current target action observer)).

Theorem node_local_product_lifting_correct :
  node_local_product_contract.
Proof.
  unfold node_local_product_contract.
  intros preserves.
  split.
  - apply finite_schedule_preserves_global_invariant.
    exact preserves.
  - split.
    + apply distinct_node_updates_commute.
    + split.
      * apply adjacent_independent_schedule_steps_commute.
      * split.
        -- apply distinct_node_update_preserves_enablement.
        -- apply distinct_node_update_preserves_goal.
Qed.

End NodeLocalProductLifting.

Print Assumptions local_preservation_lifts_to_arbitrary_nodes.
Print Assumptions finite_schedule_preserves_global_invariant.
Print Assumptions update_world_preserves_world_equivalence.
Print Assumptions run_schedule_preserves_world_equivalence.
Print Assumptions distinct_node_updates_commute.
Print Assumptions adjacent_independent_schedule_steps_commute.
Print Assumptions distinct_node_update_preserves_enablement.
Print Assumptions node_local_product_lifting_correct.
