------------------------ MODULE HeadOnlyDiscipline ------------------------
(***************************************************************************)
(* Head-only matching of one queue collection (P4, plan decision 11) and   *)
(* the re-entry of re-issued operations (decision 43).                     *)
(*                                                                         *)
(* A queue holds records (data) and waiters (waiting continuations). Each  *)
(* round of an operation is one space transaction, and every round bumps   *)
(* the collection counter (decision 31). The head of a side is its entry   *)
(* with the least ord, and only the two heads interact:                    *)
(*   produce: the datum fires the head waiter when it matches, or the      *)
(*            datum is stored.                                             *)
(*   consume: the continuation fires with the head record when it          *)
(*            matches, or it parks.                                        *)
(* The ord and the place of a round (QD-2, QD-9):                          *)
(*   - A fresh operation takes the counter value as its ord. It reaches    *)
(*     the head only of an empty side, and else it joins the tail.         *)
(*   - A re-issued persistent operation (!! or <=) keeps the ord of its    *)
(*     first round. It re-enters at the head when its side is empty or     *)
(*     its ord precedes the ord of the head. Else it is stored or parked   *)
(*     at its place and fires nothing.                                     *)
(*   - A re-issued datum that is not persistent takes a fresh ord.         *)
(* A round re-issues its element after a divergent pair (the element and   *)
(* its partner both stay) or at the fire-round cap. The element is then in *)
(* flight until its next round, and other operations can run between the   *)
(* two rounds. The model explores every schedule of MaxOps rounds with     *)
(* every value, pattern, persistence and peek.                             *)
(*                                                                         *)
(* Invariants (TypeOK holds the types):                                    *)
(*   ShadowedNoPendingMatch  while an element is in flight, a matching     *)
(*                           pair of heads has an element in flight that   *)
(*                           precedes the head of its own side (SHADOW)    *)
(*   QuiescentNoPendingMatch with no element in flight, the head record    *)
(*                           and the head waiter do not match (INV, QD-6). *)
(*                           Every completed deploy ends in such a state.  *)
(*   ReissueKeepsOrd         a re-issued persistent element in flight      *)
(*                           carries the ord of its first round            *)
(*   IntroducedOnce          each persistent element takes a fresh ord     *)
(*                           exactly once (a ghost count of the            *)
(*                           introductions), so every round of it stores   *)
(*                           or fires the same record                      *)
(*   FifoOrder               an element fires only when no entry of its    *)
(*                           own side precedes it: each side fires in ord  *)
(*                           order (QD-7, QD-9)                            *)
(*   LeaveOrder              the records that are not persistent leave in  *)
(*                           strictly increasing ord, and each stored one  *)
(*                           is younger than each one that left            *)
(*   HeadBlocks              every fire involves the head of the other     *)
(*                           side: a head that does not match blocks the   *)
(*                           entries behind it                             *)
(* The invariants state decision 43 in independent parts, so that each     *)
(* Unsafe control below breaks exactly one of them:                        *)
(*   - With no element in flight, SHADOW is INV. ShadowedNoPendingMatch    *)
(*     states SHADOW for the states with an element in flight, and         *)
(*     QuiescentNoPendingMatch for the other states. Together they state   *)
(*     SHADOW in every state.                                              *)
(*   - ReissueKeepsOrd states the ord that a re-issue carries              *)
(*     (reissued_ord), and IntroducedOnce states that every round takes    *)
(*     it. Together they state that a re-issued persistent element keeps   *)
(*     its first-round ord in every round.                                 *)
(*   - FifoOrder states the fire order of each side for every element.     *)
(*     LeaveOrder follows from it and the fresh ords of the elements that  *)
(*     are not persistent (T3 of decision 43).                             *)
(*                                                                         *)
(* Unsafe controls (CONSTANT flags). Each one refutes exactly one          *)
(* invariant, and the complement run without that invariant must find no   *)
(* violation:                                                              *)
(*   ScanWaiters = TRUE      HeadOnlyScanUnsafe refutes HeadBlocks. The    *)
(*                           legacy scan: a datum fires the first waiter   *)
(*                           that matches, the head or not (the legacy     *)
(*                           GenericRSpace find_fireable_continuation      *)
(*                           scans every waiter). The scan runs only when  *)
(*                           no record is queued, so the other invariants  *)
(*                           still hold.                                   *)
(*   ReissueFreshOrd = TRUE  ReissueFreshOrdUnsafe refutes IntroducedOnce. *)
(*                           operation_ord ignores the carried ord, so     *)
(*                           each round takes the counter value and the    *)
(*                           place of a fresh element, as in P4 (C0 of     *)
(*                           decision 43). The operation still carries its *)
(*                           first-round ord, so ReissueKeepsOrd holds.    *)
(*   ReentryAtHead = TRUE    ReentryAtHeadUnsafe refutes FifoOrder. A      *)
(*                           re-entering element that carries an ord fires *)
(*                           from the head of its side regardless of the   *)
(*                           ords, when the other side holds at most one   *)
(*                           entry. That entry leaves or stays, so no new  *)
(*                           pair of heads appears. Without this condition *)
(*                           the fire can expose a matching pair that no   *)
(*                           element in flight precedes, and               *)
(*                           ShadowedNoPendingMatch fails too (8 rounds).  *)
(*   NonPersistentFirstOrd = TRUE                                          *)
(*                           NonPersistentFirstOrdUnsafe refutes           *)
(*                           QuiescentNoPendingMatch (C1 of decision 43).  *)
(*                           A re-issued datum that is not persistent      *)
(*                           keeps its first-round ord. It re-enters ahead *)
(*                           of a younger record, fires the waiter in      *)
(*                           front and is spent. The younger record and    *)
(*                           the next waiter then match with no element in *)
(*                           flight. The kept ord also breaks LeaveOrder   *)
(*                           (5 rounds), so this control does not check    *)
(*                           LeaveOrder. At 10 rounds the mutation can     *)
(*                           also break ShadowedNoPendingMatch: a second   *)
(*                           element in flight does not precede the head   *)
(*                           of its side.                                  *)
(*                                                                         *)
(* Inductive check (Apalache): HeadIndInv holds every invariant above and  *)
(* the facts that make them inductive. The ords are below the counter and  *)
(* unique, and no ord at or above the counter has an introduction. Each    *)
(* element in flight has the shape of its rule: a persistent element       *)
(* carries its first-round ord, and a datum that is not persistent carries *)
(* none. No persistent waiter peeks, and the P4 fragment holds no          *)
(* persistent entry. BASE checks Init => HeadIndInv, and STEP checks       *)
(* HeadIndInv /\ Next => HeadIndInv'. One pair of checks covers the P4     *)
(* fragment (Reissues = FALSE) and one pair covers decision 43 (Reissues = *)
(* TRUE, MaxOps = 7). IndInit covers every state of the bounds that        *)
(* satisfies HeadIndInv, so BASE and STEP prove HeadIndInv on every        *)
(* reachable state of MaxOps rounds. With ScanWaiters = TRUE, the P4 STEP  *)
(* must refute HeadIndInv in its HeadBlocks conjunct (negative control).   *)
(*                                                                         *)
(* Code (under rholang/src/rust/interpreter):                              *)
(*   head, precedes,      spaces/disciplines/queue.rs:41-59, 65-74, 62     *)
(*   reaches_head         (QueueDiscipline::head_metered,                  *)
(*                        QueueDiscipline::precedes: ord < head,           *)
(*                        QueueDiscipline::reaches_head)                   *)
(*   OperationOrd         spaces/generic.rs:1715-1743 (operation_ord keeps *)
(*                        reissued_ord after the check 0 <= reissued_ord   *)
(*                        < bump.ord)                                      *)
(*   AtHead               spaces/generic.rs:918-939, 881-901               *)
(*                        (Cx::at_head: a fresh element needs an empty     *)
(*                        side, a re-entry compares with                   *)
(*                        Cx::head_ord through                             *)
(*                        Discipline::precedes)                            *)
(*   DataRound            spaces/generic.rs:1524-1591 (offer_head)         *)
(*   ContRound            spaces/generic.rs:1600-1684 (receive_head)       *)
(*   Reissued             spaces/generic.rs:1583, 1676 (the re-issue sets  *)
(*                        reissued_ord: op.persistent.then_some(ord)) and  *)
(*                        spaces/agent.rs:135, 178 (ProduceOp::reissued_ord *)
(*                        and ConsumeOp::reissued_ord)                     *)
(*   the fire-round cap   reified/costs.rs:44, 322-333 (FIRE_ROUND_CAP,    *)
(*                        FireRounds::after_fire)                          *)
(*   the counter bump     spaces/counter.rs:165-245                        *)
(*   Produce, Consume,    spaces/generic.rs:2288-2424 (run_with of         *)
(*   Reenter              FamilySpace: the bump, operation_ord, then the   *)
(*                        plan)                                            *)
(* The Rocq module HeadOnlyDisciplines.v proves the same properties for    *)
(* every matcher and every fire-round cap.                                 *)
(*                                                                         *)
(* Abstractions:                                                           *)
(*   - Values a and b. A pattern accepts a value (a, b) or every value     *)
(*     (any). A persistent receive never peeks (CONSUME_ARROWS admits no   *)
(*     persistent peek). Records and waiters are sets: the head is the     *)
(*     entry with the least ord, as list order is not state.               *)
(*   - Cap = 1: a round fires at most once, because Apalache does not      *)
(*     accept the recursive fire loop. A fire that neither spends the      *)
(*     element nor keeps its partner ends the round at the cap. A round of *)
(*     the code with k fires is k rounds of the model with no operation    *)
(*     between them. The counter then advances k times, which shifts the   *)
(*     later ords but keeps their order.                                   *)
(*   - MaxOps bounds the rounds (operations and re-entries), because a     *)
(*     divergent pair re-issues until the phlo is spent.                   *)
(*   - Reissues = FALSE gives the P4 fragment: no persistent entry and no  *)
(*     peek, so no round re-issues and no element is in flight.            *)
(*   - An element in flight records its side, the ord that it carries      *)
(*     (NoOrd for none) and the ghost first-round ord first. The re-issue  *)
(*     writes the carried ord, or the ord of the round when the element    *)
(*     carries none. With the correct operation_ord this is the            *)
(*     op.persistent.then_some(ord) of the code.                           *)
(*   - History variables replace the fire logs: partnersAtHead,            *)
(*     firersAtHead and leavesInOrder say that every fire so far kept      *)
(*     HeadBlocks, FifoOrder and LeaveOrder. lastLeft is the ord of the    *)
(*     last record that left, and intro counts the fresh ords of each      *)
(*     persistent element by its first-round ord.                          *)
(***************************************************************************)
EXTENDS Integers, FiniteSets

(* Apalache type aliases. TLC reads these lines as comments.               *)
\* @typeAlias: rec = {ord: Int, val: Str, pers: Bool};
\* @typeAlias: wtr = {ord: Int, pat: Str, pers: Bool, peek: Bool};
\* @typeAlias: elt = {side: Str, ord: Int, first: Int, term: Str, pers: Bool, peek: Bool};
HeadOnlyDiscipline_aliases == TRUE

CONSTANTS
    \* The number of rounds of one run: every operation and every re-entry
    \* bumps the counter once.
    \* @type: Int;
    MaxOps,
    \* FIRE_ROUND_CAP of the model (1, so that the cap is reachable).
    \* @type: Int;
    Cap,
    \* TRUE: persistent sends (!!), persistent receives (<=) and peek
    \* receives, so a round can re-issue its element. FALSE: the P4 fragment.
    \* @type: Bool;
    Reissues,
    \* TRUE: the Unsafe legacy waiter scan.
    \* @type: Bool;
    ScanWaiters,
    \* TRUE: the Unsafe operation_ord that ignores the carried ord.
    \* @type: Bool;
    ReissueFreshOrd,
    \* TRUE: the Unsafe re-entry at the head regardless of the ords.
    \* @type: Bool;
    ReentryAtHead,
    \* TRUE: the Unsafe re-issue that keeps the ord of a datum that is not
    \* persistent.
    \* @type: Bool;
    NonPersistentFirstOrd

ASSUME MaxOps \in 1..12
ASSUME Cap = 1
ASSUME {Reissues, ScanWaiters, ReissueFreshOrd, ReentryAtHead, NonPersistentFirstOrd} \subseteq BOOLEAN

VARIABLES
    \* The stored records.
    \* @type: Set($rec);
    records,
    \* The parked waiters.
    \* @type: Set($wtr);
    waiters,
    \* The re-issued elements in flight (F of decision 43).
    \* @type: Set($elt);
    inflight,
    \* The collection counter: the ord of the next fresh round.
    \* @type: Int;
    counter,
    \* Ghost: the number of fresh ords of each persistent element, by its
    \* first-round ord.
    \* @type: Int -> Int;
    intro,
    \* Ghost: the ord of the last record that left (NoOrd before the first).
    \* @type: Int;
    lastLeft,
    \* History: every fire so far involved the head of the other side.
    \* @type: Bool;
    partnersAtHead,
    \* History: every element so far fired with no entry of its own side
    \* ahead of it.
    \* @type: Bool;
    firersAtHead,
    \* History: every record that left so far was younger than the record
    \* that left before it.
    \* @type: Bool;
    leavesInOrder

vars == <<records, waiters, inflight, counter, intro, lastLeft,
          partnersAtHead, firersAtHead, leavesInOrder>>

Vals == {"a", "b"}
Pats == {"a", "b", "any"}
Sides == {"produce", "consume"}
Ords == 0..MaxOps
\* The ord of an element that carries none (reissued_ord = None).
NoOrd == -1
Carried == {NoOrd} \cup Ords

\* @type: (Str, Str) => Bool;
Matches(p, v) == p = "any" \/ p = v

\* The head of a side: an entry with the least ord.
\* @type: ($rec, Set($rec)) => Bool;
IsHeadR(r, S) == r \in S /\ \A s \in S : r.ord <= s.ord
\* @type: ($wtr, Set($wtr)) => Bool;
IsHeadW(w, S) == w \in S /\ \A x \in S : w.ord <= x.ord

\* A record leaves with the ord o: it is spent or removed.
\* @type: (Int) => Bool;
Leave(o) ==
    /\ leavesInOrder' = (leavesInOrder /\ lastLeft < o)
    /\ lastLeft' = o

\* operation_ord: the carried ord of a re-issued element, or the counter
\* value. Unsafe ReissueFreshOrd ignores the carried ord.
\* @type: ($elt) => Int;
OperationOrd(x) == IF x.ord # NoOrd /\ ~ReissueFreshOrd THEN x.ord ELSE counter

\* The first-round ord of an element: the counter value in its first round.
\* @type: ($elt) => Int;
FirstOrd(x) == IF x.first # NoOrd THEN x.first ELSE counter

\* A persistent element takes a fresh ord: one more introduction of it.
\* @type: ($elt, Int) => (Int -> Int);
Introduce(x, o) ==
    IF x.pers /\ o = counter THEN [intro EXCEPT ![FirstOrd(x)] = @ + 1] ELSE intro

\* Cx::at_head for the element x with the round ord o. own holds
\* the ords of its side. A fresh element (it carries no ord) reaches the
\* head only of an empty side (reaches_head). A re-entering element is at
\* the head when its ord precedes every ord of its side (head_ord and
\* precedes). Unsafe ReentryAtHead puts it at the head regardless of the
\* ords when the other side holds at most one entry.
\* @type: ($elt, Int, Set(Int), Bool) => Bool;
AtHead(x, o, own, otherAtMostOne) ==
    IF x.ord = NoOrd THEN own = {}
    ELSE IF ReentryAtHead /\ otherAtMostOne THEN TRUE
    ELSE \A q \in own : o < q

\* The re-issue of x after its round with the ord o: a persistent element
\* carries its first-round ord (reissued_ord: op.persistent.then_some(ord)),
\* and a datum that is not persistent carries none. Unsafe
\* NonPersistentFirstOrd gives the datum its ord too.
\* @type: ($elt, Int) => $elt;
Reissued(x, o) ==
    LET keeps == x.pers \/ NonPersistentFirstOrd
    IN [side |-> x.side,
        ord |-> IF keeps THEN (IF x.ord # NoOrd THEN x.ord ELSE o) ELSE NoOrd,
        first |-> IF keeps THEN FirstOrd(x) ELSE NoOrd,
        term |-> x.term,
        pers |-> x.pers,
        peek |-> x.peek]

\* The waiters that the datum with the value v fires: the head waiter when
\* it matches. Unsafe ScanWaiters: the first waiter that matches.
\* @type: (Str) => Set($wtr);
DataPartners(v) ==
    IF ScanWaiters
    THEN {w \in waiters : Matches(w.pat, v) /\ \A u \in waiters : Matches(u.pat, v) => w.ord <= u.ord}
    ELSE {w \in waiters : IsHeadW(w, waiters) /\ Matches(w.pat, v)}

\* One round of offer_head for the datum x. rest is the set of the
\* other elements in flight. The fire spends x when x is not persistent and
\* the waiter does not peek, and the waiter stays when it is persistent.
\* Else x re-issues: a divergent pair when the waiter stays, and the cap
\* (Cap = 1) when it leaves.
\* @type: ($elt, Set($elt)) => Bool;
DataRound(x, rest) ==
    LET o == OperationOrd(x)
        own == {r.ord : r \in records}
        partners == DataPartners(x.term)
    IN
    /\ counter' = counter + 1
    /\ intro' = Introduce(x, o)
    /\ IF AtHead(x, o, own, \A a, b \in waiters : a = b) /\ partners # {}
       THEN \E w \in partners :
              /\ waiters' = IF w.pers THEN waiters ELSE waiters \ {w}
              /\ partnersAtHead' = (partnersAtHead /\ IsHeadW(w, waiters))
              /\ firersAtHead' = (firersAtHead /\ \A q \in own : o < q)
              /\ UNCHANGED records
              /\ IF ~x.pers /\ ~w.peek
                 THEN /\ inflight' = rest
                      /\ Leave(o)
                 ELSE /\ inflight' = rest \cup {Reissued(x, o)}
                      /\ UNCHANGED <<lastLeft, leavesInOrder>>
       ELSE /\ records' = records \cup {[ord |-> o, val |-> x.term, pers |-> x.pers]}
            /\ inflight' = rest
            /\ UNCHANGED <<waiters, lastLeft, partnersAtHead, firersAtHead, leavesInOrder>>

\* One round of receive_head for the continuation x. The fire
\* spends x when x is not persistent, and the record stays when it is
\* persistent or x peeks. Else x re-issues as in DataRound.
\* @type: ($elt, Set($elt)) => Bool;
ContRound(x, rest) ==
    LET o == OperationOrd(x)
        own == {w.ord : w \in waiters}
        partners == {r \in records : IsHeadR(r, records) /\ Matches(x.term, r.val)}
    IN
    /\ counter' = counter + 1
    /\ intro' = Introduce(x, o)
    /\ IF AtHead(x, o, own, \A a, b \in records : a = b) /\ partners # {}
       THEN \E r \in partners :
              /\ records' = IF r.pers \/ x.peek THEN records ELSE records \ {r}
              /\ partnersAtHead' = (partnersAtHead /\ IsHeadR(r, records))
              /\ firersAtHead' = (firersAtHead /\ \A q \in own : o < q)
              /\ UNCHANGED waiters
              /\ inflight' = IF x.pers THEN rest \cup {Reissued(x, o)} ELSE rest
              /\ IF r.pers \/ x.peek
                 THEN UNCHANGED <<lastLeft, leavesInOrder>>
                 ELSE Leave(r.ord)
       ELSE /\ waiters' = waiters \cup {[ord |-> o, pat |-> x.term, pers |-> x.pers, peek |-> x.peek]}
            /\ inflight' = rest
            /\ UNCHANGED <<records, lastLeft, partnersAtHead, firersAtHead, leavesInOrder>>

Init ==
    /\ records = {}
    /\ waiters = {}
    /\ inflight = {}
    /\ counter = 0
    /\ intro = [s \in Ords |-> 0]
    /\ lastLeft = NoOrd
    /\ partnersAtHead = TRUE
    /\ firersAtHead = TRUE
    /\ leavesInOrder = TRUE

\* offer_head of a fresh send with the value v (persistent: !!).
\* @type: (Str, Bool) => Bool;
Produce(v, pers) ==
    /\ counter < MaxOps
    /\ pers => Reissues
    /\ DataRound([side |-> "produce", ord |-> NoOrd, first |-> NoOrd,
                  term |-> v, pers |-> pers, peek |-> FALSE], inflight)

\* receive_head of a fresh receive with the pattern p (persistent:
\* <=). A persistent receive never peeks.
\* @type: (Str, Bool, Bool) => Bool;
Consume(p, pers, peek) ==
    /\ counter < MaxOps
    /\ (pers \/ peek) => Reissues
    /\ ~(pers /\ peek)
    /\ ContRound([side |-> "consume", ord |-> NoOrd, first |-> NoOrd,
                  term |-> p, pers |-> pers, peek |-> peek], inflight)

\* The next round of the re-issued element e (a new space intent).
\* @type: ($elt) => Bool;
Reenter(e) ==
    /\ counter < MaxOps
    /\ e \in inflight
    /\ IF e.side = "produce"
       THEN DataRound(e, inflight \ {e})
       ELSE ContRound(e, inflight \ {e})

Next ==
    \/ \E v \in Vals, pers \in BOOLEAN : Produce(v, pers)
    \/ \E p \in Pats, pers \in BOOLEAN, peek \in BOOLEAN : Consume(p, pers, peek)
    \/ \E e \in inflight : Reenter(e)

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Invariants                                                              *)

RecType == [ord : Ords, val : Vals, pers : BOOLEAN]
WtrType == [ord : Ords, pat : Pats, pers : BOOLEAN, peek : BOOLEAN]
EltType == [side : Sides, ord : Carried, first : Carried, term : Vals \cup Pats,
            pers : BOOLEAN, peek : BOOLEAN]

TypeOK ==
    /\ records \subseteq RecType
    /\ waiters \subseteq WtrType
    /\ inflight \subseteq EltType
    /\ counter \in 0..MaxOps
    /\ intro \in [Ords -> 0..MaxOps]
    /\ lastLeft \in Carried
    /\ partnersAtHead \in BOOLEAN
    /\ firersAtHead \in BOOLEAN
    /\ leavesInOrder \in BOOLEAN

\* The heads, as sets (one entry at most, since the ords are unique).
HeadRecords == {r \in records : \A s \in records : r.ord <= s.ord}
HeadWaiters == {w \in waiters : \A x \in waiters : w.ord <= x.ord}

\* The head record and the head waiter match.
PendingMatch == \E r \in HeadRecords, w \in HeadWaiters : Matches(w.pat, r.val)

\* The element f in flight carries an ord that precedes every entry of its
\* own side: it shadows a pending match on that side.
\* @type: ($elt) => Bool;
Precedes(f) ==
    /\ f.ord # NoOrd
    /\ IF f.side = "produce"
       THEN \A r \in records : f.ord < r.ord
       ELSE \A w \in waiters : f.ord < w.ord

\* SHADOW(R, W, F) of decision 43: a pending match is shadowed.
Shadow == PendingMatch => \E f \in inflight : Precedes(f)

ShadowedNoPendingMatch == inflight # {} => Shadow

\* INV (QD-6) at every state with no element in flight.
QuiescentNoPendingMatch == inflight = {} => ~PendingMatch

ReissueKeepsOrd == \A f \in inflight : f.pers => f.ord # NoOrd /\ f.ord = f.first

IntroducedOnce ==
    /\ \A s \in Ords : intro[s] <= 1
    /\ \A r \in records : r.pers => intro[r.ord] = 1
    /\ \A w \in waiters : w.pers => intro[w.ord] = 1
    /\ \A f \in inflight : f.pers => f.first \in Ords /\ intro[f.first] = 1

FifoOrder == firersAtHead

LeaveOrder == leavesInOrder /\ \A r \in records : ~r.pers => lastLeft < r.ord

HeadBlocks == partnersAtHead

\* The ords of the entries, of the elements in flight and of the last
\* record that left are below the counter, and no ord at or above the
\* counter has an introduction.
OrdsBelowCounter ==
    /\ \A r \in records : r.ord < counter
    /\ \A w \in waiters : w.ord < counter
    /\ \A f \in inflight : f.ord < counter /\ f.first < counter
    /\ lastLeft < counter
    /\ \A s \in Ords : counter <= s => intro[s] = 0

\* One ord for each entry and for each element in flight that carries one.
UniqueOrds ==
    /\ \A r, s \in records : r.ord = s.ord => r = s
    /\ \A w, x \in waiters : w.ord = x.ord => w = x
    /\ \A r \in records, w \in waiters : r.ord # w.ord
    /\ \A f, g \in inflight : f.ord # NoOrd /\ f.ord = g.ord => f = g
    /\ \A f \in inflight : f.ord # NoOrd =>
          /\ \A r \in records : r.ord # f.ord
          /\ \A w \in waiters : w.ord # f.ord

\* The shapes that the rules allow: no persistent waiter peeks, a
\* continuation in flight is persistent, a persistent element in flight
\* carries its first-round ord, a datum in flight that is not persistent
\* carries none, and the P4 fragment holds no persistent entry and no peek.
Shapes ==
    /\ \A w \in waiters : w.pers => ~w.peek
    /\ \A f \in inflight :
          /\ f.side = "produce" => f.term \in Vals /\ ~f.peek
          /\ f.side = "consume" => f.term \in Pats /\ f.pers /\ ~f.peek
          /\ f.pers => f.ord = f.first
          /\ ~f.pers => f.ord = NoOrd /\ f.first = NoOrd
    /\ ~Reissues =>
          /\ inflight = {}
          /\ \A r \in records : ~r.pers
          /\ \A w \in waiters : ~w.pers /\ ~w.peek

\* The inductive strengthening for the Apalache BASE and STEP checks.
HeadIndInv ==
    /\ TypeOK
    /\ OrdsBelowCounter
    /\ UniqueOrds
    /\ Shapes
    /\ ShadowedNoPendingMatch
    /\ QuiescentNoPendingMatch
    /\ ReissueKeepsOrd
    /\ IntroducedOnce
    /\ FifoOrder
    /\ LeaveOrder
    /\ HeadBlocks

\* The elements in flight that Shapes allows.
InFlightShapes ==
    {[side |-> "produce", ord |-> o, first |-> o, term |-> v, pers |-> TRUE, peek |-> FALSE] :
        o \in Ords, v \in Vals}
    \cup {[side |-> "consume", ord |-> o, first |-> o, term |-> p, pers |-> TRUE, peek |-> FALSE] :
        o \in Ords, p \in Pats}
    \cup {[side |-> "produce", ord |-> NoOrd, first |-> NoOrd, term |-> v, pers |-> FALSE, peek |-> FALSE] :
        v \in Vals}

\* The STEP initializer: every state within the bounds that satisfies
\* HeadIndInv. The domains of waiters, inflight and intro hold only the
\* values that HeadIndInv allows (Shapes and IntroducedOnce), so no state
\* that satisfies HeadIndInv is missing.
IndInit ==
    /\ records \in SUBSET RecType
    /\ waiters \in SUBSET {w \in WtrType : ~(w.pers /\ w.peek)}
    /\ inflight \in SUBSET InFlightShapes
    /\ counter \in 0..MaxOps
    /\ intro \in [Ords -> 0..1]
    /\ lastLeft \in Carried
    /\ partnersAtHead \in BOOLEAN
    /\ firersAtHead \in BOOLEAN
    /\ leavesInOrder \in BOOLEAN
    /\ HeadIndInv

=============================================================================
