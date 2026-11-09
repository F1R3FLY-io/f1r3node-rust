---------------------------- MODULE SpaceOwnerRows ----------------------------
\* M4 of plan v4 (decisions 102, 103, 122 and 123, design C4): owner checks
\* in the rows of a Reified space transaction, in play and in replay.
\*
\* Purpose. This module models only the Reified transaction layer. That is
\* the stop rule, CanStop, CanContinue, the units, the mark-free finish, the
\* re-issues, the anchors, the book, the retire group, the seq claims and
\* the adversary. The row internals are the base rules of
\* NativeOwnerRefusal.tla (NOR). This module copies the NOR operators that
\* it needs over its own candidates. Each copy has a "Copied from" note.
\*
\* M4 assumes that Tail is exact (D-OBL-1). Play and replay read the same `TailOf`. T-C4-21 tests D-OBL-1.
\*
\* Terms.
\*   A row is one entry of the journal of a space transaction (a job).
\*   c0 is the candidate that the staging chose for the element row.
\*   Row d is the row at which the commit finds a deviation.
\*   A deviation is a staged fire that fires a later candidate, a staged
\*   fire that finds no candidate, or a refused retire (CR-33).
\*   S_r is the snapshot of the ledger at the start of a row (snap).
\*   fold(n) is the ledger that replay folds from the first n attempts.
\*   A unit is row d and its retire group G(actual). Every other row is a
\*   unit of one row.
\*   The book holds the refused pairs (book) and the item-failed pairs of
\*   the enumeration (ibook).
\*   The enumeration is the item test of every candidate of row d in DR-75
\*   order. It runs no guard (CR-20).
\*
\* Scenario. One collection D holds two candidates c1 < c2 in DR-75 order.
\* For a produce they are the waiters W1 and W2, and x is the incoming
\* datum. For a consume they are the records r1 and r2, and K is the
\* incoming continuation. The incoming x or K is on a client lane, so it
\* has no owner share. Init chooses these values:
\*   - the owner of each candidate in {p, q}
\*   - the guard verdict of each candidate
\*   - the budgets of p and q in 0..MaxBalance
\*   - TailOf in [{c1, c2, none} -> {return, stop}]
\*   - with Order = "ranked", the pattern verdict of c1
\*   - with ClassMode, the owner of the second member of each class
\*   - with Order = "head", the owner of the stored record r
\*   - added by plan v4 errata (V4-18): with Wide and Op = "produce", the
\*     guard verdict of the other tuple of c1
\* With ClassMode, each candidate is a class of two non-persistent members.
\* The fire binds the first member, and one K_retire row retires the second
\* member (CR-13). The racing host row H shares only the ledger. It draws
\* one unit from p when Racer holds.
\* Added by plan v4 errata (V4-18): with Wide, one continuation has two
\* binds and a guard. This is a wide continuation. Its guard runs once on
\* the GREEDY tuple of the window picks of play (decision 37, CR-1). A
\* Stored row does not record those picks.
\*   - Produce: W1 (c1) is a wide waiter. w.grd[c1] is its guard verdict on
\*     the DR-75 tuple, and w.galt[c1] is its verdict on the other tuple.
\*     The staging of play picks one of the two verdicts (staged.gv), and
\*     the commit of play reads the same verdict. A Matched row records the
\*     verdict of the tuple that its COMM names (gk).
\*   - Consume: K is wide. c1 and c2 are two tuples of K, and c1 is the
\*     DR-75 tuple.
\* Replay skips a wide waiter on a Stored row (G2-a, CR-7). Both finishes
\* book the item-failed pairs of a vetoing row only on Stop (G2-b, CR-20,
\* RowDeviation::vetoing). The stop path tests a wide continuation by its
\* pairs and not by its guard (S1, CR-18, risk K18).
\* Changed by decision 124 (L3, X7): the skip and the stop path count no
\* binds, as base E3-a counts none (native_candidate.rs:57-61,
\* selection.rs:613-619 and 747-751). On a Stored row, replay skips every
\* continuation whose commit may veto (MayVetoW, MayVetoK). The stop path
\* counts such a continuation only through a verified refusal or a pair of
\* the enumeration, and it runs no guard (S1, X7, risk K18). Init chooses
\* w.mayVeto, the base commit_may_veto of each waiter of a produce. A
\* continuation whose commit cannot veto has no guard, so its verdict is
\* TRUE, and the Stored check and the stop path test it. A wide waiter may
\* veto, and the receive K of a consume may veto (V4-13). A wide waiter
\* keeps only its second guard verdict (WideW). At a consume Wide has no
\* effect, because K has no second guard verdict and MayVetoK covers K on
\* the S1 stop path. So WideK and the gate row ConsumeWide are retired.
\*
\* Shares. Every COMM of this model binds one datum, so each owner region
\* pays PerRegion(1) = 1 (NOR:353). The item test of one bind equals its
\* full share (NOR:42-44). So only the racer H refuses a tuple whose items
\* pass (V4-11). A machinery search of a K_retire or K_lift row runs only
\* its tuple test (V4-5). So it can meet a refusal without the racer.
\*
\* Play of the element row (CR-17), in literate form:
\*   <stage>  = order := a shuffle of (c1, c2) or the ranked order. Drop a
\*              candidate whose pattern does not bind or whose pair is in
\*              the book. A produce stages the first candidate whose guard
\*              passes. A consume picks the first candidate and runs the
\*              guard once on it. A veto stages a park (vectordb.rs:41).
\*              When TailOf(staged outcome) = stop, the plan stages N.
\*   <start>  = S_r := ledger, budget_start := |attempts|.
\*   <try>    = only for a staged fire (V4-4):
\*     produce: for each candidate in the row order, an item failure at S_r
\*              is a miss, and a guard refusal is a miss. The tuple test
\*              on the live ledger grants, or records a refusal and goes on.
\*     consume: the pick is the first candidate whose items pass at S_r.
\*              A guard veto or a refused tuple test parks K.
\*   <group>  = after a class fire, the K_retire row of the second member.
\*              A refused retire parks inert, and it is a deviation.
\*   <finrow> = no deviation: N commits. A deviation: N drops. A deviating
\*              Stored or Parked row with allowed candidates runs the
\*              enumeration at S_r.
\*   <finish> = a deviation finishes by TailOf(actual). No deviation takes
\*              the plan's own return. Stop adds the re-issue J (anchor
\*              first, CR-19 case 2) while Len(chain) < MaxChain. The book
\*              gets the refusals of the unit and the enumeration. The
\*              enumeration of a Parked receive row is booked only on Stop
\*              (V4-13). Added by plan v4 errata (V4-18): the same holds for
\*              every vetoing Stored or Parked row (VetoingOf).
\*
\* Replay of a record (CR-18), in literate form:
\*   <stage>  = named first (CR-4): the recorded match is first at a random
\*              site. A Stored row names nothing, so it uses DR-75 order.
\*              A ranked site uses the ranked order (named first acts only
\*              inside a tie). The staging rules of play give c0'. A
\*              receive parks without a search on a Stored row, because its
\*              commit may veto (E3-a, CR-7). Added by plan v4 errata
\*              (V4-18): on a Stored row a send skips a wide waiter (G2-a).
\*              Added by plan v4 errata (V4-17): a store or park that E3-a
\*              produced carries its row search, so its refusals verify.
\*              Changed by decision 124 (L3, X7): on a Stored row a send
\*              skips every waiter whose commit may veto (MayVetoW), with no
\*              bind count (base E3-a).
\*   <row>    = every refusal passes its items at fold(budget_start) and
\*              falls short at its position. A match passes its items, its
\*              guard verdict (V4-3) and the ranked prefix (CR-34). A Stored
\*              produce row names each allowed candidate by a refusal or
\*              fails it at the fold. A consume parks by E3-a (CR-7).
\*              Changed by decision 124 (CR-34'): at a ranked site, the
\*              C7.16 replay runs the same search as play over the ranked
\*              order, with the recorded record first inside its score tie,
\*              and the result must equal the row. For the two candidates of
\*              this model, prefixOk gives that verdict.
\*              Changed by decision 124 (L3, X7): a Stored produce row names
\*              each allowed candidate whose commit cannot veto (w.mayVeto)
\*              by a refusal or fails it at the fold. The Stored check skips
\*              every candidate whose commit may veto (base E3-a).
\*   <stop>   = the plan of replay follows c0' (V3-14). It looks up N when
\*              TailOf(c0') = stop. A missing N ends the record, and the
\*              unit needs CanStop(U). At a vetoing site this stop path
\*              evaluates the guard on the binding candidates (CR-18). A
\*              followed N needs CanContinue(U). Added by plan v4 errata
\*              (V4-18): a wide continuation runs no guard on the stop path.
\*              Its pairs decide (S1).
\*              Changed by decision 124 (L3, X7): the stop path runs no
\*              guard. A continuation whose commit may veto is an allowed
\*              candidate only through a verified refusal or a pair of the
\*              enumeration, with no bind count (S1, X7).
\*   <verdict>= the mark is set by an own deviation (actual # c0') or by
\*              the record end. The finish rule of play then gives J.
\*              The record must hold exactly the rows of replay.
\* Replay reads no live ledger. So the model computes replay in one step,
\* as NOR computes its verdict in Ship.
\*
\* The adversary. With Adversary = TRUE and MaxEdits = 1, the producer can
\* make one edit of a kind in Edits to the shipped record:
\*   DropRefusal, AddRefusal (no shortfall), AddItemFailRefusal,
\*   NameOutside (a ranked site, an item failure or a guard refusal),
\*   ForgeStored (a produce site), AddRow (after the unit), DeniedComm (a
\*   granted COMM that the fold denies), CutNoStop (a cut after a unit).
\*
\* Invariants.
\*   TypeOK, LedgerIsFold      the types, and the copied fold (NOR:798)
\*   TxnPlayReplayAgree        replay accepts an honest record, and its book
\*                             writes and re-issues equal those of play
\*   TxnStoredClaimIsHonest    an accepted Stored produce row has a true
\*                             refusal or an item failure for each allowed
\*                             candidate. Changed by plan v4 errata
\*                             (V4-18): for each allowed candidate that is
\*                             not a wide waiter (R15)
\*                             Changed by decision 124 (L3, X7): for each
\*                             allowed candidate whose commit cannot veto
\*                             (w.mayVeto, base E3-a, R15).
\*   TxnRefusalNeedsShortfall  every accepted refusal passes its items and
\*                             falls short at its position
\*   AllowedChoice             an accepted match passes its items, its guard
\*                             verdict and the ranked prefix. Changed by
\*                             plan v4 errata (V4-18): a wide waiter has the
\*                             verdict of its named tuple (gk)
\*                             Changed by decision 124 (CR-34'): at a ranked
\*                             site, the search of play over the ranked
\*                             order, with the recorded record first inside
\*                             its score tie, gives the row. The condition
\*                             is unchanged.
\*   StopIsHonest              an accepted cut after unit U has CanStop(U).
\*                             Changed by plan v4 errata (V4-18): CanStop
\*                             reads AllowedStopTrue (S1, risk K18)
\*   ContinueIsHonest          an accepted edited record follows no row
\*                             after a unit that fails CanContinue(U)
\*   HostParity                no committed row was staged against a view
\*                             that its commit did not meet
\*   RefusedPairNotRefired     no staging proposes a pair from book
\*   ItemFailedPairNotRefired  no staging of play proposes a pair from ibook
\*   OneCounter                the put and the anchor draw on the payer only
\*   NoLostRecord              a lifted record is restored or re-offered
\*   RefusedTupleKeepsMembers  a refused class fire leaves every member
\*   SpentClassRetired         a class fire retires the second member, or
\*                             its refused retire parks inert
\*   SeqClaimsAgree            play and replay store the claims of the
\*                             committed rows
\* Witnesses: NoAlternativeFire, NoRandomPickCompletion, NoStopReissue and
\* NoRankedAlternative. Each witness config must violate its property.
\* Added by plan v4 errata (V4-18): the witnesses NoWideStoredDeviation (H1)
\* and NoWideOrderStore (H2).
\*
\* Defect selects one negative control:
\*   MarkedFinish        a marked receipt always stops -> TxnPlayReplayAgree
\*   GuardedSearch       the Stored check searches a guarded receive
\*                       -> TxnPlayReplayAgree
\*   SearchBook          play books only the pairs that its search met
\*                       -> TxnPlayReplayAgree
\*   RowStop             replay checks single rows -> TxnPlayReplayAgree
\*   UncheckedStored     replay skips the Stored check
\*                       -> TxnStoredClaimIsHonest
\*   UncheckedRefusal    replay skips the shortfall check
\*                       -> TxnRefusalNeedsShortfall
\*   UncheckedPrefix     replay skips the ranked prefix -> AllowedChoice
\*                       Changed by decision 124 (CR-34'): replay skips the
\*                       check that the search of play over the ranked
\*                       order, with the recorded record first inside its
\*                       score tie, gives the row (prefixOk).
\*   VetoExcuse          a guard refusal excuses a better candidate
\*                       -> AllowedChoice
\*   UncheckedGuard      replay skips the guard verdict of the match
\*                       -> AllowedChoice
\*   UncheckedStop       replay skips CanStop(U) -> StopIsHonest
\*   UncheckedContinue   replay skips CanContinue(U) -> ContinueIsHonest
\*   ContinueAfterDeviation  the Head rows after d commit in the shared
\*                       commit loop of both roles -> HostParity. Changed
\*                       by plan v4 errata (V4-14).
\*   NoBook              the settle ignores book -> RefusedPairNotRefired
\*   CascadeIgnoresBook  the cascade ignores book -> RefusedPairNotRefired
\*   NoItemBook          the stagings ignore ibook
\*                       -> ItemFailedPairNotRefired
\*   OwnerPaidPut        the put draws on p -> OneCounter
\*   PlainLift           the lift is not a peek -> NoLostRecord
\*   PartialClassFire    a refused class fire removes the first member
\*                       -> RefusedTupleKeepsMembers
\*   StagedRetires       the v2 rule: the rows after d drop, and the group
\*                       of the actual fire is not readied
\*                       -> SpentClassRetired
\*   StagedClaims        the v2 CR-24 rule: claims of the staged rows
\*                       -> SeqClaimsAgree
\*   ReplayBookOnMark    replay books its refused pairs only on a marked
\*                       row, through the deviation of its receipt
\*                       -> TxnPlayReplayAgree. Added by plan v4 errata
\*                       (V4-17).
\*   ParkWithoutSearch   a store or park that E3-a produced in replay has
\*                       no row search, so replay cannot verify a recorded
\*                       refusal -> TxnPlayReplayAgree. Added by plan v4
\*                       errata (V4-17 (iv)).
\*   WideSendSearch      replay searches a wide waiter on a Stored row
\*                       (no G2-a) -> TxnPlayReplayAgree. Added by plan v4
\*                       errata (V4-18).
\*                       Changed by decision 124 (L3, X7): replay searches
\*                       every waiter whose commit may veto on a Stored row
\*                       (no E3-a skip).
\*   WideItemBookOnReturn  a vetoing send row books its item-failed pairs
\*                       on Return too (no G2-b) -> TxnPlayReplayAgree.
\*                       Added by plan v4 errata (V4-18).
\*                       Added by decision 124 (L3, X7): the switch acts on
\*                       every send row with a waiter whose commit may veto
\*                       (X7). The name stays, because the cfgs and the gate
\*                       rows use it.
\*   WideStopByGuard     the stop path runs the guard of a wide
\*                       continuation on the DR-75 tuple (no S1)
\*                       -> TxnPlayReplayAgree. Added by plan v4 errata
\*                       (V4-18).
\*                       Changed by decision 124 (L3, X7): the stop path
\*                       runs the guard of every continuation whose commit
\*                       may veto (no S1).
\*
\* Rust map (plan v4 M4):
\*   Stage           generic.rs run_plan and run_with, with the discipline
\*                   stagings
\*   TryCandidate    space_matcher.rs first_match and consume_match, and
\*                   ops_produce.rs run_matcher, over RowCandidates in
\*                   space_play.rs apply_row
\*   enumeration     RowSearch::item_failures in both apply_row
\*                   implementations (FinishRow and ReplayVerdict)
\*   FinishRow and the stop checks  ssh.rs commit_staged and
\*                   commit_operation (section 2.10)
\*   the replay row  the generic selection.rs functions and
\*                   verify_row_refusals through ns/space.rs. The ranked
\*                   prefix is RowSearch::ranked_prefix with the item test
\*                   of the owner filter
\*                   Changed by decision 124 (CR-34'): the trait call
\*                   RowSearch::ranked_prefix retires. At a ranked site,
\*                   the C7.16 replay runs the same search as play over the
\*                   ranked order, with the recorded record first inside
\*                   its score tie, and the result must equal the row.
\*   Finish          generic.rs outcome and the Tail functions
\* Added by plan v4 errata (V4-17, V4-18):
\*   Wide            a join waiter or a receive of two or more binds with a
\*                   guard or a theory. GREEDY gives its tuple from the
\*                   window picks (join.rs:64-75), and admits runs the guard
\*                   once
\*   staged.gv, gk   the guard verdict of the tuple of play, and of the
\*                   tuple that the COMM of a Matched row names
\*   rskip           G2-a in join.rs Engine::offer: Cx::recorded_stored and
\*                   Cx::waiter_may_veto (matching::waiter_may_veto, C4.1,
\*                   C5.6). The skip is a refusal of the waiter
\*                   Changed by decision 124 (L3, X7): base E3-a with no
\*                   bind count. On a recorded Stored row Engine::offer
\*                   skips every waiter whose commit may veto
\*                   (Cx::recorded_stored and Cx::may_veto at
\*                   generic.rs:965-969). waiter_may_veto retires (C4.1).
\*   MayVetoW, MayVetoK  added by decision 124 (L3, X7): the base
\*                   commit_may_veto (match.rs:135-140) and Cx::may_veto
\*   StoredRuleOk    R15: RowSearch::may_veto through the shared rule
\*   VetoingOf       RowDeviation::vetoing and AppliedRow::vetoing, which
\*                   apply_row sets from RowSearch::may_veto (C7.15, C7.16)
\*   S1              CanStop(d) in ns/space.rs (C7.16): byPairs reads the
\*                   verified refusals and RowSearch::item_failures
\*   refBook         SpaceTxnReceipt::refused (C3d, C3g, C7.8)
\*   ParkWithoutSearch  the row search of an E3-a store or park, which
\*                   verify_row_refusals reads (ns/operations.rs, C7.9,
\*                   C7.16)
\*
\* Scope.
\*   - The rows of N, the anchor A, the take T and the put P are machinery
\*     rows. They are never refused (CR-15).
\*   - The incoming datum and the introductions are client-paid. So R8
\*     never skips a row, and no row is Skipped.
\*   - Every consume site has a guard, so it may veto (E3-a, R15).
\*   - The model has no owner table. Replay reads the budgets of Init, so
\*     the unlisted rule of NOR gives the empty set.
\*   - The model has no outcome fit check (ns/space.rs outcome_fits). The
\*     selection decides the actual outcome (CR-18).
\*   - In the random and ranked configs, the re-issue J is the anchor and N.
\*     N stands for the next staging step of the plan. Its continuation
\*     returns.
\*   - The adversary edits the element row of job 1, the rows after it and
\*     the attempts of the record.
\*   - The Head config is deterministic, so its replay reads the staged
\*     candidate of each journal row.
\*   - The module does not extend Sequences, because the plan names a
\*     constant Seq. It defines Len, Append and the other helpers below.
\*   - The fold unrolls over four attempt slots. Every attempt draws at
\*     least one unit, and the budgets sum to at most four.
\* -----------------------------------------------------------------------------
EXTENDS Integers, FiniteSets

CONSTANTS
  Defect,
  Order,
  Op,
  ClassMode,
  MaxBalance,
  MaxChain,
  Adversary,
  MaxEdits,
  Edits,
  Seq,
  \* Added by plan v4 errata (V4-18): Wide selects a continuation of two
  \* binds with a guard (see Scenario).
  Wide,
  Racer

EditKinds ==
  {"DropRefusal", "AddRefusal", "AddItemFailRefusal", "NameOutside",
   "ForgeStored", "AddRow", "DeniedComm", "CutNoStop"}

ASSUME Defect \in {
  "None",
  "MarkedFinish",
  "GuardedSearch",
  "SearchBook",
  "RowStop",
  "UncheckedStored",
  "UncheckedRefusal",
  "UncheckedPrefix",
  "VetoExcuse",
  "UncheckedStop",
  "UncheckedContinue",
  "ContinueAfterDeviation",
  "NoBook",
  "CascadeIgnoresBook",
  "NoItemBook",
  "OwnerPaidPut",
  "PlainLift",
  "PartialClassFire",
  "StagedRetires",
  "StagedClaims",
  "UncheckedGuard",
  \* Added by plan v4 errata (V4-17).
  "ParkWithoutSearch",
  \* Added by plan v4 errata (V4-18).
  \* Added by decision 124 (L3, X7): these three switches act on every
  \* candidate whose commit may veto (X7). They keep the name Wide, because
  \* the cfgs and the gate rows use them.
  "WideSendSearch",
  "WideItemBookOnReturn",
  "WideStopByGuard",
  \* Added by plan v4 errata (V4-17).
  "ReplayBookOnMark"
}

ASSUME /\ Order \in {"random", "ranked", "head"}
       /\ Op \in {"produce", "consume"}
       /\ ClassMode \in BOOLEAN
       /\ MaxBalance \in 0..2
       /\ MaxChain \in 1..3
       /\ Adversary \in BOOLEAN
       /\ MaxEdits \in 0..1
       /\ Edits \subseteq EditKinds
       /\ Seq \in BOOLEAN
       /\ Racer \in BOOLEAN
       /\ ClassMode => Op = "consume" /\ Order = "random" /\ ~Adversary
       /\ Order = "ranked" => Op = "consume"
       /\ Order = "head" => Op = "produce" /\ ~Adversary /\ ~Racer

\* Added by plan v4 errata (V4-18): a wide continuation has random window
\* picks, so it needs a random site. A class fire binds one datum.
ASSUME Wide \in BOOLEAN /\ (Wide => Order = "random" /\ ~ClassMode)

-----------------------------------------------------------------------------
(* Sets and sequence helpers.                                              *)

Purses == {"p", "q"}

Cands == {"c1", "c2"}

None == "none"

Outs == Cands \cup {None}

\* The pair ids of the book. "c1" and "c2" pair the incoming x or K with a
\* candidate. "m1" and "m2" pair the second member of a class with its
\* K_retire. "r1" and "r2" pair the stored record r with a waiter.
PairIds == {"c1", "c2", "m1", "m2", "r1", "r2"}

Kinds == {"E", "G", "N", "A", "T", "P", "L", "R", "F"}

MaxAtt == 4

Positions == 0..MaxAtt

MaxJobs == MaxChain + 1

Phases == {"stage", "start", "try", "group", "finrow", "finish", "reissue",
           "hstage", "hrow", "hfinish", "cascade", "ship", "edit", "end"}

Other(c) == IF c = "c1" THEN "c2" ELSE "c1"

Zero == [x \in Purses |-> 0]

One(x) == [y \in Purses |-> IF y = x THEN 1 ELSE 0]

Plus(a, b) == [x \in Purses |-> a[x] + b[x]]

Minus(a, b) == [x \in Purses |-> a[x] - b[x]]

PairOf(kind, c) ==
  IF kind = "G" THEN (IF c = "c1" THEN "m1" ELSE "m2")
  ELSE IF kind \in {"L", "R"} THEN (IF c = "c1" THEN "r1" ELSE "r2")
  ELSE c

Len(s) == Cardinality(DOMAIN s)

Append(s, e) == [i \in 1..(Len(s) + 1) |-> IF i <= Len(s) THEN s[i] ELSE e]

Concat(s, t) ==
  [i \in 1..(Len(s) + Len(t)) |-> IF i <= Len(s) THEN s[i] ELSE t[i - Len(s)]]

InsertAt(s, k, e) ==
  [i \in 1..(Len(s) + 1) |->
     IF i < k THEN s[i] ELSE IF i = k THEN e ELSE s[i - 1]]

RemoveAt(s, k) ==
  [i \in 1..(Len(s) - 1) |-> IF i < k THEN s[i] ELSE s[i + 1]]

Range(s) == {s[i] : i \in DOMAIN s}

\* Added by plan v4 errata (V4-18): the sequence s without the members of
\* the set S. Every sequence of this module has at most two elements.
Without(s, S) ==
  IF Len(s) = 2
  THEN IF s[1] \in S /\ s[2] \in S THEN <<>>
       ELSE IF s[1] \in S THEN <<s[2]>>
       ELSE IF s[2] \in S THEN <<s[1]>>
       ELSE s
  ELSE IF Len(s) = 1 /\ s[1] \in S THEN <<>> ELSE s

\* Added by plan v4 errata (V4-18): the wide waiters of a produce, and the
\* wide receive K of a consume. A wide continuation has two or more binds,
\* and its commit may veto (a guard or a theory, R15). With Wide = FALSE,
\* WideW is empty and WideK is FALSE.
\* Changed by decision 124 (L3, X7): WideW holds the wide waiters of a
\* produce. With Wide = FALSE, WideW is empty. No formula reads WideK, so it
\* is retired (below), and Wide has no effect at a consume.
WideW == IF Wide /\ Op = "produce" THEN {"c1"} ELSE {}

\* Retired by decision 124 (L3, X7): no formula reads WideK. Its three uses
\* (wideTest, stopGuard and AllowedStopTrue) were the S1 stop path, and
\* MayVetoK replaced each of them.

\* Added by decision 124 (L3, X7): the candidates of a produce whose commit
\* may veto (MayVetoW), and the flag that the receive K of a consume may
\* veto (MayVetoK). They are the model form of the base commit_may_veto (a
\* guard that is present and not empty, match.rs:135-140) and of
\* Cx::may_veto (a guard or a theory, generic.rs:965-969). w.mayVeto[c] is
\* the base predicate of candidate c, and Init chooses it. A continuation
\* whose commit cannot veto has no guard and no theory, so its verdict is
\* TRUE. A wide waiter has a guard, so its commit may veto. At a consume,
\* w.mayVeto has no use, so Init sets it TRUE. Every receive of this model
\* may veto (see Scope and V4-13), so MayVetoK holds at every consume. The
\* E3-a rule (VetoingOf, StoredRuleOk, rskip and TxnStoredClaimIsHonest)
\* and the S1 rule (wideTest, stopGuard and AllowedStopTrue) read them,
\* with no bind count (base E3-a at native_candidate.rs:57-61 and
\* selection.rs:613-619 and 747-751). WideW keeps its second job only: it
\* marks the waiter of two binds that has a second guard verdict (Init,
\* Verdicts, rv, AllowedChoice, NoWideStoredDeviation and
\* NoWideOrderStore). WideK had no second job, so it is retired (above).
\* MayVetoW reads w, which VARIABLES declares below, so its definition
\* follows `world`.

MayVetoK == Op = "consume"

\* Added by plan v4 errata (V4-18): the vetoing flag of a row with the
\* candidates cs (RowDeviation::vetoing, CR-20 item 2). apply_row computes
\* it in both roles over every candidate of the row search. Every receive of
\* this model may veto (V4-13). A send row is vetoing when a wide waiter is
\* a candidate of the row. Control "WideItemBookOnReturn" drops the send
\* part. Arbiter A names this flag e3a (E3aOf).
\* Changed by decision 124 (L3, X7): a send row is vetoing when a waiter
\* whose commit may veto (MayVetoW) is a candidate of the row, with no bind
\* count (base E3-a). VetoingOf reads MayVetoW, which reads w. VARIABLES
\* declares w below, so the definition follows `world`.

\* The world values that Init chooses. They do not change after Init.
\* Changed by plan v4 errata (V4-18): WorldSet gains galt, the guard verdict
\* of the other tuple of a wide waiter. Init sets galt = grd for every other
\* candidate.
\* Changed by decision 124 (L3, X7): WorldSet gains mayVeto, the base
\* commit_may_veto of each candidate (MayVetoW). Init constrains it.
WorldSet ==
  [own: [Cands -> Purses], own2: [Cands -> Purses], grd: [Cands -> BOOLEAN],
   bud: [Purses -> 0..MaxBalance], tail: [Outs -> {"return", "stop"}],
   pat: BOOLEAN, ownr: Purses, galt: [Cands -> BOOLEAN],
   mayVeto: [Cands -> BOOLEAN]]

\* Changed by plan v4 errata (V4-18): a row gains gk, the guard verdict of
\* the tuple that its COMM names. TryCandidate sets it on the element row.
\* Every other row keeps TRUE.
MkRow(j, k, c, s, rf, st, cm, sl, dr) ==
  [job |-> j, kind |-> k, cand |-> c, staged |-> s, refs |-> rf,
   start |-> st, comm |-> cm, stale |-> sl, draw |-> dr, gk |-> TRUE]

\* Changed by plan v4 errata (V4-18): every staging record has the field gv,
\* the guard verdicts of the window picks of play. The empty staging takes
\* them as the parameter g, because w is declared after this definition.
NoStaging(g) ==
  [ord |-> <<>>, c0 |-> None, plan |-> <<>>, next |-> 1, n |-> FALSE,
   props |-> {}, fresh |-> FALSE, gv |-> g]

NoDev ==
  [on |-> FALSE, kind |-> None, refused |-> {}, ifail |-> {}, met |-> {}]

NoReplay ==
  [done |-> FALSE, verdict |-> FALSE, book |-> {}, ibook |-> {},
   reissues |-> 0, claims |-> {}, c0 |-> None, ended |-> FALSE,
   followed |-> FALSE, cont |-> TRUE]

VARIABLES
  \* The world record of Init (constant after Init).
  w,
  \* The live owner ledger of play.
  ledger,
  \* The granted owner draws of play, in append order (the fold record).
  attempts,
  \* S_r and budget_start of the current row.
  snap,
  \* The stored candidates, second members, inert parks, x or K, and r.
  store,
  \* The staging of the current job.
  staged,
  \* The rows that play committed, in commit order.
  prow,
  \* The deviation of the current job and its receipt pairs.
  dev,
  \* The refused pairs and the item-failed pairs of play.
  book,
  ibook,
  \* The re-issues. Each entry is TRUE when the re-issue stages its anchor
  \* first.
  chain,
  \* The seq claims that play stores.
  pclaims,
  \* The shipped record: its rows and its attempts.
  journal,
  \* The number of adversary edits.
  edits,
  \* The replay result.
  rp,
  \* H drew or met its refusal.
  raced,
  \* The phase of play, then of the adversary and replay.
  pc,
  \* The current job: 1 is the program operation, and later jobs are
  \* re-issues.
  job

world == <<w>>

\* Added by decision 124 (L3, X7): MayVetoW and VetoingOf read w, so their
\* definitions follow VARIABLES. VetoingOf moved here from its place after
\* MayVetoK. The note of MayVetoW is before MayVetoK, and the notes of
\* VetoingOf are after it.
MayVetoW == IF Op = "produce" THEN {c \in Cands : w.mayVeto[c]} ELSE {}

VetoingOf(cs) ==
  \/ Op = "consume"
  \* Changed by decision 124 (L3, X7): every waiter whose commit may veto.
  \* Control "WideItemBookOnReturn" acts on each of them (X7).
  \/ Defect # "WideItemBookOnReturn" /\ MayVetoW \cap Range(cs) # {}

vars == <<w, ledger, attempts, snap, store, staged, prow, dev, book, ibook,
          chain, pclaims, journal, edits, rp, raced, pc, job>>

\* A record is honest when the producer made no edit.
honest == edits = 0

\* The adversary can make one more edit of the given kind.
CanEdit(kind) == Adversary /\ edits < MaxEdits /\ kind \in Edits

-----------------------------------------------------------------------------
(* Shares and item tests.                                                  *)

\* Copied from NativeOwnerRefusal.tla:373-376 (FitsIn).
FitsIn(share, budget, free) ==
  \A x \in Purses : x \in free \/ share[x] <= budget[x]

\* Copied from NativeOwnerRefusal.tla:378-379 (PursesOf).
PursesOf(share) == {x \in Purses : share[x] > 0}

\* Copied from NativeOwnerRefusal.tla:494-497 (UnlistedPasses). M4 has no
\* owner table. No purse is unlisted, so the set is empty.
UnlistedPasses == {}

\* The share of the COMM of candidate c. A class fire binds the first
\* member, and the owner of the class owns that member (CR-13).
ShareOf(c) == One(w.own[c])

\* The share of the K_retire COMM of the second member of class c. The
\* K_retire is sealed to the payer, so only the member pays (CR-15).
RetireShare(c) == One(w.own2[c])

\* The share of the lift K_lift of r (a peek, D.3).
LiftShare == One(w.ownr)

\* The share of the re-offer of r to the waiter c.
ReofferShare(c) == Plus(One(w.own[c]), One(w.ownr))

\* Copied from NativeOwnerRefusal.tla:406-408 (ItemFundable) and 412-414
\* (TupleItemsPass) as ItemPasses(c, S). The incoming datum or
\* continuation is on a client lane, so its own item test always passes.
\* A class passes only when every member passes at S (CR-13).
ItemPasses(c, S) ==
  /\ FitsIn(ShareOf(c), S, UnlistedPasses)
  /\ (ClassMode => FitsIn(RetireShare(c), S, UnlistedPasses))

\* Copied from NativeOwnerRefusal.tla:385-401 (GreedyMatch) as TupleOf(c).
\* One bind takes the incoming value and c. So the greedy pass has one
\* tuple, and the tuple is found when its items pass at S.
TupleOf(c, S) ==
  [found |-> ItemPasses(c, S), pick |-> c, seen |-> PursesOf(ShareOf(c))]

\* The pattern of K does not bind c1 when the ranked pattern verdict is
\* FALSE (V3-13). Every other pattern binds.
PatternBinds(c) == c = "c2" \/ w.pat

-----------------------------------------------------------------------------
(* The fold of the recorded attempts.                                      *)

\* Copied from NativeOwnerRefusal.tla:422-440 (AttemptDraw, Drawn,
\* FoldFrom and ListedFold) as FoldOf. The budgets of Init take the place
\* of the listed budgets. Every attempt is a granted COMM.
AttemptDraw(att, j, n, x) ==
  IF j <= n /\ j <= Len(att) THEN att[j][x] ELSE 0

Drawn(att, n, x) ==
  AttemptDraw(att, 1, n, x) + AttemptDraw(att, 2, n, x)
    + AttemptDraw(att, 3, n, x) + AttemptDraw(att, 4, n, x)

FoldOf(att, n) == [x \in Purses |-> w.bud[x] - Drawn(att, n, x)]

\* Copied from NativeOwnerRefusal.tla:470-473 (FoldOk). Every granted COMM
\* fits the fold of the attempts before it.
FoldOk(att) ==
  \A j \in 1..MaxAtt :
    j <= Len(att) => FitsIn(att[j], FoldOf(att, j - 1), {})

\* Copied from NativeOwnerRefusal.tla:482-489 (RefusalOk). The caller gives
\* the item test at fold(budget_start) (F1-a). A machinery row has no item
\* test, so its caller gives TRUE (V4-5). Control "UncheckedRefusal" skips
\* the shortfall.
RefusalOk(att, itemsPass, pos, share) ==
  /\ itemsPass
  /\ (Defect # "UncheckedRefusal" =>
        \E x \in Purses : share[x] > FoldOf(att, pos)[x])

\* Copied from NativeOwnerRefusal.tla:502-505 (StoredRuleOk). Each allowed
\* candidate of a Stored row has a named refusal, or it fails its items at
\* fold(budget_start). The exact search of a send tests the guard, so a
\* candidate whose guard refuses is not allowed (CR-7).
\* Changed by plan v4 errata (V4-18): the Stored check skips a wide waiter
\* (R15, base E3-a at selection.rs:749-751). Its verdict depends on the
\* window picks of play, and the row does not record them.
\* Changed by decision 124 (L3, X7): the Stored check skips every candidate
\* whose commit may veto (MayVetoW), with no bind count (R15, base E3-a at
\* native_candidate.rs:57-61 and selection.rs:613-619 and 747-751). The
\* window picks of play are not the reason. The check tests every candidate
\* whose commit cannot veto (w.mayVeto).
StoredRuleOk(att, e, cs) ==
  \* Changed by decision 124 (L3, X7): every candidate whose commit may
  \* veto.
  \A c \in Range(cs) \ MayVetoW :
    (w.grd[c] /\ ~\E r \in e.refs : r.c = c) =>
      ~ItemPasses(c, FoldOf(att, e.start))

-----------------------------------------------------------------------------
(* Staging.                                                                *)

\* The orders of the staging: a shuffle at a random site, and the score
\* order at a ranked site.
Orders ==
  IF Order = "random"
  THEN {<<"c1", "c2">>, <<"c2", "c1">>}
  ELSE {<<"c1", "c2">>}

\* A candidate of the row: its pattern binds, and its pair is not in the
\* book that the staging reads (CR-20).
Keep(c, bk) == PatternBinds(c) /\ PairOf("E", c) \notin bk

Filtered(o, bk) ==
  IF Keep(o[1], bk) /\ Keep(o[2], bk) THEN o
  ELSE IF Keep(o[1], bk) THEN <<o[1]>>
  ELSE IF Keep(o[2], bk) THEN <<o[2]>>
  ELSE <<>>

\* The staged candidate c0. A produce stages the first candidate whose
\* guard passes. A consume picks the first candidate and runs its guard
\* once. A veto stages a park (vectordb.rs:41, ops_consume.rs:202-210).
\* Changed by plan v4 errata (V4-18): StagePickBy reads the guard verdicts g
\* in place of w.grd. StagePick(cs) is StagePickBy(cs, w.grd).
StagePickBy(cs, g) ==
  IF Op = "produce"
  THEN IF Len(cs) >= 1 /\ g[cs[1]] THEN cs[1]
       ELSE IF Len(cs) = 2 /\ g[cs[2]] THEN cs[2]
       ELSE None
  ELSE IF Len(cs) >= 1 /\ g[cs[1]] THEN cs[1] ELSE None

StagePick(cs) == StagePickBy(cs, w.grd)

\* Added by plan v4 errata (V4-18): the guard verdicts that the window picks
\* of play can give. A wide waiter c has the verdict w.grd[c] on the DR-75
\* tuple or w.galt[c] on the other tuple. Every other candidate has one
\* verdict, because Init sets w.galt[c] = w.grd[c]. With Wide = FALSE,
\* Verdicts = {w.grd}.
Verdicts ==
  {g \in [Cands -> BOOLEAN] : \A c \in Cands : g[c] \in {w.grd[c], w.galt[c]}}

-----------------------------------------------------------------------------
(* Init.                                                                   *)

Init ==
  /\ w \in WorldSet
  /\ ~ClassMode => w.own2 = [c \in Cands |-> "p"]
  /\ Order # "ranked" => w.pat
  /\ Order # "head" => w.ownr = "p"
  \* Added by plan v4 errata (V4-18): only a wide waiter has a second
  \* verdict.
  /\ \A c \in Cands : c \notin WideW => w.galt[c] = w.grd[c]
  \* Added by decision 124 (L3, X7): a continuation whose commit cannot veto
  \* has no guard, so its verdict is TRUE. A wide waiter has a guard, so its
  \* commit may veto. At a consume, mayVeto has no use (MayVetoW = {}). A
  \* FALSE value only forces grd[c] = TRUE, and the TRUE world already holds
  \* that verdict. So the third conjunct removes repeated worlds and no
  \* behavior.
  /\ \A c \in Cands : ~w.mayVeto[c] => w.grd[c]
  /\ \A c \in WideW : w.mayVeto[c]
  /\ Op = "consume" => \A c \in Cands : w.mayVeto[c]
  /\ ledger = w.bud
  /\ attempts = <<>>
  /\ snap = [v |-> w.bud, at |-> 0]
  /\ store = [cand |-> Cands,
              mem2 |-> IF ClassMode THEN Cands ELSE {},
              inert |-> {},
              inc |-> "flight",
              r |-> IF Order = "head" THEN "stored" ELSE "gone"]
  \* Changed by plan v4 errata (V4-18): the empty staging has gv = w.grd.
  /\ staged = NoStaging(w.grd)
  /\ prow = <<>>
  /\ dev = NoDev
  /\ book = {}
  /\ ibook = {}
  /\ chain = <<>>
  /\ pclaims = {}
  /\ journal = [rows |-> <<>>, att |-> <<>>]
  /\ edits = 0
  /\ rp = NoReplay
  /\ raced = FALSE
  /\ pc = IF Order = "head" THEN "hstage" ELSE "stage"
  /\ job = 1

-----------------------------------------------------------------------------
(* Play of the random and ranked configs.                                  *)

\* Stage: the row order and c0. The plan stages N when TailOf(staged
\* outcome) = stop (D-OBL-1).
Stage ==
  /\ pc = "stage"
  \* Changed by plan v4 errata (V4-18): play also picks the guard verdicts g
  \* of its window picks (decision 37, CR-1). The staging and the commit of
  \* play read g, and the staging record keeps it as gv.
  /\ \E o \in Orders, g \in Verdicts :
       LET cs == Filtered(o, book \cup ibook)
           c0 == StagePickBy(cs, g)
       IN staged' = [ord |-> cs, c0 |-> c0, plan |-> <<>>, next |-> 1,
                     n |-> w.tail[c0] = "stop",
                     props |-> IF c0 = None THEN {} ELSE {PairOf("E", c0)},
                     fresh |-> TRUE, gv |-> g]
  /\ pc' = "start"
  /\ UNCHANGED <<w, ledger, attempts, snap, store, prow, dev, book, ibook,
                 chain, pclaims, journal, edits, rp, raced, job>>

\* StartRow: S_r and budget_start in one critical section.
StartRow ==
  /\ pc = "start"
  /\ snap' = [v |-> ledger, at |-> Len(attempts)]
  /\ pc' = "try"
  /\ UNCHANGED <<w, ledger, attempts, store, staged, prow, dev, book, ibook,
                 chain, pclaims, journal, edits, rp, raced, job>>

\* The test of one produce candidate (space_matcher.rs:307-318).
ProduceStep(c) ==
  IF ~TupleOf(c, snap.v).found THEN "itemfail"
  \* Changed by plan v4 errata (V4-18): the commit runs the guard on the
  \* tuple of the window picks of play, so it reads the staged verdict.
  ELSE IF ~staged.gv[c] THEN "veto"
  ELSE IF FitsIn(ShareOf(c), ledger, {}) THEN "fire"
  ELSE "refuse"

\* The produce loop over the row order. A refusal records its position
\* and the search goes on.
ProduceRow ==
  LET cs == staged.ord
      s1 == ProduceStep(cs[1])
      s2 == IF Len(cs) = 2 THEN ProduceStep(cs[2]) ELSE "absent"
      a == IF s1 = "fire" THEN cs[1] ELSE IF s2 = "fire" THEN cs[2] ELSE None
      p == Len(attempts)
  IN [act |-> a,
      refs |-> (IF s1 = "refuse" THEN {[c |-> cs[1], pos |-> p]} ELSE {})
                 \cup (IF s1 # "fire" /\ s2 = "refuse"
                       THEN {[c |-> cs[2], pos |-> p]} ELSE {}),
      met |-> {cs[1]} \cup (IF s1 # "fire" /\ Len(cs) = 2 THEN {cs[2]} ELSE {})]

\* The consume: one greedy pick, one guard run and one tuple test. A veto
\* or a refusal parks K (ops_consume.rs:156-166, 202-210).
ConsumeRow ==
  LET cs == staged.ord
      i1 == TupleOf(cs[1], snap.v).found
      i2 == Len(cs) = 2 /\ TupleOf(cs[2], snap.v).found
      pick == IF i1 THEN cs[1] ELSE IF i2 THEN cs[2] ELSE None
      p == Len(attempts)
      fires == pick # None /\ w.grd[pick] /\ FitsIn(ShareOf(pick), ledger, {})
      refused == pick # None /\ w.grd[pick] /\ ~fires
  IN [act |-> IF fires THEN pick ELSE None,
      refs |-> IF refused THEN {[c |-> pick, pos |-> p]} ELSE {},
      met |-> {cs[1]} \cup (IF ~i1 /\ Len(cs) = 2 THEN {cs[2]} ELSE {})]

\* TryCandidate runs only for a staged fire (V4-4). A staged store or park
\* commits as staged. Control "PartialClassFire" removes the first member
\* of a refused class.
TryCandidate ==
  /\ pc = "try"
  /\ LET res == IF staged.c0 = None
                THEN [act |-> None, refs |-> {}, met |-> {}]
                ELSE IF Op = "produce" THEN ProduceRow ELSE ConsumeRow
         a == res.act
         fire == a \in Cands
         dr == IF fire THEN ShareOf(a) ELSE Zero
         pfire == Defect = "PartialClassFire" /\ ClassMode /\ res.refs # {}
     IN \* Changed by plan v4 errata (V4-18): the row records gk, the guard
        \* verdict of the tuple that its COMM names. A row that does not
        \* fire has gk = FALSE.
        /\ prow' = Append(prow,
                          [MkRow(job, "E", a, staged.c0, res.refs, snap.at,
                                 IF fire THEN Len(attempts) + 1 ELSE 0,
                                 FALSE, dr)
                           EXCEPT !.gk = IF fire THEN staged.gv[a] ELSE FALSE])
        /\ attempts' = IF fire THEN Append(attempts, dr) ELSE attempts
        /\ ledger' = Minus(ledger, dr)
        /\ store' = [store EXCEPT
                       !.cand = IF fire THEN @ \ {a}
                                ELSE IF pfire
                                     THEN @ \ {r.c : r \in res.refs}
                                     ELSE @,
                       !.inc = IF fire THEN "gone" ELSE "stored"]
        /\ dev' = [on |-> a # staged.c0, kind |-> "E",
                   refused |-> {PairOf("E", r.c) : r \in res.refs},
                   ifail |-> {}, met |-> res.met]
        /\ pc' = IF ClassMode /\ fire THEN "group" ELSE "finrow"
  /\ UNCHANGED <<w, snap, staged, book, ibook, chain, pclaims, journal, edits,
                 rp, raced, job>>

\* The retire group G(actual) of a class fire (CR-33). Its machinery search
\* runs only the tuple test (V4-5). A refused retire parks inert, and it is
\* a deviation. Control "StagedRetires" drops the group when row d deviates.
RetireRow ==
  /\ pc = "group"
  /\ LET a == prow[Len(prow)].cand
         sh == RetireShare(a)
         ok == FitsIn(sh, ledger, {})
         p == Len(attempts)
     IN IF Defect = "StagedRetires" /\ a # staged.c0
        THEN /\ pc' = "finrow"
             /\ UNCHANGED <<prow, attempts, ledger, store, dev>>
        ELSE /\ prow' = Append(prow,
                          MkRow(job, "G", IF ok THEN a ELSE None, a,
                                IF ok THEN {} ELSE {[c |-> a, pos |-> p]},
                                p, IF ok THEN p + 1 ELSE 0, FALSE,
                                IF ok THEN sh ELSE Zero))
             /\ attempts' = IF ok THEN Append(attempts, sh) ELSE attempts
             /\ ledger' = IF ok THEN Minus(ledger, sh) ELSE ledger
             /\ store' = IF ok THEN [store EXCEPT !.mem2 = @ \ {a}]
                         ELSE [store EXCEPT !.inert = @ \cup {a}]
             /\ dev' = IF ok THEN dev
                       ELSE [dev EXCEPT !.on = TRUE,
                                        !.refused = @ \cup {PairOf("G", a)}]
             /\ pc' = "finrow"
  /\ UNCHANGED <<w, snap, staged, book, ibook, chain, pclaims, journal, edits,
                 rp, raced, job>>

\* FinishRow: no deviation commits N. A deviation drops N. A deviating
\* Stored or Parked row with allowed candidates runs the enumeration at S_r
\* over every candidate of the row, with no guard (CR-20). Control
\* "SearchBook" keeps only the pairs that the search met. Changed by plan
\* v4 errata (V4-14): control "ContinueAfterDeviation" acts in HCommit, the
\* shared commit loop of the Head config, and not here. In a random config
\* replay follows c0' (V3-14, B4), so it cannot stage the rows of play after
\* row d, and the control would refute more than HostParity.
FinishRow ==
  /\ pc = "finrow"
  /\ LET e == prow[1]
         failed == {c \in Range(staged.ord) : ~ItemPasses(c, snap.v)}
         seen == IF Defect = "SearchBook" THEN failed \cap dev.met ELSE failed
         enumOn == dev.on /\ e.cand = None /\ staged.c0 # None
         \* Changed by plan v4 errata (V4-14): the control acts in HCommit.
         addN == staged.n /\ ~dev.on
     IN /\ prow' = IF addN
                   THEN Append(prow, MkRow(job, "N", None, None, {},
                                           Len(attempts), 0, dev.on, Zero))
                   ELSE prow
        /\ dev' = [dev EXCEPT !.ifail =
                     IF enumOn THEN {PairOf("E", c) : c \in seen} ELSE {}]
        /\ pc' = "finish"
  /\ UNCHANGED <<w, ledger, attempts, snap, store, staged, book, ibook, chain,
                 pclaims, journal, edits, rp, raced, job>>

\* Finish (CR-31): a deviation finishes by TailOf(actual). No deviation
\* takes the plan's own return, which N already holds. Stop adds the
\* re-issue while Len(chain) < MaxChain. The book writes come from the
\* receipt. Control "MarkedFinish" stops after every deviation. Control
\* "StagedClaims" stores the claims of the staged rows.
Finish ==
  /\ pc = "finish"
  /\ LET e == prow[1]
         stop == dev.on /\ (Defect = "MarkedFinish" \/ w.tail[e.cand] = "stop")
         re == stop /\ Len(chain) < MaxChain
     IN /\ book' = book \cup dev.refused
        \* Changed by plan v4 errata (V4-13): replay sees a deviating Parked
        \* row of a receive whose commit may veto only on its stop path
        \* (CR-7, CR-18). By D-OBL-1 that is the Stop branch. So the finish
        \* books the item-failed pairs of such a row only on Stop (U2 (a):
        \* "on a deviating row that replay sees").
        \* Changed by plan v4 errata (V4-18): the rule covers every vetoing
        \* Stored or Parked row (G2-b, RowDeviation::vetoing). That is a
        \* receive whose commit may veto (V4-13), or a send row with a wide
        \* waiter. Replay skips such a waiter on a Stored row (CR-7), so on
        \* Return it has no mark.
        \* Changed by decision 124 (L3, X7): a send row is vetoing when a
        \* waiter whose commit may veto is a candidate of the row (VetoingOf,
        \* MayVetoW), with no bind count (base E3-a). Replay skips every such
        \* waiter on a Stored row. The code is unchanged.
        /\ ibook' = ibook \cup (IF e.cand = None /\ VetoingOf(staged.ord) /\ ~stop
                               THEN {} ELSE dev.ifail)
        /\ pclaims' =
             IF ~Seq THEN {}
             ELSE IF Defect = "StagedClaims"
                  THEN (IF staged.c0 = None THEN {} ELSE {staged.c0})
                  ELSE (IF e.cand = None THEN {} ELSE {e.cand})
        /\ chain' = IF re THEN Append(chain, TRUE) ELSE chain
        /\ staged' = [staged EXCEPT !.fresh = FALSE]
        /\ pc' = IF re THEN "reissue" ELSE "ship"
  /\ UNCHANGED <<w, ledger, attempts, snap, store, prow, dev, journal, edits,
                 rp, raced, job>>

\* The re-issue J stages its anchor first (CR-19 case 2), then N. These are
\* machinery rows, so J commits with no deviation and returns.
Reissue ==
  /\ pc = "reissue"
  /\ prow' = Concat(prow,
               <<MkRow(job + 1, "A", None, None, {}, Len(attempts), 0, FALSE,
                       Zero),
                 MkRow(job + 1, "N", None, None, {}, Len(attempts), 0, FALSE,
                       Zero)>>)
  /\ job' = job + 1
  /\ pc' = "ship"
  /\ UNCHANGED <<w, ledger, attempts, snap, store, staged, dev, book, ibook,
                 chain, pclaims, journal, edits, rp, raced>>

