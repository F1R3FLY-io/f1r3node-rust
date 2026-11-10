------------------------------ MODULE VectorDb ------------------------------
(***************************************************************************)
(* The VectorDB discipline of one collection (P6, design v3 sections 4.4  *)
(* to 4.6; plan decisions 30, 45, 49, 66 and I-21).                        *)
(*                                                                         *)
(* A collection holds records (an id, a payload and a persistence flag)    *)
(* and parked waiters. Each step of the model is one space transaction:    *)
(* a fresh send or receive, the re-issue of an operation, or a settle job. *)
(* No other transaction runs inside it, so the step computes the whole     *)
(* transaction. The payload img scores above doc for every query. A query  *)
(* "all" makes every record eligible, and a query "img" makes the img      *)
(* records eligible (a bound). The selection order is the better score     *)
(* first, then the lower id (the DR-75 tie-break). A plain receive has no  *)
(* query and reads every record in id order (the canonical order).         *)
(*                                                                         *)
(* Changed by decision 102 (DR-129 of the base): the DR-75 order is the    *)
(* replay order. Play shuffles the ties of a single-mode selection and the *)
(* records of a plain read. So a single-mode waiter binds any best record  *)
(* that its pattern binds. A plain waiter binds any record that its        *)
(* pattern binds. A list keeps the DR-75 tie-break (Q-102-1).              *)
(*                                                                         *)
(* A plain or single-mode waiter binds the first record of its order that  *)
(* its pattern binds. A list waiter takes the first k eligible records of  *)
(* its order (all of them for k = 0), and it fires when the list is not    *)
(* empty and its pattern binds the list: "pair" binds a list of two, and   *)
(* "docpair" a list of two whose first payload is doc (CE-2).              *)
(*                                                                         *)
(* RECEIVE-VDB fires the receive on its selection or parks it. The fire    *)
(* removes the members that are not persistent, and a peek owes them as    *)
(* second sends (Rule E, decision 45). A persistent receive whose partner  *)
(* left fires again. OFFER-VDB offers an element to the first parked       *)
(* waiter (by id) that it fires. A list waiter fires on the list with the  *)
(* element at its place, and without a peek its other members that are    *)
(* not persistent retire (K_retire). An element that the fire does not     *)
(* spend is offered again (decision 30). Otherwise the element is stored.  *)
(* The TAIL offers the owed second sends first (a stack: the last owed     *)
(* runs first), and then LIST-CASCADE fires the first parked list waiter   *)
(* that can fire after a removal. It lifts the first member of the list    *)
(* (K_lift), and a peek owes the lifted member again.                      *)
(*                                                                         *)
(* Changed by decision 101 (CR-13): a list receive fires one bind of the   *)
(* first member of its list. Without a peek, K_retire then retires the     *)
(* other non-persistent members. A peek removes the first member when it   *)
(* is not persistent, and owes it as one second send. Changed by decision  *)
(* 102 (Q-102-2): OFFER-VDB offers the element to any parked waiter that   *)
(* it fires, and LIST-CASCADE fires any parked list waiter that can fire.  *)
(*                                                                         *)
(* One fire count runs through the transaction (decision 49). When the     *)
(* element of the operation stops at a divergent pair, the tail still      *)
(* runs, and the operation re-issues at the end. When it stops at Cap      *)
(* fires with work owed, the transaction stops at once. A stop of the tail *)
(* (Cap fires, or a second send at a divergent pair) commits a settle job  *)
(* with the pending second sends and the resume operation. A stop that     *)
(* owes nothing re-issues the operation alone (DV-5). A settle job runs in *)
(* the join engine: it offers its pending sends and then runs the cascade  *)
(* (joins plan section 4.10). The model has no guard and no capacity: a    *)
(* store into a full collection aborts the deploy, so it commits nothing.  *)
(*                                                                         *)
(* Invariants (TypeOK holds the types):                                    *)
(*   QuiescentNoPendingMatch  with no job outstanding, no parked waiter    *)
(*                            can fire on the records (LM-b). Rocq:        *)
(*                            vdb_no_pending_match (S22).                  *)
(*   ListAtomic               a list fire without a peek spends each of    *)
(*                            its members that is not persistent in the    *)
(*                            transaction of the fire.                     *)
(*   PeekConservation         every record that was stored is stored,      *)
(*                            spent by a fire of a waiter that does not    *)
(*                            peek, or pending in a settle job (Rule E).   *)
(*                            Rocq: vdb_no_record_lost (S25).              *)
(*   CascadeTerminates        each step of each tail lowers the measure    *)
(*                            2 * (Cap - fired) + |owed| or stops, and no  *)
(*                            transaction runs out of its fuel. Rocq:      *)
(*                            vdb_txn_bounded (S24).                       *)
(* Changed by decision 102: every invariant holds for every pick.          *)
(*                                                                         *)
(* Unsafe controls (CONSTANT flags). Each one refutes exactly one          *)
(* invariant, and the complement run without that invariant finds no       *)
(* violation:                                                              *)
(*   NoCascade = TRUE           VdbNoCascadeUnsafe refutes                 *)
(*                              QuiescentNoPendingMatch (CE-2).            *)
(*   ParkOnlyWhileEmpty = TRUE  VdbParkOnlyWhileEmptyUnsafe refutes        *)
(*                              QuiescentNoPendingMatch (CE-1, LM-a).      *)
(*   NoRetire = TRUE            VdbNoRetireUnsafe refutes ListAtomic.      *)
(*                              Changed by decision 101: the flag also     *)
(*                              drops the K_retire of a list receive.      *)
(*   DropOwed = TRUE            VdbDropOwedUnsafe refutes                  *)
(*                              PeekConservation.                          *)
(*   UncountedCascade = TRUE    VdbUncountedCascadeUnsafe refutes          *)
(*                              CascadeTerminates.                         *)
(***************************************************************************)
\* Code (rholang/src/rust/interpreter/spaces; the line numbers are those of
\* the commit of step 8c):
\*   Eligible, Order       disciplines/vectordb.rs:1028-1129 (eligible)
\*   Sel, FiresOn          disciplines/vectordb.rs:1651-1703, 1064-1113 (receive_selection, list_selection)
\*   OFires                disciplines/vectordb.rs:1480-1536, 1119-1138 (first_offer_target, list_offer)
\*   AfterFire             reified/costs.rs:71-82 (FireRounds::after_fire)
\*   Offer                 disciplines/vectordb.rs:1418-1467 (offer_vdb), 1151-1194 (retire_members)
\*   Recv                  disciplines/vectordb.rs:1550-1640 (receive_plan)
\*   Lift                  disciplines/vectordb.rs:1929-1974 (lift_and_reoffer)
\*   RunTail               disciplines/vectordb.rs:1787-1822, 1874-1922 (run_tail, first_cascade_target)
\*   RunOp                 disciplines/vectordb.rs:1768-1782 (finish_tail)
\*   NewJobs               disciplines/vectordb.rs:1828-1854 (conclude), join.rs:4757-4819 (Engine::commit)
\*   RunSettle             join.rs:371-404 (classify), 2645-2732 (Engine::run), 4362-4394 (drain_and_cascade)

EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS
    Vals,                \* the payloads: a subset of {"img", "doc"}
    Modes,               \* the receive modes: a subset of {"plain", "single", "list"}
    Queries,             \* the queries of single and list waiters: a subset of {"all", "img"}
    SinglePats,          \* the patterns of plain and single waiters: a subset of {"any", "img", "doc"}
    ListPats,            \* the patterns of list waiters: a subset of {"pair", "docpair"}
    Ks,                  \* the lengths of list waiters: 0 stands for results = "all"
    Peeks,               \* the peek flags of a receive: a subset of BOOLEAN
    RecvPers,            \* the persistence flags of a receive: a subset of BOOLEAN
    SendPers,            \* the persistence flags of a send: a subset of BOOLEAN
    MaxOps,              \* the fresh operations of a run
    MaxRounds,           \* the transactions of a run
    Cap,                 \* FIRE_ROUND_CAP
    NoCascade,           \* Unsafe: the tail runs no LIST-CASCADE
    ParkOnlyWhileEmpty,  \* Unsafe: LM-a
    NoRetire,            \* Unsafe: a list fire without a peek retires no member
    DropOwed,            \* Unsafe: a peek fire owes no second sends
    UncountedCascade     \* Unsafe: a cascade fire does not count toward the fires

ASSUME Vals \subseteq {"img", "doc"} /\ Vals # {}
ASSUME Modes \subseteq {"plain", "single", "list"} /\ Modes # {}
ASSUME Queries \subseteq {"all", "img"} /\ Queries # {}
ASSUME SinglePats \subseteq {"any", "img", "doc"}
ASSUME ListPats \subseteq {"pair", "docpair"}
ASSUME Ks \subseteq 0..3
ASSUME Peeks \subseteq BOOLEAN /\ Peeks # {}
ASSUME RecvPers \subseteq BOOLEAN /\ RecvPers # {}
ASSUME SendPers \subseteq BOOLEAN /\ SendPers # {}
ASSUME MaxOps \in 1..8 /\ MaxRounds \in 1..16 /\ Cap \in 1..4
ASSUME {NoCascade, ParkOnlyWhileEmpty, NoRetire, DropOwed, UncountedCascade} \subseteq BOOLEAN

