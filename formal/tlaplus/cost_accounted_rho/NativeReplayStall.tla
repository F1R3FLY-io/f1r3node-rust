-------------------- MODULE NativeReplayStall --------------------
\* DR-132 (decision D2-B): the stall rule of native replay.
\*
\* Native replay waits until the recorded predecessors of a live operation
\* complete. Forged evidence can make that wait permanent. The stall rule
\* counts the live tasks of one evaluation and the waiters among them. When
\* every live task waits on a recorded operation, replay declares a stall:
\* every waiter wakes with an error, and the block is rejected.
\*
\* The model follows the Rust protocol of
\* `rholang/src/rust/interpreter/accounting/native_runtime/checked_operations/
\* trace/replay.rs` (`wait_ready`, `reserve_ready`, `changed`) and
\* `trace/replay/progress.rs` (`ReplayProgress`, `Registration`):
\*   - A task counts as live from before its spawn until its program ends.
\*     The root counts from the start.
\*   - A waiter registers its notification, then checks its row under the
\*     state lock. A row that is not ready counts the waiter as parked at the
\*     current version, in the same critical section. A waiter that is
\*     already counted at that version does not count again.
\*   - A publish completes its row, bumps the version and clears the parked
\*     count inside its critical section. It notifies after it unlocks.
\*   - The check that declares a stall runs on park and on finish.
\*
\* Each guard switch removes one safeguard. Every switch is FALSE in a safe
\* configuration. Each refutation configuration sets one switch and lists
\* the invariant that the missing safeguard breaks. The lifecycle guards
\* (cancellation, panics, drop order, a second evaluation) are in
\* `NativeReplayStallLifecycle.tla`.
\*
\* NoFreshLeave checks that a waiter leaves its registration only when the
\* registration is stale: a counted waiter cannot meet a ready row or an
\* error at the version at which it parked, because every change that raises
\* readiness bumps the version and a reserve needs a ready row. So the leave
\* on the ready and error paths is a no-op, and only the leave on drop
\* matters (lifecycle model).
\*
\* Added by Reified RSpaces (decision 90): space transactions. A space
\* operation of a Reified RSpace stages several operations and commits them
\* as one transaction under its stripes, so it cannot wait before it locks,
\* as a host operation does. A transaction task of the scenarios "txn..."
\* follows the readiness wait of space transactions in native replay: the
\* commit check of the native space backend
\* (`rspace++/src/rspace/replay_rspace/native_session/space.rs`) and the
\* readiness module of native replay
\* (`trace/replay/session/execution/space_readiness.rs` in rholang):
\*   - It takes its stripes.
\*   - Under the state lock it simulates the selection of each staged
\*     operation: the first open row of its source that no earlier operation
\*     of the transaction took. The transaction is ready when each selected
\*     row fits its staged operation (the recorded candidate) and each
\*     predecessor is complete or an earlier row of the transaction.
\*   - A selection without a row, or a row that does not fit, is NotReady,
\*     not an error, because a stale view can stage other operations.
\*   - Ready: it reserves the rows, publishes them in order, releases its
\*     stripes and notifies.
\*   - NotReady: it releases its stripes first. Then it registers its
\*     notification and, under the state lock, retries at once when the
\*     version changed since its check, and otherwise it parks as a counted
\*     waiter at that version. Every change bumps the version under the
\*     state lock and notifies after the unlock, so a change before the
\*     registration shows as a new version and a change after it notifies.
\* The control scenarios each remove one part: "txnracefailclosed" (the code
\* before decision 90: a row that is not ready fails and closes the epoch),
\* "txnstalemismatchfails" and "txnsharedmismatchfails" (a selection that
\* does not fit is an error), "txnstripesunderstripes" (the transaction
\* parks while it holds its stripes), "txnracenointxn" (a predecessor in the
\* transaction does not count) and "txnrepeatnorepeat" (a repeated source
\* selects one row twice).
\*
\* Changed by decision 124 (L10): a commit that does not pass leaves and fails
\* with the base action LeaveAndFail, and the epoch stays open.
\* Changed by Reified RSpaces (decision 92): the check reads the first staged
\* row only. Every row of a space transaction names the space counter S(sid)
\* in its footprint, so the rows of one space form one chain. The first row
\* of each transaction is unique to its job, or the transaction stages an
\* anchor first (`formal/tlaplus/reified_rspaces/SpaceReplayOrder.tla`).
\* When the first row is ready, the space is in the state of play, so every
\* later row is ready and fits. At commit, a later row that is not ready or
\* does not fit fails closed (`begin_row`): the transaction leaves its
\* registration, fails the replay with the base action LeaveAndFail and
\* releases its stripe (`TxnCheck`). The epoch stays open.
\*
\* Added by decision 124 (L10): the scenario "txnforgedcommit" takes this
\* branch of `TxnCheck`. Its record is forged: the second staged operation
\* of the transaction does not fit its recorded row. The first row is ready,
\* so the readiness check passes, and the commit fails at the second row.
\* The root waits on a later row, so the stall rule declares a stall when
\* the failed transaction finishes, and replay ends in a stall verdict.
\*
\* Changed by Reified RSpaces (decision 92): a transaction is ready when the
\* first open row of its first source exists and its predecessors are
\* complete. The rows of each transaction scenario form one chain in `Pred`,
\* and in "txnshared" each transaction stages its anchor before its re-offer.
\* The controls "txnstalemismatchfails", "txnsharedmismatchfails",
\* "txnracenointxn" and "txnrepeatnorepeat" are disabled, because they remove
\* parts of the check of every row, which is gone.
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS
  \* @type: Str;
  Scenario,
  \* @type: Int;
  TaskMax,
  \* @type: Bool;
  ParkAfterUnlock,
  \* @type: Bool;
  SkipReset,
  \* @type: Bool;
  ResetAfterUnlock,
  \* @type: Bool;
  SkipVersion,
  \* @type: Bool;
  CountInChild,
  \* @type: Bool;
  NotifiedAfterCheck,
  \* @type: Bool;
  RootUncounted,
  \* @type: Bool;
  AdmitUncounted,
  \* @type: Bool;
  BoundaryWhileArmed

VARIABLES
  \* @type: Int -> Str;
  pc,
  \* @type: Int -> Int;
  ip,
  \* @type: Int -> Int;
  sel,
  \* @type: Int -> Int;
  reg,
  \* @type: Int -> Str;
  ntf,
  \* @type: Int -> Bool;
  err,
  \* @type: Set(Int);
  done,
  \* @type: Set(Int);
  reserved,
  \* @type: Int;
  version,
  \* @type: Bool;
  closed,
  \* @type: Bool;
  boundary,
  \* @type: Int;
  boundaries,
  \* @type: Int;
  live,
  \* @type: Int;
  parked,
  \* @type: Bool;
  wStalled,
  \* @type: Bool;
  falseStall,
  \* @type: Bool;
  freshLeave,
  \* Added by Reified RSpaces (decision 90): the task that holds the one
  \* stripe of the transaction scenarios, or None.
  \* @type: Int;
  stripe,
  \* The rows that a transaction task reserved, in staging order.
  \* @type: Int -> Seq(Int);
  tsel,
  \* The next row that a transaction task publishes.
  \* @type: Int -> Int;
  tpos,
  \* The version at the last check of a transaction task.
  \* @type: Int -> Int;
  seen

\* Added by Reified RSpaces (decision 90).
txnVars == <<stripe, tsel, tpos, seen>>

vars == <<pc, ip, sel, reg, ntf, err, done, reserved, version, closed,
          boundary, boundaries, live, parked, wStalled, falseStall,
          freshLeave, stripe, tsel, tpos, seen>>

