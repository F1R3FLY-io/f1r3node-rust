From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia
  Sorting.Permutation.
Import ListNotations.

From CasperHostWork Require Import HostWorkBudget.

Inductive host_phase : Type :=
  | StructuralAdmission
  | PureReduction
  | PrimitiveEvaluation
  | Substitution
  | AuthorityDiscovery
  | PhysicalSearch
  | WitnessDecoding
  | WitnessVerification.

Inductive host_dimension : Type :=
  | StructuralItems
  | StructuralBytes
  | ReductionSteps
  | ReductionTermBytes
  | PrimitiveCalls
  | PrimitiveInputBytes
  | SubstitutionBindings
  | SubstitutionBytes
  | AuthorityNodes
  | AuthorityDepth
  | SearchCandidates
  | SearchStateBytes
  | WitnessFields
  | WitnessBytes
  | VerificationOperations
  | VerificationBytes.

Definition host_dimension_phase (dimension : host_dimension) : host_phase :=
  match dimension with
  | StructuralItems | StructuralBytes => StructuralAdmission
  | ReductionSteps | ReductionTermBytes => PureReduction
  | PrimitiveCalls | PrimitiveInputBytes => PrimitiveEvaluation
  | SubstitutionBindings | SubstitutionBytes => Substitution
  | AuthorityNodes | AuthorityDepth => AuthorityDiscovery
  | SearchCandidates | SearchStateBytes => PhysicalSearch
  | WitnessFields | WitnessBytes => WitnessDecoding
  | VerificationOperations | VerificationBytes => WitnessVerification
  end.

Theorem every_host_dimension_has_one_phase :
  forall dimension,
    exists phase, host_dimension_phase dimension = phase.
Proof.
  intros dimension.
  exists (host_dimension_phase dimension).
  reflexivity.
Qed.

Section GenericDimensions.

Context {Dimension ScheduleId : Type}.
Variable dimension_eq_dec :
  forall left right : Dimension, {left = right} + {left <> right}.
Variable schedule_eq_dec :
  forall left right : ScheduleId, {left = right} + {left <> right}.
Variable schedule_limit : ScheduleId -> Dimension -> nat.

Definition dimension_usage := Dimension -> nat.

Definition update_dimension
  (usage : dimension_usage)
  (target : Dimension)
  (next : nat)
  : dimension_usage :=
  fun dimension =>
    if dimension_eq_dec dimension target then next else usage dimension.

Record work_item := {
  work_dimension : Dimension;
  work_units : nat
}.

Definition reserve_dimension
  (counter_max : nat)
  (schedule : ScheduleId)
  (usage : dimension_usage)
  (item : work_item)
  : dimension_usage * reserve_result :=
  let dimension := work_dimension item in
  let current := usage dimension in
  let result := checked_reserve
    counter_max
    (schedule_limit schedule dimension)
    current
    (work_units item) in
  match result with
  | ReserveGranted next => (update_dimension usage dimension next, result)
  | ReserveLimitRejected => (usage, result)
  | ReserveOverflowRejected => (usage, result)
  end.

Lemma update_dimension_at_target :
  forall usage target next,
    update_dimension usage target next target = next.
Proof.
  intros usage target next.
  unfold update_dimension.
  destruct (dimension_eq_dec target target); congruence.
Qed.

Lemma update_dimension_frames_other :
  forall usage target next observed,
    observed <> target ->
    update_dimension usage target next observed = usage observed.
Proof.
  intros usage target next observed distinct.
  unfold update_dimension.
  destruct (dimension_eq_dec observed target); congruence.
Qed.

Theorem reserve_dimension_granted_bounds :
  forall counter_max schedule usage item next_usage next_used,
    reserve_dimension counter_max schedule usage item =
      (next_usage, ReserveGranted next_used) ->
    next_usage (work_dimension item) = next_used /\
    next_used <= counter_max /\
    next_used <= schedule_limit schedule (work_dimension item).
Proof.
  intros counter_max schedule usage item next_usage next_used reserved.
  unfold reserve_dimension in reserved.
  remember (checked_reserve counter_max
    (schedule_limit schedule (work_dimension item))
    (usage (work_dimension item))
    (work_units item)) as result eqn:decision.
  destruct result as [granted_used | |]; try discriminate.
  inversion reserved; subst next_usage next_used.
  rewrite update_dimension_at_target.
  pose proof (checked_reserve_granted_bounds
    counter_max
    (schedule_limit schedule (work_dimension item))
    (usage (work_dimension item))
    (work_units item)
    granted_used
    (eq_sym decision)) as bounds.
  tauto.