VARIABLES
    recs,     \* the stored records
    wtrs,     \* the parked waiters
    jobs,     \* the outstanding jobs: re-issued operations and settle jobs
    ops,      \* the fresh operations so far
    rounds,   \* the transactions so far
    nid,      \* the next id of a record or a waiter
    stored,   \* the ids of every record that was stored
    spent,    \* the ids of the records that a fire of a waiter that does not peek spent
    aOk,      \* ListAtomic held in every transaction so far
    mOk       \* CascadeTerminates held in every transaction so far

vars == <<recs, wtrs, jobs, ops, rounds, nid, stored, spent, aOk, mOk>>

-----------------------------------------------------------------------------
(* Records, waiters and operations *)

Rec == [id : Nat, val : Vals, pers : BOOLEAN]
NoRec == [id |-> 0, val |-> CHOOSE v \in Vals : TRUE, pers |-> FALSE]
Wtr == [id : Nat, mode : Modes, q : Queries \cup {"none"}, pat : SinglePats \cup ListPats,
        k : Ks \cup {0}, peek : BOOLEAN, pers : BOOLEAN]
Op == [kind : {"send"}, x : Rec] \cup [kind : {"recv"}, w : Wtr]
NoOp == [kind |-> "none"]
Job == [kind : {"op"}, op : Op]
       \cup [kind : {"settle"}, pending : Seq(Rec), resume : Op \cup {NoOp}]

Ids(S) == {r.id : r \in S}
Range(s) == {s[i] : i \in DOMAIN s}
\* Retired by decision 102 (Q-102-2): OFFER-VDB and LIST-CASCADE pick any waiter, so no operator
\* takes the waiter with the least id. A list orders its members with Sorted.
\* MinBy(S) == CHOOSE m \in S : \A s \in S : m.id <= s.id

-----------------------------------------------------------------------------
(* Selection *)

Score(r) == IF r.val = "img" THEN 2 ELSE 1
Eligible(W, r) == W.mode = "plain" \/ W.q = "all" \/ (W.q = "img" /\ r.val = "img")
Before(W, r, s) ==
    IF W.mode = "plain" THEN r.id < s.id
    ELSE Score(r) > Score(s) \/ (Score(r) = Score(s) /\ r.id < s.id)

RECURSIVE Sorted(_, _)
Sorted(W, S) ==
    IF S = {} THEN << >>
    ELSE LET m == CHOOSE m \in S : \A s \in S \ {m} : Before(W, m, s)
         IN <<m>> \o Sorted(W, S \ {m})

Order(W, R) == Sorted(W, {r \in R : Eligible(W, r)})
Binds1(p, v) == p = "any" \/ p = v
BindsL(p, L) == Len(L) = 2 /\ (p = "pair" \/ (p = "docpair" /\ L[1].val = "doc"))
Take(L, k) == IF k = 0 \/ k >= Len(L) THEN L ELSE SubSeq(L, 1, k)
ListOf(W, R) == Take(Order(W, R), W.k)

\* Changed by decision 102 (Q-102-1, Q-102-2): SEL returns the selections of every pick. The old
\* form follows.
\* \* The selection of a receive: one record, or the list.
\* Sel(W, R) ==
\*     IF W.mode = "list" THEN ListOf(W, R)
\*     ELSE LET L == SelectSeq(Order(W, R), LAMBDA r : Binds1(W.pat, r.val))
\*          IN IF L = << >> THEN << >> ELSE <<L[1]>>
\*
\* FiresOn(W, R) ==
\*     LET L == Sel(W, R) IN L # << >> /\ (W.mode # "list" \/ BindsL(W.pat, L))

\* The records that a plain or single-mode receive may bind: the eligible
\* records that its pattern binds. A plain read picks any of them, and a
\* single-mode pick takes any of them with the best score.
Bound1(W, R) == {r \in R : Eligible(W, r) /\ Binds1(W.pat, r.val)}
Best1(W, R) ==
    IF W.mode = "plain" THEN Bound1(W, R)
    ELSE {r \in Bound1(W, R) : \A s \in Bound1(W, R) : Score(s) <= Score(r)}

