--------------------------- MODULE BlockAdmission ---------------------------
(***************************************************************************)
(* Byte-bounded admission for the inbound block-processing pipeline,       *)
(* written to DISCOVER (not assume) the two failure modes bracketing the   *)
(* design space:                                                           *)
(*                                                                         *)
(*   1. The PRE-FIX design (ByteBounded = FALSE) bounds the processor      *)
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
(*   casper/src/rust/blocks/block_processing_queue.rs                      *)
(*     try_enqueue reserves encoded bytes before nonblocking count         *)
(*     admission. Queue-owned reservations transfer to the processor.      *)
(*   node/src/rust/instances/block_processor_instance.rs                   *)
(*     create/run — drain loop holding a Semaphore(max_parallel_blocks)    *)
(*     (`processing` here, |processing| <= MaxParallel); a dequeued        *)
(*     BlockMessage stays resident until its replay completes, so the      *)
(*     byte budget must cover queued + in-flight, not queued alone.        *)
(*   casper/src/rust/engine/running.rs                                     *)
(*     the decoded BlockMessage held by a receiving task between arrival   *)
(*     and the admission decision (`resident` here). Backpressure under    *)
(*     the pre-fix design is a block waiting in `resident` for queue       *)
(*     capacity; the fix releases it and reopens retriever state.           *)
(*   casper/src/rust/engine/block_retriever.rs                             *)
(*     defer_for_admission — the re-request pool transition that            *)
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
EXTENDS Naturals, Sequences, FiniteSets, Apalache

CONSTANTS
    \* @type: Set(Str);
    Blocks,              \* model values: the finite universe of blocks
    \* @type: Int;
    MaxBlockBytes,       \* max nondeterministic block size (scaled, e.g. 2)
    \* @type: Int;
    ByteCap,             \* retained-bytes budget for queued + in-flight
    \* @type: Int;
    CountCap,            \* the mpsc capacity (message count), in force always
    \* @type: Int;
    RequestCap,          \* bounded BlockRetriever hash-tracking capacity
    \* @type: Int;
    MaxParallel,         \* semaphore permits (F1R3_MAX_PARALLEL_BLOCKS)
    \* @type: Int;
    MaxDeliveries,       \* concurrent delivered-but-undecided payloads
    \* @type: Bool;
    ByteBounded,         \* TRUE = byte-gated admission; FALSE = pre-fix design
    \* @type: Bool;
    DeferralRerequests   \* TRUE = deferred blocks stay requestable (the fix);
                         \* FALSE = over-budget blocks are dropped (the wedge)

ASSUME MaxBlockBytes <= ByteCap
ASSUME MaxParallel >= 1 /\ CountCap >= 1 /\ MaxBlockBytes >= 1
ASSUME MaxDeliveries >= 1
ASSUME RequestCap >= 1

VARIABLES
    \* @type: Str -> Int;
    size,       \* [Blocks -> Nat] block size, fixed at broadcast (0 = not yet)
    \* @type: Set(Str);
    tracked,    \* bounded BlockRetriever request-state keys
    \* @type: Set(Str);
    unsolicited,\* one-shot full payloads received without a request slot
    \* @type: Set(Str);
    pending,    \* broadcast, not delivered or deferred (network + re-request
                \* pool); retains NO payload bytes — Defer's release target
    \* @type: Set(Str);
    resident,   \* delivered, bytes held awaiting the admission decision
    \* @type: Seq(Str);
    queued,     \* FIFO processor queue (sequence of blocks)
    \* @type: Set(Str);
    processing, \* dequeued, replay in progress
    \* @type: Set(Str);
    processed,  \* replay complete
    \* @type: Set(Str);
    dropped     \* shed under the naive fix; unreachable under the real fix

vars == <<size, tracked, unsolicited, pending, resident, queued, processing, processed, dropped>>

\* @type: Seq(Str) => Set(Str);
SeqRange(s) == {s[i] : i \in DOMAIN s}

\* @type: (Int, Str) => Int;
AddBlockBytes(total, block) == total + size[block]

\* @type: Set(Str) => Int;
SetBytes(S) == ApaFoldSet(AddBlockBytes, 0, S)

\* @type: Seq(Str) => Int;
SeqBytes(s) == SetBytes(SeqRange(s))