\* Each scenario declares only the tasks it uses, so the symbolic encoding of
\* Apalache stays small. `ScenarioTaskMax` checks the constant.
Tasks == 0..TaskMax
Rows == 0..4
None == -1
\* Task 2 runs outside the evaluation in the "uncounted" scenario.
External == IF Scenario = "uncounted" THEN 2 ELSE None

\* ---------------------------------------------------------------------
\* Scenarios
\* ---------------------------------------------------------------------

\* @type: (Int) => {kind: Str, arg: Int};
Op(s) == [kind |-> "op", arg |-> s]
\* @type: (Int) => {kind: Str, arg: Int};
Spawn(t) == [kind |-> "spawn", arg |-> t]
\* @type: {kind: Str, arg: Int};
Fail == [kind |-> "fail", arg |-> 0]
\* Added by Reified RSpaces (decision 90): a space transaction. Its staged
\* operations are `TxnOps(t)` of the task.
\* @type: {kind: Str, arg: Int};
TxnStep == [kind |-> "txn", arg |-> 0]

\* Added by Reified RSpaces (decision 90): the transaction scenarios. A
\* control scenario runs the transactions of its base scenario with one part
\* of the protocol removed.
\* Changed by Reified RSpaces (decision 92): the controls of the check of
\* every row are disabled.
\* @type: Str;
TxnBase ==
  CASE Scenario \in {"txnrace", "txnracefailclosed", "txnraceregisterlate"} -> "txnrace"
    [] Scenario \in {"txnstripes", "txnstripesunderstripes"} -> "txnstripes"
    [] Scenario = "txnstale" -> "txnstale"
    [] Scenario = "txnshared" -> "txnshared"
    [] Scenario = "txnswap" -> "txnswap"
    [] Scenario = "txnrepeat" -> "txnrepeat"
    [] Scenario = "txnforged" -> "txnforged"
    \* Added by decision 124 (L10).
    [] Scenario = "txnforgedcommit" -> "txnforgedcommit"
    [] OTHER -> "none"
TxnScenario == TxnBase /= "none"
TxnFailClosed == Scenario = "txnracefailclosed"
\* Disabled by Reified RSpaces (decision 92): the check of every row is gone.
\* A mismatch, a predecessor in the transaction and a repeated source no
\* longer enter the readiness check, and the commit checks the later rows.
TxnParkUnderStripes == Scenario = "txnstripesunderstripes"
\* TxnNoInTxnPreds == Scenario = "txnracenointxn"
\* TxnNoRepeat == Scenario = "txnrepeatnorepeat"
\* Added by Reified RSpaces (decision 90): the control "txnraceregisterlate"
\* registers the notification of a parked transaction after its park, outside
\* the critical section of the version check.
TxnRegisterLate == Scenario = "txnraceregisterlate"
\* @type: Seq({kind: Str, arg: Int});
Nothing == <<>>

\* The largest task of the scenario.
ScenarioTaskMax ==
  CASE Scenario = "honest" -> 3
    [] Scenario = "missing" -> 4
    [] Scenario = "inverted" -> 3
    [] Scenario = "uncounted" -> 2
    [] Scenario = "samesource" -> 3
    [] Scenario = "oversubscribed" -> 3
    [] Scenario = "pair" -> 1
    [] Scenario = "missingpair" -> 1
    [] Scenario = "chain" -> 2
    \* Added by Reified RSpaces (decision 90).
    [] TxnBase \in {"txnrace", "txnstripes", "txnstale", "txnshared", "txnswap"} -> 2
    [] TxnBase \in {"txnrepeat", "txnforged"} -> 1
    \* Added by decision 124 (L10).
    [] TxnBase = "txnforgedcommit" -> 1

ASSUME TaskMax = ScenarioTaskMax

\* The rows that the scenario records.
RowCount ==
  CASE Scenario = "honest" -> 4
    [] Scenario = "missing" -> 5
    [] Scenario = "inverted" -> 3
    [] Scenario = "uncounted" -> 2
    [] Scenario = "samesource" -> 3
    [] Scenario = "oversubscribed" -> 2
    [] Scenario = "pair" -> 2
    [] Scenario = "missingpair" -> 3
    [] Scenario = "chain" -> 2
    \* Added by Reified RSpaces (decision 90).
    [] TxnBase = "txnrace" -> 3
    \* Added by Reified RSpaces (decision 92): two anchors and two re-offers.
    [] TxnBase = "txnshared" -> 4
    \* Added by decision 124 (L10): the two rows of the transaction and the
    \* row of the root.
    [] TxnBase = "txnforgedcommit" -> 3
    [] TxnScenario -> 2

UsedRows == {r \in Rows : r < RowCount}

\* The recorded source of each row. Rows of one source share their channel.
\* In "samesource" rows 0 and 2 have source 10 and row 1 has source 11, all
\* on one channel. In "oversubscribed" rows 0 and 1 have source 10.
\* Added by Reified RSpaces (decision 90): in "txnrace" row 0 is the row of
\* a host operation (source 0), and rows 1 and 2 are the two rows of one
\* transaction (sources 11 and 12). In "txnstripes" row 0 (source 10) and
\* row 1 (source 11) belong to two transactions on one stripe. In
\* "txnstale" and "txnforged" row 0 is a host row and row 1 (source 11) a
\* transaction row. In "txnshared", "txnswap" and "txnrepeat" both rows have
\* source 10, as the re-offers of one persistent record do.
\* Changed by Reified RSpaces (decision 92): in "txnshared" each transaction
\* stages its anchor first, so rows 0 and 2 are the anchors (sources 20 and
\* 21), and rows 1 and 3 are the re-offers (source 10).
Src(r) ==
  CASE Scenario = "samesource" -> (IF r = 1 THEN 11 ELSE 10)
    [] Scenario = "oversubscribed" -> 10
    [] TxnBase = "txnrace" -> (IF r = 0 THEN 0 ELSE IF r = 1 THEN 11 ELSE 12)
    [] TxnBase = "txnstripes" -> (IF r = 0 THEN 10 ELSE 11)
    [] TxnBase \in {"txnstale", "txnforged"} -> (IF r = 0 THEN 0 ELSE 11)
    \* [] TxnBase \in {"txnshared", "txnswap", "txnrepeat"} -> 10
    [] TxnBase = "txnshared" -> (IF r = 0 THEN 20 ELSE IF r = 2 THEN 21 ELSE 10)
    [] TxnBase \in {"txnswap", "txnrepeat"} -> 10
    \* Added by decision 124 (L10): in "txnforgedcommit" rows 0 and 1
    \* (sources 10 and 11) are the two rows of the transaction, and row 2
    \* (source 12) is the row of a host operation of the root.
    [] TxnBase = "txnforgedcommit" -> (IF r = 0 THEN 10 ELSE IF r = 1 THEN 11 ELSE 12)
    [] OTHER -> r

\* Added by Reified RSpaces (decision 90): the recorded candidate of a row
\* (the COMM of a fire, or the outcome of a store). A staged operation fits
\* a row when its candidate is the recorded one. The rows of a scenario have
\* distinct candidates, so in "txnshared" the two re-offers fire against two
\* waiters. In "txnswap" the two operations are identical.
\* @type: (Int) => Int;
Cand(r) == IF TxnBase = "txnswap" THEN 1 ELSE r

