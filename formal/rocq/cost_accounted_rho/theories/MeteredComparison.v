(* DR-74: metered key comparisons charge only the inspected prefix and return
   exactly the order of the key's [Ord].

   A native index uses two comparators on one AVL tree: the metered
   comparator (lookup and insertion) and the plain [Ord] (prepaid lookup and
   batch commit). The tree invariant therefore requires the two orders to be
   identical. This module proves that identity for both chunked comparators.

   Rust correspondence (rholang native_runtime):
   - [metered_lex_compare]: index.rs [NativeBudgetOccurrence::compare_metered]
     on the path component: chunks of [k] = 16 segments over the shared prefix,
     then the length comparison. The derived [Ord] of [Vec] is [lex_compare].
   - [metered_operation_compare]: operations.rs [OperationKey::compare_metered]:
     the length comparison, then chunks over the reversed segments
     ([CausalPath::segments_rev]). The custom [Ord for OperationKey] is
     [operation_order].
   - [chunk_scan]: the shared [while] loop; its second component is the number
     of segments charged before each chunk is compared. *)

From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
Import ListNotations.

Section ChunkedComparison.

Context {A : Type} (cmp : A -> A -> comparison).

Fixpoint lex_compare (xs ys : list A) : comparison :=
  match xs, ys with
  | [], [] => Eq
  | [], _ :: _ => Lt
  | _ :: _, [] => Gt
  | x :: xs', y :: ys' =>
      match cmp x y with
      | Eq => lex_compare xs' ys'
      | other => other
      end
  end.

Fixpoint compare_prefix (n : nat) (xs ys : list A) : option comparison :=
  match n, xs, ys with
  | S n', x :: xs', y :: ys' =>
      match cmp x y with
      | Eq => compare_prefix n' xs' ys'
      | other => Some other
      end
  | _, _, _ => None
  end.

Lemma compare_prefix_nil_l : forall n ys, compare_prefix n [] ys = None.
Proof. intros [|n] ys; reflexivity. Qed.

Lemma compare_prefix_nil_r : forall n xs, compare_prefix n xs [] = None.
Proof. intros [|n] [|x xs]; reflexivity. Qed.

Lemma compare_prefix_split : forall a b xs ys,
  compare_prefix (a + b) xs ys =
  match compare_prefix a xs ys with
  | Some order => Some order
  | None => compare_prefix b (skipn a xs) (skipn a ys)
  end.
Proof.
  induction a as [|a IH]; intros b xs ys; [reflexivity |].
  destruct xs as [|x xs], ys as [|y ys]; simpl.
  - symmetry. apply compare_prefix_nil_l.
  - symmetry. apply compare_prefix_nil_l.
  - symmetry. apply compare_prefix_nil_r.
  - destruct (cmp x y); [apply IH | reflexivity | reflexivity].
Qed.

Lemma lex_compare_via_prefix : forall xs ys,
  lex_compare xs ys =
  match compare_prefix (Nat.min (length xs) (length ys)) xs ys with
  | Some order => order
  | None => Nat.compare (length xs) (length ys)
  end.
Proof.
  induction xs as [|x xs IH]; intros [|y ys]; simpl; try reflexivity.
  destruct (cmp x y); [apply IH | reflexivity | reflexivity].
Qed.

Fixpoint chunk_scan (fuel k remaining : nat) (xs ys : list A)
    : option comparison * nat :=
  match fuel with
  | 0 => (None, 0)
  | S fuel' =>
      match remaining with
      | 0 => (None, 0)
      | S _ =>
          let count := Nat.min k remaining in
          match compare_prefix count xs ys with
          | Some order => (Some order, count)
          | None =>
              let (result, charged) :=
                chunk_scan fuel' k (remaining - count) (skipn count xs) (skipn count ys)
              in (result, count + charged)
          end
      end
  end.

(* The chunked scan finds exactly the first difference within [remaining]. *)
Theorem chunk_scan_result : forall k fuel remaining xs ys,
  1 <= k ->
  remaining <= fuel ->
  fst (chunk_scan fuel k remaining xs ys) = compare_prefix remaining xs ys.
Proof.
  intros k fuel.
  induction fuel as [|fuel IH]; intros remaining xs ys positive bounded.
  - assert (remaining = 0) as -> by lia. reflexivity.
  - destruct remaining as [|remaining]; [reflexivity |].
    cbn [chunk_scan].
    set (count := Nat.min k (S remaining)).
    assert (count_positive : 1 <= count) by (unfold count; lia).
    assert (count_bounded : count <= S remaining) by (unfold count; lia).
    replace (compare_prefix (S remaining) xs ys)
      with (compare_prefix (count + (S remaining - count)) xs ys)
      by (f_equal; lia).
    rewrite compare_prefix_split.
    destruct (compare_prefix count xs ys) as [order |]; [reflexivity |].
    pose proof (IH (S remaining - count) (skipn count xs) (skipn count ys) positive
      ltac:(lia)) as recursive.
    destruct (chunk_scan fuel k (S remaining - count) (skipn count xs) (skipn count ys))
      as [result charged] eqn:scan.
    simpl in recursive |- *.
    exact recursive.
