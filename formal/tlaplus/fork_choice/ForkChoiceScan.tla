----------------------------- MODULE ForkChoiceScan -----------------------------
(***************************************************************************)
(* LCA depth-filter DETERMINISM — the LATEST_MESSAGE_MAX_DEPTH layer that    *)
(* ForkChoice.tla abstracts away.                                           *)
(*                                                                         *)
(* Models casper/src/rust/estimator.rs `calculate_lca` (lines 53, 187-190): *)
(* before the LCA is computed, latest messages more than                     *)
(* LATEST_MESSAGE_MAX_DEPTH (=1000) below the top block are filtered out, so  *)
(* one stale validator cannot drag the LCA (and thus the scored band) down to *)
(* genesis. The safety property is that this filter — hence the LCA — is a    *)
(* pure function of the DAG: identical DAG => identical filter on every node, *)
(* regardless of which deep messages exist.                                  *)
(*                                                                          *)
(* The `NodeLocalTop` constant is the bug knob:                              *)
(*   NodeLocalTop = 0 -> the CODE: `top` is the STRUCTURAL max height        *)
(*                       (dag.latest_block_number = get_max_height), a pure   *)
(*                       function of the DAG, identical across nodes.         *)
(*   NodeLocalTop = 1 -> the BUG: `top` is a NODE-LOCAL view that may differ  *)
(*                       across nodes, so two nodes filter different message  *)
(*                       sets and compute different LCAs -> divergence.       *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS
    MaxTip,       \* bound on block height so TLC terminates (e.g. 4)
    Cap,          \* the depth cap (LATEST_MESSAGE_MAX_DEPTH), small here (e.g. 2)
    NodeLocalTop  \* 0 = structural top (the code); 1 = node-local top (the bug)

VARIABLES
    lms,    \* the set of latest-message heights present in the DAG
    topA,   \* node A's `top`
    topB    \* node B's `top`

vars == <<lms, topA, topB>>

Max(S) == CHOOSE x \in S : \A y \in S : y <= x

\* Keep a latest message iff it is within Cap of the top height. `m + Cap > top`
\* is `m > top - Cap` without natural-number underflow.
FilteredBy(top, S) == { m \in S : m + Cap > top }

Init ==
    /\ lms = {}
    /\ topA = 0
    /\ topB = 0

Step ==
    \E S \in SUBSET (1..MaxTip) :
      /\ lms' = S
      /\ IF NodeLocalTop = 0
           THEN \* CODE: both nodes use the structural max height => identical.
                /\ topA' = (IF S = {} THEN 0 ELSE Max(S))
                /\ topB' = (IF S = {} THEN 0 ELSE Max(S))
           ELSE \* BUG: each node's top is a local view; they may differ.
                \E ta \in 0..MaxTip :
                  \E tb \in 0..MaxTip :
                    /\ topA' = ta
                    /\ topB' = tb

Next == Step

Spec == Init /\ [][Next]_vars

------------------------------------------------------------------------------
TypeOK ==
    /\ lms \subseteq (1..MaxTip)
    /\ topA \in 0..MaxTip
    /\ topB \in 0..MaxTip

\* Identical DAG => identical LCA depth-filter, regardless of which deep messages
\* exist. Holds when `top` is the structural height (the code); violated when `top`
\* is a node-local view (the bug), where two nodes filter different sets and diverge.
Inv_LcaDeterministic == FilteredBy(topA, lms) = FilteredBy(topB, lms)

\* The filtered band is bounded by the cap window — no unbounded LCA drag.
Inv_BandBounded == \A m \in FilteredBy(topA, lms) : m + Cap > topA
==============================================================================
