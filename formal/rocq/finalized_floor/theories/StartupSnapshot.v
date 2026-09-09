From Stdlib Require Import Lists.List Arith Lia.
Import ListNotations.

Section Snapshot.
Context {A : Type}.
Variable readable : A -> bool.

Theorem selected_hash_came_from_snapshot : forall snapshot key,
  In key (filter readable snapshot) -> In key snapshot.
Proof. intros. apply filter_In in H. tauto. Qed.

Theorem selected_count_is_bounded : forall snapshot,
  length (filter readable snapshot) <= length snapshot.
Proof.
  induction snapshot; simpl; [lia|].
  destruct (readable a); simpl; lia.
Qed.

Theorem remaining_cursor_bound : forall (snapshot : list A) prefix suffix,
  snapshot = prefix ++ suffix -> length suffix <= length snapshot.
Proof. intros. subst. rewrite app_length. lia. Qed.

Theorem each_cursor_step_reduces_work : forall (key : A) suffix,
  length suffix < length (key :: suffix).
Proof. simpl. intros. lia. Qed.

Theorem cursor_preserves_remaining_sequence : forall (prefix suffix : list A) key,
  prefix ++ key :: suffix = (prefix ++ [key]) ++ suffix.
Proof. intros. rewrite <- app_assoc. reflexivity. Qed.

Theorem unique_snapshot_is_not_visited_twice : forall (prefix suffix : list A) key,
  NoDup (prefix ++ key :: suffix) -> ~ In key prefix /\ ~ In key suffix.
Proof.
  induction prefix as [|head tail IH]; intros suffix key H; simpl in *.
  - inversion H; subst. split; [tauto|assumption].
  - inversion H; subst. destruct (IH suffix key H3) as [P S].
    split; [|exact S]. intros [E|E]; [subst; apply H2; apply in_or_app; right; simpl; auto|auto].
Qed.
End Snapshot.

Print Assumptions selected_hash_came_from_snapshot.
Print Assumptions selected_count_is_bounded.
Print Assumptions remaining_cursor_bound.
Print Assumptions each_cursor_step_reduces_work.
Print Assumptions cursor_preserves_remaining_sequence.
Print Assumptions unique_snapshot_is_not_visited_twice.