\* The racing host row H draws one unit from p, or meets its own refusal.
Race ==
  /\ Racer
  /\ ~raced
  /\ pc \in {"stage", "start", "try", "group"}
  /\ raced' = TRUE
  /\ IF ledger["p"] >= 1
     THEN /\ attempts' = Append(attempts, One("p"))
          /\ ledger' = Minus(ledger, One("p"))
     ELSE UNCHANGED <<attempts, ledger>>
  /\ UNCHANGED <<w, snap, store, staged, prow, dev, book, ibook, chain,
                 pclaims, journal, edits, rp, pc, job>>

-----------------------------------------------------------------------------
(* Play of the Head config (the v2 rows, plan v4 M4 scenario).             *)

\* The head waiter in DR-75 order (CR-2).
HeadOf(cs) == IF "c1" \in cs THEN "c1" ELSE IF "c2" \in cs THEN "c2" ELSE None

\* The book that the settle and the re-issues read (CR-20). Control
\* "NoBook" ignores the refused pairs. Control "NoItemBook" ignores the
\* item-failed pairs.
SettleBook(bk, ibk) ==
  (IF Defect = "NoBook" THEN {} ELSE bk)
    \cup (IF Defect = "NoItemBook" THEN {} ELSE ibk)

\* The staging of a Head job. Job 1 takes T and puts P. A re-issue stages
\* its anchor A first (CR-19 case 2). The settle lifts r (L) and re-offers
\* it to the head (R). The element F offers x to the head after the
\* settle. The plan stages N when TailOf(staged F outcome) = stop.
HeadPlan ==
  LET sb == SettleBook(book, ibook)
      h == HeadOf(store.cand)
      settle == store.r = "stored" /\ h # None /\ PairOf("R", h) \notin sb
      fh == IF settle THEN HeadOf(store.cand \ {h}) ELSE h
      fx == store.inc \in {"flight", "stored"}
      fc0 == IF fx /\ fh # None /\ w.grd[fh] /\ PairOf("F", fh) \notin sb
             THEN fh ELSE None
      first == IF job = 1
               THEN <<[k |-> "T", c |-> None], [k |-> "P", c |-> None]>>
               ELSE <<[k |-> "A", c |-> None]>>
      mid == IF settle
             THEN <<[k |-> "L", c |-> h], [k |-> "R", c |-> h]>>
             ELSE <<>>
      el == IF fx THEN <<[k |-> "F", c |-> fc0]>> ELSE <<>>
      nx == IF fx /\ w.tail[fc0] = "stop"
            THEN <<[k |-> "N", c |-> None]>> ELSE <<>>
  IN [plan |-> Concat(Concat(Concat(first, mid), el), nx),
      props |-> (IF settle THEN {PairOf("R", h)} ELSE {})
                  \cup (IF fc0 # None THEN {PairOf("F", fc0)} ELSE {})]

HStage ==
  /\ pc = "hstage"
  \* Changed by plan v4 errata (V4-18): every staging record has gv. The
  \* Head config has no wide continuation.
  /\ LET hp == HeadPlan
     IN staged' = [ord |-> <<>>, c0 |-> None, plan |-> hp.plan, next |-> 1,
                   n |-> FALSE, props |-> hp.props, fresh |-> TRUE,
                   gv |-> w.grd]
  /\ dev' = NoDev
  /\ pc' = "hrow"
  /\ UNCHANGED <<w, ledger, attempts, snap, store, prow, book, ibook, chain,
                 pclaims, journal, edits, rp, raced, job>>

\* The commit of one Head row. A deviation stops the commit after the row.
\* Added by plan v4 errata (V4-14): control "ContinueAfterDeviation" acts in
\* the shared commit loop of both roles. The rows after a deviating row
\* commit against a view that their commit did not meet, and the receipt
\* collects every deviation of the job. Head order is deterministic, so
\* replay stages the same rows.
HCommit(row, dr, st, dv, last) ==
  LET cad == Defect = "ContinueAfterDeviation"
  IN /\ prow' = Append(prow, [row EXCEPT !.stale = cad /\ dev.on])
     /\ attempts' = IF dr = Zero THEN attempts ELSE Append(attempts, dr)
     /\ ledger' = Minus(ledger, dr)
     /\ store' = st
     /\ dev' = IF ~dv.on THEN dev
               ELSE IF dev.on
                    THEN [dev EXCEPT !.refused = @ \cup dv.refused,
                                     !.ifail = @ \cup dv.ifail]
                    ELSE dv
     /\ staged' = [staged EXCEPT !.next = @ + 1]
     /\ pc' = IF (dv.on /\ ~cad) \/ last THEN "hfinish" ELSE "hrow"
     /\ UNCHANGED <<w, snap, book, ibook, chain, pclaims, journal, edits, rp,
                    raced, job>>

HDev(kind, rf, ifl) ==
  [on |-> TRUE, kind |-> kind, refused |-> rf, ifail |-> ifl, met |-> {}]

\* The Head config has no racer. So S_r is the ledger at the row start, and
\* the item test of a head row equals its tuple test. A head row R or F
\* never meets a refusal. The lift L has no item test, so it can.
\* Control "OwnerPaidPut" lets the put draw on p. Control "PlainLift" loses
\* r when its re-offer does not fire, because a lift that is not a peek
\* reserves no introduction for the re-store (D.3).
HRow ==
  /\ pc = "hrow"
  /\ LET pr == staged.plan[staged.next]
         at == Len(attempts)
         last == staged.next = Len(staged.plan)
         pd == IF Defect = "OwnerPaidPut" /\ ledger["p"] >= 1
               THEN One("p") ELSE Zero
     IN CASE pr.k \in {"T", "A", "N"} ->
               HCommit(MkRow(job, pr.k, None, None, {}, at, 0, FALSE, Zero),
                       Zero, store, NoDev, last)
          [] pr.k = "P" ->
               HCommit(MkRow(job, "P", None, None, {}, at,
                             IF pd = Zero THEN 0 ELSE at + 1, FALSE, pd),
                       pd, store, NoDev, last)
          [] pr.k = "L" ->
               IF FitsIn(LiftShare, ledger, {})
               THEN HCommit(MkRow(job, "L", pr.c, pr.c, {}, at, at + 1, FALSE,
                                  LiftShare),
                            LiftShare, [store EXCEPT !.r = "lifted"], NoDev,
                            last)
               ELSE HCommit(MkRow(job, "L", None, pr.c,
                                  {[c |-> pr.c, pos |-> at]}, at, 0, FALSE,
                                  Zero),
                            Zero, [store EXCEPT !.inert = @ \cup {"L"}],
                            HDev("L", {PairOf("L", pr.c)}, {}), last)
          [] pr.k = "R" ->
               IF FitsIn(ReofferShare(pr.c), ledger, {})
               THEN HCommit(MkRow(job, "R", pr.c, pr.c, {}, at, at + 1, FALSE,
                                  ReofferShare(pr.c)),
                            ReofferShare(pr.c),
                            [store EXCEPT !.cand = @ \ {pr.c}, !.r = "gone"],
                            NoDev, last)
               ELSE HCommit(MkRow(job, "R", None, pr.c, {}, at, 0, FALSE, Zero),
                            Zero,
                            [store EXCEPT !.r = IF Defect = "PlainLift"
                                                THEN "lost" ELSE "stored"],
                            HDev("R", {}, {PairOf("R", pr.c)}), last)
          [] OTHER ->
               IF pr.c = None
               THEN HCommit(MkRow(job, "F", None, None, {}, at, 0, FALSE, Zero),
                            Zero, [store EXCEPT !.inc = "stored"], NoDev, last)
               ELSE IF FitsIn(ShareOf(pr.c), ledger, {})
               THEN HCommit(MkRow(job, "F", pr.c, pr.c, {}, at, at + 1, FALSE,
                                  ShareOf(pr.c)),
                            ShareOf(pr.c),
                            [store EXCEPT !.cand = @ \ {pr.c}, !.inc = "gone"],
                            NoDev, last)
               ELSE HCommit(MkRow(job, "F", None, pr.c, {}, at, 0, FALSE, Zero),
                            Zero, [store EXCEPT !.inc = "stored"],
                            HDev("F", {}, {PairOf("F", pr.c)}), last)

\* The finish of a Head job (CR-31, B.6). The settle rows L and R come
\* before the element, so a deviation at L or R stops, because the element
\* follows (rows 1 and 8). The element F finishes by TailOf(actual). No
\* deviation takes the plan's own return. Row 9 (a re-offer after the
\* element) does not occur in this scenario.
HFinish ==
  /\ pc = "hfinish"
  /\ LET e == prow[Len(prow)]
         bk == book \cup dev.refused
         ibk == ibook \cup dev.ifail
         stop == dev.on /\ (\/ Defect = "MarkedFinish"
                            \/ dev.kind \in {"L", "R"}
                            \/ dev.kind = "F" /\ w.tail[e.cand] = "stop")
         re == stop /\ Len(chain) < MaxChain
     IN /\ book' = bk
        /\ ibook' = ibk
        /\ chain' = IF re THEN Append(chain, TRUE) ELSE chain
        /\ job' = IF re THEN job + 1 ELSE job
        /\ staged' = [staged EXCEPT !.fresh = FALSE]
        /\ pc' = IF re THEN "hstage" ELSE "cascade"
  /\ UNCHANGED <<w, ledger, attempts, snap, store, prow, dev, pclaims, journal,
                 edits, rp, raced>>

