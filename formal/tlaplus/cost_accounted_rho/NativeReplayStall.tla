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
  freshLeave

vars == <<pc, ip, sel, reg, ntf, err, done, reserved, version, closed,
          boundary, boundaries, live, parked, wStalled, falseStall,
          freshLeave>>

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

UsedRows == {r \in Rows : r < RowCount}

\* The recorded source of each row. Rows of one source share their channel.
\* In "samesource" rows 0 and 2 have source 10 and row 1 has source 11, all
\* on one channel. In "oversubscribed" rows 0 and 1 have source 10.
Src(r) ==
  CASE Scenario = "samesource" -> (IF r = 1 THEN 11 ELSE 10)
    [] Scenario = "oversubscribed" -> 10
    [] OTHER -> r

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

Completes == Scenario \in {"honest", "samesource", "pair", "chain"}

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

\* A parked waiter whose row is ready now.
WaitsOnReady(t) == pc[t] = "parked" /\ RowReady(t)

\* A task that can still change the replay state: it runs a step, holds a
\* ready check or a reservation, publishes, or waits on a row that is ready
\* now. A waiter that wakes with no change and checks a row that is not ready
\* only parks again, so it is not progress.
CanProgress(u) ==
  /\ (InScope(u) \/ AdmitUncounted)
  /\ \/ pc[u] \in {"run", "born", "reserve", "publish", "notify"}
     \/ (pc[u] \in {"check", "parking", "registering", "parked"} /\ RowReady(u))

\* A stall declared now is false when some task other than the declarer can
\* still make progress, the declarer's own row is ready (a park counted from
\* a stale check), or a boundary will end.
FalseStallNow(t) ==
  \/ \E u \in Tasks \ {t} : CanProgress(u)
  \/ (pc[t] \in {"check", "parking"} /\ RowReady(t))
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

Next ==
  \/ \E t \in Tasks :
       \/ Run(t) \/ Born(t) \/ Check(t) \/ ApplyPark(t) \/ Register(t)
       \/ Wake(t) \/ Reserve(t) \/ Publish(t) \/ Notify(t)
       \/ StallNotify(t) \/ End(t) \/ Declare(t)
  \/ OpenBoundary
  \/ CloseBoundary

Spec == Init /\ [][Next]_vars

\* ---------------------------------------------------------------------
\* Invariants
\* ---------------------------------------------------------------------

TypeOK ==
  /\ pc \in [Tasks -> {"unborn", "born", "run", "check", "parking",
                       "registering", "parked", "reserve", "publish",
                       "notify", "stallnotify", "end", "declare", "ended"}]
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
CountedAreBlocked ==
  \A t \in Tasks : (reg[t] = version /\ pc[t] = "parked") => ~WaitsOnReady(t)

\* No lost wakeup: a parked waiter whose row is ready is notified, or a
\* notification is still pending.
NoLostWakeup ==
  \A t \in Tasks :
    WaitsOnReady(t) =>
      \/ ntf[t] = "notified"
      \/ \E u \in Tasks : pc[u] \in {"notify", "stallnotify", "declare"}
      \/ boundary

\* No undetected hang: when every live task is parked and not notified, and
\* nothing else can act, a stall is declared or a declaration is pending.
Quiet ==
  /\ live > 0
  /\ ~boundary
  /\ \A t \in Tasks :
       \/ pc[t] \in {"unborn", "ended", "end", "declare"}
       \/ (pc[t] = "parked" /\ ntf[t] /= "notified")
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