\* The selections of a receive: the list (with DR-75 ties), or one record
\* for each pick.
Sel(W, R) ==
    IF W.mode = "list" THEN {ListOf(W, R)}
    ELSE IF Bound1(W, R) = {} THEN {<< >>} ELSE {<<r>> : r \in Best1(W, R)}

\* The selection L fires W.
FiresL(W, L) == L # << >> /\ (W.mode # "list" \/ BindsL(W.pat, L))

\* W fires on R under a pick. The model has no guard, so whether a pick
\* fires does not depend on the pick.
FiresOn(W, R) == \E L \in Sel(W, R) : FiresL(W, L)

\* OFFER-VDB: the waiter W fires on the element x. Under LM-a a list waiter
\* fires on an offer only when its list is the element alone.
OFires(W, R, x) ==
    /\ Eligible(W, x)
    /\ IF W.mode = "list"
         THEN LET L == ListOf(W, R \cup {x})
              IN /\ \E i \in DOMAIN L : L[i].id = x.id
                 /\ BindsL(W.pat, L)
                 /\ (ParkOnlyWhileEmpty => L = <<x>>)
         ELSE Binds1(W.pat, x.val)

\* FireRounds::after_fire.
AfterFire(n, spentIn, stays) ==
    IF spentIn THEN "done" ELSE IF stays THEN "stop" ELSE IF n >= Cap THEN "stop" ELSE "cont"

-----------------------------------------------------------------------------
(* One transaction *)

\* The state of a transaction: the staged records and waiters, the fire
\* count, the owed second sends, whether a record left since the last
\* cascade search, the spent ids, the fires, the stop, whether the element
\* of the operation is still in flight at the stop, and the checks.
Txn(R, WS, owed, rm) ==
    [recs |-> R, wtrs |-> WS, fired |-> 0, owed |-> owed, rm |-> rm, spent |-> {},
     fires |-> << >>, stop |-> FALSE, own |-> "settled", failed |-> FALSE, err |-> FALSE,
     mOk |-> TRUE]

Fire(W, L, x) == [w |-> W, members |-> L, x |-> x]

\* Changed by decision 102 (Q-102-2): OFFER-VDB returns the set of the states of every pick,
\* because the element fires any waiter of C (first_offer_target). The old form follows.
\* \* OFFER-VDB of x. own: x is the datum of the operation. When x stops in
\* \* flight, the operation re-issues (own records the cause), or a second
\* \* send goes back on top of the owed sends (decision 49).
\* RECURSIVE Offer(_, _, _, _)
\* Offer(T, x, own, f) ==
\*     IF f = 0 THEN [T EXCEPT !.err = TRUE]
\*     ELSE LET C == {W \in T.wtrs : OFires(W, T.recs, x)} IN
\*     IF C = {} THEN [T EXCEPT !.recs = @ \cup {x}]
\*     ELSE
\*       LET W == MinBy(C)
\*           L == IF W.mode = "list" THEN ListOf(W, T.recs \cup {x}) ELSE <<x>>
\*           out == IF W.peek \/ NoRetire THEN {} ELSE {m \in Range(L) \ {x} : ~m.pers}
\*           spentX == ~x.pers /\ ~W.peek
\*           T1 == [T EXCEPT
\*                    !.recs = @ \ out,
\*                    !.wtrs = IF W.pers THEN @ ELSE @ \ {W},
\*                    !.fired = @ + 1,
\*                    !.rm = @ \/ out # {},
\*                    !.spent = @ \cup Ids(out) \cup (IF spentX THEN {x.id} ELSE {}),
\*                    !.fires = Append(@, Fire(W, L, x))]
\*           step == AfterFire(T1.fired, spentX, W.pers)
\*       IN CASE step = "done" -> T1
\*            [] step = "stop" /\ own -> [T1 EXCEPT !.own = IF W.pers THEN "pair" ELSE "cap"]
\*            [] step = "stop" -> [T1 EXCEPT !.stop = TRUE, !.owed = <<x>> \o @]
\*            [] OTHER -> Offer(T1, x, own, f - 1)