Qed.

Theorem reserve_dimension_frames_other_dimensions :
  forall counter_max schedule usage item next_usage result observed,
    reserve_dimension counter_max schedule usage item = (next_usage, result) ->
    observed <> work_dimension item ->
    next_usage observed = usage observed.
Proof.
  intros counter_max schedule usage item next_usage result observed
    reserved distinct.
  unfold reserve_dimension in reserved.
  destruct (checked_reserve counter_max
    (schedule_limit schedule (work_dimension item))
    (usage (work_dimension item))
    (work_units item)); inversion reserved; subst next_usage result.
  - apply update_dimension_frames_other.
    exact distinct.
  - reflexivity.
  - reflexivity.
Qed.

Theorem failed_dimension_reserve_preserves_all_dimensions :
  forall counter_max schedule usage item next_usage rejection,
    (rejection = ReserveLimitRejected \/
     rejection = ReserveOverflowRejected) ->
    reserve_dimension counter_max schedule usage item =
      (next_usage, rejection) ->
    forall observed, next_usage observed = usage observed.
Proof.
  intros counter_max schedule usage item next_usage rejection
    failed reserved observed.
  destruct failed as [limit_rejected | overflow_rejected];
    subst rejection; unfold reserve_dimension in reserved;
    destruct (checked_reserve counter_max
      (schedule_limit schedule (work_dimension item))
      (usage (work_dimension item))
      (work_units item)); inversion reserved; reflexivity.
Qed.

Definition dimension_step
  (counter_max : nat)
  (schedule : ScheduleId)
  (usage : dimension_usage)
  (item : work_item)
  : dimension_usage :=
  fst (reserve_dimension counter_max schedule usage item).

Lemma dimension_step_frames_other :
  forall counter_max schedule usage item observed,
    observed <> work_dimension item ->
    dimension_step counter_max schedule usage item observed = usage observed.
Proof.
  intros counter_max schedule usage item observed distinct.
  unfold dimension_step, reserve_dimension.
  destruct (checked_reserve counter_max
    (schedule_limit schedule (work_dimension item))
    (usage (work_dimension item))
    (work_units item)).
  - apply update_dimension_frames_other.
    exact distinct.
  - reflexivity.
  - reflexivity.
Qed.

Lemma dimension_step_at_target_depends_only_on_target :
  forall counter_max schedule left_usage right_usage item,
    left_usage (work_dimension item) = right_usage (work_dimension item) ->
    dimension_step counter_max schedule left_usage item (work_dimension item) =
      dimension_step counter_max schedule right_usage item (work_dimension item).
Proof.
  intros counter_max schedule left_usage right_usage item same.
  unfold dimension_step, reserve_dimension.
  rewrite same.
  destruct (checked_reserve counter_max
    (schedule_limit schedule (work_dimension item))
    (right_usage (work_dimension item))
    (work_units item)).
  - simpl.
    repeat rewrite update_dimension_at_target.
    reflexivity.
  - simpl.
    exact same.
  - simpl.
    exact same.
Qed.

Theorem zero_unit_reserve_preserves_usage :
  forall counter_max schedule usage dimension next_usage result,
    reserve_dimension counter_max schedule usage
      {| work_dimension := dimension; work_units := 0 |} =
      (next_usage, result) ->
    usage dimension <= counter_max ->
    usage dimension <= schedule_limit schedule dimension ->
    forall observed, next_usage observed = usage observed.
Proof.
  intros counter_max schedule usage dimension next_usage result
    reserved within_counter within_limit observed.
  unfold reserve_dimension in reserved.
  simpl in reserved.
  assert ((counter_max <? usage dimension) = false) as counter_before.
  { apply Nat.ltb_ge. exact within_counter. }
  assert ((counter_max - usage dimension <? 0) = false) as counter_after.
  { apply Nat.ltb_ge. lia. }
  assert ((schedule_limit schedule dimension <? usage dimension) = false)
    as limit_before.
  { apply Nat.ltb_ge. exact within_limit. }
  assert ((schedule_limit schedule dimension - usage dimension <? 0) = false)
    as limit_after.
  { apply Nat.ltb_ge. lia. }
  unfold checked_reserve in reserved.
  rewrite counter_before, counter_after, limit_before, limit_after in reserved.
  inversion reserved; subst next_usage result.
  destruct (dimension_eq_dec observed dimension) as [same | different].
  - subst observed.
    rewrite update_dimension_at_target.
    lia.
  - apply update_dimension_frames_other.
    exact different.