Qed.

(* The scan never charges beyond the compared range. *)
Theorem chunk_scan_charge_bounded : forall k fuel remaining xs ys,
  snd (chunk_scan fuel k remaining xs ys) <= remaining.
Proof.
  intros k fuel.
  induction fuel as [|fuel IH]; intros remaining xs ys; [cbn; lia |].
  destruct remaining as [|remaining]; [cbn; lia |].
  cbn [chunk_scan].
  pose proof (Nat.le_min_r k (S remaining)) as count_le.
  set (count := Nat.min k (S remaining)) in *.
  destruct (compare_prefix count xs ys) as [order |]; [cbn [snd]; exact count_le |].
  pose proof (IH (S remaining - count) (skipn count xs) (skipn count ys)) as bound.
  destruct (chunk_scan fuel k (S remaining - count) (skipn count xs) (skipn count ys))
    as [result charged] eqn:scan.
  cbn [snd] in bound |- *.
  lia.
Qed.

(* A scan that finds no difference charges the whole compared range. *)
Theorem chunk_scan_equal_prefix_charges_all : forall k fuel remaining xs ys,
  1 <= k ->
  remaining <= fuel ->
  fst (chunk_scan fuel k remaining xs ys) = None ->
  snd (chunk_scan fuel k remaining xs ys) = remaining.
Proof.
  intros k fuel.
  induction fuel as [|fuel IH]; intros remaining xs ys positive bounded no_difference.
  - cbn. lia.
  - destruct remaining as [|remaining]; [reflexivity |].
    cbn [chunk_scan] in *.
    pose proof (Nat.le_min_r k (S remaining)) as count_le.
    assert (count_positive : 1 <= Nat.min k (S remaining)) by lia.
    set (count := Nat.min k (S remaining)) in *.
    destruct (compare_prefix count xs ys) as [order |]; [discriminate |].
    pose proof (IH (S remaining - count) (skipn count xs) (skipn count ys) positive
      ltac:(lia)) as recursive.
    destruct (chunk_scan fuel k (S remaining - count) (skipn count xs) (skipn count ys))
      as [result charged] eqn:scan.
    cbn [fst snd] in recursive, no_difference |- *.
    specialize (recursive no_difference).
    lia.
Qed.

Definition metered_lex_compare (k : nat) (xs ys : list A) : comparison :=
  let shared := Nat.min (length xs) (length ys) in
  match fst (chunk_scan shared k shared xs ys) with
  | Some order => order
  | None => Nat.compare (length xs) (length ys)
  end.

(* The chunked path comparison equals the derived lexicographic order. *)
Theorem metered_lex_compare_correct : forall k xs ys,
  1 <= k -> metered_lex_compare k xs ys = lex_compare xs ys.
Proof.
  intros k xs ys positive.
  unfold metered_lex_compare.
  rewrite chunk_scan_result by (exact positive || lia).
  symmetry. apply lex_compare_via_prefix.
Qed.

Definition operation_order (xs ys : list A) : comparison :=
  match Nat.compare (length xs) (length ys) with
  | Eq => lex_compare (rev xs) (rev ys)
  | other => other
  end.

Definition metered_operation_compare (k : nat) (xs ys : list A) : comparison :=
  match Nat.compare (length xs) (length ys) with
  | Eq =>
      match fst (chunk_scan (length xs) k (length xs) (rev xs) (rev ys)) with
      | Some order => order
      | None => Eq
      end
  | other => other
  end.

(* The chunked operation-key comparison equals [Ord for OperationKey]. *)
Theorem metered_operation_compare_correct : forall k xs ys,
  1 <= k -> metered_operation_compare k xs ys = operation_order xs ys.
Proof.
  intros k xs ys positive.
  unfold metered_operation_compare, operation_order.
  destruct (Nat.compare (length xs) (length ys)) eqn:lengths; [| reflexivity | reflexivity].
  apply Nat.compare_eq in lengths.
  rewrite chunk_scan_result by (exact positive || lia).
  rewrite lex_compare_via_prefix.
  rewrite !length_rev, lengths, Nat.min_id, Nat.compare_refl.
  reflexivity.
Qed.

End ChunkedComparison.

Print Assumptions chunk_scan_result.
Print Assumptions chunk_scan_charge_bounded.
Print Assumptions chunk_scan_equal_prefix_charges_all.
Print Assumptions metered_lex_compare_correct.
Print Assumptions metered_operation_compare_correct.