\* OFFER-VDB of x. own: x is the datum of the operation. When x stops in
\* flight, the operation re-issues (own records the cause), or a second
\* send goes back on top of the owed sends (decision 49). OfferTo is the
\* fire of the picked waiter W.
RECURSIVE Offer(_, _, _, _), OfferTo(_, _, _, _, _)
OfferTo(T, x, own, f, W) ==
    LET L == IF W.mode = "list" THEN ListOf(W, T.recs \cup {x}) ELSE <<x>>
        out == IF W.peek \/ NoRetire THEN {} ELSE {m \in Range(L) \ {x} : ~m.pers}
        spentX == ~x.pers /\ ~W.peek
        T1 == [T EXCEPT
                 !.recs = @ \ out,
                 !.wtrs = IF W.pers THEN @ ELSE @ \ {W},
                 !.fired = @ + 1,
                 !.rm = @ \/ out # {},
                 !.spent = @ \cup Ids(out) \cup (IF spentX THEN {x.id} ELSE {}),
                 !.fires = Append(@, Fire(W, L, x))]
        step == AfterFire(T1.fired, spentX, W.pers)
    IN CASE step = "done" -> {T1}
         [] step = "stop" /\ own -> {[T1 EXCEPT !.own = IF W.pers THEN "pair" ELSE "cap"]}
         [] step = "stop" -> {[T1 EXCEPT !.stop = TRUE, !.owed = <<x>> \o @]}
         [] OTHER -> Offer(T1, x, own, f - 1)
Offer(T, x, own, f) ==
    IF f = 0 THEN {[T EXCEPT !.err = TRUE]}
    ELSE LET C == {W \in T.wtrs : OFires(W, T.recs, x)} IN
    IF C = {} THEN {[T EXCEPT !.recs = @ \cup {x}]}
    ELSE UNION {OfferTo(T, x, own, f, W) : W \in C}

\* Changed by decision 102 (Q-102-1, Q-102-2) and decision 101 (CR-13): RECEIVE-VDB returns the
\* set of the states of every pick. A fire binds the first member of its selection. Without a
\* peek, K_retire retires the other non-persistent members of a list, and the Unsafe NoRetire
\* drops that K_retire. A peek removes and owes the first member only. The old form follows.
\* \* RECEIVE-VDB of W. A stop records the cause in own (the receive re-issues).
\* RECURSIVE Recv(_, _, _)
\* Recv(T, W, f) ==
\*     IF f = 0 THEN [T EXCEPT !.err = TRUE]
\*     ELSE IF FiresOn(W, T.recs) THEN
\*       LET L == Sel(W, T.recs)
\*           gone == {m \in Range(L) : ~m.pers}
\*           T1 == [T EXCEPT
\*                    !.recs = @ \ gone,
\*                    !.fired = @ + 1,
\*                    !.owed = IF W.peek /\ ~DropOwed THEN SelectSeq(L, LAMBDA m : ~m.pers) \o @ ELSE @,
\*                    !.rm = TRUE,
\*                    !.spent = IF W.peek THEN @ ELSE @ \cup Ids(gone),
\*                    !.fires = Append(@, Fire(W, L, NoRec))]
\*           stays == W.peek \/ \E m \in Range(L) : m.pers
\*           step == AfterFire(T1.fired, ~W.pers, stays)
\*       IN CASE step = "done" -> T1
\*            [] step = "stop" -> [T1 EXCEPT !.own = IF stays THEN "pair" ELSE "cap"]
\*            [] OTHER -> Recv(T1, W, f - 1)
\*     ELSE IF ParkOnlyWhileEmpty /\ W.mode = "list" /\ Order(W, T.recs) # << >>
\*       THEN [T EXCEPT !.failed = TRUE]
\*       ELSE [T EXCEPT !.wtrs = @ \cup {W}]

\* RECEIVE-VDB of W. A stop records the cause in own (the receive
\* re-issues). RecvOn is the fire on the selection L of a pick.
RECURSIVE Recv(_, _, _), RecvOn(_, _, _, _)
RecvOn(T, W, f, L) ==
    LET first == L[1]
        rest == IF W.mode # "list" \/ W.peek \/ NoRetire THEN {}
                ELSE {m \in Range(L) \ {first} : ~m.pers}
        gone == (IF first.pers THEN {} ELSE {first}) \cup rest
        T1 == [T EXCEPT
                 !.recs = @ \ gone,
                 !.fired = @ + 1,
                 !.owed = IF W.peek /\ ~first.pers /\ ~DropOwed THEN <<first>> \o @ ELSE @,
                 !.rm = TRUE,
                 !.spent = IF W.peek THEN @ ELSE @ \cup Ids(gone),
                 !.fires = Append(@, Fire(W, L, NoRec))]
        stays == W.peek \/ \E m \in Range(L) : m.pers
        step == AfterFire(T1.fired, ~W.pers, stays)
    IN CASE step = "done" -> {T1}
         [] step = "stop" -> {[T1 EXCEPT !.own = IF stays THEN "pair" ELSE "cap"]}
         [] OTHER -> Recv(T1, W, f - 1)
