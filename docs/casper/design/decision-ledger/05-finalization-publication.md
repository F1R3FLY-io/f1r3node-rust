# D-05 Durable Finalization Publication and Concurrency

**Status.** Proposed. Pending maintainer ratification.

**Kind.** Node-local architecture with three consensus-visible invariants.

**Sources.**

- dev [Consensus Protocol](../../CONSENSUS_PROTOCOL.md) section 7 trigger and FT caching, [Byzantine fault tolerance](../../BYZANTINE_FAULT_TOLERANCE.md) FT propagation.
- PR #216 DR-54 and the DR-55 finalization boundary.
- PR #216 rules R-FINALIZATION-APPEND, R-FINALIZATION-BASE, R-FINALIZATION-LINEAGE, R-FINALIZATION-PROJECTION, R-FINALIZATION-EFFECTS, R-FINALIZATION-COMPACTION, R-FINALIZATION-SCHEDULER, and R-FINALIZATION-PROPOSAL-READINESS.
- PR #216 rules R-VALIDATOR-LOCAL-TRANSACTION, R-LOCAL-ROOT-AUTHORITY, R-LOCAL-SUPPORT, R-ATOMIC-FLOOR-PUBLICATION, R-PARALLEL-FRAME, and R-VALIDATION-RESTART, with invariants S35 to S38.
- PR #216 `finalization_ledger.rs`, `finalization-atomicity-and-recovery.md`, and `FinalizationAtomicity.tla`.

## 1. Question

How does a node record a finalization decision durably, how many evaluations may overlap, and what may a node-local finality marker do?

## 2. Position on dev

Finalization runs after each valid block. A single-flight guard prevents concurrent runs. A `finalization_in_progress` flag blocks snapshot creation during finalization. On advancement the node updates the LFB, cleans deploy storage, and emits `BlockFinalised`. `record_directly_finalized` stores the FT value. `propagate_ft_to_finalized_blocks` raises the cached FT of every finalized block with a lower value.

## 3. Position on PR #216

- A monotonic request sequencer coalesces covered requests and launches up to `finalizer.max-parallel-workers` immutable evaluations. A snapshot may overlap an evaluation.
- A successful round appends one immutable, hash-chained finalization round against the durable head by compare-and-append. A stale result has no effect. The round is projected into metadata in revision order. Deploy, cosigner, runtime-cache, and event effects apply with durable receipts. Restart resumes the unfinished suffix.
- `finalization_in_progress` becomes a reference count for overlapping effect application.
- Each validator runs one local transaction over candidate receipt, floor capture, replay, and publication. Support is emitted only after exact local replay. Concurrent promotion never exposes a torn block, root, and effect tuple.
- The immutable round retains the exact FT computed for its directly finalized block. A raised metadata FT is a display projection, not a replacement certificate.
- A node-local finalization marker cannot evict a deploy. Only a write-once lifecycle terminal verdict authorizes pool and cosigner removal.

## 4. Divergence

| Aspect | dev | PR #216 |
|---|---|---|
| Concurrency | Single-flight finalizer | Bounded parallel immutable evaluations |
| Snapshot during finalization | Blocked | Allowed. Proposal preflight checks floor authority. |
| Durability | Metadata writes | Hash-chained rounds with receipts and recovery cursors |
| FT propagation | Metadata update | Display projection over an immutable round |
| Deploy eviction | On LFB advancement | Only on a lifecycle terminal verdict |

## 5. Options

- **A. Adopt the ledger architecture and ratify every rule.**
- **B. Ratify three consensus-visible invariants. Accept the architecture as an implementation choice with a design document.**
- **C. Keep single-flight finalization.**

## 6. Unification proposal

Adopt option B.

The ledger, the sequencer, and the worker count are node-local. They change no block byte and no verdict. Principle P3 places them in the safe extension surface. They need an accepted design document, not a ratified rule.

Three invariants are consensus-visible and need a ratified row.

1. **Publication atomicity.** A node never exposes a torn block, root, and effect tuple. Restart never publishes a partial round.
2. **Eviction authority.** A node-local finality marker never evicts a deploy. A write-once lifecycle terminal verdict does.
3. **FT identity.** The FT a node reports for a directly finalized block is the value computed at its round. Any raised value is a projection and is labeled as such in the API.

## 7. Ratification checklist

- A design document for the ledger under the finalized-floor theory directory, accepted by review.
- `FinalizationAtomicity.tla` safe and unsafe configurations in the TLA+ gate list.
- After ratification, edit the protocol section 7 trigger and FT caching text, and the BFT document's propagation paragraph.

## 8. Open questions

1. Does any peer-observable ordering change with parallel evaluation? R-FINALIZATION-SCHEDULER says only the publication point is linearized. A test that fixes the point would settle it.
2. Which API field carries the projection label for a raised FT?
