-------------------- MODULE NativeReplayStallLifecycle --------------------
\* DR-132 (decision D2-B): the lifecycle of the stall rule.
\*
\* `NativeReplayStall.tla` checks the wait protocol of one evaluation. This
\* module checks how the progress word of
\* `rholang/src/rust/interpreter/accounting/native_runtime/checked_operations/
\* trace/replay/progress.rs` behaves across the life of a replay:
\*   - a panic drops a counted waiter: its wait leaves the registration, then
\*     its live token drops (`async move { let _live = token; future.await }`
\*     binds the token first, so it drops last);
\*   - a cancel drops the evaluation: `close` sets CLOSED before any task is
\*     aborted, so the aborts declare no stall;
\*   - the root's live token drops when the root eval returns, before the
\*     drain;
\*   - a second evaluation can `begin` only from an idle word.
\*
\* Each evaluation e has a root, a waiter and a producer. The waiter's row
\* lists the producer's row as its predecessor. The producer either performs
\* its row or fails before it, as a recorded call without its record does.
\* A wake is always possible, so the model covers spurious wakes; the
\* notification protocol itself is checked in `NativeReplayStall.tla`.
\*
\* Each guard switch removes one safeguard. Every switch is FALSE in the safe
\* configuration, and each refutation configuration sets one switch.
EXTENDS Integers, FiniteSets

CONSTANTS
  \* @type: Bool;
  NoLeaveOnDrop,
  \* @type: Bool;
  FinishBeforeDrop,
  \* @type: Bool;
  BeginNonZero,
  \* @type: Bool;
  AbortBeforeClose,
  \* @type: Bool;
  HoldRootThroughDrain

VARIABLES
  \* @type: Int -> Str;
  pc,
  \* @type: Int -> Bool;
  token,
  \* @type: Int -> Bool;
  waitGone,
  \* @type: Int -> Int;
  reg,
  \* @type: Int -> Bool;
  doomed,
  \* @type: Set(Int);
  done,
  \* @type: Int;
  version,
  \* @type: Int;
  live,
  \* @type: Int;
  parked,
  \* @type: Bool;
  wStalled,
  \* @type: Bool;
  wClosed,
  \* @type: Bool;
  closePending,
  \* @type: Bool;
  stateClosed,
  \* @type: Int;
  eval,
  \* @type: Str;
  phase,
  \* @type: Bool;
  failed,
  \* @type: Bool;
  falseStall,
  \* @type: Bool;
  lateStall

vars == <<pc, token, waitGone, reg, doomed, done, version, live, parked,
          wStalled, wClosed, closePending, stateClosed, eval, phase, failed,
          falseStall, lateStall>>

Tasks == 0..5
None == -1

EvalOf(t) == IF t < 3 THEN 1 ELSE 2
\* 0 is the root, 1 the waiter, 2 the producer.
Role(t) == t % 3
RootOf(e) == 3 * (e - 1)

\* The waiter's row is 2(e - 1). The producer's row is 2(e - 1) + 1.
RowOf(t) == 2 * (EvalOf(t) - 1) + (IF Role(t) = 1 THEN 0 ELSE 1)
RowReady(t) == Role(t) /= 1 \/ (RowOf(t) + 1) \in done

\* `stalls(w)`: no flag, some live task, every live task parked.
Stalls(l, p) == ~wStalled /\ ~wClosed /\ l > 0 /\ p = l

\* A task of the current evaluation that can still change the replay state.
CanProgress(u) ==
  /\ EvalOf(u) = eval
  /\ token[u]
  /\ ~doomed[u]
  /\ \/ pc[u] \in {"spawnA", "spawnB", "return", "run", "publish"}
     \/ (pc[u] \in {"check", "parked"} /\ RowReady(u))

FalseStallNow(t) == \E u \in Tasks \ {t} : CanProgress(u)

