From Coq Require Import List Arith Bool Lia Permutation.
Import ListNotations.

Fixpoint record_sum (weight : nat -> nat) (records : list nat) : nat :=
  match records with [] => 0 | key :: tail => weight key + record_sum weight tail end.

Fixpoint remove_once (key : nat) (records : list nat) : list nat :=
  match records with
  | [] => []
  | head :: tail => if Nat.eq_dec key head then tail else head :: remove_once key tail
  end.

Lemma remove_once_keeps_other : forall key other records,
  key <> other -> In other records -> In other (remove_once key records).
Proof.
  intros key other records Hneq. induction records as [|head tail IH]; simpl; intros Hin.
  - contradiction.
  - destruct (Nat.eq_dec key head) as [Heq|Heq].
    + subst head. destruct Hin as [Hin|Hin]; [congruence|exact Hin].
    + simpl. destruct Hin as [Hin|Hin]; [left; exact Hin|right; apply IH; exact Hin].
Qed.

Lemma remove_once_sum : forall weight key records,
  In key records -> weight key + record_sum weight (remove_once key records) = record_sum weight records.
Proof.
  intros weight key records. induction records as [|head tail IH]; simpl; intros Hin.
  - contradiction.
  - destruct (Nat.eq_dec key head) as [Heq|Heq].
    + subst. reflexivity.
    + simpl. destruct Hin as [Hin|Hin]; [congruence|]. specialize (IH Hin). lia.
Qed.

Theorem display_matched_weight_bounded : forall weight records roster,
  NoDup records -> incl records roster -> record_sum weight records <= record_sum weight roster.
Proof.
  intros weight records roster Hnodup. revert roster.
  induction Hnodup as [|key tail Hnotin Hnodup IH]; intros roster Hincl; simpl.
  - lia.
  - assert (Hkey : In key roster) by (apply Hincl; simpl; auto).
    assert (Htail : incl tail (remove_once key roster)).
    { intros other Hother. apply remove_once_keeps_other.
      - intro Heq. subst other. contradiction.
      - apply Hincl. simpl. auto. }
    specialize (IH _ Htail). pose proof (remove_once_sum weight key roster Hkey). lia.
Qed.

Theorem display_record_multiplicity : forall weight left right,
  Permutation left right -> record_sum weight left = record_sum weight right.
Proof.
  intros weight left right Hperm. induction Hperm; simpl; lia.
Qed.

Theorem display_duplicate_records_add_duplicate_terms : forall weight key,
  record_sum weight [key;key] = weight key + weight key.
Proof. intros. simpl. lia. Qed.

Definition checked_sum (bound : nat) (weight : nat -> nat) (records : list nat) : option nat :=
  if record_sum weight records <=? bound then Some (record_sum weight records) else None.

Theorem display_weight_sum_checked : forall bound weight records result,
  checked_sum bound weight records = Some result -> result = record_sum weight records /\ result <= bound.
Proof.
  intros bound weight records result. unfold checked_sum.
  destruct (record_sum weight records <=? bound) eqn:Hle; intros H; inversion H; subst.
  split; [reflexivity|]. apply Nat.leb_le. exact Hle.
Qed.

Theorem display_overflow_refuses : forall bound weight records,
  bound < record_sum weight records -> checked_sum bound weight records = None.
Proof.
  intros bound weight records H. unfold checked_sum.
  assert (record_sum weight records <=? bound = false) by (apply Nat.leb_gt; exact H).
  rewrite H0. reflexivity.
Qed.

Definition display_value (captured ready : bool) (bits : nat) : option nat :=
  if captured && ready then Some bits else None.

Theorem display_refusal_has_no_value : forall captured ready bits,
  captured = false \/ ready = false -> display_value captured ready bits = None.
Proof.
  intros captured ready bits H. destruct H as [H|H].
  - subst captured. reflexivity.
  - subst ready. unfold display_value. destruct captured; reflexivity.
Qed.

Definition bound_display (input_digest value_digest bits : nat) : option nat :=
  if input_digest =? value_digest then Some bits else None.

Theorem display_requires_equal_digest : forall input_digest value_digest bits value,
  bound_display input_digest value_digest bits = Some value -> input_digest = value_digest.
Proof.
  intros input_digest value_digest bits value. unfold bound_display.
  destruct (input_digest =? value_digest) eqn:Heq; intros H; [apply Nat.eqb_eq; exact Heq|discriminate].
Qed.
