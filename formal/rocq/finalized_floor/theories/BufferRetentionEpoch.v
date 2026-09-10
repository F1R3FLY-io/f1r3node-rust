From Stdlib Require Import Arith Bool Lia Lists.List.
Import ListNotations.

Definition age (seen : option nat) (now : nat) : nat :=
  match seen with Some first => now - first | None => 0 end.

Theorem seeded_age_reaches_every_positive_deadline : forall first now ttl,
  first + ttl <= now -> ttl <= age (Some first) now.
Proof. intros. unfold age. lia. Qed.

Theorem missing_age_never_reaches_a_positive_deadline : forall now ttl,
  ttl > 0 -> age None now < ttl.
Proof. intros. unfold age. lia. Qed.

Theorem seeded_age_never_decreases_with_time : forall first before after,
  before <= after -> age (Some first) before <= age (Some first) after.
Proof. intros. unfold age. lia. Qed.

Theorem restart_resets_age_without_changing_identity : forall now,
  age (Some now) now = 0.
Proof. intros. unfold age. lia. Qed.

Definition incoming_count (flags : list bool) : nat :=
  length (filter (fun flag => flag) flags).

Definition ready_count (flags : list bool) : nat :=
  length (filter negb flags).

Theorem disjoint_degree_partition_counts_every_identity : forall flags,
  incoming_count flags + ready_count flags = length flags.
Proof.
  induction flags as [|flag rest IH]; unfold incoming_count, ready_count in *; simpl in *.
  - reflexivity.
  - destruct flag; simpl; lia.
Qed.

Theorem isolated_identity_counts_once : forall flags,
  incoming_count (false :: flags) + ready_count (false :: flags) =
  S (incoming_count flags + ready_count flags).
Proof. intros. unfold incoming_count, ready_count. simpl. lia. Qed.

Print Assumptions seeded_age_reaches_every_positive_deadline.
Print Assumptions missing_age_never_reaches_a_positive_deadline.
Print Assumptions seeded_age_never_decreases_with_time.
Print Assumptions restart_resets_age_without_changing_identity.
Print Assumptions disjoint_degree_partition_counts_every_identity.
Print Assumptions isolated_identity_counts_once.