\* The cascade proposer reads the book (CR-20 readers) and proposes the
\* first stored pair of the head. Control "CascadeIgnoresBook" ignores the
\* refused pairs.
HCascade ==
  /\ pc = "cascade"
  /\ LET cb == (IF Defect = "CascadeIgnoresBook" THEN {} ELSE book)
                 \cup (IF Defect = "NoItemBook" THEN {} ELSE ibook)
         h == HeadOf(store.cand)
         props == IF h # None /\ store.r = "stored" /\ PairOf("R", h) \notin cb
                  THEN {PairOf("R", h)}
                  ELSE IF h # None /\ store.inc = "stored" /\ w.grd[h]
                          /\ PairOf("F", h) \notin cb
                  THEN {PairOf("F", h)}
                  ELSE {}
     IN staged' = [staged EXCEPT !.props = props, !.fresh = TRUE]
  /\ pc' = "ship"
  /\ UNCHANGED <<w, ledger, attempts, snap, store, prow, dev, book, ibook,
                 chain, pclaims, journal, edits, rp, raced, job>>

\* The producer ships the record.
Ship ==
  /\ pc = "ship"
  /\ journal' = [rows |-> prow, att |-> attempts]
  /\ pc' = "edit"
  /\ UNCHANGED <<w, ledger, attempts, snap, store, staged, prow, dev, book,
                 ibook, chain, pclaims, edits, rp, raced, job>>

