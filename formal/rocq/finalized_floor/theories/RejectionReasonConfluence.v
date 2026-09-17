From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

Inductive rejection_reason : Type :=
| Unspecified
| CollateralChainDrop
| MergeConflict
| DuplicateOccurrence.

Definition canonical_reason_join
  (left right : rejection_reason) : rejection_reason :=
  match left, right with
  | DuplicateOccurrence, _ | _, DuplicateOccurrence => DuplicateOccurrence
  | MergeConflict, _ | _, MergeConflict => MergeConflict
  | CollateralChainDrop, _ | _, CollateralChainDrop => CollateralChainDrop
  | Unspecified, Unspecified => Unspecified
  end.

Theorem canonical_reason_join_commutative :
  forall left right,
    canonical_reason_join left right = canonical_reason_join right left.
Proof.
  intros left right.
  destruct left, right; reflexivity.
Qed.

Theorem canonical_reason_join_associative :
  forall left middle right,
    canonical_reason_join (canonical_reason_join left middle) right =
    canonical_reason_join left (canonical_reason_join middle right).
Proof.
  intros left middle right.
  destruct left, middle, right; reflexivity.
Qed.

Theorem canonical_reason_join_idempotent :
  forall reason,
    canonical_reason_join reason reason = reason.
Proof.
  intros reason.
  destruct reason; reflexivity.
Qed.

Theorem duplicate_reason_dominates :
  forall reason,
    canonical_reason_join DuplicateOccurrence reason = DuplicateOccurrence.
Proof.
  intros reason.
  destruct reason; reflexivity.
Qed.

Theorem merge_reason_dominates_collateral :
  canonical_reason_join MergeConflict CollateralChainDrop = MergeConflict.
Proof.
  reflexivity.
Qed.

Fixpoint fold_rejection_reasons
  (reasons : list rejection_reason) : rejection_reason :=
  match reasons with
  | [] => Unspecified
  | reason :: remaining =>
      canonical_reason_join reason (fold_rejection_reasons remaining)
  end.

Theorem fold_rejection_reasons_permutation :
  forall left right,
    Permutation left right ->
    fold_rejection_reasons left = fold_rejection_reasons right.
Proof.
  intros left right Hperm.
  induction Hperm.
  - reflexivity.
  - simpl. now rewrite IHHperm.
  - simpl. destruct x, y, (fold_rejection_reasons l); reflexivity.
  - now rewrite IHHperm1, IHHperm2.
Qed.