\* Admission's budget: queued + in-flight. Delivery-window residency is
\* accounted separately (ResidentBytes) and bounded by MaxDeliveries.
RetainedBytes == SeqBytes(queued) + SetBytes(processing)

ResidentBytes == SetBytes(resident)

Broadcasted ==
    {b \in Blocks : size[b] # 0}

Init ==
    /\ size = [b \in Blocks |-> 0]
    /\ tracked = {}
    /\ unsolicited = {}
    /\ pending = {}
    /\ resident = {}
    /\ queued = <<>>
    /\ processing = {}
    /\ processed = {}
    /\ dropped = {}

Broadcast(b, s) ==
    /\ b \notin Broadcasted
    /\ size' = [size EXCEPT ![b] = s]
    /\ IF Cardinality(tracked) < RequestCap
          THEN /\ tracked' = tracked \cup {b}
               /\ pending' = pending \cup {b}
               /\ unsolicited' = unsolicited
          ELSE /\ tracked' = tracked
               /\ pending' = pending
               /\ unsolicited' = unsolicited \cup {b}
    /\ UNCHANGED <<resident, queued, processing, processed, dropped>>

Reannounce(b) ==
    /\ b \in Broadcasted \ (processed \cup dropped)
    /\ b \notin tracked
    /\ b \notin resident \cup SeqRange(queued) \cup processing
    /\ Cardinality(tracked) < RequestCap
    /\ tracked' = tracked \cup {b}
    /\ pending' = pending \cup {b}
    /\ unsolicited' = unsolicited \ {b}
    /\ UNCHANGED <<size, resident, queued, processing, processed, dropped>>

\* Transport hands a payload to the node: bytes become resident. Bounded by
\* the concurrent-delivery window, not by the admission budget — admission
\* cannot govern what peers send, only what it retains past the decision.
Deliver(b) ==
    /\ b \in pending
    /\ Cardinality(resident) < MaxDeliveries
    /\ resident' = resident \cup {b}
    /\ pending' = pending \ {b}
    /\ UNCHANGED <<size, tracked, unsolicited, queued, processing, processed, dropped>>

DeliverUntracked(b) ==
    /\ b \in unsolicited
    /\ Cardinality(resident) < MaxDeliveries
    /\ resident' = resident \cup {b}
    /\ unsolicited' = unsolicited \ {b}
    /\ UNCHANGED <<size, tracked, pending, queued, processing, processed, dropped>>

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
    /\ UNCHANGED <<size, tracked, unsolicited, pending, processing, processed, dropped>>

\* The fix's deferral: the payload buffer is released (b leaves `resident`,
\* whose bytes are counted, for `pending`, which retains none) and the block
\* stays requestable. This transition IS the release/re-delivery model:
\* deferred memory is accounted while held and provably relinquished here.
\* Under the historical design (~ByteBounded) there is no deferral — a block
\* that cannot be admitted waits in `resident` (mpsc backpressure).
Defer(b) ==
    /\ ByteBounded
    /\ DeferralRerequests
    /\ b \in resident
    /\ ~AdmissionOk(b)
    /\ pending' = IF b \in tracked THEN pending \cup {b} ELSE pending
    /\ resident' = resident \ {b}
    /\ UNCHANGED <<size, tracked, unsolicited, queued, processing, processed, dropped>>

\* The naive fix's load shed: only enabled when deferral is NOT wired to the
\* re-request loop. The real fix has no such transition.
Drop(b) ==
    /\ ByteBounded
    /\ ~DeferralRerequests
    /\ b \in resident
    /\ ~AdmissionOk(b)
    /\ resident' = resident \ {b}
    /\ dropped' = dropped \cup {b}
    /\ tracked' = tracked \ {b}
    /\ UNCHANGED <<size, unsolicited, pending, queued, processing, processed>>

StartProcessing ==
    /\ queued /= <<>>
    /\ Cardinality(processing) < MaxParallel
    /\ processing' = processing \cup {Head(queued)}
    /\ queued' = Tail(queued)
    /\ UNCHANGED <<size, tracked, unsolicited, pending, resident, processed, dropped>>

Complete(b) ==
    /\ b \in processing
    /\ processing' = processing \ {b}
    /\ processed' = processed \cup {b}
    /\ tracked' = tracked \ {b}
    /\ UNCHANGED <<size, unsolicited, pending, resident, queued, dropped>>

