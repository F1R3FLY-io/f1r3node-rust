From Stdlib Require Import Lists.List Lists.ListDec Bool.Bool Arith.PeanoNat Lia Sorting.Permutation.
Import ListNotations.

Record native_attempt := {
  attempt_occurrence : nat;
  attempt_source : nat;
  attempt_cost : nat;
  attempt_granted : bool
}.

Definition attempt_debit row := if attempt_granted row then attempt_cost row else 0.

Fixpoint accepted_usage rows :=
  match rows with
  | [] => 0
  | row :: rest => attempt_debit row + accepted_usage rest
  end.

Fixpoint check_attempt_prefix bound used rows :=
  match rows with
  | [] => Nat.leb used bound
  | row :: rest =>
      Bool.eqb (attempt_granted row) (Nat.leb (used + attempt_cost row) bound) &&
      check_attempt_prefix bound (used + attempt_debit row) rest
  end.

Inductive valid_attempt_prefix (bound : nat) : nat -> list native_attempt -> Prop :=
| ValidAttemptEnd : forall used, used <= bound -> valid_attempt_prefix bound used []
| ValidAttemptStep : forall used row rest,
    attempt_granted row = Nat.leb (used + attempt_cost row) bound ->
    valid_attempt_prefix bound (used + attempt_debit row) rest ->
    valid_attempt_prefix bound used (row :: rest).

Theorem check_attempt_prefix_exact : forall bound used rows,
  check_attempt_prefix bound used rows = true <-> valid_attempt_prefix bound used rows.
Proof.
  intros bound used rows. revert used.
  induction rows as [|row rest IH]; intros used; simpl.
  - rewrite Nat.leb_le. split; [constructor; assumption|]. intros H. inversion H. assumption.
  - rewrite andb_true_iff, Bool.eqb_true_iff, IH. split.
    + intros [decision valid]. now constructor.
    + intros H. inversion H. auto.
Qed.

Theorem checked_attempt_usage_bounded : forall bound used rows,
  check_attempt_prefix bound used rows = true -> used + accepted_usage rows <= bound.
Proof.
  intros bound used rows. revert used.
  induction rows as [|row rest IH]; intros used valid; simpl in *.
  - apply Nat.leb_le in valid. lia.
  - apply andb_true_iff in valid. destruct valid as [_ valid].
    apply IH in valid. lia.
Qed.

Theorem checked_attempt_decision_exact : forall bound used row rest,
  check_attempt_prefix bound used (row :: rest) = true ->
  (attempt_granted row = true <-> used + attempt_cost row <= bound).
Proof.
  intros bound used row rest checked.
  apply check_attempt_prefix_exact in checked. inversion checked as [|u r rs decision valid]; subst.
  rewrite decision. apply Nat.leb_le.
Qed.

Theorem checked_denial_is_unaffordable : forall bound used row rest,
  check_attempt_prefix bound used (row :: rest) = true ->
  attempt_granted row = false -> bound < used + attempt_cost row.
Proof.
  intros bound used row rest checked denied.
  apply check_attempt_prefix_exact in checked. inversion checked as [|u r rs decision valid]; subst.
  rewrite denied in decision. symmetry in decision. now apply Nat.leb_gt in decision.
Qed.

Theorem checked_zero_cost_cannot_be_denied : forall bound used row rest,
  check_attempt_prefix bound used (row :: rest) = true ->
  attempt_cost row = 0 -> attempt_granted row = true.
Proof.
  intros bound used row rest checked zero.
  pose proof (checked_attempt_usage_bounded bound used (row :: rest) checked) as ceiling.
  apply (proj2 (checked_attempt_decision_exact bound used row rest checked)). lia.
Qed.

Theorem accepted_usage_permutation : forall left right,
  Permutation left right -> accepted_usage left = accepted_usage right.
Proof.
  intros left right permutation. induction permutation; simpl; lia.
Qed.

Theorem checked_replay_permutation_bounded : forall bound used original replay,
  check_attempt_prefix bound used original = true ->
  Permutation original replay -> used + accepted_usage replay <= bound.
Proof.
  intros bound used original replay checked permutation.
  rewrite <- (accepted_usage_permutation original replay permutation).
  now apply checked_attempt_usage_bounded.
Qed.

Theorem checked_replay_every_prefix_bounded : forall bound used original prefix suffix,
  check_attempt_prefix bound used original = true ->
  Permutation original (prefix ++ suffix) -> used + accepted_usage prefix <= bound.
Proof.
  assert (append_usage : forall left right,
    accepted_usage (left ++ right) = accepted_usage left + accepted_usage right).
  { intros left right. induction left; simpl; lia. }
  intros bound used original prefix suffix checked permutation.
  pose proof (checked_replay_permutation_bounded bound used original (prefix ++ suffix)
    checked permutation) as ceiling.
  rewrite append_usage in ceiling. lia.
Qed.

Definition check_attempt_measurements (measure : nat -> nat) rows :=
  forallb (fun row => Nat.eqb (attempt_cost row) (measure (attempt_source row))) rows.

Theorem checked_measurements_exact : forall measure rows,
  check_attempt_measurements measure rows = true <->
  forall row, In row rows -> attempt_cost row = measure (attempt_source row).
Proof.
  intros measure rows. unfold check_attempt_measurements.
  rewrite forallb_forall. split; intros H row included; specialize (H row included).
  - now apply Nat.eqb_eq in H.
  - now apply Nat.eqb_eq.
Qed.

Definition check_attempt_occurrences rows :=
  if NoDup_dec Nat.eq_dec (map attempt_occurrence rows) then true else false.

Theorem checked_occurrences_unique : forall rows,
  check_attempt_occurrences rows = true <-> NoDup (map attempt_occurrence rows).
Proof.
  intros rows. unfold check_attempt_occurrences.
  destruct (NoDup_dec Nat.eq_dec (map attempt_occurrence rows)); split; auto; discriminate || contradiction.
Qed.

Theorem replay_keeps_occurrence_multiplicity : forall original replay,
  Permutation original replay ->
  forall occurrence,
    count_occ Nat.eq_dec (map attempt_occurrence original) occurrence =
    count_occ Nat.eq_dec (map attempt_occurrence replay) occurrence.
Proof.
  intros original replay permutation occurrence.
  apply (proj1 (Permutation_count_occ Nat.eq_dec _ _)).
  now apply Permutation_map.
Qed.

Theorem forged_affordable_denial_rejected : forall bound used row rest,
  attempt_granted row = false -> used + attempt_cost row <= bound ->
  check_attempt_prefix bound used (row :: rest) = false.
Proof.
  intros bound used row rest denied affordable.
  destruct (check_attempt_prefix bound used (row :: rest)) eqn:checked; [|reflexivity].
  pose proof (checked_denial_is_unaffordable bound used row rest checked denied). lia.
Qed.

Theorem oversized_charge_cannot_be_accepted : forall word_max bound used row rest,
  bound <= word_max -> word_max < attempt_cost row ->
  check_attempt_prefix bound used (row :: rest) = true -> attempt_granted row = false.
Proof.
  intros word_max bound used row rest bound_fits oversized checked.
  destruct (attempt_granted row) eqn:granted; [|reflexivity].
  apply (proj1 (checked_attempt_decision_exact bound used row rest checked)) in granted. lia.
Qed.
