(* ===========================================================================
   CertifiedContext.v - Complete floor-descendant fork-choice projection.

   The concrete estimator accepts only a CertifiedConsensusContext with one
   exact latest-message slot for every active finalized-floor validator. It
   scores only the context's eligible projection, whose messages must descend
   from the certified floor. Receiver-local caches are not inputs.
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Bool.Bool.
Import ListNotations.

From ForkChoice Require Import Foundation.

Definition slot_present (v : Validator)
                        (exact : list (Validator * BlockHash)) : bool :=
  existsb (fun entry => Nat.eqb (fst entry) v) exact.

Definition complete_slots (active : list Validator)
                          (exact : list (Validator * BlockHash)) : bool :=
  forallb (fun v => slot_present v exact) active.

Definition project_floor_descendants
           (eligible : list Validator)
           (descends_from_floor : BlockHash -> bool)
           (exact : list (Validator * BlockHash))
  : list (Validator * BlockHash) :=
  filter
    (fun entry =>
       andb (existsb (Nat.eqb (fst entry)) eligible)
            (descends_from_floor (snd entry)))
    exact.

Lemma complete_slots_sound :
  forall active exact,
    complete_slots active exact = true ->
    forall v, In v active -> exists h, In (v, h) exact.
Proof.
  intros active exact Hcomplete v Hin.
  unfold complete_slots in Hcomplete.
  apply forallb_forall with (x := v) in Hcomplete; [| exact Hin].
  unfold slot_present in Hcomplete.
  apply existsb_exists in Hcomplete.
  destruct Hcomplete as [[v' h] [Hentry Heq]].
  simpl in Heq. apply Nat.eqb_eq in Heq. subst v'.
  exists h. exact Hentry.
Qed.

Lemma floor_projection_sound :
  forall eligible descends exact v h,
    In (v, h) (project_floor_descendants eligible descends exact) ->
    descends h = true.
Proof.
  intros eligible descends exact v h Hin.
  unfold project_floor_descendants in Hin.
  apply filter_In in Hin. destruct Hin as [_ Hpredicate].
  simpl in Hpredicate. apply andb_true_iff in Hpredicate.
  exact (proj2 Hpredicate).
Qed.

Lemma outside_floor_excluded :
  forall eligible descends exact v h,
    descends h = false ->
    ~ In (v, h) (project_floor_descendants eligible descends exact).
Proof.
  intros eligible descends exact v h Houtside Hin.
  pose proof (floor_projection_sound eligible descends exact v h Hin) as Hinside.
  rewrite Houtside in Hinside. discriminate.
Qed.

Definition fork_choice_ready
           (active : list Validator)
           (exact : list (Validator * BlockHash)) : bool :=
  complete_slots active exact.

Lemma incomplete_slots_fail_closed :
  forall active exact,
    complete_slots active exact = false ->
    fork_choice_ready active exact = false.
Proof. intros active exact H. unfold fork_choice_ready. exact H. Qed.

Definition projection_with_receiver_state
           (eligible : list Validator)
           (descends_from_floor : BlockHash -> bool)
           (exact : list (Validator * BlockHash))
           (_receiver_latest : list (Validator * BlockHash))
           (_receiver_invalid : list Validator)
           (_receiver_finalized : list BlockHash)
           (_receiver_top : nat)
  : list (Validator * BlockHash) :=
  project_floor_descendants eligible descends_from_floor exact.

Lemma receiver_state_noninterference :
  forall eligible descends exact latest1 latest2 invalid1 invalid2 finalized1 finalized2 top1 top2,
    projection_with_receiver_state eligible descends exact latest1 invalid1 finalized1 top1 =
    projection_with_receiver_state eligible descends exact latest2 invalid2 finalized2 top2.
Proof. reflexivity. Qed.