Qed.

Theorem independent_dimension_reservations_commute :
  forall counter_max schedule usage left_item right_item observed,
    work_dimension left_item <> work_dimension right_item ->
    dimension_step counter_max schedule
      (dimension_step counter_max schedule usage left_item)
      right_item observed =
    dimension_step counter_max schedule
      (dimension_step counter_max schedule usage right_item)
      left_item observed.
Proof.
  intros counter_max schedule usage left_item right_item observed distinct.
  destruct (dimension_eq_dec observed (work_dimension left_item))
    as [is_left | not_left].
  - subst observed.
    rewrite dimension_step_frames_other by exact distinct.
    symmetry.
    apply dimension_step_at_target_depends_only_on_target.
    apply dimension_step_frames_other.
    exact distinct.
  - destruct (dimension_eq_dec observed (work_dimension right_item))
      as [is_right | not_right].
    + subst observed.
      rewrite (dimension_step_frames_other
        counter_max schedule
        (dimension_step counter_max schedule usage right_item)
        left_item (work_dimension right_item) not_left).
      apply dimension_step_at_target_depends_only_on_target.
      apply dimension_step_frames_other.
      intro same.
      apply distinct.
      symmetry.
      exact same.
    + rewrite dimension_step_frames_other by exact not_right.
      rewrite dimension_step_frames_other by exact not_left.
      rewrite dimension_step_frames_other by exact not_left.
      rewrite dimension_step_frames_other by exact not_right.
      reflexivity.
Qed.

Fixpoint run_items
  (counter_max : nat)
  (schedule : ScheduleId)
  (usage : dimension_usage)
  (items : list work_item)
  : dimension_usage * list reserve_result :=
  match items with
  | [] => (usage, [])
  | item :: remaining =>
      let '(next_usage, result) :=
        reserve_dimension counter_max schedule usage item in
      let '(final_usage, results) :=
        run_items counter_max schedule next_usage remaining in
      (final_usage, result :: results)
  end.

Record execution_checkpoint := {
  checkpoint_counter_max : nat;
  checkpoint_schedule : ScheduleId;
  checkpoint_initial_usage : dimension_usage;
  checkpoint_items : list work_item;
  checkpoint_usage : dimension_usage;
  checkpoint_results : list reserve_result
}.

Definition checkpoint_valid (checkpoint : execution_checkpoint) : Prop :=
  let '(usage, results) := run_items
    (checkpoint_counter_max checkpoint)
    (checkpoint_schedule checkpoint)
    (checkpoint_initial_usage checkpoint)
    (checkpoint_items checkpoint) in
  (forall dimension, checkpoint_usage checkpoint dimension = usage dimension) /\
  checkpoint_results checkpoint = results.

Definition replay_checkpoint
  (supplied_schedule : ScheduleId)
  (checkpoint : execution_checkpoint)
  : option (dimension_usage * list reserve_result) :=
  if schedule_eq_dec supplied_schedule (checkpoint_schedule checkpoint)
  then Some (run_items
    (checkpoint_counter_max checkpoint)
    supplied_schedule
    (checkpoint_initial_usage checkpoint)
    (checkpoint_items checkpoint))
  else None.

Theorem replay_schedule_agreement :
  forall checkpoint supplied_schedule replay_usage replay_results,
    checkpoint_valid checkpoint ->
    replay_checkpoint supplied_schedule checkpoint =
      Some (replay_usage, replay_results) ->
    supplied_schedule = checkpoint_schedule checkpoint /\
    (forall dimension,
      replay_usage dimension = checkpoint_usage checkpoint dimension) /\
    replay_results = checkpoint_results checkpoint.
Proof.
  intros checkpoint supplied_schedule replay_usage replay_results
    valid replayed.
  unfold replay_checkpoint in replayed.
  destruct (schedule_eq_dec supplied_schedule
    (checkpoint_schedule checkpoint)) as [same | mismatch].
  - subst supplied_schedule.
    destruct (run_items
      (checkpoint_counter_max checkpoint)
      (checkpoint_schedule checkpoint)
      (checkpoint_initial_usage checkpoint)
      (checkpoint_items checkpoint)) as [expected_usage expected_results]
      eqn:execution.
    inversion replayed; subst replay_usage replay_results.
    unfold checkpoint_valid in valid.
    rewrite execution in valid.
    destruct valid as [usage_agrees results_agree].
    split.
    + reflexivity.
    + split.
      * intros dimension.
        symmetry.
        apply usage_agrees.
      * symmetry.
        exact results_agree.
  - discriminate.