Recv(T, W, f) ==
    IF f = 0 THEN {[T EXCEPT !.err = TRUE]}
    ELSE IF FiresOn(W, T.recs)
      THEN UNION {RecvOn(T, W, f, L) : L \in {S \in Sel(W, T.recs) : FiresL(W, S)}}
    ELSE IF ParkOnlyWhileEmpty /\ W.mode = "list" /\ Order(W, T.recs) # << >>
      THEN {[T EXCEPT !.failed = TRUE]}
      ELSE {[T EXCEPT !.wtrs = @ \cup {W}]}

Measure(T) == 2 * (Cap - T.fired) + Len(T.owed)

\* LIFT-AND-REOFFER of a list target.
Lift(T, W) ==
    LET L == ListOf(W, T.recs)
        lead == L[1]
        lifted == ~lead.pers
        ret == IF W.peek \/ NoRetire THEN {} ELSE {m \in Range(L) \ {lead} : ~m.pers}
        out == (IF lifted THEN {lead} ELSE {}) \cup ret
        spentLead == lifted /\ ~W.peek
        T1 == [T EXCEPT
                 !.recs = @ \ out,
                 !.wtrs = IF W.pers THEN @ ELSE @ \ {W},
                 !.fired = IF UncountedCascade THEN @ ELSE @ + 1,
                 !.owed = IF W.peek /\ lifted /\ ~DropOwed THEN <<lead>> ELSE << >>,
                 !.rm = TRUE,
                 !.spent = @ \cup Ids(ret) \cup (IF spentLead THEN {lead.id} ELSE {}),
                 !.fires = Append(@, Fire(W, L, lead))]
    IN IF AfterFire(T1.fired, spentLead, W.pers) = "stop" THEN [T1 EXCEPT !.stop = TRUE] ELSE T1

\* Changed by decision 102 (Q-102-2): the TAIL returns the set of the states of every pick,
\* because LIST-CASCADE fires any waiter of C (first_cascade_target). The old form follows.
\* \* The TAIL: the owed second sends first, then LIST-CASCADE. A second send
\* \* that meets Cap fires goes back on top and the transaction stops.
\* RECURSIVE RunTail(_, _)
\* RunTail(T, f) ==
\*     IF T.stop \/ T.failed THEN T
\*     ELSE IF f = 0 THEN [T EXCEPT !.err = TRUE]
\*     ELSE IF T.owed # << >> THEN
\*       IF T.fired >= Cap THEN [T EXCEPT !.stop = TRUE]
\*       ELSE LET T0 == [T EXCEPT !.owed = SubSeq(@, 2, Len(@))]
\*                T1 == Offer(T0, Head(T.owed), FALSE, Cardinality(T.wtrs) + 1)
\*            IN RunTail([T1 EXCEPT !.mOk = @ /\ (T1.stop \/ Measure(T1) < Measure(T))], f - 1)
\*     ELSE IF ~T.rm \/ NoCascade THEN T
\*     ELSE LET C == {W \in T.wtrs : W.mode = "list" /\ FiresOn(W, T.recs)} IN
\*       IF C = {} THEN [T EXCEPT !.rm = FALSE]
\*       ELSE IF T.fired >= Cap THEN [T EXCEPT !.stop = TRUE]
\*       ELSE LET T1 == Lift(T, MinBy(C))
\*            IN RunTail([T1 EXCEPT !.mOk = @ /\ (T1.stop \/ Measure(T1) < Measure(T))], f - 1)

\* The TAIL: the owed second sends first, then LIST-CASCADE. A second send
\* that meets Cap fires goes back on top and the transaction stops.
\* TailStep goes on from the state T1 after one step of T, with the check of
\* the measure.
RECURSIVE RunTail(_, _), TailStep(_, _, _)
TailStep(T, T1, f) ==
    RunTail([T1 EXCEPT !.mOk = @ /\ (T1.stop \/ Measure(T1) < Measure(T))], f - 1)