\* The stall declaration under the state lock: set the flags, close the
\* replay state, bump the version and clear the parked count. A declaration
\* is late when the evaluation is already being cancelled.
DeclareStall(t) ==
  /\ falseStall' = (falseStall \/ FalseStallNow(t))
  /\ lateStall' = (lateStall \/ phase = "cancelling")
  /\ wStalled' = TRUE
  /\ stateClosed' = TRUE
  /\ version' = version + 1
  /\ parked' = 0

NoDeclaration ==
  UNCHANGED <<falseStall, lateStall, wStalled, stateClosed, version, parked>>

Init ==
  /\ pc = [t \in Tasks |-> IF t = 0 THEN "spawnA" ELSE "unborn"]
  /\ token = [t \in Tasks |-> t = 0]
  /\ waitGone = [t \in Tasks |-> Role(t) /= 1]
  /\ reg = [t \in Tasks |-> None]
  /\ doomed = [t \in Tasks |-> FALSE]
  /\ done = {}
  /\ version = 0
  /\ live = 1
  /\ parked = 0
  /\ wStalled = FALSE
  /\ wClosed = FALSE
  /\ closePending = FALSE
  /\ stateClosed = FALSE
  /\ eval = 1
  /\ phase = "running"
  /\ failed = FALSE
  /\ falseStall = FALSE
  /\ lateStall = FALSE

\* ---------------------------------------------------------------------
\* The root
\* ---------------------------------------------------------------------

\* `enter` counts the child before the spawn.
SpawnWaiter(r) ==
  /\ Role(r) = 0
  /\ pc[r] = "spawnA"
  /\ ~doomed[r]
  /\ pc' = [pc EXCEPT ![r] = "spawnB", ![r + 1] = "check"]
  /\ token' = [token EXCEPT ![r + 1] = TRUE]
  /\ live' = live + 1
  /\ UNCHANGED <<waitGone, reg, doomed, done, version, parked, wStalled,
                 wClosed, closePending, stateClosed, eval, phase, failed,
                 falseStall, lateStall>>

SpawnProducer(r) ==
  /\ Role(r) = 0
  /\ pc[r] = "spawnB"
  /\ ~doomed[r]
  /\ pc' = [pc EXCEPT ![r] = "return", ![r + 2] = "run"]
  /\ token' = [token EXCEPT ![r + 2] = TRUE]
  /\ live' = live + 1
  /\ UNCHANGED <<waitGone, reg, doomed, done, version, parked, wStalled,
                 wClosed, closePending, stateClosed, eval, phase, failed,
                 falseStall, lateStall>>

\* The root eval returns. Its live token drops before the drain, unless
\* HoldRootThroughDrain keeps it until the drain ends.
RootReturn(r) ==
  /\ Role(r) = 0
  /\ pc[r] = "return"
  /\ ~doomed[r]
  /\ pc' = [pc EXCEPT ![r] = "drain"]
  /\ IF HoldRootThroughDrain
       THEN /\ UNCHANGED <<token, live>>
            /\ NoDeclaration
       ELSE /\ token' = [token EXCEPT ![r] = FALSE]
            /\ live' = live - 1
            /\ IF Stalls(live - 1, parked) THEN DeclareStall(r) ELSE NoDeclaration
  /\ UNCHANGED <<waitGone, reg, doomed, done, wClosed, closePending, eval,
                 phase, failed>>

\* The drain ends when every task of the evaluation has ended.
RootEnd(r) ==
  /\ Role(r) = 0
  /\ pc[r] = "drain"
  /\ ~doomed[r]
  /\ pc[r + 1] \in {"unborn", "ended"}
  /\ pc[r + 2] \in {"unborn", "ended"}
  /\ pc' = [pc EXCEPT ![r] = "ended"]
  /\ phase' = IF failed \/ wStalled THEN "failed" ELSE "ended"
  /\ IF token[r]
       THEN /\ token' = [token EXCEPT ![r] = FALSE]
            /\ live' = live - 1
            /\ IF Stalls(live - 1, parked) THEN DeclareStall(r) ELSE NoDeclaration
       ELSE /\ UNCHANGED <<token, live>>
            /\ NoDeclaration
  /\ UNCHANGED <<waitGone, reg, doomed, done, wClosed, closePending, eval,
                 failed>>

