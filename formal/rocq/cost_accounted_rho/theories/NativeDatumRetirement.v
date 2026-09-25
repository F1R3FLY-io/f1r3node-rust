From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
Import ListNotations.

Fixpoint retire_at {A : Type} (index : nat) (data : list A) {struct data} : list A :=
  match data, index with
  | [], _ => []
  | _ :: tail, 0 => tail
  | value :: tail, S next => value :: retire_at next tail
  end.

Theorem retirement_preserves_earlier_index : forall A (data : list A) earlier later,
  earlier < later -> nth_error (retire_at later data) earlier = nth_error data earlier.
Proof.
  intros A data. induction data as [|value tail IH]; intros earlier later below.
  - destruct earlier, later; reflexivity.
  - destruct earlier, later; simpl; try lia; auto. apply IH. lia.
Qed.

Theorem retirement_removes_one_linear_occurrence : forall A (data : list A) index,
  index < length data -> S (length (retire_at index data)) = length data.
Proof.
  intros A data. induction data as [|value tail IH]; intros index below; simpl in *; try lia.
  destruct index; simpl; auto. specialize (IH index). lia.
Qed.

Theorem retirement_descending_equals_adjusted_ascending : forall A (data : list A) earlier later,
  earlier < later ->
  retire_at earlier (retire_at later data) = retire_at (Nat.pred later) (retire_at earlier data).
Proof.
  intros A data. induction data as [|value tail IH]; intros earlier later below.
  - destruct earlier, later; reflexivity.
  - destruct earlier, later; simpl; try lia; auto.
    destruct later; simpl; try lia. f_equal. apply IH. lia.
Qed.

Fixpoint retire_indices {A : Type} (indices : list nat) (data : list A) :=
  match indices with [] => data | index :: tail => retire_indices tail (retire_at index data) end.

Theorem retirement_batch_preserves_lower_survivor : forall A (indices : list nat) (data : list A) survivor,
  (forall index, In index indices -> survivor < index) ->
  nth_error (retire_indices indices data) survivor = nth_error data survivor.
Proof.
  intros A indices. induction indices as [|index tail IH]; intros data survivor below; simpl; auto.
  rewrite IH.
  - apply retirement_preserves_earlier_index. apply below. now left.
  - intros. apply below. now right.
Qed.

Theorem retirement_next_original_index_survives : forall A (data : list A) next completed,
  (forall index, In index completed -> next < index) ->
  nth_error (retire_indices completed data) next = nth_error data next.
Proof. intros. now apply retirement_batch_preserves_lower_survivor. Qed.

Example retirement_ascending_corrupts_survivor : retire_indices [0; 1] [10; 20; 30] = [20].
Proof. reflexivity. Qed.

Definition retirement_selected (entry : option nat * bool) :=
  match entry with (Some _, false) => true | _ => false end.

Definition retirement_selection entries := filter retirement_selected entries.

Theorem retirement_selection_exact : forall entries position persistent,
  In (position, persistent) (retirement_selection entries) <->
  In (position, persistent) entries /\ persistent = false /\ position <> None.
Proof.
  intros. unfold retirement_selection. rewrite filter_In.
  destruct position, persistent; simpl; intuition discriminate.
Qed.

Theorem retirement_selection_excludes_incoming : forall entries persistent,
  ~In (None, persistent) (retirement_selection entries).
Proof. intros. rewrite retirement_selection_exact. intuition. Qed.

Theorem retirement_selection_excludes_persistent : forall entries position,
  ~In (position, true) (retirement_selection entries).
Proof. intros. rewrite retirement_selection_exact. intuition discriminate. Qed.

Theorem retirement_selection_capacity : forall entries,
  length (retirement_selection entries) <= length entries.
Proof. intros. apply filter_length_le. Qed.

Definition prepared_result_backing count result_width retirement_width :=
  count * result_width + count * retirement_width.

Theorem prepared_result_arrays_fit_reserved_backing : forall count retired result_width retirement_width,
  retired <= count ->
  count * result_width + retired * retirement_width <=
  prepared_result_backing count result_width retirement_width.
Proof. unfold prepared_result_backing. intros. nia. Qed.

Theorem prepared_result_backing_composes : forall first second result_width retirement_width,
  prepared_result_backing (first + second) result_width retirement_width =
  prepared_result_backing first result_width retirement_width +
  prepared_result_backing second result_width retirement_width.
Proof. unfold prepared_result_backing. intros. nia. Qed.

Example retirement_descending_keeps_survivor : retire_indices [1; 0] [10; 20; 30] = [30].
Proof. reflexivity. Qed.
