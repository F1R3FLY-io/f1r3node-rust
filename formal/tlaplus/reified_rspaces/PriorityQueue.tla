--------------------------- MODULE PriorityQueue ---------------------------
(***************************************************************************)
(* The priority queue discipline of the native spaces (plan decisions 11   *)
(* and 38) and the settle of a priority queue (the P5 design v3, section   *)
(* A.8, choice F2).                                                        *)
(*                                                                         *)
(* A priority queue holds records (data) in levels and waiters (waiting    *)
(* continuations) in lanes. The levels ascend, and the unprioritized level *)
(* Unp comes after every priority level (F6). The lane of a waiter is the  *)
(* waiter queue of a level p, or the unrestricted queue AnyLane. Each      *)
(* level and each lane is a FIFO queue. Its head is its entry with the     *)
(* least ord, and only heads interact (decision 11). The waiter queue of   *)
(* the level p sees the head of the level p. The unrestricted queue sees   *)
(* the heads of all levels in ascending level order. Each round of an      *)
(* operation is one space transaction, and every round bumps the one       *)
(* counter of the priority queue (decision 31).                            *)
(*                                                                         *)
(* A round of an operation has these phases (offer_pq and receive_pq with  *)
(* the hooks of PriorityQueueDiscipline). One space transaction makes at   *)
(* most Cap fires, its settles included (revision I-21). So one count of   *)
(* fires (txn.fired, Cx::rounds) runs through the phases of a round, and   *)
(* only a new round resets it:                                             *)
(*   before  the settle before the plan. It starts the count at 0. At the  *)
(*           cap or at a divergent pair, the round re-issues the operation *)
(*           (decision 43 order).                                          *)
(*   plan    open_plan: when the settle before the plan ended at the cap,  *)
(*           the plan stages nothing and the round re-issues the operation *)
(*           (decision 43 order). Else a fresh element joins the tail of   *)
(*           its lane, so it plans only on an empty lane. A re-entering    *)
(*           persistent element plans when its lane is empty or its ord is *)
(*           less than every ord of its lane (precedes is <). Else it is   *)
(*           stored or parked at its FIFO place with no fire.              *)
(*   loop    a send offers its datum to the candidate heads: the head of   *)
(*           the waiter queue of its level and the head of the             *)
(*           unrestricted queue. The older candidate that fits fires, and  *)
(*           a refusal moves on (decisions 37 and 38). A receive binds the *)
(*           first level head that its lane sees and that its pattern      *)
(*           matches. It checks its guard once, and a refusal parks it     *)
(*           (decision 38). The loop continues the count of the settle     *)
(*           before the plan. A divergent pair or the cap re-issues the    *)
(*           operation with no settle after the plan.                      *)
(*   after   the settle after the plan. It continues the count of the      *)
(*           plan. At the cap or at a divergent pair, the round re-issues  *)
(*           a settle job.                                                 *)
(* A settle job (SpaceOp::Settle) is a round with one settle (phase job).  *)
(* It is a transaction of its own, so its count starts at 0.               *)
(*                                                                         *)
(* The settle (settle.rs with PriorityQueueView) fires the oldest fireable *)
(* waiter-lane head (readiness review S4). A head is fireable when the     *)
(* first level head that it sees and matches passes its guard. The record  *)
(* in flight counts as the head of its level. When that fire binds a       *)
(* stored record while a record is in flight, the record in flight lands   *)
(* first, and the view plans again. The settle lifts the record when it is *)
(* stored and not persistent (a consume of K_lift), and it offers the      *)
(* record again to the waiter. A record that a waiter that peeks did not   *)
(* spend stays in flight (decision 30). The record in flight lands at its  *)
(* ord with its identity when the settle has no further fire, at the cap   *)
(* and at a divergent pair (decision 45, Rule E). A peek of a record in    *)
(* the plan of a receive removes the record and stores it again at once    *)
(* (Rule E), so the records do not change.                                 *)
(*                                                                         *)
(* Each phase is a sequence of steps of the model, one step for each fire. *)
(* No other round runs between the steps of a round, because a round is    *)
(* one space transaction. The model explores every schedule of MaxOps      *)
(* rounds with every value, pattern, level, lane, persistence and peek of  *)
(* its bounds.                                                             *)
(*                                                                         *)
(* Invariants (TypeOK holds the types):                                    *)
(*   PqLevelOrder      every fire binds the head of its lane and the head  *)
(*                     of its level. A receive and a settle bind the first *)
(*                     level head that the lane of the waiter sees and     *)
(*                     matches, in ascending level order. The datum of a   *)
(*                     send precedes every stored record of its level, and *)
(*                     the lane of the fired waiter sees that level. So    *)
(*                     each level and each lane leaves in FIFO order       *)
(*                     (Rocq: pq_level_order, pq_fifo_within_level,        *)
(*                     pq_restricted_waiter_in_level_queue and             *)
(*                     pq_unrestricted_first_matching_level).              *)
(*   PqOldestFirst     every fire of a settle fires the oldest fireable    *)
(*                     waiter-lane head, and every fire of a send fires    *)
(*                     the oldest candidate that fits (Rocq:               *)
(*                     pq_settle_oldest_first).                            *)
(*   PqNoPendingMatch  with no round in progress, no element in flight and *)
(*                     no settle job, no waiter-lane head can fire (INV;   *)
(*                     Rocq: pq_produce_preserves_inv and                  *)
(*                     pq_consume_preserves_inv). Every completed deploy   *)
(*                     ends in such a state.                               *)
(*   NoRecordLost      a record that was stored is stored, or it is in     *)
(*                     flight in the current settle, or a fire spent it    *)
(*                     (Rocq: pq_settle_keeps_unspent_records).            *)
(*   ReissueKeepsOrd   a re-issued persistent element carries the ord of   *)
(*                     its first round, and a re-issued element that is    *)
(*                     not persistent carries none (decision 43; Rocq:     *)
(*                     pq_reissue_takes_level_fifo_place and               *)
(*                     pq_reentered_waiter_keeps_seniority).               *)
(*   TxnFiresBounded   no transaction makes more than Cap fires, its       *)
(*                     settles included (revision I-21; Rocq:              *)
(*                     pq_settle_terminates). The ghost txnFires counts    *)
(*                     the fires of the round in progress, or of the last  *)
(*                     round while no round is in progress.                *)
(*                                                                         *)
(* Unsafe controls (CONSTANT flags). Each one refutes exactly one          *)
(* invariant, and the complement run without that invariant must find no   *)
(* violation:                                                              *)
(*   NoSettle = TRUE       PqNoSettleUnsafe refutes PqNoPendingMatch. The  *)
(*                         hooks of the priority queue do nothing, so a    *)
(*                         fire can expose a pair. In the shortest trace,  *)
(*                         the sends of a and of b at the level 0 are      *)
(*                         stored, in this order. The unrestricted receive *)
(*                         of b parks, because the level head a does not   *)
(*                         match it. The receive of a in the waiter queue  *)
(*                         of the level 0 takes a and exposes b, which the *)
(*                         parked receive matches. No round is in progress *)
(*                         and no element is in flight, so INV fails. The  *)
(*                         fires of the plans still bind heads, and no     *)
(*                         record is lifted, so the other invariants hold  *)
(*                         (Rocq: pq_without_settle_breaks_inv).           *)
(*   AnyOrder = TRUE       PqAnyOrderUnsafe refutes PqOldestFirst. The     *)
(*                         settle and the produce plan fire any fireable   *)
(*                         head. In the shortest trace, the unrestricted   *)
(*                         receive of a parks, and then the receive of a   *)
(*                         in the waiter queue of the level 0 parks. The   *)
(*                         send of a at the level 0 fires the younger head *)
(*                         of the level queue, although the older          *)
(*                         unrestricted head fits. Every fire still binds  *)
(*                         heads, and every settle still ends with no      *)
(*                         pending match, so the other invariants hold     *)
(*                         (Rocq: pq_settle_order_matters).                *)
(*   ReentryAtHead = TRUE  PqReentryAtHeadUnsafe refutes PqLevelOrder. A   *)
(*                         re-entering element plans at the head of its    *)
(*                         lane whatever the ords, so it can fire while an *)
(*                         older entry of its lane waits. Its cfg narrows  *)
(*                         the universe and allows seven rounds, because a *)
(*                         FIFO lane makes this case rare. In the shortest *)
(*                         trace, the sends of a, a and of a persistent b  *)
(*                         at the level 0 are stored. A persistent         *)
(*                         unrestricted receive of a fires both records a  *)
(*                         and re-issues at the cap with its ord 3. A      *)
(*                         persistent unrestricted receive of b meets the  *)
(*                         persistent record b, a divergent pair, and      *)
(*                         re-issues with its ord 4. The receive of a      *)
(*                         re-enters and parks, because b does not match   *)
(*                         it. The receive of b then re-enters and fires,  *)
(*                         although the older receive of a waits in its    *)
(*                         lane. The settle still restores INV, so the     *)
(*                         other invariants hold.                          *)
(*   PerPhaseCount = TRUE  PqPerPhaseCountUnsafe refutes TxnFiresBounded.  *)
(*                         Each phase starts its own count of fires at 0,  *)
(*                         as each settle call and each plan did before    *)
(*                         revision I-21. In the shortest trace, the send  *)
(*                         of b at the level 0 is stored. The unrestricted *)
(*                         peek receive of a parks, and the persistent     *)
(*                         unrestricted receive of any parks behind it.    *)
(*                         The send of a at the unprioritized level fires  *)
(*                         the peek receive and offers the datum again to  *)
(*                         the persistent receive (Cap = 2 fires). The     *)
(*                         settle after the plan starts its own count at   *)
(*                         0, and it fires the record b with the           *)
(*                         persistent receive. This is a third fire in one *)
(*                         round. Every phase still stops at Cap fires of  *)
(*                         its own, so the other invariants hold.          *)
(*   NoPlanOpenCheck = TRUE                                                *)
(*                         PqPlanOpensAtCapUnsafe refutes TxnFiresBounded. *)
(*                         open_plan has no cap check, so a plan opens     *)
(*                         after a settle before the plan that ended at    *)
(*                         the cap. Its cfg narrows the universe and       *)
(*                         allows six rounds. In the shortest trace, the   *)
(*                         sends of b and a at the level 0 are stored. The *)
(*                         unrestricted receive of a parks, and the        *)
(*                         persistent unrestricted receive of any parks    *)
(*                         behind it. A persistent send of a at the        *)
(*                         unprioritized level fires the receive of a and  *)
(*                         meets the persistent receive, a divergent pair. *)
(*                         It re-issues with no settle after the plan, so  *)
(*                         the two records face the persistent receive.    *)
(*                         The settle before the plan of a persistent send *)
(*                         of a at the level 0 fires both records and ends *)
(*                         at the cap. The plan then opens and fires a     *)
(*                         third time. TypeOK bounds txn.fired by Nat      *)
(*                         only, so that this control refutes exactly      *)
(*                         TxnFiresBounded.                                *)
(*                                                                         *)
(* Code (under rholang/src/rust/interpreter):                              *)
(*   LevelHeads, LaneHeads spaces/disciplines/priority_queue.rs:355-411    *)
(*                         (lane_heads, record_heads and waiter_heads: the *)
(*                         least ord, then the lesser bincode bytes)       *)
(*   Sees, FirstMatch      spaces/disciplines/priority_queue.rs:186-192    *)
(*                         (Lane::sees) and 510-536 (first_match)          *)
(*   Fireable, FireCandidates, Oldest                                      *)
(*                         spaces/disciplines/priority_queue.rs:978-1072   *)
(*                         (PriorityQueueView::next_fire: the guard once,  *)
(*                         and the oldest fireable head)                   *)
(*   AtLaneHead            spaces/disciplines/priority_queue.rs:254-268    *)
(*                         and 416-437 (reaches_head, precedes and         *)
(*                         at_lane_head)                                   *)
(*   AfterFire, Cap        reified/costs.rs:44, 322-333 (FIRE_ROUND_CAP,   *)
(*                         FireRounds::after_fire)                         *)
(*   txn.fired             spaces/generic.rs:707, 772 and 2426 (one        *)
(*                         FireRounds for each transaction: Plan.rounds,   *)
(*                         Cx::rounds, and the new count of run_with)      *)
(*   SettleStep            spaces/settle.rs:404-498 (settle, with the      *)
(*                         landing before a fire of a stored record at     *)
(*                         425-430 and the cap check at 431),              *)
(*                         spaces/settle.rs:508-513 (land) and 519-563     *)
(*                         (lift), and                                     *)
(*                         spaces/disciplines/priority_queue.rs:446-506    *)
(*                         (record_head_list with the record in flight)    *)
(*   SettleReissue         spaces/settle.rs:569-582, 588-601 and 606-616   *)
(*                         (settle_before_plan, settle_after_plan and      *)
(*                         settle_job) and spaces/settle.rs:643-675,       *)
(*                         681-703 (reissued_operation, settle_operation)  *)
(*   PlanStart             spaces/generic.rs:1556-1569 (open_plan) and     *)
(*                         spaces/disciplines/priority_queue.rs:765-824    *)
(*                         and 821-924 (the head check of offer_pq and     *)
(*                         receive_pq)                                     *)
(*   OfferStep             spaces/disciplines/priority_queue.rs:545-597    *)
(*                         (offer_target) and 756-815 (offer_pq)           *)
(*   ReceiveStep           spaces/disciplines/priority_queue.rs:830-933    *)
(*                         and 929-938 (receive_pq and restore_peeked)     *)
(*   Reissued              spaces/disciplines/priority_queue.rs:816 and    *)
(*                         916 (reissued_ord:                              *)
(*                         op.persistent.then_some(ord))                   *)
(*   Begin, SettleJob      spaces/generic.rs:2349-2513 (run_with of        *)
(*                         FamilySpace: the bump, then the plan) and       *)
(*                         spaces/counter.rs:165-245 (bump)                *)
(* The Rocq module PriorityQueue.v proves the same properties for every    *)
(* matcher, every guard and every fire-round cap.                          *)
(*                                                                         *)
(* Abstractions:                                                           *)
(*   - The values a and b. A pattern accepts one value, every value (any), *)
(*     or every value with a guard that refuses a (notA). A persistent     *)
(*     receive never peeks (CONSUME_ARROWS admits no persistent peek).     *)
(*   - PrioLevels holds the priority levels of the model, and Unp is the   *)
(*     unprioritized level. SendLevels and ReceiveLanes hold the levels of *)
(*     the sends and the lanes of the receives. A cfg can narrow them to   *)
(*     reach a deep trace in a small state space.                          *)
(*   - Records and waiters are sets. The ords are unique, so the head of a *)
(*     level or of a lane is its entry with the least ord, and the bincode *)
(*     tie-break never applies. The Rocq module covers equal ords through  *)
(*     the rank.                                                           *)
(*   - Cap is FIRE_ROUND_CAP of the model, small so that the cap is        *)
(*     reachable. One count of fires (txn.fired) runs through the phases   *)
(*     of each round, as in the code (revision I-21). Begin and SettleJob  *)
(*     start a transaction, so only they reset it.                         *)
(*   - MaxOps bounds the rounds (operations, re-entries and settle jobs),  *)
(*     because a divergent pair re-issues until the phlo is spent.         *)
(*   - An element in flight records its side, its slot (the level of a     *)
(*     datum or the lane of a continuation), the ord that it carries       *)
(*     (NoOrd for none) and the ghost first-round ord first.               *)
(*   - A send marks every candidate that refuses its datum, also a younger *)
(*     candidate that the code does not test. A refused head stays the     *)
(*     head of its lane, and it refuses the same datum again, so the       *)
(*     result is the same.                                                 *)
(*   - The identities of K_lift are not modeled, as in                     *)
(*     StackDiscipline.tla.                                                *)
(*   - History variables replace the fire logs: levelOk and oldestOk say   *)
(*     that every fire so far kept PqLevelOrder and PqOldestFirst. The     *)
(*     ghosts born and spent hold the ords of the records that were stored *)
(*     and of the records that a fire spent. The ghost txnFires counts     *)
(*     every fire of the round, also the last fire of a round that         *)
(*     re-issues.                                                          *)
(***************************************************************************)
EXTENDS Integers, FiniteSets

