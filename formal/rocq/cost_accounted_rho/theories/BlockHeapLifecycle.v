From Stdlib Require Import Arith.PeanoNat Bool.Bool Lia.

Record block_heap_lifecycle := {
  heap_retained : nat;
  completions_since_trim : nat;
  semantic_commits : nat
}.

Definition next_trim_counter
  (current interval : nat)
  : nat * bool :=
  match interval with
  | 0 => (current, false)
  | S boundary =>
      if boundary <=? current then (0, true) else (S current, false)
  end.

Definition finish_block
  (reclamation_enabled : bool)
  (interval released : nat)
  (state : block_heap_lifecycle)
  : block_heap_lifecycle :=
  let '(next_counter, boundary) :=
    next_trim_counter (completions_since_trim state) interval in
  {|
    heap_retained :=
      if reclamation_enabled && boundary
      then 0
      else heap_retained state + released;
    completions_since_trim := next_counter;
    semantic_commits := S (semantic_commits state)
  |}.

Theorem positive_interval_counter_is_bounded :
  forall current interval,
    0 < interval ->
    current < interval ->
    fst (next_trim_counter current interval) < interval.
Proof.
  intros current [|boundary] positive bounded; try lia.
  unfold next_trim_counter.
  destruct (boundary <=? current) eqn:at_boundary; simpl.
  - lia.
  - apply Nat.leb_gt in at_boundary.
    lia.
Qed.

Theorem trim_is_requested_exactly_at_the_boundary :
  forall current interval,
    0 < interval ->
    current < interval ->
    (snd (next_trim_counter current interval) = true <->
     current = interval - 1).
Proof.
  intros current [|boundary] positive bounded; try lia.
  unfold next_trim_counter.
  destruct (boundary <=? current) eqn:at_boundary; simpl.
  - apply Nat.leb_le in at_boundary.
    split; intros; lia.
  - apply Nat.leb_gt in at_boundary.
    split; intros impossible; try discriminate; lia.
Qed.

Theorem every_block_default_reclaims_retained_heap :
  forall released state,
    heap_retained (finish_block true 1 released state) = 0.
Proof.
  intros released [retained counter commits].
  unfold finish_block, next_trim_counter.
  reflexivity.
Qed.

Theorem block_reclamation_is_semantically_invisible :
  forall reclamation_enabled interval released state,
    semantic_commits
      (finish_block reclamation_enabled interval released state) =
    S (semantic_commits state).
Proof.
  intros reclamation_enabled interval released state.
  unfold finish_block.
  destruct (next_trim_counter (completions_since_trim state) interval).
  reflexivity.
Qed.

Theorem reclamation_choice_does_not_change_semantic_commits :
  forall interval released state,
    semantic_commits (finish_block true interval released state) =
    semantic_commits (finish_block false interval released state).
Proof.
  intros interval released state.
  repeat rewrite block_reclamation_is_semantically_invisible.
  reflexivity.
Qed.

Theorem safe_finish_preserves_retained_counter_bound :
  forall interval released capacity state,
    0 < interval ->
    completions_since_trim state < interval ->
    heap_retained state <= completions_since_trim state * capacity ->
    released <= capacity ->
    heap_retained (finish_block true interval released state) <=
    completions_since_trim (finish_block true interval released state) * capacity.
Proof.
  intros [|boundary] released capacity [retained counter commits]
    positive counter_bounded retained_bounded released_bounded; try lia.
  unfold finish_block, next_trim_counter.
  simpl in *.
  destruct (boundary <=? counter) eqn:at_boundary; simpl.
  - lia.
  - apply Nat.leb_gt in at_boundary.
    nia.
Qed.

Theorem default_boundary_bounds_resident_heap :
  forall active parallelism capacity released state,
    active <= parallelism * capacity ->
    active + heap_retained (finish_block true 1 released state) <=
    parallelism * capacity.
Proof.
  intros active parallelism capacity released state active_bounded.
  rewrite every_block_default_reclaims_retained_heap.
  lia.
Qed.

Definition empty_heap_lifecycle : block_heap_lifecycle :=
  {| heap_retained := 0;
     completions_since_trim := 0;
     semantic_commits := 0 |}.

Definition three_blocks_without_reclamation : block_heap_lifecycle :=
  finish_block false 1 1
    (finish_block false 1 1
      (finish_block false 1 1 empty_heap_lifecycle)).

Theorem missing_boundary_reclamation_exceeds_two_slot_envelope :
  heap_retained three_blocks_without_reclamation > 2.
Proof.
  change (2 < 3).
  lia.
Qed.