\* Quiescence: every block has been broadcast and reached a terminal set
\* (processed, or dropped under the naive fix). Explicit stuttering here
\* keeps TLC's deadlock check meaningful for every non-terminal state.
Done ==
    /\ Broadcasted = Blocks
    /\ processed \cup dropped = Blocks
    /\ tracked = {}
    /\ unsolicited = {}
    /\ pending = {}
    /\ resident = {}
    /\ queued = <<>>
    /\ processing = {}
    /\ UNCHANGED vars

Next ==
    \/ \E b \in Blocks, s \in 1..MaxBlockBytes : Broadcast(b, s)
    \/ \E b \in Blocks : Reannounce(b)
    \/ \E b \in Blocks : Deliver(b)
    \/ \E b \in Blocks : DeliverUntracked(b)
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
    /\ \A b \in Blocks : WF_vars(Reannounce(b))
    /\ \A b \in Blocks : WF_vars(Deliver(b))
    /\ \A b \in Blocks : WF_vars(Admit(b))
    /\ \A b \in Blocks : WF_vars(Defer(b))
    /\ \A b \in Blocks : WF_vars(Drop(b))
    /\ WF_vars(StartProcessing)
    /\ \A b \in Blocks : WF_vars(Complete(b))

Spec == Init /\ [][Next]_vars /\ Fairness

----------------------------------------------------------------------------

TypeOK ==
    /\ size \in [Blocks -> 0..MaxBlockBytes]
    /\ tracked \subseteq Blocks
    /\ Cardinality(tracked) <= RequestCap
    /\ unsolicited \subseteq Blocks
    /\ pending \subseteq Blocks
    /\ pending \subseteq tracked
    /\ resident \subseteq Blocks
    /\ Cardinality(resident) <= MaxDeliveries
    /\ Len(queued) <= CountCap
    /\ \A index \in DOMAIN queued : queued[index] \in Blocks
    /\ processing \subseteq Blocks
    /\ processed \subseteq Blocks
    /\ dropped \subseteq Blocks
    /\ tracked \subseteq pending \cup resident \cup SeqRange(queued) \cup processing
    /\ unsolicited \subseteq Broadcasted
    /\ unsolicited \cap tracked = {}
    /\ unsolicited \cap resident = {}
    /\ unsolicited \cap SeqRange(queued) = {}
    /\ unsolicited \cap processing = {}
    /\ unsolicited \cap processed = {}
    /\ unsolicited \cap dropped = {}
    /\ Cardinality(SeqRange(queued)) = Len(queued)
    /\ pending \cap resident = {}
    /\ pending \cap SeqRange(queued) = {}
    /\ pending \cap processing = {}
    /\ resident \cap SeqRange(queued) = {}
    /\ resident \cap processing = {}
    /\ SeqRange(queued) \cap processing = {}
    /\ tracked \cap processed = {}
    /\ tracked \cap dropped = {}
    /\ processed \cap dropped = {}
    /\ processed \cup dropped \subseteq Broadcasted

\* THE byte bound: what the observer node lacked. Under ByteBounded this is
\* an invariant; under the historical design (ByteBounded = FALSE, the pre-fix
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

Inv_RetrieverTrackingBounded == Cardinality(tracked) <= RequestCap

\* The real fix sheds nothing: dropped stays empty. (Trivial under
\* DeferralRerequests — Drop is disabled — but stated so the gating config
\* fails loudly if a load-shedding transition is ever added without
\* revisiting liveness.)
Inv_NoBlocksShed == dropped = {}

Safety ==
    /\ TypeOK
    /\ Inv_RetainedBytesBounded
    /\ Inv_TotalResidencyBounded
    /\ Inv_RetrieverTrackingBounded
    /\ Inv_NoBlocksShed

\* Every block that was ever broadcast is eventually processed. The naive
\* fix violates this (broadcast -> deliver -> drop -> stuck in `dropped`);
\* the real fix satisfies it under weak fairness because deferred blocks
\* return to `pending`, are re-delivered, and are admittable whenever
\* budget frees. Per-block weak fairness on Reannounce means a request that
\* first arrived at tracker capacity is reconsidered after an older finite
\* request completes; no assumption requires the work universe to fit the
\* tracker simultaneously.
Live_AllBroadcastProcessed ==
    \A b \in Blocks : (b \in Broadcasted) ~> (b \in processed)

=============================================================================
