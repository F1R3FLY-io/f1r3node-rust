--------------------------- MODULE CounterConflict ---------------------------
(***************************************************************************)
(* Cross-branch merges of queue operations (P4, plan revision M-1).        *)
(*                                                                         *)
(* Two branches A and B of one block start from one pre-state of a space   *)
(* with the queues q1 and q2. Each branch runs one deploy of one or two    *)
(* queue operations: enqueue, dequeue or peek. Every operation, the peek   *)
(* included, bumps the collection counter S(sid,k) in its own transaction  *)
(* (decision 31), and an enqueue takes the counter value as its ord. The   *)
(* first operation on a queue consumes the space counter S(sid) instead    *)
(* and opens S(sid,k) at 1 (A25). HEAD's merge then runs check #1: the     *)
(* branches race when they consume one non-persistent produce. Reified     *)
(* channels are never mergeable. Without a race, the merge applies the     *)
(* changes of both branches to the pre-state.                              *)
(*                                                                         *)
(* Invariant:                                                              *)
(*   NoMergedOrderFabrication  when check #1 finds no race, the merged     *)
(*                             queues and the observations of both         *)
(*                             deploys are those of a serial run, A then   *)
(*                             B or B then A. So the merge never           *)
(*                             fabricates an order: two records with one   *)
(*                             ord, or a read that no serial order gives.  *)
(* Auxiliary invariants (the Rocq headlines of CounterConflict.v):         *)
(*   SharedQueueRaces          two branches that use one queue race        *)
(*   FirstOpsRace              two branches that open queues of one space  *)
(*                             race on S(sid)                              *)
(*                                                                         *)
(* Unsafe controls (each refutes exactly NoMergedOrderFabrication):        *)
(*   CounterDroppedUnsafe     WriteCounters = ReadCounters = FALSE: no     *)
(*                            operation uses a counter, and an enqueue     *)
(*                            takes the greatest ord of the queue plus 1.  *)
(*                            Two enqueues of one queue in two branches    *)
(*                            then take one ord, and nothing races.        *)
(*   PeekCounterFreeUnsafe    ReadCounters = FALSE: a peek does not bump   *)
(*                            the counter (the design before A2). A peek   *)
(*                            of q1 and a dequeue of q2 against a peek of  *)
(*                            q2 and a dequeue of q1 then merge, though    *)
(*                            no serial order gives both peeks (the        *)
(*                            write skew of P4 design D.2).                *)
(*                                                                         *)
(* Code (the P4 commit, with the paths without a prefix under              *)
(* rholang/src/rust/interpreter,                                           *)
(* rs = rspace++/src/rspace):                                              *)
(*   the counter bump     spaces/counter.rs:163-231, 235-292 (the          *)
(*                        counter COMM has no peeks)                       *)
(*   the plans            spaces/generic.rs:573-634, 640-700, 813-879      *)
(*   the peek COMM        rs/merger/event_log_index.rs:253-272 (a COMM     *)
(*                        with peeks files its produces under              *)
(*                        produces_peeked)                                 *)
(*   check #1             rs/merger/merging_logic.rs:279-386               *)
(*   the merged state     rs/merger/state_change.rs:359-392,               *)
(*                        rs/merger/state_change_merger.rs:389-509         *)
(* The Rocq module CounterConflict.v proves the race theorems and the      *)
(* serializability of a merge without a race for every plan.               *)
(*                                                                         *)
(* Abstractions:                                                           *)
(*   - A record is its ord, its value and its maker (the pre-state, A or   *)
(*     B): two branches never make one record, because each record holds   *)
(*     the random state of its deploy.                                     *)
(*   - A produce is named by its channel, its value and its maker. A       *)
(*     counter COMM consumes the counter produce that the operation        *)
(*     finds: the pre-state one for the first operation of a branch on a   *)
(*     queue, and a produce of the branch afterwards.                      *)
(*   - A dequeue of an empty queue observes "none" and changes no record.  *)
(*     The code parks a waiter there. A parked waiter of one branch        *)
(*     never meets a record of the other branch in this model, because     *)
(*     the counters already make two branches on one queue race.           *)
(*   - Checks #2 and #3 find nothing here: every consume of the model      *)
(*     meets its record in the branch, and no join has two channels.       *)
(***************************************************************************)
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS
    \* TRUE: an enqueue and a dequeue bump the counter, and an enqueue takes
    \* the counter value as its ord (M-1).
    WriteCounters,
    \* TRUE: a peek bumps the counter too (A2, decision 31).
    ReadCounters

ASSUME WriteCounters \in BOOLEAN
ASSUME ReadCounters \in BOOLEAN

VARIABLES
    \* The pre-state of the space.
    pre,
    \* The operations of the deploy of branch A, and of branch B.
    opsA,
    opsB

vars == <<pre, opsA, opsB>>

Qs == {"q1", "q2"}
Vals == {"x", "y"}

\* A queue of the pre-state: its records in ord order, its counter, and
\* whether S(sid,k) exists. A never-used queue has no counter.
Unopened == [recs |-> <<>>, ctr |-> 0, open |-> FALSE]
Holding(v) == [recs |-> <<[ord |-> 0, val |-> v, mk |-> "base"]>>, ctr |-> 1, open |-> TRUE]
PreQueues == {Unopened, Holding("x"), Holding("y")}

\* The space counter S(sid) counts the opened queues.
SpaceCounter(p) == Cardinality({q \in Qs : p[q].open})

PreStates == [Qs -> PreQueues]

Ops == {[kind |-> "enq", q |-> q, v |-> v] : q \in Qs, v \in Vals}
       \cup {[kind |-> k, q |-> q, v |-> "none"] : k \in {"deq", "peek"}, q \in Qs}

BranchOps == {<<o>> : o \in Ops} \cup {<<o1, o2>> : o1 \in Ops, o2 \in Ops}

-----------------------------------------------------------------------------
(* One branch                                                              *)

\* The state of a run: the queues (with "mine" when the branch already made
\* the counter record of the queue), the space counter and its maker.
StartState(p) ==
    [qs |-> [q \in Qs |-> [recs |-> p[q].recs, ctr |-> p[q].ctr, open |-> p[q].open, mine |-> FALSE]],
     sp |-> [ctr |-> SpaceCounter(p), mine |-> FALSE]]

\* Does the operation bump a counter?
Bumps(op) == IF op.kind = "peek" THEN ReadCounters ELSE WriteCounters

\* The counter COMM of an operation of branch b on queue q: the consumed
\* counter produce and the state after the bump.
CounterStep(st, q, b) ==
    IF st.qs[q].open
    THEN [st |-> [st EXCEPT !.qs[q].ctr = @ + 1, !.qs[q].mine = TRUE],
          ev |-> {<<"S", q, st.qs[q].ctr, IF st.qs[q].mine THEN b ELSE "base">>}]
    ELSE [st |-> [st EXCEPT !.qs[q].ctr = 1, !.qs[q].open = TRUE, !.qs[q].mine = TRUE,
                            !.sp = [ctr |-> st.sp.ctr + 1, mine |-> TRUE]],
          ev |-> {<<"SP", st.sp.ctr, IF st.sp.mine THEN b ELSE "base">>}]

\* The ord of an enqueue: the counter value (the first operation takes 0),
\* or, without counters, the greatest ord of the queue plus 1.
EnqOrd(st, q) ==
    IF WriteCounters
    THEN IF st.qs[q].open THEN st.qs[q].ctr ELSE 0
    ELSE IF st.qs[q].recs = <<>> THEN 0 ELSE st.qs[q].recs[Len(st.qs[q].recs)].ord + 1

\* One operation of branch b: the next state, the observation and the
\* consumed produces.
Exec(st, op, b) ==
    LET q == op.q
        bump == IF Bumps(op) THEN CounterStep(st, q, b) ELSE [st |-> st, ev |-> {}]
        s1 == bump.st
        recs == s1.qs[q].recs
    IN CASE op.kind = "enq" ->
              [st |-> [s1 EXCEPT !.qs[q].recs = Append(recs, [ord |-> EnqOrd(st, q), val |-> op.v, mk |-> b])],
               obs |-> "ok", ev |-> bump.ev]
         [] op.kind = "deq" /\ recs = <<>> ->
              [st |-> s1, obs |-> "none", ev |-> bump.ev]
         [] op.kind = "deq" ->
              [st |-> [s1 EXCEPT !.qs[q].recs = Tail(recs)], obs |-> recs[1].val,
               ev |-> bump.ev \cup {<<"D", q, recs[1].ord, recs[1].mk>>}]
         [] OTHER ->
              \* a peek: a COMM with peeks, filed under produces_peeked
              [st |-> s1, obs |-> IF recs = <<>> THEN "none" ELSE recs[1].val, ev |-> bump.ev]

\* The run of the deploy ops of branch b from the state st: the end state,
\* the observations in order, and every consumed produce.
Run(st, ops, b) ==
    LET r1 == Exec(st, ops[1], b) IN
    IF Len(ops) = 1
    THEN [st |-> r1.st, obs |-> <<r1.obs>>, ev |-> r1.ev]
    ELSE LET r2 == Exec(r1.st, ops[2], b) IN
         [st |-> r2.st, obs |-> <<r1.obs, r2.obs>>, ev |-> r1.ev \cup r2.ev]

RunA == Run(StartState(pre), opsA, "A")
RunB == Run(StartState(pre), opsB, "B")

-----------------------------------------------------------------------------
(* The merge                                                               *)

\* Check #1: the two branches consume one non-persistent produce. No
\* reified channel is mergeable.
Race == RunA.ev \cap RunB.ev /= {}

RecSet(recs) == {recs[i] : i \in DOMAIN recs}

\* The merged records of queue q: the pre-state minus what either branch
\* removed, plus what either branch added.
Merged(q) ==
    LET base == RecSet(pre[q].recs)
        endA == RecSet(RunA.st.qs[q].recs)
        endB == RecSet(RunB.st.qs[q].recs)
    IN (base \ ((base \ endA) \cup (base \ endB))) \cup (endA \ base) \cup (endB \ base)

\* The serial run of first, then second, gives the merged queues and the
\* observations of both branches.
SerialMatches(first, second, bf, bs, obsFirst, obsSecond) ==
    LET r1 == Run(StartState(pre), first, bf)
        r2 == Run(r1.st, second, bs)
    IN /\ \A q \in Qs : RecSet(r2.st.qs[q].recs) = Merged(q)
       /\ r1.obs = obsFirst
       /\ r2.obs = obsSecond

NoMergedOrderFabrication ==
    ~Race =>
       \/ SerialMatches(opsA, opsB, "A", "B", RunA.obs, RunB.obs)
       \/ SerialMatches(opsB, opsA, "B", "A", RunB.obs, RunA.obs)

\* The queues that a deploy uses.
Touched(ops) == {ops[i].q : i \in DOMAIN ops}

SharedQueueRaces == (Touched(opsA) \cap Touched(opsB) /= {}) => Race

FirstOpsRace ==
    (\E q1 \in Touched(opsA), q2 \in Touched(opsB) : ~pre[q1].open /\ ~pre[q2].open) => Race

TypeOK ==
    /\ pre \in PreStates
    /\ opsA \in BranchOps
    /\ opsB \in BranchOps

-----------------------------------------------------------------------------

Init ==
    /\ pre \in PreStates
    /\ opsA \in BranchOps
    /\ opsB \in BranchOps

\* The model checks the merge of each pair of branches once: every state is
\* an initial state.
Next == UNCHANGED vars

Spec == Init /\ [][Next]_vars

=============================================================================
