# Block admission and transport residency

TLA+ model of byte-bounded admission for the block-processing pipeline,
written from the counterexamples found after the 2026-08-04 daily-soak breach
(run 30880995655). It distinguishes the historical count-only queue, a lossy
byte-bound repair, the implemented request-backed byte-bound refinement, and
the historical whole-buffer materialization path.

The same directory models the transport boundary immediately before block
admission: byte/item ownership, compressed wire residency, lazy chunking,
fanout, remote completion, HTTP/2 admission, service parallelism, peer-slot
initialization, queue retirement, and request-local validation.

## Model ↔ code

| Model | Code |
|---|---|
| `queued` (FIFO, `CountCap`) | `BlockProcessingQueueSender::try_enqueue` — nonblocking mpsc count admission |
| `RetainedBytes` (≤ `ByteCap`) | an RAII byte reservation owned by each queue item and moved intact into `BlockProcessorInstance` for replay |
| `processing` (≤ `MaxParallel`) | `BlockProcessorInstance` semaphore drain; the byte reservation remains owned until replay completes |
| `resident` (≤ `MaxDeliveries`) | decoded inbound `BlockMessage` held by a receiving task between arrival and the admission decision |
| `tracked` (≤ `RequestCap`) | `BlockRetriever::requested_blocks`; existing unresolved work is never evicted to admit a new hash, while queue ownership remains valid without a tracker entry |
| `unsolicited` | a one-shot full `BlockMessage` received while the request tracker is full; it may enter the byte-owning queue without consuming a hash slot |
| `pending` re-request pool | unresolved tracked hashes; no decoded block payload is retained |
| `Reannounce` | a later hash announcement or dependency scan admits previously untracked work after request capacity becomes available |
| `Defer` transition | `Running::handle` removes the in-flight marker, releases the payload, and reopens retriever state when a slot exists; otherwise only later reannouncement can readmit the hash |
| `RetainedBytes + ResidentBytes` | total node-side residency incl. the delivery window (`Inv_TotalResidencyBounded`) |
| `BufferScanResidency.scanning` | the queue coordinator's shared mutex serializes startup and replay-completion scans; each scan loads and releases one `BlockMessage` at a time while returning only sorted hashes |
| `TransportPayloadResidency.Live` | inbound or outbound payloads whose RAII byte/item reservation has not reached its terminal owner |
| `TransportPayloadResidency.reportedSuccess` | public stream successes; membership requires completion by every target peer, not local enqueue |
| `TransportConcurrency.TransportActive` | HTTP/2 requests initiated before or after SETTINGS; bounded independently from application handler execution |
| `TransportConcurrency.Handling` | gRPC application calls holding a shared service semaphore permit before and during generated protobuf decoding; their worst-case decoded ownership is `Cardinality(Handling) × MaxDecodedBytes` |
| `TransportConcurrency.Retained` | acknowledged payloads still owned by a bounded peer queue or handler |
| `TransportPeerLifecycle.initGuards` | map-locked ownership spanning peer-slot lookup and `OnceCell` initialization |
| `TransportPeerLifecycle.sendGuards` | the single-word activity gate carried through queue residence and handling |

## Configurations

