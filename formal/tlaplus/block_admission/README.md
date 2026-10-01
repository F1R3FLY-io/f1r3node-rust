# Block admission — byte-bounded inbound pipeline

TLA+ model of byte-bounded admission for the block-processing pipeline,
written for the 2026-08-04 daily-soak breach (run 30880995655): after the
replay-cache runaway fix, per-node attribution showed the readonly observer
peaking at 6,492MB against a 947–3,371MB validator baseline — role-shaped
retention on the receive-only path, whose processor queue is bounded by
message count (2048), not bytes.

## Model ↔ code

| Model | Code |
|---|---|
| `queued` (FIFO, `CountCap`) | `node/src/rust/runtime/setup.rs` — `block_processor_queue` mpsc (count cap in force in every mode) |
| `processing` (≤ `MaxParallel`) | `node/src/rust/instances/block_processor_instance.rs` — semaphore drain |
| `resident` (≤ `MaxDeliveries`) | decoded inbound `BlockMessage` held by a receiving task between arrival and the admission decision |
| `pending` re-request pool | `casper/src/rust/engine/block_retriever.rs` — requested-blocks / dependency recovery; retains **no** payload bytes |
| `Defer` transition | deferral **releases** the payload buffer (resident → pending); re-delivery is a later `Deliver`, not a held buffer |
| `RetainedBytes` | bytes held by queued **plus in-flight** messages |
| `RetainedBytes + ResidentBytes` | total node-side residency incl. the delivery window (`Inv_TotalResidencyBounded`) |

## Configurations

| Config | Knobs | Expected | Shows |
|---|---|---|---|
| `MC_BlockAdmission` | `ByteBounded`, `DeferralRerequests` | **clean** (CI-gated) | The fix: byte bound and total-residency bound, nothing shed, every broadcast block eventually processed |
| `MC_BlockAdmission_pre_fix` | `¬ByteBounded` | `Inv_RetainedBytesBounded` violated | Today's design: a count cap admits up to `(CountCap + MaxParallel) × MaxBlockBytes` (full queue plus in-flight replay) regardless of budget |
| `MC_BlockAdmission_drop_pre_fix` | `ByteBounded`, `¬DeferralRerequests` | `Live_AllBroadcastProcessed` violated | The naive fix: shedding over-budget blocks wedges the shard |

Pre-fix configs are the formal-side counter-examples; run them manually
(they are excluded from CI, same convention as the slashing suite):

```bash
java -jar ~/.tla/tla2tools.jar -workers auto \
  -config MC_BlockAdmission_pre_fix.cfg MC_BlockAdmission_pre_fix.tla
```

## Implementation obligations

This model imposes four obligations on any implementing PR (budget
queued **and** in-flight; defer, never drop; deferral releases the
payload buffer — re-delivery comes from the retriever, not a held
buffer; cap ≥ max block size). The
full treatment — claims-to-tools mapping, the remaining verification
ladder, and the process conventions this area follows — lives in
[docs/formal-verification.md](../../../docs/formal-verification.md).
