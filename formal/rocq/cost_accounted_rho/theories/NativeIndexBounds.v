From Stdlib Require Import Lists.List Bool.Bool Arith.PeanoNat Lia.
Import ListNotations.

Inductive index_shape :=
| IndexEmpty
| IndexNode (left_tree right_tree : index_shape).

Fixpoint index_nodes (tree : index_shape) : nat :=
  match tree with
  | IndexEmpty => 0
  | IndexNode left_tree right_tree => S (index_nodes left_tree + index_nodes right_tree)
  end.

Fixpoint index_height (tree : index_shape) : nat :=
  match tree with
  | IndexEmpty => 0
  | IndexNode left_tree right_tree => S (Nat.max (index_height left_tree) (index_height right_tree))
  end.

Fixpoint index_avl (tree : index_shape) : Prop :=
  match tree with
  | IndexEmpty => True
  | IndexNode left_tree right_tree =>
      index_avl left_tree /\ index_avl right_tree /\
      index_height left_tree <= S (index_height right_tree) /\
      index_height right_tree <= S (index_height left_tree)
  end.

Theorem index_avl_exponential_size : forall tree,
  index_avl tree -> forall exponent,
  2 * exponent <= index_height tree ->
  2 ^ exponent <= S (index_nodes tree).
Proof.
  induction tree as [|left IHleft right IHright]; intros balanced exponent tall.
  - simpl in tall. assert (exponent = 0) by lia. subst. simpl. lia.
  - simpl in balanced. destruct balanced as [left_avl [right_avl [lr rl]]].
    destruct exponent as [|exponent]; [simpl; lia|].
    simpl in tall.
    assert (left_tall : 2 * exponent <= index_height left).
    { destruct (Nat.max_spec (index_height left) (index_height right))
        as [[order equal] | [order equal]]; rewrite equal in tall; lia. }
    assert (right_tall : 2 * exponent <= index_height right).
    { destruct (Nat.max_spec (index_height left) (index_height right))
        as [[order equal] | [order equal]]; rewrite equal in tall; lia. }
    specialize (IHleft left_avl exponent left_tall).
    specialize (IHright right_avl exponent right_tall).
    simpl. lia.
Qed.

Definition index_height_budget (maximum_nodes : nat) : nat :=
  2 * S (Nat.log2 (S maximum_nodes)).

Theorem index_avl_height_bound : forall tree,
  index_avl tree -> index_height tree < index_height_budget (index_nodes tree).
Proof.
  intros tree balanced. unfold index_height_budget.
  destruct (Nat.lt_ge_cases (index_height tree)
    (2 * S (Nat.log2 (S (index_nodes tree))))) as [bounded | tall]; auto.
  pose proof (index_avl_exponential_size tree balanced
    (S (Nat.log2 (S (index_nodes tree)))) tall) as size_bound.
  pose proof (Nat.log2_spec (S (index_nodes tree)) ltac:(lia)) as [_ log_bound].
  lia.
Qed.

Theorem index_height_budget_monotone : forall actual maximum,
  actual <= maximum -> index_height_budget actual <= index_height_budget maximum.
Proof.
  intros actual maximum bounded. unfold index_height_budget.
  pose proof (Nat.log2_le_mono (S actual) (S maximum) ltac:(lia)). lia.
Qed.

Theorem index_height_budget_machine_word : forall maximum word_bits,
  maximum < 2 ^ word_bits -> index_height_budget maximum <= 2 * S word_bits.
Proof.
  intros maximum word_bits fits. unfold index_height_budget.
  pose proof (Nat.log2_le_mono (S maximum) (2 ^ word_bits) ltac:(lia)) as bounded.
  rewrite Nat.log2_pow2 in bounded by lia. lia.
Qed.

Theorem index_avl_configured_height_bound : forall tree maximum,
  index_avl tree -> index_nodes tree <= maximum ->
  index_height tree < index_height_budget maximum.
Proof.
  intros tree maximum balanced bounded.
  pose proof (index_avl_height_bound tree balanced).
  pose proof (index_height_budget_monotone _ _ bounded). lia.
Qed.

