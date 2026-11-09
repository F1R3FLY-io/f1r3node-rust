----------------------------- MODULE NativeReplay -----------------------------
(***************************************************************************)
(* Native replay of space operations (decision 47, design v2, section 5).  *)
(*                                                                         *)
(* Changed by decision 90 (DR-132): only OutcomeSpec is checked. The       *)
(* scheduler of ScheduleSpec is gone, and the readiness wait of space      *)
(* transactions extends the stall rule (cost_accounted_rho/                *)
(* NativeReplayStall.tla, the "txn..." scenarios). ScheduleSpec stays      *)
(* below as a comment.                                                     *)
(*                                                                         *)
(* Changed by decision 92: the readiness check of a space transaction      *)
(* reads its first row only (SpaceReplayOrder.tla). A missing first row    *)
(* ends the replay NotReady, and the transaction parks and stages again. A *)
(* missing later row ends it Mismatch, because the commit fails closed at  *)
(* that row (begin_row). In OutcomeSpec, a published journal has one row   *)
(* for each operation that play began, and replay never goes past its last *)
(* row. Neither case is reachable here, and SpaceReplayOrder.tla checks    *)
(* both.                                                                   *)
(*                                                                         *)
(* The module held two specifications. Each configuration picks one.      *)
(*                                                                         *)
(* OutcomeSpec (T8). Native play stages the operations of a space          *)
(* transaction and records one journal row for each of them: Stored or     *)
(* Matched with the COMM of its candidate, or a denial. The proposer       *)
(* publishes the journal, honestly or with one tampered row. Native replay *)
(* stages the same operations and replays each one against its row, as     *)
(* NativeBackend::apply_op does (rspace++/src/rspace/replay_rspace/        *)
(* native_session/space.rs): no row ends the replay NotReady; a recorded   *)
(* outcome that does not fit the staged kind, and a recorded candidate     *)
(* that is not the staged COMM, end it Mismatch; a recorded denial must be *)
(* a denial of the budget. The budget charges 1 for an introduction and 1  *)
(* for a COMM, against the limit Budget, in play and in replay alike.      *)
(*                                                                         *)
(* ScheduleSpec (NoBlockedReadiness, NoDeadlock). The intents of one       *)
(* frontier run in conflict groups: two intents share a group when their   *)
(* footprint keys meet. A group runs its intents in operation order, and   *)
(* groups run concurrently. A space intent has the key [2]||sid (I-11) and *)
(* the extra channels of its job. A host intent has its channel keys. In   *)
(* native execution every intent also has the key [2, 0] (DR-78, DR-79).   *)
(* The journal rows have predecessors: the last earlier row that touches   *)
(* each channel of the footprint (check_dependencies). A host operation    *)
(* waits until its row is ready and then takes its stripes (F4). A space   *)
(* transaction takes its stripes, never waits (P1), and fails closed when  *)
(* a row is not ready or when its footprint differs from the row's         *)
(* journal footprint. A failure closes the epoch, so every waiting         *)
(* operation wakes and fails (Closed). A forgery may add one channel to    *)
(* the journal footprint of one space row, with predecessors derived from  *)
(* the forged footprints, so it passes the journal check.                  *)
(* Changed by decision 124 (L10): a failure no longer closes the epoch,    *)
(* as in the base. The failed transaction leaves its registration, fails,  *)
(* and releases its stripes (LeaveAndFail in                               *)
(* cost_accounted_rho/NativeReplayStall.tla). An operation that waits on   *)
(* its row parks as a counted waiter, so the stall rule ends the replay    *)
(* (Stalled). The sentence "A failure closes the epoch, so every waiting   *)
(* operation wakes and fails (Closed)." above gives the code before        *)
(* decision 124.                                                           *)
(*                                                                         *)
(* Invariants:                                                             *)
(*   T8 NativeReplayOutcomeEqual  replay accepts the journal of play, and  *)
(*                         only it, and then it has the observer calls,    *)
(*                         the spending, the effects and the status of     *)
(*                         play                                            *)
(*   NoBlockedReadiness    with an honest journal, no space row finds a    *)
(*                         predecessor that is not complete                *)
(*   NoDeadlock            until every intent ends, some intent can step   *)
(*                                                                         *)
(* Unsafe controls (each refutes exactly one invariant):                   *)
(*   TamperedOutcomeUnsafe  replay skips the outcome and candidate checks  *)
(*                          -> T8                                          *)
(*   NoSpaceKeyUnsafe       space intents lose [2]||sid and [2, 0]         *)
(*                          -> NoBlockedReadiness                          *)
(*   WaitUnderLocksUnsafe   a space transaction waits for readiness while  *)
(*                          it holds its stripes -> NoDeadlock             *)
(*   NoInvalidateUnsafe     a failed space row does not close the epoch,   *)
(*                          so a host operation that waits on it waits     *)
(*                          forever -> NoDeadlock                          *)
(*   Changed by decision 124 (L10): no failed space row closes the epoch.  *)
(*   The host operation that waits on it parks as a counted waiter, and    *)
(*   the stall rule ends the replay (NativeReplayStall.tla). So the wait   *)
(*   is not forever, and this control stays disabled with ScheduleSpec.    *)
(*                                                                         *)
(* The scheduling scenario has two spaces A and B, two host operations and *)
(* five intents in operation order:                                        *)
(*   1 space A, rows 1 {a1} and 2 {a2}   2 host, row 3 {x}                 *)
(*   3 space B, row 4 {b1}               4 space A, rows 5 {a1} and 6 {a2} *)
(*   5 host, row 7 {x, h}                                                  *)
(* The stripes collide: a1 and x share stripe 1, and b1 and h stripe 3.    *)
(***************************************************************************)
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS
    \* The COMMs of fires in OutcomeSpec.
    Comms,
    \* The longest transaction of OutcomeSpec.
    MaxOps,
    \* The largest phlo limit of OutcomeSpec.
    MaxBudget,
    \* OutcomeSpec: the proposer may tamper one row of the journal.
    Tamper,
    \* OutcomeSpec control: replay skips the fit and candidate checks.
    TamperedOutcomeUnsafe,
    \* ScheduleSpec: native execution adds the key [2, 0] to every intent.
    NativeKey,
    \* ScheduleSpec: the journal may carry one forged footprint.
    Forge,
    \* ScheduleSpec control: space intents lose [2]||sid and [2, 0].
    NoSpaceKeyUnsafe,
    \* ScheduleSpec control: a space transaction waits under its stripes.
    WaitUnderLocksUnsafe,
    \* ScheduleSpec control: a failed space row does not close the epoch.
    NoInvalidateUnsafe

ASSUME MaxOps \in 1..3
ASSUME MaxBudget \in 0..8

VARIABLES
    \* OutcomeSpec
    ops, budget, phase,
    pPos, pSpent, pStatus, pRows, pCalls, pApplied,
    jrnl,
    rPos, rSpent, rStatus, rCalls, rApplied,
    \* ScheduleSpec
    forgery, st, ipos, holder, complete, closed, blocked

outcomeVars == <<ops, budget, phase, pPos, pSpent, pStatus, pRows, pCalls, pApplied, jrnl,
                 rPos, rSpent, rStatus, rCalls, rApplied>>
scheduleVars == <<forgery, st, ipos, holder, complete, closed, blocked>>
vars == <<outcomeVars, scheduleVars>>

\* The journal of ScheduleSpec without a forgery (row 0 does not exist).
NoForgery == <<0, "none">>

(* ════════════════════════════ OutcomeSpec (T8) ════════════════════════════ *)

\* A staged operation is a store, or a fire with its COMM.
OpKinds == {"store"} \cup Comms
IsFire(o) == o /= "store"
Outcomes == {"stored", "matched", "deniedIntro", "deniedComm"}
\* The well-formed rows: a store or a denied introduction has no COMM, and a fire or a
\* denied COMM has one. The journal check rejects any other row before replay begins.
Rows == [out : {"stored", "deniedIntro"}, comm : {"none"}]
        \cup [out : {"matched", "deniedComm"}, comm : Comms]
Calls == ({"intro"} \X (1..MaxOps)) \cup ({"comm"} \X Comms)
OpSeqs == UNION {[1..n -> OpKinds] : n \in 1..MaxOps}
Statuses == {"ok", "outOfPhlo", "mismatch", "notReady"}

\* The row that play records for an operation whose charges pass.
DoneRow(o) == IF IsFire(o) THEN [out |-> "matched", comm |-> o] ELSE [out |-> "stored", comm |-> "none"]

\* The calls that an operation makes when its charges pass.
DoneCalls(k, o) == IF IsFire(o) THEN <<<<"intro", k>>, <<"comm", o>>>> ELSE <<<<"intro", k>>>>

OutcomeInit ==
    /\ ops \in OpSeqs
    /\ budget \in 0..MaxBudget
    /\ phase = "play"
    /\ pPos = 1 /\ pSpent = 0 /\ pStatus = "ok" /\ pRows = <<>> /\ pCalls = <<>> /\ pApplied = 0
    /\ jrnl = <<>>
    /\ rPos = 1 /\ rSpent = 0 /\ rStatus = "ok" /\ rCalls = <<>> /\ rApplied = 0

\* Native play of the next operation (the native budget records each row).
PlayStep ==
    /\ phase = "play"
    /\ pStatus = "ok"
    /\ pPos <= Len(ops)
    /\ LET o == ops[pPos]
           introOk == pSpent + 1 <= budget
           commOk == pSpent + 2 <= budget
       IN CASE ~introOk ->
                 /\ pRows' = Append(pRows, [out |-> "deniedIntro", comm |-> "none"])
                 /\ pCalls' = Append(pCalls, <<"intro", pPos>>)
                 /\ pStatus' = "outOfPhlo"
                 /\ UNCHANGED <<pPos, pSpent, pApplied>>
            [] IsFire(o) /\ ~commOk ->
                 /\ pRows' = Append(pRows, [out |-> "deniedComm", comm |-> o])
                 /\ pCalls' = pCalls \o <<<<"intro", pPos>>, <<"comm", o>>>>
                 /\ pSpent' = pSpent + 1
                 /\ pStatus' = "outOfPhlo"
                 /\ UNCHANGED <<pPos, pApplied>>
            [] OTHER ->
                 /\ pRows' = Append(pRows, DoneRow(o))
                 /\ pCalls' = pCalls \o DoneCalls(pPos, o)
                 /\ pSpent' = pSpent + (IF IsFire(o) THEN 2 ELSE 1)
                 /\ pApplied' = pApplied + 1
                 /\ pPos' = pPos + 1
                 /\ UNCHANGED pStatus
    /\ UNCHANGED <<ops, budget, phase, jrnl, rPos, rSpent, rStatus, rCalls, rApplied>>

PlayEnd ==
    /\ phase = "play"
    /\ pStatus /= "ok" \/ pPos > Len(ops)
    /\ phase' = "publish"
    /\ UNCHANGED <<ops, budget, pPos, pSpent, pStatus, pRows, pCalls, pApplied, jrnl,
                   rPos, rSpent, rStatus, rCalls, rApplied>>

\* The proposer publishes the journal of play, or (Tamper) a copy with one row changed.
Publish ==
    /\ phase = "publish"
    /\ \/ jrnl' = pRows
       \/ /\ Tamper
          /\ \E i \in 1..Len(pRows), r \in Rows :
                /\ r /= pRows[i]
                /\ jrnl' = [pRows EXCEPT ![i] = r]
    /\ phase' = "replay"
    /\ UNCHANGED <<ops, budget, pPos, pSpent, pStatus, pRows, pCalls, pApplied,
                   rPos, rSpent, rStatus, rCalls, rApplied>>

\* The recorded outcome fits the staged kind.
Fits(o, r) == IF IsFire(o) THEN r.out \in {"matched", "deniedIntro", "deniedComm"}
                          ELSE r.out \in {"stored", "deniedIntro"}

\* Native replay of the next operation against its row (NativeBackend::apply_op).
ReplayStep ==
    /\ phase = "replay"
    /\ rStatus = "ok"
    /\ rPos <= Len(ops)
    /\ LET o == ops[rPos]
           r == IF rPos <= Len(jrnl) THEN jrnl[rPos] ELSE [out |-> "stored", comm |-> "none"]
           introOk == rSpent + 1 <= budget
           commOk == rSpent + 2 <= budget
           checked == ~TamperedOutcomeUnsafe
       \* Changed by decision 92: a missing first row ends the replay NotReady, and a
       \* missing later row ends it Mismatch, because the commit fails closed there.
       \* IN CASE rPos > Len(jrnl) ->
       \*          /\ rStatus' = "notReady"
       \*          /\ UNCHANGED <<rPos, rSpent, rCalls, rApplied>>
       IN CASE rPos > Len(jrnl) ->
                 /\ rStatus' = IF rPos = 1 THEN "notReady" ELSE "mismatch"
                 /\ UNCHANGED <<rPos, rSpent, rCalls, rApplied>>
            [] checked /\ ~Fits(o, r) ->
                 /\ rStatus' = "mismatch"
                 /\ UNCHANGED <<rPos, rSpent, rCalls, rApplied>>
            [] r.out = "deniedIntro" ->
                 /\ rCalls' = Append(rCalls, <<"intro", rPos>>)
                 /\ rStatus' = IF introOk THEN "mismatch" ELSE "outOfPhlo"
                 /\ UNCHANGED <<rPos, rSpent, rApplied>>
            [] ~introOk ->
                 /\ rCalls' = Append(rCalls, <<"intro", rPos>>)
                 /\ rStatus' = "mismatch"
                 /\ UNCHANGED <<rPos, rSpent, rApplied>>
            [] IsFire(o) /\ checked /\ r.comm /= o ->
                 /\ rCalls' = Append(rCalls, <<"intro", rPos>>)
                 /\ rSpent' = rSpent + 1
                 /\ rStatus' = "mismatch"
                 /\ UNCHANGED <<rPos, rApplied>>
            [] IsFire(o) /\ r.out = "deniedComm" ->
                 /\ rCalls' = rCalls \o <<<<"intro", rPos>>, <<"comm", o>>>>
                 /\ rSpent' = rSpent + 1
                 /\ rStatus' = IF commOk THEN "mismatch" ELSE "outOfPhlo"
                 /\ UNCHANGED <<rPos, rApplied>>
            [] IsFire(o) /\ ~commOk ->
                 /\ rCalls' = rCalls \o <<<<"intro", rPos>>, <<"comm", o>>>>
                 /\ rSpent' = rSpent + 1
                 /\ rStatus' = "mismatch"
                 /\ UNCHANGED <<rPos, rApplied>>
            [] OTHER ->
                 /\ rCalls' = rCalls \o DoneCalls(rPos, o)
                 /\ rSpent' = rSpent + (IF IsFire(o) THEN 2 ELSE 1)
                 /\ rApplied' = rApplied + 1
                 /\ rPos' = rPos + 1
                 /\ UNCHANGED rStatus
    /\ UNCHANGED <<ops, budget, phase, pPos, pSpent, pStatus, pRows, pCalls, pApplied, jrnl>>

ReplayEnd ==
    /\ phase = "replay"
    /\ rStatus /= "ok" \/ rPos > Len(ops)
    /\ phase' = "done"
    /\ UNCHANGED <<ops, budget, pPos, pSpent, pStatus, pRows, pCalls, pApplied, jrnl,
                   rPos, rSpent, rStatus, rCalls, rApplied>>

\* The rows that replay consumed: every row it began, and a denied row is consumed.
RowsUsed == IF rStatus = "ok" THEN rPos - 1 ELSE rPos

\* Replay accepts: it ends without a mismatch and consumes every row (check_complete).
Accepted == rStatus \in {"ok", "outOfPhlo"} /\ RowsUsed = Len(jrnl)

ScheduleConst ==
    /\ forgery = NoForgery /\ st = <<>> /\ ipos = <<>> /\ holder = <<>>
    /\ complete = <<>> /\ closed = FALSE /\ blocked = FALSE

OutcomeNext ==
    /\ (PlayStep \/ PlayEnd \/ Publish \/ ReplayStep \/ ReplayEnd)
    /\ UNCHANGED scheduleVars

OutcomeDone ==
    /\ phase = "done"
    /\ UNCHANGED vars

OutcomeSpec == OutcomeInit /\ ScheduleConst /\ [][OutcomeNext \/ OutcomeDone]_vars

OutcomeTypeOK ==
    /\ ops \in OpSeqs
    /\ budget \in 0..MaxBudget
    /\ phase \in {"play", "publish", "replay", "done"}
    /\ pPos \in 1..(MaxOps + 1) /\ rPos \in 1..(MaxOps + 1)
    /\ pSpent \in 0..MaxBudget /\ rSpent \in 0..MaxBudget
    /\ pStatus \in Statuses /\ rStatus \in Statuses
    /\ pApplied \in 0..MaxOps /\ rApplied \in 0..MaxOps
    /\ pRows \in Seq(Rows) /\ jrnl \in Seq(Rows)
    /\ pCalls \in Seq(Calls) /\ rCalls \in Seq(Calls)

\* T8: native replay accepts the journal of play, and only it, and then it has the
\* observer calls, the spending, the effects (the applied prefix) and the status of play.
NativeReplayOutcomeEqual ==
    phase = "done" =>
        /\ (jrnl = pRows) => Accepted
        /\ Accepted =>
              /\ jrnl = pRows
              /\ rStatus = pStatus
              /\ rSpent = pSpent
              /\ rCalls = pCalls
              /\ rApplied = pApplied

\* Disabled by Reified RSpaces (decision 90, DR-132): ScheduleSpec models the
\* deterministic scheduler, which DR-132 removed. Its conflict groups ran the
\* intents of one frontier in operation order (Turn), and a space transaction
\* never waited (P1). Racing evaluation has neither. The readiness wait of space
\* transactions now extends the stall rule of native replay:
\* formal/tlaplus/cost_accounted_rho/NativeReplayStall.tla, the "txn..."
\* scenarios, with the controls of the code before decision 90
\* ("txnracefailclosed"), of a mismatch that fails ("txn...mismatchfails"), of a
\* park under stripes ("txnstripesunderstripes"), of in-transaction predecessors
\* ("txnracenointxn") and of a repeated source ("txnrepeatnorepeat").
\* Changed by Reified RSpaces (decision 92): the readiness check reads the first
\* row of a space transaction only. The controls of a mismatch that fails, of
\* in-transaction predecessors and of a repeated source are disabled.
\* formal/tlaplus/reified_rspaces/SpaceReplayOrder.tla has the controls of the
\* first-row check.
\* (* ═════════════════ ScheduleSpec (NoBlockedReadiness, NoDeadlock) ═════════════════ *)
\*
\* Intents == 1..5
\* RowIds == 1..7
\* Chans == {"a1", "a2", "b1", "x", "h"}
\* Stripes == 1..3
\*
\* Kind(i) == IF i \in {2, 5} THEN "host" ELSE "space"
\* Sid(i) == IF i \in {1, 4} THEN "A" ELSE "B"
\* IntentRows(i) == CASE i = 1 -> <<1, 2>>
\*                    [] i = 2 -> <<3>>
\*                    [] i = 3 -> <<4>>
\*                    [] i = 4 -> <<5, 6>>
\*                    [] i = 5 -> <<7>>
\* RowChans(r) == CASE r = 1 -> {"a1"}
\*                  [] r = 2 -> {"a2"}
\*                  [] r = 3 -> {"x"}
\*                  [] r = 4 -> {"b1"}
\*                  [] r = 5 -> {"a1"}
\*                  [] r = 6 -> {"a2"}
\*                  [] r = 7 -> {"x", "h"}
\* Stripe(c) == CASE c = "a1" -> 1 [] c = "x" -> 1 [] c = "a2" -> 2 [] c = "b1" -> 3 [] c = "h" -> 3
\*
\* RowsOf(i) == {IntentRows(i)[k] : k \in 1..Len(IntentRows(i))}
\* ChansOf(i) == UNION {RowChans(r) : r \in RowsOf(i)}
\* StripesOf(i) == {Stripe(c) : c \in ChansOf(i)}
\*
\* SpaceRowIds == UNION {RowsOf(i) : i \in {j \in Intents : Kind(j) = "space"}}
\*
\* \* A forgery adds one channel that another row touches to the journal footprint of one
\* \* space row. The predecessors follow from the forged footprints, so the forgery passes
\* \* the journal check.
\* Forgeries == {NoForgery}
\*              \cup UNION {{<<r, c>> : c \in {d \in Chans : d \notin RowChans(r)}} : r \in SpaceRowIds}
\*
\* JournalChans(f, r) ==
\*     IF f[1] = r THEN RowChans(r) \cup {f[2]} ELSE RowChans(r)
\*
\* \* check_dependencies: the last earlier row that touches each channel of the footprint.
\* Preds(f, r) ==
\*     {p \in 1..(r - 1) :
\*         \E c \in JournalChans(f, r) :
\*             /\ c \in JournalChans(f, p)
\*             /\ \A q \in (p + 1)..(r - 1) : c \notin JournalChans(f, q)}
\*
\* \* The footprint keys of an intent: [2]||sid, the native key [2, 0] and the channel keys.
\* Keys(i) ==
\*     IF Kind(i) = "space"
\*     THEN (IF NoSpaceKeyUnsafe THEN {} ELSE {"sid:" \o Sid(i)})
\*          \cup (IF NativeKey /\ ~NoSpaceKeyUnsafe THEN {"native"} ELSE {})
\*     ELSE ChansOf(i) \cup (IF NativeKey THEN {"native"} ELSE {})
\*
\* Linked(i, j) == Keys(i) \cap Keys(j) /= {}
\*
\* \* The conflict group of an intent: the closure of Linked (five intents, four steps).
\* Reach(S) == S \cup {j \in Intents : \E i \in S : Linked(i, j)}
\* Group(i) == Reach(Reach(Reach(Reach({i}))))
\*
\* Ended(i) == st[i] \in {"done", "failed"}
\*
\* \* The group runs its intents in operation order.
\* Turn(i) == \A j \in Group(i) : j < i => Ended(j)
\*
\* Ready(r) == \A p \in Preds(forgery, r) : complete[p]
\*
\* Authentic(r) == JournalChans(forgery, r) = RowChans(r)
\*
\* Free(i) == \A s \in StripesOf(i) : holder[s] = 0
\*
\* Release(i) == [s \in Stripes |-> IF holder[s] = i THEN 0 ELSE holder[s]]
\*
\* ScheduleInit ==
\*     /\ forgery \in (IF Forge THEN Forgeries ELSE {NoForgery})
\*     /\ st = [i \in Intents |-> "pending"]
\*     /\ ipos = [i \in Intents |-> 1]
\*     /\ holder = [s \in Stripes |-> 0]
\*     /\ complete = [r \in RowIds |-> FALSE]
\*     /\ closed = FALSE
\*     /\ blocked = FALSE
\*
\* OutcomeConst ==
\*     /\ ops = <<"store">> /\ budget = 0 /\ phase = "done"
\*     /\ pPos = 1 /\ pSpent = 0 /\ pStatus = "ok" /\ pRows = <<>> /\ pCalls = <<>> /\ pApplied = 0
\*     /\ jrnl = <<>> /\ rPos = 1 /\ rSpent = 0 /\ rStatus = "ok" /\ rCalls = <<>> /\ rApplied = 0
\*
\* \* A host operation waits until its row is ready and then takes its stripes (F4). It runs,
\* \* publishes its row and releases its stripes in one step.
\* CanHostRun(i) ==
\*     /\ Kind(i) = "host" /\ st[i] = "pending" /\ ~closed /\ Turn(i)
\*     /\ Ready(IntentRows(i)[1]) /\ Free(i)
\*
\* HostRun(i) ==
\*     /\ CanHostRun(i)
\*     /\ complete' = [complete EXCEPT ![IntentRows(i)[1]] = TRUE]
\*     /\ st' = [st EXCEPT ![i] = "done"]
\*     /\ UNCHANGED <<forgery, ipos, holder, closed, blocked>>
\*
\* \* A space transaction takes the stripes of its channels before its first row.
\* CanSpaceBegin(i) ==
\*     /\ Kind(i) = "space" /\ st[i] = "pending" /\ ~closed /\ Turn(i) /\ Free(i)
\*
\* SpaceBegin(i) ==
\*     /\ CanSpaceBegin(i)
\*     /\ holder' = [s \in Stripes |-> IF s \in StripesOf(i) THEN i ELSE holder[s]]
\*     /\ st' = [st EXCEPT ![i] = "running"]
\*     /\ UNCHANGED <<forgery, ipos, complete, closed, blocked>>
\*
\* \* The next row of a running space transaction.
\* Current(i) == IntentRows(i)[ipos[i]]
\*
\* \* The row begins (begin_operation returns a ticket) and authenticates its footprint.
\* CanSpaceRow(i) ==
\*     /\ Kind(i) = "space" /\ st[i] = "running" /\ ~closed
\*     /\ Ready(Current(i)) /\ Authentic(Current(i))
\*
\* SpaceRow(i) ==
\*     /\ CanSpaceRow(i)
\*     /\ complete' = [complete EXCEPT ![Current(i)] = TRUE]
\*     /\ IF ipos[i] = Len(IntentRows(i))
\*        THEN /\ st' = [st EXCEPT ![i] = "done"]
\*             /\ holder' = Release(i)
\*             /\ UNCHANGED ipos
\*        ELSE /\ ipos' = [ipos EXCEPT ![i] = ipos[i] + 1]
\*             /\ UNCHANGED <<st, holder>>
\*     /\ UNCHANGED <<forgery, closed, blocked>>
\*
\* \* The row fails closed: it is not ready (P1: no wait) or its footprint is forged
\* \* (authenticate_footprint). The failure releases the stripes and closes the epoch
\* \* (NoInvalidateUnsafe: it does not). WaitUnderLocksUnsafe: a row that is not ready waits.
\* CanSpaceFail(i) ==
\*     /\ Kind(i) = "space" /\ st[i] = "running" /\ ~closed
\*     /\ \/ ~Ready(Current(i)) /\ ~WaitUnderLocksUnsafe
\*        \/ Ready(Current(i)) /\ ~Authentic(Current(i))
\*
\* SpaceFail(i) ==
\*     /\ CanSpaceFail(i)
\*     /\ st' = [st EXCEPT ![i] = "failed"]
\*     /\ holder' = Release(i)
\*     /\ closed' = ~NoInvalidateUnsafe
\*     /\ blocked' = (blocked \/ ~Ready(Current(i)))
\*     /\ UNCHANGED <<forgery, ipos, complete>>
\*
\* \* After the epoch closes, every operation that waits or runs wakes and fails (Closed).
\* CanCloseOut(i) == closed /\ st[i] \in {"pending", "running"}
\*
\* CloseOut(i) ==
\*     /\ CanCloseOut(i)
\*     /\ st' = [st EXCEPT ![i] = "failed"]
\*     /\ holder' = Release(i)
\*     /\ UNCHANGED <<forgery, ipos, complete, closed, blocked>>
\*
\* CanStep(i) ==
\*     \/ CanHostRun(i) \/ CanSpaceBegin(i) \/ CanSpaceRow(i) \/ CanSpaceFail(i)
\*     \/ CanCloseOut(i)
\*
\* AllEnded == \A i \in Intents : Ended(i)
\*
\* ScheduleNext ==
\*     /\ \E i \in Intents : HostRun(i) \/ SpaceBegin(i) \/ SpaceRow(i) \/ SpaceFail(i)
\*                           \/ CloseOut(i)
\*     /\ UNCHANGED outcomeVars
\*
\* ScheduleDone ==
\*     /\ AllEnded
\*     /\ UNCHANGED vars
\*
\* ScheduleSpec == OutcomeConst /\ ScheduleInit /\ [][ScheduleNext \/ ScheduleDone]_vars
\*
\* ScheduleTypeOK ==
\*     /\ forgery \in Forgeries
\*     /\ st \in [Intents -> {"pending", "running", "done", "failed"}]
\*     /\ ipos \in [Intents -> 1..2]
\*     /\ holder \in [Stripes -> 0..5]
\*     /\ complete \in [RowIds -> BOOLEAN]
\*     /\ closed \in BOOLEAN
\*     /\ blocked \in BOOLEAN
\*
\* \* With an honest journal, no space row finds a predecessor that is not complete.
\* NoBlockedReadiness == forgery = NoForgery => ~blocked
\*
\* \* Until every intent ends, some intent can step: no wait cycle and no lost wake-up.
\* NoDeadlock == AllEnded \/ \E i \in Intents : CanStep(i)

=============================================================================
