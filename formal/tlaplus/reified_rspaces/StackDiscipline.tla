-------------------------- MODULE StackDiscipline --------------------------
(***************************************************************************)
(* The stack discipline of the native spaces (plan decision 11) and the    *)
(* settle of a collection (the P5 design v3, section A.8, choice F2).      *)
(*                                                                         *)
(* A stack holds records (data) and waiters (waiting continuations). Each  *)
(* round of an operation is one space transaction, and every round bumps   *)
(* the collection counter (decision 31). The top of a side is its entry    *)
(* with the greatest ord, and only the two tops interact. A fresh element  *)
(* takes the counter value as its ord, so it is always the top of its side *)
(* (reaches_head is true).                                                 *)
(*                                                                         *)
(* A round of an operation has these phases (offer_head and receive_head   *)
(* with the hooks of StackDiscipline). One space transaction makes at most *)
(* Cap fires, its settles included (revision I-21). So one count of fires  *)
(* (txn.fired, Cx::rounds) runs through the phases of a round, and only a  *)
(* new round resets it:                                                    *)
(*   before  the settle before the plan. It starts the count at 0. At the  *)
(*           cap or at a divergent pair, the round re-issues the operation *)
(*           (decision 43 order).                                          *)
(*   plan    open_plan: when the settle before the plan ended at the cap,  *)
(*           the plan stages nothing and the round re-issues the operation *)
(*           (decision 43 order). Else a fresh element is at the top of    *)
(*           its side. A re-entering persistent element is at the top when *)
(*           its side is empty. It is also at the top when its ord is      *)
(*           greater than the ord of the top (precedes is >). Else it is   *)
(*           stored or parked at its LIFO place with no fire.              *)
(*   loop    at the top, the element fires the top of the other side while *)
(*           that top fits, and then it is stored or parked. The loop      *)
(*           continues the count of the settle before the plan. A          *)
(*           divergent pair or the cap re-issues the operation with no     *)
(*           settle after the plan.                                        *)
(*   after   the settle after the plan. It continues the count of the      *)
(*           plan. At the cap or at a divergent pair, the round re-issues  *)
(*           a settle job.                                                 *)
(* A settle job (SpaceOp::Settle) is a round with one settle (phase job).  *)
(* It is a transaction of its own, so its count starts at 0.               *)
(*                                                                         *)
(* The settle (settle.rs): while the top waiter fits the top record or the *)
(* record in flight, lift the record when it is stored and not persistent  *)
(* (a consume of K_lift), and offer it again to the waiter. A record that  *)
(* a waiter that peeks did not spend stays in flight (decision 30). The    *)
(* record in flight lands at its ord with its identity when the settle has *)
(* no further fire, at the cap and at a divergent pair (decision 45, Rule  *)
(* E). A peek of a record in the plan of a consume removes the record and  *)
(* stores it again at once (Rule E), so the records do not change.         *)
(*                                                                         *)
(* Each phase is a sequence of steps of the model, one step for each fire. *)
(* No other round runs between the steps of a round, because a round is    *)
(* one space transaction. The model explores every schedule of MaxOps      *)
(* rounds with every value, pattern, persistence and peek.                 *)
(*                                                                         *)
(* Invariants (TypeOK holds the types):                                    *)
(*   LifoOrder           every fire binds the top of the other side, and   *)
(*                       its incoming element (the datum, the continuation *)
(*                       or the record of a settle fire) has an ord at     *)
(*                       least the ord of every stored entry of its own    *)
(*                       side. Every lift takes the top record. So each    *)
(*                       side leaves in LIFO order (Rocq: stack_lifo).     *)
(*   StackNoPendingMatch with no round in progress, no element in flight   *)
(*                       and no settle job, the top record and the top     *)
(*                       waiter do not match (INV; Rocq:                   *)
(*                       stack_produce_preserves_inv and                   *)
(*                       stack_consume_preserves_inv). Every completed     *)
(*                       deploy ends in such a state.                      *)
(*   NoRecordLost        a record that was stored is stored, or it is in   *)
(*                       flight in the current settle, or a fire spent it  *)
(*                       (Rocq: stack_settle_keeps_unspent_records)        *)
(*   ReissueKeepsOrd     a re-issued persistent element carries the ord of *)
(*                       its first round, and a re-issued element that is  *)
(*                       not persistent carries none (decision 43; Rocq:   *)
(*                       stack_reissue_keeps_ord)                          *)
(*   TxnFiresBounded     no transaction makes more than Cap fires, its     *)
(*                       settles included (revision I-21). Rocq:           *)
(*                       stack_settle_terminates and txn_bounded. The      *)
(*                       ghost txnFires counts the fires of the round in   *)
(*                       progress, or of the last round while no round is  *)
(*                       in progress.                                      *)
(*                                                                         *)
(* Unsafe controls (CONSTANT flags). Each one refutes exactly one          *)
(* invariant, and the complement run without that invariant must find no   *)
(* violation:                                                              *)
(*   NoSettle = TRUE      StackNoSettleUnsafe refutes StackNoPendingMatch. *)
(*                        The hooks of the stack do nothing. A fire of a   *)
(*                        fresh element can then expose an older pair      *)
(*                        below the tops: the receive of a parks, the      *)
(*                        receive of b parks on top of it, the send of a   *)
(*                        is stored (the top waiter b does not fit it),    *)
(*                        and the send of b fires the top waiter. The top  *)
(*                        record a and the top waiter a then match with    *)
(*                        no element in flight (the counterexample of the  *)
(*                        P5 design v2, section 3.1; Rocq:                 *)
(*                        stack_without_settle_breaks_inv). The shortest   *)
(*                        trace of TLC is its mirror on the consume side:  *)
(*                        a and b are stored, the receive of a parks, and  *)
(*                        the receive of b fires b. The fires of the plans *)
(*                        still bind the tops, and no record is lifted, so *)
(*                        the other invariants hold.                       *)
(*   ReentryAtTop = TRUE  StackReentryAtTopUnsafe refutes LifoOrder. A     *)
(*                        re-entering element plans at the top whatever    *)
(*                        the ords, so it can fire while a younger entry   *)
(*                        of its own side is stored. In the shortest trace *)
(*                        a persistent receive of a re-issues after a      *)
(*                        divergent pair with a persistent record a, the   *)
(*                        receive of b parks, and the receive of a then    *)
(*                        fires below the waiter b. The settle after the   *)
(*                        plan still restores INV, and an element that     *)
(*                        does not fire is stored below the younger top,   *)
(*                        so the other invariants hold.                    *)
(*   PerPhaseCount = TRUE StackPerPhaseCountUnsafe refutes                 *)
(*                        TxnFiresBounded. Each phase starts its own count *)
(*                        of fires at 0, as each settle call and each plan *)
(*                        did before revision I-21. In the shortest trace, *)
(*                        a persistent receive of any value parks. A peek  *)
(*                        receive of a parks on top of it, and the send of *)
(*                        b is stored. The send of a fires the two waiters *)
(*                        (Cap = 2 fires). The settle after the plan then  *)
(*                        fires the record b with the persistent waiter.   *)
(*                        This is a third fire in one round. Every phase   *)
(*                        still stops at Cap fires of its own, so the      *)
(*                        other invariants hold.                           *)
(*   NoPlanOpenCheck      StackPlanOpensAtCapUnsafe refutes                *)
(*   = TRUE               TxnFiresBounded. open_plan has no cap check, so  *)
(*                        a plan opens after a settle before the plan that *)
(*                        ended at the cap. In the shortest trace, a       *)
(*                        persistent send meets a divergent pair. It       *)
(*                        re-issues with no settle after the plan, so two  *)
(*                        records face a persistent waiter. The settle     *)
(*                        before the plan of the next send fires both and  *)
(*                        ends at the cap, and the plan then fires a third *)
(*                        time. TypeOK bounds txn.fired by Nat only, so    *)
(*                        that this control refutes exactly                *)
(*                        TxnFiresBounded. The settles still stop at the   *)
(*                        cap, so the other invariants hold.               *)
(*                                                                         *)
(* Code (under rholang/src/rust/interpreter):                              *)
(*   TopRecords,          spaces/disciplines/stack.rs:75-93, 227-262       *)
(*   TopWaiters           (StackDiscipline::head_metered: the greatest     *)
(*                        ord, then the greater bincode bytes)             *)
(*   AtTop                spaces/disciplines/stack.rs:96, 99-108           *)
(*                        (reaches_head is true, precedes is ord > head)   *)
(*                        and spaces/generic.rs:745-766 (Cx::at_head)      *)
(*   AfterFire, Cap       reified/costs.rs:44, 322-333 (FIRE_ROUND_CAP,    *)
(*                        FireRounds::after_fire)                          *)
(*   txn.fired            spaces/generic.rs:530, 583 and 1828 (one         *)
(*                        FireRounds for each transaction: Plan.rounds,    *)
(*                        Cx::rounds, and the new count of run_with)       *)
(*   SettleStep           spaces/settle.rs:404-498 and 508-513 (settle     *)
(*                        and land), spaces/settle.rs:519-563 (lift),      *)
(*                        spaces/settle.rs:431 (the cap check of the       *)
(*                        transaction) and                                 *)
(*                        spaces/disciplines/stack.rs:159-212              *)
(*                        (StackView::next_fire)                           *)
(*   SettleReissue        spaces/settle.rs:569-582 (settle_before_plan     *)
(*                        re-issues the operation),                        *)
(*                        spaces/settle.rs:588-601 (settle_after_plan      *)
(*                        re-issues a settle job),                         *)
(*                        spaces/settle.rs:606-616 (settle_job) and        *)
(*                        spaces/settle.rs:643-675, 681-703                *)
(*                        (reissued_operation, settle_operation)           *)
(*   PlanStart            spaces/generic.rs:1156-1169 (open_plan: the      *)
(*                        settle before the plan, then the cap check of    *)
(*                        the transaction)                                 *)
(*   PlanStart, OfferStep spaces/generic.rs:1177-1244 (offer_head)         *)
(*   PlanStart,           spaces/generic.rs:1253-1337, 1340-1349           *)
(*   ReceiveStep          (receive_head, restore_peeked)                   *)
(*   Reissued             spaces/generic.rs:1236, 1329 (reissued_ord:      *)
(*                        op.persistent.then_some(ord))                    *)
(*   Begin, SettleJob     spaces/generic.rs:1784-1892 (run_with of         *)
(*                        FamilySpace: the bump, then the plan) and        *)
(*                        spaces/counter.rs:165-245 (bump)                 *)
(* The Rocq module StackDiscipline.v proves the same properties for every  *)
(* matcher and every fire-round cap.                                       *)
(*                                                                         *)
(* Abstractions:                                                           *)
(*   - Values a and b. A pattern accepts a value (a, b) or every value     *)
(*     (any). A persistent receive never peeks (CONSUME_ARROWS admits no   *)
(*     persistent peek).                                                   *)
(*   - Records and waiters are sets. The ords are unique, so the top is    *)
(*     the entry with the greatest ord and the bincode tie-break never     *)
(*     applies. The Rocq module covers equal ords through the rank.        *)
(*   - Cap is FIRE_ROUND_CAP of the model, small so that the cap is        *)
(*     reachable. One count of fires (txn.fired) runs through the phases   *)
(*     of each round, as in the code (revision I-21). Begin and SettleJob  *)
(*     start a transaction, so only they reset it.                         *)
(*   - MaxOps bounds the rounds (operations, re-entries and settle jobs),  *)
(*     because a divergent pair re-issues until the phlo is spent.         *)
(*   - An element in flight records its side, the ord that it carries      *)
(*     (NoOrd for none) and the ghost first-round ord first, which also    *)
(*     tells apart two elements in flight with the same value.             *)
(*   - The identities of K_lift are not modeled. The Rust tests            *)
(*     space_settle_lift_identity_is_unique_per_counter_and_step and       *)
(*     space_stack_two_settles_of_one_round_lift_with_distinct_steps pin   *)
(*     them.                                                               *)
(*   - History variables replace the fire logs: lifoOk says that every     *)
(*     fire and every lift so far kept LifoOrder. The ghosts born and      *)
(*     spent hold the ords of the records that were stored and of the      *)
(*     records that a fire spent. The ghost txnFires counts every fire of  *)
(*     the round. It also counts the last fire of a round that re-issues.  *)
(*     So it does not depend on the count txn.fired that the model checks. *)
(***************************************************************************)
EXTENDS Integers, FiniteSets

