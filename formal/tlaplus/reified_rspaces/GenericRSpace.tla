--------------------------- MODULE GenericRSpace ---------------------------
(***************************************************************************)
(* The engine of the native spaces with the scan driver of bag and set and *)
(* the head driver of the cell (P5a, the P5 design v3, A.7).               *)
(*                                                                         *)
(* One collection holds records (data) and waiters (waiting                *)
(* continuations). Each round of an operation is one space transaction.    *)
(* Every record and every waiter has an id: the identity of its source     *)
(* produce or consume. The canonical order of DR-75 (decision 41) orders   *)
(* candidates by that identity, so the model orders them by id, and a new  *)
(* element takes any unused id. So the model explores every canonical      *)
(* order of the entries.                                                   *)
(*                                                                         *)
(* The plans, with the current guard rule of decision 37:                  *)
(*   bag, set   OFFER-SCAN: the datum fires the first waiter, in canonical *)
(*              order, that matches it spatially and whose guard admits    *)
(*              it. A waiter that does not peek and fires on a set retires *)
(*              the stored non-persistent members of the class of the      *)
(*              datum (K_retire, decision 37). FireRounds decides Done,    *)
(*              Continue (the datum goes to the next waiter, decision 30)  *)
(*              and Reissue (a divergent pair or the round cap).           *)
(*              RECEIVE-SCAN: the first record, in canonical order, that   *)
(*              matches spatially; the guard is checked once, and a        *)
(*              refusal parks. A non-peek receive on a set fires over the  *)
(*              whole class of that record ([D; n], F3). A peek fire       *)
(*              removes a non-persistent record, and RESTORE-SCAN offers   *)
(*              the same record again, not persistent, with its own        *)
(*              authority (Rule E of decision 45).                         *)
(*   cell       the head driver of S1 with a capacity of one entry on each *)
(*              side (F15): only the head waiter and the head record       *)
(*              interact, a store or a park on a full side aborts with     *)
(*              CellOccupied and changes nothing, and a peek stores its    *)
(*              record again with a plain store.                           *)
(* A re-issued element is in flight until its next round, and other        *)
(* operations can run between the two rounds. Bag and set have no counter, *)
(* so a re-issued element keeps its record and its id (decision 43). A     *)
(* re-issued element of the cell keeps its id when it is persistent, and   *)
(* takes a fresh id otherwise.                                             *)
(*                                                                         *)
(* Invariants (TypeOK holds the types):                                    *)
(*   V1 NoPendingMatch     a stored waiter without a guard matches no      *)
(*                         stored record spatially (decision 37)           *)
(*   V2 PatternEnforcement every fire binds a value that matches its       *)
(*                         pattern and that its guard admits               *)
(*   V3 Conservation       every produced id is stored, consumed or in     *)
(*                         flight, exactly one of the three, and no        *)
(*                         persistent id is consumed                       *)
(*   V4 NoDoubleSpend      a non-persistent id is spent at most once       *)
(*   V5 AtomicReservation  the records that a fire removes were present    *)
(*                         before it and are absent after it               *)
(*   V6 SetClassRetired    (set) after a fire of class v that does not     *)
(*                         peek, no non-persistent record of value v       *)
(*                         remains                                         *)
(*   V7 CellCapacity       (cell) at most one record and one waiter        *)
(*   V8 PeekRestoresRecord every stored record and every datum in flight   *)
(*                         carries the authority of its own introduction,  *)
(*                         and every store of a record, a re-store         *)
(*                         included, happens when no waiter matches the    *)
(*                         record and admits it: a peek re-stores the      *)
(*                         record itself as the host's second send         *)
(*                                                                         *)
(* Unsafe controls (CONSTANT flags). Each one refutes exactly one of the   *)
(* invariants that its configuration checks, and the complement run        *)
(* without that invariant must find no violation:                          *)
(*   WildcardFire = TRUE           WildcardUnsafe refutes                  *)
(*                                 PatternEnforcement: the legacy          *)
(*                                 WildcardMatch, under which every        *)
(*                                 pattern matches every value (D6)        *)
(*   RetireClass = FALSE           SetRetireOneUnsafe refutes              *)
(*                                 SetClassRetired: a consume removes only *)
(*                                 the representative                      *)
(*   RetireOnProduce = FALSE       SetNoProduceRetireUnsafe refutes        *)
(*                                 SetClassRetired: no K_retire after a    *)
(*                                 produce-side fire (the R5 S3 trace)     *)
(*   CapacityCheck = FALSE         CellNoCapacityUnsafe refutes            *)
(*                                 CellCapacity. Without the capacity, the *)
(*                                 head driver also leaves a match behind  *)
(*                                 the head, so that configuration does    *)
(*                                 not check NoPendingMatch and            *)
(*                                 PeekRestoresRecord                      *)
(*   PeekRestore = FALSE           PeekNoRestoreUnsafe refutes             *)
(*                                 Conservation: a peek removes its record *)
(*                                 and stores nothing                      *)
(*   RestoreOwnAuthority = FALSE   RestoreOpAuthorityUnsafe refutes        *)
(*                                 PeekRestoresRecord: the re-store        *)
(*                                 registers the authority of the peek     *)
(*   RestoreReoffer = FALSE        RestoreNoReofferUnsafe refutes          *)
(*                                 PeekRestoresRecord: the re-store of a   *)
(*                                 bag is a plain store, so a parked       *)
(*                                 guarded waiter that admits the record   *)
(*                                 does not fire                           *)
(*                                                                         *)
(* Code (under rholang/src/rust/interpreter, the S1 commit 21b2b27fd):     *)
(*   FirstById            Cx::canonical_data_order and                     *)
(*                        Cx::canonical_waiter_order (spaces/generic.rs:   *)
(*                        602-617)                                         *)
(*   EngineMatches,       Cx::bind and Cx::admits (spaces/generic.rs:      *)
(*   Admits               618-634)                                         *)
(*   OfferScan,           the scan driver of A.7 in                        *)
(*   ReceiveScan,         spaces/disciplines/scan.rs: a receive counts its *)
(*   Restore,             fire before its restore re-offers (I-21)         *)
(*   RetireClass                                                           *)
(*   CellOffer,           offer_head and receive_head (spaces/generic.rs:  *)
(*   CellReceive          1128-1199, 1201-1290) with Cx::check_capacity    *)
(*                        (758-773) and the plain Cx::restore (864-880)    *)
(*   FireOffer, FireRecv  Cx::fire_offer_on and Cx::fire_receive_group     *)
(*                        (spaces/generic.rs:1103-1229)                    *)
(*   Retire               Cx::retire (spaces/generic.rs:1231-1243)         *)
(*   the round cap        reified/costs.rs:41-44, 302-334                  *)
(*                        (FIRE_ROUND_CAP, FireRounds::after_fire)         *)
(* The Rocq modules Match.v and GenericRSpace.v prove the same rules for   *)
(* every matcher, every order and every fire-round cap.                    *)
(*                                                                         *)
(* Abstractions:                                                           *)
(*   - Values a and b. A pattern accepts a value (a, b) or every value     *)
(*     (any). A guard admits every value (none) or only b (onlyB). A set   *)
(*     class is a value.                                                   *)
(*   - Records and waiters are sets, and an id is unique, so the model     *)
(*     has no list order. A peek receive is never persistent              *)
(*     (CONSUME_ARROWS).                                                   *)
(*   - Cap is FIRE_ROUND_CAP (2 in the configurations, so that both a      *)
(*     Continue and the cap are reachable). One round of an operation is   *)
(*     one step of the model: the recursive operators compute the staged   *)
(*     operations of the round and their effect, and the step commits      *)
(*     them. An aborted round (CellOccupied) is no step.                   *)
(*   - MaxOps bounds the rounds, because a divergent pair re-issues until  *)
(*     the phlo is spent.                                                  *)
(*   - The authority of a record is the id of the operation whose          *)
(*     authority its store registered. A fresh produce registers its own,  *)
(*     so a record carries its own authority when auth = id.               *)
(*   - History variables replace the fire logs: firesEnforced,             *)
(*     spentOnce, atomicFires, classesRetired and storedUnadmitted say     *)
(*     that every round so far kept V2, V4, V5, V6 and the store rule of   *)
(*     V8.                                                                 *)
(***************************************************************************)
EXTENDS Integers, FiniteSets

CONSTANTS
    \* "bag", "set" or "cell".
    Discipline,
    \* The rounds of one run: every operation and every re-entry.
    MaxOps,
    \* FIRE_ROUND_CAP of the model.
    Cap,
    \* TRUE: the Unsafe legacy WildcardMatch.
    WildcardFire,
    \* FALSE: the Unsafe consume that removes only the representative.
    RetireClass,
    \* FALSE: the Unsafe produce-side fire with no K_retire.
    RetireOnProduce,
    \* FALSE: the Unsafe cell without its capacity check.
    CapacityCheck,
    \* FALSE: the Unsafe peek that stores nothing again.
    PeekRestore,
    \* FALSE: the Unsafe re-store with the authority of the peek.
    RestoreOwnAuthority,
    \* FALSE: the Unsafe re-store of a bag or a set that is a plain store.
    RestoreReoffer

ASSUME Discipline \in {"bag", "set", "cell"}
ASSUME MaxOps \in 1..8
ASSUME Cap \in 1..4
ASSUME {WildcardFire, RetireClass, RetireOnProduce, CapacityCheck, PeekRestore,
        RestoreOwnAuthority, RestoreReoffer} \subseteq BOOLEAN

VARIABLES
    \* The stored records: [id, val, pers, auth].
    records,
    \* The parked waiters: [id, pat, guard, pers, peek].
    waiters,
    \* The re-issued elements in flight: [side, id, term, guard, pers, peek, auth].
    inflight,
    \* Ghost: the ids of every datum that a produce introduced.
    produced,
    \* Ghost: the ids of every datum that a persistent produce introduced.
    persistents,
    \* Ghost: the ids of the data that a fire spent or that a retirement
    \* removed.
    consumed,
    \* Ghost: the ids that an element has taken.
    used,
    \* The rounds so far.
    ops,
    \* History: every fire so far bound a matching value that its guard
    \* admitted.
    firesEnforced,
    \* History: no non-persistent id was spent twice.
    spentOnce,
    \* History: every fire removed present records, and none of them
    \* remained after the fire.
    atomicFires,
    \* History: after every fire of class v that did not peek, no
    \* non-persistent record of value v remained (set).
    classesRetired,
    \* History: every store of a record happened when no waiter matched the
    \* record and admitted it.
    storedUnadmitted

vars == <<records, waiters, inflight, produced, persistents, consumed, used, ops,
          firesEnforced, spentOnce, atomicFires, classesRetired, storedUnadmitted>>

Vals == {"a", "b"}
Pats == {"a", "b", "any"}
Guards == {"none", "onlyB"}
Ids == 1..MaxOps
NoAuth == 0

\* The real spatial match, and the match of the engine (the injected
\* matcher, or the Unsafe WildcardMatch).
Matches(p, v) == p = "any" \/ p = v
EngineMatches(p, v) == WildcardFire \/ Matches(p, v)
Admits(g, v) == g = "none" \/ v = "b"

\* The first entry of S in the canonical order (DR-75: by identity).
FirstById(S) == CHOOSE x \in S : \A y \in S : x.id <= y.id

\* ── The state of one round ─────────────────────────────────────────────

\* The view and the ghosts of a round at its start.
Begin ==
    [recs |-> records, wtrs |-> waiters, consumed |-> consumed, add |-> {},
     firesOk |-> TRUE, spentOk |-> TRUE, atomicOk |-> TRUE, storedOk |-> TRUE,
     nonPeek |-> {}, abort |-> FALSE]

\* A produce element and a consume element in flight.
DataElt(x) ==
    [side |-> "produce", id |-> x.id, term |-> x.val, guard |-> "none",
     pers |-> x.pers, peek |-> FALSE, auth |-> x.auth]
ContElt(k) ==
    [side |-> "consume", id |-> k.id, term |-> k.pat, guard |-> k.guard,
     pers |-> k.pers, peek |-> k.peek, auth |-> NoAuth]

\* Records that a set of ids spends: the spend rule of V4.
Spend(ids, st) ==
    [st EXCEPT !.consumed = @ \cup ids,
               !.spentOk = @ /\ ids \cap st.consumed = {}]

\* store_on: the record x is stored. A cell checks its capacity first
\* (check_capacity) unless the store is a re-store (plain). The store rule
\* of V8: no waiter matches the record and admits it.
StoreRec(x, st, checked) ==
    IF checked /\ Discipline = "cell" /\ CapacityCheck /\ st.recs # {}
    THEN [st EXCEPT !.abort = TRUE]
    ELSE [st EXCEPT
            !.recs = @ \cup {[id |-> x.id, val |-> x.val, pers |-> x.pers, auth |-> x.auth]},
            !.storedOk = @ /\ ~\E w \in st.wtrs : Matches(w.pat, x.val) /\ Admits(w.guard, x.val)]

Store(x, st) == StoreRec(x, st, TRUE)

\* park_on, with the capacity check of a cell.
Park(k, st) ==
    IF Discipline = "cell" /\ CapacityCheck /\ st.wtrs # {}
    THEN [st EXCEPT !.abort = TRUE]
    ELSE [st EXCEPT !.wtrs = @ \cup {[id |-> k.id, pat |-> k.pat, guard |-> k.guard,
                                       pers |-> k.pers, peek |-> k.peek]}]

\* fire_offer_on: the datum x fires the waiter w. The waiter leaves unless
\* it is persistent, and x is spent when it is not persistent and w does
\* not peek. The incoming datum is never stored.
FireOffer(x, w, st) ==
    LET spent == ~x.pers /\ ~w.peek
        st1 == [st EXCEPT
                  !.wtrs = IF w.pers THEN @ ELSE @ \ {w},
                  !.firesOk = @ /\ Matches(w.pat, x.val) /\ Admits(w.guard, x.val),
                  !.nonPeek = IF w.peek THEN @ ELSE @ \cup {x.val}]
    IN IF spent THEN Spend({x.id}, st1) ELSE st1

\* fire_receive_group: the continuation k fires with the candidates C,
\* whose first is r. The fire removes each non-persistent candidate. A fire
\* that does not peek spends them.
FireRecv(k, r, C, st) ==
    LET removed == {c \in C : ~c.pers}
        after == st.recs \ removed
        st1 == [st EXCEPT
                  !.recs = after,
                  !.firesOk = @ /\ Matches(k.pat, r.val) /\ Admits(k.guard, r.val),
                  !.atomicOk = @ /\ removed \subseteq st.recs
                                 /\ \A c \in removed : ~\E x \in after : x.id = c.id,
                  !.nonPeek = IF k.peek THEN @ ELSE @ \cup {r.val}]
    IN IF k.peek THEN st1 ELSE Spend({c.id : c \in removed}, st1)

\* RETIRE-CLASS: a K_retire of each stored non-persistent member of the
\* class v.
RetireClassOf(v, st) ==
    LET members == {m \in st.recs : m.val = v /\ ~m.pers}
    IN Spend({m.id : m \in members}, [st EXCEPT !.recs = @ \ members])

\* A re-issue: the element is in flight.
Reissue(x, st) == [st EXCEPT !.add = @ \cup {DataElt(x)}]
ReissueK(k, st) == [st EXCEPT !.add = @ \cup {ContElt(k)}]

\* ── The scan driver of bag and set ──────────────────────────────────────

\* OFFER-SCAN of the datum x after fired fires of the round.
RECURSIVE OfferScan(_, _, _)
OfferScan(x, st, fired) ==
    LET cands == {w \in st.wtrs : EngineMatches(w.pat, x.val) /\ Admits(w.guard, x.val)}
    IN IF cands = {}
       THEN Store(x, st)
       ELSE LET w == FirstById(cands)
                st1 == FireOffer(x, w, st)
                st2 == IF ~w.peek /\ Discipline = "set" /\ RetireOnProduce
                       THEN RetireClassOf(x.val, st1) ELSE st1
            IN IF ~x.pers /\ ~w.peek THEN st2
               ELSE IF w.pers \/ fired + 1 >= Cap THEN Reissue(x, st2)
               ELSE OfferScan(x, st2, fired + 1)

\* The head driver of the cell: the datum x meets the head waiter.
RECURSIVE CellOffer(_, _, _)
CellOffer(x, st, fired) ==
    IF st.wtrs = {} THEN Store(x, st)
    ELSE LET w == FirstById(st.wtrs)
         IN IF ~(EngineMatches(w.pat, x.val) /\ Admits(w.guard, x.val))
            THEN Store(x, st)
            ELSE LET st1 == FireOffer(x, w, st)
                 IN IF ~x.pers /\ ~w.peek THEN st1
                    ELSE IF w.pers \/ fired + 1 >= Cap THEN Reissue(x, st1)
                    ELSE CellOffer(x, st1, fired + 1)

Offer(x, st, fired) ==
    IF Discipline = "cell" THEN CellOffer(x, st, fired) ELSE OfferScan(x, st, fired)

\* The re-store of the record r that the peek k removed (Rule E): the same
\* record, not persistent, with its own authority. The scan driver offers it
\* again (RESTORE-SCAN) with the fire rounds of the operation, and the cell
\* stores it with a plain store.
Restore(k, r, st, fired) ==
    LET x == [id |-> r.id, val |-> r.val, pers |-> FALSE,
              auth |-> IF RestoreOwnAuthority THEN r.auth ELSE k.id]
    IN IF Discipline = "cell" \/ ~RestoreReoffer
       THEN StoreRec(x, st, FALSE)
       ELSE IF fired >= Cap THEN Reissue(x, st)
       ELSE OfferScan(x, st, fired)

\* RECEIVE-SCAN of the continuation k after fired fires of the round.
RECURSIVE ReceiveScan(_, _, _)
ReceiveScan(k, st, fired) ==
    LET spat == {r \in st.recs : EngineMatches(k.pat, r.val)}
    IN IF spat = {} THEN Park(k, st)
       ELSE LET r == FirstById(spat)
            IN IF ~Admits(k.guard, r.val) THEN Park(k, st)
               ELSE LET C == IF k.peek \/ Discipline # "set" \/ ~RetireClass THEN {r}
                             ELSE {m \in st.recs : m.val = r.val}
                        st1 == FireRecv(k, r, C, st)
                        st2 == IF k.peek /\ ~r.pers /\ PeekRestore
                               THEN Restore(k, r, st1, fired + 1) ELSE st1
                    IN IF ~k.pers THEN st2
                       ELSE IF k.peek \/ (\E c \in C : c.pers) \/ fired + 1 >= Cap
                            THEN ReissueK(k, st2)
                            ELSE ReceiveScan(k, st2, fired + 1)

\* The head driver of the cell: the continuation k meets the head record.
RECURSIVE CellReceive(_, _, _)
CellReceive(k, st, fired) ==
    IF st.recs = {} THEN Park(k, st)
    ELSE LET r == FirstById(st.recs)
         IN IF ~(EngineMatches(k.pat, r.val) /\ Admits(k.guard, r.val))
            THEN Park(k, st)
            ELSE LET st1 == FireRecv(k, r, {r}, st)
                     st2 == IF k.peek /\ ~r.pers /\ PeekRestore
                            THEN Restore(k, r, st1, fired + 1) ELSE st1
                 IN IF ~k.pers THEN st2
                    ELSE IF k.peek \/ r.pers \/ fired + 1 >= Cap THEN ReissueK(k, st2)
                    ELSE CellReceive(k, st2, fired + 1)

Receive(k, st, fired) ==
    IF Discipline = "cell" THEN CellReceive(k, st, fired) ELSE ReceiveScan(k, st, fired)

\* ── Steps ───────────────────────────────────────────────────────────────

\* The ids that no element has taken.
FreeIds == Ids \ used

\* Commits the round res, which is not aborted. rest are the elements in
\* flight that stay.
Commit(res, rest) ==
    /\ ~res.abort
    /\ records' = res.recs
    /\ waiters' = res.wtrs
    /\ consumed' = res.consumed
    /\ inflight' = rest \cup res.add
    /\ firesEnforced' = (firesEnforced /\ res.firesOk)
    /\ spentOnce' = (spentOnce /\ res.spentOk)
    /\ atomicFires' = (atomicFires /\ res.atomicOk)
    /\ storedUnadmitted' = (storedUnadmitted /\ res.storedOk)
    /\ classesRetired' =
         (classesRetired /\
          (Discipline = "set" =>
             \A v \in res.nonPeek : ~\E r \in res.recs : r.val = v /\ ~r.pers))
    /\ ops' = ops + 1

Init ==
    /\ records = {}
    /\ waiters = {}
    /\ inflight = {}
    /\ produced = {}
    /\ persistents = {}
    /\ consumed = {}
    /\ used = {}
    /\ ops = 0
    /\ firesEnforced = TRUE
    /\ spentOnce = TRUE
    /\ atomicFires = TRUE
    /\ classesRetired = TRUE
    /\ storedUnadmitted = TRUE

\* A send of the value v (persistent: !!) with the fresh id id.
Produce(v, pers, id) ==
    LET x == [id |-> id, val |-> v, pers |-> pers, auth |-> id]
    IN /\ ops < MaxOps
       /\ Commit(Offer(x, Begin, 0), inflight)
       /\ produced' = produced \cup {id}
       /\ persistents' = IF pers THEN persistents \cup {id} ELSE persistents
       /\ used' = used \cup {id}

\* A receive with the pattern p and the guard g (persistent: <=, peek: <<-)
\* with the fresh id id.
Consume(p, g, pers, peek, id) ==
    LET k == [id |-> id, pat |-> p, guard |-> g, pers |-> pers, peek |-> peek]
    IN /\ ops < MaxOps
       /\ ~(pers /\ peek)
       /\ Commit(Receive(k, Begin, 0), inflight)
       /\ used' = used \cup {id}
       /\ UNCHANGED <<produced, persistents>>

\* The next round of the element e in flight (a new space intent). A datum
\* of the cell that is not persistent takes a fresh order, so a fresh id.
ReenterData(e) ==
    /\ ops < MaxOps
    /\ e \in inflight
    /\ e.side = "produce"
    /\ IF Discipline = "cell" /\ ~e.pers
       THEN \E nid \in FreeIds :
              LET x == [id |-> nid, val |-> e.term, pers |-> FALSE, auth |-> nid]
              IN /\ Commit(Offer(x, Begin, 0), inflight \ {e})
                 /\ produced' = (produced \ {e.id}) \cup {nid}
                 /\ used' = used \cup {nid}
                 /\ UNCHANGED persistents
       ELSE LET x == [id |-> e.id, val |-> e.term, pers |-> e.pers, auth |-> e.auth]
            IN /\ Commit(Offer(x, Begin, 0), inflight \ {e})
               /\ UNCHANGED <<produced, persistents, used>>

ReenterCont(e) ==
    /\ ops < MaxOps
    /\ e \in inflight
    /\ e.side = "consume"
    /\ LET k == [id |-> e.id, pat |-> e.term, guard |-> e.guard, pers |-> e.pers, peek |-> e.peek]
       IN Commit(Receive(k, Begin, 0), inflight \ {e})
    /\ UNCHANGED <<produced, persistents, used>>

Next ==
    \/ \E v \in Vals, pers \in BOOLEAN, id \in FreeIds : Produce(v, pers, id)
    \/ \E p \in Pats, g \in Guards, pers \in BOOLEAN, peek \in BOOLEAN, id \in FreeIds :
          Consume(p, g, pers, peek, id)
    \/ \E e \in inflight : ReenterData(e) \/ ReenterCont(e)

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Invariants                                                              *)

