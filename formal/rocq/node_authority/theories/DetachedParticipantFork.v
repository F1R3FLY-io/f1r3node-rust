From Stdlib Require Import List Bool Arith Lia.
Import ListNotations.

Section DetachedParticipantFork.
  Definition admissible (participants : list nat) (parent child : nat) : Prop :=
    In parent participants /\ ~ In child participants /\ parent <> child.

  Definition fork (participants : list nat) (child : nat) : list nat :=
    child :: participants.

  Definition complete (participants : list nat) (participant : nat) : list nat :=
    filter (fun p => negb (Nat.eqb participant p)) participants.

  Definition split (participants : list nat) (parent child : nat) : list nat :=
    child :: complete participants parent.

  Definition frontier_ready (participants waiting : list nat) : Prop :=
    forall participant, In participant participants -> In participant waiting.

  Definition frontier_ready_with_drain
    (participants waiting draining : list nat) : Prop :=
    waiting <> [] /\
    forall participant, In participant participants ->
      In participant waiting \/ In participant draining.

  Record Schedule := {
    registered : list nat;
    parked : list nat;
  }.

  Definition park (schedule : Schedule) (participant : nat) : Schedule :=
    {| registered := registered schedule;
       parked := participant :: parked schedule |}.

  Definition resume (schedule : Schedule) (participant : nat) : Schedule :=
    {| registered := registered schedule;
       parked := complete (parked schedule) participant |}.

  Theorem fork_keeps_parent : forall participants parent child,
    admissible participants parent child ->
    In parent (fork participants child).
  Proof.
    intros participants parent child [Hparent _].
    right. exact Hparent.
  Qed.

  Theorem fork_registers_child : forall participants child,
    In child (fork participants child).
  Proof.
    intros participants child. left. reflexivity.
  Qed.

  Theorem fork_keeps_existing : forall participants child participant,
    In participant participants -> In participant (fork participants child).
  Proof.
    intros participants child participant Hin. right. exact Hin.
  Qed.

  Theorem fork_preserves_uniqueness : forall participants parent child,
    NoDup participants -> admissible participants parent child ->
    NoDup (fork participants child).
  Proof.
    intros participants parent child Hunique [_ [Hfresh _]].
    constructor; assumption.
  Qed.

  Lemma complete_keeps_other : forall participants removed retained,
    In retained participants -> removed <> retained ->
    In retained (complete participants removed).
  Proof.
    intros participants removed retained Hin Hneq.
    unfold complete. apply filter_In. split.
    - exact Hin.
    - apply negb_true_iff. apply Nat.eqb_neq. exact Hneq.
  Qed.

  Lemma complete_removes_self : forall participants participant,
    ~ In participant (complete participants participant).
  Proof.
    intros participants participant Hin.
    unfold complete in Hin. apply filter_In in Hin.
    destruct Hin as [_ Hkeep].
    rewrite Nat.eqb_refl in Hkeep. discriminate.
  Qed.

  Theorem completed_parent_leaves_child : forall participants parent child,
    admissible participants parent child ->
    In child (complete (fork participants child) parent).
  Proof.
    intros participants parent child [_ [_ Hneq]].
    apply complete_keeps_other.
    - apply fork_registers_child.
    - exact Hneq.
  Qed.

  Theorem completed_child_leaves_parent : forall participants parent child,
    admissible participants parent child ->
    In parent (complete (fork participants child) child).
  Proof.
    intros participants parent child Hadmit.
    apply complete_keeps_other.
    - apply fork_keeps_parent. exact Hadmit.
    - destruct Hadmit as [_ [_ Hneq]]. congruence.
  Qed.

  Theorem split_cannot_continue_parent : forall participants parent child,
    parent <> child -> ~ In parent (split participants parent child).
  Proof.
    intros participants parent child Hneq [Heq | Hin].
    - congruence.
    - apply (complete_removes_self participants parent). exact Hin.
  Qed.

  Theorem waiting_child_blocks_on_active_parent : forall participants parent child,
    admissible participants parent child ->
    ~ frontier_ready (fork participants child) [child].
  Proof.
    intros participants parent child Hadmit Hready.
    pose proof (fork_keeps_parent participants parent child Hadmit) as Hparent.
    specialize (Hready parent Hparent).
    destruct Hready as [Heq | []].
    destruct Hadmit as [_ [_ Hneq]]. congruence.
  Qed.

  Theorem parent_completion_restores_child_frontier :
    forall participants waiting parent child,
      admissible participants parent child ->
      (forall participant, In participant participants ->
        participant <> parent -> In participant waiting) ->
      In child waiting ->
      frontier_ready (complete (fork participants child) parent) waiting.
  Proof.
    intros participants waiting parent child Hadmit Hothers Hchild participant Hin.
    unfold complete, fork in Hin.
    apply filter_In in Hin.
    destruct Hin as [[Hthis | Hold] Hkeep].
    - subst. exact Hchild.
    - apply negb_true_iff in Hkeep.
      apply Nat.eqb_neq in Hkeep.
      apply Hothers; try assumption. congruence.
  Qed.

  Theorem draining_parent_allows_child_frontier :
    forall participants waiting draining parent child,
      admissible participants parent child ->
      In child waiting -> In parent draining ->
      (forall participant, In participant participants ->
        participant <> parent ->
        In participant waiting \/ In participant draining) ->
      frontier_ready_with_drain
        (fork participants child) waiting draining.
  Proof.
    intros participants waiting draining parent child
      Hadmit Hchild Hparent Hothers.
    split.
    - intro Hempty. subst. contradiction.
    - intros participant [Hnew | Hold].
      + subst. left. exact Hchild.
      + destruct (Nat.eq_dec participant parent) as [Heq | Hneq].
        * subst. right. exact Hparent.
        * apply Hothers; assumption.
  Qed.

  Theorem draining_parent_keeps_evaluation_permit :
    forall participants parent child,
      admissible participants parent child ->
      fork participants child <> [].
  Proof.
    intros participants parent child _ Hempty.
    discriminate Hempty.
  Qed.

  Theorem parking_keeps_registration : forall schedule participant,
    In participant (registered schedule) ->
    In participant (registered (park schedule participant)).
  Proof.
    intros schedule participant Hin. exact Hin.
  Qed.

  Theorem resuming_keeps_registration : forall schedule participant,
    In participant (registered schedule) ->
    In participant (registered (resume schedule participant)).
  Proof.
    intros schedule participant Hin. exact Hin.
  Qed.

  Theorem parking_marks_participant : forall schedule participant,
    In participant (parked (park schedule participant)).
  Proof.
    intros schedule participant. left. reflexivity.
  Qed.

  Theorem resuming_clears_parked_status : forall schedule participant,
    ~ In participant (parked (resume schedule participant)).
  Proof.
    intros schedule participant.
    apply complete_removes_self.
  Qed.
End DetachedParticipantFork.

Print Assumptions fork_keeps_parent.
Print Assumptions fork_registers_child.
Print Assumptions fork_keeps_existing.
Print Assumptions fork_preserves_uniqueness.
Print Assumptions completed_parent_leaves_child.
Print Assumptions completed_child_leaves_parent.
Print Assumptions split_cannot_continue_parent.
Print Assumptions waiting_child_blocks_on_active_parent.
Print Assumptions parent_completion_restores_child_frontier.
Print Assumptions draining_parent_allows_child_frontier.
Print Assumptions draining_parent_keeps_evaluation_permit.
Print Assumptions parking_keeps_registration.
Print Assumptions resuming_keeps_registration.
Print Assumptions parking_marks_participant.
Print Assumptions resuming_clears_parked_status.