CONSTANTS
    \* The number of rounds of one run: every operation, every re-entry and
    \* every settle job bumps the counter once.
    MaxOps,
    \* FIRE_ROUND_CAP of the model: the fires of one space transaction, its
    \* settles included (revision I-21).
    Cap,
    \* TRUE: persistent sends (!!), persistent receives (<=) and peek
    \* receives. FALSE: plain sends and receives only.
    Reissues,
    \* TRUE: the Unsafe stack without the settle hooks.
    NoSettle,
    \* TRUE: the Unsafe re-entry at the top whatever the ords.
    ReentryAtTop,
    \* TRUE: the Unsafe count of fires for each phase (each settle call and
    \* each plan starts its own count, as before revision I-21).
    PerPhaseCount,
    \* TRUE: the Unsafe open_plan with no cap check (the plan opens after a
    \* settle before the plan that ended at the cap).
    NoPlanOpenCheck

ASSUME MaxOps \in 1..12
ASSUME Cap \in 1..4
ASSUME {Reissues, NoSettle, ReentryAtTop, PerPhaseCount, NoPlanOpenCheck} \subseteq BOOLEAN

VARIABLES
    \* The stored records.
    records,
    \* The parked waiters.
    waiters,
    \* The re-issued operations in flight (F of decision 43).
    pending,
    \* The number of re-issued settle jobs.
    jobs,
    \* The collection counter: the ord of the next fresh round.
    counter,
    \* The round in progress: its phase, its element, its ord, the fires of
    \* the transaction so far (Cx::rounds), and the record in flight of its
    \* settle.
    txn,
    \* History: every fire and every lift so far kept LifoOrder.
    lifoOk,
    \* Ghost: the ords of the records that were stored.
    born,
    \* Ghost: the ords of the records that a fire spent.
    spent,
    \* Ghost: the fires of the round in progress, or of the last round while
    \* no round is in progress. Only Begin and SettleJob reset it.
    txnFires

