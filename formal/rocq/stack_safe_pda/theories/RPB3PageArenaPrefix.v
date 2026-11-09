From Stdlib Require Import List Bool Arith Lia.

Import ListNotations.

Module CompletedPagePrefix.

Inductive node :=
| Empty
| Registered (previous : node)
| Segment (previous : node) (page page_start used : nat)
| Concat (lhs rhs : node).

Fixpoint certified (physical_full : nat -> bool) (root : node) : bool :=
  match root with
  | Empty => true
  | Registered previous => certified physical_full previous
  | Segment previous page _ _ =>
      certified physical_full previous && physical_full page
  | Concat lhs rhs =>
      certified physical_full lhs && certified physical_full rhs
  end.

Inductive reaches_generated : node -> nat -> Prop :=
| reaches_own_segment : forall previous page start used,
    reaches_generated (Segment previous page start used) page
| reaches_segment_previous : forall previous page start used target,
    reaches_generated previous target ->
    reaches_generated (Segment previous page start used) target
| reaches_registered_previous : forall previous target,
    reaches_generated previous target ->
    reaches_generated (Registered previous) target
| reaches_concat_left : forall lhs rhs target,
    reaches_generated lhs target ->
    reaches_generated (Concat lhs rhs) target
| reaches_concat_right : forall lhs rhs target,
    reaches_generated rhs target ->
    reaches_generated (Concat lhs rhs) target.

Theorem certificate_covers_every_reachable_generated_page :
  forall physical_full root page,
    certified physical_full root = true ->
    reaches_generated root page ->
    physical_full page = true.
Proof.
  intros physical_full root.
  induction root as [|previous IH|previous IH own start used|lhs IHleft rhs IHright];
    intros page Hcert Hreach; simpl in Hcert.
  - inversion Hreach.
  - inversion Hreach; subst; eauto.
  - apply andb_true_iff in Hcert as [Hprevious Hown].
    inversion Hreach; subst; eauto.
  - apply andb_true_iff in Hcert as [Hleft Hright].
    inversion Hreach; subst; eauto.
Qed.

Definition bit_sound (physical_full : nat -> bool) (root : node)
    (stored_bit : bool) : Prop :=
  stored_bit = true -> certified physical_full root = true.

Theorem empty_bit_is_sound :
  forall physical_full, bit_sound physical_full Empty true.
Proof.
  intros physical_full _. reflexivity.
Qed.

Theorem registered_bit_inherits_soundness :
  forall physical_full previous stored_bit,
    bit_sound physical_full previous stored_bit ->
    bit_sound physical_full (Registered previous) stored_bit.
Proof.
  intros physical_full previous stored_bit Hsound Hbit.
  exact (Hsound Hbit).
Qed.

Theorem segment_bit_conjoins_predecessor_and_physical_fullness :
  forall physical_full previous page start used predecessor_bit,
    bit_sound physical_full previous predecessor_bit ->
    bit_sound physical_full (Segment previous page start used)
      (predecessor_bit && physical_full page).
Proof.
  intros physical_full previous page start used predecessor_bit Hsound Hbit.
  apply andb_true_iff in Hbit as [Hprevious Hpage].
  simpl. apply andb_true_iff. split; [now apply Hsound|exact Hpage].
Qed.

Theorem concat_bit_conjoins_both_children :
  forall physical_full lhs rhs left_bit right_bit,
    bit_sound physical_full lhs left_bit ->
    bit_sound physical_full rhs right_bit ->
    bit_sound physical_full (Concat lhs rhs) (left_bit && right_bit).
Proof.
  intros physical_full lhs rhs left_bit right_bit Hleft Hright Hbit.
  apply andb_true_iff in Hbit as [Hleft_bit Hright_bit].
  simpl. apply andb_true_iff. split; [now apply Hleft|now apply Hright].
Qed.

Theorem certificate_remains_sound_when_pages_only_fill :
  forall old_full new_full root,
    (forall page, old_full page = true -> new_full page = true) ->
    certified old_full root = true ->
    certified new_full root = true.
Proof.
  intros old_full new_full root.
  induction root as [|previous IH|previous IH page start used|lhs IHleft rhs IHright];
    intros Hmonotone Hcert; simpl in *.
  - reflexivity.
  - now apply IH.
  - apply andb_true_iff in Hcert as [Hprevious Hpage].
    apply andb_true_iff. split; [now apply IH|now apply Hmonotone].
  - apply andb_true_iff in Hcert as [Hleft Hright].
    apply andb_true_iff. split; [now apply IHleft|now apply IHright].
Qed.

Theorem stored_bit_remains_sound_when_pages_only_fill :
  forall old_full new_full root stored_bit,
    (forall page, old_full page = true -> new_full page = true) ->
    bit_sound old_full root stored_bit ->
    bit_sound new_full root stored_bit.
Proof.
  intros old_full new_full root stored_bit Hmonotone Hsound Hbit.
  eapply certificate_remains_sound_when_pages_only_fill;
    [exact Hmonotone|now apply Hsound].
Qed.