\* The recorded predecessors of each row: the previous row on each channel
\* of its footprint.
\* @type: (Int) => Set(Int);
Pred(r) ==
  CASE Scenario = "honest" -> (IF r = 1 THEN {0} ELSE IF r = 3 THEN {2} ELSE {})
    [] Scenario = "missing" -> (IF r \in {2, 4} THEN {1} ELSE {})
    [] Scenario = "inverted" -> (IF r = 1 THEN {0} ELSE IF r = 2 THEN {1} ELSE {})
    [] Scenario = "uncounted" -> (IF r = 1 THEN {0} ELSE {})
    [] Scenario = "samesource" -> (IF r = 1 THEN {0} ELSE IF r = 2 THEN {1} ELSE {})
    [] Scenario = "oversubscribed" -> (IF r = 1 THEN {0} ELSE {})
    [] Scenario = "pair" -> (IF r = 1 THEN {0} ELSE {})
    [] Scenario = "missingpair" -> (IF r = 2 THEN {1} ELSE {})
    [] Scenario = "chain" -> (IF r = 1 THEN {0} ELSE {})
    \* Added by Reified RSpaces (decision 90): each later row depends on the
    \* row before it. In "txnrace" the second row of the transaction depends
    \* on its first row, which depends on the host row. The rows of one
    \* source form a chain, as `select_slot` requires.
    [] TxnBase = "txnrace" -> (IF r = 1 THEN {0} ELSE IF r = 2 THEN {1} ELSE {})
    \* Changed by Reified RSpaces (decision 92): every row of a space
    \* transaction names S(sid), so each row of "txnshared" depends on the row
    \* before it, too.
    [] TxnScenario -> (IF r >= 1 THEN {r - 1} ELSE {})

\* The program of each task. An operation names a source, and replay picks
\* the row. "honest" and "samesource" complete every row. In "missing" task
\* 1 fails before it performs row 1, which rows 2 and 4 wait on. In
\* "inverted" the consume (row 1) waits on the produce of its own body (row
\* 0, task 3). In "uncounted" task 2 would perform row 0 from outside the
\* evaluation. In "oversubscribed" three identical tasks race for two rows.
\* The three small scenarios give each refutation a short counterexample:
\* "pair" (task 1 waits on the root's row), "missingpair" (task 1 waits on
\* row 1, which no task performs) and "chain" (task 1 spawns task 2 and waits
\* on its row).
\* @type: (Int) => Seq({kind: Str, arg: Int});
Prog(t) ==
  CASE Scenario = "honest" ->
         (IF t = 0 THEN <<Spawn(1), Spawn(2), Op(0)>>
          ELSE IF t = 1 THEN <<Op(1)>>
          ELSE IF t = 2 THEN <<Spawn(3), Op(3)>>
          ELSE IF t = 3 THEN <<Op(2)>>
          ELSE Nothing)
    [] Scenario = "missing" ->
         (IF t = 0 THEN <<Spawn(1), Spawn(2), Spawn(3), Spawn(4)>>
          ELSE IF t = 1 THEN <<Op(0), Fail>>
          ELSE IF t = 2 THEN <<Op(2)>>
          ELSE IF t = 3 THEN <<Op(3)>>
          ELSE IF t = 4 THEN <<Op(4)>>
          ELSE Nothing)
    [] Scenario = "inverted" ->
         (IF t = 0 THEN <<Spawn(1), Spawn(2)>>
          ELSE IF t = 1 THEN <<Op(1), Spawn(3)>>
          ELSE IF t = 2 THEN <<Op(2)>>
          ELSE IF t = 3 THEN <<Op(0)>>
          ELSE Nothing)
    [] Scenario = "uncounted" ->
         (IF t = 0 THEN <<Spawn(1)>>
          ELSE IF t = 1 THEN <<Op(1)>>
          ELSE IF t = External THEN <<Op(0)>>
          ELSE Nothing)
    [] Scenario = "samesource" ->
         (IF t = 0 THEN <<Spawn(1), Spawn(2), Spawn(3)>>
          ELSE IF t \in {1, 2} THEN <<Op(10)>>
          ELSE IF t = 3 THEN <<Op(11)>>
          ELSE Nothing)
    [] Scenario = "oversubscribed" ->
         (IF t = 0 THEN <<Spawn(1), Spawn(2), Spawn(3)>>
          ELSE IF t \in {1, 2, 3} THEN <<Op(10)>>
          ELSE Nothing)
    [] Scenario = "pair" ->
         (IF t = 0 THEN <<Spawn(1), Op(0)>>
          ELSE IF t = 1 THEN <<Op(1)>>
          ELSE Nothing)
    [] Scenario = "missingpair" ->
         (IF t = 0 THEN <<Spawn(1), Op(0)>>
          ELSE IF t = 1 THEN <<Op(2)>>
          ELSE Nothing)
    [] Scenario = "chain" ->
         (IF t = 0 THEN <<Spawn(1)>>
          ELSE IF t = 1 THEN <<Spawn(2), Op(1)>>
          ELSE IF t = 2 THEN <<Op(0)>>
          ELSE Nothing)
    \* Added by Reified RSpaces (decision 90): the root spawns the tasks. In
    \* "txnrace", "txnstale" and "txnforged" task 1 is a host operation on
    \* row 0 ("txnforged" has no such task, so row 0 never completes), and
    \* the other task runs one transaction.
    [] TxnBase \in {"txnrace", "txnstale"} ->
         (IF t = 0 THEN <<Spawn(1), Spawn(2)>>
          ELSE IF t = 1 THEN <<Op(0)>>
          ELSE IF t = 2 THEN <<TxnStep>>
          ELSE Nothing)
    [] TxnBase \in {"txnstripes", "txnshared", "txnswap"} ->
         (IF t = 0 THEN <<Spawn(1), Spawn(2)>>
          ELSE IF t \in {1, 2} THEN <<TxnStep>>
          ELSE Nothing)
    [] TxnBase \in {"txnrepeat", "txnforged"} ->
         (IF t = 0 THEN <<Spawn(1)>>
          ELSE IF t = 1 THEN <<TxnStep>>
          ELSE Nothing)
    \* Added by decision 124 (L10): in "txnforgedcommit" the root spawns task
    \* 1, which runs one transaction, and then performs row 2, which waits on
    \* row 1 of the transaction.
    [] TxnBase = "txnforgedcommit" ->
         (IF t = 0 THEN <<Spawn(1), Op(12)>>
          ELSE IF t = 1 THEN <<TxnStep>>
          ELSE Nothing)

\* Added by Reified RSpaces (decision 90): the staged operations of the
\* transaction of task `t`, each with its source and its candidate. In
\* "txnstale" the staging reads the store: before host row 0 completes it
\* stages a stale candidate (9).
\* @type: (Int) => Seq({src: Int, cand: Int});
TxnOps(t) ==
  CASE TxnBase = "txnrace" -> <<[src |-> 11, cand |-> 1], [src |-> 12, cand |-> 2]>>
    [] TxnBase = "txnstripes" ->
         (IF t = 1 THEN <<[src |-> 10, cand |-> 0]>> ELSE <<[src |-> 11, cand |-> 1]>>)
    [] TxnBase = "txnstale" ->
         (IF 0 \in done THEN <<[src |-> 11, cand |-> 1]>> ELSE <<[src |-> 11, cand |-> 9]>>)
    \* Changed by Reified RSpaces (decision 92): each transaction stages its
    \* anchor, then its re-offer.
    [] TxnBase = "txnshared" ->
         (IF t = 1 THEN <<[src |-> 20, cand |-> 0], [src |-> 10, cand |-> 1]>>
          ELSE <<[src |-> 21, cand |-> 2], [src |-> 10, cand |-> 3]>>)
    [] TxnBase = "txnswap" -> <<[src |-> 10, cand |-> 1]>>
    [] TxnBase = "txnrepeat" -> <<[src |-> 10, cand |-> 0], [src |-> 10, cand |-> 1]>>
    [] TxnBase = "txnforged" -> <<[src |-> 11, cand |-> 1]>>
    \* Added by decision 124 (L10): the record of "txnforgedcommit" is forged.
    \* The second staged operation selects row 1, whose recorded candidate (1)
    \* is not the staged one (9), so the commit fails at row 1.
    [] TxnBase = "txnforgedcommit" -> <<[src |-> 10, cand |-> 0], [src |-> 11, cand |-> 9]>>
    [] OTHER -> <<>>