CONSTANTS
    \* The number of rounds of one run: every operation, every re-entry and
    \* every settle job bumps the one counter of the priority queue once.
    MaxOps,
    \* FIRE_ROUND_CAP of the model: the fires of one space transaction, its
    \* settles included (revision I-21).
    Cap,
    \* The values of the sends.
    Vals,
    \* The patterns of the receives: a value, "any" (every value), and
    \* "notA" (every value spatially, with a guard that refuses "a").
    Pats,
    \* The priority levels of the model. The unprioritized level Unp comes
    \* after them, and each one has a waiter queue.
    PrioLevels,
    \* The levels of the sends (a subset of AllLevels).
    SendLevels,
    \* The lanes of the receives (a subset of AllLanes).
    ReceiveLanes,
    \* TRUE: persistent sends (!!), persistent receives (<=) and peek
    \* receives. FALSE: plain sends and receives only.
    Reissues,
    \* TRUE: the Unsafe priority queue without the settle hooks.
    NoSettle,
    \* TRUE: the Unsafe settle and produce plan that fire any fireable head,
    \* not the oldest one.
    AnyOrder,
    \* TRUE: the Unsafe re-entry at the head of its lane whatever the ords.
    ReentryAtHead,
    \* TRUE: the Unsafe count of fires for each phase (each settle call and
    \* each plan starts its own count, as before revision I-21).
    PerPhaseCount,
    \* TRUE: the Unsafe open_plan with no cap check (the plan opens after a
    \* settle before the plan that ended at the cap).
    NoPlanOpenCheck

