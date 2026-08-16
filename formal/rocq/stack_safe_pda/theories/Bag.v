From Stdlib Require Import Arith PeanoNat Lia FunctionalExtensionality.

Set Implicit Arguments.

(** An exact-key Bag is a finite-support function in the implementation.  The
    algebraic laws do not depend on the concrete finite-support carrier, so the
    proof model uses a total function whose zero cells are absent.  PathMap's
    prefix relation never appears in these definitions: different byte paths
    denote independent cells. *)
Section ExactKeyBag.

Context {Key : Type}.
Variable key_eq_dec : forall left right : Key, {left = right} + {left <> right}.

Definition bag := Key -> nat.
Definition empty : bag := fun _ => 0.
Definition sum (left right : bag) : bag := fun key => left key + right key.
Definition join (left right : bag) : bag := fun key => Nat.max (left key) (right key).
Definition meet (left right : bag) : bag := fun key => Nat.min (left key) (right key).
Definition subtract (left right : bag) : bag := fun key => left key - right key.
Definition subbag (left right : bag) : Prop := forall key, left key <= right key.

Definition set_count (selected : Key) (count : nat) (subject : bag) : bag :=
  fun candidate => if key_eq_dec candidate selected then count else subject candidate.

Theorem set_count_is_exact :
  forall subject selected other count,
    other <> selected -> set_count selected count subject other = subject other.
Proof.
  intros subject selected other count different.
  unfold set_count. destruct (key_eq_dec other selected); congruence.
Qed.

Theorem sum_empty_identity :
  forall subject, sum empty subject = subject /\ sum subject empty = subject.
Proof.
  intro subject. split; apply functional_extensionality; intro key;
    unfold sum, empty; lia.
Qed.

Theorem sum_commutative : forall left right, sum left right = sum right left.
Proof.
  intros left right. apply functional_extensionality. intro key.
  unfold sum. lia.
Qed.

Theorem sum_associative :
  forall first second third,
    sum (sum first second) third = sum first (sum second third).
Proof.
  intros first second third. apply functional_extensionality. intro key.
  unfold sum. lia.
Qed.

Theorem join_commutative : forall left right, join left right = join right left.
Proof.
  intros left right. apply functional_extensionality. intro key.
  unfold join. apply Nat.max_comm.
Qed.

Theorem join_associative :
  forall first second third,
    join (join first second) third = join first (join second third).
Proof.
  intros first second third. apply functional_extensionality. intro key.
  unfold join. symmetry. apply Nat.max_assoc.
Qed.

Theorem join_idempotent : forall subject, join subject subject = subject.
Proof.
  intro subject. apply functional_extensionality. intro key.
  unfold join. apply Nat.max_id.
Qed.

Theorem join_empty_identity :
  forall subject, join empty subject = subject /\ join subject empty = subject.
Proof.
  intro subject. split; apply functional_extensionality; intro key;
    unfold join, empty; lia.
Qed.

Theorem meet_commutative : forall left right, meet left right = meet right left.
Proof.
  intros left right. apply functional_extensionality. intro key.
  unfold meet. apply Nat.min_comm.
Qed.

Theorem meet_associative :
  forall first second third,
    meet (meet first second) third = meet first (meet second third).
Proof.
  intros first second third. apply functional_extensionality. intro key.
  unfold meet. symmetry. apply Nat.min_assoc.
Qed.

Theorem meet_idempotent : forall subject, meet subject subject = subject.
Proof.
  intro subject. apply functional_extensionality. intro key.
  unfold meet. apply Nat.min_id.
Qed.

Theorem meet_empty_absorbing :
  forall subject, meet empty subject = empty /\ meet subject empty = empty.
Proof.
  intro subject. split; apply functional_extensionality; intro key;
    unfold meet, empty; lia.
Qed.

Theorem join_absorbs_meet :
  forall left right, join left (meet left right) = left.
Proof.
  intros left right. apply functional_extensionality. intro key.
  unfold join, meet. apply Nat.min_max_absorption.
Qed.

Theorem meet_absorbs_join :
  forall left right, meet left (join left right) = left.