\* Changed by Reified RSpaces (decision 90): every transaction scenario except
\* "txnforged" records an honest journal.
\* Completes == Scenario \in {"honest", "samesource", "pair", "chain"}
\* Changed by decision 124 (L10): "txnforgedcommit" records a forged journal
\* too.
Completes ==
  \/ Scenario \in {"honest", "samesource", "pair", "chain"}
  \/ (TxnScenario /\ TxnBase \notin {"txnforged", "txnforgedcommit"})

\* ---------------------------------------------------------------------
\* Helpers
\* ---------------------------------------------------------------------

\* `counted_task()`: the task runs inside the evaluation's scope. The
\* uncounted check and the parked count read this.
InScope(t) == t /= External

\* The live count includes the task. The root counts from the start unless
\* RootUncounted removes that safeguard.
LiveCounted(t) == IF t = 0 THEN ~RootUncounted ELSE InScope(t)

\* The progress word is armed while some task is live and no stall is
\* declared.
Armed == live > 0 /\ ~wStalled

\* `stalls(w)`: no flag, some live task, every live task parked.
Stalls(l, p, s) == ~s /\ l > 0 /\ p = l

Ready(r) == Pred(r) \subseteq done /\ ~boundary

\* `select_slot`: the first open row of a source, in journal order. A row is
\* open while it is neither completed nor reserved. None when no row is open.
Open(s) == {r \in UsedRows : Src(r) = s /\ r \notin done /\ r \notin reserved}
Known(s) == \E r \in UsedRows : Src(r) = s
Select(s) ==
  IF 0 \in Open(s) THEN 0
  ELSE IF 1 \in Open(s) THEN 1
  ELSE IF 2 \in Open(s) THEN 2
  ELSE IF 3 \in Open(s) THEN 3
  ELSE IF 4 \in Open(s) THEN 4
  ELSE None

Source(t) == Prog(t)[ip[t]].arg

\* The row that the task's next operation selects is ready now.
RowReady(t) ==
  /\ ip[t] <= Len(Prog(t))
  /\ Prog(t)[ip[t]].kind = "op"
  /\ Select(Source(t)) /= None
  /\ Ready(Select(Source(t)))

\* ---------------------------------------------------------------------
\* Added by Reified RSpaces (decision 90): the readiness of a transaction
\* ---------------------------------------------------------------------

\* The first row of a set, in journal order, or None.
\* @type: (Set(Int)) => Int;
FirstOf(S) ==
  IF 0 \in S THEN 0
  ELSE IF 1 \in S THEN 1
  ELSE IF 2 \in S THEN 2
  ELSE IF 3 \in S THEN 3
  ELSE IF 4 \in S THEN 4
  ELSE None

\* The open rows of source `s` that the transaction has not taken yet.
\* Changed by Reified RSpaces (decision 92): the control "txnrepeatnorepeat"
\* is disabled.
\* @type: (Int, Set(Int)) => Set(Int);
OpenExcept(s, taken) ==
  {r \in UsedRows : Src(r) = s /\ r \notin done /\ r \notin reserved /\ r \notin taken}

\* The simulated selection of the staged operations of task `t`, in staging
\* order. A transaction stages at most two operations.
\* Changed by Reified RSpaces (decision 92): the selection that the commit
\* makes, one row after the other (`begin_row`). The first element is the row
\* that the readiness check reads.
\* @type: (Int) => Seq(Int);
TxnSel(t) ==
  LET ops == TxnOps(t)
      first == FirstOf(OpenExcept(ops[1].src, {}))
  IN IF Len(ops) = 1 THEN <<first>>
     ELSE <<first, FirstOf(OpenExcept(ops[2].src, {first}))>>

\* Each staged operation selects a row, and the row fits it.
\* Changed by Reified RSpaces (decision 92): a check of the commit.
TxnFits(t) ==
  LET ops == TxnOps(t)
      rows == TxnSel(t)
  IN \A i \in 1..Len(ops) : rows[i] /= None /\ Cand(rows[i]) = ops[i].cand

\* Changed by Reified RSpaces (decision 92): a check of the commit, and the
\* control "txnracenointxn" is disabled.
\* Each predecessor of a selected row is complete or an earlier row of the
\* transaction.
TxnPredsOK(t) ==
  LET rows == TxnSel(t)
  IN \A i \in 1..Len(rows) :
       Pred(rows[i]) \subseteq (done \cup {rows[j] : j \in 1..(i - 1)})

\* Changed by Reified RSpaces (decision 92): the check reads the first row
\* only.
\* The transaction of task `t` is ready now: the first open row of its first
\* source exists, and the predecessors of that row are complete.
TxnReady(t) ==
  LET first == TxnSel(t)[1]
  IN ~boundary /\ first /= None /\ Pred(first) \subseteq done

\* Added by Reified RSpaces (decision 92): the commit of a ready transaction
\* passes. Each staged operation selects a row that fits it, and each
\* predecessor is complete or an earlier row of the transaction. Otherwise
\* `begin_row` or the fit check fails closed at the first row that does not
\* pass.
TxnCommits(t) == TxnFits(t) /\ TxnPredsOK(t)

\* The task's next step is a transaction.
IsTxnTask(t) == ip[t] <= Len(Prog(t)) /\ Prog(t)[ip[t]].kind = "txn"

\* The next operation or transaction of the task is ready now.
TaskReady(t) == IF IsTxnTask(t) THEN TxnReady(t) ELSE RowReady(t)

\* Changed by Reified RSpaces (decision 90): a parked transaction waits too.
\* WaitsOnReady(t) == pc[t] = "parked" /\ RowReady(t)
\* A parked waiter whose row is ready now.
WaitsOnReady(t) ==
  \/ pc[t] = "parked" /\ RowReady(t)
  \/ pc[t] = "tparked" /\ TxnReady(t)

\* A task that can still change the replay state: it runs a step, holds a
\* ready check or a reservation, publishes, or waits on a row that is ready
\* now. A waiter that wakes with no change and checks a row that is not ready
\* only parks again, so it is not progress.
\* Changed by Reified RSpaces (decision 90): a transaction task makes
\* progress when it locks a free stripe, checks, releases, publishes or
\* notifies, or when its transaction is ready.
CanProgress(u) ==
  /\ (InScope(u) \/ AdmitUncounted)
  /\ \/ pc[u] \in {"run", "born", "reserve", "publish", "notify"}
     \/ (pc[u] \in {"check", "parking", "registering", "parked"} /\ RowReady(u))
     \/ pc[u] \in {"tcheck", "trelease", "tpublish", "tnotify"}
     \/ (pc[u] = "tlock" /\ (stripe = None \/ closed))
     \/ (pc[u] \in {"tpark", "tparked"} /\ TxnReady(u))

\* A stall declared now is false when some task other than the declarer can
\* still make progress, the declarer's own row is ready (a park counted from
\* a stale check), or a boundary will end.
\* Changed by Reified RSpaces (decision 90): or the declarer's own
\* transaction is ready.
FalseStallNow(t) ==
  \/ \E u \in Tasks \ {t} : CanProgress(u)
  \/ (pc[t] \in {"check", "parking"} /\ RowReady(t))
  \/ (pc[t] = "tpark" /\ TxnReady(t))
  \/ boundary

