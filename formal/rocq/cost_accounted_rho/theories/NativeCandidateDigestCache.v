From Stdlib Require Import Lists.List Arith.PeanoNat.
Import ListNotations.

Section CandidateOrdering.
Variable Candidate : Type.
Variable digest : Candidate -> nat.

Record indexed_candidate := {
  indexed_value : Candidate;
  original_index : nat
}.

Record cached_candidate := {
  cached_value : Candidate;
  cached_index : nat;
  cached_digest : nat
}.

Definition cache (candidate : indexed_candidate) : cached_candidate :=
  {| cached_value := indexed_value candidate;
     cached_index := original_index candidate;
     cached_digest := digest (indexed_value candidate) |}.

Definition before (left_digest left_index right_digest right_index : nat) : bool :=
  Nat.ltb left_digest right_digest ||
    (Nat.eqb left_digest right_digest && Nat.leb left_index right_index).

Definition original_before (left right : indexed_candidate) : bool :=
  before (digest (indexed_value left)) (original_index left)
    (digest (indexed_value right)) (original_index right).

Definition cached_before (left right : cached_candidate) : bool :=
  before (cached_digest left) (cached_index left)
    (cached_digest right) (cached_index right).

Theorem cached_comparison_is_original_comparison : forall left right,
  cached_before (cache left) (cache right) = original_before left right.
Proof. reflexivity. Qed.

Fixpoint insert_original candidate ordered :=
  match ordered with
  | [] => [candidate]
  | head :: tail =>
      if original_before candidate head
      then candidate :: head :: tail
      else head :: insert_original candidate tail
  end.

Fixpoint insert_cached candidate ordered :=
  match ordered with
  | [] => [candidate]
  | head :: tail =>
      if cached_before candidate head
      then candidate :: head :: tail
      else head :: insert_cached candidate tail
  end.

Lemma cache_commutes_with_insert : forall candidate ordered,
  insert_cached (cache candidate) (map cache ordered) =
    map cache (insert_original candidate ordered).
Proof.
  intros candidate ordered. induction ordered as [|head tail IH]; simpl.
  - reflexivity.
  - rewrite cached_comparison_is_original_comparison.
    destruct (original_before candidate head); simpl; auto.
    now rewrite IH.
Qed.

Fixpoint original_sort candidates :=
  match candidates with
  | [] => []
  | candidate :: rest => insert_original candidate (original_sort rest)
  end.

Fixpoint cached_sort candidates :=
  match candidates with
  | [] => []
  | candidate :: rest => insert_cached candidate (cached_sort rest)
  end.

Theorem caching_preserves_candidate_order : forall candidates,
  cached_sort (map cache candidates) = map cache (original_sort candidates).
Proof.
  induction candidates as [|candidate rest IH]; simpl.
  - reflexivity.
  - rewrite IH. apply cache_commutes_with_insert.
Qed.
End CandidateOrdering.

Print Assumptions cached_comparison_is_original_comparison.
Print Assumptions caching_preserves_candidate_order.