-----------------------------------------------------------------------------
(* The adversary. Each edit changes the shipped record only.               *)

\* The element row of job 1 in the record, and the fold of the record.
EJ == journal.rows[1]

JF(n) == FoldOf(journal.att, n)

\* An edit replaces the record and counts one edit.
Edit(kind, rows, att) ==
  /\ kind \in Edits
  /\ journal' = [rows |-> rows, att |-> att]
  /\ edits' = edits + 1
  /\ UNCHANGED <<w, ledger, attempts, snap, store, staged, prow, dev, book,
                 ibook, chain, pclaims, rp, raced, pc, job>>

\* The edit phase: Adversary holds, and an edit is left.
EditPhase ==
  /\ pc = "edit"
  /\ Order # "head"
  /\ Adversary
  /\ edits < MaxEdits

\* Drop a refusal of the element row.
AdvDropRefusal ==
  /\ EditPhase
  /\ \E r \in EJ.refs :
       Edit("DropRefusal", [journal.rows EXCEPT ![1].refs = @ \ {r}],
            journal.att)

\* Add a refusal at budget_start. With passing items it has no shortfall
\* (AddRefusal). With failing items it is AddItemFailRefusal.
AdvAddRefusal ==
  /\ EditPhase
  /\ \E c \in Cands :
       LET r == [c |-> c, pos |-> EJ.start]
           pass == ItemPasses(c, JF(EJ.start))
       IN /\ r \notin EJ.refs
          /\ (pass => FitsIn(ShareOf(c), JF(EJ.start), {}))
          /\ Edit(IF pass THEN "AddRefusal" ELSE "AddItemFailRefusal",
                  [journal.rows EXCEPT ![1].refs = @ \cup {r}], journal.att)

