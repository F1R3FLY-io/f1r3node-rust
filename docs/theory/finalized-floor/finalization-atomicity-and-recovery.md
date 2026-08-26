# Atomic finalization and crash recovery

**Status:** protocol-v5 implementation and verification contract  
**Audience:** consensus engineers, node operators, reviewers, and formal-methods maintainers  
**Scope:** the transition from an independently computed Casper finality result to durable node state and externally visible effects

This document specifies how a node turns a valid finalization result into a crash-consistent local state transition without serializing block admission or independent validator computation. It does not change the Casper clique certificate, fault-tolerance threshold, or vote-counting rule. It closes the implementation boundary after those rules have selected a candidate.

![Two finalizer workers evaluate immutable DAG snapshots concurrently. Their candidates meet at a short compare-and-append transaction in the finalization ledger. The winning immutable round is projected in order into DAG metadata, then its idempotent deploy, cosigner, runtime-cache, and event effects are receipted. Projection, effect-completion, and compaction cursors survive restart; losing stale workers have no effects.](diagrams/10-finalization-atomicity-recovery.svg)

## 1. Terms

A **finalization request** asks the local node to recompute its last finalized block (LFB). A request is scheduling work, not a vote.

A **recovery deferral reason** is a typed explanation for refusing to create a
block from a proposal snapshot. `FinalizedFloorMaterializationPending` is the
only reason that schedules finalization automatically. Incomplete committee
slots, an inactive proposer, and a stale recovery permit are authority failures
and remain distinct fail-closed outcomes.

A **finalization evaluation** reads an immutable DAG snapshot and applies the existing Casper finalizer. Evaluations may overlap.

A **durable head** is the last immutable finalization round committed to the
local ledger. Its revision, block hash, height, and record digest form the
compare-and-append authority for that node's next round. Revision and record
digest identify local publication history and are not portable consensus
identities.

A **local finalization witness** is the immutable input persisted before one
round record: canonical genesis, exact local predecessor, target block and
post-state, exact fault-tolerance threshold, the frozen eligible latest-message
map, its supporting dependency closure, the frozen authority-context digest,
and the finalized manifest. Its deterministic digest binds the record to what
the local finalizer evaluated. Another node may retain a different witness and
ledger revision for the same finalized target after observing a different
sequence of intermediate rounds.

A **genesis anchor** is the immutable ledger root: approved-genesis block hash,
block height, and genesis-record digest. It identifies the one chain whose
finalization records this store may contain. It is not the current head and is
never rewritten when the head advances.

A **projection** materializes a committed ledger round in the block-metadata index. Projection is local persistence; it does not manufacture a consensus certificate.

An **effect receipt** records completion of one idempotent post-finalization action for one block in one ledger revision. The four effect kinds are deploy removal, cosigner removal, runtime-cache eviction, and finalized-event publication.

A **cursor** is a durable high-water mark for a contiguous prefix. Projection, effect completion, and receipt compaction have separate cursors because they cross different persistence boundaries.

## 2. Safety contract

Let $`H`$ be the durable ledger revision, $`P`$ the projection cursor, $`E`$ the
completed-effects cursor, and $`C`$ the compaction cursor. The node maintains:

```math
0 \le C \le E \le P \le H
```

For the immutable round-record set $`R`$, projected set $`Q`$, fully completed
effect-round set $`F`$, and prefix operator
$`\operatorname{prefix}(n)=\{1,\ldots,n\}`$:

```math
R = \operatorname{prefix}(H),\qquad
Q = \operatorname{prefix}(P),\qquad
\operatorname{prefix}(E) \subseteq F.
```

Every externally applied effect belongs to a projected round. Every effect receipt belongs to an applied effect. A round enters $`F`$ only after all four effect receipts exist for every block in its immutable manifest.

Let $`G`$ be the persisted genesis anchor and $`G_0`$ the approved genesis
derived during bootstrap. Every initialized state satisfies $`G=G_0`$. Append,
projection, effects, compaction, exact genesis retry, and restart all preserve
$`G`$. The only transition from an empty store writes $`G`$, the revision-zero
head, and all three zero cursors in one transaction.

The block selected for revision $`H+1`$ must strictly increase block height and
must DAG-descend from the block named by revision $`H`$. Consequently, local
materialization cannot replace a finalized floor with an equal-height sibling
or an unrelated branch even if a caller is defective.

## 3. Durable representation

The `finalization-ledger-v5` store contains the following typed records.