ASSUME MaxOps \in 1..12
ASSUME Cap \in 1..4
ASSUME Vals \subseteq {"a", "b"} /\ Vals # {}
ASSUME Pats \subseteq {"a", "b", "any", "notA"} /\ Pats # {}
ASSUME PrioLevels \subseteq 0..9
ASSUME {Reissues, NoSettle, AnyOrder, ReentryAtHead, PerPhaseCount, NoPlanOpenCheck}
         \subseteq BOOLEAN

VARIABLES
    \* The stored records.
    records,
    \* The parked waiters.
    waiters,
    \* The re-issued operations in flight (F of decision 43).
    pending,
    \* The number of re-issued settle jobs.
    jobs,
    \* The counter of the priority queue: the ord of the next fresh round.
    counter,
    \* The round in progress: its phase, its element and its ord. The count
    \* fired holds the fires of the transaction so far (Cx::rounds). The
    \* round also holds the record in flight of its settle and the refused
    \* lanes of its produce plan.
    txn,
    \* History: every fire and every lift so far kept PqLevelOrder.
    levelOk,
    \* History: every fire of a settle and of a produce plan so far kept
    \* PqOldestFirst.
    oldestOk,
    \* Ghost: the ords of the records that were stored.
    born,
    \* Ghost: the ords of the records that a fire spent.
    spent,
    \* Ghost: the fires of the round in progress, or of the last round while
    \* no round is in progress. Only Begin and SettleJob reset it.
    txnFires