Definition eligible_anchor (physical_full : nat -> bool)
    (capacity : nat) (root : node) (stored_bit : bool) : bool :=
  match root with
  | Segment _ page start used =>
      stored_bit &&
      Nat.eqb (start + used) capacity &&
      physical_full page
  | _ => false
  end.

Theorem eligible_anchor_has_completed_own_page :
  forall physical_full capacity root stored_bit,
    eligible_anchor physical_full capacity root stored_bit = true ->
    exists previous page start used,
      root = Segment previous page start used /\
      start + used = capacity /\
      physical_full page = true /\
      stored_bit = true.
Proof.
  intros physical_full capacity root stored_bit Heligible.
  destruct root as [|previous|previous page start used|lhs rhs];
    simpl in Heligible; try discriminate.
  apply andb_true_iff in Heligible as [Hprefix Hfull].
  apply andb_true_iff in Hprefix as [Hbit Hend].
  apply Nat.eqb_eq in Hend.
  exists previous, page, start, used.
  repeat split; auto.
Qed.

Theorem anchored_prefix_cannot_pin_an_appendable_generated_page :
  forall physical_full capacity root stored_bit page,
    bit_sound physical_full root stored_bit ->
    eligible_anchor physical_full capacity root stored_bit = true ->
    reaches_generated root page ->
    physical_full page = false ->
    False.
Proof.
  intros physical_full capacity root stored_bit page Hsound Hanchor Hreach Hpartial.
  pose proof (eligible_anchor_has_completed_own_page
    physical_full capacity root stored_bit Hanchor) as
    [previous [own [start [used [Hroot [_ [_ Hbit]]]]]]].
  subst root.
  specialize (Hsound Hbit).
  pose proof (certificate_covers_every_reachable_generated_page
    physical_full (Segment previous own start used) page Hsound Hreach) as Hfull.
  rewrite Hpartial in Hfull.
  discriminate.
Qed.

Theorem newer_completed_fork_cannot_anchor_over_older_partial_page :
  forall physical_full capacity previous newer_page start used old_page stored_bit,
    bit_sound physical_full (Segment previous newer_page start used) stored_bit ->
    reaches_generated previous old_page ->
    physical_full old_page = false ->
    eligible_anchor physical_full capacity
      (Segment previous newer_page start used) stored_bit = false.
Proof.
  intros physical_full capacity previous newer_page start used old_page
    stored_bit Hsound Hreach Hpartial.
  destruct (eligible_anchor physical_full capacity
    (Segment previous newer_page start used) stored_bit) eqn:Hanchor;
    [|reflexivity].
  exfalso.
  eapply anchored_prefix_cannot_pin_an_appendable_generated_page;
    [exact Hsound|exact Hanchor| |exact Hpartial].
  now apply reaches_segment_previous.
Qed.

Fixpoint anchor_count (page_indexed : list (option node)) : nat :=
  match page_indexed with
  | [] => 0
  | None :: rest => anchor_count rest
  | Some _ :: rest => S (anchor_count rest)
  end.

Fixpoint insert_if_empty (page_indexed : list (option node))
    (page : nat) (candidate : node) : list (option node) :=
  match page_indexed, page with
  | [], _ => []
  | None :: rest, 0 => Some candidate :: rest
  | Some existing :: rest, 0 => Some existing :: rest
  | slot :: rest, S earlier => slot :: insert_if_empty rest earlier candidate
  end.

Definition ensure_page_slot (page_indexed : list (option node))
    (page : nat) : list (option node) :=
  page_indexed ++ repeat None (S page - length page_indexed).

Definition publish_anchor (physical_full : nat -> bool) (capacity : nat)
    (page_indexed : list (option node)) (candidate : node)
    (stored_bit : bool) : list (option node) :=
  match candidate with
  | Segment _ page _ _ =>
      if eligible_anchor physical_full capacity candidate stored_bit then
        insert_if_empty (ensure_page_slot page_indexed page) page candidate
      else page_indexed
  | _ => page_indexed
  end.

Lemma occupied_slot_survives_insertion :
  forall page_indexed page existing candidate,
    nth_error page_indexed page = Some (Some existing) ->
    nth_error (insert_if_empty page_indexed page candidate) page =
      Some (Some existing).
Proof.
  induction page_indexed as [|slot rest IH];
    intros [|page] existing candidate Hoccupied; simpl in *;
    try discriminate.
  - destruct slot as [old|]; inversion Hoccupied; subst; reflexivity.
  - destruct slot; simpl; now apply IH.
Qed.

Lemma occupied_slot_survives_growth :
  forall page_indexed page existing,
    nth_error page_indexed page = Some (Some existing) ->
    nth_error (ensure_page_slot page_indexed page) page = Some (Some existing).
Proof.
  intros page_indexed page existing Hoccupied.
  unfold ensure_page_slot.
  rewrite nth_error_app1.
  - exact Hoccupied.
  - apply nth_error_Some. rewrite Hoccupied. discriminate.
Qed.

