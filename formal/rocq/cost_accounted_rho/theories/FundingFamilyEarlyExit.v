(* DR-131 (decision D-J): the early exit of the funding family search.

   The search accepts a node of its branch and bound only when the padded
   sum of the node fits the exposure limit. The padded sum adds, for each
   source, the larger of the source's hold and the source's lower bound. The
   hold of a source is the largest debit of that source over the cases. The
   debits of one case sum to the charge of that case, because the selection
   funds every obligation and the unit fee.

   So the padded sum of every node is at least the sum of each row of debits,
   and at least the charge of each case (case_charge_le_padded_sum). When the
   largest charge of the cases exceeds the exposure limit, no node fits the
   limit (largest_charge_above_exposure_admits_no_node). The early exit then
   returns "no allocation" at once, which is the result of the full search.

   Results:
   - length_zip_max, length_holds: a pointwise maximum keeps the length of
     its rows.
   - zip_max_left, zip_max_right: each operand is at most the pointwise
     maximum, entry by entry.
   - row_le_holds: each row of debits is at most the holds, entry by entry.
   - case_charge_le_padded_sum: the sum of each row is at most the padded
     sum.
   - charge_above_exposure_rejects_every_node: a row whose sum exceeds the
     exposure puts the padded sum above the exposure.
   - largest_charge_above_exposure_admits_no_node: the same result for the
     largest charge of the cases, as the Rust code computes it.
   Negative control:
   - minimum_in_place_of_maximum_breaks_the_bound: with the pointwise
     minimum in place of the maximum, two cases that each charge 2 give a
     sum of 0, so the bound fails.

   No axiom, parameter or admission is used.

   Rust correspondence: rholang/src/rust/interpreter/accounting/
   monetary_allocation/funding_family_optimizer.rs (validate computes the
   largest case charge; optimize_funding_family returns no allocation when
   that charge exceeds the exposure limit; relaxed_optimum takes each hold as
   the largest debit of its source over the cases; search_funding_family
   accepts a node only when its padded sum fits the limit). The property test
   family_optimizer_early_exit_matches_the_search_loop compares the early exit
   with the search loop alone. *)

From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
Import ListNotations.

Definition total (xs : list nat) : nat := fold_right Nat.add 0 xs.

Fixpoint zip_max (a b : list nat) : list nat :=
  match a, b with
  | x :: a', y :: b' => Nat.max x y :: zip_max a' b'
  | _, _ => []
  end.

Fixpoint zip_min (a b : list nat) : list nat :=
  match a, b with
  | x :: a', y :: b' => Nat.min x y :: zip_min a' b'
  | _, _ => []
  end.

(* The holds of n sources: the pointwise maximum of the rows of debits. *)
Definition holds (n : nat) (cases : list (list nat)) : list nat :=
  fold_right zip_max (repeat 0 n) cases.

(* The padded sum of a node with the lower bounds [lower]. *)
Definition padded_sum (n : nat) (cases : list (list nat)) (lower : list nat) : nat :=
  total (zip_max (holds n cases) lower).

Lemma length_zip_max : forall a b,
  length (zip_max a b) = Nat.min (length a) (length b).
Proof.
  induction a as [| x a IH]; intros [| y b]; simpl; try reflexivity.
  rewrite IH. reflexivity.
Qed.

Lemma length_holds : forall n cases,
  Forall (fun row => length row = n) cases -> length (holds n cases) = n.
Proof.
  intros n cases rows. unfold holds.
  induction rows as [| row cases Hrow Hrest IH]; simpl.
  - apply repeat_length.
  - rewrite length_zip_max, Hrow, IH. apply Nat.min_id.
Qed.

Lemma zip_max_left : forall a b,
  length a = length b -> Forall2 le a (zip_max a b).
Proof.
  induction a as [| x a IH]; intros [| y b] Hlen; simpl in *; try discriminate.
  - constructor.
  - constructor; [lia | apply IH; lia].
Qed.

Lemma zip_max_right : forall a b,
  length a = length b -> Forall2 le b (zip_max a b).
Proof.
  induction a as [| x a IH]; intros [| y b] Hlen; simpl in *; try discriminate.
  - constructor.
  - constructor; [lia | apply IH; lia].
Qed.

Lemma forall2_le_trans : forall a b c,
  Forall2 le a b -> Forall2 le b c -> Forall2 le a c.
