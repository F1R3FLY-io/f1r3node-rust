--------------------- MODULE MCV6MergeSerialization ---------------------
(***************************************************************************)
(* DR-120 (gap G9): the bounded scenarios of V6MergeSerialization.         *)
(*                                                                         *)
(* Each family varies the features of one part of the rule and fixes the  *)
(* others, so the state space stays small while every check meets the     *)
(* scenarios that its control needs:                                      *)
(*   FOrder    three one-chain branches: settled flags, losses, heights   *)
(*             and races (K, S5, Agreement, #341, #294)                   *)
(*   FLedgerA  a three-chain branch and a one-chain branch with user and  *)
(*             system diffs (the S3 pre-check and the folds)              *)
(*   FLedger3  three one-chain branches with diffs and a base value       *)
(*             (S5 sign splits, S6.4 repair, the fast-path ledger)        *)
(*   FType     two merge types on one channel (LOW-4)                     *)
(*   FLineage  a two-chain branch whose consumer descends from a          *)
(*             base-conflicting block (S6.1, MEDIUM-1)                    *)
(*   FCell     produces and consumes on a single-value cell, with and     *)
(*             without the base value (S3 walk, S6.3, LOW-3, MEDIUM-2)    *)
(*   FClaims   folded and plain claims (C1)                               *)
(*   FDup      copies of one deploy with equal or different costs (C2)    *)
(*   FBase     chains that conflict with the base (dev's base partition) *)
(*   FSettled  a settled chain and a racing chain in one branch (LOW-5)   *)
(* Diffs, base values and the bounds lie in -1..1 with MAX = 1.            *)
(***************************************************************************)
EXTENDS V6MergeSerialization

Chain(id, cand) ==
    [id |-> id, cand |-> cand, pinned |-> FALSE, loss |-> 0, height |-> 0,
     deploy |-> id, cost |-> 1, u |-> NONE, y |-> NONE, mt |-> "add",
     claim |-> "none", vadd |-> {}, vrm |-> {}, prod |-> FALSE, cons |-> FALSE,
     ancs |-> {}, baseConf |-> FALSE]

Scn(chains, races, baseL, baseCell) ==
    [chains |-> chains, races |-> races, baseL |-> baseL, baseCell |-> baseCell]

Pairs3 == {{1, 2}, {1, 3}, {2, 3}}
Diff == {NONE, -1, 0, 1}
\* A user and a system diff whose sum fits: a chain's combined part is built
\* with checked addition, so (1, 1) and (-1, -1) cannot occur.
LPairs == {pr \in {NONE, -1, 1} \X {NONE, -1, 1} : ~(pr[1] # NONE /\ pr[2] # NONE /\ pr[1] = pr[2])}

FOrder ==
    { Scn({[Chain(1, 1) EXCEPT !.pinned = p1, !.loss = l1, !.height = h1],
           [Chain(2, 2) EXCEPT !.pinned = p2, !.loss = l2, !.height = h2],
           [Chain(3, 3) EXCEPT !.pinned = p3, !.loss = l3, !.height = h3]}, r, 0, FALSE)
      : p1, p2, p3 \in BOOLEAN, l1, l2, l3, h1, h2, h3 \in 0..1, r \in SUBSET Pairs3 }

FLedgerA ==
    { Scn({[Chain(1, 1) EXCEPT !.u = q1[1], !.y = q1[2]],
           [Chain(2, 1) EXCEPT !.u = q2[1], !.y = q2[2]],
           [Chain(3, 1) EXCEPT !.u = q3[1], !.y = q3[2]],
           [Chain(4, 2) EXCEPT !.u = u4]}, {}, b, FALSE)
      : q1, q2, q3 \in LPairs, u4 \in Diff, b \in -1..1 }

FLedger3 ==
    { Scn({[Chain(1, 1) EXCEPT !.u = u1, !.pinned = pn],
           [Chain(2, 2) EXCEPT !.u = u2],
           [Chain(3, 3) EXCEPT !.u = u3, !.loss = l3]}, {}, b, FALSE)
      : u1, u2, u3 \in Diff, pn \in BOOLEAN, l3 \in 0..1, b \in -1..1 }

FType ==
    { Scn({[Chain(1, 1) EXCEPT !.u = v1, !.mt = m1],
           [Chain(2, 2) EXCEPT !.u = v2, !.mt = m2]}, {}, b, FALSE)
      : v1, v2 \in 0..1, m1, m2 \in {"add", "or"}, b \in 0..1 }

FLineage ==
    { Scn({[Chain(1, 1) EXCEPT !.prod = pr1, !.pinned = pn1],
           [Chain(2, 1) EXCEPT !.cons = cs2, !.ancs = a2],
           [Chain(3, 2) EXCEPT !.cons = cs3, !.prod = pr3,
                               !.ancs = IF 2 \in a3 THEN a3 \cup a2 ELSE a3],
           [Chain(9, 9) EXCEPT !.baseConf = TRUE]}, {}, 0, FALSE)
      : pr1, pn1, cs2, cs3, pr3 \in BOOLEAN, a2 \in {{}, {9}}, a3 \in {{}, {2}} }

FCell ==
    { Scn({[Chain(1, 1) EXCEPT !.vadd = a1, !.vrm = r1, !.ancs = IF inc9 THEN g1 ELSE {}],
           [Chain(2, 1) EXCEPT !.vadd = a2, !.vrm = r2, !.loss = l2],
           [Chain(3, 2) EXCEPT !.vadd = a3, !.vrm = r3]}
          \cup (IF inc9 THEN {[Chain(9, 9) EXCEPT !.baseConf = TRUE]} ELSE {}), {}, 0, bc)
      : a1 \in {{}, {"x"}}, r1 \in {{}, {"b"}}, g1 \in {{}, {9}},
        a2 \in {{}, {"z"}}, r2 \in {{}, {"x"}, {"b"}}, l2 \in 0..1,
        a3 \in {{}, {"w"}}, r3 \in {{}, {"b"}}, inc9 \in BOOLEAN, bc \in BOOLEAN }

FClaims ==
    { Scn({[Chain(1, 1) EXCEPT !.claim = k1, !.pinned = pn],
           [Chain(2, 2) EXCEPT !.claim = k2],
           [Chain(3, 3) EXCEPT !.claim = k3]}, {}, 0, FALSE)
      : k1, k2, k3 \in {"none", "folded", "plain"}, pn \in BOOLEAN }

FDup ==
    { Scn({[Chain(1, 1) EXCEPT !.deploy = d1, !.cost = c1],
           [Chain(2, 2) EXCEPT !.deploy = d2, !.cost = c2],
           [Chain(3, 3) EXCEPT !.deploy = d3, !.cost = c3]}, r, 0, FALSE)
      : d1, d2, d3 \in 1..2, c1, c2, c3 \in 1..2, r \in SUBSET Pairs3 }

FBase ==
    { Scn({[Chain(1, 1) EXCEPT !.baseConf = b1, !.pinned = p1],
           [Chain(2, 2) EXCEPT !.baseConf = b2, !.pinned = p2],
           [Chain(3, 3) EXCEPT !.baseConf = b3, !.pinned = p3]}, r, 0, FALSE)
      : b1, b2, b3, p1, p2, p3 \in BOOLEAN, r \in SUBSET Pairs3 }

FSettled ==
    { Scn({[Chain(1, 1) EXCEPT !.pinned = pn], Chain(2, 1), Chain(3, 2)}, r, 0, FALSE)
      : pn \in BOOLEAN, r \in SUBSET {{1, 2}, {2, 3}} }

MCScenarios ==
    FOrder \cup FLedgerA \cup FLedger3 \cup FType \cup FLineage \cup FCell
    \cup FClaims \cup FDup \cup FBase \cup FSettled
=============================================================================
