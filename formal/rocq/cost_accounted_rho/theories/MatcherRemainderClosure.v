(* The remainder projection of the matcher (CBR-052).

   Changed by the merge of feature/cost-accounted-rho (finding R1-35). The
   F1R3Lang matcher gives a Par remainder exactly the target positions that
   the assignment gives to the remainder row. That is CBR-052, from commit
   406506646. The checkpoint merge aed9f4427 replaced this rule with a value
   closure. The closure keeps every target that is equal to a selected
   target, and this module then proved properties of the closure. The merge
   restored the positional rule in three places: the stack-safe matcher
   (finish_list in spatial_matcher_pda.rs), the compatibility matcher
   (list_match.rs) and the recursive test oracle. This module now models the
   positional rule. It keeps the value closure as a proved negative control.

   The model. [targets] is the list of the original targets. A number stands
   for a target value, so equal numbers are equal targets. [mask] has one
   Boolean for each target. A bit is true when the complete assignment gives
   its target to the remainder row. The Rust code builds this mask from the
   assignment (remainder_target_indices). It keeps the targets whose bit is
   true, in target order.

   Sections:
   1. The value closure of the checkpoint merge: [remainder_closure],
      [subsequence] and their lemmas, unchanged.
   2. The positional projection [remainder_positions], as finish_list
      computes it.
   3. Its theorems. The projection keeps the target order
      ([positions_preserve_original_order]). It keeps each value once for
      each selected position ([positions_keep_exact_multiplicity],
      [positions_length]). With the complement mask of the term rows, it
      partitions the targets ([positions_partition_the_targets]).
   4. Negative control with a concrete witness: the closure keeps both
      occurrences of a duplicate when the mask selects one of them
      ([value_closure_double_counts_a_duplicate]).
   5. The search alternatives. A projection runs after the choice, so it
      prunes no alternative.

   The model uses no axiom. *)

From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
Import ListNotations.

(* Section 1. The value closure of the checkpoint merge, unchanged. *)

(* The search chooses distinct target indices. Only after that choice is
   complete does the legacy matcher project by value over the original list. *)
Definition selected_valueb (targets selected : list nat) (value : nat) : bool :=
  existsb (fun index =>
    match nth_error targets index with
    | Some candidate => Nat.eqb value candidate
    | None => false
    end) selected.

Definition remainder_closure (targets selected : list nat) : list nat :=
  filter (selected_valueb targets selected) targets.

Lemma selected_valueb_iff : forall targets selected value,
  selected_valueb targets selected value = true <->
  exists index, In index selected /\ nth_error targets index = Some value.
Proof.
  intros targets selected value. unfold selected_valueb. rewrite existsb_exists.
  split.
  - intros [index [member found]]. exists index. split; [exact member|].
    destruct (nth_error targets index) as [candidate|] eqn:lookup; try discriminate.
    apply Nat.eqb_eq in found. now subst candidate.
  - intros [index [member lookup]]. exists index. split; [exact member|].
    rewrite lookup. apply Nat.eqb_refl.
Qed.

Inductive subsequence : list nat -> list nat -> Prop :=
| subsequence_nil : subsequence [] []
| subsequence_keep : forall value kept original,
    subsequence kept original -> subsequence (value :: kept) (value :: original)
| subsequence_drop : forall value kept original,
    subsequence kept original -> subsequence kept (value :: original).

Lemma filter_preserves_order : forall predicate original,
  subsequence (filter predicate original) original.
Proof.
  intros predicate original. induction original as [|value rest IH]; simpl.
  - constructor.
  - destruct (predicate value); [now constructor|now constructor].
Qed.

Theorem closure_preserves_original_order : forall targets selected,
  subsequence (remainder_closure targets selected) targets.
Proof. intros. apply filter_preserves_order. Qed.

Lemma count_filter_by_value : forall predicate targets value,
  count_occ Nat.eq_dec (filter predicate targets) value =
  if predicate value then count_occ Nat.eq_dec targets value else 0.
Proof.
  intros predicate targets. induction targets as [|head rest IH]; intros value; simpl.
  - now destruct (predicate value).
  - destruct (Nat.eq_dec head value) as [same|different].
    + subst head. destruct (predicate value) eqn:keep; simpl.
      * destruct (Nat.eq_dec value value) as [_|impossible]; [|contradiction].
        rewrite IH, keep. simpl. destruct (Nat.eq_dec value value); [lia|contradiction].
      * rewrite IH, keep. simpl. destruct (Nat.eq_dec value value); [reflexivity|contradiction].
    + destruct (predicate head); simpl.
      * destruct (Nat.eq_dec head value) as [impossible|_]; [contradiction|].
        rewrite IH. simpl. destruct (Nat.eq_dec head value); [contradiction|reflexivity].
      * rewrite IH. simpl. destruct (Nat.eq_dec head value); [contradiction|].
        now destruct (predicate value).
Qed.

Theorem closure_keeps_every_equal_occurrence : forall targets selected value,
  count_occ Nat.eq_dec (remainder_closure targets selected) value =
  if selected_valueb targets selected value
  then count_occ Nat.eq_dec targets value else 0.
Proof. intros. apply count_filter_by_value. Qed.

Example one_selected_duplicate_keeps_both :
  remainder_closure [7; 7] [0] = [7; 7].
Proof. reflexivity. Qed.

Example empty_remainder_keeps_none : forall targets,
  remainder_closure targets [] = [].
Proof. intros. unfold remainder_closure, selected_valueb. now induction targets. Qed.