\* Name a match outside the allowed set: at a ranked site, with an item
\* failure, or with a guard verdict that refuses it (V4-3).
AdvNameOutside ==
  /\ EditPhase
  /\ \E b \in Cands \ {EJ.cand} :
       /\ Order = "ranked" \/ ~ItemPasses(b, JF(EJ.start)) \/ ~w.grd[b]
       /\ LET k == IF EJ.comm > 0 THEN EJ.comm ELSE EJ.start + 1
              att == IF EJ.comm > 0
                     THEN [journal.att EXCEPT ![EJ.comm] = ShareOf(b)]
                     ELSE InsertAt(journal.att, k, ShareOf(b))
          IN Edit("NameOutside",
                  [journal.rows EXCEPT ![1].cand = b, ![1].comm = k,
                                       ![1].draw = ShareOf(b)],
                  att)

\* Claim Stored at an unguarded site (a send) while a candidate passes.
AdvForgeStored ==
  /\ EditPhase
  /\ Op = "produce"
  /\ EJ.cand \in Cands
  /\ Edit("ForgeStored",
          [journal.rows EXCEPT ![1].cand = None, ![1].comm = 0,
                               ![1].draw = Zero],
          RemoveAt(journal.att, EJ.comm))

\* Add a row after the unit, outside its group (V3-12).
AdvAddRow ==
  /\ EditPhase
  /\ ~(Len(journal.rows) >= 2 /\ journal.rows[2].job = 1)
  /\ Edit("AddRow",
          InsertAt(journal.rows, 2,
                   MkRow(1, "N", None, None, {}, EJ.start, 0, FALSE, Zero)),
          journal.att)

