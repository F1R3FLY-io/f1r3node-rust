------------------------ MODULE HeadOnlyDiscipline ------------------------
(***************************************************************************)
(* Head-only matching of one queue collection (P4, plan decision 11).      *)
(*                                                                         *)
(* A queue holds records (data) and waiters (waiting continuations). Each  *)
(* operation is one space transaction. It bumps the collection counter     *)
(* and takes the counter value as its ord (decision 31). The head of a     *)
(* side is its entry with the least ord, and only the two heads interact:  *)
(*   produce: a record is queued -> the datum joins the tail. Else the     *)
(*            head waiter fires when it matches, or the datum is stored.   *)
(*   consume: a waiter is queued -> the continuation joins the tail. Else  *)
(*            it fires with the head record when it matches, or it parks.  *)
(* The model explores every schedule of MaxOps produces and consumes with  *)
(* every value and pattern.                                                *)
(*                                                                         *)
(* Invariants:                                                             *)
(*   HeadOnlyNoPendingMatch  the head record and the head waiter never     *)
(*                           match (INV of P4 design B.4)                  *)
(*   HeadBlocks              every fire involves the head of its side: a   *)
(*                           head that does not match blocks the entries   *)
(*                           behind it                                     *)
(*   FifoOrder               records leave in arrival (ord) order          *)
(*                                                                         *)
(* Unsafe control (refutes exactly HeadBlocks):                            *)
(*   ScanWaiters = TRUE      the legacy scan: a datum fires the first      *)
(*                           waiter that matches, the head or not (the     *)
(*                           legacy GenericRSpace find_fireable_           *)
(*                           continuation scans every waiter). The scan    *)
(*                           runs only when no record is queued, so the    *)
(*                           other two invariants still hold.              *)
(*                                                                         *)
(* Inductive check (Apalache): HeadIndInv strengthens the three            *)
(* invariants with the facts that make them inductive: the ords are        *)
(* below the counter and unique, and every queued record follows every     *)
(* delivered one. BASE checks Init => HeadIndInv, STEP checks              *)
(* HeadIndInv /\ Next => HeadIndInv'. With ScanWaiters = TRUE, STEP must   *)
(* refute HeadIndInv in its HeadBlocks conjunct (negative control).        *)
(*                                                                         *)
(* Code (the P4 commit, under rholang/src/rust/interpreter):               *)
(*   head, precedes       spaces/disciplines/queue.rs:32-48, 56-65         *)
(*   reaches_head         spaces/disciplines/queue.rs:51                   *)
(*   Produce (offer)      spaces/generic.rs:337-401                        *)
(*   Consume (receive)    spaces/generic.rs:405-472                        *)
(*   the counter bump     spaces/counter.rs:118-182                        *)
(*   prepare_produce,     spaces/generic.rs:537-595                        *)
(*   prepare_consume                                                       *)
(* The Rocq module HeadOnlyDisciplines.v proves the same properties for    *)
(* every matcher, with persistent entries, peeks and the fire rounds.      *)
(*                                                                         *)
(* Abstractions:                                                           *)
(*   - Values a and b. A pattern accepts a value (a, b) or every value     *)
(*     (any). Every entry is non-persistent and no waiter peeks, so a      *)
(*     fire spends both partners and an operation fires at most once.      *)
(*   - The ord is the counter value. Records and waiters are sets: the     *)
(*     head is the entry with the least ord, as list order is not state.   *)
(*   - delivered and fires are fixed-size logs with a length (dlen),       *)
(*     because Apalache needs bounded structures and constant ranges.      *)
(***************************************************************************)
EXTENDS Integers, FiniteSets

(* Apalache type aliases. TLC reads these lines as comments.               *)
\* @typeAlias: rec = {ord: Int, val: Str};
\* @typeAlias: wtr = {ord: Int, pat: Str};
\* @typeAlias: fire = {side: Str, fired: Int, head: Int};
HeadOnlyDiscipline_aliases == TRUE

CONSTANTS
    \* The number of operations of one run.
    \* @type: Int;
    MaxOps,
    \* FALSE: head-only matching. TRUE: the Unsafe legacy waiter scan.
    \* @type: Bool;
    ScanWaiters

ASSUME MaxOps \in 1..6
ASSUME ScanWaiters \in BOOLEAN

VARIABLES
    \* The queued records.
    \* @type: Set($rec);
    records,
    \* The queued waiters.
    \* @type: Set($wtr);
    waiters,
    \* The collection counter: the ord of the next operation.
    \* @type: Int;
    counter,
    \* The ords of the delivered records, in delivery order (slots 1..dlen).
    \* @type: Int -> Int;
    dlog,
    \* The fires, in order (slots 1..dlen): one delivery for each fire.
    \* @type: Int -> $fire;
    flog,
    \* The number of fires so far.
    \* @type: Int;
    dlen

vars == <<records, waiters, counter, dlog, flog, dlen>>

Vals == {"a", "b"}
Pats == {"a", "b", "any"}
Sides == {"none", "produce", "consume"}
Slots == 1..MaxOps
Ords == 0..MaxOps
NoFire == [side |-> "none", fired |-> 0, head |-> 0]

\* @type: (Str, Str) => Bool;
Matches(p, v) == p = "any" \/ p = v

\* The head of a side: an entry with the least ord.
\* @type: ($rec, Set($rec)) => Bool;
IsHeadR(r, S) == r \in S /\ \A s \in S : r.ord <= s.ord
\* @type: ($wtr, Set($wtr)) => Bool;
IsHeadW(w, S) == w \in S /\ \A x \in S : w.ord <= x.ord

\* Append a fire and its delivered record to the logs.
\* @type: (Str, Int, Int, Int) => Bool;
Deliver(side, fired, head, ord) ==
    /\ dlog' = [dlog EXCEPT ![dlen + 1] = ord]
    /\ flog' = [flog EXCEPT ![dlen + 1] = [side |-> side, fired |-> fired, head |-> head]]
    /\ dlen' = dlen + 1

Init ==
    /\ records = {}
    /\ waiters = {}
    /\ counter = 0
    /\ dlog = [i \in Slots |-> 0]
    /\ flog = [i \in Slots |-> NoFire]
    /\ dlen = 0

\* GenericSpace::offer of a non-persistent datum with the value v.
\* @type: (Str) => Bool;
Produce(v) ==
    /\ counter < MaxOps
    /\ counter' = counter + 1
    /\ LET r == [ord |-> counter, val |-> v] IN
       IF records /= {}
       THEN \* a record is queued: the datum joins the tail
            /\ records' = records \cup {r}
            /\ UNCHANGED <<waiters, dlog, flog, dlen>>
       ELSE IF ScanWaiters
       THEN \* Unsafe: the legacy scan fires the first waiter that matches
            IF \E w \in waiters : Matches(w.pat, v)
            THEN \E w \in waiters, h \in waiters :
                    /\ Matches(w.pat, v)
                    /\ \A x \in waiters : Matches(x.pat, v) => w.ord <= x.ord
                    /\ IsHeadW(h, waiters)
                    /\ waiters' = waiters \ {w}
                    /\ Deliver("produce", w.ord, h.ord, r.ord)
                    /\ UNCHANGED records
            ELSE /\ records' = {r}
                 /\ UNCHANGED <<waiters, dlog, flog, dlen>>
       ELSE \* head-only: only the head waiter can fire
            IF \E h \in waiters : IsHeadW(h, waiters) /\ Matches(h.pat, v)
            THEN \E h \in waiters :
                    /\ IsHeadW(h, waiters)
                    /\ Matches(h.pat, v)
                    /\ waiters' = waiters \ {h}
                    /\ Deliver("produce", h.ord, h.ord, r.ord)
                    /\ UNCHANGED records
            ELSE /\ records' = {r}
                 /\ UNCHANGED <<waiters, dlog, flog, dlen>>

\* GenericSpace::receive of a non-persistent continuation with pattern p.
\* @type: (Str) => Bool;
Consume(p) ==
    /\ counter < MaxOps
    /\ counter' = counter + 1
    /\ LET w == [ord |-> counter, pat |-> p] IN
       IF waiters /= {}
       THEN \* a waiter is queued: the continuation joins the tail
            /\ waiters' = waiters \cup {w}
            /\ UNCHANGED <<records, dlog, flog, dlen>>
       ELSE IF \E h \in records : IsHeadR(h, records) /\ Matches(p, h.val)
       THEN \E h \in records :
               /\ IsHeadR(h, records)
               /\ Matches(p, h.val)
               /\ records' = records \ {h}
               /\ Deliver("consume", h.ord, h.ord, h.ord)
               /\ UNCHANGED waiters
       ELSE /\ waiters' = {w}
            /\ UNCHANGED <<records, dlog, flog, dlen>>

Next == (\E v \in Vals : Produce(v)) \/ (\E p \in Pats : Consume(p))

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Invariants                                                              *)