vars == <<records, waiters, pending, jobs, counter, txn, levelOk, oldestOk, born, spent,
          txnFires>>

\* The unprioritized level: it comes after every priority level (F6).
Unp == 99
\* The unrestricted waiter queue.
AnyLane == -1
Levels == PrioLevels \cup {Unp}
Lanes == PrioLevels \cup {AnyLane}
\* The sets that a cfg binds to SendLevels and ReceiveLanes, for example
\* SendLevels <- AllLevels.
AllLevels == Levels
AllLanes == Lanes
PrioOnly == PrioLevels
AnyOnly == {AnyLane}
ASSUME SendLevels \subseteq Levels /\ SendLevels # {}
ASSUME ReceiveLanes \subseteq Lanes /\ ReceiveLanes # {}
Ords == 0..MaxOps
\* The ord of an element that carries none (reissued_ord = None).
NoOrd == -1
Carried == {NoOrd} \cup Ords
Phases == {"idle", "before", "plan", "loop", "after", "job"}

\* The spatial match of a pattern (Cx::bind).
Matches(p, v) == p \in {"any", "notA"} \/ p = v
\* The guard over the match (Cx::admits).
Admits(p, v) == ~(p = "notA" /\ v = "a")
\* Cx::matches: the spatial match, then the guard.
Fits(p, v) == Matches(p, v) /\ Admits(p, v)
\* Lane::sees: a level queue sees its own level, and the unrestricted
\* queue sees every level.
Sees(lane, lvl) == lane = AnyLane \/ lane = lvl