| Config | Knobs | Expected | Shows |
|---|---|---|---|
| `MC_BlockAdmission` | `ByteBounded`, `DeferralRerequests` | **clean** (CI-gated) | The fix: byte and total-residency bounds, no tracked work shed, and finite-work progress under explicit per-block fairness |
| `MC_BlockAdmission_pre_fix` | `¬ByteBounded` | `Inv_RetainedBytesBounded` violated | Historical design: a count cap admits up to `(CountCap + MaxParallel) × MaxBlockBytes` regardless of budget |
| `MC_BlockAdmission_drop_pre_fix` | `ByteBounded`, `¬DeferralRerequests` | `Live_AllBroadcastProcessed` violated | The naive fix: shedding over-budget blocks wedges the shard |
| `MC_BufferScanResidency` | `ScanBounded` | **clean** (CI-gated) | Durable buffered work is scanned one payload at a time, stays inside the total residency envelope, and eventually processes |
| `MC_BufferScanResidency_pre_fix` | `¬ScanBounded` | `Inv_ScannerSinglePayload` violated | Historical scan materializes every eligible buffered block at once |
| `TransportPayloadResidency` | byte/item bounds, compressed-wire charge, lazy chunks, completion ACK | **clean** (CI-gated) | Exact live residency, one shared fanout reservation, terminal release, and success only after remote completion |
| `TransportPayloadResidencyCountOnlyUnsafe` | no byte bound | `Inv_ActualResidencyBounded` violated | A count cap alone does not bound retained bytes |
| `TransportPayloadResidencyDecodedOnlyUnsafe` | compressed wire omitted | `Inv_ReservationCoversActual` violated | Decoded length alone undercounts simultaneous compressed and decoded ownership |
| `TransportPayloadResidencyEagerChunksUnsafe` | eager copied chunks | `Inv_ReservationCoversActual` violated | A second full wire representation escapes the reservation |
| `TransportPayloadResidencyEnqueueSuccessUnsafe` | local enqueue reports success | `Inv_SuccessRequiresRemoteCompletion` violated | A caller can observe success before any peer completes |
| `TransportConcurrency` | aligned finite wire/item window; smaller handler limit; decoded-message bound | **clean** (CI-gated) | Pre-SETTINGS bursts are accepted while application execution and pre-reservation decoder bytes remain bounded |
| `TransportConcurrencyWireLimitUnsafe` | wire limit below the client pre-SETTINGS window | `Inv_NoRequestsRefused` violated | HTTP/2 resets an otherwise valid initial request before service execution |
| `TransportConcurrencyItemLimitUnsafe` | item limit below the wire window | `Inv_NoPayloadBudgetRejection` violated | An admitted tiny request is rejected while earlier ACKed work remains live |
| `TransportConcurrencyHandlerLimitUnsafe` | service execution bypasses the handler semaphore | `Inv_PreReservationDecodedBounded` violated | Parallel generated decoders escape the configured aggregate byte envelope |
| `TransportPeerLifecycle` | guarded initialization, idle-only retirement, request-local validation | **clean** (CI-gated) | Accepted work retains an owner and parallel requests cannot exchange validation context |
| `TransportPeerLifecycleInitRaceUnsafe` | unguarded initialization | `Inv_InitializingOwnsMappedSlot` violated | Cleanup orphans a new queue between lookup and initialization |
| `TransportPeerLifecycleActiveEvictionUnsafe` | post-ACK active retirement | `Inv_AcknowledgedWorkPreserved` violated | Cleanup aborts resident or handling work after ACK; the control cannot terminate through unrelated idle cleanup |
| `TransportPeerLifecycleSharedContextUnsafe` | shared validation context | `Inv_ValidationUsesRequestContext` violated | Concurrent headers can be decided using another request's network |

Pre-fix configs are required formal counterexamples. Run the area gate from
the repository root; it bounds the TLC heap and workers, writes search state
under `target/`, verifies the safe models, and requires every named unsafe
configuration to fail for its exact advertised property:

```bash
scripts/check-cost-accounted-rho-block-admission.sh
```

## Implementation obligations

The block models impose seven implementation obligations: budget queued and
in-flight bytes; carry reservation ownership with the queued value; defer
tracked work rather than shed it; release the payload on deferral; derive the
byte cap from a protocol block-size ceiling; bound retriever hash tracking
without evicting existing unresolved work; and serialize dependency-buffer
materialization to one payload while scheduling deterministic hashes. The full
claims-to-tools mapping, verification ladder, and process conventions live in
[docs/formal-verification.md](../../../docs/formal-verification.md).

The transport models add six obligations: reserve every post-handoff wire and
decoded representation; bound pre-reservation generated-decoder ownership by
the service semaphore and checked message-size product; retain both byte and
item guards until the last owner releases; generate chunks lazily and share
fanout payloads; report success only after each remote ACK; align the finite
pre-SETTINGS wire and item windows while making slot initialization, activity,
cleanup, and request validation linearizable. The
implementation rationale and operational envelope are specified in
[P2P transport resource and completion semantics](../../../docs/node/transport-resource-lifecycle.md).

The gate checks each `Safety` conjunction with both TLC and bounded symbolic
Apalache exploration. TLC additionally checks temporal progress and executes
the exact historical negative controls.