vars == <<records, waiters, pending, jobs, counter, txn, lifoOk, born, spent, txnFires>>

Vals == {"a", "b"}
Pats == {"a", "b", "any"}
Ords == 0..MaxOps
\* The ord of an element that carries none (reissued_ord = None).
NoOrd == -1
Carried == {NoOrd} \cup Ords
Phases == {"idle", "before", "plan", "loop", "after", "job"}

Matches(p, v) == p = "any" \/ p = v

\* No record in flight.
NoRec == [ord |-> NoOrd, val |-> "a", pers |-> FALSE]
\* The element of a settle job, and of no round.
NoElt == [side |-> "settle", ord |-> NoOrd, first |-> NoOrd, term |-> "a",
          pers |-> FALSE, peek |-> FALSE]
\* No round in progress.
Idle == [phase |-> "idle", elt |-> NoElt, ord |-> NoOrd, fired |-> 0, flight |-> NoRec]

\* The top of a side: the entry with the greatest ord.
IsTopR(r, S) == r \in S /\ \A s \in S : s.ord <= r.ord
IsTopW(w, S) == w \in S /\ \A x \in S : x.ord <= w.ord
\* The tops, as sets (one entry at most, since the ords are unique).
TopRecords(S) == {r \in S : IsTopR(r, S)}
TopWaiters(S) == {w \in S : IsTopW(w, S)}