(* Carry the complete index-distinct alternative roster through projection;
   no equality-based deduplication is allowed during the search itself. *)
Definition publish_alternatives (targets : list nat) (choices : list (list nat)) :=
  map (fun selected => (selected, remainder_closure targets selected)) choices.

Theorem projection_does_not_prune_search_alternatives : forall targets choices,
  map fst (publish_alternatives targets choices) = choices.
Proof. intros. unfold publish_alternatives. now induction choices as [|choice rest IH]; simpl; rewrite ?IH. Qed.

(* Disabled by the merge of feature/cost-accounted-rho (DR-131): the matcher
   reserves no host work before the projection now, so finish_list has no
   refused projection. The model of that reservation follows.

Inductive projection_result := Accepted (values : list nat) | Refused.

Definition project_after_reservation (reserved : bool) targets selected :=
  if reserved then Accepted (remainder_closure targets selected) else Refused.

Theorem refused_projection_publishes_nothing : forall targets selected,
  project_after_reservation false targets selected = Refused.
Proof. reflexivity. Qed.
*)

(* Section 2. The positional projection, as finish_list computes it. *)

Fixpoint remainder_positions (targets : list nat) (mask : list bool) : list nat :=
  match targets, mask with
  | target :: targets', selected :: mask' =>
      if selected
      then target :: remainder_positions targets' mask'
      else remainder_positions targets' mask'
  | _, _ => []
  end.

(* The number of positions that the mask selects and whose target is [value]. *)
Fixpoint selected_occurrences (targets : list nat) (mask : list bool) (value : nat) : nat :=
  match targets, mask with
  | target :: targets', selected :: mask' =>
      (if selected then (if Nat.eq_dec target value then 1 else 0) else 0)
      + selected_occurrences targets' mask' value
  | _, _ => 0
  end.

(* Section 3. The theorems of the positional projection. *)

Lemma subsequence_empty : forall original, subsequence [] original.
Proof.
  induction original as [|value rest IH]; [constructor|now apply subsequence_drop].
Qed.

Theorem positions_preserve_original_order : forall targets mask,
  subsequence (remainder_positions targets mask) targets.
Proof.
  induction targets as [|target rest IH]; intros mask.
  - constructor.
  - destruct mask as [|selected mask']; simpl.
    + apply subsequence_empty.
    + destruct selected.
      * apply subsequence_keep. apply IH.
      * apply subsequence_drop. apply IH.
Qed.

Theorem positions_keep_exact_multiplicity : forall targets mask value,
  count_occ Nat.eq_dec (remainder_positions targets mask) value =
  selected_occurrences targets mask value.
Proof.
  induction targets as [|target rest IH]; intros mask value.
  - reflexivity.
  - destruct mask as [|selected mask']; [reflexivity|].
    destruct selected; simpl; rewrite IH;
      destruct (Nat.eq_dec target value); simpl; lia.
Qed.

Theorem positions_length : forall targets mask,
  length mask = length targets ->
  length (remainder_positions targets mask) = count_occ Bool.bool_dec mask true.
Proof.
  induction targets as [|target rest IH]; intros mask same_length.
  - destruct mask; [reflexivity|discriminate].
  - destruct mask as [|selected mask']; [discriminate|].
    simpl in same_length. injection same_length as same_length.
    destruct selected; simpl; rewrite (IH mask' same_length); reflexivity.
Qed.

Theorem positions_partition_the_targets : forall targets mask value,
  length mask = length targets ->
  count_occ Nat.eq_dec (remainder_positions targets mask) value
  + count_occ Nat.eq_dec (remainder_positions targets (map negb mask)) value
  = count_occ Nat.eq_dec targets value.
Proof.
  induction targets as [|target rest IH]; intros mask value same_length.
  - destruct mask; reflexivity.
  - destruct mask as [|selected mask']; [discriminate|].
    simpl in same_length. injection same_length as same_length.
    specialize (IH mask' value same_length).
    destruct selected; simpl; destruct (Nat.eq_dec target value); lia.
Qed.

Example one_selected_duplicate_keeps_one :
  remainder_positions [7; 7] [true; false] = [7].
Proof. reflexivity. Qed.

Example three_identical_sends_keep_two :
  remainder_positions [7; 7; 7] [false; true; true] = [7; 7].
Proof. reflexivity. Qed.

Example empty_mask_keeps_none :
  remainder_positions [7; 3; 7] [false; false; false] = [].
Proof. reflexivity. Qed.

(* Section 4. Negative control. The mask gives one of two equal targets to
   the remainder, and the term row takes the other one. The value closure
   keeps both targets. So the remainder and the term row hold three
   occurrences of a value that the targets hold twice. The positional
   projection gives two, as [positions_partition_the_targets] requires. *)
Theorem value_closure_double_counts_a_duplicate :
  length (remainder_closure [7; 7] [0])
  + length (remainder_positions [7; 7] [false; true]) = 3
  /\ length (remainder_positions [7; 7] [true; false])
  + length (remainder_positions [7; 7] [false; true]) = 2.
Proof. split; reflexivity. Qed.

(* Section 5. The positional projection runs after the choice of each
   alternative, so it prunes none of them. *)
Definition publish_position_alternatives (targets : list nat) (masks : list (list bool)) :=
  map (fun mask => (mask, remainder_positions targets mask)) masks.

Theorem position_projection_does_not_prune_search_alternatives : forall targets masks,
  map fst (publish_position_alternatives targets masks) = masks.
Proof.
  intros. unfold publish_position_alternatives.
  now induction masks as [|mask rest IH]; simpl; rewrite ?IH.
Qed.
