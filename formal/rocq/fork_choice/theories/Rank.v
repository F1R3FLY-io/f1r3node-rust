(* ===========================================================================
   Rank.v - The LMD-GHOST upward descent: terminates, and picks the heaviest
   scored child at every level.

   `greedy_ghost_head` starts at the LCA and moves to the argmax scored child
   (highest score, ties by hash) at each level, stopping at a block with no
   scored child. `TerminalFrontier.v` separately proves exact concurrent
   frontier retention and the head-first composition performed by
   `rank_forkchoices`.

   Proved:
     * `rank_terminates`   - with fuel = 1 + dag_max_num, the descent reaches a
                             fixpoint (a tip: no scored child). Each step strictly
                             INCREASES the block number, bounded by dag_max_num
                             (Foundation.dag_height_bound) - the ascending mirror
                             of finalized_floor's spine_walk_terminates.
     * `still_same_fixpoint` / `replace_keep_self_when_no_scored_children`
                           - the `still_same` stop condition: no scored child =>
                             rank returns the block unchanged (any fuel).
     * `rank_selects_heaviest` - each step's chosen child OUTRANKS every scored
                             child (the (score DESC, hash ASC) argmax = GHOST's
                             heaviest subtree, via TieBreak.sort_argmax_unique).
     * `rank_head_is_argmax`  - the chosen child has the MAXIMUM score.

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                         | Rust (casper/src/rust/estimator.rs)
   -----------------------------+------------------------------------------------
   children / scored_children   | block_dag.children + scores.contains_key
   best_child (argmax)          | sort_by_with_decreasing_order head
   rank (upward descent)        | greedy_ghost_head
   still_same_fixpoint          | no-scored-child terminal condition
   rank_terminates              | finite ascent (bounded by DAG height)
   rank_selects_heaviest        | heaviest-subtree pick (GHOST)
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Import Lia.
Import ListNotations.

From ForkChoice Require Import Foundation.
From ForkChoice Require Import Score.
From ForkChoice Require Import TieBreak.

(* ===========================================================================
   Section 1 - Children, scored children, and the argmax best child
   =========================================================================== *)

(* The DAG children of `h`: blocks that list `h` among their parents. *)
Definition children (d : DAG) (h : BlockHash) : list BlockHash :=
  map blk_hash (filter (fun b => existsb (Nat.eqb h) (blk_parents b)) d).

(* Children that were scored (lie on some validator's supporting chain). *)
Definition scored_children (d : DAG) (is_scored : BlockHash -> bool) (h : BlockHash)
  : list BlockHash :=
  filter is_scored (children d h).

(* Scored children paired with their (score, hash) for the tie-break sort. *)
Definition child_entries (d : DAG) (score : BlockHash -> nat)
                         (is_scored : BlockHash -> bool) (h : BlockHash) : list entry :=
  map (fun x => (score x, x)) (scored_children d is_scored h).

Lemma map_ehash_child_entries :
  forall d score is_scored h,
    map ehash (child_entries d score is_scored h) = scored_children d is_scored h.
Proof.
  intros d score is_scored h. unfold child_entries.
  induction (scored_children d is_scored h) as [| x l IH]; simpl.
  - reflexivity.
  - f_equal. exact IH.
Qed.

(* The GHOST winner at a level: the argmax (head of the sorted child entries). *)
Definition best_child (d : DAG) (score : BlockHash -> nat)
                      (is_scored : BlockHash -> bool) (h : BlockHash) : option BlockHash :=
  match sort (child_entries d score is_scored h) with
  | [] => None
  | e :: _ => Some (ehash e)
  end.

Lemma best_child_in_scored :
  forall d score is_scored h c,
    best_child d score is_scored h = Some c -> In c (scored_children d is_scored h).
Proof.
  intros d score is_scored h c H. unfold best_child in H.
  destruct (sort (child_entries d score is_scored h)) as [| e s] eqn:Es; [discriminate |].
  injection H as Hc.
  assert (Hin : In e (child_entries d score is_scored h)).
  { apply (Permutation_in e (sort_is_permutation (child_entries d score is_scored h))).
    rewrite Es. left; reflexivity. }
  unfold child_entries in Hin. apply in_map_iff in Hin.
  destruct Hin as [x [Hex Hxin]]. subst e. simpl in Hc. subst c. exact Hxin.
Qed.

Lemma best_child_in_children :
  forall d score is_scored h c,
    best_child d score is_scored h = Some c -> In c (children d h).
Proof.
  intros d score is_scored h c H.
  apply best_child_in_scored in H. unfold scored_children in H.
  apply filter_In in H. destruct H as [H _]. exact H.
Qed.

(* A child is strictly higher than its parent (parents are older). Needs
   hash-collision-freedom (wf_lookup) to identify the child's own height. *)
Lemma child_num_gt :
  forall d h c, wf_dag d -> wf_lookup d -> In c (children d h) -> numof d h < numof d c.
Proof.
  intros d h c Hwf Hwl Hin.
  unfold children in Hin. apply in_map_iff in Hin.
  destruct Hin as [b [Hbh Hbf]]. apply filter_In in Hbf. destruct Hbf as [Hbd Hex].
  (* In h (blk_parents b) *)
  apply existsb_exists in Hex. destruct Hex as [ph [Hphin Hpheq]].
  apply Nat.eqb_eq in Hpheq. subst ph.
  (* wf_dag gives a smaller-numbered parent for h *)
  destruct (proj1 (Hwf b Hbd) h Hphin) as [p [Hlp Hlt]].
  assert (Hnh : numof d h = blk_num p) by (unfold numof; rewrite Hlp; reflexivity).
  assert (Hnc : numof d c = blk_num b).
  { rewrite <- Hbh. apply numof_blk; assumption. }
  lia.
Qed.

(* ===========================================================================
   Section 2 - The upward descent and its one-step unfolding
   =========================================================================== *)

Fixpoint rank (d : DAG) (score : BlockHash -> nat) (is_scored : BlockHash -> bool)
              (fuel : nat) (h : BlockHash) : BlockHash :=
  match fuel with
  | 0 => h
  | S f =>
      match best_child d score is_scored h with
      | None => h
      | Some c => rank d score is_scored f c
      end
  end.

Lemma rank_eq :
  forall d score is_scored fuel h,
    rank d score is_scored fuel h =
      match fuel with
      | 0 => h
      | S f =>
          match best_child d score is_scored h with
          | None => h
          | Some c => rank d score is_scored f c
          end
      end.
Proof. intros; destruct fuel; reflexivity. Qed.

(* The `still_same` stop condition: with no scored child, rank returns h. *)
Theorem still_same_fixpoint :
  forall d score is_scored fuel h,
    best_child d score is_scored h = None -> rank d score is_scored fuel h = h.
Proof.
  intros d score is_scored fuel h H. rewrite rank_eq.
  destruct fuel; [reflexivity | rewrite H; reflexivity].
Qed.

(* No scored children => best_child is None => keep self (models
   replace_block_hash_with_children returning [b] at estimator.rs:361). *)
Theorem replace_keep_self_when_no_scored_children :
  forall d score is_scored h,
    scored_children d is_scored h = [] -> best_child d score is_scored h = None.
Proof.
  intros d score is_scored h H. unfold best_child, child_entries. rewrite H. simpl. reflexivity.
Qed.

(* ===========================================================================
   Section 3 - Termination: the ascent reaches a tip within the DAG height
   =========================================================================== *)

Lemma rank_stops :
  forall d score is_scored fuel h,
    wf_dag d -> wf_lookup d ->
    dag_max_num d - numof d h < fuel ->
    best_child d score is_scored (rank d score is_scored fuel h) = None.
Proof.
  intros d score is_scored fuel.
  induction fuel as [| f IH]; intros h Hwf Hwl Hfuel.
  - exfalso. lia.
  - rewrite rank_eq. destruct (best_child d score is_scored h) as [c |] eqn:Eb.
    + (* advance to c; the remaining height strictly decreases *)
      apply IH; [exact Hwf | exact Hwl |].
      assert (Hin : In c (children d h)) by (apply (best_child_in_children d score is_scored h c Eb)).
      assert (Hgt : numof d h < numof d c) by (apply (child_num_gt d h c Hwf Hwl Hin)).
      assert (Hle : numof d c <= dag_max_num d) by apply numof_le_max.
      lia.
    + (* fixpoint reached at h itself *)
      exact Eb.
Qed.

(* T-TERM: with fuel = 1 + dag_max_num, the descent always lands on a tip. *)
Theorem rank_terminates :
  forall d score is_scored h,
    wf_dag d -> wf_lookup d ->
    best_child d score is_scored (rank d score is_scored (S (dag_max_num d)) h) = None.
Proof.
  intros d score is_scored h Hwf Hwl.
  apply rank_stops; [exact Hwf | exact Hwl |].
  assert (numof d h <= dag_max_num d) by apply numof_le_max. lia.
Qed.

(* ===========================================================================
   Section 4 - Heaviest-child selection (GHOST) via the tie-break argmax
   =========================================================================== *)

Theorem rank_selects_heaviest :
  forall d score is_scored h c,
    NoDup (scored_children d is_scored h) ->
    best_child d score is_scored h = Some c ->
    In c (scored_children d is_scored h) /\
    (forall c', In c' (scored_children d is_scored h) ->
       c' = c \/ ord (score c, c) (score c', c')).
Proof.
  intros d score is_scored h c Hnd Hbest.
  set (L := child_entries d score is_scored h) in *.
  assert (HndL : NoDup (map ehash L)).
  { unfold L. rewrite map_ehash_child_entries. exact Hnd. }
  (* best_child = Some c gives sort L = w :: s with c = ehash w *)
  unfold best_child in Hbest. fold L in Hbest.
  destruct (sort L) as [| w s] eqn:Es; [discriminate |].
  injection Hbest as Hc.
  (* argmax property from TieBreak *)
  pose proof (sort_argmax_unique L HndL) as Hargmax.
  rewrite Es in Hargmax. destruct Hargmax as [HwinL Hmax].
  (* w = (score c, c) *)
  assert (Hwform : w = (score c, c)).
  { unfold L, child_entries in HwinL. apply in_map_iff in HwinL.
    destruct HwinL as [x [Hx Hxin]]. subst w. simpl in Hc. subst c. reflexivity. }
  split.
  - (* In c scored_children *)
    apply (best_child_in_scored d score is_scored h c).
    unfold best_child. fold L. rewrite Es. rewrite <- Hc. reflexivity.
  - intros c' Hc'.
    assert (Hc'L : In (score c', c') L).
    { unfold L, child_entries. apply in_map_iff. exists c'. split; [reflexivity | exact Hc']. }
    destruct (Hmax (score c', c') Hc'L) as [Heq | Hord].
    + left. rewrite Hwform in Heq. injection Heq as _ Hcc. exact Hcc.
    + right. rewrite Hwform in Hord. exact Hord.
Qed.

(* The chosen child has the MAXIMUM score (the heaviest-subtree invariant). *)
Theorem rank_head_is_argmax :
  forall d score is_scored h c,
    NoDup (scored_children d is_scored h) ->
    best_child d score is_scored h = Some c ->
    forall c', In c' (scored_children d is_scored h) -> score c' <= score c.
Proof.
  intros d score is_scored h c Hnd Hbest c' Hc'.
  destruct (rank_selects_heaviest d score is_scored h c Hnd Hbest) as [_ Hmax].
  destruct (Hmax c' Hc') as [Heq | Hord].
  - subst c'. apply Nat.le_refl.
  - unfold ord, escore, ehash in Hord. simpl in Hord. lia.
Qed.

(* ===========================================================================
   Section 5 - The concrete GHOST instantiation (score = Score.build_scores)

   Ties the abstract descent to the estimator's actual scoring: a candidate is
   "scored" iff it lies on some latest message's supporting chain, and its score
   is the cumulative supporter weight (Score.build_scores). Exercises the Score
   dependency; the termination carries over verbatim.
   =========================================================================== *)

Definition supported_b (d : DAG) (fuel : nat) (lms : list (Validator * BlockHash))
                       (b : BlockHash) : bool :=
  existsb (fun e => anc_ofb d fuel b (snd e)) lms.

Corollary ghost_rank_terminates :
  forall authority d sfuel lms h,
    wf_dag d -> wf_lookup d ->
    best_child d (build_scores authority d sfuel lms) (supported_b d sfuel lms)
      (rank d (build_scores authority d sfuel lms) (supported_b d sfuel lms)
        (S (dag_max_num d)) h)
      = None.
Proof.
  intros. apply rank_terminates; assumption.
Qed.