\* ---------------------------------------------------------------------
\* The waiter and the producer
\* ---------------------------------------------------------------------

\* Leave the registration: a no-op unless it is at the current version.
Leave(t) ==
  /\ parked' = IF reg[t] = version THEN parked - 1 ELSE parked
  /\ reg' = [reg EXCEPT ![t] = None]

\* `wait_ready`: the check under the state lock.
Check(t) ==
  /\ Role(t) = 1
  /\ pc[t] = "check"
  /\ ~doomed[t]
  /\ IF stateClosed
       THEN /\ Leave(t)
            /\ pc' = [pc EXCEPT ![t] = "end"]
            /\ failed' = TRUE
            /\ UNCHANGED <<falseStall, lateStall, wStalled, stateClosed, version>>
     ELSE IF RowReady(t)
       THEN /\ Leave(t)
            /\ pc' = [pc EXCEPT ![t] = "publish"]
            /\ UNCHANGED <<failed, falseStall, lateStall, wStalled, stateClosed,
                           version>>
     ELSE
       LET counts == reg[t] /= version
           p == IF counts THEN parked + 1 ELSE parked
       IN
       IF counts /\ Stalls(live, p)
         THEN /\ DeclareStall(t)
              /\ reg' = [reg EXCEPT ![t] = None]
              /\ pc' = [pc EXCEPT ![t] = "end"]
              /\ failed' = TRUE
         ELSE /\ parked' = p
              /\ reg' = [reg EXCEPT ![t] = version]
              /\ pc' = [pc EXCEPT ![t] = "parked"]
              /\ UNCHANGED <<failed, falseStall, lateStall, wStalled,
                             stateClosed, version>>
  /\ UNCHANGED <<token, waitGone, doomed, done, live, wClosed, closePending,
                 eval, phase>>

\* A wake, after a change or spurious.
Wake(t) ==
  /\ pc[t] = "parked"
  /\ ~doomed[t]
  /\ pc' = [pc EXCEPT ![t] = "check"]
  /\ UNCHANGED <<token, waitGone, reg, doomed, done, version, live, parked,
                 wStalled, wClosed, closePending, stateClosed, eval, phase,
                 failed, falseStall, lateStall>>

\* A publish under the state lock: complete the row, bump the version and
\* clear the parked count.
PublishRow(t) ==
  /\ IF stateClosed
       THEN /\ failed' = TRUE
            /\ UNCHANGED <<done, version, parked>>
       ELSE /\ done' = done \cup {RowOf(t)}
            /\ version' = version + 1
            /\ parked' = 0
            /\ UNCHANGED failed
  /\ pc' = [pc EXCEPT ![t] = "end"]
  /\ UNCHANGED <<token, waitGone, reg, doomed, live, wStalled, wClosed,
                 closePending, stateClosed, eval, phase, falseStall, lateStall>>

Publish(t) ==
  /\ Role(t) = 1
  /\ pc[t] = "publish"
  /\ ~doomed[t]
  /\ PublishRow(t)

ProducerOp(t) ==
  /\ Role(t) = 2
  /\ pc[t] = "run"
  /\ ~doomed[t]
  /\ PublishRow(t)

\* The producer fails before its row.
ProducerFail(t) ==
  /\ Role(t) = 2
  /\ pc[t] = "run"
  /\ ~doomed[t]
  /\ pc' = [pc EXCEPT ![t] = "end"]
  /\ failed' = TRUE
  /\ UNCHANGED <<token, waitGone, reg, doomed, done, version, live, parked,
                 wStalled, wClosed, closePending, stateClosed, eval, phase,
                 falseStall, lateStall>>

\* `finish`: the live token drops when the task's future ends.
Finish(t) ==
  /\ Role(t) /= 0
  /\ pc[t] = "end"
  /\ ~doomed[t]
  /\ token' = [token EXCEPT ![t] = FALSE]
  /\ live' = live - 1
  /\ pc' = [pc EXCEPT ![t] = "ended"]
  /\ IF Stalls(live - 1, parked) THEN DeclareStall(t) ELSE NoDeclaration
  /\ UNCHANGED <<waitGone, reg, doomed, done, wClosed, closePending, eval,
                 phase, failed>>

