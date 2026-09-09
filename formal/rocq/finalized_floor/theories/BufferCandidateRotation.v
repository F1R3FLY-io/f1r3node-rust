From Stdlib Require Import Arith Bool Lia Lists.List.
Import ListNotations.

Fixpoint rank (target : nat) (queue : list nat) : nat :=
  match queue with
  | [] => 0
  | head :: rest => if Nat.eq_dec target head then 0 else S (rank target rest)
  end.

Definition insert (key : nat) (queue : list nat) : list nat :=
  if in_dec Nat.eq_dec key queue then queue else queue ++ [key].

Definition remove (key : nat) (queue : list nat) : list nat :=
  filter (fun other => negb (Nat.eqb other key)) queue.

Theorem rank_is_less_than_length_for_a_present_identity : forall queue target,
  In target queue -> rank target queue < length queue.
Proof.
  induction queue as [|head rest IH]; intros target H; simpl in *; try contradiction.
  destruct (Nat.eq_dec target head); try lia.
  destruct H as [H|H]; [congruence|]. specialize (IH target H). lia.
Qed.

Theorem tail_arrivals_cannot_overtake_present_identities : forall queue target suffix,
  In target queue -> rank target (queue ++ suffix) = rank target queue.
Proof.
  induction queue as [|head rest IH]; intros target suffix H; simpl in *; try contradiction.
  destruct (Nat.eq_dec target head); auto.
  destruct H as [H|H]; [congruence|]. now rewrite IH by assumption.
Qed.

Theorem insertion_preserves_the_rank_of_every_present_identity : forall queue target key,
  In target queue -> rank target (insert key queue) = rank target queue.
Proof.
  intros queue target key H. unfold insert.
  destruct (in_dec Nat.eq_dec key queue); auto.
  now apply tail_arrivals_cannot_overtake_present_identities.
Qed.

Theorem duplicate_insertion_preserves_the_complete_order : forall queue key,
  In key queue -> insert key queue = queue.
Proof. intros. unfold insert. destruct (in_dec Nat.eq_dec key queue); congruence. Qed.

Theorem removing_another_identity_cannot_increase_rank : forall queue target key,
  target <> key -> rank target (remove key queue) <= rank target queue.
Proof.
  induction queue as [|head rest IH]; intros target key Hneq; unfold remove in *; simpl in *.
  - lia.
  - specialize (IH target key Hneq).
    destruct (head =? key) eqn:E; simpl.
    + apply Nat.eqb_eq in E. subst head. destruct (Nat.eq_dec target key); [congruence|lia].
    + destruct (Nat.eq_dec target head); simpl.
      * destruct (Nat.eq_dec target head); [lia|congruence].
      * destruct (Nat.eq_dec target head); [congruence|lia].
Qed.

Theorem examining_another_head_reduces_rank_by_one : forall rest target head,
  In target rest -> target <> head ->
  S (rank target (rest ++ [head])) = rank target (head :: rest).
Proof.
  intros rest target head Hin Hneq.
  rewrite tail_arrivals_cannot_overtake_present_identities by assumption.
  simpl. destruct (Nat.eq_dec target head); congruence.
Qed.

Inductive bypass_step (target : nat) : list nat -> list nat -> nat -> Prop :=
| BypassInsert : forall queue key,
    In target queue -> bypass_step target queue (insert key queue) 0
| BypassRemove : forall queue key,
    target <> key -> bypass_step target queue (remove key queue) 0
| BypassExamine : forall head rest,
    In target rest -> target <> head ->
    bypass_step target (head :: rest) (rest ++ [head]) 1.

Theorem bypass_step_spends_only_the_initial_rank : forall target before after cost,
  bypass_step target before after cost -> rank target after + cost <= rank target before.
Proof.
  intros target before after cost H. destruct H.
  - rewrite insertion_preserves_the_rank_of_every_present_identity by assumption. lia.
  - pose proof (removing_another_identity_cannot_increase_rank queue target key H). lia.
  - pose proof (examining_another_head_reduces_rank_by_one rest target head H H0). lia.
Qed.

Inductive bypass_history (target : nat) : list nat -> list nat -> nat -> Prop :=
| BypassDone : forall queue, bypass_history target queue queue 0
| BypassMore : forall before middle after cost total,
    bypass_step target before middle cost ->
    bypass_history target middle after total ->
    bypass_history target before after (cost + total).

Theorem arbitrary_arrivals_and_removals_preserve_the_examination_bound :
  forall target before after count,
    bypass_history target before after count -> rank target after + count <= rank target before.
Proof.
  intros target before after count H. induction H.
  - lia.
  - apply bypass_step_spends_only_the_initial_rank in H. lia.
Qed.

Theorem no_present_identity_can_be_bypassed_for_a_complete_initial_rotation :
  forall target before after count,
    In target before -> bypass_history target before after count -> count < length before.
Proof.
  intros target before after count Hin Hrun.
  apply arbitrary_arrivals_and_removals_preserve_the_examination_bound in Hrun.
  apply rank_is_less_than_length_for_a_present_identity in Hin. lia.
Qed.

Print Assumptions rank_is_less_than_length_for_a_present_identity.
Print Assumptions tail_arrivals_cannot_overtake_present_identities.
Print Assumptions insertion_preserves_the_rank_of_every_present_identity.
Print Assumptions duplicate_insertion_preserves_the_complete_order.
Print Assumptions removing_another_identity_cannot_increase_rank.
Print Assumptions examining_another_head_reduces_rank_by_one.
Print Assumptions bypass_step_spends_only_the_initial_rank.
Print Assumptions arbitrary_arrivals_and_removals_preserve_the_examination_bound.
Print Assumptions no_present_identity_can_be_bypassed_for_a_complete_initial_rotation.
