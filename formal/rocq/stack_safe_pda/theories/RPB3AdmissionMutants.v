From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import StackSafePDA RPB3Admission.

Import ListNotations.
Set Implicit Arguments.

(** Independent red controls for the RPB3 admission kernel.  Each mutant
    changes one causal rule and is refuted by a closed finite witness; these
    are not alternative implementations and are never used by the positive
    equivalence theorem. *)

Definition reject_label (label : nat) : bool := negb (label =? 0).

Definition mutant_omit_local (label : nat) (children : list bool) : bool :=
  forallb (fun value => value) children.

Definition mutant_local_validation (subject : @tree nat) : bool :=
  @fold_tree nat bool mutant_omit_local subject.

Definition valid_local_validation (subject : @tree nat) : bool :=
  @fold_tree nat bool
    (fun label children => reject_label label && forallb (fun value => value) children)
    subject.

Definition invalid_root : @tree nat := Node 0 FNil.
Definition valid_root : @tree nat := Node 1 FNil.

Theorem mutant_omit_locality_is_refuted :
  mutant_local_validation invalid_root = true /\
  valid_local_validation invalid_root = false.
Proof. split; reflexivity. Qed.

Theorem local_validation_accepts_valid_root :
  valid_local_validation valid_root = true.
Proof. reflexivity. Qed.

Definition mutant_accepts_trailing (input : list nat) : bool :=
  match input with
  | _ :: _ => true
  | [] => false
  end.

Definition canonical_exact (input : list nat) : bool :=
  match input with
  | [42] => true
  | _ => false
  end.

Theorem mutant_trailing_acceptance_is_refuted :
  mutant_accepts_trailing [42; 7] = true /\
  canonical_exact [42; 7] = false.
Proof. split; reflexivity. Qed.

Print Assumptions mutant_omit_locality_is_refuted.
Print Assumptions mutant_trailing_acceptance_is_refuted.