NotifyAll == [u \in Tasks |-> IF ntf[u] = "registered" THEN "notified" ELSE ntf[u]]

FreshNotification == IF NotifiedAfterCheck THEN "none" ELSE "registered"

\* ---------------------------------------------------------------------
\* Initial state
\* ---------------------------------------------------------------------

Init ==
  /\ pc = [t \in Tasks |->
             IF t = 0 \/ t = External THEN "run"
             ELSE "unborn"]
  /\ ip = [t \in Tasks |-> 1]
  /\ sel = [t \in Tasks |-> None]
  /\ reg = [t \in Tasks |-> None]
  /\ ntf = [t \in Tasks |-> "none"]
  /\ err = [t \in Tasks |-> FALSE]
  /\ done = {}
  /\ reserved = {}
  /\ version = 0
  /\ closed = FALSE
  /\ boundary = FALSE
  /\ boundaries = 0
  /\ live = IF RootUncounted THEN 0 ELSE 1
  /\ parked = 0
  /\ wStalled = FALSE
  /\ falseStall = FALSE
  /\ freshLeave = FALSE
  \* Added by Reified RSpaces (decision 90).
  /\ stripe = None
  /\ tsel = [t \in Tasks |-> <<>>]
  /\ tpos = [t \in Tasks |-> 1]
  /\ seen = [t \in Tasks |-> None]

\* ---------------------------------------------------------------------
\* Actions
\* ---------------------------------------------------------------------

\* A task runs its next program step.
Run(t) ==
  /\ pc[t] = "run"
  /\ IF ip[t] > Len(Prog(t))
       THEN /\ pc' = [pc EXCEPT ![t] = "end"]
            /\ UNCHANGED <<ip, sel, reg, ntf, err, done, reserved, version,
                           closed, boundary, boundaries, live, parked,
                           wStalled, falseStall, freshLeave>>
       ELSE
         LET step == Prog(t)[ip[t]] IN
         CASE step.kind = "spawn" ->
                \* `enter` runs in the parent before the spawn.
                /\ pc' = [pc EXCEPT ![step.arg] =
                            IF CountInChild THEN "born" ELSE "run"]
                /\ ip' = [ip EXCEPT ![t] = @ + 1]
                /\ live' = IF CountInChild THEN live ELSE live + 1
                /\ UNCHANGED <<sel, reg, ntf, err, done, reserved, version,
                               closed, boundary, boundaries, parked,
                               wStalled, falseStall, freshLeave>>
           [] step.kind = "fail" ->
                /\ pc' = [pc EXCEPT ![t] = "end"]
                /\ err' = [err EXCEPT ![t] = TRUE]
                /\ UNCHANGED <<ip, sel, reg, ntf, done, reserved, version,
                               closed, boundary, boundaries, live, parked,
                               wStalled, falseStall, freshLeave>>
           [] step.kind = "op" ->
                IF ~InScope(t) /\ Armed /\ ~AdmitUncounted
                  THEN \* `Uncounted`: an operation from outside the
                       \* evaluation is refused while the evaluation is
                       \* armed.
                       /\ pc' = [pc EXCEPT ![t] = "end"]
                       /\ err' = [err EXCEPT ![t] = TRUE]
                       /\ UNCHANGED <<ip, sel, reg, ntf, done, reserved,
                                      version, closed, boundary, boundaries,
                                      live, parked, wStalled, falseStall,
                                      freshLeave>>
                  ELSE \* `notified()` registers before the check.
                       /\ pc' = [pc EXCEPT ![t] = "check"]
                       /\ ntf' = [ntf EXCEPT ![t] = FreshNotification]
                       /\ UNCHANGED <<ip, sel, reg, err, done, reserved,
                                      version, closed, boundary, boundaries,
                                      live, parked, wStalled, falseStall,
                                      freshLeave>>
           \* Added by Reified RSpaces (decision 90): a space transaction
           \* first takes its stripes.
           [] step.kind = "txn" ->
                /\ pc' = [pc EXCEPT ![t] = "tlock"]
                /\ UNCHANGED <<ip, sel, reg, ntf, err, done, reserved, version,
                               closed, boundary, boundaries, live, parked,
                               wStalled, falseStall, freshLeave>>

\* CountInChild only: the child counts itself when it first runs.
Born(t) ==
  /\ pc[t] = "born"
  /\ pc' = [pc EXCEPT ![t] = "run"]
  /\ live' = live + 1
  /\ UNCHANGED <<ip, sel, reg, ntf, err, done, reserved, version, closed,
                 boundary, boundaries, parked, wStalled, falseStall,
                 freshLeave>>

\* The task leaves its registration and ends with an error.
LeaveAndFail(t) ==
  /\ freshLeave' = (freshLeave \/ reg[t] = version)
  /\ parked' = IF reg[t] = version THEN parked - 1 ELSE parked
  /\ reg' = [reg EXCEPT ![t] = None]
  /\ ntf' = [ntf EXCEPT ![t] = "none"]
  /\ pc' = [pc EXCEPT ![t] = "end"]
  /\ err' = [err EXCEPT ![t] = TRUE]
  /\ UNCHANGED <<ip, sel, done, reserved, version, closed, boundary,
                 boundaries, live, wStalled, falseStall>>

\* The stall declaration: under the state lock, set the flags, close the
\* replay, bump the version and clear the parked count.
DeclareStall(t, next) ==
  /\ falseStall' = (falseStall \/ FalseStallNow(t))
  /\ wStalled' = TRUE
  /\ closed' = TRUE
  /\ version' = version + 1
  /\ parked' = 0
  /\ pc' = [pc EXCEPT ![t] = next]

\* Count the waiter as parked at the current version, and declare a stall
\* when every live task is parked.
CountPark(t, next) ==
  LET counts == SkipVersion \/ reg[t] /= version
      p == IF counts THEN parked + 1 ELSE parked
  IN
  IF counts /\ Stalls(live, p, wStalled)
    THEN /\ DeclareStall(t, "stallnotify")
         /\ reg' = [reg EXCEPT ![t] = None]
         /\ UNCHANGED <<ip, sel, ntf, err, done, reserved, boundary,
                        boundaries, live, freshLeave>>
    ELSE /\ parked' = p
         /\ reg' = [reg EXCEPT ![t] = version]
         /\ pc' = [pc EXCEPT ![t] = next]
         /\ UNCHANGED <<ip, sel, ntf, err, done, reserved, version, closed,
                        boundary, boundaries, live, wStalled, falseStall,
                        freshLeave>>

\* `wait_ready`: the readiness check under the state lock.
Check(t) ==
  /\ pc[t] = "check"
  /\ LET s == Source(t)
         r == Select(s)
         after == IF NotifiedAfterCheck THEN "registering" ELSE "parked"
     IN
     IF closed \/ ~Known(s) \/ r = None
       THEN LeaveAndFail(t)
     ELSE IF Ready(r)
       THEN /\ freshLeave' = (freshLeave \/ reg[t] = version)
            /\ parked' = IF reg[t] = version THEN parked - 1 ELSE parked
            /\ reg' = [reg EXCEPT ![t] = None]
            /\ ntf' = [ntf EXCEPT ![t] = "none"]
            /\ pc' = [pc EXCEPT ![t] = "reserve"]
            /\ UNCHANGED <<ip, sel, err, done, reserved, version, closed,
                           boundary, boundaries, live, wStalled, falseStall>>
     ELSE IF ~InScope(t)
       THEN /\ pc' = [pc EXCEPT ![t] = after]
            /\ UNCHANGED <<ip, sel, reg, ntf, err, done, reserved, version,
                           closed, boundary, boundaries, live, parked,
                           wStalled, falseStall, freshLeave>>
     ELSE IF ParkAfterUnlock
       THEN /\ pc' = [pc EXCEPT ![t] = "parking"]
            /\ UNCHANGED <<ip, sel, reg, ntf, err, done, reserved, version,
                           closed, boundary, boundaries, live, parked,
                           wStalled, falseStall, freshLeave>>
     ELSE CountPark(t, after)