RecType == [id : Ids, val : Vals, pers : BOOLEAN, auth : Ids]
WtrType == [id : Ids, pat : Pats, guard : Guards, pers : BOOLEAN, peek : BOOLEAN]
EltType == [side : {"produce", "consume"}, id : Ids, term : Vals \cup Pats, guard : Guards,
            pers : BOOLEAN, peek : BOOLEAN, auth : Ids \cup {NoAuth}]

TypeOK ==
    /\ records \subseteq RecType
    /\ waiters \subseteq WtrType
    /\ inflight \subseteq EltType
    /\ produced \subseteq Ids
    /\ persistents \subseteq produced
    /\ consumed \subseteq Ids
    /\ used \subseteq Ids
    /\ ops \in 0..MaxOps
    /\ {firesEnforced, spentOnce, atomicFires, classesRetired, storedUnadmitted} \subseteq BOOLEAN

\* V1: a waiter without a guard has no pending spatial match.
NoPendingMatch ==
    \A w \in waiters, r \in records : Matches(w.pat, r.val) => w.guard # "none"

\* V2.
PatternEnforcement == firesEnforced

\* V3.
Stored(id) == \E r \in records : r.id = id
InFlightData(id) == \E e \in inflight : e.side = "produce" /\ e.id = id
Conservation ==
    /\ \A id \in produced :
          /\ Stored(id) \/ id \in consumed \/ InFlightData(id)
          /\ ~(Stored(id) /\ id \in consumed)
          /\ ~(InFlightData(id) /\ id \in consumed)
          /\ ~(Stored(id) /\ InFlightData(id))
    /\ consumed \cap persistents = {}

\* V4.
NoDoubleSpend == spentOnce

\* V5.
AtomicReservation == atomicFires

\* V6.
SetClassRetired == classesRetired

\* V7.
CellCapacity ==
    Discipline = "cell" => Cardinality(records) <= 1 /\ Cardinality(waiters) <= 1

\* V8.
PeekRestoresRecord ==
    /\ \A r \in records : r.auth = r.id
    /\ \A e \in inflight : e.side = "produce" => e.auth = e.id
    /\ storedUnadmitted

=============================================================================