\* No record in flight.
NoRec == [ord |-> NoOrd, val |-> "a", lvl |-> Unp, pers |-> FALSE]
\* The element of a settle job, and of no round.
NoElt == [side |-> "settle", ord |-> NoOrd, first |-> NoOrd, term |-> "a", slot |-> Unp,
          pers |-> FALSE, peek |-> FALSE]
\* No round in progress.
Idle == [phase |-> "idle", elt |-> NoElt, ord |-> NoOrd, fired |-> 0, flight |-> NoRec,
         refused |-> {}]

\* The head of a level is its record with the least ord, and the head of a
\* lane is its waiter with the least ord.
IsLevelHead(r, S) == r \in S /\ \A s \in S : s.lvl = r.lvl => r.ord <= s.ord
LevelHeads(S) == {r \in S : IsLevelHead(r, S)}
IsLaneHead(w, W) == w \in W /\ \A x \in W : x.lane = w.lane => w.ord <= x.ord
LaneHeads(W) == {w \in W : IsLaneHead(w, W)}

\* first_match: the level head with the least level that the lane sees and
\* whose payload matches the pattern (one head at most).
FirstMatch(S, lane, pat) ==
    LET ok == {h \in LevelHeads(S) : Sees(lane, h.lvl) /\ Matches(pat, h.val)}
    IN {h \in ok : \A g \in ok : h.lvl <= g.lvl}

\* The record that a waiter fires on the records S: its first match, when
\* its guard admits that match. The guard is checked once (decision 38).
Fireable(S, w) == {h \in FirstMatch(S, w.lane, w.pat) : Admits(w.pat, h.val)}

