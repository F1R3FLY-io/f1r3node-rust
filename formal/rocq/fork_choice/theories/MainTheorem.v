(* ===========================================================================
   MainTheorem.v - Capstone: the LMD-GHOST fork-choice estimator is correct.

   Four end-to-end statements, each a conjunction discharged by `exact` against
   the already-proven, axiom-free module lemmas, so the capstones introduce NO
   new assumptions (verify with `Print Assumptions` - all Closed under the global
   context). The four axes and what each rules out:

     fork_choice_determinism_correct  (S1 - no fork from enumeration order)
       (a) sort_total_order          - the (score DESC, hash ASC) tie-break is a
                                        STRICT TOTAL order on distinct hashes;
       (b) output_indep_of_input_perm - so permuted candidate lists sort to the
                                        IDENTICAL ranked output (node-identical);
       (c) weight_is_pure            - a validator's weight is pure in the
                                        resolved main parent (node-identical);
       (d) lca_depth_filter_deterministic - the LCA cutoff is a pure function.

     fork_choice_ghost_correct        (heaviest-subtree / GHOST correctness)
       (a) rank_selects_heaviest     - each descent step picks the argmax child;
       (b) score_perm_invariant      - the score is order-independent (no fork);
       (c) score_eq_support_sum      - score = cumulative supporter weight;
       (d) lca_is_common_ancestor    - the descent starts at a common ancestor.

     fork_choice_bound_correct        (parent-count/-depth truncation - B2/P2-8)
       (a) head_preserved            - a cap keeps the ghost main parent (head);
       (b) take_never_drops_head     - firstn never drops the head;
       (c) empty_tips_typed_err      - empty tips is a typed error, not a panic;
       (d) invalid_excluded          - slashed validators excluded (T-10).

     fork_choice_bridge_correct       (Rocq assumes what Rust enforces)
       (a) validation_implies_wf_dag - validation ENFORCES the well-formed DAG;
       (b) honest_forkchoice_parents_validate - honest parents pass validation
                                        (validators range-check, don't recompute);
       (c) main_parent_first_deterministic - main-parent ordering is a total order;
       (d) rank_terminates           - the fork-choice descent always halts.

   A `Recovery`-analog capstone (finalized_floor's T-NDA) is intentionally absent:
   fork choice is a stateless re-derivation each round (no effect application).
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

From ForkChoice Require Import Foundation.
From ForkChoice Require Import Score.
From ForkChoice Require Import Filter.
From ForkChoice Require Import TieBreak.
From ForkChoice Require Import Lca.
From ForkChoice Require Import Rank.
From ForkChoice Require Import Bound.
From ForkChoice Require Import GuardBridge.

(* ===========================================================================
   Capstone 1 - DETERMINISM (S1: every honest node ranks tips identically)
   =========================================================================== *)

Theorem fork_choice_determinism_correct :
  (* (a) the tie-break order is a STRICT TOTAL order on distinct hashes *)
  ((forall a, ~ ord a a)
   /\ (forall a b c, ord a b -> ord b c -> ord a c)
   /\ (forall a b, ehash a <> ehash b -> ord a b \/ ord b a))
  /\
  (* (b) permuted candidate lists sort to the IDENTICAL output (no fork) *)
  (forall l l', NoDup (map ehash l) -> Permutation l l' -> sort l = sort l')
  /\
  (* (c) a validator's weight is pure in the resolved main parent *)
  (forall d1 d2 h v, resolve_main d1 h = resolve_main d2 h -> weight d1 h v = weight d2 h v)
  /\
  (* (d) the LCA depth cutoff is a deterministic function of the tip *)
  (forall d top1 top2 lms, top1 = top2 -> depth_filter d top1 lms = depth_filter d top2 lms).
Proof.
  exact (conj sort_total_order
          (conj output_indep_of_input_perm
            (conj weight_is_pure lca_depth_filter_deterministic))).
Qed.

(* ===========================================================================
   Capstone 2 - GHOST (heaviest-subtree selection over the score fold)
   =========================================================================== *)

Theorem fork_choice_ghost_correct :
  (* (a) each descent step selects the heaviest (argmax) scored child (GHOST) *)
  (forall d score is_scored h c,
     NoDup (scored_children d is_scored h) ->
     best_child d score is_scored h = Some c ->
     In c (scored_children d is_scored h) /\
     (forall c', In c' (scored_children d is_scored h) ->
        c' = c \/ ord (score c, c) (score c', c')))
  /\
  (* (b) the score is invariant under latest-message fold order (no fork) *)
  (forall d fuel lms lms' b,
     Permutation lms lms' -> build_scores d fuel lms b = build_scores d fuel lms' b)
  /\
  (* (c) the score equals the sum of the supporting validators' weights *)
  (forall d fuel lms b,
     build_scores d fuel lms b
     = fold_right Nat.add 0
         (map (fun e => weight d b (fst e))
              (filter (fun e => anc_ofb d fuel b (snd e)) lms)))
  /\
  (* (d) the LCA is a common ancestor of every filtered latest message. THREE
     premises have been discharged into the fold model: the old `lcua_common`
     "assume the output is a common ancestor" hypothesis (now DERIVED via the
     covering invariant), the fold's TERMINATION (Lca.reduce_converges, a
     lexicographic (max_numof, count_at_max) measure), and `common_ancestor …
     root` itself (now DERIVED from single_root + wf_dag + all_real via
     Lca.descends_from_root). Only `single_root` + `all_real` remain — both
     NECESSARY (a 2-genesis DAG makes even the Rust fold return a
     non-common-ancestor; see Lca.v's RESIDUAL note). *)
  (forall genesis root d top lms,
     wf_dag d -> wf_lookup d ->
     single_root d root ->
     all_real d (map snd (depth_filter d top lms)) ->
     forall lm, In lm (depth_filter d top lms) ->
       anc_of d (lca genesis (lcua_many d (map snd (depth_filter d top lms))) d top lms) (snd lm)).
Proof.
  exact (conj rank_selects_heaviest
          (conj score_perm_invariant
            (conj score_eq_support_sum lca_is_common_ancestor))).
Qed.

(* ===========================================================================
   Capstone 3 - BOUND (parent-count/-depth truncation keeps the main parent)
   =========================================================================== *)

Theorem fork_choice_bound_correct :
  (* (a) an admissible cap keeps the ghost main parent (head) *)
  (forall (A : Type) (n : nat) (unlimited : bool) (l : list A),
     (unlimited = true \/ 1 <= n) -> hd_error (cap_tips n unlimited l) = hd_error l)
  /\
  (* (b) firstn never drops the head *)
  (forall (A : Type) (k : nat) (x : A) (xs : list A),
     firstn (S k) (x :: xs) = x :: firstn k xs)
  /\
  (* (c) capping [] is [] and flags the typed error (P2-8) *)
  (forall (A : Type) (n : nat) (unlimited : bool),
     cap_tips n unlimited (@nil A) = []
     /\ tips_error (cap_tips n unlimited (@nil A)) = true)
  /\
  (* (d) a slashed validator's latest message is excluded (T-10) *)
  (forall lms inv v h, In v inv -> ~ In (v, h) (filter_inv lms inv)).
Proof.
  exact (conj head_preserved
          (conj take_never_drops_head
            (conj empty_tips_typed_err invalid_excluded))).
Qed.

(* ===========================================================================
   Capstone 4 - BRIDGE (Rocq assumes what Rust enforces)
   =========================================================================== *)

Theorem fork_choice_bridge_correct :
  (* (a) validation ENFORCES the well-formed DAG *)
  (forall d, (forall b, In b d -> validated_block d b) -> wf_dag d)
  /\
  (* (b) honest depth-filtered parents pass the validator's buffered check *)
  (forall maxn mpd buf nums,
     parents_ok maxn (mpd + buf) (prop_filter maxn mpd nums) = true)
  /\
  (* (c) main-parent-first parent ordering is deterministic (total order) *)
  (forall main (l l' : list BlockHash),
     NoDup l -> Permutation l l' ->
     sort (map (parent_entry main) l) = sort (map (parent_entry main) l'))
  /\
  (* (d) the fork-choice descent terminates at a tip *)
  (forall d score is_scored h,
     wf_dag d -> wf_lookup d ->
     best_child d score is_scored (rank d score is_scored (S (dag_max_num d)) h) = None).
Proof.
  exact (conj validation_implies_wf_dag
          (conj honest_forkchoice_parents_validate
            (conj main_parent_first_deterministic rank_terminates))).
Qed.
