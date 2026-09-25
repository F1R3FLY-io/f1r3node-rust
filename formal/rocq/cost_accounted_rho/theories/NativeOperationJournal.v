From Stdlib Require Import Lists.List Lists.ListDec Bool.Bool Arith.PeanoNat Lia.
Import ListNotations.

Definition journal_ready (completed : nat -> bool) (slot : nat -> nat) predecessors :=
  forallb (fun predecessor => completed (slot predecessor)) predecessors.

Theorem journal_ready_exact : forall completed slot predecessors,
  journal_ready completed slot predecessors = true <->
  forall predecessor, In predecessor predecessors -> completed (slot predecessor) = true.
Proof. intros. unfold journal_ready. apply forallb_forall. Qed.

Theorem journal_ready_preserves_independent_progress : forall completed slot,
  journal_ready completed slot [] = true.
Proof. reflexivity. Qed.

Theorem journal_ready_requires_recheck_after_restore : forall before after slot predecessors predecessor,
  journal_ready before slot predecessors = true ->
  In predecessor predecessors -> after (slot predecessor) = false ->
  journal_ready after slot predecessors = false.
Proof.
  intros before after slot predecessors predecessor ready member restored.
  destruct (journal_ready after slot predecessors) eqn:checked; auto.
  pose proof (proj1 (journal_ready_exact after slot predecessors) checked predecessor member).
  congruence.
Qed.

Theorem journal_ready_is_invariant_under_correct_slot_renaming : forall left right left_slot right_slot predecessors,
  (forall predecessor, In predecessor predecessors ->
     left (left_slot predecessor) = right (right_slot predecessor)) ->
  journal_ready left left_slot predecessors = journal_ready right right_slot predecessors.
Proof.
  intros left right left_slot right_slot predecessors equivalent.
  unfold journal_ready. induction predecessors as [|head tail IH]; simpl; auto.
  rewrite (equivalent head (or_introl eq_refl)).
  rewrite IH; auto. intros. apply equivalent. now right.
Qed.

Inductive journal_link :=
| JournalFresh (index : nat)
| JournalRetry (cut accepted : nat).

Definition journal_link_start link :=
  match link with JournalFresh i => i | JournalRetry cut _ => cut end.

Definition journal_link_end link :=
  match link with JournalFresh i => S i | JournalRetry cut _ => cut end.

Definition journal_contains lo hi link :=
  (lo <=? journal_link_start link) && (journal_link_end link <=? hi).

Definition journal_stage_order intro comm :=
  journal_link_end intro <=? journal_link_start comm.

Theorem journal_contains_exact : forall lo hi link,
  journal_contains lo hi link = true <->
  lo <= journal_link_start link /\ journal_link_end link <= hi.
Proof. intros. unfold journal_contains. now rewrite andb_true_iff, !Nat.leb_le. Qed.

Theorem journal_fresh_interval : forall lo hi i,
  journal_contains lo hi (JournalFresh i) = true <-> lo <= i /\ i < hi.
Proof. intros. rewrite journal_contains_exact. simpl. lia. Qed.

Theorem journal_retry_interval : forall lo hi cut accepted,
  journal_contains lo hi (JournalRetry cut accepted) = true <->
  lo <= cut /\ cut <= hi.
Proof. intros. rewrite journal_contains_exact. reflexivity. Qed.

Theorem journal_fresh_fresh_order : forall i j,
  journal_stage_order (JournalFresh i) (JournalFresh j) = true <-> i < j.
Proof. intros. change ((S i <=? j) = true <-> i < j). rewrite Nat.leb_le. lia. Qed.

Theorem journal_fresh_retry_order : forall i cut accepted,
  journal_stage_order (JournalFresh i) (JournalRetry cut accepted) = true <-> i < cut.
Proof. intros. change ((S i <=? cut) = true <-> i < cut). rewrite Nat.leb_le. lia. Qed.

Theorem journal_retry_fresh_order : forall cut accepted j,
  journal_stage_order (JournalRetry cut accepted) (JournalFresh j) = true <-> cut <= j.
Proof. intros. unfold journal_stage_order. simpl. apply Nat.leb_le. Qed.

Theorem journal_retry_retry_order : forall cut accepted next previous,
  journal_stage_order (JournalRetry cut accepted) (JournalRetry next previous) = true <->
  cut <= next.
Proof. intros. unfold journal_stage_order. simpl. apply Nat.leb_le. Qed.

Theorem journal_empty_retry_interval : forall cut accepted,
  journal_contains cut cut (JournalRetry cut accepted) = true.
Proof. intros. apply journal_contains_exact. simpl. lia. Qed.

Theorem journal_equal_retry_cuts : forall cut first second,
  journal_stage_order (JournalRetry cut first) (JournalRetry cut second) = true.
Proof. intros. unfold journal_stage_order. simpl. apply Nat.leb_refl. Qed.

Theorem journal_dependency_orders_links : forall lo hi next_lo next_hi left right,
  journal_contains lo hi left = true ->
  journal_contains next_lo next_hi right = true -> hi <= next_lo ->
  journal_stage_order left right = true.
Proof.
  intros lo hi next_lo next_hi left right included next_included edge.
  apply journal_contains_exact in included, next_included.
  unfold journal_stage_order. apply Nat.leb_le. lia.
Qed.

Theorem journal_dependency_orders_fresh : forall lo hi next_lo next_hi i j,
  journal_contains lo hi (JournalFresh i) = true ->
  journal_contains next_lo next_hi (JournalFresh j) = true -> hi <= next_lo -> i < j.
Proof.
  intros. apply journal_fresh_fresh_order.
  eapply journal_dependency_orders_links; eauto.
Qed.