\* The fireable waiter-lane heads, and the oldest of them.
FireCandidates(S, W) == {w \in LaneHeads(W) : Fireable(S, w) # {}}
Oldest(C) == {w \in C : \A u \in C : w.ord <= u.ord}

\* FireRounds::after_fire with the count of fires after this one.
AfterFire(n, isSpent, stays) ==
    IF isSpent THEN "done"
    ELSE IF stays THEN "divergent"
    ELSE IF Cap <= n THEN "cap"
    ELSE "continue"

\* operation_ord: the carried ord of a re-entering element, or the counter
\* value for a fresh element.
OperationOrd(x) == IF x.ord # NoOrd THEN x.ord ELSE counter

\* The re-issue of the element x after its round with the ord o. A
\* persistent element carries its first-round ord
\* (op.persistent.then_some(ord)). Another element carries none.
Reissued(x, o) ==
    [x EXCEPT !.ord = IF x.pers THEN (IF x.ord # NoOrd THEN x.ord ELSE o) ELSE NoOrd]

\* at_lane_head for the element x of the round with the ord o. A fresh
\* element joins the tail of its lane, so it is at the head only of an empty
\* lane. A re-entering element is at the head when its lane is empty or its
\* ord is less than every ord of its lane (precedes is <). Unsafe
\* ReentryAtHead puts a re-entering element at the head whatever the ords.
AtLaneHead(x, o) ==
    LET own == IF x.side = "produce"
               THEN {r.ord : r \in {s \in records : s.lvl = x.slot}}
               ELSE {w.ord : w \in {u \in waiters : u.lane = x.slot}}
    IN IF x.ord = NoOrd THEN own = {}
       ELSE ReentryAtHead \/ \A q \in own : o < q

\* The record and the waiter of the element x with the ord o.
AsRecord(x, o) == [ord |-> o, val |-> x.term, lvl |-> x.slot, pers |-> x.pers]
AsWaiter(x, o) == [ord |-> o, pat |-> x.term, lane |-> x.slot, pers |-> x.pers, peek |-> x.peek]

\* The landing of the record in flight f (Cx::restore_on).
Land(S, f) == IF f.ord # NoOrd THEN S \cup {f} ELSE S

\* The element x with the ord o is stored or parked.
StoreOrPark(x, o) ==
    IF x.side = "produce"
    THEN /\ records' = records \cup {AsRecord(x, o)}
         /\ born' = born \cup {o}
         /\ UNCHANGED waiters
    ELSE /\ waiters' = waiters \cup {AsWaiter(x, o)}
         /\ UNCHANGED <<records, born>>

\* The count of fires at the start of the next phase of the round, from the
\* count n (revision I-21). Unsafe PerPhaseCount starts the count of each
\* phase at 0.
PhaseCount(n) == IF PerPhaseCount THEN 0 ELSE n

\* A settle that cannot complete re-issues. After the settle before the
\* plan, it re-issues the operation itself. After the settle after the plan
\* and after the settle of a settle job, it re-issues a settle job.
SettleReissue ==
    IF txn.phase = "before"
    THEN /\ pending' = pending \cup {Reissued(txn.elt, txn.ord)}
         /\ UNCHANGED jobs
    ELSE /\ jobs' = jobs + 1
         /\ UNCHANGED pending

-----------------------------------------------------------------------------
(* The rounds                                                              *)

Init ==
    /\ records = {}
    /\ waiters = {}
    /\ pending = {}
    /\ jobs = 0
    /\ counter = 0
    /\ txn = Idle
    /\ levelOk = TRUE
    /\ oldestOk = TRUE
    /\ born = {}
    /\ spent = {}
    /\ txnFires = 0

\* The start of a round of the element x: the bump, then the settle before
\* the plan (run_with of FamilySpace). A fresh element records the counter
\* value as its first-round ord. A round is a new transaction, so its count
\* of fires starts at 0.
Begin(x) ==
    /\ txn.phase = "idle"
    /\ counter < MaxOps
    /\ counter' = counter + 1
    /\ txn' = [phase |-> "before",
               elt |-> IF x.first = NoOrd THEN [x EXCEPT !.first = counter] ELSE x,
               ord |-> OperationOrd(x), fired |-> 0, flight |-> NoRec, refused |-> {}]
    /\ txnFires' = 0

\* A fresh send with the value v at the level l (persistent: !!).
Produce(v, l, pers) ==
    /\ pers => Reissues
    /\ Begin([side |-> "produce", ord |-> NoOrd, first |-> NoOrd, term |-> v, slot |-> l,
              pers |-> pers, peek |-> FALSE])
    /\ UNCHANGED <<records, waiters, pending, jobs, levelOk, oldestOk, born, spent>>

\* A fresh receive with the pattern p in the lane l (persistent: <=). A
\* persistent receive never peeks.
Consume(p, l, pers, peek) ==
    /\ (pers \/ peek) => Reissues
    /\ ~(pers /\ peek)
    /\ Begin([side |-> "consume", ord |-> NoOrd, first |-> NoOrd, term |-> p, slot |-> l,
              pers |-> pers, peek |-> peek])
    /\ UNCHANGED <<records, waiters, pending, jobs, levelOk, oldestOk, born, spent>>

\* The next round of the re-issued operation e (a new space intent).
Reenter(e) ==
    /\ Begin(e)
    /\ pending' = pending \ {e}
    /\ UNCHANGED <<records, waiters, jobs, levelOk, oldestOk, born, spent>>

\* The round of a re-issued settle job: the bump, then one settle. A settle
\* job is a transaction of its own, so its count of fires starts at 0.
SettleJob ==
    /\ txn.phase = "idle"
    /\ jobs > 0
    /\ counter < MaxOps
    /\ counter' = counter + 1
    /\ jobs' = jobs - 1
    /\ txn' = [Idle EXCEPT !.phase = "job"]
    /\ txnFires' = 0
    /\ UNCHANGED <<records, waiters, pending, levelOk, oldestOk, born, spent>>

\* One step of the settle of the round (settle.rs with PriorityQueueView). F
\* is the oldest fireable waiter-lane head (readiness review S4), and the
\* record that it fires on the view. The record in flight counts as the
\* head of its level in the view. When F binds a stored record while a
\* record is in flight, the record in flight lands first, and the view
\* plans again. A stored record that is not persistent is lifted before its
\* re-offer. A record that the fire did not spend stays in flight when it
\* is out of the store. The cap check counts the fires of the whole round
\* (I-21). Unsafe NoSettle ends every settle at once, and Unsafe AnyOrder
\* fires any fireable head.
SettleStep ==
    /\ txn.phase \in {"before", "after", "job"}
    /\ LET flight == txn.flight
           inFlight == flight.ord # NoOrd
           view == IF inFlight THEN records \cup {flight} ELSE records
           cands == FireCandidates(view, waiters)
           chosen == IF AnyOrder THEN cands ELSE Oldest(cands)
       IN
       IF NoSettle \/ cands = {}
       THEN \* Quiescent: land the record in flight and end the settle.
            /\ records' = Land(records, flight)
            /\ txn' = IF txn.phase = "before"
                      THEN [txn EXCEPT !.phase = "plan", !.fired = PhaseCount(@), !.flight = NoRec]
                      ELSE Idle
            /\ UNCHANGED <<waiters, pending, jobs, counter, levelOk, oldestOk, born, spent,
                           txnFires>>
       ELSE \E w \in chosen : \E r \in Fireable(view, w) :
            IF inFlight /\ r # flight
            THEN \* The record in flight has no fire: it lands, and the view
                 \* plans again with the record in its place.
                 /\ records' = records \cup {flight}
                 /\ txn' = [txn EXCEPT !.flight = NoRec]
                 /\ UNCHANGED <<waiters, pending, jobs, counter, levelOk, oldestOk, born, spent,
                                txnFires>>
            ELSE IF Cap <= txn.fired
            THEN \* At the cap: land the record in flight and re-issue.
                 /\ records' = Land(records, flight)
                 /\ SettleReissue
                 /\ txn' = Idle
                 /\ UNCHANGED <<waiters, counter, levelOk, oldestOk, born, spent, txnFires>>
            ELSE LET lifted == ~inFlight /\ ~r.pers
                     stored == IF lifted THEN records \ {r} ELSE records
                     isSpent == ~r.pers /\ ~w.peek
                     flight1 == IF (inFlight \/ lifted) /\ ~isSpent THEN r ELSE NoRec
                     step == AfterFire(txn.fired + 1, isSpent, w.pers)
                 IN
                 /\ levelOk' = (levelOk /\ IsLaneHead(w, waiters) /\ IsLevelHead(r, view)
                                /\ r \in FirstMatch(view, w.lane, w.pat))
                 /\ oldestOk' = (oldestOk /\ \A u \in cands : w.ord <= u.ord)
                 /\ waiters' = IF w.pers THEN waiters ELSE waiters \ {w}
                 /\ spent' = IF isSpent THEN spent \cup {r.ord} ELSE spent
                 /\ txnFires' = txnFires + 1
                 /\ IF step \in {"done", "continue"}
                    THEN /\ records' = stored
                         /\ txn' = [txn EXCEPT !.fired = @ + 1, !.flight = flight1]
                         /\ UNCHANGED <<pending, jobs>>
                    ELSE \* A divergent pair or the cap: land and re-issue.
                         /\ records' = Land(stored, flight1)
                         /\ SettleReissue
                         /\ txn' = Idle
                 /\ UNCHANGED <<counter, born>>

\* The start of the plan (open_plan, then at_lane_head). The settle before
\* the plan can leave the round at the cap. Then the plan stages nothing,
\* and the operation re-issues with the order rule of decision 43 (I-21).
\* Else the element is at the head of its lane, or it is stored or parked
\* at its FIFO place with no fire. The plan continues the count of the
\* settle before it. Unsafe NoPlanOpenCheck opens the plan also at the cap.
PlanStart ==
    /\ txn.phase = "plan"
    /\ IF ~NoPlanOpenCheck /\ Cap <= txn.fired
       THEN /\ pending' = pending \cup {Reissued(txn.elt, txn.ord)}
            /\ txn' = Idle
            /\ UNCHANGED <<records, waiters, born>>
       ELSE IF AtLaneHead(txn.elt, txn.ord)
       THEN /\ txn' = [txn EXCEPT !.phase = "loop", !.fired = PhaseCount(@), !.refused = {}]
            /\ UNCHANGED <<records, waiters, born, pending>>
       ELSE /\ StoreOrPark(txn.elt, txn.ord)
            /\ txn' = [txn EXCEPT !.phase = "after", !.fired = PhaseCount(@)]
            /\ UNCHANGED pending
    /\ UNCHANGED <<jobs, counter, levelOk, oldestOk, spent, txnFires>>

\* After a fire of the plan, the plan completes (the element is spent), or
\* it goes on with the refused lanes ref. Else it re-issues the operation
\* with no settle after the plan. The settle after the plan continues the
\* count of the plan (I-21).
PlanNext(step, ref) ==
    CASE step = "done" -> /\ txn' = [txn EXCEPT !.phase = "after", !.fired = PhaseCount(@ + 1)]
                          /\ UNCHANGED pending
      [] step = "continue" -> /\ txn' = [txn EXCEPT !.fired = @ + 1, !.refused = ref]
                              /\ UNCHANGED pending
      [] OTHER -> /\ pending' = pending \cup {Reissued(txn.elt, txn.ord)}
                  /\ txn' = Idle

\* One step of the fire loop of offer_pq for the datum of the round. The
\* candidates are the heads of the waiter queue of its level and of the
\* unrestricted queue, without the refused lanes. The older candidate that
\* fits fires (decision 38), and each candidate that refuses refuses its
\* lane (decision 37). The fire spends the datum when the datum is not
\* persistent and the waiter does not peek, and the waiter stays when it is
\* persistent. Unsafe AnyOrder fires any candidate that fits.
OfferStep ==
    LET x == txn.elt
        o == txn.ord
        cands == {w \in LaneHeads(waiters) :
                    (w.lane = x.slot \/ w.lane = AnyLane) /\ w.lane \notin txn.refused}
        fitting == {w \in cands : Fits(w.pat, x.term)}
        chosen == IF AnyOrder THEN fitting ELSE Oldest(fitting)
    IN
    IF fitting = {}
    THEN /\ StoreOrPark(x, o)
         /\ txn' = [txn EXCEPT !.phase = "after", !.fired = PhaseCount(@)]
         /\ UNCHANGED <<pending, levelOk, oldestOk, spent, txnFires>>
    ELSE \E w \in chosen :
         LET step == AfterFire(txn.fired + 1, ~x.pers /\ ~w.peek, w.pers)
             ref == txn.refused \cup {c.lane : c \in cands \ fitting}
         IN /\ levelOk' = (levelOk /\ IsLaneHead(w, waiters) /\ Sees(w.lane, x.slot)
                           /\ \A h \in records : h.lvl = x.slot => o < h.ord)
            /\ oldestOk' = (oldestOk /\ \A u \in fitting : w.ord <= u.ord)
            /\ waiters' = IF w.pers THEN waiters ELSE waiters \ {w}
            /\ txnFires' = txnFires + 1
            /\ PlanNext(step, ref)
            /\ UNCHANGED <<records, born, spent>>