\* FireRounds::after_fire with the count of fires after this one.
AfterFire(n, isSpent, stays) ==
    IF isSpent THEN "done"
    ELSE IF stays THEN "divergent"
    ELSE IF Cap <= n THEN "cap"
    ELSE "continue"

\* operation_ord: the carried ord of a re-entering element, or the counter
\* value for a fresh element.
OperationOrd(x) == IF x.ord # NoOrd THEN x.ord ELSE counter

\* The re-issue of the element x after its round with the ord o: a
\* persistent element carries its first-round ord
\* (op.persistent.then_some(ord)), and another element carries none.
Reissued(x, o) ==
    [x EXCEPT !.ord = IF x.pers THEN (IF x.ord # NoOrd THEN x.ord ELSE o) ELSE NoOrd]

\* Cx::at_head for the element x of the round with the ord o. A fresh
\* element is always at the top (reaches_head). A re-entering element is at
\* the top when its side is empty or its ord is greater than every ord of
\* its side (precedes). Unsafe ReentryAtTop puts it at the top whatever the
\* ords.
AtTop(x, o) ==
    LET own == IF x.side = "produce" THEN {r.ord : r \in records} ELSE {w.ord : w \in waiters}
    IN x.ord = NoOrd \/ ReentryAtTop \/ \A q \in own : q < o

\* The record and the waiter of the element x with the ord o.
AsRecord(x, o) == [ord |-> o, val |-> x.term, pers |-> x.pers]
AsWaiter(x, o) == [ord |-> o, pat |-> x.term, pers |-> x.pers, peek |-> x.peek]

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
\* count n. One count runs through the phases of a round (revision I-21,
\* Cx::rounds). Unsafe PerPhaseCount starts the count of each phase at 0.
PhaseCount(n) == IF PerPhaseCount THEN 0 ELSE n

\* A settle that cannot complete re-issues: the operation itself after the
\* settle before the plan, and a settle job after the settle after the plan
\* or after the settle of a settle job.
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
    /\ lifoOk = TRUE
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
               ord |-> OperationOrd(x), fired |-> 0, flight |-> NoRec]
    /\ txnFires' = 0

\* A fresh send with the value v (persistent: !!).
Produce(v, pers) ==
    /\ pers => Reissues
    /\ Begin([side |-> "produce", ord |-> NoOrd, first |-> NoOrd, term |-> v,
              pers |-> pers, peek |-> FALSE])
    /\ UNCHANGED <<records, waiters, pending, jobs, lifoOk, born, spent>>

\* A fresh receive with the pattern p (persistent: <=). A persistent receive
\* never peeks.
Consume(p, pers, peek) ==
    /\ (pers \/ peek) => Reissues
    /\ ~(pers /\ peek)
    /\ Begin([side |-> "consume", ord |-> NoOrd, first |-> NoOrd, term |-> p,
              pers |-> pers, peek |-> peek])
    /\ UNCHANGED <<records, waiters, pending, jobs, lifoOk, born, spent>>

\* The next round of the re-issued operation e (a new space intent).
Reenter(e) ==
    /\ Begin(e)
    /\ pending' = pending \ {e}
    /\ UNCHANGED <<records, waiters, jobs, lifoOk, born, spent>>

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
    /\ UNCHANGED <<records, waiters, pending, lifoOk, born, spent>>

\* One step of the settle of the round: a fire, or the end of the settle.
\* The candidate record is the record in flight, or else the top record. A
\* stored record that is not persistent is lifted before its re-offer. A
\* record that the fire did not spend stays in flight when it is out of the
\* store. The cap check counts the fires of the whole round (I-21). The
\* settle before the plan hands its count to the plan. Unsafe NoSettle ends
\* every settle at once.
SettleStep ==
    /\ txn.phase \in {"before", "after", "job"}
    /\ LET flight == txn.flight
           inFlight == flight.ord # NoOrd
           candidates == IF inFlight THEN {flight} ELSE TopRecords(records)
           pairs == {p \in TopWaiters(waiters) \X candidates : Matches(p[1].pat, p[2].val)}
       IN
       IF NoSettle \/ pairs = {}
       THEN \* Quiescent: land the record in flight and end the settle.
            /\ records' = Land(records, flight)
            /\ txn' = IF txn.phase = "before"
                      THEN [txn EXCEPT !.phase = "plan", !.fired = PhaseCount(@), !.flight = NoRec]
                      ELSE Idle
            /\ UNCHANGED <<waiters, pending, jobs, counter, lifoOk, born, spent, txnFires>>
       ELSE IF Cap <= txn.fired
       THEN \* At the cap: land the record in flight and re-issue.
            /\ records' = Land(records, flight)
            /\ SettleReissue
            /\ txn' = Idle
            /\ UNCHANGED <<waiters, counter, lifoOk, born, spent, txnFires>>
       ELSE \E p \in pairs :
            LET w == p[1]
                r == p[2]
                lifted == ~inFlight /\ ~r.pers
                stored == IF lifted THEN records \ {r} ELSE records
                isSpent == ~r.pers /\ ~w.peek
                flight1 == IF (inFlight \/ lifted) /\ ~isSpent THEN r ELSE NoRec
                step == AfterFire(txn.fired + 1, isSpent, w.pers)
            IN
            /\ lifoOk' = (lifoOk /\ (lifted => IsTopR(r, records)) /\ IsTopW(w, waiters)
                          /\ \A h \in stored : h.ord <= r.ord)
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

