(* ===========================================================================
   Lca.v - The lowest universal common ancestor (LCUA) and the depth cutoff.

   Fork choice starts its upward rank from the LCA of the (filtered) latest
   messages (estimator.rs:96, calculate_lca -> rank_forkchoices([lca])). The LCA
   must be a COMMON ANCESTOR of every latest message, else the upward walk would
   start off-chain and miss reachable tips. `calculate_lca` (estimator.rs:184):

     * filters out latest messages older than LATEST_MESSAGE_MAX_DEPTH (=1000)
       below the DAG tip (`msg.block_number > top - 1000`), then
     * returns genesis if none remain, else
       DagOperations::lowest_universal_common_ancestor_many(...).

   We model faithfully what the fork choice DEPENDS ON:

     * `depth_filter`  - the concrete `> top - 1000` cutoff (exactly reproduced),
                         and its determinism in `top` (`lca_depth_filter_deterministic`);
     * `lca`           - LCUA-many with the empty->genesis fallback wired in;
     * `lca_is_common_ancestor` - the LCUA is a common ancestor of the filtered
                         messages (the LCUA-many CONTRACT, supplied as a premise
                         reflecting the Rust computation - the "Rocq assumes what
                         Rust enforces" seam; genesis is used when empty);
     * `lca_below_has_zero_rank_influence` - a message strictly below a block is
                         NOT in that block's past, so it never scores it.

   RESIDUAL: the interior LCUA-many computation (pairwise LCA fold) is abstracted
   to its defining CONTRACT (`lcua_common`, a hypothesis) rather than recomputed;
   likewise LOWEST-ness (that no lower common ancestor exists) is NOT modeled -
   it affects only the DEPTH of the score BFS (a scope/efficiency concern), never
   the common-ancestor invariant or fork-choice determinism. The strongest
   fork-choice-relevant property - "lca is a common ancestor" - IS proved.

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                            | Rust (casper/src/rust/estimator.rs)
   --------------------------------+-------------------------------------------
   depth_filter (> top - 1000)     | filter msg.block_number > top - 1000 (:201)
   lca (empty -> genesis)          | calculate_lca (:204-210)
   lca_is_common_ancestor          | lowest_universal_common_ancestor_many (:207)
   lca_below_has_zero_rank_influence| hash_parents cutoff below LCA (:230)
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Lia.
Import ListNotations.

From ForkChoice Require Import Foundation.

(* The DAG tip height minus the max look-back window (LATEST_MESSAGE_MAX_DEPTH). *)
Definition LATEST_MESSAGE_MAX_DEPTH : nat := 1000.

(* Keep only latest messages whose block sits within the look-back window of the
   tip: `numof(msg) > top - 1000`. Exactly estimator.rs:201. *)
Definition depth_filter (d : DAG) (top : nat) (lms : list (Validator * BlockHash))
  : list (Validator * BlockHash) :=
  filter (fun e => Nat.ltb (top - LATEST_MESSAGE_MAX_DEPTH) (numof d (snd e))) lms.

(* The LCA: the LCUA-many result `lcua` when the filtered set is nonempty, else
   genesis (estimator.rs:204). *)
Definition lca (genesis lcua : BlockHash) (d : DAG) (top : nat)
               (lms : list (Validator * BlockHash)) : BlockHash :=
  match depth_filter d top lms with
  | [] => genesis
  | _  => lcua
  end.

(* The depth cutoff is a pure function of `top`: equal tips give equal filters
   (determinism - two nodes with the same tip filter identically). *)
Theorem lca_depth_filter_deterministic :
  forall d top1 top2 lms,
    top1 = top2 -> depth_filter d top1 lms = depth_filter d top2 lms.
Proof. intros d top1 top2 lms H. subst. reflexivity. Qed.

(* When the filtered set is empty, the LCA is genesis (estimator.rs:204). *)
Theorem lca_empty_is_genesis :
  forall genesis lcua d top lms,
    depth_filter d top lms = [] -> lca genesis lcua d top lms = genesis.
Proof.
  intros genesis lcua d top lms H. unfold lca. rewrite H. reflexivity.
Qed.

(* The LCA is a common ancestor of every (filtered) latest message. For the
   nonempty case this is the LCUA-many contract `lcua_common` (the Rust
   computation guarantees it); the empty case never arises here since a member
   of `depth_filter` witnesses nonemptiness. *)
Theorem lca_is_common_ancestor :
  forall genesis lcua d top lms,
    (forall e, In e (depth_filter d top lms) -> anc_of d lcua (snd e)) ->
    forall lm, In lm (depth_filter d top lms) ->
      anc_of d (lca genesis lcua d top lms) (snd lm).
Proof.
  intros genesis lcua d top lms Hlcua lm Hin.
  unfold lca.
  destruct (depth_filter d top lms) as [| e0 rest] eqn:E.
  - (* empty contradicts membership (Hin : In lm []) *)
    destruct Hin.
  - (* nonempty: lca = lcua, apply the LCUA-many contract *)
    apply Hlcua. exact Hin.
Qed.

(* A latest message strictly BELOW a block never lies in that block's past, so
   it contributes nothing to the block's fork-choice score (Score.contrib is 0).
   This is why messages dropped by the LCA cutoff cannot influence any ranked
   (higher) candidate. Stated at the Foundation ancestry level (the strongest
   form independent of Score): num(h) < num(b) => b is not an ancestor of h. *)
Theorem lca_below_has_zero_rank_influence :
  forall d b h, wf_dag d -> numof d h < numof d b -> ~ anc_of d b h.
Proof.
  intros d b h Hwf Hlt Hanc.
  pose proof (anc_of_num_le Hwf Hanc) as Hle.   (* numof d b <= numof d h *)
  lia.
Qed.
