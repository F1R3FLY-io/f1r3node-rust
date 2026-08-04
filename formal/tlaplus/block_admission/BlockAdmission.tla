--------------------------- MODULE BlockAdmission ---------------------------
(***************************************************************************)
(* Byte-bounded admission for the inbound block-processing pipeline,       *)
(* written to DISCOVER (not assume) the two failure modes bracketing the   *)
(* design space:                                                           *)
(*                                                                         *)
(*   1. The CURRENT design (ByteBounded = FALSE) bounds the processor      *)
(*      queue by MESSAGE COUNT only. BlockMessage size is unbounded by     *)
(*      the queue, so retained bytes are unbounded in the count cap:       *)
(*      Inv_RetainedBytesBounded is violated. This is the mechanism        *)
(*      behind the 2026-08-04 daily-soak breach signature: the readonly    *)
(*      observer (a node whose entire workload is inbound blocks it must   *)
(*      replay, produced by four validators) peaked at 6,492MB against a   *)
(*      947-3,371MB validator baseline while total node RSS plateaued —    *)
(*      the runaway replay-cache fix held, and what remained was           *)
(*      role-shaped retention on the receive-only path.                    *)
(*                                                                         *)
(*   2. The NAIVE fix (ByteBounded = TRUE, DeferralRerequests = FALSE)     *)
(*      sheds load by dropping blocks that do not fit the byte budget.     *)
(*      A dropped block is protocol data: nothing re-delivers it, so       *)
(*      Live_AllBroadcastProcessed is violated — the shard wedges          *)
(*      quietly, which is strictly worse than the memory it saved.         *)
(*                                                                         *)
(* The FIX this module gates (ByteBounded = TRUE, DeferralRerequests =     *)
(* TRUE): admission is gated on the byte budget covering BOTH queued and   *)
(* in-flight messages, and a block that does not fit is NOT dropped — it   *)
(* remains requestable and is re-delivered when budget frees (the          *)
(* block-retriever's requested-blocks / dependency-recovery loop). Under   *)
(* weak fairness this satisfies both the byte bound and liveness.          *)
(*                                                                         *)
(* Models:                                                                 *)
(*   node/src/rust/runtime/setup.rs                                        *)
(*     block_processor_queue — mpsc::channel(block_processor_queue_max_   *)
(*     pending()), the count-bounded queue (`queued` here, FIFO).          *)
(*   node/src/rust/instances/block_processor_instance.rs                   *)
(*     create/run — drain loop holding a Semaphore(max_parallel_blocks)    *)
(*     (`processing` here, |processing| <= MaxParallel); a dequeued        *)
(*     BlockMessage stays resident until its replay completes, so the      *)
(*     byte budget must cover queued + in-flight, not queued alone.        *)
(*   casper/src/rust/engine/block_retriever.rs                             *)
(*     requested-blocks / dependency recovery — the re-request pool that   *)
(*     makes deferral sound (`pending` here retains deferred blocks when   *)
(*     DeferralRerequests).                                                *)
(*                                                                         *)
(* Size abstraction: each block carries a nondeterministic size in         *)
(* 1..MaxBlockBytes chosen at broadcast, so TLC explores every size        *)
(* pattern. The ASSUME below (MaxBlockBytes <= ByteCap) is a REAL          *)
(* obligation on the implementation: the byte cap must be no smaller       *)
(* than the protocol's max block size (validate.rs block-size limit),     *)
(* otherwise an oversized block is unadmittable forever and liveness is    *)
(* forfeit by configuration rather than by design.                         *)
(***************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS
    Blocks,              \* model values: the finite universe of blocks
    MaxBlockBytes,       \* max nondeterministic block size (scaled, e.g. 2)
    ByteCap,             \* retained-bytes budget for queued + in-flight
    CountCap,            \* the mpsc capacity (message count), today's only bound
    MaxParallel,         \* semaphore permits (F1R3_MAX_PARALLEL_BLOCKS)
    ByteBounded,         \* TRUE = byte-gated admission; FALSE = current design
    DeferralRerequests   \* TRUE = deferred blocks stay requestable (the fix);
                         \* FALSE = over-budget blocks are dropped (the wedge)

ASSUME MaxBlockBytes <= ByteCap
ASSUME MaxParallel >= 1 /\ CountCap >= 1 /\ MaxBlockBytes >= 1

VARIABLES
    size,       \* [Blocks -> Nat] block size, fixed at broadcast (0 = not yet)
    pending,    \* broadcast, not yet admitted (network + re-request pool)
    queued,     \* FIFO processor queue (sequence of blocks)
    processing, \* dequeued, replay in progress
    processed,  \* replay complete
    dropped     \* shed under the naive fix; unreachable under the real fix

vars == <<size, pending, queued, processing, processed, dropped>>

SeqRange(s) == {s[i] : i \in 1..Len(s)}

RECURSIVE SeqBytes(_)
SeqBytes(s) == IF s = <<>> THEN 0 ELSE size[Head(s)] + SeqBytes(Tail(s))

RECURSIVE SetBytes(_)
SetBytes(S) ==
    IF S = {} THEN 0
    ELSE LET x == CHOOSE y \in S : TRUE
         IN size[x] + SetBytes(S \ {x})

RetainedBytes == SeqBytes(queued) + SetBytes(processing)

Broadcasted == pending \cup SeqRange(queued) \cup processing \cup processed \cup dropped

Init ==
    /\ size = [b \in Blocks |-> 0]
    /\ pending = {}
    /\ queued = <<>>
    /\ processing = {}
    /\ processed = {}
    /\ dropped = {}

Broadcast(b, s) ==
    /\ b \notin Broadcasted
    /\ size' = [size EXCEPT ![b] = s]
    /\ pending' = pending \cup {b}
    /\ UNCHANGED <<queued, processing, processed, dropped>>

AdmissionOk(b) ==
    IF ByteBounded
    THEN RetainedBytes + size[b] <= ByteCap
    ELSE Len(queued) < CountCap

Admit(b) ==
    /\ b \in pending
    /\ AdmissionOk(b)
    /\ queued' = Append(queued, b)
    /\ pending' = pending \ {b}
    /\ UNCHANGED <<size, processing, processed, dropped>>

\* The naive fix's load shed: only enabled when deferral is NOT wired to the
\* re-request loop. The real fix has no such transition — an over-budget
\* block simply stays in `pending` until Admit is enabled again.
Drop(b) ==
    /\ ByteBounded
    /\ ~DeferralRerequests
    /\ b \in pending
    /\ ~AdmissionOk(b)
    /\ pending' = pending \ {b}
    /\ dropped' = dropped \cup {b}
    /\ UNCHANGED <<size, queued, processing, processed>>

StartProcessing ==
    /\ queued /= <<>>
    /\ Cardinality(processing) < MaxParallel
    /\ processing' = processing \cup {Head(queued)}
    /\ queued' = Tail(queued)
    /\ UNCHANGED <<size, pending, processed, dropped>>

Complete(b) ==
    /\ b \in processing
    /\ processing' = processing \ {b}
    /\ processed' = processed \cup {b}
    /\ UNCHANGED <<size, pending, queued, dropped>>

\* Quiescence: every block has been broadcast and reached a terminal set
\* (processed, or dropped under the naive fix). Explicit stuttering here
\* keeps TLC's deadlock check meaningful for every non-terminal state.
Done ==
    /\ Broadcasted = Blocks
    /\ pending = {}
    /\ queued = <<>>
    /\ processing = {}
    /\ UNCHANGED vars

Next ==
    \/ \E b \in Blocks, s \in 1..MaxBlockBytes : Broadcast(b, s)
    \/ \E b \in Blocks : Admit(b)
    \/ \E b \in Blocks : Drop(b)
    \/ StartProcessing
    \/ \E b \in Blocks : Complete(b)
    \/ Done

\* Weak fairness on drain and admission: the retriever re-delivers, the
\* drain loop keeps running. Broadcast is deliberately unfair — liveness is
\* conditioned on a block having been broadcast, not on broadcasts occurring.
Fairness ==
    /\ WF_vars(\E b \in Blocks : Admit(b))
    /\ WF_vars(StartProcessing)
    /\ WF_vars(\E b \in Blocks : Complete(b))

Spec == Init /\ [][Next]_vars /\ Fairness

----------------------------------------------------------------------------

TypeOK ==
    /\ size \in [Blocks -> 0..MaxBlockBytes]
    /\ pending \subseteq Blocks
    /\ queued \in Seq(Blocks)
    /\ processing \subseteq Blocks
    /\ processed \subseteq Blocks
    /\ dropped \subseteq Blocks

\* THE byte bound: what the observer node lacked. Under ByteBounded this is
\* an invariant; under the current design (ByteBounded = FALSE, the pre-fix
\* config) TLC exhibits a violation with any CountCap * MaxBlockBytes that
\* exceeds ByteCap — the count cap is not a byte cap.
Inv_RetainedBytesBounded == RetainedBytes <= ByteCap

\* The real fix sheds nothing: dropped stays empty. (Trivial under
\* DeferralRerequests — Drop is disabled — but stated so the gating config
\* fails loudly if a load-shedding transition is ever added without
\* revisiting liveness.)
Inv_NoBlocksShed == dropped = {}

\* Every block that was ever broadcast is eventually processed. The naive
\* fix violates this (broadcast -> drop -> stuck in `dropped`); the real
\* fix satisfies it under weak fairness because `pending` blocks are
\* re-admittable whenever budget frees, and the finite universe guarantees
\* budget eventually frees.
Live_AllBroadcastProcessed ==
    \A b \in Blocks : (b \in Broadcasted) ~> (b \in processed)

=============================================================================
