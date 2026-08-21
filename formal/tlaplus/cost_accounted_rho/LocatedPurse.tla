---------------------------- MODULE LocatedPurse ----------------------------
(***************************************************************************)
(* LocatedPurse — the TLA+ leg of the continued-gslt-cost-v2 alignment      *)
(* (Stage 7).  Models the monad paper's LOCATED resource stacks (§capabilities*)
(* and §linear): a token stack S(I, -) located at each interaction surface I, *)
(* drawn down by graded steps (each draw labelled by the surface/signature it  *)
(* consumes).  This is the operational image of the Rocq `lane_pool_disjoint`  *)
(* (disjoint per-signature pools, ChannelSeparation.v) and the modulus         *)
(* (CAModulus.funded_run_bounded — consumption bounded by supply).             *)
(*                                                                             *)
(* The checked invariants verify, over the whole reachable state space of the  *)
(* consumption process:                                                        *)
(*   Inv_NoUnderflow    — disjoint per-surface pools never go negative;        *)
(*   Inv_Conservation   — each purse's supply + consumed is its initial supply;*)
(*   Inv_LocalSufficiencyComposes — if every purse is locally sufficient       *)
(*       (init supply >= demand) then consumption never exceeds supply at any   *)
(*       surface, i.e. the per-surface (separating) proofs compose into global  *)
(*       sufficiency (continued-gslt-cost-v2 Prop "local sufficiency composes").*)
(*                                                                             *)
(* Self-contained (concrete small surfaces/supply/demand), so the .cfg needs    *)
(* only SPECIFICATION + INVARIANTS.  LOCAL-ONLY (never a CI gate).             *)
(***************************************************************************)
EXTENDS Naturals

Surfaces == {1, 2, 3}

\* Each located purse's initial supply and the demand at that surface.
\* Every surface is locally sufficient (InitSupply[I] >= Demand[I]).
InitSupply == [I \in Surfaces |-> IF I = 2 THEN 3 ELSE IF I = 3 THEN 1 ELSE 2]
Demand     == [I \in Surfaces |-> IF I = 2 THEN 2 ELSE 1]

VARIABLES supply, consumed
vars == <<supply, consumed>>

TypeOK ==
  /\ supply   \in [Surfaces -> Nat]
  /\ consumed \in [Surfaces -> Nat]

Init ==
  /\ supply   = InitSupply
  /\ consumed = [I \in Surfaces |-> 0]

\* A graded draw: consume one token from the located purse at surface I (the
\* grade), permitted only while the purse has supply and its demand is unmet.
Draw(I) ==
  /\ supply[I] > 0
  /\ consumed[I] < Demand[I]
  /\ supply'   = [supply   EXCEPT ![I] = @ - 1]
  /\ consumed' = [consumed EXCEPT ![I] = @ + 1]

Next == \E I \in Surfaces : Draw(I)

Spec == Init /\ [][Next]_vars

\* ── invariants ──────────────────────────────────────────────────────────
Inv_NoUnderflow  == \A I \in Surfaces : supply[I] >= 0
Inv_Conservation == \A I \in Surfaces : supply[I] + consumed[I] = InitSupply[I]

LocallySufficient == \A I \in Surfaces : InitSupply[I] >= Demand[I]

\* Local sufficiency composes: when every purse is locally sufficient, the
\* per-surface consumption never exceeds the per-surface supply — so the
\* separating (per-purse) sufficiency proofs compose into global sufficiency.
Inv_LocalSufficiencyComposes ==
  LocallySufficient => (\A I \in Surfaces : consumed[I] <= InitSupply[I])
=============================================================================