\* ---------------------------------------------------------------------
\* Panics, cancellation and drops
\* ---------------------------------------------------------------------

\* A waiter panics while it holds a wait, for example on a poisoned lock.
Panic(t) ==
  /\ Role(t) = 1
  /\ pc[t] \in {"check", "parked"}
  /\ ~doomed[t]
  /\ phase = "running"
  /\ doomed' = [doomed EXCEPT ![t] = TRUE]
  /\ failed' = TRUE
  /\ UNCHANGED <<pc, token, waitGone, reg, done, version, live, parked,
                 wStalled, wClosed, closePending, stateClosed, eval, phase,
                 falseStall, lateStall>>

\* The evaluation's future drops. `CloseOnDrop` closes the word before any
\* handle aborts, unless AbortBeforeClose lets the aborts run first.
Cancel ==
  /\ phase = "running"
  /\ phase' = "cancelling"
  /\ doomed' = [t \in Tasks |->
                  doomed[t] \/ (EvalOf(t) = eval /\ pc[t] \notin {"unborn", "ended"})]
  /\ IF AbortBeforeClose
       THEN /\ closePending' = TRUE
            /\ UNCHANGED wClosed
       ELSE /\ wClosed' = (wClosed \/ live /= 0)
            /\ UNCHANGED closePending
  /\ UNCHANGED <<pc, token, waitGone, reg, done, version, live, parked,
                 wStalled, stateClosed, eval, failed, falseStall, lateStall>>

\* AbortBeforeClose only: the close runs after some aborts.
CloseWord ==
  /\ closePending
  /\ closePending' = FALSE
  /\ wClosed' = (wClosed \/ live /= 0)
  /\ UNCHANGED <<pc, token, waitGone, reg, doomed, done, version, live, parked,
                 wStalled, stateClosed, eval, phase, failed, falseStall,
                 lateStall>>

\* A dropped task drops its wait, then its live token. FinishBeforeDrop
\* reverses the order. NoLeaveOnDrop destroys the registration without
\* leaving it.
DropWait(t) ==
  /\ doomed[t]
  /\ ~waitGone[t]
  /\ FinishBeforeDrop => ~token[t]
  /\ waitGone' = [waitGone EXCEPT ![t] = TRUE]
  /\ reg' = [reg EXCEPT ![t] = None]
  /\ parked' = IF reg[t] = version /\ ~NoLeaveOnDrop THEN parked - 1 ELSE parked
  /\ pc' = [pc EXCEPT ![t] = IF token[t] THEN pc[t] ELSE "ended"]
  /\ UNCHANGED <<token, doomed, done, version, live, wStalled, wClosed,
                 closePending, stateClosed, eval, phase, failed, falseStall,
                 lateStall>>

DropToken(t) ==
  /\ doomed[t]
  /\ token[t]
  /\ ~FinishBeforeDrop => waitGone[t]
  /\ token' = [token EXCEPT ![t] = FALSE]
  /\ live' = live - 1
  /\ pc' = [pc EXCEPT ![t] = IF waitGone[t] THEN "ended" ELSE pc[t]]
  /\ IF Stalls(live - 1, parked) THEN DeclareStall(t) ELSE NoDeclaration
  /\ UNCHANGED <<waitGone, reg, doomed, done, wClosed, closePending, eval,
                 phase, failed>>

\* A dropped task that holds neither a wait nor a token is gone.
Gone(t) ==
  /\ doomed[t]
  /\ ~token[t]
  /\ waitGone[t]
  /\ pc[t] /= "ended"
  /\ pc' = [pc EXCEPT ![t] = "ended"]
  /\ UNCHANGED <<token, waitGone, reg, doomed, done, version, live, parked,
                 wStalled, wClosed, closePending, stateClosed, eval, phase,
                 failed, falseStall, lateStall>>