Theorem publication_never_replaces_an_occupied_physical_page :
  forall physical_full capacity page_indexed previous page start used
      stored_bit existing,
    nth_error page_indexed page = Some (Some existing) ->
    nth_error (publish_anchor physical_full capacity page_indexed
      (Segment previous page start used) stored_bit) page =
      Some (Some existing).
Proof.
  intros physical_full capacity page_indexed previous page start used
    stored_bit existing Hoccupied.
  unfold publish_anchor.
  destruct (eligible_anchor physical_full capacity
    (Segment previous page start used) stored_bit) eqn:Heligible.
  - apply occupied_slot_survives_insertion.
    now apply occupied_slot_survives_growth.
  - exact Hoccupied.
Qed.

Theorem one_anchor_per_physical_page_bound :
  forall page_indexed,
    anchor_count page_indexed <= length page_indexed.
Proof.
  induction page_indexed as [|slot rest IH]; simpl; [lia|].
  destruct slot; simpl; lia.
Qed.

Fixpoint full_slots_only (physical_full : nat -> bool) (page : nat)
    (page_indexed : list (option node)) : Prop :=
  match page_indexed with
  | [] => True
  | None :: rest => full_slots_only physical_full (S page) rest
  | Some _ :: rest =>
      physical_full page = true /\
      full_slots_only physical_full (S page) rest
  end.

Lemma repeated_empty_slots_are_full_only :
  forall physical_full page count,
    full_slots_only physical_full page (repeat None count).
Proof.
  intros physical_full page count.
  revert page.
  induction count as [|count IH]; intros page; simpl; auto.
Qed.

Lemma growing_with_empty_slots_preserves_full_only :
  forall physical_full page_indexed page count,
    full_slots_only physical_full page page_indexed ->
    full_slots_only physical_full page
      (page_indexed ++ repeat None count).
Proof.
  intros physical_full page_indexed.
  induction page_indexed as [|slot rest IH];
    intros page count Hsound; simpl in *.
  - apply repeated_empty_slots_are_full_only.
  - destruct slot as [existing|].
    + destruct Hsound as [Hfull Hrest].
      split; [exact Hfull|now apply IH].
    + now apply IH.
Qed.

Lemma insertion_on_a_full_page_preserves_full_only :
  forall physical_full page_indexed offset index candidate,
    full_slots_only physical_full offset page_indexed ->
    physical_full (offset + index) = true ->
    full_slots_only physical_full offset
      (insert_if_empty page_indexed index candidate).
Proof.
  intros physical_full page_indexed.
  induction page_indexed as [|slot rest IH];
    intros offset [|index] candidate Hsound Hfull; simpl in *; auto.
  - destruct slot as [existing|]; simpl.
    + exact Hsound.
    + split; [replace (offset + 0) with offset in Hfull by lia; exact Hfull|exact Hsound].
  - destruct slot as [existing|]; simpl in *.
    + destruct Hsound as [Hhead Hrest].
      split; [exact Hhead|].
      apply IH; [exact Hrest|].
      replace (S offset + index) with (offset + S index) by lia.
      exact Hfull.
    + apply IH; [exact Hsound|].
      replace (S offset + index) with (offset + S index) by lia.
      exact Hfull.
Qed.

Theorem publication_retains_only_physically_completed_page_slots :
  forall physical_full capacity page_indexed candidate stored_bit,
    full_slots_only physical_full 0 page_indexed ->
    full_slots_only physical_full 0
      (publish_anchor physical_full capacity page_indexed candidate stored_bit).
Proof.
  intros physical_full capacity page_indexed candidate stored_bit Hsound.
  unfold publish_anchor.
  destruct candidate as [|previous|previous page start used|lhs rhs];
    try exact Hsound.
  destruct (eligible_anchor physical_full capacity
    (Segment previous page start used) stored_bit) eqn:Heligible;
    [|exact Hsound].
  apply insertion_on_a_full_page_preserves_full_only.
  - unfold ensure_page_slot.
    now apply growing_with_empty_slots_preserves_full_only.
  - replace (0 + page) with page by lia.
    unfold eligible_anchor in Heligible; simpl in Heligible.
    now apply andb_true_iff in Heligible as [_ Hfull].
Qed.

Fixpoint physically_completed_slots (physical_full : nat -> bool)
    (page : nat) (page_indexed : list (option node)) : nat :=
  match page_indexed with
  | [] => 0
  | _ :: rest =>
      (if physical_full page then 1 else 0) +
      physically_completed_slots physical_full (S page) rest
  end.

Theorem anchors_are_bounded_by_physically_completed_pages :
  forall physical_full page_indexed page,
    full_slots_only physical_full page page_indexed ->
    anchor_count page_indexed <=
      physically_completed_slots physical_full page page_indexed.
Proof.
  intros physical_full page_indexed.
  induction page_indexed as [|slot rest IH];
    intros page Hsound; simpl in *; [lia|].
  destruct slot as [existing|].
  - destruct Hsound as [Hfull Hrest].
    rewrite Hfull. simpl. specialize (IH (S page) Hrest). lia.
  - destruct (physical_full page); simpl;
      specialize (IH (S page) Hsound); lia.
Qed.

End CompletedPagePrefix.