\* ParkAfterUnlock only: the park is counted after the lock is released.
ApplyPark(t) ==
  /\ pc[t] = "parking"
  /\ CountPark(t, "parked")

\* NotifiedAfterCheck only: the notification registers after the check.
Register(t) ==
  /\ pc[t] = "registering"
  /\ ntf' = [ntf EXCEPT ![t] = "registered"]
  /\ pc' = [pc EXCEPT ![t] = "parked"]
  /\ UNCHANGED <<ip, sel, reg, err, done, reserved, version, closed,
                 boundary, boundaries, live, parked, wStalled, falseStall,
                 freshLeave>>

\* A notified waiter wakes, registers a new notification and checks again.
Wake(t) ==
  /\ pc[t] = "parked"
  /\ ntf[t] = "notified"
  /\ pc' = [pc EXCEPT ![t] = "check"]
  /\ ntf' = [ntf EXCEPT ![t] = FreshNotification]
  /\ UNCHANGED <<ip, sel, reg, err, done, reserved, version, closed,
                 boundary, boundaries, live, parked, wStalled, falseStall,
                 freshLeave>>

\* `reserve_ready`: select again under the lock and reserve a ready row.
\* A row that is not ready sends the task back to `wait_ready`.
Reserve(t) ==
  /\ pc[t] = "reserve"
  /\ LET s == Source(t)
         r == Select(s)
     IN
     IF closed \/ r = None
       THEN LeaveAndFail(t)
     ELSE IF Ready(r)
       THEN /\ reserved' = reserved \cup {r}
            /\ sel' = [sel EXCEPT ![t] = r]
            /\ pc' = [pc EXCEPT ![t] = "publish"]
            /\ UNCHANGED <<ip, reg, ntf, err, done, version, closed, boundary,
                           boundaries, live, parked, wStalled, falseStall,
                           freshLeave>>
       ELSE /\ pc' = [pc EXCEPT ![t] = "check"]
            /\ ntf' = [ntf EXCEPT ![t] = FreshNotification]
            /\ UNCHANGED <<ip, sel, reg, err, done, reserved, version, closed,
                           boundary, boundaries, live, parked, wStalled,
                           falseStall, freshLeave>>

\* A publish under the state lock: complete the row, bump the version and
\* clear the parked count.
Publish(t) ==
  /\ pc[t] = "publish"
  /\ done' = done \cup {sel[t]}
  /\ reserved' = reserved \ {sel[t]}
  /\ version' = version + 1
  /\ parked' = IF SkipReset \/ ResetAfterUnlock THEN parked ELSE 0
  /\ pc' = [pc EXCEPT ![t] = "notify"]
  /\ UNCHANGED <<ip, sel, reg, ntf, err, closed, boundary, boundaries, live,
                 wStalled, falseStall, freshLeave>>

\* `notify_waiters` after the unlock.
Notify(t) ==
  /\ pc[t] = "notify"
  /\ ntf' = NotifyAll
  /\ parked' = IF ResetAfterUnlock THEN 0 ELSE parked
  /\ ip' = [ip EXCEPT ![t] = @ + 1]
  /\ sel' = [sel EXCEPT ![t] = None]
  /\ pc' = [pc EXCEPT ![t] = "run"]
  /\ UNCHANGED <<reg, err, done, reserved, version, closed, boundary,
                 boundaries, live, wStalled, falseStall, freshLeave>>

\* The declarer of a park stall notifies, then returns its error.
StallNotify(t) ==
  /\ pc[t] = "stallnotify"
  /\ ntf' = NotifyAll
  /\ pc' = [pc EXCEPT ![t] = "end"]
  /\ err' = [err EXCEPT ![t] = TRUE]
  /\ UNCHANGED <<ip, sel, reg, done, reserved, version, closed, boundary,
                 boundaries, live, parked, wStalled, falseStall, freshLeave>>

\* `finish`: the live token drops when the task's program ends.
End(t) ==
  /\ pc[t] = "end"
  /\ IF LiveCounted(t)
       THEN /\ live' = live - 1
            /\ pc' = [pc EXCEPT ![t] =
                        IF Stalls(live - 1, parked, wStalled) THEN "declare"
                        ELSE "ended"]
       ELSE /\ pc' = [pc EXCEPT ![t] = "ended"]
            /\ UNCHANGED live
  /\ UNCHANGED <<ip, sel, reg, ntf, err, done, reserved, version, closed,
                 boundary, boundaries, parked, wStalled, falseStall,
                 freshLeave>>

\* The finish that detected a stall declares it under the lock.
Declare(t) ==
  /\ pc[t] = "declare"
  /\ IF wStalled
       THEN /\ pc' = [pc EXCEPT ![t] = "ended"]
            /\ UNCHANGED <<ntf, version, closed, parked, wStalled, falseStall>>
       ELSE /\ DeclareStall(t, "ended")
            /\ ntf' = NotifyAll
  /\ UNCHANGED <<ip, sel, reg, err, done, reserved, boundary, boundaries,
                 live, freshLeave>>

\* The environment opens one boundary (checkpoint, restore, export). The
\* safe protocol refuses it while the evaluation is armed.
OpenBoundary ==
  /\ boundaries = 0
  /\ ~boundary
  /\ reserved = {}
  /\ (BoundaryWhileArmed \/ ~Armed)
  /\ boundary' = TRUE
  /\ boundaries' = 1
  /\ UNCHANGED <<pc, ip, sel, reg, ntf, err, done, reserved, version, closed,
                 live, parked, wStalled, falseStall, freshLeave>>

\* The end of a boundary is a change: bump the version, clear the parked
\* count and notify.
CloseBoundary ==
  /\ boundary
  /\ boundary' = FALSE
  /\ version' = version + 1
  /\ parked' = 0
  /\ ntf' = NotifyAll
  /\ UNCHANGED <<pc, ip, sel, reg, err, done, reserved, closed, boundaries,
                 live, wStalled, falseStall, freshLeave>>

\* ---------------------------------------------------------------------
\* Added by Reified RSpaces (decision 90): the space transaction protocol
\* ---------------------------------------------------------------------

\* Changed by Reified RSpaces (decision 90): the code registers the
\* notification of a transaction in its park (`TxnPark`), after it releases
\* its stripes, so the lock registers nothing.
\* The transaction takes the stripe before its check. After the epoch closes
\* it fails without the stripe.
TxnLock(t) ==
  /\ pc[t] = "tlock"
  /\ IF closed
       THEN /\ LeaveAndFail(t)
            /\ UNCHANGED txnVars
       ELSE \* "txnstripesunderstripes" wakes a transaction that kept it.
            /\ stripe \in {None, t}
            /\ stripe' = t
            /\ pc' = [pc EXCEPT ![t] = "tcheck"]
            /\ UNCHANGED <<ip, sel, reg, ntf, err, done, reserved, version,
                           closed, boundary, boundaries, live, parked,
                           wStalled, falseStall, freshLeave, tsel, tpos, seen>>