RunTail(T, f) ==
    IF T.stop \/ T.failed THEN {T}
    ELSE IF f = 0 THEN {[T EXCEPT !.err = TRUE]}
    ELSE IF T.owed # << >> THEN
      IF T.fired >= Cap THEN {[T EXCEPT !.stop = TRUE]}
      ELSE LET T0 == [T EXCEPT !.owed = SubSeq(@, 2, Len(@))]
           IN UNION {TailStep(T, T1, f) : T1 \in Offer(T0, Head(T.owed), FALSE, Cardinality(T.wtrs) + 1)}
    ELSE IF ~T.rm \/ NoCascade THEN {T}
    ELSE LET C == {W \in T.wtrs : W.mode = "list" /\ FiresOn(W, T.recs)} IN
      IF C = {} THEN {[T EXCEPT !.rm = FALSE]}
      ELSE IF T.fired >= Cap THEN {[T EXCEPT !.stop = TRUE]}
      ELSE UNION {TailStep(T, Lift(T, W), f) : W \in C}

TailFuel(T) == 2 * Cap + Len(T.owed) + 2

\* Changed by decision 102: the transaction of an operation returns the set of its end states.
\* The old form follows.
\* RunOp(op) ==
\*     LET T0 == Txn(recs, wtrs, << >>, FALSE)
\*         T1 == IF op.kind = "send"
\*                 THEN Offer(T0, op.x, TRUE, Cardinality(wtrs) + 1)
\*                 ELSE Recv(T0, op.w, Cardinality(recs) + 1)
\*     IN IF T1.own = "cap"
\*          THEN (IF T1.owed = << >> /\ ~T1.rm THEN T1 ELSE [T1 EXCEPT !.stop = TRUE])
\*          ELSE RunTail(T1, TailFuel(T1))

\* The end of the transaction of an operation after its element (finish_tail):
\* an own element that stopped at the round cap stops the transaction when
\* work is owed, and otherwise only re-issues. After a divergent pair, or
\* when the element settled, the tail runs.
RunOpEnd(T1) ==
    IF T1.own = "cap"
      THEN {IF T1.owed = << >> /\ ~T1.rm THEN T1 ELSE [T1 EXCEPT !.stop = TRUE]}
      ELSE RunTail(T1, TailFuel(T1))

\* The transaction of a fresh or re-issued operation.
RunOp(op) ==
    LET T0 == Txn(recs, wtrs, << >>, FALSE)
        T1s == IF op.kind = "send"
                 THEN Offer(T0, op.x, TRUE, Cardinality(wtrs) + 1)
                 ELSE Recv(T0, op.w, Cardinality(recs) + 1)
    IN UNION {RunOpEnd(T1) : T1 \in T1s}

\* A settle job runs in the join engine (classify): it marks the collection
\* dirty, offers its pending second sends first, and then runs the cascade
\* (joins plan section 4.10, DV-1).
RunSettle(job) ==
    LET T0 == Txn(recs, wtrs, job.pending, TRUE) IN RunTail(T0, TailFuel(T0))

\* The jobs that a transaction leaves (DV-5, decisions 43 and 49): resume is
\* the operation to re-issue when its element is still in flight, or the
\* resume operation that a settle job carries.
NewJobs(T, resume) ==
    IF T.failed THEN {}
    ELSE IF ~T.stop THEN (IF resume = NoOp THEN {} ELSE {[kind |-> "op", op |-> resume]})
    ELSE IF T.owed # << >> \/ T.rm
      THEN {[kind |-> "settle", pending |-> T.owed, resume |-> resume]}
      ELSE IF resume = NoOp THEN {} ELSE {[kind |-> "op", op |-> resume]}

\* ListAtomic for the fires of one transaction.
AtomicIn(T) ==
    \A i \in DOMAIN T.fires :
      LET f == T.fires[i] IN
        (f.w.mode = "list" /\ ~f.w.peek) => \A m \in Range(f.members) : m.pers \/ m.id \notin Ids(T.recs)

\* A transaction that runs out of fuel commits nothing: it only records the
\* failed check, so no other invariant sees a partial transaction.
Commit(T, resume, job) ==
    IF T.err
      THEN /\ mOk' = FALSE
           /\ rounds' = rounds + 1
           /\ UNCHANGED <<recs, wtrs, jobs, stored, spent, aOk>>
      ELSE /\ recs' = T.recs
           /\ wtrs' = T.wtrs
           /\ jobs' = (jobs \ {job}) \cup NewJobs(T, resume)
           /\ stored' = stored \cup Ids(T.recs)
           /\ spent' = spent \cup T.spent
           /\ aOk' = (aOk /\ AtomicIn(T))
           /\ mOk' = (mOk /\ T.mOk)
           /\ rounds' = rounds + 1