\* One step of the fire loop of receive_pq for the continuation of the
\* round: its first match, and its guard once (decision 38). A refusal
\* parks. The fire spends the continuation when it is not persistent, and
\* the record stays when it is persistent or the continuation peeks. A peek
\* removes the record and stores it again at once (Rule E), so the records
\* do not change.
ReceiveStep ==
    LET k == txn.elt
        o == txn.ord
        hs == FirstMatch(records, k.slot, k.term)
    IN
    IF \A h \in hs : ~Admits(k.term, h.val)
    THEN /\ StoreOrPark(k, o)
         /\ txn' = [txn EXCEPT !.phase = "after", !.fired = PhaseCount(@)]
         /\ UNCHANGED <<pending, levelOk, oldestOk, spent, txnFires>>
    ELSE \E h \in hs :
         LET removes == ~h.pers /\ ~k.peek
             step == AfterFire(txn.fired + 1, ~k.pers, h.pers \/ k.peek)
         IN /\ levelOk' = (levelOk /\ IsLevelHead(h, records)
                           /\ \A u \in waiters : u.lane = k.slot => o < u.ord)
            /\ records' = IF removes THEN records \ {h} ELSE records
            /\ spent' = IF removes THEN spent \cup {h.ord} ELSE spent
            /\ txnFires' = txnFires + 1
            /\ PlanNext(step, txn.refused)
            /\ UNCHANGED <<waiters, born, oldestOk>>

