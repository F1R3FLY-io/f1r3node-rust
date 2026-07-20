------------------------- MODULE FinalizedFloorScan -------------------------
(***************************************************************************)
(* H3 (floor-bounded merge scope) + T-PS (unconstrained parent oracle).    *)
(*                                                                         *)
(* FinalizedFloor.tla proves the H1 Δ-backstop (no lost parent write under  *)
(* the cap) + Liveness for a FIXED two-lane parent set, but it ASSUMES the  *)
(* merge reflects all parents (mergeKeys' = Lanes) — it does not model the   *)
(* SCAN that builds the merge scope. This module fills that gap: it models   *)
(* per-block writes and the floor-bounded ancestor scan explicitly, over an  *)
(* UNCONSTRAINED parent set (T-PS: parent selection as an oracle), and       *)
(* checks that NO parent write at or above the floor is ever dropped (H3).   *)
(*                                                                          *)
(* The `BadCut` constant is the knob that distinguishes the fix from the     *)
(* bug it replaced:                                                          *)
(*   BadCut = 0  -> the FIX: the scan is bounded exactly at the floor        *)
(*                  height, so it collects every block above the floor.      *)
(*   BadCut > 0  -> the BUG: the old `max_parent_depth` bound could cut      *)
(*                  ABOVE the floor, silently dropping the writes in         *)
(*                  (floor, floor+BadCut].                                   *)
(*                                                                          *)
(* Linear-chain ancestry (block b is an ancestor of parent p iff b <= p) is  *)
(* the faithful abstraction for the main-parent spine; a parent set is any    *)
(* nonempty subset of the produced blocks.                                   *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS
    MaxTip,   \* bound the chain length so TLC terminates (e.g. 4)
    BadCut    \* 0 = the fix (bound at floor); >0 = the old cut-above-floor bug

VARIABLES
    tip,      \* highest block produced
    floor,    \* the finalized floor height
    parents,  \* the (unconstrained) declared parent set
    merged    \* the writes the merge reflects: floor base + scanned scope

vars == <<tip, floor, parents, merged>>

Blocks == 1..MaxTip
Max(S) == CHOOSE x \in S : \A y \in S : y <= x

\* The floor-bounded scan: every block strictly above (floor + BadCut) that is an
\* ancestor of some parent (b <= p on the linear spine). Base: blocks sealed at or
\* below the floor. A parent write is "reflected" iff it is in the merge.
Scope(fl, ps)  == { b \in Blocks : (b > fl + BadCut) /\ (\E p \in ps : b <= p) }
Base(fl)       == { b \in Blocks : b <= fl }
MergedSet(fl, ps) == Scope(fl, ps) \union Base(fl)

Init ==
    /\ tip = 0
    /\ floor = 0
    /\ parents = {}
    /\ merged = {}

\* One merge step: nondeterministically pick a tip, a floor at/below it, and an
\* ARBITRARY nonempty parent set (the unconstrained oracle — T-PS), then compute
\* the scanned merge. No precondition constrains which parents are chosen.
Step ==
    \E t  \in 1..MaxTip :
      \E fl \in 0..t :
        \E ps \in (SUBSET (1..t)) \ {{}} :
          /\ tip' = t
          /\ floor' = fl
          /\ parents' = ps
          /\ merged' = MergedSet(fl, ps)

Next == Step

Spec == Init /\ [][Next]_vars

------------------------------------------------------------------------------
TypeOK ==
    /\ tip \in 0..MaxTip
    /\ floor \in 0..MaxTip
    /\ parents \subseteq Blocks
    /\ merged \subseteq Blocks

\* H3 / no-lost-write for ANY parent list (T-PS): every parent's write is
\* reflected in the merge — sealed in the floor base (p <= floor) or collected by
\* the scanned scope (p > floor). With BadCut = 0 this holds for every reachable
\* state and every parent set; with BadCut > 0 a parent in (floor, floor+BadCut]
\* is dropped and TLC exhibits the counterexample.
Inv_NoParentWriteDropped == \A p \in parents : p \in merged

\* Stronger band coverage: every block from 1 up to the highest parent is
\* reflected — nothing between the floor and the tip is dropped by the scan.
Inv_BandCovered == (parents = {}) \/ (\A b \in 1..Max(parents) : b \in merged)
==============================================================================
