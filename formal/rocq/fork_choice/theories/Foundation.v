(* ===========================================================================
   Foundation.v - DAG, block numbers, parents/main-parent, ancestry, height.

   Models the immutable structural facts a fork-choice round reads: each block's
   number, its main parent (parents[0] = the GHOST-chosen edge), its full parent
   list, and its recorded bond/weight map. The estimator (estimator.rs) walks
   these edges two ways:

     * DOWN a validator's supporting chain (build_scores_map) to accumulate
       weight - modeled by `anc_of` (general DAG ancestry over `blk_parents`);
     * UP from the LCA toward the tips (rank_forkchoices) picking the heaviest
       scored child at each level - whose termination rests on block numbers
       being a finite height (theorem `dag_height_bound` here).

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq Definition   | Paper / Spec Notation      | Rust Implementation
   ------------------+----------------------------+--------------------------
   Block             | B in the DAG               | BlockMetadata / BlockMessage
   blk_num           | num(B)                     | meta.block_number
   blk_main_parent   | main-parent, spine <|      | meta.parents[0]
   blk_parents       | parents(B), DAG <=         | meta.parents
   blk_bonds         | bonds/weight map of B      | meta.weight_map (proto bonds)
   anc_of            | DAG ancestry a <= x        | supporting-chain BFS (estimator.rs:257)
   numof / dag_max_num | height / max height      | block_number / latest_block_number
   dag_height_bound  | finite height             | rank_forkchoices upward-walk bound
   ---------------------------------------------------------------------------

   Companion: casper/src/rust/estimator.rs, util/proto_util.rs.
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Arith.Wf_nat.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
Import ListNotations.

Set Implicit Arguments.

(* ===========================================================================
   Section 1 - Blocks and the DAG
   =========================================================================== *)

Definition BlockHash := nat.
Definition Validator := nat.

Record Block : Type := mkBlock {
  blk_hash        : BlockHash;
  blk_num         : nat;
  blk_main_parent : option BlockHash;              (* None = genesis; else parents[0] *)
  blk_parents     : list BlockHash;                (* index 0 = main parent *)
  blk_bonds       : list (Validator * nat)         (* the block's bond/weight map *)
}.

(* A DAG is a finite list of blocks; lookup is by hash. *)
Definition DAG := list Block.

Fixpoint lookup (d : DAG) (h : BlockHash) : option Block :=
  match d with
  | [] => None
  | b :: rest => if Nat.eqb (blk_hash b) h then Some b else lookup rest h
  end.

Lemma lookup_In :
  forall d h b, lookup d h = Some b -> In b d.
Proof.
  induction d as [| x rest IH]; simpl; intros h b Hl.
  - discriminate.
  - destruct (Nat.eqb (blk_hash x) h) eqn:E.
    + inversion Hl; subst. left; reflexivity.
    + right; eapply IH; eauto.
Qed.

(* The looked-up block carries the queried hash. *)
Lemma lookup_hash :
  forall d h b, lookup d h = Some b -> blk_hash b = h.
Proof.
  induction d as [| x rest IH]; simpl; intros h b Hl.
  - discriminate.
  - destruct (Nat.eqb (blk_hash x) h) eqn:E.
    + inversion Hl; subst. apply Nat.eqb_eq in E. exact E.
    + eapply IH; eauto.
Qed.

(* ===========================================================================
   Section 2 - Height by hash, and the maximum height of a DAG
   =========================================================================== *)

Definition numof (d : DAG) (h : BlockHash) : nat :=
  match lookup d h with
  | Some b => blk_num b
  | None => 0
  end.

Fixpoint dag_max_num (d : DAG) : nat :=
  match d with
  | [] => 0
  | b :: rest => Nat.max (blk_num b) (dag_max_num rest)
  end.

(* T-HEIGHT: every block's number is bounded by the DAG's maximum height. This
   is the concrete termination bound for the LMD upward walk (Rank.v): each
   ascent step strictly increases the block number, which cannot exceed this. *)
Lemma dag_height_bound :
  forall d b, In b d -> blk_num b <= dag_max_num d.
Proof.
  induction d as [| x rest IH]; simpl; intros b Hin.
  - contradiction.
  - destruct Hin as [Heq | Hin].
    + subst x. apply Nat.le_max_l.
    + eapply Nat.le_trans; [apply IH; exact Hin | apply Nat.le_max_r].
Qed.

(* Consequence in the `numof` view: no resolved height exceeds the maximum. *)
Lemma numof_le_max :
  forall d h, numof d h <= dag_max_num d.
Proof.
  intros d h. unfold numof. destruct (lookup d h) as [b |] eqn:E.
  - apply dag_height_bound. eapply lookup_In; eauto.
  - apply Nat.le_0_l.
Qed.

(* ===========================================================================
   Section 3 - Well-formed DAG: parents are older, main parent is the head

   Structural facts of a signed, validated DAG. Every parent is present with a
   strictly smaller number (parents are older); the main parent (when present)
   is the head of the parent list and likewise present-and-older; genesis (no
   main parent) has number 0. `validate.rs::block_number` (:703) ENFORCES this
   along every edge (num child = 1 + max parent num) - the derivation from that
   validation predicate is GuardBridge.validation_implies_wf_dag.
   =========================================================================== *)

Definition wf_dag (d : DAG) : Prop :=
  forall b, In b d ->
    (forall ph, In ph (blk_parents b) ->
        exists p, lookup d ph = Some p /\ blk_num p < blk_num b)
    /\ match blk_main_parent b with
       | None    => blk_num b = 0
       | Some mph =>
           (exists p, lookup d mph = Some p /\ blk_num p < blk_num b)
           /\ hd_error (blk_parents b) = Some mph
       end.

(* Hash-collision-freedom: a block is found by its own hash (cryptographic
   digests do not collide). Kept SEPARATE from wf_dag because only the upward
   ascent's strict-increase step (Rank.child_num_gt) needs it. *)
Definition wf_lookup (d : DAG) : Prop :=
  forall b, In b d -> lookup d (blk_hash b) = Some b.

Lemma numof_blk :
  forall d b, wf_lookup d -> In b d -> numof d (blk_hash b) = blk_num b.
Proof.
  intros d b Hwf Hin. unfold numof. rewrite (Hwf b Hin). reflexivity.
Qed.

(* ===========================================================================
   Section 4 - DAG ancestry (reflexive-transitive closure over blk_parents)

   `anc_of d a x` reads "a is an ancestor-or-self of x": a is reachable from x
   by following parent edges. This is the relation the score BFS descends
   (a validator's latest message supports every block in its DAG-past).
   =========================================================================== *)

Inductive anc_of (d : DAG) : BlockHash -> BlockHash -> Prop :=
  | anc_refl : forall x, anc_of d x x
  | anc_par  : forall a x b p,
      lookup d x = Some b ->
      In p (blk_parents b) ->
      anc_of d a p ->
      anc_of d a x.

Lemma anc_of_refl : forall d x, anc_of d x x.
Proof. intros; apply anc_refl. Qed.

Lemma anc_of_trans :
  forall d a b c, anc_of d a b -> anc_of d b c -> anc_of d a c.
Proof.
  intros d a b c Hab Hbc.
  induction Hbc as [x | a' x bb p Hlook Hin Hanc IH].
  - exact Hab.
  - eapply anc_par; eauto.
Qed.

(* An ancestor never has a larger number than its descendant (parents are
   older). This is the monotonicity the LCA-cutoff argument rests on. *)
Lemma anc_of_num_le :
  forall d, wf_dag d -> forall a x, anc_of d a x -> numof d a <= numof d x.
Proof.
  intros d Hwf a x H. induction H as [x | a x b p Hlk Hin Hanc IH].
  - apply Nat.le_refl.
  - (* lookup d x = Some b, In p (blk_parents b), anc_of d a p, IH: numof a <= numof p *)
    assert (Hinb : In b d) by (eapply lookup_In; eauto).
    destruct (proj1 (Hwf b Hinb) p Hin) as [p' [Hlp Hlt]].
    (* numof d p = blk_num p' ; numof d x = blk_num b *)
    assert (Hnp : numof d p = blk_num p') by (unfold numof; rewrite Hlp; reflexivity).
    assert (Hnx : numof d x = blk_num b) by (unfold numof; rewrite Hlk; reflexivity).
    lia.
Qed.

(* ===========================================================================
   Section 5 - Decidable boolean ancestry (fuel-bounded), sound wrt anc_of

   Models is_dag_ancestor / the supporting-chain membership test. We need only
   SOUNDNESS (true => anc_of), which holds for any fuel; used by Score.v to
   decide whether a block lies on a validator's supporting chain.
   =========================================================================== *)

Fixpoint anc_ofb (d : DAG) (fuel : nat) (a x : BlockHash) : bool :=
  if Nat.eqb a x then true
  else match fuel with
       | 0 => false
       | S f =>
           match lookup d x with
           | None => false
           | Some b => existsb (fun p => anc_ofb d f a p) (blk_parents b)
           end
       end.

Lemma anc_ofb_sound :
  forall d fuel a x, anc_ofb d fuel a x = true -> anc_of d a x.
Proof.
  intros d fuel. induction fuel as [| f IH]; intros a x H; simpl in H.
  - destruct (Nat.eqb a x) eqn:E.
    + apply Nat.eqb_eq in E. subst x. apply anc_refl.
    + discriminate.
  - destruct (Nat.eqb a x) eqn:E.
    + apply Nat.eqb_eq in E. subst x. apply anc_refl.
    + destruct (lookup d x) as [b |] eqn:Elk; [| discriminate].
      apply existsb_exists in H. destruct H as [p [Hin Hp]].
      eapply anc_par; [exact Elk | exact Hin | apply IH; exact Hp].
Qed.
