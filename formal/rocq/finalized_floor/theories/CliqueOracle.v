(* ===========================================================================
   CliqueOracle.v - Agreeing sets, quorum finalization, and the two
   monotonicity lemmas the frontier cache rests on: L-ANC and L-SNAP.

   The finalized floor is derived by walking a parent's main-parent spine and
   asking, at each block, whether the clique oracle certifies it finalized over
   a frozen justification snapshot (`CliqueOracle::ft_witnessed`, clique_oracle.rs
   :437). The Phase-2 fix caches the per-block frontier and resolves later
   frontiers by an incremental UP-walk from that pivot instead of a full
   down-walk. That cache is only sound if it is TRANSPARENT: the warm up-walk
   must return the identical block the cold down-walk would. Transparency rests
   on two facts about finalization:

     L-ANC  (ancestor-monotone):  if a block is finalized then every ancestor
            of it on the spine is finalized too (over the same snapshot and the
            same committee). => finalized blocks form a downward-closed prefix,
            so "highest finalized" is well defined and the up-walk may stop at
            the first non-finalized block.

     L-SNAP (snapshot-monotone):  a block finalized over a snapshot J stays
            finalized over any larger snapshot J' \supseteq J. => the cached
            pivot F(parent) (finalized over parent's own snapshot) is still
            finalized over the child's larger snapshot, so it is a valid pivot.

   ---------------------------------------------------------------------------
   Faithful abstraction of the clique oracle
   ---------------------------------------------------------------------------
   `ft_witnessed(b,J) >= t` holds when a max-weight clique of validators that
   AGREE on `b` (each has `b` in the DAG-past of its latest message in J) carries
   > 1/2 of the committee's stake. We model finalization as:

       Finalized c J b  :=  there is a majority-weight sub-committee Q of c
                            every member of which agrees on b.

   A clique is a special such Q, so this is the clique rule with the pairwise-
   compatibility structure abstracted away. Crucially, L-ANC and L-SNAP hold by
   the SAME-QUORUM argument: the identical validators Q that finalized `b` also
   agree on every ancestor of `b` (they have `b`, hence its ancestors, in their
   past) and still agree under a larger snapshot. This is exactly WHY the lemmas
   are true for the real oracle -- the witnessing set is reused unchanged, so
   the pairwise-clique refinement carries over verbatim. Committee weights never
   enter these proofs (Q, its weight, and the total are all unchanged); only the
   `agrees` predicate moves, and it moves monotonically.

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                    | Paper / Spec            | Rust Implementation
   ------------------------+-------------------------+-------------------------
   anc_of                  | DAG ancestry <=         | is_dag_ancestor (bdkvs.rs:517)
   agrees                  | validator agrees on msg | agreeing_weight_map_f (clique:445)
   Finalized               | ft_witnessed >= t       | CliqueOracle::ft_witnessed (:437)
   L_ANC                   | L-ANC                   | floor.rs warm up-walk stop rule
   L_SNAP                  | L-SNAP                  | floor.rs warm pivot guard
   snap_extends            | just(B) \supseteq just(P)| child snapshot dominates parent
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Lia.
Import ListNotations.

From FinalizedFloor Require Import Foundation.

(* ===========================================================================
   Section 1 - DAG ancestry (general, over all parent edges)

   `walk_spine` (Foundation) descends only the MAIN-parent spine; agreement is
   about GENERAL DAG ancestry (a validator's latest message may DAG-descend from
   `b` via any parent path). `anc_of d a x` reads "a is an ancestor-or-self of x".
   =========================================================================== *)

Inductive anc_of (d : DAG) : BlockHash -> BlockHash -> Prop :=
  | anc_refl : forall x, anc_of d x x
  | anc_par  : forall a x b p,
      lookup d x = Some b ->
      In p (blk_parents b) ->
      anc_of d a p ->
      anc_of d a x.

(* Ancestry is transitive: standard for a reachability closure. *)
Lemma anc_of_trans :
  forall d a b c, anc_of d a b -> anc_of d b c -> anc_of d a c.
Proof.
  intros d a b c Hab Hbc.
  induction Hbc as [x | a' x bb p Hlook Hin Hanc IH].
  - exact Hab.
  - eapply anc_par; eauto.
Qed.

(* ===========================================================================
   Section 2 - Well-formed main parent, and: the main parent is an ancestor

   In a signed block, parents[0] is the main parent; Foundation carries it as a
   separate field `blk_main_parent`. `wf_mainparent` ties them together (the main
   parent is among the parents), which is all we need to conclude the main parent
   is a general DAG ancestor.
   =========================================================================== *)

Definition wf_mainparent (d : DAG) : Prop :=
  forall x b ph,
    lookup d x = Some b -> blk_main_parent b = Some ph -> In ph (blk_parents b).

Lemma mainparent_anc :
  forall d x b ph,
    wf_mainparent d ->
    lookup d x = Some b ->
    blk_main_parent b = Some ph ->
    anc_of d ph x.
Proof.
  intros d x b ph Hwf Hlook Hmp.
  eapply anc_par.
  - exact Hlook.
  - apply (Hwf x b ph Hlook Hmp).
  - apply anc_refl.
Qed.

(* ===========================================================================
   Section 3 - Justification snapshots and agreement
   =========================================================================== *)

Definition Snapshot := list (Validator * BlockHash).

Fixpoint snap_get (J : Snapshot) (v : Validator) : option BlockHash :=
  match J with
  | [] => None
  | (v', h) :: rest => if Nat.eqb v' v then Some h else snap_get rest v
  end.

(* J' extends J: every binding of J is preserved in J' (the child's snapshot
   dominates the parent's; new validators may be added). *)
Definition snap_extends (J' J : Snapshot) : Prop :=
  forall v h, snap_get J v = Some h -> snap_get J' v = Some h.

Lemma snap_extends_refl : forall J, snap_extends J J.
Proof. intros J v h H. exact H. Qed.

Lemma snap_extends_trans :
  forall J1 J2 J3, snap_extends J2 J1 -> snap_extends J3 J2 -> snap_extends J3 J1.
Proof. intros J1 J2 J3 H12 H23 v h H. apply H23, H12, H. Qed.

(* A validator agrees on `b` (over snapshot J) iff `b` is a DAG-ancestor of the
   validator's latest message in J. *)
Definition agrees (d : DAG) (J : Snapshot) (v : Validator) (b : BlockHash) : Prop :=
  exists h, snap_get J v = Some h /\ anc_of d b h.

(* Agreement is monotone downward along ancestry: if `b'` is an ancestor of `b`
   and v agrees on `b`, then v agrees on `b'` (it has `b`, hence `b'`, in past). *)
Lemma agrees_anc_mono :
  forall d J v b b', anc_of d b' b -> agrees d J v b -> agrees d J v b'.
Proof.
  intros d J v b b' Hanc [h [Hh Hbh]].
  exists h. split; [exact Hh |].
  eapply anc_of_trans; [exact Hanc | exact Hbh].
Qed.

(* Agreement is monotone under snapshot growth. *)
Lemma agrees_snap_mono :
  forall d J J' v b, snap_extends J' J -> agrees d J v b -> agrees d J' v b.
Proof.
  intros d J J' v b Hext [h [Hh Hbh]].
  exists h. split; [apply Hext; exact Hh | exact Hbh].
Qed.

(* ===========================================================================
   Section 4 - Committees, quorums, and finalization
   =========================================================================== *)

Definition Committee := list (Validator * nat).

Fixpoint cweight (c : Committee) : nat :=
  match c with
  | [] => 0
  | (_, w) :: rest => w + cweight rest
  end.

(* Q is a majority-weight sub-committee of c. `incl` keeps the proof of L-ANC/
   L-SNAP weight-free: we reuse the SAME Q, so its inclusion and weight are
   unchanged and never need re-derivation. *)
Definition is_quorum (c Q : Committee) : Prop :=
  incl Q c /\ 2 * cweight Q > cweight c.

(* Finalization: some majority-weight sub-committee all agree on `b`. Faithful
   monotone abstraction of `ft_witnessed(b,J) >= t` (a clique is such a Q). *)
Definition Finalized (d : DAG) (c : Committee) (J : Snapshot) (b : BlockHash) : Prop :=
  exists Q, is_quorum c Q /\ (forall v w, In (v, w) Q -> agrees d J v b).

(* ===========================================================================
   Section 5 - L-ANC : ancestor-monotone finalization

   The SAME quorum that finalizes `b` finalizes every ancestor `b'` of `b`: each
   member agrees on `b`, hence on `b'` (agrees_anc_mono). Committee and weights
   untouched. This is the downward-closure that makes the warm up-walk's
   stop-at-first-non-finalized rule equal to the cold down-walk.
   =========================================================================== *)

Theorem L_ANC :
  forall d c J b b',
    anc_of d b' b -> Finalized d c J b -> Finalized d c J b'.
Proof.
  intros d c J b b' Hanc [Q [Hq Hag]].
  exists Q. split; [exact Hq |].
  intros v w Hin.
  eapply agrees_anc_mono; [exact Hanc | apply (Hag v w Hin)].
Qed.

(* Spine specialization: a finalized block's main parent is finalized. This is
   the exact step the warm up-walk relies on when advancing/stopping. *)
Corollary L_ANC_mainparent :
  forall d c J x b ph,
    wf_mainparent d ->
    lookup d x = Some b ->
    blk_main_parent b = Some ph ->
    Finalized d c J x ->
    Finalized d c J ph.
Proof.
  intros d c J x b ph Hwf Hlook Hmp Hfin.
  eapply L_ANC; [eapply mainparent_anc; eauto | exact Hfin].
Qed.

(* ===========================================================================
   Section 6 - L-SNAP : snapshot-monotone finalization

   The SAME quorum finalizes `b` over any larger snapshot: each member still
   agrees (agrees_snap_mono). This is the pivot guard's justification: F(parent),
   finalized over parent's own snapshot, remains finalized over the child's
   larger snapshot, so it is a sound up-walk pivot.
   =========================================================================== *)

Theorem L_SNAP :
  forall d c J J' b,
    snap_extends J' J -> Finalized d c J b -> Finalized d c J' b.
Proof.
  intros d c J J' b Hext [Q [Hq Hag]].
  exists Q. split; [exact Hq |].
  intros v w Hin.
  eapply agrees_snap_mono; [exact Hext | apply (Hag v w Hin)].
Qed.

(* Combined: finalization is monotone in BOTH arguments at once - the exact
   shape the frontier cache uses (a lower ancestor over a larger snapshot). *)
Corollary L_ANC_SNAP :
  forall d c J J' b b',
    snap_extends J' J -> anc_of d b' b ->
    Finalized d c J b -> Finalized d c J' b'.
Proof.
  intros d c J J' b b' Hext Hanc Hfin.
  eapply L_SNAP; [exact Hext |].
  eapply L_ANC; [exact Hanc | exact Hfin].
Qed.