\* The start of the plan (open_plan, then Cx::at_head). The settle before
\* the plan can leave the round at the cap. Then the plan stages nothing,
\* and the operation re-issues with the order rule of decision 43 (I-21).
\* Else the element is at the top, or it is stored or parked at its LIFO
\* place with no fire. The plan continues the count of the settle before
\* it. Unsafe NoPlanOpenCheck opens the plan also at the cap.
PlanStart ==
    /\ txn.phase = "plan"
    /\ IF ~NoPlanOpenCheck /\ Cap <= txn.fired
       THEN /\ pending' = pending \cup {Reissued(txn.elt, txn.ord)}
            /\ txn' = Idle
            /\ UNCHANGED <<records, waiters, born>>
       ELSE IF AtTop(txn.elt, txn.ord)
       THEN /\ txn' = [txn EXCEPT !.phase = "loop", !.fired = PhaseCount(@)]
            /\ UNCHANGED <<records, waiters, born, pending>>
       ELSE /\ StoreOrPark(txn.elt, txn.ord)
            /\ txn' = [txn EXCEPT !.phase = "after", !.fired = PhaseCount(@)]
            /\ UNCHANGED pending
    /\ UNCHANGED <<jobs, counter, lifoOk, spent, txnFires>>

\* After a fire of the plan: the plan completes (the element is spent), it
\* goes on, or it re-issues the operation with no settle after the plan.
\* The settle after the plan continues the count of the plan (I-21).
PlanNext(step) ==
    CASE step = "done" -> /\ txn' = [txn EXCEPT !.phase = "after", !.fired = PhaseCount(@ + 1)]
                          /\ UNCHANGED pending
      [] step = "continue" -> /\ txn' = [txn EXCEPT !.fired = @ + 1]
                              /\ UNCHANGED pending
      [] OTHER -> /\ pending' = pending \cup {Reissued(txn.elt, txn.ord)}
                  /\ txn' = Idle

