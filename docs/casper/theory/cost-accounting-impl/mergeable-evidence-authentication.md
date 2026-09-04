# Mergeable Evidence Authentication

**Status:** Implemented protocol-6 local replay refinement. The aggregate
verification gate remains pending.

**Scope:** Mergeable-channel evidence identity, publication, synchronization,
lookup, deletion, and replay reconstruction.

**Governing sources:** [*Cost-Accounted Rho
Calculus*](https://github.com/F1R3FLY-io/publications/blob/main/cost-accounting/cost-accounted-rho.tex)
transaction, witness, database-atomicity, and replay requirements;
[*Continued Interactive GSLTs and the Cost
Endofunctor*](https://github.com/F1R3FLY-io/publications/blob/main/cost-accounting-as-monad/continued-gslt-cost-v2.tex)
local-sufficiency and schedule-independent cost boundaries; DR-50 and DR-51.

The papers require validators to agree on the authenticated transition and its
cost witness. F1R3node additionally derives a per-deployment vector of
mergeable-channel differences for later multi-parent merging. That vector is
not committed in `BlockMessage`, so it is auxiliary evidence: a validator may
cache it, but may accept it only after deriving it by replaying the committed
block locally.

![A downloaded block is authenticated independently of any peer cache response. The receiving validator ignores peer-supplied merge evidence, locally replays the block from its authenticated pre-state, validates every effect and the final post-state, derives a complete execution key, and only then publishes merge evidence for deterministic lookup. Distinct equivocations retain distinct keys regardless of arrival order.](../diagrams/mergeable-evidence-authentication.svg)

## Terms

| Term | Meaning |
| --- | --- |
| Mergeable evidence | The ordered per-deployment `NumberChannelsDiff` vectors used by the multi-parent merge calculation. |
| Execution identity | The complete tuple that determines one replay: pre-state, post-state, creator, sequence number, canonical replay payload, and genesis mode. |
| Legacy key | The former cache key containing only post-state, creator, and sequence number. |
| Canonical replay payload | Domain-separated Blake2b-256 over the executed user and system deployment witnesses after event-log canonicalization, including the genesis discriminator. |
| Local replay | Execution by the receiving validator from the block's authenticated pre-state, followed by exact effect and final-state validation. |
| Auxiliary cache | Reconstructible node-local storage that cannot authorize a block, vote, finalization certificate, or state transition. |
| Declared post-state root | The state root that the signed block commits. |
| Computed post-state root | The state root that local replay calculates from the authenticated pre-state and payload. |
| Publication barrier | The exact-root check that must succeed before durable evidence and its cache entry become visible. |

## Why the legacy boundary was unsafe

The former key was:

```math
K_{old}(B)=(H_{post}(B),creator(B),sequence(B)).
```

Two equivocations can share that tuple while carrying different pre-states,
deployment witnesses, system deployments, or mergeable differences. Inserting
both into one key makes the last arrival overwrite the first. Validators that
receive the equivocations in opposite orders can then load different evidence
for the same requested block. Because `BlockIndex` zips that evidence with the
block's processed effects, the overwrite is consensus-significant rather than
a harmless cache miss.

The network path created a second independent defect. Last-finalized-state
synchronization accepted a peer's bincode payload directly into the mergeable
store. A block commits its state roots and processed deployment witnesses, but
not the derived mergeable vector. The receiver therefore had no authenticated
value against which to compare the peer payload. Neither transport
authentication nor possession of the block proves that the supplied vector was
derived from that block.

## Complete execution key

The cache now uses:

```math
K_{v2}(B)=\operatorname{Encode}\bigl(
H_{pre}(B),H_{post}(B),creator(B),sequence(B),
h_{v2}(U(B),S(B),genesis(B))
\bigr).
```

Here $`U(B)`$ is the ordered executed user-deployment witness and $`S(B)`$ is
the ordered system-deployment witness. For an ordinary block, admission-rejected
records are excluded because they were not executed and have no mergeable
vector. Genesis includes all genesis deployments. The payload digest:

1. begins with the domain `f1r3node:replay-payload:v2`;
2. records the user-deployment count;
3. canonicalizes each user event log as its replay-semantic multiset,
   protobuf-encodes the complete processed deployment, and length-prefixes it;
4. records the system-deployment count;
5. applies the same canonicalization and length-prefixing to every complete
   processed system deployment;
6. appends the genesis discriminator; and
7. hashes the resulting bytes with Blake2b-256.

Event-log permutation is the only intentional quotient. RSpace rigs replay
evidence as a multiset, the merge index computes the state difference from the
same event set and exact pre/post roots, and Rocq proves user and system trace
permutations replay-equivalent. Deployment order and every non-log protobuf
field remain part of the digest. Thus schedule-only permutations may safely
reuse one entry, while any semantically distinct execution receives a distinct
key.

The block hash is intentionally absent. Proposal execution must save evidence
before the final block hash exists. The complete transition identity already
separates distinct executions, while byte-identical executions may safely reuse
the same derived evidence.

## Publication algorithm

Evidence publication is the final action of replay, not part of speculative
execution.

```text
deriveMergeableEvidence(block):
    key = completeExecutionKey(block)
    if localStore contains key:
        return localStore[key]

    candidateState, candidateEvidence = replayWithoutPublishing(
        preState = block.preState,
        executedPayload = admittedPayload(block)
    )

    validateEveryRecordedEffect(candidateState, block)
    if candidateState != block.postState:
        resetActiveRoot(block.preState)
        reject EffectStateMismatch(final-post-state)

    localStore[key] = candidateEvidence
    return candidateEvidence
```

Every failure before the final assignment resets the replay runtime to the
authenticated block pre-state and publishes no entry. A replay-cache hit is
usable only if the fully bound mergeable entry also exists; otherwise the node
performs full replay. Garbage collection and explicit deletion derive the same
complete key from the block, so one equivocation cannot remove another's entry.

The publication barrier compares the computed and declared post-state roots.
Only exact equality permits durable publication and cache publication.

The durable evidence write occurs before the cache write. A concurrent reader
can observe missing cache state and recover it from durable evidence. It cannot
observe authoritative cache state that has no durable source.

Post-state mismatch restores the authenticated pre-state. It leaves both stores
unchanged. The result is objective replay rejection, not a retryable dependency.

## Network and initialization protocol

Last-finalized-state synchronization transfers authenticated block and trie
data. It does not transfer trusted mergeable evidence.

- An initializer never requests mergeable entries.
- A running node receiving a legacy request silently ignores it when the block
  is absent. When the block exists, it returns an empty compatibility response.
- An initializer ignores every mergeable response, including nonempty payloads.
- After block synchronization, missing evidence is reconstructed by local replay
  before a merge can use it.

The request and response protobuf variants remain decodable for rolling wire
compatibility, but `serialized_entry` is deprecated and has no authority. This
is fail-closed: old traffic cannot mutate consensus-adjacent cache state, and
honest nodes recover availability by deterministic replay.

## Invariants

For validator $`v`$, block $`B`$, cache $`M_v`$, and locally replayed set
$`R_v`$:

```math
B\in R_v\Rightarrow M_v[K_{v2}(B)]=Evidence(B).
```

```math
M_v[k]\neq\varnothing\Rightarrow
\exists B\in R_v:\ k=K_{v2}(B)\land M_v[k]=Evidence(B).
```

For distinct execution identities $`B_1`$ and $`B_2`$:

```math
K_{v2}(B_1)\neq K_{v2}(B_2).
```

For validators replaying the same finite block set in opposite orders:

```math
M_1=M_2=\{K_{v2}(B)\mapsto Evidence(B)\mid B\in R\}.
```

These laws concern derived merge inputs. They do not change majority voting,
clique calculation, finality thresholds, fork choice, or RSpace matching.

## Finalized evidence retirement

Retirement is local cache reclamation, not a consensus transition. Let $`B`$
be a block, let $`K_{v2}(B)`$ be its complete execution key, and let $`M`$ be
the local merge-evidence store. The retirement operation is:

```math
Retire(B,M)=M\setminus\{K_{v2}(B)\}.
```

It must satisfy both exactness laws:

```math
Retire(B,M)[K_{v2}(B)]=\varnothing.
```

```math
B\neq C\Rightarrow
K_{v2}(B)\neq K_{v2}(C)\Rightarrow
Retire(B,M)[K_{v2}(C)]=M[K_{v2}(C)].
```

The second law is why garbage collection receives the complete authenticated
block rather than the legacy post-state, creator, and sequence tuple. Two
equivocations may share that tuple while differing in pre-state or replay
payload. Retiring through the legacy tuple can erase the live execution's
evidence; retiring through $`K_{v2}`$ cannot.

Eligibility is conservative and independent of key exactness. A block is
eligible only when it is finalized, lies strictly beyond the configured
parent-depth horizon plus its safety buffer, has at least one child, has at
least one concrete latest-message witness, and every recorded latest message
has advanced through one of those children along any parent path in the DAG.
This includes a child that appears only on a later block's secondary-parent
branch: multi-parent integration is still causal advancement and is sufficient
for retirement. Restricting this check to the main-parent spine is safe against
premature deletion, but incomplete because it can retain unreachable evidence
forever. An empty latest-message set cannot establish advancement and therefore
retains the evidence. Unknown DAG data fails closed. The depth convention is shared exactly with
receiver-side parent validation. If $`N`$ is `latest_block_number()`—the next
height boundary, equal to the highest stored block number plus one—and $`n_B`$
is the candidate block number, then:

```math
d(B)=N-n_B.
```

For configured parent depth $`D`$ and safety buffer $`G`$, retention continues
through $`d(B)\leq D+G`$ and retirement becomes eligible only when
$`d(B)>D+G`$. The same boundary makes $`B`$ inadmissible as a non-genesis
parent, so GC cannot retire an execution that the receiver would still admit
through the configured horizon. The node runtime owns the periodic GC loop;
the finalizer only records finalized state and never deletes evidence while its
finalization effect is active. Each interval takes an immutable DAG snapshot
before evaluating the following algorithm:

```text
retire-finalized-evidence(dag, block-store, evidence-store, configuration):
    enumerate finalized blocks in topological order
    for each block identity:
        continue unless the block is beyond the retention horizon
        continue unless at least one latest message is recorded
        continue unless every latest message DAG-descends from a child
        load the authenticated block
        derive its complete execution key from consensus data
        delete exactly that key and count the deletion if it existed
    return the deletion count
```

If a node later requires retired evidence, the authority rule does not change:
the node reconstructs it by authenticated local replay. Retirement never makes
a peer response authoritative and never changes block validity, majority
support, or finalized state.

## Implementation map

| Responsibility | Implementation |
| --- | --- |
| Canonical payload digest and complete key | `casper/src/rust/util/rholang/runtime_manager.rs` |
| Uncommitted replay and post-state-gated publication | `RuntimeManager::{replay_compute_state_uncommitted,replay_block_from_consensus_data}` |
| Local reconstruction on cache miss | `RuntimeManager::ensure_mergeable_entry` |
| Merge lookup and garbage collection | `load_mergeable_channels`, `delete_mergeable_channels`, `mergeable_channels_gc.rs` |
| Initializer peer-input exclusion | `casper/src/rust/engine/initializing.rs` |
| Empty rolling-compatibility response | `casper/src/rust/engine/running.rs` |
| Block-only last-finalized-state requester | `casper/src/rust/engine/lfs_block_requester.rs` |

## Verification matrix

| Obligation | Formal evidence | Executable evidence |
| --- | --- | --- |
| Every execution-identity component separates keys | Rocq `complete_key_is_injective` and five component-separation theorems | `mergeable_key_binds_complete_execution_identity`; generated component mutation property |
| The legacy key admits a real alias | Rocq `legacy_key_alias_witness` | TLA+/Apalache legacy-key expected refutation |
| Peer input cannot publish or overwrite evidence | Rocq peer-response theorems; TLA+/Apalache `LocallyDerivedEvidenceOnly` | initializer forged-response regression; running-node empty-response regression; Loom peer race |
| Distinct insertions preserve both entries | Rocq `distinct_replays_preserve_both_entries` | Loom concurrent equivocation replay |
| Opposite arrival orders converge | Rocq pointwise commutation theorem; TLA+/Apalache `OppositeArrivalOrdersConverge` | Loom two-validator opposite-order test |
| Post-state mismatch publishes no evidence | Rocq [`ReplayAdmissionPublication.v`](../../../../formal/rocq/cost_accounted_rho/theories/ReplayAdmissionPublication.v) proves `post_state_mismatch_preserves_durable_evidence_and_cache` and `changed_replay_publication_requires_post_state_equality`. TLA+, TLC, and Apalache check [`ReplayAdmissionPublication.tla`](../../../../formal/tlaplus/cost_accounted_rho/ReplayAdmissionPublication.tla), its safe configurations, and the early-publication control. | [Loom](../../../../formal/loom/cost_accounting/tests/loom_mergeable_evidence_authentication.rs) checks `only_authenticated_exact_root_replay_publishes_durable_and_cached_evidence`. The production regression [`rejected_block_final_state_does_not_publish_mergeable_evidence`](../../../../casper/tests/util/rholang/runtime_manager_test.rs) checks rollback and nonpublication. Runtime-manager properties vary equal and unequal post-state roots through the production validator. |
| Finalized retirement deletes only one complete execution key, fails closed without an advancement witness, and recognizes advancement through every DAG parent edge | Rocq exact-deletion, idempotence, distinct-entry-preservation, deletion/insertion commutation, complete-retirement-guard, every-parent-path completeness, and main-spine-incompleteness theorems; TLA+/Apalache exact deletion, preservation, idempotence, `DeletionCommutesWithDistinctReplay`, `RetirementRequiresEverySafetyGuard`, and `SecondaryParentRetirementComplete`; legacy-delete, vacuous-latest, and main-spine-only expected refutations | Runtime-manager legacy-alias deletion test, exhaustive GC eligibility tests including the empty-latest case, diamond-DAG secondary-parent regression, storage-backed idempotent collection test, and Loom retirement/replay interleaving |

The aggregate gates must compile and kernel-check the Rocq module, pass the safe
TLC and Apalache model, refute all five unsafe configurations by name, exhaust every
Loom interleaving under the configured bound, and pass the Rust example and
property tests.

See CA-P-198, TM-CA-187, UC-CA-178, REL-016, DR-50, and DR-51.
