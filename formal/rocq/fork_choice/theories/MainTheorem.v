(* ===========================================================================
   MainTheorem.v - Capstone: the LMD-GHOST fork-choice estimator is correct.

   Seven end-to-end statements, each a conjunction discharged by `exact` against
   the already-proven, axiom-free module lemmas, so the capstones introduce NO
   new assumptions (verify with `Print Assumptions` - all Closed under the global
   context). The seven axes and what each rules out:

     fork_choice_determinism_correct  (S1 - no fork from enumeration order)
       (a) sort_total_order          - the (score DESC, hash ASC) tie-break is a
                                        STRICT TOTAL order on distinct hashes;
       (b) output_indep_of_input_perm - so permuted candidate lists sort to the
                                        IDENTICAL ranked output (node-identical);
       (c) candidate_bonds_noninterference - authority is frozen at the
                                        certified finalized floor;
       (d) certified_messages_receiver_state_irrelevant - receiver-local head
                                        height cannot remove certified votes;
       (e) receiver_cache_noninterference - receiver invalid caches cannot
                                        re-filter certified votes.

     fork_choice_certified_context_correct
       complete exact slots, floor-descending vote projection, fail-closed
       incomplete contexts, and receiver-state noninterference.

     fork_choice_parent_antichain_correct
       complete reachability-maximal compaction and explicit coverage of every
       retained live causal tip.

     fork_choice_ghost_correct        (heaviest-subtree / GHOST correctness)
       (a) rank_selects_heaviest     - each descent step picks the argmax child;
       (b) score_perm_invariant      - the score is order-independent (no fork);
       (c) score_eq_support_sum      - score = cumulative supporter weight;
       (d) lca_is_common_ancestor_validated - the descent starts at a common
                                        ancestor, with `single_root` DERIVED from
                                        block validation (no longer assumed).

     fork_choice_terminal_frontier_correct
       exact duplicate-free terminal enumeration and expansion-order
       confluence, composed behind the greedy GHOST head.

     fork_choice_bound_correct        (parent-count/-depth truncation - B2/P2-8)
       (a) head_preserved            - a cap keeps the ghost main parent (head);
       (b) take_never_drops_head     - firstn never drops the head;
       (c) empty_tips_typed_err      - empty tips is a typed error, not a panic;
       (d) invalid_excluded          - slashed validators excluded (T-10);
       (e) depth_filter_preserves_head - expiry cannot replace the main parent;
       (f) configured capacity carries active validators plus floor backstop;
       (g) every undersized cap has a blocked-frontier witness.

     fork_choice_bridge_correct       (Rocq assumes what Rust enforces)
       (a) validation_implies_wf_dag - validation ENFORCES the well-formed DAG;
       (b) honest_forkchoice_parents_validate - honest parents pass validation
                                        (validators range-check, don't recompute);
       (c) ghost_sort_first_deterministic - the (is_main DESC, hash
                                        ASC) parent sort is a total order, hence
                                        permutation-invariant (`snapshot.rs:325-331`);
       (d) rank_terminates           - the fork-choice descent always halts;
       (e) consensus_parent_pipeline_deterministic - the complete production
                                        parent ordering is permutation-invariant;
       (f) consensus_parent_pipeline_preserves_ghost_head - when the selected
                                        GHOST head is in the causal parent set,
                                        it remains the block's main parent.

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
From ForkChoice Require Import CertifiedContext.
From ForkChoice Require Import TieBreak.
From ForkChoice Require Import Lca.
From ForkChoice Require Import Rank.
From ForkChoice Require Import TerminalFrontier.
From ForkChoice Require Import Bound.
From ForkChoice Require Import ParentAntichain.
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
  (* (c) candidate and traversed-block bonds cannot reweight a round *)
  (forall authority candidate_bonds1 candidate_bonds2 v,
     estimator_weight authority candidate_bonds1 v =
     estimator_weight authority candidate_bonds2 v)
  /\
  (* (d) every receiver retains the same certified vote projection *)
  (forall d top1 top2 lms,
     certified_messages d top1 lms = certified_messages d top2 lms)
  /\
  (* (e) receiver-local invalid caches cannot alter the certified projection *)
  (forall lms certified_exclusions receiver_cache1 receiver_cache2,
     projection_with_receiver_cache lms certified_exclusions receiver_cache1 =
     projection_with_receiver_cache lms certified_exclusions receiver_cache2).
Proof.
  exact (conj sort_total_order
          (conj output_indep_of_input_perm
            (conj candidate_bonds_noninterference
              (conj certified_messages_receiver_state_irrelevant
                    receiver_cache_noninterference)))).
Qed.

Theorem fork_choice_certified_context_correct :
  (forall active exact,
     complete_slots active exact = true ->
     forall v, In v active -> exists h, In (v, h) exact)
  /\
  (forall eligible descends exact v h,
     In (v, h) (project_floor_descendants eligible descends exact) ->
     descends h = true)
  /\
  (forall eligible descends exact v h,
     descends h = false ->
     ~ In (v, h) (project_floor_descendants eligible descends exact))
  /\
  (forall active exact,
     complete_slots active exact = false ->
     fork_choice_ready active exact = false)
  /\
  (forall eligible descends exact latest1 latest2 invalid1 invalid2
          finalized1 finalized2 top1 top2,
     projection_with_receiver_state eligible descends exact
       latest1 invalid1 finalized1 top1 =
     projection_with_receiver_state eligible descends exact
       latest2 invalid2 finalized2 top2).
Proof.
  exact (conj complete_slots_sound
          (conj floor_projection_sound
            (conj outside_floor_excluded
              (conj incomplete_slots_fail_closed receiver_state_noninterference)))).
Qed.

Theorem fork_choice_parent_antichain_correct :
  (forall ancestorb parents left right,
    In left (reachability_maximal_antichain ancestorb parents) ->
    In right (reachability_maximal_antichain ancestorb parents) ->
    left <> right ->
    ancestorb left right = false)
  /\
  (forall ancestorb tips candidates,
    causal_coverageb
      ancestorb
      tips
      (reachability_maximal_antichain ancestorb candidates) = true ->
    forall tip,
      In tip tips ->
      exists parent,
        In parent (reachability_maximal_antichain ancestorb candidates) /\
        (tip = parent \/ ancestorb tip parent = true)).
Proof.
  exact
    (conj retained_parents_are_pairwise_uncovered
          compaction_and_coverage_guard_preserve_every_causal_tip).
Qed.

Print Assumptions fork_choice_parent_antichain_correct.

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
  (forall authority d fuel lms lms' b,
     Permutation lms lms' ->
     build_scores authority d fuel lms b =
     build_scores authority d fuel lms' b)
  /\
  (* (c) the score equals the sum of the supporting validators' weights *)
  (forall authority d fuel lms b,
     build_scores authority d fuel lms b
     = fold_right Nat.add 0
         (map (fun e => weight authority (fst e))
              (filter (fun e => anc_ofb d fuel b (snd e)) lms)))
  /\
  (* (d) the LCA is a common ancestor of every certified latest message. FOUR
     premises have been discharged into the model: the old `lcua_common`
     "assume the output is a common ancestor" hypothesis (now DERIVED via the
     covering invariant), the fold's TERMINATION (Lca.reduce_converges, a
     lexicographic (max_numof, count_at_max) measure), `common_ancestor … root`
     itself (DERIVED from single_root + wf_dag + all_real via
     Lca.descends_from_root), and now `single_root` ITSELF — no longer an
     assumed premise but DERIVED from block validation via
     GuardBridge.validation_implies_single_root (the approved-genesis pin
     modeling validate.rs::justification_follows, which rejects every other
     parentless block as InvalidParents). The clause therefore takes the
     Rust-enforced validation predicate `(forall b, In b d -> validated_block
     genesis_hash d b)` instead of a bare `single_root`; only `all_real`
     remains as a DAG-shape premise. *)
  (forall genesis genesis_hash d top lms,
     wf_dag d -> wf_lookup d ->
     (forall b, In b d -> validated_block genesis_hash d b) ->
     all_real d (map snd (certified_messages d top lms)) ->
     forall lm, In lm (certified_messages d top lms) ->
       anc_of d (lca genesis (lcua_many d (map snd (certified_messages d top lms))) d top lms) (snd lm)).
Proof.
  exact (conj rank_selects_heaviest
          (conj score_perm_invariant
            (conj score_eq_support_sum lca_is_common_ancestor_validated))).
Qed.

Theorem fork_choice_terminal_frontier_correct :
  (forall d score is_scored root,
    wf_dag d -> wf_lookup d -> NoDup (map blk_hash d) ->
    In root (map blk_hash d) -> is_scored root = true ->
    let ghost := rank d score is_scored (S (dag_max_num d)) root in
    In ghost (terminal_frontier d is_scored root)
    /\ NoDup (terminal_frontier d is_scored root)
    /\ hd_error (ranked_tips score ghost (terminal_frontier d is_scored root)) = Some ghost
    /\ Permutation
         (tl (ranked_tips score ghost (terminal_frontier d is_scored root)))
         (remove Nat.eq_dec ghost (terminal_frontier d is_scored root)))
  /\
  (forall d score is_scored root frontier1 frontier2,
    NoDup frontier1 -> NoDup frontier2 ->
    (forall h, In h frontier1 <-> terminal_reachable d is_scored root h) ->
    (forall h, In h frontier2 <-> terminal_reachable d is_scored root h) ->
    forall ghost,
      ranked_tips score ghost frontier1 = ranked_tips score ghost frontier2).
Proof.
  exact (conj ranked_ghost_frontier_correct terminal_frontier_confluent).
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
  (forall lms inv v h, In v inv -> ~ In (v, h) (filter_inv lms inv))
  /\
  (* (e) depth filtering cannot remove or replace the selected head *)
  (forall maxn mpd head tail,
     hd_error (prop_filter_head maxn mpd (head :: tail)) = Some head)
  /\
  (* (f) a cap at least as large as the active committee cannot truncate the live frontier *)
  (forall (A : Type) max_active_validators max_parents (parents : list A),
     length parents <= S max_active_validators ->
     parent_frontier_capacity_valid max_active_validators max_parents ->
     length parents <= max_parents)
  /\
  (* (g) every undersized cap admits a reachable frontier-size witness it cannot carry *)
  (forall max_active_validators max_parents,
     max_parents < S max_active_validators ->
     exists frontier_size,
       frontier_size <= S max_active_validators /\ max_parents < frontier_size).
Proof.
  exact (conj head_preserved
          (conj take_never_drops_head
            (conj empty_tips_typed_err
              (conj invalid_excluded
                (conj depth_filter_preserves_head
                  (conj configured_parent_capacity_prevents_frontier_truncation
                        undersized_parent_capacity_has_a_blocked_frontier_witness)))))).
Qed.

(* ===========================================================================
   Capstone 4 - BRIDGE (Rocq assumes what Rust enforces)
   =========================================================================== *)

Theorem fork_choice_bridge_correct :
  (* (a) validation ENFORCES the well-formed DAG (the genesis-pinned
     `validated_block` is stronger than needed for wf_dag; the pin is consumed
     separately by validation_implies_single_root, threaded into the ghost
     capstone clause (d)). *)
  (forall genesis_hash d, (forall b, In b d -> validated_block genesis_hash d b) -> wf_dag d)
  /\
  (* (b) honest depth-filtered parents pass the validator's buffered check *)
  (forall maxn mpd buf nums,
     declared_parents_ok maxn (mpd + buf) (prop_filter_head maxn mpd nums) = true)
  /\
  (* (c) the (is_main DESC, hash ASC) sort is a total order, hence
         permutation-invariant. *)
  (forall main (l l' : list BlockHash),
     NoDup l -> Permutation l l' ->
     sort (map (parent_entry main) l) = sort (map (parent_entry main) l'))
  /\
  (* (d) the fork-choice descent terminates at a tip *)
  (forall d score is_scored h,
     wf_dag d -> wf_lookup d ->
     best_child d score is_scored (rank d score is_scored (S (dag_max_num d)) h) = None)
  /\
  (* (e) the production parent-ordering pipeline is deterministic. *)
  (forall main (l l' : list BlockHash),
     NoDup l -> Permutation l l' ->
     consensus_parent_pipeline main l = consensus_parent_pipeline main l')
  /\
  (* (f) no post-fork-choice concern may replace the GHOST head. *)
  (forall main (parents : list BlockHash),
     NoDup parents -> In main parents ->
     hd_error (consensus_parent_pipeline main parents) = Some main).
Proof.
  exact (conj validation_implies_wf_dag
          (conj honest_forkchoice_parents_validate
            (conj ghost_sort_first_deterministic
              (conj rank_terminates
                (conj consensus_parent_pipeline_deterministic
                      consensus_parent_pipeline_preserves_ghost_head))))).
Qed.
