-------------------------- MODULE NativeOwnerRefusal --------------------------
(***************************************************************************)
(* G3 (DR-135, design v2 rules R3, R5 and R7, and section 4): owner        *)
(* refusals in native play and native replay under racing evaluation       *)
(* (DR-132).                                                               *)
(*                                                                         *)
(* Relation to NativeCandidateOrder.tla. That model covers one pattern for *)
(* each step, and its scope note leaves joins and commit vetoes to Rust    *)
(* tests. The owner checks of G3 need a two-pattern join, two racing       *)
(* operations, an owner ledger and a record of refusals. These parts need  *)
(* new constants and new variables. In NativeCandidateOrder.tla they would *)
(* change all six of its configs, and they would multiply its store        *)
(* enumeration by the owner state. So this module is a sibling with its    *)
(* own configs. It also has Apalache types, which NativeCandidateOrder.tla *)
(* does not have.                                                          *)
(*                                                                         *)
(* Terms.                                                                  *)
(*   A row is one entry of the native operation journal (one locked RSpace *)
(*   operation). Rows are in start order (DR-132 decision 2).              *)
(*   An owner region is a region of a stored process whose signature is    *)
(*   not Unit and whose lane is not a client lane (v2 section 3).          *)
(*   The owner table T lists at most room purses with their budgets. An    *)
(*   unlisted purse has budget 0 in play (v2 R3).                          *)
(*   The snapshot S_r of row r is the owner ledger at budget_start of r.   *)
(*   Play captures it in the critical section of start_operation.          *)
(*                                                                         *)
(* Scenario. Two produces race on disjoint channels. They share only the   *)
(* owner ledger.                                                           *)
(*   Row "A" produces the datum a on channel x. The stored continuation K  *)
(*   is the two-pattern join  for (u <- x & v <- y). The stored data y1    *)
(*   and y2 wait on channel y.                                             *)
(*   Row "B" produces the datum b on channel z. The stored continuations   *)
(*   L1 and L2 each have one pattern on z.                                 *)
(* The data a and b belong to the running deployments (client lanes), so   *)
(* they have no owner share. K, L1, L2, y1 and y2 are stored processes.    *)
(* The purses p and q own them. K belongs to p. The other owners are free, *)
(* up to the symmetry of L1 with L2 and of y1 with y2.                     *)
(*                                                                         *)
(* Shares. A COMM of continuation k with n data charges each owner region  *)
(* m(n) = 1 + growth * (n - 1) units. Each region pays the whole           *)
(* measurement (regions.rs), and the transfer grows with the data. An      *)
(* item test of datum d measures a COMM of k with d alone (n = 1). So an   *)
(* item test is exact for one pattern and a lower bound for a join (v2     *)
(* R5). The constant Growths gives the values of growth.                   *)
(*                                                                         *)
(* Play of one row r (v2 section 4.1), in literate form:                   *)
(*   <start> = append the row; budget_start := |attempts|; S_r := ledger.  *)
(*             One critical section.                                       *)
(*   <intro> = append the introduction attempt (client lane, no owner      *)
(*             share).                                                     *)
(*   for each continuation k of r, in the random order of play:            *)
(*     <greedy match> = for each pattern, test the candidates in a random  *)
(*             order with the item test at S_r, and take the first that    *)
(*             passes. The incoming datum fills its own pattern (R6).      *)
(*     if no tuple: try the next continuation (a pattern miss).            *)
(*     <tuple test> = in the critical section of the charge:               *)
(*       if every share fits the ledger: append the COMM attempt, deduct   *)
(*         the shares, and stop (Matched);                                 *)
(*       else: no charge, record (k, tuple, position = |attempts|), and    *)
(*         try the next continuation.                                      *)
(*   <finish> = Stored when no continuation fired.                         *)
(* Every purse that a test reads is consulted. A run that consulted an     *)
(* unlisted purse while T is not full does not ship. The producer then     *)
(* adds the purse and plays again (discovery, v2 R3). The model starts     *)
(* from the final table, so only the shipped run is explored.              *)
(*                                                                         *)
(* Replay of a record (v2 section 4.3), in literate form:                  *)
(*   <journal> = start positions do not decrease in journal order; every   *)
(*             attempt lies inside the interval of its row                 *)
(*             (checked_operations.rs check_link); every refusal position  *)
(*             lies inside the interval of its row and before its COMM     *)
(*             attempt.                                                    *)
(*   <fold>    = every granted COMM fits the fold of the attempts before   *)
(*             it, with the listed budgets and 0 for an unlisted purse.    *)
(*   <reads>   = |T| <= room, and every listed budget equals the balance   *)
(*             of the purse at R_{i-1}.                                    *)
(*   for each row r:                                                       *)
(*     for each named refusal (k, tuple, pos):                             *)
(*       the items of the tuple pass the item test at S_r (see F1-a);      *)
(*       some share of the tuple exceeds the fold at pos (the shortfall).  *)
(*     if r is Stored, for each continuation k with no named refusal:      *)
(*       no tuple of k passes the item tests at S_r. An unlisted purse     *)
(*       passes when |T| < room and has budget 0 when T is full. A full    *)
(*       tuple is a mismatch.                                              *)
(* Replay reads no live ledger. Each check reads the record and the live   *)
(* items of the row's own channels. So every replay schedule reaches the   *)
(* same verdict, and the model computes the verdict in one step.           *)
(*                                                                         *)
(* Finding F1-a (an addition to v2 section 4.3). v2 checks a named refusal *)
(* by its shortfall only. A producer can then name a refusal for a tuple   *)
(* whose item fails the item test at S_r. That tuple always shows a        *)
(* shortfall, because a full share is at least a partial share and the     *)
(* fold only falls. The Stored rule then skips the continuation and hides  *)
(* a fundable tuple of the same continuation. Honest play never refuses    *)
(* such a tuple, because its greedy match takes only item-passing data.    *)
(* So replay must also item-test the items of each refused tuple at S_r.   *)
(* The control "UnpickableRefusal" removes this test and violates          *)
(* StoredClaimIsHonest.                                                    *)
(*                                                                         *)
(* The adversary. With Adversary = TRUE the producer can also:             *)
(*   - list wrong budgets in T (Init);                                     *)
(*   - claim a pattern miss for any continuation (AdvSkip);                *)
(*   - name any refusal, for any tuple and any position (AdvRefuse);       *)
(*   - record a later budget_start and snapshot for a row (AdvRestart);    *)
(*   - ship a run that consulted an unlisted purse while T is not full.    *)
(* Each of these is one deviation. The counter deviations bounds them by   *)
(* MaxDeviations, and a record is honest when it has no deviation. With    *)
(* MaxDeviations = 1 the soundness invariants range over every record      *)
(* with one deviation, which each control needs. A first run with no bound *)
(* passed 181 million distinct states at depth 9 after 26 minutes, so the  *)
(* bound keeps the gate practical.                                         *)
(*                                                                         *)
(* Invariants.                                                             *)
(*   LedgerIsFold          the play ledger is the fold of the recorded     *)
(*                         attempts (v2 section 4.4 item 1).               *)
(*   HonestStoredReplays   replay accepts every Stored row of an honest    *)
(*                         record (completeness, section 4.4 item 5).      *)
(*   PlayReplayAgree       replay accepts an honest record, its fold at    *)
(*                         each budget_start is the snapshot of play, and  *)
(*                         its final fold is the play ledger (items 2, 3). *)
(*   StoredClaimIsHonest   every Stored row of an accepted record is       *)
(*                         justified at some start that an honest          *)
(*                         interleaving reaches: each continuation either  *)
(*                         has a true refusal or has no tuple that passes  *)
(*                         the item tests at the true snapshot (item 6).   *)
(*   RefusalNeedsShortfall every refusal of an accepted record has a true  *)
(*                         shortfall at its position.                      *)
(* "True" means the balances at R_{i-1}, with budget 0 for an unlisted     *)
(* purse when T is full (the producer's discretion past the room, v2       *)
(* section 10). A start v of row r is honest when every attempt before v   *)
(* belongs to an earlier row and v is not after the first attempt of r.    *)
(*                                                                         *)
(* Defect selects one negative control. Each control must violate the      *)
(* named invariant in its *Unsafe.cfg:                                     *)
(*   "UnrecordedRefusal"    play refuses and records nothing               *)
(*                          -> HonestStoredReplays                         *)
(*   "LiveLedgerItemTest"   play item-tests the live ledger, not S_r       *)
(*                          -> PlayReplayAgree                             *)
(*   "UnverifiedRefusal"    replay accepts a refusal with no shortfall     *)
(*                          check -> RefusalNeedsShortfall                 *)
(*   "PessimisticUnlisted"  replay gives an unlisted purse budget 0 also   *)
(*                          when T is not full -> StoredClaimIsHonest      *)
(*   "LateBudgetStart"      replay does not bound budget_start by the      *)
(*                          first attempt of the row -> StoredClaimIsHonest*)
(*   "UnpickableRefusal"    replay does not item-test a refused tuple      *)
(*                          (finding F1-a) -> StoredClaimIsHonest          *)
(*                                                                         *)
(* Rust correspondence (v2 sections 3 and 4; G3 code does not exist yet):  *)
(*   start, snapshot        native_runtime/operations.rs start_operation   *)
(*   item test              rspace++ space_matcher.rs (R5 item handle)     *)
(*   tuple test, refusal    observer admit_comm, native_runtime            *)
(*                          recording.rs (R5, R7)                          *)
(*   owner table T          budget recording section, wire.rs (R3)         *)
(*   journal checks         native_runtime/checked_operations.rs           *)
(*                          check_operation_journal, check_link            *)
(*   fold                   execution/budget_trace.rs check_budget_evidence*)
(*   Stored rule, refusals  replay_rspace/native_candidate/selection.rs    *)
(*   reads                  execution.rs, after the replayed run (R3)      *)
(*                                                                         *)
(* Scope. Both rows are produces. A consume behaves like a produce with    *)
(* one candidate continuation, its own. Commit vetoes (E3-a of DR-129),    *)
(* repeated channels (E1), introductions on owner lanes (R8) and peek      *)
(* restorations (R9) are outside this model. The introduction attempts are *)
(* client-paid and always granted.                                         *)
(*                                                                         *)
(* G3 step S11 (DR-135 item 13): the signed limit and the hold of a caller *)
(* cover only the work that the caller pays. This model agrees and needs   *)
(* no change. It has no client bound: the introduction attempts are the    *)
(* only client work, and they are always granted. The owner shares come    *)
(* only from the owner ledger. Every share of this model is at least 1, so *)
(* every owner here has a price of 1 or more. A share at price 0 is 0, so  *)
(* it always fits an owner budget and never causes a refusal. Its work     *)
(* counts against the limit of the caller (S11 decision 2).                *)
(*                                                                         *)
(* The model has no recursion. The fold unrolls over the four attempt      *)
(* slots, which is the largest record of the scenario.                     *)
(***************************************************************************)
EXTENDS Integers, FiniteSets

CONSTANTS
  \* @type: Str;
  Defect,
  \* @type: Int;
  MaxBalance,
  \* @type: Bool;
  Adversary,
  \* @type: Set(Int);
  Rooms,
  \* @type: Set(Int);
  Growths,
  \* @type: Int;
  MaxDeviations

ASSUME Defect \in {
  "None",
  "UnrecordedRefusal",
  "UnverifiedRefusal",
  "LiveLedgerItemTest",
  "PessimisticUnlisted",
  "LateBudgetStart",
  "UnpickableRefusal"
}

ASSUME /\ MaxBalance \in 0..4
       /\ Adversary \in BOOLEAN
       /\ Rooms \subseteq 1..2
       /\ Rooms # {}
       /\ Growths \subseteq 0..1
       /\ Growths # {}
       /\ MaxDeviations \in 0..2

Rows == {"A", "B"}

Purses == {"p", "q"}

Conts == {"K", "L1", "L2"}

NoName == "none"

Client == "c"

IntroName == "intro"

OwnerDomain == {"K", "L1", "L2", "y1", "y2", NoName, IntroName}

\* The unordered owner pairs of (L1, L2) and of (y1, y2).
\* @type: Set(<<Str, Str>>);
OwnerPairs == {<<"p", "p">>, <<"p", "q">>, <<"q", "q">>}

\* The largest record: two introductions and two COMM attempts.
MaxAtt == 4

Slots == 1..MaxAtt

Positions == 0..MaxAtt

Phases == {"Play", "Done"}

RowPhases == {"Idle", "Started", "Introduced", "Matched", "Done"}

\* @type: Str => Set(Str);
ContsOf(r) == IF r = "A" THEN {"K"} ELSE {"L1", "L2"}

\* @type: Str => Str;
RowOf(k) == IF k = "K" THEN "A" ELSE "B"

\* The stored datum that a tuple of k takes; NoName for one pattern.
\* @type: Str => Set(Str);
PicksOf(k) == IF k = "K" THEN {"y1", "y2"} ELSE {NoName}

\* @type: Str => Str;
OtherPick(y) == IF y = "y1" THEN "y2" ELSE "y1"

\* @type: Str => Str;
OtherRow(r) == IF r = "A" THEN "B" ELSE "A"

\* The number of data of a full COMM of k: the incoming datum and, for the
\* join, one stored datum.
\* @type: Str => Int;
NData(k) == IF k = "K" THEN 2 ELSE 1

\* @type: {row: Str, cont: Str, pick: Str};
EmptyAttempt == [row |-> NoName, cont |-> NoName, pick |-> NoName]

\* @type: {named: Bool, pick: Str, pos: Int};
NoRefusal == [named |-> FALSE, pick |-> NoName, pos |-> 0]

\* @type: Str -> Int;
Zero == [x \in Purses |-> 0]

VARIABLES
  \* The owner of each stored process (constant after Init).
  \* @type: Str -> Str;
  owner,
  \* The per-region growth of a COMM with an extra datum (constant).
  \* @type: Int;
  growth,
  \* The balances of the purses at R_{i-1} (constant).
  \* @type: Str -> Int;
  balance,
  \* The room of the owner table (constant).
  \* @type: Int;
  room,
  \* The purses of the owner table T (constant).
  \* @type: Set(Str);
  listed,
  \* The budgets that T lists (constant; 0 for an unlisted purse).
  \* @type: Str -> Int;
  tval,
  \* The number of adversary steps.
  \* @type: Int;
  deviations,
  \* @type: Str -> Str;
  pc,
  \* The row that started first (the first row of the journal).
  \* @type: Str;
  firstRow,
  \* @type: Str -> Int;
  budgetStart,
  \* @type: Str -> Int;
  budgetEnd,
  \* The attempt index of the introduction of each row.
  \* @type: Str -> Int;
  introIdx,
  \* The snapshot S_r of each row.
  \* @type: Str -> (Str -> Int);
  snap,
  \* The live owner ledger of play.
  \* @type: Str -> Int;
  live,
  \* The recorded attempts, in append order.
  \* @type: Int -> {row: Str, cont: Str, pick: Str};
  att,
  \* @type: Int;
  nAtt,
  \* The attempt index of the COMM of each row; 0 for none.
  \* @type: Str -> Int;
  commIdx,
  \* The continuations that a row tried without a grant.
  \* @type: Str -> Set(Str);
  tried,
  \* The refusal record of each continuation.
  \* @type: Str -> {named: Bool, pick: Str, pos: Int};
  refusal,
  \* The purses that play consulted.
  \* @type: Set(Str);
  consulted,
  \* @type: Str -> Bool;
  restarted,
  \* @type: Str;
  phase,
  \* The verdict of replay on the shipped record.
  \* @type: Bool;
  verdict

world == <<owner, growth, balance, room, listed, tval>>

vars == <<owner, growth, balance, room, listed, tval, deviations, pc, firstRow,
          budgetStart, budgetEnd, introIdx, snap, live, att, nAtt, commIdx,
          tried, refusal, consulted, restarted, phase, verdict>>

\* A record is honest when the producer took no adversary step.
honest == deviations = 0

\* The adversary can take one more step.
CanDeviate == Adversary /\ deviations < MaxDeviations

-----------------------------------------------------------------------------
(* Shares and item tests.                                                  *)

\* The charge of one owner region in a COMM with n data.
\* @type: Int => Int;
PerRegion(n) == 1 + growth * (n - 1)

\* The owner shares of the full COMM of k with its tuple.
\* @type: (Str, Str) => (Str -> Int);
FullShare(k, pick) ==
  [x \in Purses |->
     (IF owner[k] = x THEN PerRegion(NData(k)) ELSE 0)
       + (IF k = "K" /\ owner[pick] = x THEN PerRegion(NData(k)) ELSE 0)]

\* The item test of the incoming datum: a COMM of k with that datum alone.
\* The datum is on a client lane, so only the region of k is an owner region.
\* @type: Str => (Str -> Int);
IncomingShare(k) == [x \in Purses |-> IF owner[k] = x THEN 1 ELSE 0]

\* The item test of a stored datum y: a COMM of k with y alone.
\* @type: (Str, Str) => (Str -> Int);
StoredShare(k, y) ==
  [x \in Purses |->
     (IF owner[k] = x THEN 1 ELSE 0) + (IF owner[y] = x THEN 1 ELSE 0)]

\* A share fits a budget. A purse in free passes (the unlisted rule).
\* @type: (Str -> Int, Str -> Int, Set(Str)) => Bool;
FitsIn(share, budget, free) ==
  \A x \in Purses : x \in free \/ share[x] <= budget[x]

\* @type: (Str -> Int) => Set(Str);
PursesOf(share) == {x \in Purses : share[x] > 0}

\* The greedy match of play for continuation k. The stored candidates are
\* tested in a random order, first then the other one, and the first that
\* passes is taken. The result names the consulted purses.
\* @type: (Str, Str, Str -> Int) => {found: Bool, pick: Str, seen: Set(Str)};
GreedyMatch(k, first, view) ==
  LET inc == IncomingShare(k)
      second == OtherPick(first)
      sf == StoredShare(k, first)
      ss == StoredShare(k, second)
  IN IF ~FitsIn(inc, view, {})
     THEN [found |-> FALSE, pick |-> NoName, seen |-> PursesOf(inc)]
     ELSE IF k # "K"
     THEN [found |-> TRUE, pick |-> NoName, seen |-> PursesOf(inc)]
     ELSE IF FitsIn(sf, view, {})
     THEN [found |-> TRUE, pick |-> first,
           seen |-> PursesOf(inc) \cup PursesOf(sf)]
     ELSE IF FitsIn(ss, view, {})
     THEN [found |-> TRUE, pick |-> second,
           seen |-> PursesOf(inc) \cup PursesOf(sf) \cup PursesOf(ss)]
     ELSE [found |-> FALSE, pick |-> NoName,
           seen |-> PursesOf(inc) \cup PursesOf(sf) \cup PursesOf(ss)]

\* Some tuple of k has every item passing the item test (the existence form
\* of the greedy match that replay runs in its Stored rule).
\* @type: (Str, Str -> Int, Set(Str)) => Bool;
ItemFundable(k, view, free) ==
  /\ FitsIn(IncomingShare(k), view, free)
  /\ (k = "K" => \E y \in {"y1", "y2"} : FitsIn(StoredShare(k, y), view, free))

\* Every item of the given tuple of k passes the item test.
\* @type: (Str, Str, Str -> Int, Set(Str)) => Bool;
TupleItemsPass(k, pick, view, free) ==
  /\ FitsIn(IncomingShare(k), view, free)
  /\ (k = "K" => FitsIn(StoredShare(k, pick), view, free))

-----------------------------------------------------------------------------
(* The fold of the recorded attempts.                                      *)

\* The share of purse x that attempt slot j draws, when j is among the
\* first n attempts and is a COMM.
\* @type: (Int, Int, Str) => Int;
AttemptDraw(j, n, x) ==
  IF j <= n /\ att[j].cont \in Conts
  THEN FullShare(att[j].cont, att[j].pick)[x]
  ELSE 0

\* @type: (Int, Str) => Int;
Drawn(n, x) ==
  AttemptDraw(1, n, x) + AttemptDraw(2, n, x)
    + AttemptDraw(3, n, x) + AttemptDraw(4, n, x)

\* @type: (Str -> Int, Int) => (Str -> Int);
FoldFrom(base, n) == [x \in Purses |-> base[x] - Drawn(n, x)]

\* The budgets of play and of replay: the listed budget, or 0.
\* @type: Str -> Int;
ListedBudget == [x \in Purses |-> IF x \in listed THEN tval[x] ELSE 0]

\* @type: Int => (Str -> Int);
ListedFold(n) == FoldFrom(ListedBudget, n)

-----------------------------------------------------------------------------
(* Replay.                                                                 *)

\* @type: Str;
SecondRow == OtherRow(firstRow)

\* checked_operations.rs check_link: an attempt of row r lies inside the
\* interval [budget_start, budget_end) of r. Control "LateBudgetStart"
\* drops the lower bound.
\* @type: Int => Bool;
AttemptInInterval(j) ==
  LET r == att[j].row
  IN /\ (Defect # "LateBudgetStart" => budgetStart[r] <= j - 1)
     /\ j <= budgetEnd[r]

\* @type: Str => Bool;
RefusalInInterval(k) ==
  LET r == RowOf(k)
  IN /\ budgetStart[r] <= refusal[k].pos
     /\ refusal[k].pos <= budgetEnd[r]
     /\ (commIdx[r] > 0 => refusal[k].pos <= commIdx[r] - 1)

JournalOk ==
  /\ budgetStart[firstRow] <= budgetStart[SecondRow]
  /\ \A r \in Rows : budgetStart[r] <= budgetEnd[r] /\ budgetEnd[r] <= nAtt
  /\ \A j \in Slots : j <= nAtt => AttemptInInterval(j)
  /\ \A k \in Conts : refusal[k].named => RefusalInInterval(k)

FoldOk ==
  \A j \in Slots :
    (j <= nAtt /\ att[j].cont \in Conts) =>
      FitsIn(FullShare(att[j].cont, att[j].pick), ListedFold(j - 1), {})

ReadsOk ==
  /\ Cardinality(listed) <= room
  /\ \A x \in listed : tval[x] = balance[x]

\* A named refusal: a valid tuple whose items pass the item test at S_r
\* (finding F1-a) and that shows a shortfall at its position.
\* @type: Str => Bool;
RefusalOk(k) ==
  LET r == RowOf(k)
      pick == refusal[k].pick
  IN /\ pick \in PicksOf(k)
     /\ (Defect # "UnpickableRefusal" =>
           TupleItemsPass(k, pick, ListedFold(budgetStart[r]), {}))
     /\ (Defect # "UnverifiedRefusal" =>
           \E x \in Purses : FullShare(k, pick)[x] > ListedFold(refusal[k].pos)[x])

\* The unlisted rule of the Stored rule: an unlisted purse passes when T is
\* not full. Control "PessimisticUnlisted" never lets it pass.
\* @type: Set(Str);
UnlistedPasses ==
  IF Defect = "PessimisticUnlisted" \/ Cardinality(listed) >= room
  THEN {}
  ELSE Purses \ listed

\* The Stored rule: a continuation with no named refusal has no tuple that
\* passes the item tests at S_r.
\* @type: Str => Bool;
StoredRuleOk(r) ==
  \A k \in ContsOf(r) :
    ~refusal[k].named =>
      ~ItemFundable(k, ListedFold(budgetStart[r]), UnlistedPasses)

\* @type: Str => Bool;
RowOk(r) ==
  /\ \A k \in ContsOf(r) : refusal[k].named => RefusalOk(k)
  /\ (commIdx[r] = 0 => StoredRuleOk(r))

Verdict ==
  /\ JournalOk
  /\ FoldOk
  /\ ReadsOk
  /\ \A r \in Rows : RowOk(r)

-----------------------------------------------------------------------------
(* The semantic side of the invariants.                                    *)

\* The true budgets: the balance at R_{i-1}, or 0 for an unlisted purse
\* when T is full.
\* @type: Str -> Int;
EffectiveBudget ==
  [x \in Purses |->
     IF x \notin listed /\ Cardinality(listed) >= room THEN 0 ELSE balance[x]]

\* @type: Int => (Str -> Int);
TrueFold(n) == FoldFrom(EffectiveBudget, n)

\* A start v of row r that an honest interleaving reaches.
\* @type: (Str, Int) => Bool;
HonestStart(r, v) ==
  /\ v <= introIdx[r] - 1
  /\ \A j \in Slots : j <= v => (r # firstRow /\ att[j].row = firstRow)

\* @type: Str => Int;
LastPosition(r) == IF commIdx[r] > 0 THEN commIdx[r] - 1 ELSE nAtt

\* A refusal that honest play makes when row r starts at v: its tuple passes
\* the item tests at the true snapshot, and the tuple test fails at its
\* position.
\* @type: (Str, Int) => Bool;
TrulyRefused(k, v) ==
  LET r == RowOf(k)
      pick == refusal[k].pick
  IN /\ refusal[k].named
     /\ pick \in PicksOf(k)
     /\ v <= refusal[k].pos
     /\ refusal[k].pos <= LastPosition(r)
     /\ TupleItemsPass(k, pick, TrueFold(v), {})
     /\ \E x \in Purses : FullShare(k, pick)[x] > TrueFold(refusal[k].pos)[x]

\* @type: Str => Bool;
Justified(r) ==
  \E v \in Positions :
    /\ HonestStart(r, v)
    /\ \A k \in ContsOf(r) :
         TrulyRefused(k, v) \/ ~ItemFundable(k, TrueFold(v), {})

-----------------------------------------------------------------------------
(* Play.                                                                   *)

\* The item tests read S_r. Control "LiveLedgerItemTest" reads the ledger.
\* @type: Str => (Str -> Int);
ItemView(r) == IF Defect = "LiveLedgerItemTest" THEN live ELSE snap[r]

\* @type: (Str, Str, Str) => {found: Bool, pick: Str, seen: Set(Str)};
MatchOf(r, k, first) == GreedyMatch(k, first, ItemView(r))

Init ==
  /\ \E lp \in OwnerPairs, yp \in OwnerPairs :
       owner = [i \in OwnerDomain |->
                  IF i = "K" THEN "p"
                  ELSE IF i = "L1" THEN lp[1]
                  ELSE IF i = "L2" THEN lp[2]
                  ELSE IF i = "y1" THEN yp[1]
                  ELSE IF i = "y2" THEN yp[2]
                  ELSE Client]
  /\ growth \in Growths
  /\ balance \in [Purses -> 0..MaxBalance]
  /\ room \in Rooms
  /\ listed \in SUBSET Purses
  /\ Cardinality(listed) <= room
  /\ \/ /\ Adversary
        /\ tval \in [Purses -> 0..MaxBalance]
        /\ \A x \in Purses : x \notin listed => tval[x] = 0
     \/ /\ ~Adversary
        /\ tval = [x \in Purses |-> IF x \in listed THEN balance[x] ELSE 0]
  /\ deviations = IF \A x \in listed : tval[x] = balance[x] THEN 0 ELSE 1
  /\ deviations <= MaxDeviations
  /\ pc = [r \in Rows |-> "Idle"]
  /\ firstRow = NoName
  /\ budgetStart = [r \in Rows |-> 0]
  /\ budgetEnd = [r \in Rows |-> 0]
  /\ introIdx = [r \in Rows |-> 0]
  /\ snap = [r \in Rows |-> Zero]
  /\ live = [x \in Purses |-> IF x \in listed THEN tval[x] ELSE 0]
  /\ att = [j \in Slots |-> EmptyAttempt]
  /\ nAtt = 0
  /\ commIdx = [r \in Rows |-> 0]
  /\ tried = [r \in Rows |-> {}]
  /\ refusal = [k \in Conts |-> NoRefusal]
  /\ consulted = {}
  /\ restarted = [r \in Rows |-> FALSE]
  /\ phase = "Play"
  /\ verdict = FALSE

\* @type: Str => Bool;
Start(r) ==
  /\ phase = "Play"
  /\ pc[r] = "Idle"
  /\ pc' = [pc EXCEPT ![r] = "Started"]
  /\ budgetStart' = [budgetStart EXCEPT ![r] = nAtt]
  /\ snap' = [snap EXCEPT ![r] = live]
  /\ firstRow' = IF firstRow = NoName THEN r ELSE firstRow
  /\ UNCHANGED <<world, deviations, budgetEnd, introIdx, live, att, nAtt, commIdx,
                 tried, refusal, consulted, restarted, phase, verdict>>

\* @type: Str => Bool;
Intro(r) ==
  /\ phase = "Play"
  /\ pc[r] = "Started"
  /\ att' = [att EXCEPT ![nAtt + 1] = [row |-> r, cont |-> IntroName, pick |-> NoName]]
  /\ nAtt' = nAtt + 1
  /\ introIdx' = [introIdx EXCEPT ![r] = nAtt + 1]
  /\ pc' = [pc EXCEPT ![r] = "Introduced"]
  /\ UNCHANGED <<world, deviations, firstRow, budgetStart, budgetEnd, snap, live,
                 commIdx, tried, refusal, consulted, restarted, phase, verdict>>

\* @type: (Str, Str, Str) => Bool;
TryGuard(r, k, first) ==
  /\ phase = "Play"
  /\ pc[r] = "Introduced"
  /\ k \in ContsOf(r)
  /\ k \notin tried[r]
  /\ first \in PicksOf(k)

\* No tuple passes the item tests: a pattern miss.
\* @type: (Str, Str, Str) => Bool;
TryMiss(r, k, first) ==
  LET m == MatchOf(r, k, first)
  IN /\ TryGuard(r, k, first)
     /\ ~m.found
     /\ consulted' = consulted \cup m.seen
     /\ tried' = [tried EXCEPT ![r] = @ \cup {k}]
     /\ UNCHANGED <<world, deviations, pc, firstRow, budgetStart, budgetEnd,
                    introIdx, snap, live, att, nAtt, commIdx, refusal,
                    restarted, phase, verdict>>

\* The tuple test passes: the COMM attempt and its deduction, in one
\* critical section.
\* @type: (Str, Str, Str) => Bool;
TryGrant(r, k, first) ==
  LET m == MatchOf(r, k, first)
      f == FullShare(k, m.pick)
  IN /\ TryGuard(r, k, first)
     /\ m.found
     /\ FitsIn(f, live, {})
     /\ consulted' = consulted \cup m.seen \cup PursesOf(f)
     /\ att' = [att EXCEPT ![nAtt + 1] = [row |-> r, cont |-> k, pick |-> m.pick]]
     /\ nAtt' = nAtt + 1
     /\ live' = [x \in Purses |-> live[x] - f[x]]
     /\ commIdx' = [commIdx EXCEPT ![r] = nAtt + 1]
     /\ pc' = [pc EXCEPT ![r] = "Matched"]
     /\ UNCHANGED <<world, deviations, firstRow, budgetStart, budgetEnd, introIdx,
                    snap, tried, refusal, restarted, phase, verdict>>

\* The tuple test fails: no charge, and a refusal record at position
\* |attempts| (R7). Control "UnrecordedRefusal" records nothing.
\* @type: (Str, Str, Str) => Bool;
TryRefuse(r, k, first) ==
  LET m == MatchOf(r, k, first)
      f == FullShare(k, m.pick)
  IN /\ TryGuard(r, k, first)
     /\ m.found
     /\ ~FitsIn(f, live, {})
     /\ consulted' = consulted \cup m.seen \cup PursesOf(f)
     /\ refusal' =
          IF Defect = "UnrecordedRefusal"
          THEN refusal
          ELSE [refusal EXCEPT ![k] = [named |-> TRUE, pick |-> m.pick, pos |-> nAtt]]
     /\ tried' = [tried EXCEPT ![r] = @ \cup {k}]
     /\ UNCHANGED <<world, deviations, pc, firstRow, budgetStart, budgetEnd,
                    introIdx, snap, live, att, nAtt, commIdx, restarted,
                    phase, verdict>>

\* The adversary claims a pattern miss.
\* @type: (Str, Str) => Bool;
AdvSkip(r, k) ==
  /\ CanDeviate
  /\ TryGuard(r, k, IF k = "K" THEN "y1" ELSE NoName)
  /\ tried' = [tried EXCEPT ![r] = @ \cup {k}]
  /\ deviations' = deviations + 1
  /\ UNCHANGED <<world, pc, firstRow, budgetStart, budgetEnd, introIdx, snap,
                 live, att, nAtt, commIdx, refusal, consulted, restarted,
                 phase, verdict>>

\* The adversary names any refusal, for any tuple and any position.
\* @type: (Str, Str, Str, Int) => Bool;
AdvRefuse(r, k, pick, pos) ==
  /\ CanDeviate
  /\ TryGuard(r, k, pick)
  /\ refusal' = [refusal EXCEPT ![k] = [named |-> TRUE, pick |-> pick, pos |-> pos]]
  /\ tried' = [tried EXCEPT ![r] = @ \cup {k}]
  /\ deviations' = deviations + 1
  /\ UNCHANGED <<world, pc, firstRow, budgetStart, budgetEnd, introIdx, snap,
                 live, att, nAtt, commIdx, consulted, restarted, phase,
                 verdict>>

\* The adversary records a later budget_start and its snapshot.
\* @type: Str => Bool;
AdvRestart(r) ==
  /\ CanDeviate
  /\ phase = "Play"
  /\ pc[r] \in {"Started", "Introduced"}
  /\ ~restarted[r]
  /\ budgetStart' = [budgetStart EXCEPT ![r] = nAtt]
  /\ snap' = [snap EXCEPT ![r] = live]
  /\ restarted' = [restarted EXCEPT ![r] = TRUE]
  /\ deviations' = deviations + 1
  /\ UNCHANGED <<world, pc, firstRow, budgetEnd, introIdx, live, att, nAtt,
                 commIdx, tried, refusal, consulted, phase, verdict>>

\* @type: Str => Bool;
Finish(r) ==
  /\ phase = "Play"
  /\ \/ pc[r] = "Matched"
     \/ pc[r] = "Introduced" /\ tried[r] = ContsOf(r)
  /\ pc' = [pc EXCEPT ![r] = "Done"]
  /\ budgetEnd' = [budgetEnd EXCEPT ![r] = nAtt]
  /\ UNCHANGED <<world, deviations, firstRow, budgetStart, introIdx, snap, live,
                 att, nAtt, commIdx, tried, refusal, consulted, restarted,
                 phase, verdict>>

\* Discovery (v2 R3): the shipped run consulted only listed purses, or T is
\* full.
Discovered == consulted \subseteq listed \/ Cardinality(listed) >= room

\* The producer ships the record, and replay computes its verdict.
Ship ==
  /\ phase = "Play"
  /\ \A r \in Rows : pc[r] = "Done"
  /\ \/ /\ Discovered
        /\ deviations' = deviations
     \/ /\ CanDeviate
        /\ ~Discovered
        /\ deviations' = deviations + 1
  /\ phase' = "Done"
  /\ verdict' = Verdict
  /\ UNCHANGED <<world, pc, firstRow, budgetStart, budgetEnd, introIdx, snap,
                 live, att, nAtt, commIdx, tried, refusal, consulted,
                 restarted>>

Next ==
  \/ \E r \in Rows : Start(r)
  \/ \E r \in Rows : Intro(r)
  \/ \E r \in Rows, k \in Conts, y \in {"y1", "y2", NoName} :
       \/ TryMiss(r, k, y)
       \/ TryGrant(r, k, y)
       \/ TryRefuse(r, k, y)
  \/ \E r \in Rows, k \in Conts : AdvSkip(r, k)
  \/ \E r \in Rows, k \in Conts, y \in {"y1", "y2", NoName}, n \in Positions :
       AdvRefuse(r, k, y, n)
  \/ \E r \in Rows : AdvRestart(r)
  \/ \E r \in Rows : Finish(r)
  \/ Ship

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Invariants.                                                             *)

TypeOK ==
  /\ owner \in [OwnerDomain -> Purses \cup {Client}]
  /\ growth \in Growths
  /\ balance \in [Purses -> 0..MaxBalance]
  /\ room \in Rooms
  /\ listed \subseteq Purses
  /\ tval \in [Purses -> 0..MaxBalance]
  /\ deviations \in 0..MaxDeviations
  /\ pc \in [Rows -> RowPhases]
  /\ firstRow \in Rows \cup {NoName}
  /\ budgetStart \in [Rows -> Positions]
  /\ budgetEnd \in [Rows -> Positions]
  /\ introIdx \in [Rows -> Positions]
  /\ nAtt \in Positions
  /\ commIdx \in [Rows -> Positions]
  /\ \A r \in Rows : tried[r] \subseteq ContsOf(r)
  /\ \A k \in Conts : refusal[k].pos \in Positions
  /\ consulted \subseteq Purses
  /\ restarted \in [Rows -> BOOLEAN]
  /\ phase \in Phases
  /\ verdict \in BOOLEAN

\* v2 section 4.4 item 1: the critical section appends and deducts at once,
\* so the ledger is the fold of the recorded attempts.
LedgerIsFold == live = ListedFold(nAtt)

\* Completeness: replay accepts every Stored row of an honest record.
HonestStoredReplays ==
  (phase = "Done" /\ honest) =>
    \A r \in Rows : commIdx[r] = 0 => RowOk(r)

\* Replay accepts an honest record. Its fold at each budget_start is the
\* snapshot of play, and its final fold is the ledger of play.
PlayReplayAgree ==
  (phase = "Done" /\ honest) =>
    /\ verdict
    /\ \A r \in Rows : snap[r] = ListedFold(budgetStart[r])
    /\ ListedFold(nAtt) = live

\* Soundness: every Stored row of an accepted record is justified at a start
\* that an honest interleaving reaches.
StoredClaimIsHonest ==
  (phase = "Done" /\ verdict) =>
    \A r \in Rows : commIdx[r] = 0 => Justified(r)

\* Soundness: every refusal of an accepted record has a true shortfall.
RefusalNeedsShortfall ==
  (phase = "Done" /\ verdict) =>
    \A k \in Conts :
      refusal[k].named =>
        /\ refusal[k].pick \in PicksOf(k)
        /\ \E x \in Purses :
             FullShare(k, refusal[k].pick)[x] > TrueFold(refusal[k].pos)[x]

=============================================================================