\* Claim a granted COMM that the fold denies, at budget_start or at the
\* position of a refusal.
AdvDeniedComm ==
  /\ EditPhase
  /\ EJ.cand = None
  /\ \E c \in Cands, i \in {EJ.start} \cup {r.pos : r \in EJ.refs} :
       /\ ~FitsIn(ShareOf(c), JF(i), {})
       /\ Edit("DeniedComm",
               [journal.rows EXCEPT ![1].cand = c, ![1].comm = i + 1,
                                    ![1].draw = ShareOf(c),
                                    ![1].refs = {r \in @ : r.c # c}],
               InsertAt(journal.att, i + 1, ShareOf(c)))

\* Cut the record after the unit. The forged record carries the re-issue
\* that a stop after the unit would create.
AdvCutNoStop ==
  /\ EditPhase
  /\ Len(journal.rows) = 2
  /\ journal.rows[2].job = 1
  /\ journal.rows[2].kind = "N"
  /\ Edit("CutNoStop",
          <<EJ,
            MkRow(2, "A", None, None, {}, EJ.start, 0, FALSE, Zero),
            MkRow(2, "N", None, None, {}, EJ.start, 0, FALSE, Zero)>>,
          journal.att)

-----------------------------------------------------------------------------
(* Replay of the random and ranked configs.                                *)

\* Copied from NativeOwnerRefusal.tla:508-510 (RowOk) as the row checks of
\* the element row, and from NativeOwnerRefusal.tla:512-516 (Verdict) as
\* the verdict. ReplayRow, ReplayStop and ReplayVerdict of the plan are
\* the parts of this one function.
RandomReplay ==
  LET rows == journal.rows
      att == journal.att
      e == rows[1]
      sv == FoldOf(att, e.start)
      hasG == Len(rows) >= 2 /\ rows[2].job = 1 /\ rows[2].kind = "G"
      g == rows[2]
      u == IF hasG THEN 3 ELSE 2
      hasN == Len(rows) >= u /\ rows[u].job = 1 /\ rows[u].kind = "N"
      j2 == IF hasN THEN u + 1 ELSE u
      hasJ == Len(rows) >= j2
      \* ReplayStage: named first at a random site, DR-75 order otherwise.
      rord == IF Order = "random" /\ e.cand \in Cands
              THEN <<e.cand, Other(e.cand)>> ELSE <<"c1", "c2">>
      rcs == Filtered(rord, {})
      \* Added by plan v4 errata (V4-18): rv is the guard verdict that replay
      \* reads. A Matched row names the records of its COMM, so a wide waiter
      \* that the row names has the verdict gk of the row. Every other
      \* candidate has its verdict on the DR-75 tuple.
      rv == [c \in Cands |-> IF c \in WideW /\ c = e.cand THEN e.gk
                             ELSE w.grd[c]]
      \* Added by plan v4 errata (V4-18): G2-a (CR-7, C5.6). On a recorded
      \* Stored row the staging of a send skips a wide waiter, because its
      \* guard ran once on the tuple of the window picks of play and the row
      \* does not record those picks. Control "WideSendSearch" does not skip.
      \* Changed by decision 124 (L3, X7): base E3-a (CR-7, C5.6) with no
      \* bind count. On a recorded Stored row the staging of a send skips
      \* every waiter whose commit may veto (MayVetoW, base
      \* native_candidate.rs:57-61 and selection.rs:747-751). The window
      \* picks of play are not the reason. Control "WideSendSearch" does not
      \* skip, and it acts on every such waiter (X7).
      rskip == IF e.cand = None /\ Defect # "WideSendSearch" THEN MayVetoW ELSE {}
      \* E3-a (CR-7): a receive whose commit may veto parks without a
      \* search on a recorded Stored row (base selection.rs:613-619). Every
      \* consume site of the model may veto.
      \* Changed by plan v4 errata (V4-18): the staging reads rv and skips
      \* rskip. rcs keeps the skipped waiter for refOk, rallowed and enum.
      \* Changed by decision 124 (L3, X7): rcs keeps every skipped waiter
      \* whose commit may veto (rskip) for refOk, rallowed and enum.
      rc0 == IF Op = "consume" /\ e.cand = None THEN None
             ELSE StagePickBy(Without(rcs, rskip), rv)
      \* The allowed candidates of the staging. At a vetoing site the stop
      \* path evaluates the guard on the binding candidates, and only on
      \* that path (CR-18). For a produce, rallowed # {} exactly when
      \* rc0 # None.
      \* Added by plan v4 errata (V4-18): S1 (CR-18). A wide continuation
      \* runs no guard on the stop path. It is an allowed candidate only
      \* through its pairs: a verified refusal of the unit names it, or a
      \* pair of the enumeration of the row names it. No condition on the
      \* matched COMM applies (decision record, section 2.2). Every candidate
      \* of a wide receive K is a tuple of K. stopGuard is the guard on the
      \* DR-75 tuple, which control "WideStopByGuard" runs in place of S1.
      \* With a wide waiter the produce rule above does not hold: replay
      \* skips the waiter, so rc0 can be None while the waiter is allowed.
      \* Changed by decision 124 (L3, X7): S1 counts no binds, as base E3-a
      \* counts none. A continuation whose commit may veto (MayVetoW,
      \* MayVetoK) runs no guard on the stop path. It is an allowed candidate
      \* only through a verified refusal of the unit or a pair of the
      \* enumeration of the row. A continuation whose commit cannot veto has
      \* no guard, so stopGuard gives TRUE for it (Init). So the produce rule
      \* above (rallowed # {} exactly when rc0 # None) can fail only when
      \* some candidate may veto. stopGuard is the guard that control
      \* "WideStopByGuard" runs in place of S1: on the first tuple of the row
      \* order of a consume, and on the DR-75 tuple of each waiter of a
      \* produce.
      byPairs(c) == (\E r \in e.refs : r.c = c) \/ ~ItemPasses(c, sv)
      \* Changed by decision 124 (L3, X7): every continuation whose commit
      \* may veto. Control "WideStopByGuard" acts on each of them (X7).
      wideTest(c) == (c \in MayVetoW \/ MayVetoK) /\ Defect # "WideStopByGuard"
      stopGuard(c) == IF MayVetoK THEN c = rcs[1] /\ w.grd[c] ELSE w.grd[c]
      \* Changed by plan v4 errata (V4-18): S1.
      rallowed == {c \in Range(rcs) :
                     IF wideTest(c) THEN byPairs(c) ELSE stopGuard(c)}
      \* ReplayRow: the refusals, the selection and the Stored check.
      inRow(r) == /\ e.start <= r.pos
                  /\ r.pos <= (IF e.comm > 0 THEN e.comm - 1 ELSE Len(att))
      \* Changed by plan v4 errata (V4-17): a store or park that E3-a
      \* produced in replay carries its row search (iv), so
      \* verify_row_refusals runs for it. Control "ParkWithoutSearch" gives
      \* such a row no search, so replay cannot verify a recorded refusal.
      refOk == /\ \A r \in e.refs :
                    /\ r.c \in Range(rcs)
                    /\ inRow(r)
                    /\ RefusalOk(att, ItemPasses(r.c, sv), r.pos, ShareOf(r.c))
               /\ ((Defect = "ParkWithoutSearch" /\ e.cand = None /\ rc0 = None)
                     => e.refs = {})
      gRefOk == hasG => \A r \in g.refs :
                          /\ r.c = e.cand
                          /\ RefusalOk(att, TRUE, r.pos, RetireShare(r.c))
      lifecycle == (Op = "consume" /\ e.cand \in Cands) => e.refs = {}
      prefixOk == (Order = "ranked" /\ e.cand = "c2"
                   /\ Defect # "UncheckedPrefix") =>
                    \/ ~PatternBinds("c1")
                    \/ ~ItemPasses("c1", sv)
                    \/ Defect = "VetoExcuse" /\ ~w.grd["c1"]
      matchOk == e.cand \in Cands =>
                   /\ e.cand \in Range(rcs)
                   /\ ItemPasses(e.cand, sv)
                   \* Changed by plan v4 errata (V4-18): the verdict of the
                   \* named tuple.
                   /\ (Defect # "UncheckedGuard" => rv[e.cand])
                   /\ e.comm \in 1..Len(att)
                   /\ att[e.comm] = ShareOf(e.cand)
                   /\ e.start <= e.comm - 1
                   /\ prefixOk
      storedOk == e.cand = None =>
                    /\ e.comm = 0
                    /\ (Op = "produce" /\ Defect # "UncheckedStored") =>
                         StoredRuleOk(att, e, rcs)
                    /\ (Op = "consume" /\ Defect = "GuardedSearch") =>
                         StoredRuleOk(att, e, rcs)
      gRowOk == /\ g.staged = e.cand
                /\ \/ (g.cand = e.cand /\ g.comm \in 1..Len(att)
                       /\ att[g.comm] = RetireShare(e.cand))
                   \/ (g.cand = None /\ g.refs # {})
      gShape == IF ClassMode /\ e.cand \in Cands /\ Defect # "StagedRetires"
                THEN hasG /\ gRowOk
                ELSE hasG => (ClassMode /\ e.cand \in Cands /\ gRowOk)
      \* ReplayStop: the plan of replay follows rc0 (V3-14).
      rdev == e.cand # rc0
      groupEnded == /\ Defect = "StagedRetires"
                    /\ ClassMode /\ rc0 \in Cands /\ ~hasG
      wantN == ~groupEnded /\ w.tail[rc0] = "stop"
      followed == wantN /\ hasN
      ended == groupEnded \/ (wantN /\ ~hasN)
      cont == ~rdev /\ e.refs = {} /\ ~(hasG /\ g.refs # {})
      contOk == (followed /\ Defect # "UncheckedContinue") =>
                  IF Defect = "RowStop"
                  THEN ~rdev /\ e.refs = {} /\ (hasG => g.refs = {})
                  ELSE cont
      \* Changed by plan v4 errata (V4-18): S1 drops the guard of a wide
      \* candidate in the third bullet.
      \* Changed by decision 124 (L3, X7): the third bullet drops the guard
      \* of every candidate whose commit may veto (wideTest), with no bind
      \* count. The code is unchanged.
      canStopE == \/ e.refs # {}
                  \/ e.cand = None /\ rallowed # {}
                  \/ (e.cand \in Cands
                      /\ \E c \in Range(rcs) \ {e.cand} :
                           /\ IF wideTest(c) THEN TRUE ELSE stopGuard(c)
                           /\ ~ItemPasses(c, sv))
      canStopU == canStopE \/ (hasG /\ g.refs # {})
      stopOk == (ended /\ Defect # "UncheckedStop") =>
                  IF Defect = "RowStop"
                  THEN IF hasG
                       THEN g.refs # {} /\ ~rdev /\ e.refs = {}
                       ELSE canStopE
                  ELSE canStopU
      \* ReplayVerdict: the mark, the finish and the re-issue.
      mark == rdev \/ ended
      stop == mark /\ (Defect = "MarkedFinish" \/ w.tail[e.cand] = "stop")
      jOk == IF stop
             THEN /\ hasJ
                  /\ Len(rows) = j2 + 1
                  /\ rows[j2].job = 2 /\ rows[j2].kind = "A"
                  /\ rows[j2 + 1].job = 2 /\ rows[j2 + 1].kind = "N"
             ELSE ~hasJ
      ok == /\ e.job = 1 /\ e.kind = "E"
            /\ e.start <= Len(att)
            /\ FoldOk(att)
            /\ refOk /\ gRefOk /\ lifecycle /\ matchOk /\ storedOk /\ gShape
            /\ contOk /\ stopOk
            /\ ~(hasN /\ ~followed)
            /\ jOk
      enum == IF mark /\ e.cand = None /\ rallowed # {}
              THEN {PairOf("E", c) : c \in {c \in Range(rcs) : ~ItemPasses(c, sv)}}
              ELSE {}
      \* Added by plan v4 errata (V4-17): replay books the verified refusals
      \* of every row of the unit, also on a row that is not marked. A row
      \* that fires a later candidate after a refusal, or a refused park,
      \* has no mark when its Tail is Return. Control "ReplayBookOnMark"
      \* books them only on a marked row.
      refBook == {PairOf("E", r.c) : r \in e.refs}
                   \cup (IF hasG THEN {PairOf("G", r.c) : r \in g.refs} ELSE {})
  IN [done |-> TRUE,
      verdict |-> ok,
      \* Changed by plan v4 errata (V4-17).
      book |-> IF Defect = "ReplayBookOnMark" /\ ~mark THEN {} ELSE refBook,
      \* Changed by plan v4 errata (V4-18): G2-b. Replay books the item-failed
      \* pairs of a vetoing Stored or Parked row only on Stop, as play does.
      \* Replay can have its own mark on such a row, so its finish needs the
      \* filter too. For a consume row a mark implies Stop, so the filter
      \* drops nothing there.
      ibook |-> IF e.cand = None /\ VetoingOf(rcs) /\ ~stop THEN {} ELSE enum,
      reissues |-> IF stop THEN 1 ELSE 0,
      claims |-> IF Seq /\ e.cand \in Cands THEN {e.cand} ELSE {},
      c0 |-> rc0,
      ended |-> ended,
      followed |-> followed,
      cont |-> cont]

-----------------------------------------------------------------------------
(* Replay of the Head config.                                              *)

\* Head order is deterministic, and the Head config has no adversary. So
\* replay restages each row as play did, and it reads the staged candidate
\* of the journal row. It checks each row at fold(budget_start).
HeadShare(k, c) ==
  CASE k = "L" -> LiftShare
    [] k = "R" -> ReofferShare(c)
    [] OTHER -> ShareOf(c)

HeadItems(att, k, c, n) ==
  IF k = "L" THEN TRUE ELSE FitsIn(HeadShare(k, c), FoldOf(att, n), {})

HeadReplay ==
  LET rows == journal.rows
      att == journal.att
      rowOk(i) ==
        LET r == rows[i]
        IN /\ \A f \in r.refs :
                RefusalOk(att, HeadItems(att, r.kind, f.c, r.start), f.pos,
                          HeadShare(r.kind, f.c))
           /\ (r.kind \in {"L", "R", "F"} /\ r.cand \in Cands) =>
                /\ r.comm \in 1..Len(att)
                /\ att[r.comm] = HeadShare(r.kind, r.cand)
                /\ HeadItems(att, r.kind, r.cand, r.start)
                /\ (r.kind = "F" => w.grd[r.cand])
           /\ (r.kind \in {"R", "F"} /\ r.cand = None /\ r.staged # None) =>
                \/ \E f \in r.refs : f.c = r.staged
                \/ ~HeadItems(att, r.kind, r.staged, r.start)
           /\ (r.kind = "P" /\ r.comm > 0) =>
                (r.comm \in 1..Len(att) /\ att[r.comm] = r.draw)
      devRow(i) ==
        LET r == rows[i]
        IN \/ r.refs # {}
           \/ r.kind \in {"R", "F"} /\ r.cand = None /\ r.staged # None
      enumOf(i) ==
        LET r == rows[i]
        IN IF /\ r.kind \in {"R", "F"} /\ r.cand = None /\ r.staged # None
              /\ ~HeadItems(att, r.kind, r.staged, r.start)
           THEN {PairOf(r.kind, r.staged)} ELSE {}
      firstOk == \A i \in DOMAIN rows :
                   (i = 1 \/ rows[i].job # rows[i - 1].job) =>
                     IF rows[i].job = 1 THEN rows[i].kind = "T"
                     ELSE rows[i].kind = "A"
      stopOk == \A i \in DOMAIN rows :
                  devRow(i) => (i = Len(rows) \/ rows[i + 1].job # rows[i].job)
      \* Changed by plan v4 errata (V4-14): control "ContinueAfterDeviation"
      \* removes the stop rule from the replay of the shared commit loop too,
      \* so both roles commit the rows after a deviating row.
      ok == /\ FoldOk(att)
            /\ firstOk
            /\ (Defect # "ContinueAfterDeviation" => stopOk)
            /\ \A i \in DOMAIN rows : rowOk(i)
  IN [done |-> TRUE,
      verdict |-> ok,
      book |-> UNION {{PairOf(rows[i].kind, f.c) : f \in rows[i].refs} :
                        i \in DOMAIN rows},
      ibook |-> UNION {enumOf(i) : i \in DOMAIN rows},
      reissues |-> Cardinality({rows[i].job : i \in DOMAIN rows}) - 1,
      claims |-> {},
      c0 |-> None,
      ended |-> FALSE,
      followed |-> FALSE,
      cont |-> TRUE]

\* Replay reads no live ledger, so the model computes it in one step.
ReplayVerdict ==
  /\ pc = "edit"
  /\ rp' = IF Order = "head" THEN HeadReplay ELSE RandomReplay
  /\ pc' = "end"
  /\ UNCHANGED <<w, ledger, attempts, snap, store, staged, prow, dev, book,
                 ibook, chain, pclaims, journal, edits, raced, job>>

-----------------------------------------------------------------------------
(* The specification.                                                      *)

Next ==
  \/ Stage
  \/ StartRow
  \/ TryCandidate
  \/ RetireRow
  \/ FinishRow
  \/ Finish
  \/ Reissue
  \/ Race
  \/ HStage
  \/ HRow
  \/ HFinish
  \/ HCascade
  \/ Ship
  \/ AdvDropRefusal
  \/ AdvAddRefusal
  \/ AdvNameOutside
  \/ AdvForgeStored
  \/ AdvAddRow
  \/ AdvDeniedComm
  \/ AdvCutNoStop
  \/ ReplayVerdict

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* The semantic side of the invariants.                                    *)

\* The record is shipped, and replay has its verdict.
Shipped == pc = "end"

\* The true fold: the attempts of play. An edit changes only the record.
TrueFold(n) == FoldOf(attempts, n)

\* The element row and the group row of job 1 in the record.
JE == journal.rows[1]

JHasG == Len(journal.rows) >= 2 /\ journal.rows[2].job = 1
           /\ journal.rows[2].kind = "G"

JG == journal.rows[2]

\* The allowed candidates of the element row: the pattern binds and the
\* guard passes. The book is empty at job 1.
AllowedTrue == {c \in Cands : PatternBinds(c) /\ w.grd[c]}

\* A refusal that honest play makes: its items pass at the true snapshot,
\* and its share falls short at its position.
TrulyRefused(c, start, pos, share) ==
  /\ ItemPasses(c, TrueFold(start))
  /\ \E x \in Purses : share[x] > TrueFold(pos)[x]

\* Added by plan v4 errata (V4-18): the allowed candidates of the stop path
\* (S1, risk K18). A wide waiter, or every candidate of a wide receive K,
\* counts when it fails its items at the true fold or a true refusal names
\* it. Its guard does not count, because the record does not hold the window
\* picks of play. Every other candidate counts when its guard passes. With
\* Wide = FALSE, AllowedStopTrue = AllowedTrue.
\* Changed by decision 124 (L3, X7): S1 counts no binds (X7). A candidate
\* whose commit may veto (MayVetoW, MayVetoK) counts only when it fails its
\* items at the true fold or a true refusal names it. Its guard does not
\* count, as in base E3-a. A candidate whose commit cannot veto (w.mayVeto)
\* has no guard, so its verdict is TRUE (Init), and it counts when its
\* pattern binds.
AllowedStopTrue ==
  {c \in Cands :
     /\ PatternBinds(c)
     \* Changed by decision 124 (L3, X7): every candidate whose commit may
     \* veto.
     /\ IF c \in MayVetoW \/ MayVetoK
        THEN \/ ~ItemPasses(c, TrueFold(JE.start))
             \/ \E r \in JE.refs :
                  r.c = c /\ TrulyRefused(c, JE.start, r.pos, ShareOf(c))
        ELSE w.grd[c]}

\* CanStop(U) at the true fold (CR-18).
\* Changed by plan v4 errata (V4-18): bullets 2 and 3 read AllowedStopTrue
\* (S1). So bullet 3 drops the guard of a wide candidate. This relaxation is
\* risk K18 of the plan.
\* Changed by decision 124 (L3, X7): bullet 3 drops the guard of every
\* candidate whose commit may veto, with no bind count (X7). The code is
\* unchanged.
TrueCanStop ==
  \/ \E r \in JE.refs : TrulyRefused(r.c, JE.start, r.pos, ShareOf(r.c))
  \/ (JE.cand = None /\ AllowedStopTrue # {})
  \/ (JE.cand \in Cands
      /\ \E c \in AllowedStopTrue \ {JE.cand} :
           ~ItemPasses(c, TrueFold(JE.start)))
  \/ (JHasG
      /\ \E r \in JG.refs :
           \E x \in Purses : RetireShare(r.c)[x] > TrueFold(r.pos)[x])

-----------------------------------------------------------------------------
(* Invariants.                                                             *)

TypeOK ==
  /\ w \in WorldSet
  /\ ledger \in [Purses -> 0..MaxBalance]
  /\ Len(attempts) \in 0..MaxAtt
  /\ snap.at \in Positions
  /\ store.cand \subseteq Cands
  /\ store.mem2 \subseteq Cands
  /\ store.inert \subseteq {"L", "c1", "c2"}
  /\ store.inc \in {"flight", "stored", "gone"}
  /\ store.r \in {"stored", "lifted", "gone", "lost"}
  /\ \A i \in DOMAIN prow : prow[i].kind \in Kinds /\ prow[i].cand \in Outs
  /\ book \subseteq PairIds
  /\ ibook \subseteq PairIds
  /\ Len(chain) \in 0..MaxChain
  /\ pclaims \subseteq Cands
  /\ edits \in 0..MaxEdits
  /\ raced \in BOOLEAN
  /\ pc \in Phases
  /\ job \in 1..MaxJobs

\* Copied from NativeOwnerRefusal.tla:798 (LedgerIsFold).
LedgerIsFold == ledger = FoldOf(attempts, Len(attempts))

\* For an honest record, replay accepts. Its book writes (refused and
\* item-failed) and its re-issues equal those of play. Replay follows
\* exactly the rows and anchors of the record, so its rows equal them. The
\* deviation mark is not compared.
TxnPlayReplayAgree ==
  (Shipped /\ honest) =>
    /\ rp.verdict
    /\ rp.book = book
    /\ rp.ibook = ibook
    /\ rp.reissues = Len(chain)

\* An accepted Stored row at a non-vetoing site (a send) has a true refusal
\* or an item failure for every allowed candidate.
\* Changed by plan v4 errata (V4-18): a wide waiter is exempt. The Stored
\* check skips it (R15), as the base Stored check skips a continuation whose
\* commit may veto (E3-a, R17).
\* Changed by decision 124 (L3, X7): every candidate whose commit may veto
\* (MayVetoW) is exempt, with no bind count, as the base Stored check skips
\* it (E3-a, R17). Each allowed candidate of a produce whose commit cannot
\* veto (w.mayVeto) remains to check.
TxnStoredClaimIsHonest ==
  (Shipped /\ rp.verdict /\ Order # "head" /\ Op = "produce"
   /\ JE.cand = None) =>
    \A c \in AllowedTrue \ MayVetoW :
      \/ \E r \in JE.refs :
           r.c = c /\ TrulyRefused(c, JE.start, r.pos, ShareOf(c))
      \/ ~ItemPasses(c, TrueFold(JE.start))

\* Every accepted refusal passes its items at the fold and falls short at
\* its position.
TxnRefusalNeedsShortfall ==
  (Shipped /\ rp.verdict /\ Order # "head") =>
    /\ \A r \in JE.refs : TrulyRefused(r.c, JE.start, r.pos, ShareOf(r.c))
    /\ JHasG =>
         \A r \in JG.refs :
           \E x \in Purses : RetireShare(r.c)[x] > TrueFold(r.pos)[x]

\* An accepted match passes its items at the fold and its guard verdict
\* (V4-3). At a ranked site, the earlier candidate outside the tie fails its
\* pattern or its items (CR-34).
\* Changed by decision 124 (CR-34'): at a ranked site, the search of play
\* over the ranked order, with the recorded record first inside its score
\* tie, gives the row. For the two candidates of this model, the condition
\* below gives that verdict, and it is unchanged.
AllowedChoice ==
  (Shipped /\ rp.verdict /\ Order # "head" /\ JE.cand \in Cands) =>
    /\ ItemPasses(JE.cand, TrueFold(JE.start))
    \* Changed by plan v4 errata (V4-18): a wide waiter has the verdict of
    \* the tuple that the COMM names (gk).
    /\ IF JE.cand \in WideW THEN JE.gk ELSE w.grd[JE.cand]
    /\ (Order = "ranked" /\ JE.cand = "c2") =>
         \/ ~PatternBinds("c1")
         \/ ~ItemPasses("c1", TrueFold(JE.start))

\* An accepted cut after unit U needs CanStop(U).
StopIsHonest ==
  (Shipped /\ rp.verdict /\ rp.ended /\ Order # "head") => TrueCanStop

\* Over edited records only, an accepted record follows no row after a
\* unit that fails CanContinue(U). The group rows are part of the unit.
ContinueIsHonest ==
  (Shipped /\ rp.verdict /\ ~honest /\ rp.followed) => rp.cont

\* Each committed row has the effect of the host operation of its actual
\* outcome. A row that commits after a deviating unit was staged against a
\* view that the commit did not meet.
HostParity == \A i \in DOMAIN prow : ~prow[i].stale

\* No staging proposes a pair from book.
RefusedPairNotRefired == staged.fresh => staged.props \cap book = {}

\* No staging of play proposes a pair from the ibook of play.
ItemFailedPairNotRefired == staged.fresh => staged.props \cap ibook = {}

\* The put and the anchor draw on the payer only.
OneCounter ==
  \A i \in DOMAIN prow : prow[i].kind \in {"P", "A"} => prow[i].draw = Zero

\* A lifted record is restored or re-offered.
NoLostRecord == store.r # "lost"

\* A refused class fire leaves every member stored.
RefusedTupleKeepsMembers ==
  \A i \in DOMAIN prow :
    (ClassMode /\ prow[i].kind = "E" /\ prow[i].refs # {}) =>
      \A r \in prow[i].refs : r.c \in store.cand /\ r.c \in store.mem2

\* After a committed class fire, the second member of the fired class is
\* retired, or its refused retire parks inert.
SpentClassRetired ==
  (ClassMode /\ pc # "group") =>
    \A i \in DOMAIN prow :
      (prow[i].kind = "E" /\ prow[i].cand \in Cands) =>
        \/ prow[i].cand \notin store.mem2
        \/ prow[i].cand \in store.inert

\* Play and replay store the same claims, the claims of the committed rows.
SeqClaimsAgree == (Shipped /\ honest) => rp.claims = pclaims

-----------------------------------------------------------------------------
(* Witnesses. Each witness config must violate its property.               *)

\* An honest record fires a later candidate and replays.
NoAlternativeFire ==
  ~(/\ Shipped /\ honest /\ rp.verdict /\ Order # "head"
    /\ JE.cand \in Cands /\ JE.staged \in Cands /\ JE.cand # JE.staged)

\* Play stages c2, which is second in DR-75 order, and replay completes.
NoRandomPickCompletion ==
  ~(Shipped /\ honest /\ rp.verdict /\ Order = "random" /\ JE.staged = "c2")

\* A stop re-issue commits and replays.
NoStopReissue ==
  ~(Shipped /\ honest /\ rp.verdict /\ Len(chain) > 0)

\* An honest ranked deviation replays (N1).
NoRankedAlternative ==
  ~(/\ Shipped /\ honest /\ rp.verdict /\ Order = "ranked"
    /\ JE.staged = "c1" /\ JE.cand = "c2")

\* Added by plan v4 errata (V4-18): H1 is reached. Play stages a wide
\* waiter, its commit stores, and replay accepts the honest record. Replay
\* skips the waiter on the Stored row (G2-a), so it stages no fire of it.
\* Changed by decision 124 (L3, X7): replay skips the waiter by base E3-a,
\* because its commit may veto, with no bind count. The code is unchanged.
NoWideStoredDeviation ==
  ~(/\ Shipped /\ honest /\ rp.verdict
    /\ JE.cand = None /\ JE.staged \in WideW)

\* Added by plan v4 errata (V4-18): H2 is reached. Play stages no fire and
\* the datum stores, while the DR-75 tuple of a wide waiter passes its
\* guard. Replay skips the waiter (G2-a) and accepts the honest record.
\* Changed by decision 124 (L3, X7): replay skips the waiter by base E3-a,
\* because its commit may veto, with no bind count. The code is unchanged.
NoWideOrderStore ==
  ~(/\ Shipped /\ honest /\ rp.verdict
    /\ JE.cand = None /\ JE.staged = None
    /\ \E c \in WideW : w.grd[c])

=============================================================================