\* Added by Reified RSpaces (decision 90): the park of a transaction
\* registers its notification and counts the park in one critical section
\* (`CountPark` with the registration). The code enables the notification
\* just before it takes the state lock. A change between the two bumps the
\* version, so the transaction sees it and retries (`TxnPark`).
TxnCountPark(t) ==
  LET counts == SkipVersion \/ reg[t] /= version
      p == IF counts THEN parked + 1 ELSE parked
  IN
  IF counts /\ Stalls(live, p, wStalled)
    THEN /\ DeclareStall(t, "stallnotify")
         /\ reg' = [reg EXCEPT ![t] = None]
         /\ UNCHANGED <<ip, sel, ntf, err, done, reserved, boundary,
                        boundaries, live, freshLeave>>
    ELSE /\ parked' = p
         /\ reg' = [reg EXCEPT ![t] = version]
         /\ ntf' = IF TxnRegisterLate THEN ntf ELSE [ntf EXCEPT ![t] = "registered"]
         /\ pc' = [pc EXCEPT ![t] = IF TxnRegisterLate THEN "tregistering" ELSE "tparked"]
         /\ UNCHANGED <<ip, sel, err, done, reserved, version, closed,
                        boundary, boundaries, live, wStalled, falseStall,
                        freshLeave>>

\* "txnracefailclosed" (the code before decision 90): a transaction whose rows
\* are not ready fails, and the failure invalidates the epoch. The
\* invalidation is a change: it closes the replay, bumps the version, clears
\* the parked count and notifies every waiter.
\* Changed by decision 124 (L10): a ready transaction whose commit does not
\* pass leaves and fails with the base action LeaveAndFail (`TxnCheck`), so
\* this action serves the control TxnFailClosed ("txnracefailclosed") only.
TxnFailAndClose(t) ==
  /\ closed' = TRUE
  /\ version' = version + 1
  /\ parked' = 0
  /\ ntf' = [NotifyAll EXCEPT ![t] = "none"]
  /\ freshLeave' = (freshLeave \/ reg[t] = version)
  /\ reg' = [reg EXCEPT ![t] = None]
  /\ err' = [err EXCEPT ![t] = TRUE]
  /\ pc' = [pc EXCEPT ![t] = "end"]
  /\ stripe' = None
  /\ UNCHANGED <<ip, sel, done, reserved, boundary, boundaries, live, wStalled,
                 falseStall, tsel, tpos, seen>>

\* The readiness check of the whole transaction under the state lock, while
\* the transaction holds its stripe.
TxnCheck(t) ==
  /\ pc[t] = "tcheck"
  /\ IF closed
       THEN /\ LeaveAndFail(t)
            /\ stripe' = None
            /\ UNCHANGED <<tsel, tpos, seen>>
     \* Changed by Reified RSpaces (decision 92): a ready transaction whose
     \* commit does not pass fails closed at commit.
     ELSE IF TxnReady(t) /\ TxnCommits(t)
       THEN \* Reserve every selected row and leave the registration.
            /\ freshLeave' = (freshLeave \/ reg[t] = version)
            /\ parked' = IF reg[t] = version THEN parked - 1 ELSE parked
            /\ reg' = [reg EXCEPT ![t] = None]
            /\ ntf' = [ntf EXCEPT ![t] = "none"]
            /\ reserved' = reserved \cup {TxnSel(t)[i] : i \in 1..Len(TxnSel(t))}
            /\ tsel' = [tsel EXCEPT ![t] = TxnSel(t)]
            /\ tpos' = [tpos EXCEPT ![t] = 1]
            /\ pc' = [pc EXCEPT ![t] = "tpublish"]
            /\ UNCHANGED <<ip, sel, err, done, version, closed, boundary,
                           boundaries, live, wStalled, falseStall, stripe, seen>>
     \* Added by decision 124 (L10): a ready transaction whose commit does not
     \* pass leaves its registration and fails with the base action
     \* LeaveAndFail, and it releases its stripe, as the closed branch above
     \* does. It does not close the epoch.
     \* Added by decision 124 (L10): the scenario "txnforgedcommit" takes this
     \* branch (gate rows NativeReplayStallTxnForgedCommit and
     \* NativeReplayStallTxnForgedCommitVerdictReachable).
     ELSE IF TxnReady(t)
       THEN /\ LeaveAndFail(t)
            /\ stripe' = None
            /\ UNCHANGED <<tsel, tpos, seen>>
     \* Changed by Reified RSpaces (decision 92): the controls of a mismatch
     \* are disabled.
     ELSE IF TxnFailClosed
       THEN TxnFailAndClose(t)
     ELSE IF TxnParkUnderStripes
       THEN \* The park keeps the stripe.
            \* Changed by Reified RSpaces (decision 90): the park registers
            \* the notification (`TxnCountPark`).
            /\ seen' = [seen EXCEPT ![t] = version]
            /\ TxnCountPark(t)
            /\ UNCHANGED <<stripe, tsel, tpos>>
     ELSE \* NotReady: note the version, then release the stripe first.
          /\ seen' = [seen EXCEPT ![t] = version]
          /\ pc' = [pc EXCEPT ![t] = "trelease"]
          /\ UNCHANGED <<ip, sel, reg, ntf, err, done, reserved, version, closed,
                         boundary, boundaries, live, parked, wStalled,
                         falseStall, freshLeave, stripe, tsel, tpos>>

\* The transaction releases its stripe before it parks.
TxnRelease(t) ==
  /\ pc[t] = "trelease"
  /\ stripe' = None
  /\ pc' = [pc EXCEPT ![t] = "tpark"]
  /\ UNCHANGED <<ip, sel, reg, ntf, err, done, reserved, version, closed,
                 boundary, boundaries, live, parked, wStalled, falseStall,
                 freshLeave, tsel, tpos, seen>>

\* Changed by Reified RSpaces (decision 90): the park registers the
\* notification itself (`TxnCountPark`), so a change before the registration
\* shows as a new version and a change after it notifies.
\* The transaction registers its notification, then, under the state lock, a
\* change since the check retries at once, and otherwise the transaction
\* parks as a counted waiter.
TxnPark(t) ==
  /\ pc[t] = "tpark"
  /\ IF closed
       THEN /\ LeaveAndFail(t)
            /\ UNCHANGED txnVars
     ELSE IF version /= seen[t]
       THEN /\ pc' = [pc EXCEPT ![t] = "tlock"]
            /\ ntf' = [ntf EXCEPT ![t] = "none"]
            /\ UNCHANGED <<ip, sel, reg, err, done, reserved, version, closed,
                           boundary, boundaries, live, parked, wStalled,
                           falseStall, freshLeave, stripe, tsel, tpos, seen>>
     ELSE /\ TxnCountPark(t)
          /\ UNCHANGED txnVars

\* A notified transaction wakes and takes the stripe again.
TxnWake(t) ==
  /\ pc[t] = "tparked"
  /\ ntf[t] = "notified"
  /\ pc' = [pc EXCEPT ![t] = "tlock"]
  /\ ntf' = [ntf EXCEPT ![t] = "none"]
  /\ UNCHANGED <<ip, sel, reg, err, done, reserved, version, closed, boundary,
                 boundaries, live, parked, wStalled, falseStall, freshLeave,
                 stripe, tsel, tpos, seen>>

\* Added by Reified RSpaces (decision 90): "txnraceregisterlate" registers
\* the notification after the park, outside the critical section, so a
\* change between the two notifies nothing.
TxnRegister(t) ==
  /\ pc[t] = "tregistering"
  /\ ntf' = [ntf EXCEPT ![t] = "registered"]
  /\ pc' = [pc EXCEPT ![t] = "tparked"]
  /\ UNCHANGED <<ip, sel, reg, err, done, reserved, version, closed, boundary,
                 boundaries, live, parked, wStalled, falseStall, freshLeave,
                 stripe, tsel, tpos, seen>>