-----------------------------------------------------------------------------
(* Actions *)

Init ==
    /\ recs = {}
    /\ wtrs = {}
    /\ jobs = {}
    /\ ops = 0
    /\ rounds = 0
    /\ nid = 1
    /\ stored = {}
    /\ spent = {}
    /\ aOk = TRUE
    /\ mOk = TRUE

NoJob == [kind |-> "none"]

Send ==
    /\ ops < MaxOps /\ rounds < MaxRounds
    \* Changed by decision 102: the transaction commits any of its end states. The old form
    \* follows.
    \* /\ \E v \in Vals, p \in SendPers :
    \*      LET op == [kind |-> "send", x |-> [id |-> nid, val |-> v, pers |-> p]]
    \*          T == RunOp(op)
    \*      IN Commit(T, IF T.own = "settled" THEN NoOp ELSE op, NoJob)
    /\ \E v \in Vals, p \in SendPers :
         LET op == [kind |-> "send", x |-> [id |-> nid, val |-> v, pers |-> p]]
         IN \E T \in RunOp(op) : Commit(T, IF T.own = "settled" THEN NoOp ELSE op, NoJob)
    /\ ops' = ops + 1
    /\ nid' = nid + 1

Waiters ==
    [id : {0}, mode : {"plain"}, q : {"none"}, pat : SinglePats, k : {0}, peek : Peeks, pers : RecvPers]
    \cup [id : {0}, mode : {"single"}, q : Queries, pat : SinglePats, k : {0}, peek : Peeks, pers : RecvPers]
    \cup [id : {0}, mode : {"list"}, q : Queries, pat : ListPats, k : Ks, peek : Peeks, pers : RecvPers]

Receive ==
    /\ ops < MaxOps /\ rounds < MaxRounds
    \* Changed by decision 102: the transaction commits any of its end states. The old form
    \* follows.
    \* /\ \E w0 \in {w \in Waiters : w.mode \in Modes /\ ~(w.peek /\ w.pers)} :
    \*      LET op == [kind |-> "recv", w |-> [w0 EXCEPT !.id = nid]]
    \*          T == RunOp(op)
    \*      IN Commit(T, IF T.own = "settled" THEN NoOp ELSE op, NoJob)
    /\ \E w0 \in {w \in Waiters : w.mode \in Modes /\ ~(w.peek /\ w.pers)} :
         LET op == [kind |-> "recv", w |-> [w0 EXCEPT !.id = nid]]
         IN \E T \in RunOp(op) : Commit(T, IF T.own = "settled" THEN NoOp ELSE op, NoJob)
    /\ ops' = ops + 1
    /\ nid' = nid + 1

RunJob ==
    /\ rounds < MaxRounds
    \* Changed by decision 102: the transaction commits any of its end states. The old form
    \* follows.
    \* /\ \E job \in jobs :
    \*      IF job.kind = "op"
    \*        THEN LET T == RunOp(job.op) IN Commit(T, IF T.own = "settled" THEN NoOp ELSE job.op, job)
    \*        ELSE LET T == RunSettle(job) IN Commit(T, job.resume, job)
    /\ \E job \in jobs :
         IF job.kind = "op"
           THEN \E T \in RunOp(job.op) : Commit(T, IF T.own = "settled" THEN NoOp ELSE job.op, job)
           ELSE \E T \in RunSettle(job) : Commit(T, job.resume, job)
    /\ UNCHANGED <<ops, nid>>

Next == Send \/ Receive \/ RunJob

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Invariants *)

PendingIds == UNION {(IF j.kind = "settle" THEN Ids(Range(j.pending)) ELSE {}) : j \in jobs}

TypeOK ==
    /\ recs \subseteq Rec
    /\ wtrs \subseteq Wtr
    /\ jobs \subseteq Job
    /\ ops \in 0..MaxOps
    /\ rounds \in 0..MaxRounds
    /\ nid \in Nat
    /\ stored \subseteq Nat
    /\ spent \subseteq Nat
    /\ aOk \in BOOLEAN
    /\ mOk \in BOOLEAN

QuiescentNoPendingMatch == jobs = {} => \A W \in wtrs : ~FiresOn(W, recs)

ListAtomic == aOk

PeekConservation == \A i \in stored : i \in Ids(recs) \cup spent \cup PendingIds

CascadeTerminates == mOk

=============================================================================