| Record | Purpose | Durability rule |
|---|---|---|
| `Genesis` | Immutable approved-genesis hash, height, and genesis-record digest | Created atomically with the revision-zero head and cursors; never overwritten or synthesized |
| `Head` | Current revision, block hash, height, record digest | Written atomically with its successor `Round` |
| `Round(revision)` | Predecessor identity, direct LFB, exact finalized manifest, FT display bits, manifest digest, record digest | Immutable after insertion |
| `ProjectionCursor` | Largest contiguously materialized metadata revision | Advances exactly one committed revision at a time |
| `Effect(id)` | Receipt for `(revision, block, kind)` | Written only after the corresponding idempotent effect |
| `EffectsComplete(revision)` | Out-of-order round-completion marker | Written only after every required receipt exists |
| `EffectsCursor` | Largest contiguous fully effected prefix | May coalesce already completed later rounds |
| `EffectsCompactionCursor` | Largest prefix whose individual receipts were deleted | Never exceeds `EffectsCursor` |

The head and successor round share one typed-store batch, which the LMDB implementation commits in one write transaction. A head can therefore never name a missing round after a successful transaction.

Each round record is hash-chained to its predecessor and commits its sorted manifest. Sorting makes the digest independent of hash-set iteration order. The record digest domain-separates genesis, manifests, and ordinary rounds.

Startup performs an $`O(H)`$ audit of the complete record chain, its genesis
root, head endpoint, and cursor bounds. An ordinary approved-genesis retry uses
an $`O(1)`$ endpoint check: the supplied immutable identity must equal `Genesis`,
the revision-zero record must have the same digest, and the current head must
remain a valid endpoint. A retry after revision $`H>0`$ therefore returns
`AlreadyCanonical` without writing any key. A conflict, partial bootstrap, or
unrooted historical head fails closed.

### 3.1 Genesis identity algorithm

```text
procedure ensure_genesis(supplied):
    atomically read Genesis, Head, and all three cursors

    if every finalization-ledger key is absent:
        genesis := derive_anchor(supplied)
        atomically write Genesis(genesis), Head(revision = 0, genesis),
                         ProjectionCursor(0), EffectsCursor(0),
                         EffectsCompactionCursor(0)
        return Initialized

    require Genesis, Head, and every cursor are present
    require supplied = Genesis
    require revision_zero_record_digest(Genesis) = Genesis.record_digest
    require Head is a valid endpoint and every cursor <= Head.revision
    return AlreadyCanonical without writing
```

The approved-block path also compares the supplied genesis-derived block
metadata with the stored immutable metadata. It may normalize only the local
finality fields that legitimately advance after bootstrap. A same-hash block
with different post-state, bonds, parents, protocol data, or other immutable
metadata is rejected.

## 4. Concurrent algorithm

The scheduling counters are monotonic:

- `requested_through` is the greatest issued request ticket.
- `launched_through` is the greatest ticket covered by a launched evaluation.
- `completed_through` is the greatest ticket covered by a completed evaluation.
- `dispatcher_running` identifies the current dispatcher owner.

Only a worker that returns successfully advances `completed_through`. A worker
error or panic releases its permit but leaves the request uncovered and schedules
a retry after capped exponential backoff from 25 milliseconds through 3.2
seconds. A successful worker covering a newer ticket also covers every earlier
ticket, so a late failure cannot reopen already completed work. Quiescence
requires completed coverage of every requested ticket and no pending retry.

These coordination atomics use sequentially consistent ordering. Loom found that acquire/release operations on separate request and ownership atomics admitted a weak-memory lost-wake execution: the requester could publish work while the releasing dispatcher observed an older request value after clearing ownership. A single total order over these few finalization-cadence atomics removes that execution without locking DAG reads, block admission, network delivery, or validator evaluation.

The worker semaphore bounds resource use but permits the configured number of evaluations to run concurrently. `max-parallel-workers` defaults to `2` and must be at least `1`; zero fails configuration deserialization and the internal constructor fails closed.

The retry scheduler does not alter finalizer input, candidate order, certificate
rules, or ledger publication. It repeats the same complete evaluation boundary
from a fresh coherent base. Backoff limits local resource pressure; it is not a
consensus timeout and cannot turn failure into a negative vote.

Before replay or deploy selection, block creation derives the candidate's exact
certified consensus context and compares it with the context rooted at the
durable materialized floor. A mismatch returns
`FinalizedFloorMaterializationPending`; the proposer idempotently issues a new
finalization request and leaves all deploys in storage. Once materialization
completes, a fresh snapshot retries ordinary creation. A matching context with
incomplete latest-message slots or an inactive proposer returns its own typed
deferral without scheduling finalization, because advancing the local ledger
cannot repair either authority defect.

### 4.1 Evaluation and append