\* One step of the fire loop of offer_head for the datum of the round. The
\* fire spends the datum when the datum is not persistent and the waiter
\* does not peek, and the waiter stays when it is persistent.
OfferStep ==
    LET x == txn.elt
        o == txn.ord
        ws == {w \in TopWaiters(waiters) : Matches(w.pat, x.term)}
    IN
    IF ws = {}
    THEN /\ StoreOrPark(x, o)
         /\ txn' = [txn EXCEPT !.phase = "after", !.fired = PhaseCount(@)]
         /\ UNCHANGED <<pending, lifoOk, spent, txnFires>>
    ELSE \E w \in ws :
         LET step == AfterFire(txn.fired + 1, ~x.pers /\ ~w.peek, w.pers)
         IN /\ lifoOk' = (lifoOk /\ IsTopW(w, waiters) /\ \A h \in records : h.ord <= o)
            /\ waiters' = IF w.pers THEN waiters ELSE waiters \ {w}
            /\ txnFires' = txnFires + 1
            /\ PlanNext(step)
            /\ UNCHANGED <<records, born, spent>>

\* One step of the fire loop of receive_head for the continuation of the
\* round. The fire spends the continuation when it is not persistent, and
\* the record stays when it is persistent or the continuation peeks. A fire
\* removes its record unless the record is persistent or the continuation
\* peeks: a peek removes the record and stores it again at once (Rule E).
ReceiveStep ==
    LET k == txn.elt
        o == txn.ord
        hs == {h \in TopRecords(records) : Matches(k.term, h.val)}
    IN
    IF hs = {}
    THEN /\ StoreOrPark(k, o)
         /\ txn' = [txn EXCEPT !.phase = "after", !.fired = PhaseCount(@)]
         /\ UNCHANGED <<pending, lifoOk, spent, txnFires>>
    ELSE \E h \in hs :
         LET removes == ~h.pers /\ ~k.peek
             step == AfterFire(txn.fired + 1, ~k.pers, h.pers \/ k.peek)
         IN /\ lifoOk' = (lifoOk /\ IsTopR(h, records) /\ \A u \in waiters : u.ord <= o)
            /\ records' = IF removes THEN records \ {h} ELSE records
            /\ spent' = IF removes THEN spent \cup {h.ord} ELSE spent
            /\ txnFires' = txnFires + 1
            /\ PlanNext(step)
            /\ UNCHANGED <<waiters, born>>