Proof.
  intros a b c Hab. revert c.
  induction Hab as [| x y a b Hxy Hab IH]; intros c Hbc.
  - inversion Hbc. constructor.
  - inversion Hbc as [| y' z b' c' Hyz Hbc']. subst.
    constructor; [lia | apply IH; exact Hbc'].
Qed.

Lemma total_le_of_forall2 : forall a b, Forall2 le a b -> total a <= total b.
Proof.
  intros a b H. unfold total.
  induction H as [| x y a b Hxy Hrest IH]; simpl; lia.
Qed.

Lemma row_le_holds : forall n cases row,
  Forall (fun r => length r = n) cases -> In row cases ->
  Forall2 le row (holds n cases).
Proof.
  intros n cases row Hlen.
  induction Hlen as [| r cases Hr Hrest IH]; intros Hin.
  - simpl in Hin. contradiction.
  - change (holds n (r :: cases)) with (zip_max r (holds n cases)).
    destruct Hin as [Hsame | Hin].
    + subst row. apply zip_max_left.
      rewrite Hr, length_holds by exact Hrest. reflexivity.
    + apply forall2_le_trans with (b := holds n cases).
      * apply IH. exact Hin.
      * apply zip_max_right.
        rewrite Hr, length_holds by exact Hrest. reflexivity.
Qed.

Theorem case_charge_le_padded_sum : forall n cases lower row,
  Forall (fun r => length r = n) cases -> length lower = n -> In row cases ->
  total row <= padded_sum n cases lower.
Proof.
  intros n cases lower row Hlen Hlower Hin. unfold padded_sum.
  apply total_le_of_forall2.
  apply forall2_le_trans with (b := holds n cases).
  - apply row_le_holds; assumption.
  - apply zip_max_left. rewrite length_holds by exact Hlen.
    symmetry. exact Hlower.
Qed.

Corollary charge_above_exposure_rejects_every_node :
  forall n cases lower row exposure,
  Forall (fun r => length r = n) cases -> length lower = n -> In row cases ->
  exposure < total row -> exposure < padded_sum n cases lower.
Proof.
  intros n cases lower row exposure Hlen Hlower Hin Habove.
  pose proof (case_charge_le_padded_sum n cases lower row Hlen Hlower Hin).
  lia.
Qed.

Lemma max_above_has_member : forall e xs,
  e < fold_right Nat.max 0 xs -> exists x, In x xs /\ e < x.
Proof.
  intros e xs. induction xs as [| x xs IH]; simpl; intros Habove.
  - lia.
  - destruct (Nat.max_spec x (fold_right Nat.max 0 xs))
      as [[Hlt Heq] | [Hge Heq]]; rewrite Heq in Habove.
    + destruct (IH Habove) as [y [Hin Hy]]. exists y.
      split; [right; exact Hin | exact Hy].
    + exists x. split; [left; reflexivity | exact Habove].
Qed.

Lemma forall2_in_right : forall (A B : Type) (R : A -> B -> Prop) xs ys y,
  Forall2 R xs ys -> In y ys -> exists x, In x xs /\ R x y.
Proof.
  intros A B R xs ys y H.
  induction H as [| a b la lb Hab Hrest IH]; simpl; intros Hin.
  - contradiction.
  - destruct Hin as [Hsame | Hin].
    + subst b. exists a. split; [left; reflexivity | exact Hab].
    + destruct (IH Hin) as [x [Hx Hr]]. exists x.
      split; [right; exact Hx | exact Hr].
Qed.

(* [charges] lists the charge of each case. The debits of each case sum to
   its charge, and the Rust code compares the largest charge with the
   exposure limit. *)
Corollary largest_charge_above_exposure_admits_no_node :
  forall n cases lower charges exposure,
  Forall (fun r => length r = n) cases -> length lower = n ->
  Forall2 (fun row charge => total row = charge) cases charges ->
  exposure < fold_right Nat.max 0 charges ->
  exposure < padded_sum n cases lower.
Proof.
  intros n cases lower charges exposure Hlen Hlower Hcharges Habove.
  destruct (max_above_has_member exposure charges Habove)
    as [charge [Hin Hcharge]].
  destruct (forall2_in_right _ _ _ cases charges charge Hcharges Hin)
    as [row [Hrow Htotal]].
  apply (charge_above_exposure_rejects_every_node n cases lower row exposure
    Hlen Hlower Hrow).
  lia.
Qed.

(* Negative control: the pointwise minimum of two cases that each charge 2 is
   [0; 0]. Its sum is below the charge of each case, so a bound through the
   minimum fails. The pointwise maximum keeps the bound. *)
Example minimum_in_place_of_maximum_breaks_the_bound :
  total (zip_min [2; 0] [0; 2]) < total [2; 0]
  /\ total [2; 0] <= total (zip_max [2; 0] [0; 2]).
Proof. vm_compute. split; lia. Qed.