Qed.

Theorem replay_rejects_schedule_mismatch :
  forall checkpoint supplied_schedule,
    supplied_schedule <> checkpoint_schedule checkpoint ->
    replay_checkpoint supplied_schedule checkpoint = None.
Proof.
  intros checkpoint supplied_schedule mismatch.
  unfold replay_checkpoint.
  destruct (schedule_eq_dec supplied_schedule
    (checkpoint_schedule checkpoint)); congruence.
Qed.

Definition reserve_grantedb (result : reserve_result) : bool :=
  match result with
  | ReserveGranted _ => true
  | ReserveLimitRejected => false
  | ReserveOverflowRejected => false
  end.

Fixpoint all_reservations_granted (results : list reserve_result) : bool :=
  match results with
  | [] => true
  | result :: remaining =>
      reserve_grantedb result && all_reservations_granted remaining
  end.

Inductive deployment_verdict : Type :=
  | DeploymentAccepted
  | DeploymentRejected.

Definition execute_transaction
  (counter_max : nat)
  (schedule : ScheduleId)
  (initial : dimension_usage)
  (items : list work_item)
  : deployment_verdict * dimension_usage :=
  let '(final_usage, results) :=
    run_items counter_max schedule initial items in
  if all_reservations_granted results
  then (DeploymentAccepted, final_usage)
  else (DeploymentRejected, initial).

Definition published_checkpoint
  (outcome : deployment_verdict * dimension_usage)
  : option dimension_usage :=
  match fst outcome with
  | DeploymentAccepted => Some (snd outcome)
  | DeploymentRejected => None
  end.

Definition publishes_mutation
  (outcome : deployment_verdict * dimension_usage)
  : bool :=
  match fst outcome with
  | DeploymentAccepted => true
  | DeploymentRejected => false
  end.

Theorem failed_transaction_rolls_back :
  forall counter_max schedule initial items final_usage results,
    run_items counter_max schedule initial items =
      (final_usage, results) ->
    all_reservations_granted results = false ->
    fst (execute_transaction counter_max schedule initial items) =
      DeploymentRejected /\
    forall dimension,
      snd (execute_transaction counter_max schedule initial items) dimension =
        initial dimension.
Proof.
  intros counter_max schedule initial items final_usage results
    execution failed.
  unfold execute_transaction.
  rewrite execution, failed.
  split.
  - reflexivity.
  - intros dimension.
    reflexivity.
Qed.

Theorem failed_transaction_publishes_no_checkpoint_or_mutation :
  forall counter_max schedule initial items final_usage results,
    run_items counter_max schedule initial items =
      (final_usage, results) ->
    all_reservations_granted results = false ->
    published_checkpoint
      (execute_transaction counter_max schedule initial items) = None /\
    publishes_mutation
      (execute_transaction counter_max schedule initial items) = false.
Proof.
  intros counter_max schedule initial items final_usage results
    execution failed.
  unfold execute_transaction.
  rewrite execution, failed.
  split; reflexivity.
Qed.

Theorem failed_transaction_end_verdict_is_schedule_independent :
  forall counter_max left_schedule right_schedule initial left_items right_items
         left_usage left_results right_usage right_results,
    Permutation left_items right_items ->
    run_items counter_max left_schedule initial left_items =
      (left_usage, left_results) ->
    run_items counter_max right_schedule initial right_items =
      (right_usage, right_results) ->
    all_reservations_granted left_results = false ->
    all_reservations_granted right_results = false ->
    fst (execute_transaction counter_max left_schedule initial left_items) =
      fst (execute_transaction counter_max right_schedule initial right_items) /\
    forall dimension,
      snd (execute_transaction counter_max left_schedule initial left_items) dimension =
      snd (execute_transaction counter_max right_schedule initial right_items) dimension.
Proof.
  intros counter_max left_schedule right_schedule initial left_items right_items
    left_usage left_results right_usage right_results
    complete_events left_execution right_execution left_failed right_failed.
  unfold execute_transaction.
  rewrite left_execution, right_execution, left_failed, right_failed.
  split.
  - reflexivity.
  - intros dimension.
    reflexivity.
Qed.

Record attempt_state := {
  attempt_checkpoint : dimension_usage;
  attempt_working : dimension_usage;
  attempt_economic_balance : nat
}.

