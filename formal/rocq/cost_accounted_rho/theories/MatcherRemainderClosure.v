From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
Import ListNotations.

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

Inductive projection_result := Accepted (values : list nat) | Refused.

Definition project_after_reservation (reserved : bool) targets selected :=
  if reserved then Accepted (remainder_closure targets selected) else Refused.

Theorem refused_projection_publishes_nothing : forall targets selected,
  project_after_reservation false targets selected = Refused.
Proof. reflexivity. Qed.