\* ---------------------------------------------------------------------
\* A second evaluation
\* ---------------------------------------------------------------------

\* `begin` succeeds only from an idle word (`compare_exchange(0, LIVE)`).
\* BeginNonZero adds the root's count to any word.
Begin ==
  /\ eval = 1
  /\ phase \in {"ended", "failed", "cancelling"}
  /\ BeginNonZero \/ (live = 0 /\ parked = 0 /\ ~wStalled /\ ~wClosed)
  /\ eval' = 2
  /\ phase' = "running"
  /\ failed' = FALSE
  /\ live' = live + 1
  /\ token' = [token EXCEPT ![RootOf(2)] = TRUE]
  /\ pc' = [pc EXCEPT ![RootOf(2)] = "spawnA"]
  /\ UNCHANGED <<waitGone, reg, doomed, done, version, parked, wStalled,
                 wClosed, closePending, stateClosed, falseStall, lateStall>>

Next ==
  \/ \E t \in Tasks :
       \/ SpawnWaiter(t) \/ SpawnProducer(t) \/ RootReturn(t) \/ RootEnd(t)
       \/ Check(t) \/ Wake(t) \/ Publish(t) \/ ProducerOp(t)
       \/ ProducerFail(t) \/ Finish(t) \/ Panic(t) \/ DropWait(t)
       \/ DropToken(t) \/ Gone(t)
  \/ Cancel
  \/ CloseWord
  \/ Begin

Spec == Init /\ [][Next]_vars

\* ---------------------------------------------------------------------
\* Invariants
\* ---------------------------------------------------------------------

TypeOK ==
  /\ pc \in [Tasks -> {"unborn", "spawnA", "spawnB", "return", "drain",
                       "check", "parked", "publish", "run", "end", "ended"}]
  /\ token \in [Tasks -> BOOLEAN]
  /\ waitGone \in [Tasks -> BOOLEAN]
  /\ DOMAIN reg = Tasks
  /\ \A t \in Tasks : reg[t] = None \/ reg[t] >= 0
  /\ doomed \in [Tasks -> BOOLEAN]
  /\ done \subseteq 0..3
  /\ version >= 0
  /\ live >= 0
  /\ parked >= 0
  /\ wStalled \in BOOLEAN
  /\ wClosed \in BOOLEAN
  /\ closePending \in BOOLEAN
  /\ stateClosed \in BOOLEAN
  /\ eval \in {1, 2}
  /\ phase \in {"running", "cancelling", "ended", "failed"}
  /\ failed \in BOOLEAN
  /\ falseStall \in BOOLEAN
  /\ lateStall \in BOOLEAN

\* I1: the live count is the tokens of the current evaluation.
LiveExact == live = Cardinality({t \in Tasks : EvalOf(t) = eval /\ token[t]})

\* I2: the parked count is the registrations at the current version.
CountExact == parked = Cardinality({t \in Tasks : reg[t] = version})

NoFalseStall == ~falseStall

\* No stall is declared once a cancel has started.
NoLateStall == ~lateStall

\* A normal end leaves the word idle, so `begin` succeeds again.
IdleAfterEnd ==
  phase = "ended" => (live = 0 /\ parked = 0 /\ ~wStalled /\ ~wClosed)

\* No undetected hang: when every task of the running evaluation has ended,
\* drains, or waits at the current version on a row that is not ready, a
\* stall is declared.
Quiet ==
  /\ phase = "running"
  /\ live > 0
  /\ \A t \in Tasks :
       \/ EvalOf(t) /= eval
       \/ pc[t] \in {"unborn", "ended", "drain"}
       \/ (pc[t] = "parked" /\ ~doomed[t] /\ reg[t] = version /\ ~RowReady(t))
NoHang == Quiet => wStalled

\* Reachability witness. A configuration expects it to be violated: after
\* a normal end, a second evaluation begins and ends normally.
NoRerun == ~(eval = 2 /\ phase = "ended")
=============================================================================