Fixpoint index_lookup_visits (tree : index_shape) (directions : list bool) : nat :=
  match tree with
  | IndexEmpty => 0
  | IndexNode left_tree right_tree =>
      match directions with
      | [] => 1
      | go_left :: rest => S (index_lookup_visits (if go_left then left_tree else right_tree) rest)
      end
  end.

Theorem index_lookup_visits_height : forall tree directions,
  index_lookup_visits tree directions <= index_height tree.
Proof.
  induction tree as [|left IHleft right IHright]; intros directions; simpl; [lia|].
  destruct directions as [|go_left rest]; [lia|].
  destruct go_left; simpl.
  - specialize (IHleft rest). pose proof (Nat.le_max_l (index_height left) (index_height right)). lia.
  - specialize (IHright rest). pose proof (Nat.le_max_r (index_height left) (index_height right)). lia.
Qed.

Theorem index_prepaid_finish_bound : forall tree maximum directions comparison_work,
  index_avl tree -> index_nodes tree <= maximum ->
  index_lookup_visits tree directions * comparison_work <=
    index_height_budget maximum * comparison_work.
Proof.
  intros tree maximum directions comparison_work balanced bounded.
  pose proof (index_lookup_visits_height tree directions).
  pose proof (index_avl_configured_height_bound tree maximum balanced bounded). nia.
Qed.

Theorem index_prepaid_finish_survives_growth : forall before after maximum directions comparison_work,
  index_nodes before <= index_nodes after ->
  index_avl after -> index_nodes after <= maximum ->
  index_lookup_visits after directions * comparison_work <=
    index_height_budget maximum * comparison_work.
Proof. intros. now apply index_prepaid_finish_bound. Qed.

Inductive index_geometric_storage : nat -> nat -> nat -> Prop :=
| IndexInitialStorage : forall initial,
    0 < initial -> index_geometric_storage initial initial 0
| IndexGrowStorage : forall capacity requested moved next live,
    index_geometric_storage capacity requested moved ->
    2 * capacity <= next -> live <= capacity ->
    index_geometric_storage next (requested + next) (moved + live).

Definition index_growth_target capacity required :=
  Nat.max 4 (Nat.max required (2 * capacity)).

Theorem index_growth_target_sufficient : forall capacity required,
  4 <= index_growth_target capacity required /\
  required <= index_growth_target capacity required /\
  2 * capacity <= index_growth_target capacity required.
Proof.
  intros capacity required. unfold index_growth_target.
  pose proof (Nat.le_max_l 4 (Nat.max required (2 * capacity))).
  pose proof (Nat.le_max_r 4 (Nat.max required (2 * capacity))).
  pose proof (Nat.le_max_l required (2 * capacity)).
  pose proof (Nat.le_max_r required (2 * capacity)). lia.
Qed.

Theorem index_geometric_storage_bounds : forall capacity requested moved,
  index_geometric_storage capacity requested moved ->
  0 < capacity /\ capacity <= requested /\ requested < 2 * capacity /\ moved < capacity.
Proof.
  intros capacity requested moved history. induction history; lia.
Qed.

Theorem index_geometric_requested_bytes : forall capacity requested moved slot_bytes,
  index_geometric_storage capacity requested moved ->
  requested * slot_bytes <= 2 * capacity * slot_bytes.
Proof.
  intros capacity requested moved slot_bytes history.
  pose proof (index_geometric_storage_bounds _ _ _ history). nia.
Qed.

Theorem index_geometric_movement_bytes : forall capacity requested moved slot_bytes,
  index_geometric_storage capacity requested moved ->
  moved * slot_bytes <= capacity * slot_bytes.
Proof.
  intros capacity requested moved slot_bytes history.
  pose proof (index_geometric_storage_bounds _ _ _ history). nia.
Qed.

Theorem index_resize_peak_covered : forall capacity requested moved next slot_bytes,
  index_geometric_storage capacity requested moved ->
  (capacity + next) * slot_bytes <= (requested + next) * slot_bytes.
Proof.
  intros capacity requested moved next slot_bytes history.
  pose proof (index_geometric_storage_bounds _ _ _ history). nia.
Qed.