```text
procedure evaluate_and_commit():
    expected, snapshot := capture_coherent_finalization_base()
    candidate := casper_finalizer(snapshot, expected.block)
    if candidate is absent:
        return

    require candidate.height > expected.height
    require dag_ancestor(expected.block, candidate.block)
    require state_preserves(expected.block, candidate.block)

    manifest := immutable_unfinalized_ancestor_closure(candidate.block)
    witness := bind_local_frozen_context(expected, candidate, manifest)
    persist_immutable(witness)
    record := hash_chain(expected, candidate, witness.digest, sorted(manifest))

    outcome := compare_validate_and_append(expected, record)
    case outcome of
        committed:
            reconcile_projection()
            apply_receipted_effects(record)
        exact_retry:
            reconcile_projection()
            apply_receipted_effects(stored_record)
        stale:
            restart with a fresh coherent base and no stale effects
```

The captured base is one identity: revision, block hash, height, record digest,
and the immutable DAG representation against which the certificate was evaluated.
Immediately before append, the implementation rechecks DAG ancestry and state
preservation from that exact block. The ledger compare-and-append then succeeds
only if the durable head still equals the complete captured identity. A changed
head is not substituted into an old certificate: the worker becomes stale and
restarts the complete evaluation.

This distinction is required because DAG ancestry does not imply state
preservation. In the concrete counterexample, workers certify `F0 -> F1` and
`F0 -> C` concurrently. `C` DAG-descends from `F1`, but rejects an effect made
active by `F1`. Late-binding the old `F0 -> C` certificate to the newly committed
`F1` would therefore publish the invalid transition `F1 -> C`. Exact-base
compare-and-append makes that execution impossible.

Evaluation and manifest construction happen outside the append critical
section. The append lock protects only the constant-size head comparison and
the atomic round-plus-head transaction. Two workers starting from the same head
may both finish expensive evaluation; precisely one successor commits, and the
other becomes inert. No validator evaluation or block admission is serialized.

### 4.2 Projection

```text
procedure reconcile_projection():
    for record in ledger.rounds_after(projection_cursor):
        atomically update block metadata for record.manifest
        monotonically raise finalized FT metadata
        persist projection_cursor := record.revision
```

Projection is ordered because block metadata exposes one LFB. A crash after metadata persistence but before cursor persistence repeats an idempotent metadata update. A crash before metadata persistence leaves the cursor unchanged. Effects cannot begin until projection has completed.

### 4.3 Effects and compaction

```text
procedure apply_receipted_effects(record):
    for block in sort_by_height_then_hash(record.manifest):
        for effect_kind in required_effect_kinds:
            unless receipt_or_completed_prefix(record.revision, block, effect_kind):
                apply_idempotent_effect(block, effect_kind)
                persist_receipt(record.revision, block, effect_kind)

    verify every required receipt
    persist EffectsComplete(record.revision)
    advance EffectsCursor across the largest contiguous completed prefix
    delete compactable individual receipts and completion markers
    persist EffectsCompactionCursor
```

The cursor is persisted before receipt deletion. A crash between those operations can retain redundant receipts but cannot lose completion truth. Receipt deletion precedes compaction-cursor advancement, so retrying deletion is safe. At steady state, reconciliation scans only the unfinished suffix instead of all historical finalization rounds.

The effects are deliberately idempotent:

- deleting an already absent deploy is harmless;
- removing absent cosigner sidecar entries is harmless;
- evicting an absent runtime cache entry is harmless;
- finalized events are deduplicated by block hash for the lifetime of a publisher.

The event boundary is at-least-once across a process crash because the event bus and ledger use different persistence domains. Subscribers must therefore use the block hash as their idempotency key. A restarted process has no surviving in-process subscribers from before the crash.

## 5. Crash matrix

| Crash point | Durable observation after restart | Recovery action |
|---|---|---|
| During first genesis bootstrap | Either a pristine store or the complete anchor, revision-zero head, and three cursors | LMDB atomicity selects one state; partial state is never accepted |
| After head advancement, during an approved-genesis retry | Existing anchor and advanced head | Verify identity and endpoints; perform no write |
| Before append transaction | Old head and no round | Re-evaluate normally |
| During append transaction | Either old state or complete round-plus-head | LMDB atomicity selects one state |
| After append, before projection | New round; old projection cursor | Replay metadata projection |
| After metadata write, before projection cursor | Metadata may already be updated; old cursor | Repeat idempotent projection |
| After an effect, before its receipt | Effect may already have occurred | Repeat the idempotent effect |
| After some receipts | Partial receipt set | Apply only missing effects |
| After out-of-order round completion | Completion marker above a cursor gap | Retain marker; close the gap later |
| After effects cursor, before deletion | Completion is durable; receipts remain | Compact redundant receipts |
| After deletion, before compaction cursor | Receipts are absent; old compaction cursor | Repeat harmless deletion |

## 6. Consensus impact

The clique oracle and majority calculation remain unchanged. The ledger begins only after a worker has used those existing rules to produce a candidate. The new boundary enforces three properties that the intended protocol already requires:

1. a local node publishes only a state it can replay and materialize;
2. the finalized floor is monotonic along DAG ancestry;
3. concurrent workers cannot publish competing local heads or effects.