PlanLoop ==
    /\ txn.phase = "loop"
    /\ IF txn.elt.side = "produce" THEN OfferStep ELSE ReceiveStep
    /\ UNCHANGED <<jobs, counter>>

Next ==
    \/ \E v \in Vals, pers \in BOOLEAN : Produce(v, pers)
    \/ \E p \in Pats, pers \in BOOLEAN, peek \in BOOLEAN : Consume(p, pers, peek)
    \/ \E e \in pending : Reenter(e)
    \/ SettleJob
    \/ SettleStep
    \/ PlanStart
    \/ PlanLoop

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Invariants                                                              *)

RecType == [ord : Ords, val : Vals, pers : BOOLEAN]
WtrType == [ord : Ords, pat : Pats, pers : BOOLEAN, peek : BOOLEAN]
EltType == [side : {"produce", "consume"}, ord : Carried, first : Ords,
            term : Vals \cup Pats, pers : BOOLEAN, peek : BOOLEAN]

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
    /\ lifoOk \in BOOLEAN
    /\ born \subseteq Ords
    /\ spent \subseteq Ords
    /\ txnFires \in Nat

\* The top record and the top waiter match.
PendingMatch ==
    \E r \in TopRecords(records), w \in TopWaiters(waiters) : Matches(w.pat, r.val)

LifoOrder == lifoOk

StackNoPendingMatch == (txn.phase = "idle" /\ pending = {} /\ jobs = 0) => ~PendingMatch

NoRecordLost ==
    \A o \in born : (\E r \in records : r.ord = o) \/ txn.flight.ord = o \/ o \in spent

ReissueKeepsOrd == \A f \in pending : IF f.pers THEN f.ord = f.first ELSE f.ord = NoOrd

\* No transaction makes more than Cap fires, its settles included (I-21).
TxnFiresBounded == txnFires <= Cap

=============================================================================
