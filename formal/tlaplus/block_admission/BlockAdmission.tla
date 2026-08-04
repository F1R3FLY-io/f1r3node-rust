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
(* in-flight messages, and a block that does not fit is NOT dropped — its  *)
(* payload buffer is RELEASED (the explicit Defer transition below) and    *)
(* the block remains requestable, re-delivered when budget frees (the      *)
(* block-retriever's requested-blocks / dependency-recovery loop). Under   *)
(* weak fairness this satisfies the byte bound, the total-residency        *)
(* bound, and liveness.                                                    *)
(*                                                                         *)
(* Delivery is modeled explicitly so deferred-payload memory cannot        *)
(* silently escape the accounting: a delivered block sits in `resident`    *)
(* (its bytes held by the receiving task) until admission either admits    *)
(* it (bytes move into the budget) or defers it (Defer returns it to       *)
(* `pending`, which retains NO bytes — the release is a transition, not    *)
(* a modeling assumption). `resident` is bounded by MaxDeliveries, the     *)
(* transport's concurrent-delivery window, giving the checked total        *)
(* Inv_TotalResidencyBounded: ByteCap + MaxDeliveries * MaxBlockBytes.     *)
(*                                                                         *)
(* Models:                                                                 *)
(*   node/src/rust/runtime/setup.rs                                        *)
(*     block_processor_queue — mpsc::channel(block_processor_queue_max_   *)
(*     pending()), the count-bounded queue (`queued` here, FIFO). The      *)
(*     count cap remains in force in EVERY mode: byte-gating is an         *)
(*     additional conjunct, not a replacement for the mpsc capacity.       *)
(*   node/src/rust/instances/block_processor_instance.rs                   *)
(*     create/run — drain loop holding a Semaphore(max_parallel_blocks)    *)
(*     (`processing` here, |processing| <= MaxParallel); a dequeued        *)
(*     BlockMessage stays resident until its replay completes, so the      *)
(*     byte budget must cover queued + in-flight, not queued alone.        *)
(*   inbound dispatch (comm/casper packet handlers)                        *)
(*     the decoded BlockMessage held by a receiving task between arrival   *)
(*     and the admission decision (`resident` here). Backpressure under    *)
(*     the current design is a block waiting in `resident` for queue       *)
(*     capacity; deferral under the fix is Defer releasing that buffer.    *)
(*   casper/src/rust/engine/block_retriever.rs                             *)
(*     requested-blocks / dependency recovery — the re-request pool that   *)
(*     makes deferral sound (`pending` here retains deferred blocks when   *)
(*     DeferralRerequests; a later Deliver is the re-delivery).            *)
(*                                                                         *)
(* Size abstraction: each block carries a nondeterministic size in         *)
(* 1..MaxBlockBytes chosen at broadcast, so TLC explores every size        *)
(* pattern. The ASSUME below (MaxBlockBytes <= ByteCap) is a REAL          *)
(* obligation on the implementation: the byte cap must be no smaller      *)
(* than the protocol's max block size (validate.rs block-size limit),     *)
(* otherwise an oversized block is unadmittable forever and liveness is    *)
(* forfeit by configuration rather than by design.                         *)
(***************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS
    Blocks,              \* model values: the finite universe of blocks
    MaxBlockBytes,       \* max nondeterministic block size (scaled, e.g. 2)
    ByteCap,             \* retained-bytes budget for queued + in-flight
    CountCap,            \* the mpsc capacity (message count), in force always
    MaxParallel,         \* semaphore permits (F1R3_MAX_PARALLEL_BLOCKS)
    MaxDeliveries,       \* concurrent delivered-but-undecided payloads
    ByteBounded,         \* TRUE = byte-gated admission; FALSE = current design
    DeferralRerequests   \* TRUE = deferred blocks stay requestable (the fix);
                         \* FALSE = over-budget blocks are dropped (the wedge)

ASSUME MaxBlockBytes <= ByteCap
ASSUME MaxParallel >= 1 /\ CountCap >= 1 /\ MaxBlockBytes >= 1
ASSUME MaxDeliveries >= 1

VARIABLES
    size,       \* [Blocks -> Nat] block size, fixed at broadcast (0 = not yet)
    pending,    \* broadcast, not delivered or deferred (network + re-request
                \* pool); retains NO payload bytes — Defer's release target
    resident,   \* delivered, bytes held awaiting the admission decision
    queued,     \* FIFO processor queue (sequence of blocks)
    processing, \* dequeued, replay in progress
    processed,  \* replay complete
    dropped     \* shed under the naive fix; unreachable under the real fix

vars == <<size, pending, resident, queued, processing, processed, dropped>>

SeqRange(s) == {s[i] : i \in 1..Len(s)}

RECURSIVE SeqBytes(_)
SeqBytes(s) == IF s = <<>> THEN 0 ELSE size[Head(s)] + SeqBytes(Tail(s))

RECURSIVE SetBytes(_)
SetBytes(S) ==
    IF S = {} THEN 0
    ELSE LET x == CHOOSE y \in S : TRUE
         IN size[x] + SetBytes(S \ {x})

\* Admission's budget: queued + in-flight. Delivery-window residency is
\* accounted separately (ResidentBytes) and bounded by MaxDeliveries.
RetainedBytes == SeqBytes(queued) + SetBytes(processing)

ResidentBytes == SetBytes(resident)

Broadcasted ==
    pending \cup resident \cup SeqRange(queued)
        \cup processing \cup processed \cup dropped

Init ==
    /\ size = [b \in Blocks |-> 0]
    /\ pending = {}
    /\ resident = {}
    /\ queued = <<>>
    /\ processing = {}
    /\ processed = {}
    /\ dropped = {}

Broadcast(b, s) ==
    /\ b \notin Broadcasted
    /\ size' = [size EXCEPT ![b] = s]
    /\ pending' = pending \cup {b}
    /\ UNCHANGED <<resident, queued, processing, processed, dropped>>

\* Transport hands a payload to the node: bytes become resident. Bounded by
\* the concurrent-delivery window, not by the admission budget — admission
\* cannot govern what peers send, only what it retains past the decision.
Deliver(b) ==
    /\ b \in pending
    /\ Cardinality(resident) < MaxDeliveries
    /\ resident' = resident \cup {b}
    /\ pending' = pending \ {b}
    /\ UNCHANGED <<size, queued, processing, processed, dropped>>

\* The mpsc count cap holds in every mode; byte-gating is an additional
\* conjunct under the fix, never a replacement for the channel capacity.
AdmissionOk(b) ==
    /\ Len(queued) < CountCap
    /\ ByteBounded => RetainedBytes + size[b] <= ByteCap

Admit(b) ==
    /\ b \in resident
    /\ AdmissionOk(b)
    /\ queued' = Append(queued, b)
    /\ resident' = resident \ {b}
    /\ UNCHANGED <<size, pending, processing, processed, dropped>>

\* The fix's deferral: the payload buffer is released (b leaves `resident`,
\* whose bytes are counted, for `pending`, which retains none) and the block
\* stays requestable. This transition IS the release/re-delivery model:
\* deferred memory is accounted while held and provably relinquished here.
\* Under the current design (~ByteBounded) there is no deferral — a block
\* that cannot be admitted waits in `resident` (mpsc backpressure).
Defer(b) ==
    /\ ByteBounded
    /\ DeferralRerequests
    /\ b \in resident
    /\ ~AdmissionOk(b)
    /\ pending' = pending \cup {b}
    /\ resident' = resident \ {b}
    /\ UNCHANGED <<size, queued, processing, processed, dropped>>

\* The naive fix's load shed: only enabled when deferral is NOT wired to the
\* re-request loop. The real fix has no such transition.
Drop(b) ==
    /\ ByteBounded
    /\ ~DeferralRerequests
    /\ b \in resident
    /\ ~AdmissionOk(b)
    /\ resident' = resident \ {b}
    /\ dropped' = dropped \cup {b}
    /\ UNCHANGED <<size, pending, queued, processing, processed>>

StartProcessing ==
    /\ queued /= <<>>
    /\ Cardinality(processing) < MaxParallel
    /\ processing' = processing \cup {Head(queued)}
    /\ queued' = Tail(queued)
    /\ UNCHANGED <<size, pending, resident, processed, dropped>>

Complete(b) ==
    /\ b \in processing
    /\ processing' = processing \ {b}
    /\ processed' = processed \cup {b}
    /\ UNCHANGED <<size, pending, resident, queued, dropped>>

\* Quiescence: every block has been broadcast and reached a terminal set
\* (processed, or dropped under the naive fix). Explicit stuttering here
\* keeps TLC's deadlock check meaningful for every non-terminal state.
Done ==
    /\ Broadcasted = Blocks
    /\ pending = {}
    /\ resident = {}
    /\ queued = <<>>
    /\ processing = {}
    /\ UNCHANGED vars

Next ==
    \/ \E b \in Blocks, s \in 1..MaxBlockBytes : Broadcast(b, s)
    \/ \E b \in Blocks : Deliver(b)
    \/ \E b \in Blocks : Admit(b)
    \/ \E b \in Blocks : Defer(b)
    \/ \E b \in Blocks : Drop(b)
    \/ StartProcessing
    \/ \E b \in Blocks : Complete(b)
    \/ Done

\* Weak fairness on delivery, admission and drain: the retriever
\* re-delivers, the drain loop keeps running. Broadcast is deliberately
\* unfair — liveness is conditioned on a block having been broadcast, not
\* on broadcasts occurring.
Fairness ==
    /\ WF_vars(\E b \in Blocks : Deliver(b))
    /\ WF_vars(\E b \in Blocks : Admit(b))
    /\ WF_vars(StartProcessing)
    /\ WF_vars(\E b \in Blocks : Complete(b))

Spec == Init /\ [][Next]_vars /\ Fairness

----------------------------------------------------------------------------

TypeOK ==
    /\ size \in [Blocks -> 0..MaxBlockBytes]
    /\ pending \subseteq Blocks
    /\ resident \subseteq Blocks
    /\ Cardinality(resident) <= MaxDeliveries
    /\ queued \in Seq(Blocks)
    /\ processing \subseteq Blocks
    /\ processed \subseteq Blocks
    /\ dropped \subseteq Blocks

\* THE byte bound: what the observer node lacked. Under ByteBounded this is
\* an invariant; under the current design (ByteBounded = FALSE, the pre-fix
\* config) TLC exhibits a violation whenever (CountCap + MaxParallel) *
\* MaxBlockBytes exceeds ByteCap — the count cap is not a byte cap, and
\* in-flight blocks stay resident on top of the full queue.
Inv_RetainedBytesBounded == RetainedBytes <= ByteCap

\* Total node-side residency, including the delivery window: deferred
\* payloads are counted while resident and released by Defer, so nothing
\* escapes the accounting. The bound is the admission budget plus the
\* transport's bounded concurrent-delivery window.
Inv_TotalResidencyBounded ==
    RetainedBytes + ResidentBytes <= ByteCap + MaxDeliveries * MaxBlockBytes

\* The real fix sheds nothing: dropped stays empty. (Trivial under
\* DeferralRerequests — Drop is disabled — but stated so the gating config
\* fails loudly if a load-shedding transition is ever added without
\* revisiting liveness.)
Inv_NoBlocksShed == dropped = {}

\* Every block that was ever broadcast is eventually processed. The naive
\* fix violates this (broadcast -> deliver -> drop -> stuck in `dropped`);
\* the real fix satisfies it under weak fairness because deferred blocks
\* return to `pending`, are re-delivered, and are admittable whenever
\* budget frees — and the finite universe guarantees budget eventually
\* frees.
Live_AllBroadcastProcessed ==
    \A b \in Blocks : (b \in Broadcasted) ~> (b \in processed)

=============================================================================