Definition journal_retry_reference count cut accepted granted :=
  (accepted <? cut) && (cut <=? count) && granted.

Theorem journal_retry_reference_exact : forall count cut accepted granted,
  journal_retry_reference count cut accepted granted = true <->
  accepted < cut /\ cut <= count /\ granted = true.
Proof.
  intros. unfold journal_retry_reference.
  rewrite !andb_true_iff, Nat.ltb_lt, Nat.leb_le. tauto.
Qed.

Theorem journal_retry_reference_exists : forall count cut accepted granted,
  journal_retry_reference count cut accepted granted = true -> accepted < count.
Proof. intros. apply journal_retry_reference_exact in H. lia. Qed.

Definition journal_unique (links : list nat) :=
  if NoDup_dec Nat.eq_dec links then true else false.

Definition journal_exact_ownership total links :=
  journal_unique links && (length links =? total) &&
  forallb (fun index => index <? total) links.

Theorem journal_unique_exact : forall links,
  journal_unique links = true <-> NoDup links.
Proof.
  intros. unfold journal_unique. destruct (NoDup_dec Nat.eq_dec links).
  - split; auto.
  - split; [discriminate | contradiction].
Qed.

Theorem journal_exact_ownership_characterization : forall total links,
  journal_exact_ownership total links = true <->
  NoDup links /\ length links = total /\ (forall index, In index links -> index < total).
Proof.
  intros. unfold journal_exact_ownership.
  rewrite !andb_true_iff, journal_unique_exact, Nat.eqb_eq, forallb_forall.
  split.
  - intros [[unique count] bounded]. repeat split; auto.
    intros. apply Nat.ltb_lt. auto.
  - intros [unique [count bounded]]. repeat split; auto.
    intros. apply Nat.ltb_lt. auto.
Qed.

Theorem journal_exact_ownership_covers_every_publication : forall total links,
  journal_exact_ownership total links = true ->
  forall index, index < total -> In index links.
Proof.
  intros total links checked.
  apply journal_exact_ownership_characterization in checked.
  destruct checked as [unique [count bounded]].
  assert (included : incl links (seq 0 total)).
  { intros index present. apply in_seq. split; [lia | now apply bounded]. }
  assert (covered : incl (seq 0 total) links).
  { apply NoDup_length_incl; auto. rewrite length_seq, count. lia. }
  intros index below. apply covered. apply in_seq. lia.
Qed.

Theorem journal_duplicate_ownership_rejected : forall total links index,
  journal_exact_ownership total (index :: index :: links) = false.
Proof.
  intros. destruct (journal_exact_ownership total (index :: index :: links)) eqn:checked; auto.
  apply journal_exact_ownership_characterization in checked.
  destruct checked as [unique _]. inversion unique; subst.
  exfalso. apply H1. simpl. auto.
Qed.

Theorem journal_omitted_publication_rejected : forall total links index,
  index < total -> ~In index links -> journal_exact_ownership total links = false.
Proof.
  intros. destruct (journal_exact_ownership total links) eqn:checked; auto.
  exfalso. apply H0. eapply journal_exact_ownership_covers_every_publication; eauto.
Qed.

Inductive journal_completion := JournalStored | JournalMatched | JournalRejected.

Definition journal_lifecycle (intro : bool) (comm : option bool) completion :=
  match intro, comm, completion with
  | true, None, JournalStored => true
  | true, Some true, JournalMatched => true
  | false, None, JournalRejected => true
  | true, Some false, JournalRejected => true
  | _, _, _ => false
  end.

Theorem journal_rejection_has_exact_denied_stage : forall intro comm,
  journal_lifecycle intro comm JournalRejected = true <->
  (intro = false /\ comm = None) \/ (intro = true /\ comm = Some false).
Proof. intros [] [[]|]; simpl; intuition discriminate. Qed.

Theorem journal_match_has_both_grants : forall intro comm,
  journal_lifecycle intro comm JournalMatched = true <->
  intro = true /\ comm = Some true.
Proof. intros [] [[]|]; simpl; intuition discriminate. Qed.

Theorem journal_stored_has_only_introduction : forall intro comm,
  journal_lifecycle intro comm JournalStored = true <-> intro = true /\ comm = None.
Proof. intros [] [[]|]; simpl; intuition discriminate. Qed.

Theorem journal_denied_introduction_excludes_comm : forall comm completion,
  journal_lifecycle false (Some comm) completion = false.
Proof. intros [] []; reflexivity. Qed.

Definition journal_declared_predecessors (channels : list nat) (last : nat -> option nat) :=
  flat_map (fun channel => match last channel with Some op => [op] | None => [] end) channels.

Theorem journal_declared_predecessor_exact : forall channels last predecessor,
  In predecessor (journal_declared_predecessors channels last) <->
  exists channel, In channel channels /\ last channel = Some predecessor.
Proof.
  intros. unfold journal_declared_predecessors. rewrite in_flat_map. split.
  - intros [channel [present linked]]. exists channel. split; auto.
    destruct (last channel) as [op|] eqn:previous; simpl in linked; try contradiction.
    destruct linked as [equal|impossible]; [now subst | contradiction].
  - intros [channel [present previous]]. exists channel. split; auto.
    rewrite previous. simpl. auto.
Qed.

Theorem journal_exact_predecessors_preserve_declared_conflicts : forall channels last supplied,
  (forall predecessor, In predecessor supplied <->
    In predecessor (journal_declared_predecessors channels last)) ->
  forall channel predecessor, In channel channels -> last channel = Some predecessor ->
  In predecessor supplied.
Proof.
  intros channels last supplied exact channel predecessor present previous.
  apply exact. apply journal_declared_predecessor_exact. eauto.
Qed.