\* The transaction publishes its rows in staging order. Each publication is a
\* change under the state lock.
TxnPublish(t) ==
  /\ pc[t] = "tpublish"
  /\ LET r == tsel[t][tpos[t]] IN
       /\ done' = done \cup {r}
       /\ reserved' = reserved \ {r}
  /\ version' = version + 1
  /\ parked' = 0
  /\ IF tpos[t] = Len(tsel[t])
       THEN /\ pc' = [pc EXCEPT ![t] = "tnotify"]
            /\ UNCHANGED tpos
       ELSE /\ tpos' = [tpos EXCEPT ![t] = @ + 1]
            /\ UNCHANGED pc
  /\ UNCHANGED <<ip, sel, reg, ntf, err, closed, boundary, boundaries, live,
                 wStalled, falseStall, freshLeave, stripe, tsel, seen>>

\* The commit ends: release the stripe and notify the waiters.
TxnNotify(t) ==
  /\ pc[t] = "tnotify"
  /\ ntf' = NotifyAll
  /\ stripe' = None
  /\ ip' = [ip EXCEPT ![t] = @ + 1]
  /\ tsel' = [tsel EXCEPT ![t] = <<>>]
  /\ pc' = [pc EXCEPT ![t] = "run"]
  /\ UNCHANGED <<sel, reg, err, done, reserved, version, closed, boundary,
                 boundaries, live, parked, wStalled, falseStall, freshLeave,
                 tpos, seen>>

\* Changed by Reified RSpaces (decision 90): the actions of the base model
\* leave the transaction variables unchanged, and the transaction actions
\* join them.
\* Next ==
\*   \/ \E t \in Tasks :
\*        \/ Run(t) \/ Born(t) \/ Check(t) \/ ApplyPark(t) \/ Register(t)
\*        \/ Wake(t) \/ Reserve(t) \/ Publish(t) \/ Notify(t)
\*        \/ StallNotify(t) \/ End(t) \/ Declare(t)
\*   \/ OpenBoundary
\*   \/ CloseBoundary
BaseNext ==
  \/ \E t \in Tasks :
       \/ Run(t) \/ Born(t) \/ Check(t) \/ ApplyPark(t) \/ Register(t)
       \/ Wake(t) \/ Reserve(t) \/ Publish(t) \/ Notify(t)
       \/ StallNotify(t) \/ End(t) \/ Declare(t)
  \/ OpenBoundary
  \/ CloseBoundary

TxnNext ==
  \E t \in Tasks :
    \/ TxnLock(t) \/ TxnCheck(t) \/ TxnRelease(t) \/ TxnPark(t)
    \/ TxnWake(t) \/ TxnPublish(t) \/ TxnNotify(t) \/ TxnRegister(t)

Next ==
  \/ BaseNext /\ UNCHANGED txnVars
  \/ TxnNext

Spec == Init /\ [][Next]_vars

\* ---------------------------------------------------------------------
\* Invariants
\* ---------------------------------------------------------------------

\* Changed by Reified RSpaces (decision 90): the transaction states and
\* variables.
TypeOK ==
  /\ pc \in [Tasks -> {"unborn", "born", "run", "check", "parking",
                       "registering", "parked", "reserve", "publish",
                       "notify", "stallnotify", "end", "declare", "ended",
                       "tlock", "tcheck", "trelease", "tpark", "tparked",
                       "tpublish", "tnotify", "tregistering"}]
  /\ stripe \in Tasks \cup {None}
  /\ \A t \in Tasks : \A i \in 1..Len(tsel[t]) : tsel[t][i] \in Rows
  /\ tpos \in [Tasks -> 1..3]
  /\ seen \in [Tasks -> {None} \cup (0..100)]
  /\ ip \in [Tasks -> 1..5]
  /\ sel \in [Tasks -> Rows \cup {None}]
  /\ DOMAIN reg = Tasks
  /\ \A t \in Tasks : reg[t] = None \/ reg[t] >= 0
  /\ ntf \in [Tasks -> {"none", "registered", "notified"}]
  /\ err \in [Tasks -> BOOLEAN]
  /\ done \subseteq Rows
  /\ reserved \subseteq Rows
  /\ version >= 0
  /\ closed \in BOOLEAN
  /\ boundary \in BOOLEAN
  /\ boundaries \in 0..1
  /\ live >= 0
  /\ parked >= 0
  /\ wStalled \in BOOLEAN
  /\ falseStall \in BOOLEAN
  /\ freshLeave \in BOOLEAN

\* The rule never declares a stall while progress is still possible.
NoFalseStall == ~falseStall

\* A waiter leaves only a stale registration on the ready and error paths.
NoFreshLeave == ~freshLeave

\* I1: live counts the tasks of the evaluation that exist.
LiveExact ==
  live = Cardinality({t \in Tasks :
                        InScope(t) /\ pc[t] \notin {"unborn", "ended", "declare"}})

\* I2: parked counts the waiters registered at the current version.
CountExact == parked = Cardinality({t \in Tasks : reg[t] = version})

\* I3: a waiter counted at the current version has a row that is not ready.
\* Changed by Reified RSpaces (decision 90): a parked transaction too.
CountedAreBlocked ==
  \A t \in Tasks : (reg[t] = version /\ pc[t] \in {"parked", "tparked"}) => ~WaitsOnReady(t)

\* No lost wakeup: a parked waiter whose row is ready is notified, or a
\* notification is still pending.
\* Changed by Reified RSpaces (decision 90): a transaction that publishes or
\* notifies still has its notification pending.
NoLostWakeup ==
  \A t \in Tasks :
    WaitsOnReady(t) =>
      \/ ntf[t] = "notified"
      \/ \E u \in Tasks : pc[u] \in {"notify", "stallnotify", "declare",
                                     "tpublish", "tnotify"}
      \/ boundary

\* Added by Reified RSpaces (decision 90): a task that waits for the stripe
\* of a parked transaction, which nothing notifies.
BlockedOnStripe(t) ==
  /\ pc[t] = "tlock"
  /\ ~closed
  /\ stripe \notin {None, t}
  /\ pc[stripe] \in {"parked", "tparked"}
  /\ ntf[stripe] /= "notified"

\* No undetected hang: when every live task is parked and not notified, and
\* nothing else can act, a stall is declared or a declaration is pending.
\* Changed by Reified RSpaces (decision 90): a parked transaction, and a task
\* that waits for the stripe of one, can act no more either.
Quiet ==
  /\ live > 0
  /\ ~boundary
  /\ \A t \in Tasks :
       \/ pc[t] \in {"unborn", "ended", "end", "declare"}
       \/ (pc[t] \in {"parked", "tparked"} /\ ntf[t] /= "notified")
       \/ BlockedOnStripe(t)
       \/ (~InScope(t) /\ ~AdmitUncounted)
NoHang == Quiet => (wStalled \/ \E t \in Tasks : pc[t] \in {"end", "declare"})

\* The verdict: once every task has ended, an honest replay completes every
\* row with no stall and no error. A forged one is rejected.
AllEnded == \A t \in Tasks : pc[t] \in {"unborn", "ended"}
Rejected == wStalled \/ \E t \in Tasks : err[t]
Verdict ==
  AllEnded =>
    IF Completes THEN ~Rejected /\ done = UsedRows ELSE Rejected

\* Reachability witnesses. A configuration expects each one to be violated,
\* so Verdict does not hold only because no task ever ends: an honest replay
\* completes every row, and a forged one ends in a declared stall.
NoHonestCompletion == ~(AllEnded /\ done = UsedRows /\ ~Rejected)
NoStallVerdict == ~(AllEnded /\ wStalled)
=============================================================================
