(**
  Abstract comparison contract for the metered scored-term sorter.

  The concrete score and emitted-byte encodings are modeled by natural score
  ranks and byte lists. The original input position is the final stable key.
  A failed precharge returns no transition: sorting must not drain or mutate
  the source vector until every comparison and output reservation succeeds.
*)

From Stdlib Require Import List PeanoNat.
Import ListNotations.

Module ScoredTermTieOrder.

Fixpoint byte_compare (left right : list nat) : comparison :=
  match left, right with
  | [], [] => Eq
  | [], _ :: _ => Lt
  | _ :: _, [] => Gt
  | l :: ls, r :: rs =>
      match Nat.compare l r with
      | Eq => byte_compare ls rs
      | order => order
      end
  end.

Record Key := {
  score : nat;
  emitted : list nat;
  original_index : nat
}.

Definition key_compare (left right : Key) : comparison :=
  match Nat.compare (score left) (score right) with
  | Eq =>
      match byte_compare (emitted left) (emitted right) with
      | Eq => Nat.compare (original_index left) (original_index right)
      | order => order
      end
  | order => order
  end.

Theorem strict_score_dominates : forall left right,
  score left < score right -> key_compare left right = Lt.
Proof.
  intros left right H.
  unfold key_compare.
  apply Nat.compare_lt_iff in H.
  now rewrite H.
Qed.

Theorem equal_score_uses_emitted_bytes : forall left right,
  score left = score right ->
  byte_compare (emitted left) (emitted right) <> Eq ->
  key_compare left right = byte_compare (emitted left) (emitted right).
Proof.
  intros left right Hscore Hbytes.
  unfold key_compare.
  rewrite Hscore, Nat.compare_refl.
  destruct (byte_compare (emitted left) (emitted right));
    congruence.
Qed.

Theorem equal_score_and_bytes_preserve_input_order : forall left right,
  score left = score right ->
  byte_compare (emitted left) (emitted right) = Eq ->
  key_compare left right = Nat.compare (original_index left) (original_index right).
Proof.
  intros left right Hscore Hbytes.
  unfold key_compare.
  now rewrite Hscore, Nat.compare_refl, Hbytes.
Qed.

Theorem comparison_is_total : forall left right,
  key_compare left right = Lt \/ key_compare left right = Eq \/
  key_compare left right = Gt.
Proof.
  intros left right.
  destruct (key_compare left right); auto.
Qed.

Theorem distinct_input_positions_never_tie : forall left right,
  original_index left <> original_index right ->
  key_compare left right <> Eq.
Proof.
  intros left right Hindex Hcompare.
  unfold key_compare in Hcompare.
  destruct (Nat.compare (score left) (score right)); try discriminate.
  destruct (byte_compare (emitted left) (emitted right)); try discriminate.
  apply Nat.compare_eq_iff in Hcompare.
  contradiction.
Qed.

Definition precharged_compare (allowed : bool) (source : list nat)
    (left right : Key) : option (comparison * list nat) :=
  if allowed then Some (key_compare left right, source) else None.

Theorem refusal_precedes_source_mutation : forall source left right,
  precharged_compare false source left right = None.
Proof. reflexivity. Qed.

Theorem accepted_comparison_preserves_source : forall source left right order after,
  precharged_compare true source left right = Some (order, after) ->
  order = key_compare left right /\ after = source.
Proof.
  intros source left right order after H.
  inversion H. auto.
Qed.

End ScoredTermTieOrder.
