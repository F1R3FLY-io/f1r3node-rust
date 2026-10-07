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
(* Allocations (P5d, F11, when Allocations = TRUE). An Array or a Vector   *)
(* space allocates its index names from the allocator record A(sid). An    *)
(* allocation of k names consumes A(sid) = n and produces A(sid) = n + k,  *)
(* and it observes the indices n..n+k-1. The creation of the space stages  *)
(* A(sid) = 0 (EagerAllocator), so the pre-state of every block holds the  *)
(* record, and two branches that allocate consume one produce.             *)
(*   NoDuplicateIndex          when check #1 finds no race, the branches   *)
(*                             allocated disjoint indices                  *)
(*   AllocationsRace           two branches that both allocate race on     *)
(*                             A(sid)                                      *)
(* An allocation of a fixed or cyclic Array space differs only in its      *)
(* bound and in its modulo rule (OuterStorage.v). Its race on A(sid) is    *)
(* the same, so the model allocates with the vector rule.                  *)
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
(* Unsafe control (it refutes exactly NoDuplicateIndex):                   *)
(*   LazyAllocatorUnsafe      EagerAllocator = FALSE: the creation stages  *)
(*                            no allocator, and the first allocation       *)
(*                            creates A(sid) at its next value without a   *)
(*                            consume. Two branches that each make the     *)
(*                            first allocation of a space both hand out    *)
(*                            the index 0, and check #1 sees no race.      *)
(*                                                                         *)
(* Stacks (P5b, the crossed stacks of the M-1 arbitration, when Lifo =     *)
(* TRUE). The collections are stacks: a push appends, and a pop and a peek *)
(* meet the youngest record (the greatest ord). A pop is a removal, and it *)
(* bumps the counter (RemovalCounters). MC_CounterConflict_Stacks.cfg      *)
(* checks NoMergedOrderFabrication, SharedQueueRaces and FirstOpsRace for  *)
(* stacks.                                                                 *)
(* Unsafe control (it refutes exactly NoMergedOrderFabrication):           *)
(*   RemovalCounterFreeUnsafe stacks with RemovalCounters = FALSE: a pop   *)
(*                            does not bump the counter (the claim that    *)
(*                            the M-1 arbitration refuted). Branch A       *)
(*                            pushes onto q1 and pops q2, and branch B     *)
(*                            pushes onto q2 and pops q1. Check #1 sees no *)
(*                            race, and no serial order gives both pops.   *)
(*                            TLC reports the first violation that it      *)
(*                            meets: a pop of q2 against a peek and a push *)
(*                            of q2.                                       *)
(* Lifo and RemovalCounters are definitions, not constants. The P4 and the *)
(* allocator configurations keep their defaults (queues, and a removal     *)
(* bumps as a write does), so they need no new constant. A stack           *)
(* configuration replaces the definitions with a TLC override: Lifo <-     *)
(* LifoOn, and RemovalCounters <- RemovalCountersOff.                      *)
(*                                                                         *)
(* Code (the P4 commit, with the paths without a prefix under              *)
(* rholang/src/rust/interpreter, rs = rspace++/src/rspace):                *)
(*   the counter bump     spaces/counter.rs:165-245, 251-309 (the          *)
(*                        counter COMM has no peeks)                       *)
(*   the stack            spaces/disciplines/stack.rs:71-93 (a counter,    *)
(*                        and the head is the greatest ord), at the W2     *)
(*                        checkpoint commit bc43b8653                      *)
(*   the allocator        spaces/allocator.rs (creation_records,           *)
(*                        allocate), spaces/generic.rs (run_allocate,      *)
(*                        AllocCx::take, AllocCx::put)                     *)
(*   the plans            spaces/generic.rs:1421-1488, 1497-1581,          *)
(*                        1705-1809 (offer_head, receive_head, run_with)   *)
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
    ReadCounters,
    \* TRUE: the branches can also allocate index names (P5d).
    Allocations,
    \* TRUE: the creation of a space stages A(sid) = 0 (the code). FALSE: the
    \* first allocation creates A(sid) (the Unsafe control LazyAllocator).
    EagerAllocator

ASSUME WriteCounters \in BOOLEAN
ASSUME ReadCounters \in BOOLEAN
ASSUME Allocations \in BOOLEAN
ASSUME EagerAllocator \in BOOLEAN

\* The discipline of the collections. FALSE: queues (FIFO), so a dequeue and
\* a peek meet the oldest record. TRUE: stacks (LIFO), so they meet the
\* youngest record. A stack configuration replaces Lifo with LifoOn
\* (Lifo <- LifoOn), so the P4 and allocator configurations need no new
\* constant.
Lifo == FALSE
LifoOn == TRUE

\* TRUE: a dequeue (a removal: the take of a queue, the pop of a stack)
\* bumps the counter. By default a removal follows WriteCounters, as in P4.
\* The Unsafe control RemovalCounterFree replaces it with RemovalCountersOff.
RemovalCounters == WriteCounters
RemovalCountersOff == FALSE

VARIABLES
    \* The pre-state of the space.
    pre,
    \* The allocator A(sid) of the pre-state: its value, or NoAllocator when
    \* the space has no allocator record.
    preAlloc,
    \* The operations of the deploy of branch A, and of branch B.
    opsA,
    opsB

vars == <<pre, preAlloc, opsA, opsB>>

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

\* The pre-state value of a space without an allocator record.
NoAllocator == -1

\* The allocator values of the pre-state. Without allocations the model is
\* the P4 model, with one allocator value that no operation reads.
PreAllocs ==
    IF ~Allocations THEN {0}
    ELSE IF EagerAllocator THEN {0, 1, 2}
    ELSE {NoAllocator, 0, 1}

QueueOps == {[kind |-> "enq", q |-> q, v |-> v] : q \in Qs, v \in Vals}
            \cup {[kind |-> k, q |-> q, v |-> "none"] : k \in {"deq", "peek"}, q \in Qs}

\* An allocation of k index names.
AllocOps == {[kind |-> "alloc", k |-> k] : k \in {1, 2}}

Ops == QueueOps \cup (IF Allocations THEN AllocOps ELSE {})

BranchOps == {<<o>> : o \in Ops} \cup {<<o1, o2>> : o1 \in Ops, o2 \in Ops}

-----------------------------------------------------------------------------
(* One branch                                                              *)

\* The state of a run: the queues (with "mine" when the branch already made
\* the counter record of the queue), the space counter and its maker.
StartState(p, a) ==
    [qs |-> [q \in Qs |-> [recs |-> p[q].recs, ctr |-> p[q].ctr, open |-> p[q].open, mine |-> FALSE]],
     sp |-> [ctr |-> SpaceCounter(p), mine |-> FALSE],
     al |-> IF a = NoAllocator
            THEN [exists |-> FALSE, val |-> 0, mk |-> "base"]
            ELSE [exists |-> TRUE, val |-> a, mk |-> "base"],
     idx |-> {}]

\* Does the operation bump a counter?
Bumps(op) == CASE op.kind = "peek" -> ReadCounters
               [] op.kind = "deq" -> RemovalCounters
               [] OTHER -> WriteCounters

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

\* The record that a dequeue or a peek meets in the non-empty records recs.
\* For a queue it is the oldest record (the least ord). For a stack it is the
\* youngest record (the greatest ord).
TopRec(recs) == IF Lifo THEN recs[Len(recs)] ELSE recs[1]

\* The records recs without the record that a dequeue takes.
WithoutTop(recs) == IF Lifo THEN SubSeq(recs, 1, Len(recs) - 1) ELSE Tail(recs)

\* The indices n..n+k-1 of an allocation of k names from the value n.
Indices(n, k) == {n + i : i \in 0..(k - 1)}

\* One allocation of op.k names by branch b. With an allocator record, it
\* consumes the produce of A(sid) = n and produces A(sid) = n + k. Without
\* one (LazyAllocator), it creates A(sid) = k and consumes nothing.
AllocExec(st, op, b) ==
    IF ~st.al.exists
    THEN [st |-> [st EXCEPT !.al = [exists |-> TRUE, val |-> op.k, mk |-> b],
                            !.idx = @ \cup Indices(0, op.k)],
          obs |-> Indices(0, op.k), ev |-> {}]
    ELSE [st |-> [st EXCEPT !.al = [exists |-> TRUE, val |-> st.al.val + op.k, mk |-> b],
                            !.idx = @ \cup Indices(st.al.val, op.k)],
          obs |-> Indices(st.al.val, op.k),
          ev |-> {<<"A", st.al.val, st.al.mk>>}]

\* One queue operation of branch b: the next state, the observation and the
\* consumed produces.
QueueExec(st, op, b) ==
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
              [st |-> [s1 EXCEPT !.qs[q].recs = WithoutTop(recs)], obs |-> TopRec(recs).val,
               ev |-> bump.ev \cup {<<"D", q, TopRec(recs).ord, TopRec(recs).mk>>}]
         [] OTHER ->
              \* a peek: a COMM with peeks, filed under produces_peeked
              [st |-> s1, obs |-> IF recs = <<>> THEN "none" ELSE TopRec(recs).val, ev |-> bump.ev]

\* One operation of branch b.
Exec(st, op, b) == IF op.kind = "alloc" THEN AllocExec(st, op, b) ELSE QueueExec(st, op, b)

\* The run of the deploy ops of branch b from the state st: the end state,
\* the observations in order, and every consumed produce.
Run(st, ops, b) ==
    LET r1 == Exec(st, ops[1], b) IN
    IF Len(ops) = 1
    THEN [st |-> r1.st, obs |-> <<r1.obs>>, ev |-> r1.ev]
    ELSE LET r2 == Exec(r1.st, ops[2], b) IN
         [st |-> r2.st, obs |-> <<r1.obs, r2.obs>>, ev |-> r1.ev \cup r2.ev]

RunA == Run(StartState(pre, preAlloc), opsA, "A")
RunB == Run(StartState(pre, preAlloc), opsB, "B")

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
\* The allocator records of a state: none, or its one record.
AlSet(al) == IF al.exists THEN {[val |-> al.val, mk |-> al.mk]} ELSE {}

\* The merged allocator record: the pre-state record minus what either
\* branch consumed, plus what either branch produced.
MergedAlloc ==
    LET base == AlSet(StartState(pre, preAlloc).al)
        endA == AlSet(RunA.st.al)
        endB == AlSet(RunB.st.al)
    IN (base \ ((base \ endA) \cup (base \ endB))) \cup (endA \ base) \cup (endB \ base)

SerialMatches(first, second, bf, bs, obsFirst, obsSecond) ==
    LET r1 == Run(StartState(pre, preAlloc), first, bf)
        r2 == Run(r1.st, second, bs)
    IN /\ \A q \in Qs : RecSet(r2.st.qs[q].recs) = Merged(q)
       /\ AlSet(r2.st.al) = MergedAlloc
       /\ r1.obs = obsFirst
       /\ r2.obs = obsSecond

NoMergedOrderFabrication ==
    ~Race =>
       \/ SerialMatches(opsA, opsB, "A", "B", RunA.obs, RunB.obs)
       \/ SerialMatches(opsB, opsA, "B", "A", RunB.obs, RunA.obs)

\* The queues that a deploy uses.
Touched(ops) == {ops[i].q : i \in {j \in DOMAIN ops : ops[j].kind /= "alloc"}}

\* True when a deploy allocates index names.
Allocates(ops) == \E i \in DOMAIN ops : ops[i].kind = "alloc"

\* NoDuplicateIndex: a merge without a race never hands one index name to
\* both branches.
NoDuplicateIndex == ~Race => RunA.st.idx \cap RunB.st.idx = {}

\* Two branches that both allocate race on the allocator record.
AllocationsRace == (Allocates(opsA) /\ Allocates(opsB)) => Race

SharedQueueRaces == (Touched(opsA) \cap Touched(opsB) /= {}) => Race

FirstOpsRace ==
    (\E q1 \in Touched(opsA), q2 \in Touched(opsB) : ~pre[q1].open /\ ~pre[q2].open) => Race

TypeOK ==
    /\ pre \in PreStates
    /\ preAlloc \in PreAllocs
    /\ opsA \in BranchOps
    /\ opsB \in BranchOps

-----------------------------------------------------------------------------

Init ==
    /\ pre \in PreStates
    /\ preAlloc \in PreAllocs
    /\ opsA \in BranchOps
    /\ opsB \in BranchOps

\* The model checks the merge of each pair of branches once: every state is
\* an initial state.
Next == UNCHANGED vars

Spec == Init /\ [][Next]_vars

=============================================================================
