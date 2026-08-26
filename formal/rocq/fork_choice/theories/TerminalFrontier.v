(* ===========================================================================
   TerminalFrontier.v - the complete scored terminal frontier and its
   composition with the greedy LMD-GHOST head.

   The estimator has two independent obligations. Greedy descent chooses the
   canonical GHOST head. Concurrent frontier expansion retains every scored
   terminal branch. The final ranking is the GHOST head followed by the other
   terminal leaves in score-descending, hash-ascending order.
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

From ForkChoice Require Import Foundation.
From ForkChoice Require Import Rank.
From ForkChoice Require Import TieBreak.

Definition terminal_reachable
    (d : DAG) (is_scored : BlockHash -> bool) (root h : BlockHash) : Prop :=
  In h (map blk_hash d)
  /\ anc_of d root h
  /\ is_scored h = true
  /\ scored_children d is_scored h = [].

Definition terminal_reachableb
    (d : DAG) (is_scored : BlockHash -> bool) (root h : BlockHash) : bool :=
  anc_ofb d (S (dag_max_num d)) root h
  && is_scored h
  && Nat.eqb (length (scored_children d is_scored h)) 0.

Definition terminal_frontier
    (d : DAG) (is_scored : BlockHash -> bool) (root : BlockHash) : list BlockHash :=
  filter (terminal_reachableb d is_scored root) (map blk_hash d).

Definition frontier_entries (score : BlockHash -> nat) (frontier : list BlockHash)
    : list entry :=
  map (fun h => (score h, h)) frontier.

Definition ranked_tips
    (score : BlockHash -> nat) (ghost : BlockHash) (frontier : list BlockHash)
    : list BlockHash :=
  ghost
  :: map ehash
       (sort (frontier_entries score (remove Nat.eq_dec ghost frontier))).

Lemma anc_ofb_complete_height :
  forall d, wf_dag d -> forall a x,
    anc_of d a x ->
    forall fuel, numof d x <= fuel -> anc_ofb d (S fuel) a x = true.
Proof.
  intros d Hwf a x Hanc.
  induction Hanc as [x | a x b p Hlook Hin Hanc IH]; intros fuel Hfuel.
  - simpl. rewrite Nat.eqb_refl. reflexivity.
  - simpl. destruct (Nat.eqb a x) eqn:Eax.
    + reflexivity.
    + destruct fuel as [| fuel']; [exfalso |].
      { assert (Hxb : numof d x = blk_num b).
        { unfold numof. rewrite Hlook. reflexivity. }
        assert (Hinb : In b d) by (eapply lookup_In; eauto).
        destruct (proj1 (Hwf b Hinb) p Hin) as [pb [Hplook Hplt]].
        assert (Hpn : numof d p = blk_num pb).
        { unfold numof. rewrite Hplook. reflexivity. }
        lia. }
      rewrite Hlook.
      apply existsb_exists. exists p. split; [exact Hin |].
      apply IH.
      assert (Hxb : numof d x = blk_num b).
      { unfold numof. rewrite Hlook. reflexivity. }
      assert (Hinb : In b d) by (eapply lookup_In; eauto).
      destruct (proj1 (Hwf b Hinb) p Hin) as [pb [Hplook Hplt]].
      assert (Hpn : numof d p = blk_num pb).
      { unfold numof. rewrite Hplook. reflexivity. }
      lia.
Qed.

Lemma terminal_reachableb_correct :
  forall d is_scored root h,
    wf_dag d -> In h (map blk_hash d) ->
    terminal_reachableb d is_scored root h = true
    <-> terminal_reachable d is_scored root h.
Proof.
  intros d is_scored root h Hwf Hin.
  unfold terminal_reachableb, terminal_reachable.
  rewrite !andb_true_iff, Nat.eqb_eq.
  split.
  - intros [[Hanc Hsc] Hlen].
    repeat split; try assumption.
    + apply anc_ofb_sound in Hanc. exact Hanc.
    + apply length_zero_iff_nil. exact Hlen.
  - intros [_ [Hanc [Hsc Hempty]]].
    repeat split; try assumption.
    + apply anc_ofb_complete_height; try assumption.
      apply numof_le_max.
    + rewrite Hempty. reflexivity.
Qed.

Theorem terminal_frontier_exact :
  forall d is_scored root h,
    wf_dag d ->
    In h (terminal_frontier d is_scored root)
    <-> terminal_reachable d is_scored root h.
Proof.
  intros d is_scored root h Hwf.
  unfold terminal_frontier.
  rewrite filter_In.
  split.
  - intros [Hin Hb]. apply (terminal_reachableb_correct d is_scored root h Hwf Hin).
    exact Hb.
  - intro Hterminal.
    destruct Hterminal as [Hin Hrest]. split; [exact Hin |].
    apply (proj2 (terminal_reachableb_correct d is_scored root h Hwf Hin)).
    split; [exact Hin | exact Hrest].
Qed.

Theorem terminal_frontier_nodup :
  forall d is_scored root,
    NoDup (map blk_hash d) -> NoDup (terminal_frontier d is_scored root).
Proof.
  intros d is_scored root Hnd. unfold terminal_frontier.
  apply NoDup_filter. exact Hnd.
Qed.

Lemma best_child_none_iff_no_scored_children :
  forall d score is_scored h,
    best_child d score is_scored h = None
    <-> scored_children d is_scored h = [].
Proof.
  intros d score is_scored h. split.
  - intro Hbest.
    destruct (scored_children d is_scored h) as [| c cs] eqn:Hchildren;
      [reflexivity |].
    destruct (sort ((score c, c) :: map (fun x => (score x, x)) cs)) as [| e es]
      eqn:Hsort.
    + pose proof (sort_is_permutation ((score c, c) :: map (fun x => (score x, x)) cs))
        as Hperm.
      rewrite Hsort in Hperm. apply Permutation_nil in Hperm. discriminate.
    + unfold best_child, child_entries in Hbest.
      rewrite Hchildren in Hbest.
      change
        (match sort ((score c, c) :: map (fun x => (score x, x)) cs) with
         | [] => None
         | e :: _ => Some (ehash e)
         end = None) in Hbest.
      rewrite Hsort in Hbest. discriminate.
  - apply replace_keep_self_when_no_scored_children.
Qed.

Lemma rank_reachable :
  forall d score is_scored fuel root,
    wf_lookup d -> anc_of d root (rank d score is_scored fuel root).
Proof.
  intros d score is_scored fuel.
  induction fuel as [| fuel IH]; intros root Hlookup.
  - simpl. apply anc_refl.
  - rewrite rank_eq. destruct (best_child d score is_scored root) as [child |] eqn:Hbest.
    + assert (Hchild : In child (children d root)).
      { eapply best_child_in_children; eauto. }
      unfold children in Hchild. apply in_map_iff in Hchild.
      destruct Hchild as [b [Hhash Hfiltered]]. apply filter_In in Hfiltered.
      destruct Hfiltered as [Hinb Hparent]. apply existsb_exists in Hparent.
      destruct Hparent as [parent [Hinparent Heq]]. apply Nat.eqb_eq in Heq.
      subst parent. subst child.
      eapply anc_of_trans.
      * eapply anc_par with (b := b) (p := root).
        -- apply Hlookup. exact Hinb.
        -- exact Hinparent.
        -- apply anc_refl.
      * apply IH. exact Hlookup.
    + apply anc_refl.
Qed.

Lemma rank_real :
  forall d score is_scored fuel root,
    In root (map blk_hash d) ->
    In (rank d score is_scored fuel root) (map blk_hash d).
Proof.
  intros d score is_scored fuel.
  induction fuel as [| fuel IH]; intros root Hreal.
  - exact Hreal.
  - rewrite rank_eq. destruct (best_child d score is_scored root) as [child |] eqn:Hbest.
    + apply IH. apply best_child_in_children in Hbest.
      unfold children in Hbest. apply in_map_iff in Hbest.
      destruct Hbest as [b [Hhash Hfiltered]].
      apply filter_In in Hfiltered. destruct Hfiltered as [Hinb _].
      rewrite <- Hhash. apply in_map. exact Hinb.
    + exact Hreal.
Qed.

Lemma rank_stays_scored :
  forall d score is_scored fuel root,
    is_scored root = true -> is_scored (rank d score is_scored fuel root) = true.
Proof.
  intros d score is_scored fuel.
  induction fuel as [| fuel IH]; intros root Hscored.
  - exact Hscored.
  - rewrite rank_eq. destruct (best_child d score is_scored root) as [child |] eqn:Hbest.
    + apply IH. apply best_child_in_scored in Hbest.
      unfold scored_children in Hbest. apply filter_In in Hbest. tauto.
    + exact Hscored.
Qed.

Theorem ghost_head_in_terminal_frontier :
  forall d score is_scored root,
    wf_dag d -> wf_lookup d ->
    In root (map blk_hash d) -> is_scored root = true ->
    In (rank d score is_scored (S (dag_max_num d)) root)
       (terminal_frontier d is_scored root).
Proof.
  intros d score is_scored root Hwf Hlookup Hreal Hscored.
  apply (terminal_frontier_exact d is_scored root
           (rank d score is_scored (S (dag_max_num d)) root) Hwf).
  repeat split.
  - apply rank_real. exact Hreal.
  - apply rank_reachable. exact Hlookup.
  - apply rank_stays_scored. exact Hscored.
  - apply (proj1 (best_child_none_iff_no_scored_children d score is_scored
                    (rank d score is_scored (S (dag_max_num d)) root))).
    apply rank_terminates; assumption.
Qed.

Lemma map_ehash_frontier_entries :
  forall score frontier,
    map ehash (frontier_entries score frontier) = frontier.
Proof.
  intros score frontier. unfold frontier_entries.
  induction frontier as [| h rest IH]; simpl; f_equal; assumption.
Qed.

Lemma nodup_remove_hash :
  forall ghost frontier,
    NoDup frontier -> NoDup (remove Nat.eq_dec ghost frontier).
Proof.
  intros ghost frontier. induction frontier as [| h rest IH]; intro Hnd; simpl.
  - apply NoDup_nil.
  - inversion Hnd as [| h' rest' Hnotin Hndrest]; subst.
    destruct (Nat.eq_dec ghost h).
    + apply IH. exact Hndrest.
    + apply NoDup_cons.
      * intro Hin. apply in_remove in Hin. destruct Hin as [Hin _]. contradiction.
      * apply IH. exact Hndrest.
Qed.

Theorem ranked_tips_head :
  forall score ghost frontier,
    hd_error (ranked_tips score ghost frontier) = Some ghost.
Proof. intros. reflexivity. Qed.

Theorem ranked_tips_tail_exact :
  forall score ghost frontier,
    Permutation (tl (ranked_tips score ghost frontier))
                (remove Nat.eq_dec ghost frontier).
Proof.
  intros score ghost frontier. unfold ranked_tips. simpl.
  pose proof (sort_is_permutation
    (frontier_entries score (remove Nat.eq_dec ghost frontier))) as Hperm.
  apply Permutation_map with (f := ehash) in Hperm.
  rewrite map_ehash_frontier_entries in Hperm. exact Hperm.
Qed.

Theorem ranked_tips_tail_sorted :
  forall score ghost frontier,
    is_sorted
      (sort (frontier_entries score (remove Nat.eq_dec ghost frontier))) = true.
Proof. intros. apply sort_sorted. Qed.

Theorem terminal_frontier_confluent :
  forall d score is_scored root frontier1 frontier2,
    NoDup frontier1 -> NoDup frontier2 ->
    (forall h, In h frontier1 <-> terminal_reachable d is_scored root h) ->
    (forall h, In h frontier2 <-> terminal_reachable d is_scored root h) ->
    forall ghost,
      ranked_tips score ghost frontier1 = ranked_tips score ghost frontier2.
Proof.
  intros d score is_scored root frontier1 frontier2 Hnd1 Hnd2 Hexact1 Hexact2 ghost.
  assert (Hrem : Permutation (remove Nat.eq_dec ghost frontier1)
                             (remove Nat.eq_dec ghost frontier2)).
  { apply NoDup_Permutation.
    - apply nodup_remove_hash. exact Hnd1.
    - apply nodup_remove_hash. exact Hnd2.
    - intro h. split; intro Hin.
      + apply in_remove in Hin. destruct Hin as [Hin Hne].
        apply in_in_remove; [exact Hne |]. apply Hexact2. apply Hexact1. exact Hin.
      + apply in_remove in Hin. destruct Hin as [Hin Hne].
        apply in_in_remove; [exact Hne |]. apply Hexact1. apply Hexact2. exact Hin. }
  unfold ranked_tips. f_equal.
  assert (Hentries :
      Permutation
        (frontier_entries score (remove Nat.eq_dec ghost frontier1))
        (frontier_entries score (remove Nat.eq_dec ghost frontier2))).
  { unfold frontier_entries. apply Permutation_map. exact Hrem. }
  assert (Hndentries :
      NoDup
        (map ehash (frontier_entries score (remove Nat.eq_dec ghost frontier1)))).
  { rewrite map_ehash_frontier_entries. apply nodup_remove_hash. exact Hnd1. }
  rewrite (output_indep_of_input_perm
    (frontier_entries score (remove Nat.eq_dec ghost frontier1))
    (frontier_entries score (remove Nat.eq_dec ghost frontier2))
    Hndentries Hentries).
  reflexivity.
Qed.

Theorem ranked_ghost_frontier_correct :
  forall d score is_scored root,
    wf_dag d -> wf_lookup d -> NoDup (map blk_hash d) ->
    In root (map blk_hash d) -> is_scored root = true ->
    let ghost := rank d score is_scored (S (dag_max_num d)) root in
    In ghost (terminal_frontier d is_scored root)
    /\ NoDup (terminal_frontier d is_scored root)
    /\ hd_error (ranked_tips score ghost (terminal_frontier d is_scored root)) = Some ghost
    /\ Permutation
         (tl (ranked_tips score ghost (terminal_frontier d is_scored root)))
         (remove Nat.eq_dec ghost (terminal_frontier d is_scored root)).
Proof.
  intros d score is_scored root Hwf Hlookup Hnd Hreal Hscored ghost.
  split.
  - apply ghost_head_in_terminal_frontier; assumption.
  - split.
    + apply terminal_frontier_nodup. exact Hnd.
    + split.
      * apply ranked_tips_head.
      * apply ranked_tips_tail_exact.
Qed.