Proof.
  intros left right. apply functional_extensionality. intro key.
  unfold join, meet. apply Nat.max_min_absorption.
Qed.

Theorem meet_distributes_over_join :
  forall first second third,
    meet first (join second third) =
    join (meet first second) (meet first third).
Proof.
  intros first second third. apply functional_extensionality. intro key.
  unfold join, meet. apply Nat.min_max_distr.
Qed.

Theorem join_distributes_over_meet :
  forall first second third,
    join first (meet second third) =
    meet (join first second) (join first third).
Proof.
  intros first second third. apply functional_extensionality. intro key.
  unfold join, meet. apply Nat.max_min_distr.
Qed.

Theorem subtraction_conservation :
  forall left right, sum (subtract left right) (meet left right) = left.
Proof.
  intros left right. apply functional_extensionality. intro key.
  unfold sum, subtract, meet.
  destruct (Nat.le_ge_cases (right key) (left key)).
  - rewrite Nat.min_r by assumption. lia.
  - rewrite Nat.min_l by assumption. lia.
Qed.

Theorem subtract_self_empty : forall subject, subtract subject subject = empty.
Proof.
  intro subject. apply functional_extensionality. intro key.
  unfold subtract, empty. lia.
Qed.

Theorem meet_is_subbag_left : forall left right, subbag (meet left right) left.
Proof.
  intros left right key. unfold meet. apply Nat.le_min_l.
Qed.

Theorem meet_is_subbag_right : forall left right, subbag (meet left right) right.
Proof.
  intros left right key. unfold meet. apply Nat.le_min_r.
Qed.

(** A normalized delta has disjoint addition and removal support.  This is the
    invariant maintained by `BagDelta::record_change`. *)
Definition delta_normalized (additions removals : bag) : Prop :=
  forall key, additions key = 0 \/ removals key = 0.

Definition delta_applicable (subject removals : bag) : Prop :=
  forall key, removals key <= subject key.

Definition apply_delta (subject additions removals : bag) : bag :=
  sum (subtract subject removals) additions.

Theorem normalized_delta_reversible :
  forall subject additions removals,
    delta_normalized additions removals ->
    delta_applicable subject removals ->
    apply_delta (apply_delta subject additions removals) removals additions = subject.
Proof.
  intros subject additions removals normalized applicable.
  apply functional_extensionality. intro key.
  specialize (normalized key). specialize (applicable key).
  unfold apply_delta, sum, subtract.
  destruct normalized; lia.
Qed.

Definition delta_from_to_removals (source target : bag) : bag := subtract source target.
Definition delta_from_to_additions (source target : bag) : bag := subtract target source.

Theorem canonical_delta_is_normalized :
  forall source target,
    delta_normalized
      (delta_from_to_additions source target)
      (delta_from_to_removals source target).
Proof.
  intros source target key.
  unfold delta_from_to_additions, delta_from_to_removals, subtract.
  destruct (Nat.le_ge_cases (source key) (target key)); [right | left]; lia.
Qed.

Theorem canonical_delta_is_applicable :
  forall source target,
    delta_applicable source (delta_from_to_removals source target).
Proof.
  intros source target key.
  unfold delta_from_to_removals, subtract. lia.
Qed.

Theorem canonical_delta_reaches_target :
  forall source target,
    apply_delta
      source
      (delta_from_to_additions source target)
      (delta_from_to_removals source target) = target.
Proof.
  intros source target. apply functional_extensionality. intro key.
  unfold apply_delta, delta_from_to_additions, delta_from_to_removals,
    sum, subtract.
  lia.
Qed.

Corollary canonical_delta_round_trip :
  forall source target,
    apply_delta
      (apply_delta
        source
        (delta_from_to_additions source target)
        (delta_from_to_removals source target))
      (delta_from_to_removals source target)
      (delta_from_to_additions source target) = source.
Proof.
  intros source target.
  apply normalized_delta_reversible.
  - apply canonical_delta_is_normalized.
  - apply canonical_delta_is_applicable.
Qed.

End ExactKeyBag.