PlanLoop ==
    /\ txn.phase = "loop"
    /\ IF txn.elt.side = "produce" THEN OfferStep ELSE ReceiveStep
    /\ UNCHANGED <<jobs, counter>>

Next ==
    \/ \E v \in Vals, l \in SendLevels, pers \in BOOLEAN : Produce(v, l, pers)
    \/ \E p \in Pats, l \in ReceiveLanes, pers \in BOOLEAN, peek \in BOOLEAN :
         Consume(p, l, pers, peek)
    \/ \E e \in pending : Reenter(e)
    \/ SettleJob
    \/ SettleStep
    \/ PlanStart
    \/ PlanLoop

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Invariants                                                              *)

RecType == [ord : Ords, val : Vals, lvl : Levels, pers : BOOLEAN]
WtrType == [ord : Ords, pat : Pats, lane : Lanes, pers : BOOLEAN, peek : BOOLEAN]
EltType == [side : {"produce", "consume"}, ord : Carried, first : Ords,
            term : Vals \cup Pats, slot : Levels \cup Lanes, pers : BOOLEAN, peek : BOOLEAN]

TypeOK ==
    /\ records \subseteq RecType
    /\ waiters \subseteq WtrType
    /\ pending \subseteq EltType
    /\ jobs \in 0..MaxOps
    /\ counter \in 0..MaxOps
    /\ txn.phase \in Phases
    /\ txn.elt \in EltType \cup {NoElt}
    /\ txn.ord \in Carried
    /\ txn.fired \in Nat
    /\ txn.flight \in RecType \cup {NoRec}
    /\ txn.refused \subseteq Lanes
    /\ levelOk \in BOOLEAN
    /\ oldestOk \in BOOLEAN
    /\ born \subseteq Ords
    /\ spent \subseteq Ords
    /\ txnFires \in Nat

PqLevelOrder == levelOk

PqOldestFirst == oldestOk

PqNoPendingMatch ==
    (txn.phase = "idle" /\ pending = {} /\ jobs = 0) => FireCandidates(records, waiters) = {}

NoRecordLost ==
    \A o \in born : (\E r \in records : r.ord = o) \/ txn.flight.ord = o \/ o \in spent

ReissueKeepsOrd == \A f \in pending : IF f.pers THEN f.ord = f.first ELSE f.ord = NoOrd

\* No transaction makes more than Cap fires, its settles included (I-21).
TxnFiresBounded == txnFires <= Cap

=============================================================================