Theorem index_final_clamped_growth_bounds : forall capacity requested moved next live,
  index_geometric_storage capacity requested moved ->
  capacity < next -> live <= capacity ->
  requested + next < 3 * next /\ moved + live < 2 * next.
Proof.
  intros capacity requested moved next live history growing live_bound.
  pose proof (index_geometric_storage_bounds _ _ _ history). lia.
Qed.

Record index_publication_state := {
  index_entries : list nat;
  index_debit : nat;
  index_prepared : option (nat * nat);
  index_failed : bool
}.

Definition index_preflight (state : index_publication_state)
    (entry debit : nat) (checks : list bool) : index_publication_state :=
  if index_failed state then state
  else if forallb (fun check => check) checks then
    {| index_entries := index_entries state;
       index_debit := index_debit state;
       index_prepared := Some (entry, debit);
       index_failed := false |}
  else
    {| index_entries := index_entries state;
       index_debit := index_debit state;
       index_prepared := None;
       index_failed := true |}.

Definition index_publish (state : index_publication_state) : index_publication_state :=
  if index_failed state then state
  else match index_prepared state with
  | None => state
  | Some (entry, debit) =>
      {| index_entries := index_entries state ++ [entry];
         index_debit := index_debit state + debit;
         index_prepared := None;
         index_failed := false |}
  end.

Inductive index_publication_action :=
| IndexPreflight (entry debit : nat) (checks : list bool)
| IndexPublish.

Definition index_publication_step state action :=
  match action with
  | IndexPreflight entry debit checks => index_preflight state entry debit checks
  | IndexPublish => index_publish state
  end.

Definition index_publication_run state actions :=
  fold_left index_publication_step actions state.

Theorem index_preflight_preserves_effects : forall state entry debit checks,
  index_entries (index_preflight state entry debit checks) = index_entries state /\
  index_debit (index_preflight state entry debit checks) = index_debit state.
Proof.
  intros. unfold index_preflight.
  destruct (index_failed state); [auto|].
  destruct (forallb (fun check => check) checks); simpl; auto.
Qed.

Theorem index_failed_preflight_marks_failure : forall state entry debit checks,
  In false checks -> index_failed (index_preflight state entry debit checks) = true.
Proof.
  intros state entry debit checks failed_check. unfold index_preflight.
  destruct (index_failed state) eqn:failed; [exact failed|].
  destruct (forallb (fun check => check) checks) eqn:checked; [|reflexivity].
  apply forallb_forall with (x := false) in checked; [discriminate|assumption].
Qed.

Theorem index_failed_step_unchanged : forall state action,
  index_failed state = true -> index_publication_step state action = state.
Proof.
  intros state action failed. destruct action;
    unfold index_publication_step, index_preflight, index_publish; now rewrite failed.
Qed.

Theorem index_failed_run_unchanged : forall actions state,
  index_failed state = true -> index_publication_run state actions = state.
Proof.
  induction actions as [|action rest IH]; intros state failed; [reflexivity|].
  unfold index_publication_run in *. simpl.
  rewrite index_failed_step_unchanged by assumption. now apply IH.
Qed.

Theorem index_no_publication_after_failed_preflight : forall state entry debit checks actions,
  In false checks ->
  index_entries (index_publication_run (index_preflight state entry debit checks) actions) =
    index_entries state /\
  index_debit (index_publication_run (index_preflight state entry debit checks) actions) =
    index_debit state.
Proof.
  intros state entry debit checks actions failed_check.
  rewrite index_failed_run_unchanged.
  - apply index_preflight_preserves_effects.
  - now apply index_failed_preflight_marks_failure.
Qed.

Theorem index_successful_preflight_publishes_once : forall state entry debit checks,
  index_failed state = false -> forallb (fun check => check) checks = true ->
  let published := index_publish (index_preflight state entry debit checks) in
  index_entries published = index_entries state ++ [entry] /\
  index_debit published = index_debit state + debit /\
  index_publish published = published.
Proof.
  intros state entry debit checks not_failed checked.
  unfold index_preflight. rewrite not_failed, checked. simpl.
  unfold index_publish. simpl. auto.
Qed.