This distinction matters: a clique certificate is consensus evidence, while an
LFB pointer, ledger revision, record digest, and local finalization witness are
local materialization and audit state. A node may evaluate several candidate
certificates concurrently, but it must expose one hash-chained, replayable local
floor. It must not import another node's ledger identity as authority. The
implementation preserves parallel computation and linearizes only the local
durable publication point, following the standard linearizability criterion of
Herlihy and Wing.

Live minority-fork recovery therefore exchanges only ordinary fork-choice tips.
Every missing dependency follows normal bounded certified admission, and only
the receiving node's local finalizer may append a new floor. Same-target nodes
may legitimately have different local round histories. Cold or pruned-state
checkpoint synchronization would require a separately versioned canonical
proof and is not implemented by this live recovery path.

Block admission remains concurrent with finalization evaluation. The global DAG write lock is held only while projecting a committed immutable manifest into metadata; it is not held while computing clique support, scanning candidates, replaying contracts, or waiting for a worker permit.

## 7. Schema and operations

Protocol-v5 admission schema version `9` commits the atomically rooted
hash-chain ledger rule. Version `9` adds the immutable `Genesis` record and
requires atomic bootstrap of that record, the revision-zero head, and all
cursors. Existing data with schema `8`, without a ledger, or with a partial or
unrooted ledger is rejected; the node requires a fresh protocol-v5 genesis or
an explicit verified migration. Silent synthesis of a genesis anchor or ledger
history from mutable block metadata is forbidden.

`finalization_in_progress` is a reference count, not a Boolean. Multiple effect workers may overlap, so one worker completing must not report quiescence while another remains active. Snapshot construction may proceed during this interval using its existing best-effort behavior.

Operators configure parallelism as follows:

```yaml
casper:
  finalizer:
    max-parallel-workers: 2
```

Increasing this value permits more immutable evaluations but also raises peak CPU and snapshot memory use. It does not allow more than one durable successor for a head.

## 8. Verification and executable conformance

| Obligation | Formal artifact | Executable artifact |
|---|---|---|
| One same-head winner; no split head/record | `FinalizationAtomicity.tla`, `FinalizationAtomicity.v` | `one_successor_wins_parallel_same_head_append` |
| Immutable genesis across append, duplicate retry, and restart | `FinalizationGenesisIdentity.tla`; Rocq `rooted_genesis_identity_contract` and capstone | ledger corruption/restart tests, `duplicate_approved_genesis_preserves_advanced_finalization_across_restart`, and four Loom interleaving tests |
| Unsafe reset, overwrite, split bootstrap, and auto-backfill are detected | Four TLC and four Apalache expected-counterexample configurations | partial/unrooted bootstrap and conflicting immutable-metadata regressions |
| No early effect; stale worker inert | Safe model plus unsafe controls | Ledger outcome handling and effect gating |
| No request/release lost wake | TLA+ scheduler invariant; unsafe control | `loom_finalization_atomicity::request_release_race_has_no_lost_wake` |
| Ordered projection after crash | `FinalizationRecovery.tla`; Rocq cursor bounds | `projection_cursor_advances_only_in_committed_order`, restart regression |
| Out-of-order effects close only contiguous prefix | Recovery model; Rocq prefix-extension theorem | property test over arbitrary completion orders; Loom cursor test |
| Receipt compaction never outruns completion | Recovery invariant; Rocq compaction theorem | restart and compaction regressions |
| Candidate preserves finalized floor | finalized-floor lineage models and storage precondition | DAG finalization contract tests |

Run the focused gate:

```bash
./scripts/check-finalization-atomicity.sh
cargo test -p block-storage --features test-internals -- --test-threads=1
cargo test --manifest-path formal/loom/cost_accounting/Cargo.toml --test loom_finalization_atomicity
make -C formal/rocq/finalized_floor -j1 theories/MainTheorem.vo
```

The focused gate runs SANY, exhaustive bounded TLC, bounded symbolic Apalache, and all unsafe controls. The complete finalized-floor gate invokes it automatically through `scripts/check-finalized-floor-ALL.sh`.

## 9. References

- Maurice Herlihy and Jeannette Wing, “Linearizability: A Correctness Condition for Concurrent Objects,” *ACM TOPLAS* 12(3), 1990. [DOI: 10.1145/78969.78972](https://doi.org/10.1145/78969.78972).
- Leslie Lamport, “Time, Clocks, and the Ordering of Events in a Distributed System,” *Communications of the ACM* 21(7), 1978. [DOI: 10.1145/359545.359563](https://doi.org/10.1145/359545.359563).
- [Finalized-floor specification](finalized-floor-specification.md).
- [Finalized-floor verification catalog](finalized-floor-verification.md).
- [Consensus protocol](../../casper/CONSENSUS_PROTOCOL.md).