Definition rollback_attempt (state : attempt_state) : attempt_state :=
  {|
    attempt_checkpoint := attempt_checkpoint state;
    attempt_working := attempt_checkpoint state;
    attempt_economic_balance := attempt_economic_balance state
  |}.

Theorem rollback_restores_checkpoint :
  forall state dimension,
    attempt_working (rollback_attempt state) dimension =
      attempt_checkpoint state dimension.
Proof.
  intros state dimension.
  reflexivity.
Qed.

Theorem rollback_preserves_economic_balance :
  forall state,
    attempt_economic_balance (rollback_attempt state) =
      attempt_economic_balance state.
Proof.
  intros state.
  reflexivity.
Qed.

End GenericDimensions.

Section IndependentShards.

Context {Shard Dimension ScheduleId : Type}.
Variable shard_eq_dec : forall left right : Shard, {left = right} + {left <> right}.
Variable dimension_eq_dec :
  forall left right : Dimension, {left = right} + {left <> right}.
Variable schedule_limit : ScheduleId -> Dimension -> nat.

Definition shard_world := Shard -> @dimension_usage Dimension.

Definition replace_shard
  (world : shard_world)
  (target : Shard)
  (replacement : @dimension_usage Dimension)
  : shard_world :=
  fun shard => if shard_eq_dec shard target then replacement else world shard.

Definition shard_reserve
  (counter_max : nat)
  (schedule : ScheduleId)
  (world : shard_world)
  (target : Shard)
  (item : @work_item Dimension)
  : shard_world :=
  replace_shard world target
    (fst (@reserve_dimension Dimension ScheduleId
      dimension_eq_dec schedule_limit
      counter_max schedule (world target) item)).

Lemma replace_shard_at_target :
  forall world target replacement,
    replace_shard world target replacement target = replacement.
Proof.
  intros world target replacement.
  unfold replace_shard.
  destruct (shard_eq_dec target target); congruence.
Qed.

Lemma replace_shard_frames_other :
  forall world target replacement observed,
    observed <> target ->
    replace_shard world target replacement observed = world observed.
Proof.
  intros world target replacement observed distinct.
  unfold replace_shard.
  destruct (shard_eq_dec observed target); congruence.
Qed.

Lemma shard_reserve_frames_other :
  forall counter_max schedule world target item observed,
    observed <> target ->
    shard_reserve counter_max schedule world target item observed = world observed.
Proof.
  intros counter_max schedule world target item observed distinct.
  unfold shard_reserve.
  apply replace_shard_frames_other.
  exact distinct.
Qed.

Lemma shard_reserve_at_target_depends_only_on_target :
  forall counter_max schedule left_world right_world target item,
    left_world target = right_world target ->
    shard_reserve counter_max schedule left_world target item target =
      shard_reserve counter_max schedule right_world target item target.
Proof.
  intros counter_max schedule left_world right_world target item same.
  unfold shard_reserve.
  repeat rewrite replace_shard_at_target.
  rewrite same.
  reflexivity.
Qed.

Theorem independent_shard_reservations_commute :
  forall counter_max left_schedule right_schedule world left right
         left_item right_item observed,
    left <> right ->
    shard_reserve counter_max right_schedule
      (shard_reserve counter_max left_schedule world left left_item)
      right right_item observed =
    shard_reserve counter_max left_schedule
      (shard_reserve counter_max right_schedule world right right_item)
      left left_item observed.
Proof.
  intros counter_max left_schedule right_schedule world left right
    left_item right_item observed distinct.
  destruct (shard_eq_dec observed left) as [is_left | not_left].
  - subst observed.
    rewrite shard_reserve_frames_other by exact distinct.
    symmetry.
    apply shard_reserve_at_target_depends_only_on_target.
    apply shard_reserve_frames_other.
    exact distinct.
  - destruct (shard_eq_dec observed right) as [is_right | not_right].
    + subst observed.
      rewrite (shard_reserve_frames_other
        counter_max left_schedule
        (shard_reserve counter_max right_schedule world right right_item)
        left left_item right not_left).
      apply shard_reserve_at_target_depends_only_on_target.
      apply shard_reserve_frames_other.
      intro same.
      apply distinct.
      symmetry.
      exact same.
    + rewrite shard_reserve_frames_other by exact not_right.
      rewrite shard_reserve_frames_other by exact not_left.
      rewrite shard_reserve_frames_other by exact not_left.
      rewrite shard_reserve_frames_other by exact not_right.
      reflexivity.
Qed.

End IndependentShards.