TypeOK ==
    /\ records \subseteq [ord : Ords, val : Vals]
    /\ waiters \subseteq [ord : Ords, pat : Pats]
    /\ counter \in 0..MaxOps
    /\ dlog \in [Slots -> Ords]
    /\ flog \in [Slots -> [side : Sides, fired : Ords, head : Ords]]
    /\ dlen \in 0..MaxOps

\* INV: no pending match between the two heads.
HeadOnlyNoPendingMatch ==
    \A r \in records, w \in waiters :
        (IsHeadR(r, records) /\ IsHeadW(w, waiters)) => ~Matches(w.pat, r.val)

\* Every fire involves the head of its side.
HeadBlocks == \A i \in Slots : i <= dlen => flog[i].fired = flog[i].head

\* Records leave in ord order.
FifoOrder == \A i, j \in Slots : (i <= dlen /\ j <= dlen /\ i < j) => dlog[i] < dlog[j]

\* The ords of the entries and of the delivered records are below the
\* counter, and the logs fill one slot for each operation at most.
OrdsBelowCounter ==
    /\ \A r \in records : r.ord < counter
    /\ \A w \in waiters : w.ord < counter
    /\ \A i \in Slots : i <= dlen => dlog[i] < counter
    /\ dlen <= counter

\* One ord for each queued record and for each queued waiter.
UniqueOrds ==
    /\ \A r, s \in records : r.ord = s.ord => r = s
    /\ \A w, x \in waiters : w.ord = x.ord => w = x

\* Every queued record follows every delivered one.
QueuedAfterDelivered == \A r \in records : \A i \in Slots : i <= dlen => dlog[i] < r.ord

\* The inductive strengthening for the Apalache BASE and STEP checks.
HeadIndInv ==
    /\ TypeOK
    /\ OrdsBelowCounter
    /\ UniqueOrds
    /\ QueuedAfterDelivered
    /\ HeadOnlyNoPendingMatch
    /\ HeadBlocks
    /\ FifoOrder

\* The STEP initializer: every state within the bounds that satisfies
\* HeadIndInv.
IndInit ==
    /\ records \in SUBSET [ord : Ords, val : Vals]
    /\ waiters \in SUBSET [ord : Ords, pat : Pats]
    /\ counter \in 0..MaxOps
    /\ dlog \in [Slots -> Ords]
    /\ flog \in [Slots -> [side : Sides, fired : Ords, head : Ords]]
    /\ dlen \in 0..MaxOps
    /\ HeadIndInv

=============================================================================
