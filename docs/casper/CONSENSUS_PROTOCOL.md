# Casper CBC Consensus Protocol

> How blocks go from proposal to finalization, and why each step exists.

## Summary

F1r3fly uses **CBC Casper** (Correct-by-Construction Casper), a proof-of-stake consensus protocol based on [Ethereum's CBC Casper research](https://github.com/ethereum/research/blob/master/papers/CasperTFG/CasperTFG.pdf).

**Properties:**
- **Multi-parent DAG** — each block can reference multiple parents (one per validator). Forks are merged, not discarded.
- **Mathematical finality** — blocks are finalized when the clique oracle proves >2/3 of stake agrees and will never disagree. Deterministic, not probabilistic.
- **Concurrent execution** — RSpace tuple space enables parallel deploy processing within blocks.

**Pipeline:**
1. Deploy arrives → pool
2. Operator, deploy, or heartbeat signal submits an explicit proposal intent
3. Snapshot: select parents via LMD GHOST fork choice, compute justifications and deploy scope
4. Create block: select deploys, execute in RSpace, compute state hash, sign
5. Self-validate, broadcast to peers
6. Receivers: replay deploys, verify state hash, check equivocations
7. Valid → insert into DAG → trigger finalization
8. Finalizer: clique oracle computes fault tolerance → if FT > threshold, block finalized

**Abstraction boundaries** — what is consensus-specific vs reusable:

| Consensus-specific | Consensus-agnostic (reusable) |
|---|---|
| Fork choice — LMD GHOST (`estimator.rs`) | DAG storage (`BlockDagKeyValueStorage`) |
| Safety oracle — Clique Oracle (`clique_oracle.rs`) | Block persistence (`KeyValueBlockStore`) |
| Finalization — FT threshold (`finalizer.rs`) | Deploy pool (`KeyValueDeployStorage`) |
| Synchrony constraint (`synchrony_constraint_checker.rs`) | Contract execution (`RhoRuntime` / `ReplayRSpace`) |
| Equivocation detection (`equivocation_detector.rs`) | P2P transport (`TransportLayer`) |
| Pre-proposal constraint checks | Engine trait (`Arc<dyn MultiParentCasper>`) |
| | Block creation/assembly, signing |
| | Validation steps 1-3, 6 (format, signature, deps, replay) |

---

## Contents

- [1. Node Startup & Genesis](#1-node-startup--genesis)
- [2. Block Proposal](#2-block-proposal)
- [3. Block Propagation](#3-block-propagation)
- [4. Block Validation](#4-block-validation)
- [5. Fork Choice (LMD GHOST)](#5-fork-choice-lmd-ghost)
- [6. State Merging (Multi-Parent)](#6-state-merging-multi-parent)
- [7. Finalization (Clique Oracle)](#7-finalization-clique-oracle)
- [8. Liveness (Heartbeat Proposer)](#8-liveness-heartbeat-proposer)
- [9. Equivocation & Slashing](#9-equivocation--slashing)
- [10. Configuration](#10-configuration)
- [11. Known Limitations](#11-known-limitations)
- [Source File Map](#source-file-map)

---

## 1. Node Startup & Genesis

### Engine State Machine

The consensus engine operates through a state machine defined in `engine.rs`:

```
GenesisCeremonyMaster ──┐
                        ├──→ Initializing ──→ Running
GenesisValidator ───────┘
```

| State | Role | Transitions to |
|-------|------|----------------|
| `GenesisCeremonyMaster` | One node coordinates genesis. Collects `required_signatures` block approvals from genesis validators. | Initializing (once approved) |
| `GenesisValidator` | Other validators. Send `UnapprovedBlock`, wait for `ApprovedBlock`. | Initializing (on receipt) |
| `Initializing` | Stores approved block, creates `MultiParentCasper` instance from storage. | Running |
| `Running` | Active consensus. Handles blocks, requests, proposals. | Terminal |

### Genesis Ceremony

Genesis creates the first block containing:
- Initial validator bonds (from `bonds.txt`)
- Initial wallet balances (from `wallets.txt`)
- Shard configuration (fault tolerance threshold, synchrony constraint threshold — **locked in forever**)
- System contracts (PoS, vault, registry)

**Why this matters**: The synchrony constraint threshold and fault tolerance threshold are on-chain parameters written into the genesis block's state. Changing them requires a new genesis (new network).

### Protocol-Version Authority

The cost-accounted D3 rejected-deploy format begins at Casper protocol version 2.
Exact per-execution state-effect provenance begins at version 3. Vault-backed
quantitative byte evidence begins at version 4, certified validator incarnation
identity at version 5, and signed finalized-floor commitments with detachable
certificate sidecars at version 6. This binary's supported running set is exactly
`{6}`. Versions 1 through 5 remain recognizable as historical encoding metadata,
but any historical approved genesis is rejected before Casper starts; an unknown
future version is rejected identically.

The genesis master writes the configured protocol version into the candidate.
Every genesis validator checks that version before signing. Approved-block
validation then checks support, and initialization adopts the approved version
into the authoritative running `CasperShardConf`. Proposers write that running
version into every block, and peer-interest filtering validates against the same
running value through `Casper::get_version`.

This single chain of authority prevents a ceremony/configuration split. In the
pre-fix failure, the ceremony hard-coded version 1 while proposal used configured
version 2, and receivers compared proposals with the version-1 approved header.
Honest protocol-2 proposals were discarded before validation. The repaired
lifecycle has no independent receiver-side version source.

Protocol 6 activates through a fresh protocol-6 genesis. There is no
block-height activation, node-local accounting switch, A/B mode, or mixed-version
running interval. The TLA+ and Rocq models are cataloged in
[`docs/formal-verification.md`](../formal-verification.md); the normative rules
are in
[`docs/casper/theory/finalized-floor/finalized-floor-specification.md`](theory/finalized-floor/finalized-floor-specification.md#52-protocol-version-lifecycle).

### Key Design Point

The engine uses `Arc<dyn MultiParentCasper>` for dynamic dispatch. The `Running` state holds a trait object, not a concrete type.

---

## 2. Block Proposal

Proposals are triggered by an operator/API request, by deploy arrival when
auto-propose is enabled, or by the
[heartbeat proposer](#8-liveness-heartbeat-proposer). The trigger carries one
of three explicit intents through the serialized proposer:

| Intent | Meaning | May authorize an empty block? |
|---|---|---|
| `Manual` | An operator or API requested an ordinary proposal | No |
| `PendingDeploy` | Locally stored user work may be ready | No |
| `FinalityRecovery(permit)` | The heartbeat selected this validator for one stalled-LFB recovery round | Only after the permit is revalidated and heartbeat capability is enabled |

This distinction is an authority boundary. A generic asynchronous flag or large
LFB lag cannot accidentally turn an ordinary request into permission to create
an empty block.

### Proposal admission and coalescing

Proposal execution is single-flight. The admission gate has three logical
states:

| State | Meaning |
|---|---|
| `Idle` | No proposal owns the executor |
| `Active` | One request is executing |
| `ActiveDirty` | One request is executing and at least one pending-deploy wakeup arrived |

When `PendingDeploy` collides with active work, the gate latches one dirty bit.
Any further pending signals coalesce into the same bit. When the active request
finishes, the proposer converts that bit into exactly one forced
`PendingDeploy` follow-up and reacquires the current Casper engine immediately
before taking its fresh snapshot. During normally available execution, this
bounds the request queue and preserves one pending wake across active
completion. If enqueue fails or no current Casper engine exists, cancellation
clears the gate and may drop that wake edge; it does not remove the stored
deploy, which a later heartbeat tick rescans after service returns. Manual and
recovery collisions are internally classified `Busy` and return an empty trigger
result without changing the active request's intent; a selected heartbeat
recovery therefore keeps its round open and retries on a later tick.

### Step 1: Acquire Snapshot

`MultiParentCasperImpl::get_snapshot()` captures the consensus state at proposal time:

1. **Get exact latest-message slots** for the complete positive-stake
   finalized-floor authority set (one slot per validator).
2. **Derive the causal-parent projection** by excluding an entry unless its
   block has certified accepted admission, its sender and monotonic bond
   generation match the floor authority, and its validator incarnation has no
   objective-equivocation evidence. This predicate is evaluated before floor
   ancestry.
3. **Derive the finality-vote projection** as the subset of causal-parent
   entries whose blocks descend from the captured finalized floor. LMD-GHOST,
   clique voting, and finality use only this narrower projection.
4. **Select declared parents** from the causal-parent projection. LMD-GHOST's
   selected vote tip is ordered first; an otherwise valid stale tip remains a
   secondary causal parent. A tip may be compacted only when another retained
   parent reaches it through all-parent DAG ancestry. Depth expiry and
   reachability compaction run before the exact parent-count check. If the
   resulting frozen frontier exceeds `max-number-of-parents`, proposal returns a
   typed deferred result without creating or signing a block; no parent is
   truncated. `number-of-active-validators + 1` is sufficient worst-case
   provisioning, not a startup admission rule. If no causal tip exists, the
   captured finalized floor is the parent.
5. **Compute LCA** (Lowest Common Ancestor) of selected parents — bounds the [merge scope](#6-state-merging-multi-parent)
6. **Build justifications**: the exact positive finalized-floor authority set,
   using each member's registered latest-message hash, including invalid latest
   evidence needed by slashing
7. **Compute deploy scope**: BFS traversal within `deploy_lifespan` window to find all deploys already included in ancestor blocks

**Why snapshot first?** The snapshot is immutable once created. This prevents race conditions — the proposal works against a consistent view of the DAG even as new blocks arrive concurrently.

An overlapping finalizer is observable for diagnostics, but it does not make a
partially mutable snapshot authoritative. The snapshot owns one immutable DAG
view, and proposal preflight derives its prospective structural floor from the
selected parents and frozen justifications. If that authority differs from the
captured LFB authority, proposal defers before replay.

### Step 2: Check Constraints

Before building a block, the proposer verifies:

| Constraint | What it checks | Why |
|------------|---------------|-----|
| Floor authority | Sender has positive stake in the current finalized-floor committee | Only floor-authorized validators can propose |
| Synchrony constraint | Other validators have produced recent blocks, weighted by the same finalized-floor committee (see [section 8](#8-liveness-heartbeat-proposer)) | Prevents isolation attacks without introducing a head-local authority split |
| Height constraint | Block height < LFB height + threshold | Prevents runaway chain growth |

### Step 3: Select Deploys

`block_creator::prepare_user_deploys()`:

1. Read unfinalized deploys from `KeyValueDeployStorage`
2. **Pull recovered deploys** from the `KeyValueRejectedDeployBuffer` —
   sigs that a prior multi-parent merge conflict-rejected. Their
   effects never landed in canonical state and they are eligible for
   re-inclusion in a fresh proposer's body.
3. **Filter**: Not future (`valid_after_block_number`), not expired by height, not expired by time
4. **Exclude**: Deploys with an active occurrence in the selected-parent
   closure. A historical occurrence on the proposer's self-chain does not count
   when its source branch is outside that closure; the deploy is rehomed onto
   the current candidate. An active self-chain occurrence is suppressed unless
   the immutable candidate context selected its exact-source recovery. This
   keeps admission and packaging on one scope while allowing validators to
   prepare independently.
5. **Sort deterministically**: `(valid_after_block_number, timestamp, signature)` — every validator selects the same deploys in the same order
6. **Cap**: `max_user_deploys_per_block`
7. **Adaptive cap**: EMA-based controller targets 1-second block creation latency. When blocks take longer, cap decreases. Small batches bypass the cap entirely. A backlog floor prevents deploy starvation.

**Why deterministic ordering?** All validators must select identical deploys for identical parent sets, or state hashes diverge and blocks get rejected.

### Step 4: Execute Deploys

For each selected deploy:
1. Execute Rholang via `RhoRuntime` (play runtime)
2. Reserve canonical produce/consume introduction bytes before mutation and
   reserve authority plus delivery/trace bytes for each locked atomic COMM
3. `create_soft_checkpoint()` between deploys (isolates effects)

Then execute system deploys:
- `SlashDeploy`: Penalize known equivocators
- `CloseBlockDeploy`: Finalize block state, update bonds

Finally: `create_checkpoint()` produces the post-state hash.

### Step 5: Assemble & Sign

- Header: version, timestamp, sender, block number (max parent + 1), sequence number
- Body: pre-state hash, post-state hash, processed deploys, rejected deploys, system deploys
- Justifications: exactly one per positive validator in `Auth(B)`, derived from
  `post_state(floor(B))`
- Bonds cache: the complete PoS bonds replayed from `B.post_state`; this records
  the transition and does not authorize `B`
- Hash the block via Blake2b256
- Sign with validator's Secp256k1 key

**Timestamp hardening**: If current time < max parent timestamp (clock skew), the timestamp is clamped to the parent's timestamp. This prevents `InvalidTimestamp` validation errors.

### Step 6: Self-Validate

The proposer validates its own block before broadcasting. If pre/post-state hashes don't match expectations, the block is rejected as `BlockException` (not panicked).

---

## 3. Block Propagation

1. Proposer broadcasts `BlockHashMessage` to all connected peers
2. Peers that don't have the block send `BlockRequest`
3. Proposer streams the full `BlockMessage` to requesting peer
4. Receiving peer enters the [validation pipeline](#4-block-validation)

The block retriever (`block_retriever.rs`) handles missing dependencies:
- Tracks pending requests per block hash
- Implements retry budgets, cooldowns, and quarantine for stuck requests
- Deduplicates requests to avoid flooding

Protocol-6 finalized-floor certificates use a distinct content-addressed sidecar
path. A block that names an unavailable certificate is stored as detached and
waits on a typed certificate dependency rather than being treated as invalid or
as a missing block. The certificate retriever bounds tracked obligations and
peer fanout, retries every eligible digest with monotonic backoff, and retains an
obligation after a transport failure or restart. A response is persisted only
when it satisfies a live request, parses with canonical bounded shape, and hashes
to the requested digest. Concurrent duplicate responses converge on the same
content-addressed record and schedule the waiting block at most once. The full
state machine and implementation mapping are in
[`finalization-certificate-retrieval.md`](theory/finalized-floor/finalization-certificate-retrieval.md).

---

## 4. Block Validation

`BlockProcessor` runs an 8-step pipeline on received blocks:

### Step 1: Interest Check
- Already in DAG or casper buffer? Skip.
- Shard ID and version match the approved block? Required.
- Block number >= approved block number? Required (no ancient blocks).

### Step 2: Format & Signature
- Verify cryptographic signature (Secp256k1, Schnorr, or FROST)
- Check field validity: hash length, timestamp within ±15s of local time, required fields present

### Step 3: Dependency Resolution
- All parent blocks must be in the DAG
- If missing: store block in **casper buffer** (max ~16K entries), request missing parents from peers
- Casper buffer tracks retry attempts per dependency and quarantines blocks after budget exhaustion
- A missing protocol-6 finalized-floor certificate is a typed sidecar dependency:
  retain the detached block, request the exact committed digest, and resume
  validation only after the sidecar passes shape and content-address validation

### Step 4: Snapshot Computation
- Recompute `CasperSnapshot` with the block's actual parents as tips
- This ensures validation uses the same state the proposer had

### Step 5: Block Summary and Floor Authority
- Structural consistency: block number progression and justification shape
- Derive `floor(B)` from immutable parents and justifications
- Require exact floor-committee justifications, a positive floor-authorized
  sender, and one canonical floor weight map

### Step 6: Checkpoint Validation (Deploy Replay)
- **Replay every deploy** via `ReplayRSpace` (replay runtime, not play runtime)
- Verify computed post-state hash matches the block's claimed hash
- This is the most expensive step — it proves the proposer executed correctly

### Step 7: Equivocation Checks
- **Simple equivocation**: Block creator's latest message should match their creator justification. If not:
  - `AdmissibleEquivocation`: Block was requested as a dependency — store as invalid but keep in DAG for tracking
  - `IgnorableEquivocation`: Block arrived unsolicited — drop entirely
- **Neglected equivocation**: Block justifies a known equivocator without slashing them — block is invalid

### Step 8: Deploy & State Validation
- Deploys are within scope, not duplicated
- State-bound funding evidence, realized compute/storage/byte costs, RevVault
  settlement, and replay witnesses agree exactly
- The serialized bond cache equals the PoS bonds recomputed from the replayed
  post-state and contains no duplicate validator entry
- Invalid block tracking applied

### On Success
- Block inserted into DAG
- Latest messages updated for block's sender
- Children index updated
- **Finalization triggered** asynchronously (single-flight guard)

---

## 5. Fork Choice (LMD GHOST)

**Latest Message Driven Greedy Heaviest Observed Subtree** — the algorithm that selects which blocks to build on.

### Algorithm (`estimator.rs`)

1. **Collect latest messages**: One block per bonded validator
2. **Filter**: Remove messages from slashed validators; ignore messages >1000 blocks old
3. **Compute LCA**: Lowest Common Ancestor of all latest messages (iterative LUCA-many algorithm)
4. **Score**: BFS from each latest message up to LCA. Each validator's stake weight flows down through the main-parent chain.
5. **Rank recursively**: Starting from LCA, greedily pick the highest-scored child. Repeat until no higher-scored descendants exist.
6. **Apply depth filter**: Main parent (rank 1) always included. Secondary parents filtered to within `max_parent_depth` of main parent.

### Why LMD GHOST?

- Selects by **weight** (stake), not longest chain — a validator with 51% stake immediately wins fork choice
- **History-independent** after the LCA — only recent messages matter
- Supports **multi-parent** selection — the ranked list becomes the parent set for the next block

---

## 6. State Merging (Multi-Parent)

When a block has multiple parents, their RSpace states must be merged before executing new deploys. This is the key difference from single-parent chains.

### Why Merge?

In a multi-parent DAG, different validators may have included different deploys in their blocks. Block B with parents P1 and P2 needs a combined state that includes effects from both P1 and P2 (minus conflicts).

### Algorithm (`dag_merger.rs` + `conflict_set_merger.rs`)

1. **Derive the finalized floor**: From the parents' inherited floors and the
   highest state-safe clique-certified frontier in the block's frozen
   justification snapshot.
2. **Identify visible blocks**: All blocks in the floor-bounded parent closure
   (exclusive of the floor, inclusive of the parents).
3. **Collect deploys**: Extract user deploys from all visible blocks
4. **Detect conflicts**: Branches conflict if they contain the **same user deploy ID** (not content — just the deploy signature)
5. **Resolve**: `ConflictSetMerger` selects the highest-value subset
   of non-conflicting deploys. Dependents of rejected deploys are
   also rejected. Rejected sigs land in the
   `KeyValueRejectedDeployBuffer` so a subsequent proposer can
   re-include them via `prepare_user_deploys` (see Block Creation
   step 3).
6. **Merge**: Replay selected deploys via RSpace merger to compute combined post-state

An exact rejected-deploy record is keyed by `(deploy signature, source block)`.
Its reason is diagnostic rather than authorization: causal descendants can
legitimately classify the same occurrence differently as their observed merge
closures grow. Equal evidence is normalized with the fixed precedence
`duplicate_occurrence > merge_conflict > collateral_chain_drop > unspecified`.
This max-like join is commutative, associative, and idempotent, so parent arrival
order cannot change the block body. A direct duplicate dominates a direct merge
conflict, and either direct cause dominates a merely collateral chain drop.

```text
canonical_reason(records):
    result := unspecified
    for each causal record in any order:
        result := max_by_protocol_precedence(result, record.reason)
    return result
```

### Determinism Constraint

The merge scope is derived from the block's parents, their cached structural
floors, and the block's frozen justifications—not from a node-local live
finalization view. Two validators compute the same floor and merge for the same
block. The finalized floor, not a locally observed LFB, bounds the scope.

### Performance Bounds

- Merge cost: O(visible_blocks^2 x deploys^2) for conflict resolution
- The finalized-floor distance is a deterministic work bound.
- **Backstop**: If the floor distance exceeds the configured cap, proposal parks
  and validation fails deterministically. The node never substitutes one parent's
  post-state or silently drops co-parent effects.

### System Deploys

System deploys (`SlashDeploy`, `CloseBlockDeploy`) are deterministic and non-conflicting. They are not subject to conflict resolution.

### Lifecycle-record and effect-record alignment

`BlockMessage.body.deploys` is a lifecycle-record sequence, not necessarily an
execution sequence. State-bound funding retains an underfunded deployment as a
terminal admission-rejected `ProcessedDeploy`, but that record never enters the
runtime and produces no mergeable-channel map. Multi-parent block indexing
therefore projects user records by `admissionStatus != Rejected` before checking
metadata cardinality, traversing adjacent state witnesses, splitting user and
system metadata, or assigning execution indices.

An ordinary `is_failed` deployment remains in the effect sequence when its
admission status is `Executed`; it entered runtime and owns a metadata position.
Only the pre-execution admission rejection is status-only. For user records
$`U`$, processed system executions $`S`$, and locally reconstructed metadata
$`M`$:

```math
|M|=|[u\in U\mid u.\operatorname{admissionStatus}\ne\text{Rejected}]|+|S|.
```

The exact cardinality still fails closed. This projection does not alter block
bytes, parent selection, clique voting, fault tolerance, or finality; it ensures
that an observable zero-effect rejection cannot make a valid parent impossible
to index for the next proposal. See DR-53 and
[`admission-effect-alignment.md`](theory/cost-accounting-impl/admission-effect-alignment.md).

---

## 7. Finalization (Clique Oracle)

Finalization determines when a block is **mathematically irreversible**. Once finalized, the block's state is committed and deploy effects are permanent.

### Trigger

Finalization runs asynchronously after each valid block is added to the DAG. A
monotonic request sequencer coalesces covered requests and launches up to the
configured number of immutable finalizer evaluations. A snapshot may overlap an
evaluation, but it remains internally immutable and its proposal must pass
structural-floor authority preflight before replay. Only the short durable
compare-and-append publication point is linearized; block admission, replay, and
independent evaluation remain concurrent.

### Algorithm (`finalizer.rs`)

1. **Freeze one view**: Snapshot latest messages once. Discovery, ranking, and
   clique evaluation use that same immutable map.
2. **Scope**: Propagate each validator identity from its frozen latest message
   through every parent edge down to the current LFB height. The descending
   `(block_number, block_hash)` worklist processes each block once and produces
   exactly the validators whose latest messages causally include that block.
   This includes secondary parents; restricting discovery to main-parent edges
   can permanently hide a state-certified candidate.
3. **Candidate filtering**: Keep only blocks with strictly more than half of the
   stake agreeing. This is a conservative upper-bound filter before expensive
   clique computation; it cannot admit or prune a true exact-threshold result at
   the boundary.
4. **Clique Oracle** for each candidate in deterministic descending
   `(block_number, block_hash)` order:
   - Build an agreement graph: edge between validators A and B if they "never eventually see disagreement" about the target block
   - Find the maximum weighted clique (largest fully-connected subgraph by stake)
   - Decide the strict threshold in exact integer arithmetic, equivalent to
     `$`\mathrm{FT}=(2q-S)/S > \theta`$`
5. **Certificate/state separation**: A successful clique decision is retained
   unchanged. The candidate may replace the LFB only when its replay state derives
   from the current LFB's state. The current LFB may be a secondary parent of a
   valid multi-parent rebase; requiring it on the candidate's main-parent spine
   would stall finality. The state-lineage predicate does not add, remove, or
   reweight a vote.
6. **LFB advancement**: Compare-and-append an immutable, hash-chained
   finalization round against the durable head. A stale concurrent result has no
   effects. The winning round is projected in revision order, then deploy,
   cosigner, runtime-cache, and `BlockFinalised` effects are applied with durable
   receipts. Restart resumes the unfinished suffix. A certified stale-state
   candidate remains a valid speculative block; a later proposal rebases on the
   certified floor and restores progress.

### Witness-equivalent predecessor certificates

Two honest nodes can certify the same finalized block and replay state from
different sufficient latest-message snapshots. Their content-addressed
certificate digests may therefore differ without any disagreement about the
state transition. A predecessor carrier is eligible by accepted causal
membership, running protocol version, exact floor hash, and exact floor
post-state—not by equality with the receiver's local witness digest.

Selection always retains the carrier block hash and the certificate digest
signed by that block as one proof pair. Substituting a digest from another
witness-equivalent carrier is invalid. A finalizer parked for a predecessor
proof wakes when an eligible carrier for that exact floor and state is admitted,
independent of its digest. These rules preserve asynchronous validator
concurrency without adding a vote, changing clique weight, or canonicalizing
node-local evidence. The complete rule and verification evidence are in
[Witness-equivalent certificate carriers](theory/finalized-floor/certificate-carrier-equivalence.md).

Dependency maintenance is not a consensus vote and does not impose a validator
ordering. Each local maintenance invocation freezes its visible ordinary-block
and certificate obligations, attempts every member, and only then returns the
first transport error. This prevents a failed request near the front of one
node's local iteration order from suppressing unrelated proof retrieval while
leaving replay, validation, and voting parallel across validators.

### "Never Eventually See Disagreement"

Two validators A and B agree on block T if:
- A's latest message is in T's main-parent chain
- B's latest message is in T's main-parent chain
- Walking B's self-justification chain from B's latest back to A's view of B reveals no messages that disagree with T

This is a **permanent** agreement — once two validators are in a clique for block T, no future messages can break it.

### Scheduling, not consensus budgets

The finalizer has no candidate-count cap, elapsed-time budget, per-candidate
timeout, or runner cancellation deadline. Such limits made equal frozen DAG views
produce host-speed-dependent candidate coverage. It yields cooperatively at a
configured interval to avoid monopolizing the Tokio executor, but the yield changes
latency only: it never truncates or restarts the frozen scan.

### Why message ancestry is not state ancestry

A multi-parent block may have the current LFB in its main-parent ancestry while
its post-state was replayed from an older floor to resolve a conflicting parent.
The block therefore descends from the LFB as a message without deriving from its
state. Promoting that block would omit an already committed effect even though its
clique certificate is sound. The node records an explicit state base for this
reason: a covering parent is the base only when it preserves the floor; otherwise
the floor is the base. LFB admission follows the reflexive, transitive closure of
those base edges.

The converse distinction is equally important. Parent selection may promote a
deploy-carrying branch to main parent while retaining the current LFB as a
secondary parent. The resulting replay state can derive from the LFB even though
the candidate's main-parent spine does not. LFB admission therefore checks state
ancestry directly and does not impose a second main-spine requirement. Candidate
discovery likewise follows every parent edge; this only enumerates possible
targets. Each enumerated target must still pass its own exact causal clique,
independent exact state-preserving clique, and current-LFB state-preservation
checks over the same frozen context.

Vote eligibility and causal-parent eligibility are therefore deliberately
different types. A block that is accepted, correctly attributed, current-
generation, and non-equivocating remains a causal merge input even if it does
not descend from the new floor. It cannot vote for that floor. A rejected,
wrong-generation, unregistered, sender-mismatched, or equivocating block is in
neither projection. Classifying floor ancestry before those intrinsic checks
would let a multiply-invalid stale block masquerade as an admissible parent.

The complete normative rules and their TLA+/Apalache, Rocq, and Rust evidence are
in the [finalized-floor specification](theory/finalized-floor/finalized-floor-specification.md)
and [verification dossier](theory/finalized-floor/finalized-floor-verification.md).
The publication transaction, recovery cursors, effect receipts, and concurrency
boundary are specified in
[Atomic finalization and crash recovery](theory/finalized-floor/finalization-atomicity-and-recovery.md).

### Fault Tolerance Values

| FT Value | Meaning | Finalized at FTT=0.0? | FTT=0.33? |
|----------|---------|----------------------|-----------|
| 1.0 | All stake agrees | Yes | Yes |
| 0.67 | 5/6 of stake | Yes | Yes |
| 0.33 | 2/3 of stake | Yes | No (strict >) |
| 0.0 | Exactly 50% | No | No |
| -1.0 | No majority | No | No |

### FT Caching

The FT value computed by the clique oracle at finalization time is a mathematical proof of irreversibility — it certifies the fraction of total stake that would need to be Byzantine to revert the block. This proof is permanent: once the clique is established, no future honest message can break it.

**Why caching is necessary:** The clique oracle's `normalized_fault_tolerance` function uses `latest_message_hash` to determine which validators agree on a block. This is a live DAG query — different nodes have different DAG states (due to propagation delays), so the same finalized block returns different FT values on different nodes. In a multi-parent DAG, the instability is worse because merge blocks can shift which branch is "main parent," causing validators to lose agreement through non-main parent paths.

**Implementation:**
1. `Finalizer::run` returns `(BlockHash, f32)` — the LFB hash and its computed FT
2. Each committed finalization round stores the FT in
   `BlockMetadata.fault_tolerance_value` for:
   - The **directly finalized block** — receives its own computed FT
   - **Indirectly finalized ancestors** — receive the descendant's FT as a conservative lower bound (CBC Casper guarantees ancestor FT >= descendant FT)
3. Ordered ledger projection monotonically raises the cached FT of all finalized
   metadata after materializing the round manifest. This covers previously
   finalized branches in the multi-parent DAG that are not newly present in the
   manifest.
4. `block_api.rs` returns the cached FT for finalized blocks, bypassing the clique oracle
5. Non-finalized blocks continue using the live oracle

**FT convergence:** Cached FT is monotonically non-decreasing. It only increases — never decreases. As later finalization rounds compute higher FT (more validators agree), the propagation pass updates all finalized blocks. With all validators active, FT converges toward 1.0 across all nodes.

**Data flow:**
```
Finalizer → compute FT via clique oracle
         → if FT > threshold:
              atomically append (round, durable head)
              project round manifest in revision order
              monotonically raise finalized metadata FT
              apply receipted idempotent effects
         
Block API → is_finalized?
              yes → return BlockMetadata.fault_tolerance_value
              no  → compute via clique oracle (live DAG)
```

**Code locations:**
- `casper/src/rust/finality/finalizer.rs` — FT computed and returned alongside LFB hash
- `block-storage/src/rust/finality/finalization_ledger.rs` — immutable rounds,
  compare-and-append head, receipts, recovery cursors, and compaction
- `block-storage/src/rust/dag/block_dag_key_value_storage.rs` — ordered round
  projection stores FT and updates finalized metadata
- `block-storage/src/rust/dag/block_metadata_store.rs` — `record_finalized(directly, indirectly, ft_value)` persists FT, `update_ft_if_higher` for propagation
- `casper/src/rust/api/block_api.rs` — `get_block_info_with_dag` reads cached FT for finalized blocks

---

## 8. Liveness (Heartbeat Proposer)

The heartbeat proposer (`node/src/rust/instances/heartbeat_proposer.rs`) ensures the chain makes progress even without user deploys.

### Trigger Logic

The heartbeat runs a loop that races between:
- **Timer**: `check_interval` (default: 5s)
- **Signal**: Deploy received (wakes immediately)

### Decision Tree

On each heartbeat tick:

1. **Pending deploys**: Is snapshot-admissible local work due under the lag,
   grace, cooldown, and backstop bounds? → Submit `PendingDeploy`
2. **Stale LFB recovery**: Has the exact LFB hash remained unchanged through
   the next local recovery round, and is this validator its leader? → Submit
   `FinalityRecovery(permit)`
3. **Backpressure**: Is the proposer active, the self-propose cooldown open, or
   empty-frontier pressure at its exact cap? → Coalesce pending work or defer
   recovery as appropriate

Peer-block arrival and frontier movement do not form another branch of this
decision tree. A peer block becomes causal/state evidence only after validation;
observing it cannot authorize a local support proposal.

### Recovery permits and leader rotation

This section specifies finality-recovery heartbeat proposals. It does not
authorize a rejected deploy retry. A rejected source carrier gives retry
custody only to that carrier's sender. Distinct carrier owners can retry
independent work concurrently. Ordinary inclusion and heartbeat permits keep
their deterministic leader rotation.

The heartbeat derives one canonical authority committee from the captured LFB's
post-state using the same `floor_committee` function used by proposal and receive
authority. It filters the LFB-state PoS bonds to active validators, then orders
and deduplicates them. Non-finalized parent divergence therefore cannot select
multiple recovery leaders for the same LFB and round. For LFB height $`h`$,
validator-local recovery round $`r`$, and committee $`C`$, the sole selected
leader is:

```math
leader(C,h,r)=C[(h+r)\bmod |C|].
```

The request carries a permit containing the observed LFB hash, that LFB's
metadata height, and the heartbeat task's local round. Waiting in the proposer
queue does not freeze consensus. The proposer therefore takes a fresh snapshot
immediately before execution and checks that the permit hash is still the LFB,
that the current metadata for that hash has the captured height, and that the
captured round selects the caller over the fresh LFB-derived committee. The round is only an
input to deterministic leader selection: there is no node-global or
proposer-owned current round to compare. A non-finalized head-height increase by
itself cannot stale the permit because the head height is not the LFB height.

If the LFB changed, its floor committee cannot be reconstructed, or the permit is malformed, the request is
deferred without creating a block. The owning heartbeat task awaits this result,
so it cannot advance its local round while that request is outstanding. A
selected leader records the round complete only after proposal reports `Started`
or `Success`; busy, empty, deferred, and failed outcomes leave it available for
retry on a later tick. A nonleader records its local round as skipped, allowing
later rounds to rotate past an offline leader.

The serialized `block.body.state.bonds` field has a different role: it is the
PoS bond cache replayed from that block's post-state. Receive validation compares
it with replay, and only an accepted block may use it to register a newly bonded
validator's latest-message slot. It does not authorize its own block. Proposal
and receive validation instead derive `Auth(B)` from `post_state(floor(B))`,
require the justification validators to equal `Auth(B)` exactly, require the
sender to have positive floor stake, and use the same floor weights for
synchrony. An accepted bond transition becomes authoritative only after a later
floor promotion includes it.

### Pending work composes with recovery

A recovery request does not mean “make an empty block.” The ordinary block
creator first deterministically selects every deploy admissible in the fresh
snapshot. If work is ready, the recovery block carries that work. Empty-block
authority matters only when the admissible selection is empty.

This requires distinguishing two states that are easy to conflate:

- A **stored pending deploy** is merely retained in local deploy storage.
- An **admissible pending deploy** also passes ordinary snapshot-relative
  validity, terminal/in-scope exclusion, duplicate, and capacity rules.

A future, expired, terminal, already-in-scope, or exhausted occurrence may
remain stored while being inadmissible. Such an envelope cannot mask an
otherwise authorized empty recovery, and recovery cannot include it merely to
make the block non-empty.

### Why Heartbeat Matters

Without heartbeat, a shard with no user deploys may lack the ordinary blocks
needed to advance finalization. Permit-bound recovery supplies those blocks at a
bounded cadence so validators can carry new justification evidence. Leader
selection, however, is not a finality vote: every recovery block still follows
ordinary self-validation, peer replay, clique certification, state-preserving
certification, and LFB-admissibility checks.

### Cost-accounting boundary

Proposal intent changes scheduling authority only. Deploy selection, the
authenticated pre-state, static/dependent cost certification, RSpace execution,
RevVault settlement, replay, and validation are identical whether a deploy was
selected by a `PendingDeploy` request or composed into a
`FinalityRecovery(permit)` request. Coalescing stores no deploy contents, supply,
reservation, or settlement evidence; its forced follow-up rescans current
storage against a fresh snapshot. Consequently, the liveness repair cannot
double-charge, rescue an underfunded occurrence with a later top-up, or change
the deterministic state transition. See
[End-to-end cost authority and native RevVault settlement](theory/cost-accounting-impl/end-to-end-authority-settlement.md#proposal-scheduling-and-settlement-independence).

### Synchrony Recovery

When the synchrony constraint blocks proposals:
1. Detect stall: no progress for the configured stall window
2. Allow bypass after cooldown
3. Limited bypass budget before requiring another stall window
4. Alternative: finalized-baseline mode uses LFB height instead of tip height

---

## 9. Equivocation & Slashing

### Equivocation Types

| Type | What happened | Detection | Action |
|------|--------------|-----------|--------|
| Simple | Validator created two blocks at same sequence number | Creator justification != latest message | Block rejected |
| Admissible | Equivocating block needed as dependency by another block | Same as simple, but block is in dependency chain | Stored as invalid in DAG for tracking |
| Ignorable | Equivocating block arrived unsolicited | Same as simple, not needed as dependency | Dropped entirely |
| Neglected | Validator had evidence of equivocation but didn't slash | Justifications reference known equivocator | Block rejected, validator penalized |

### Slashing Flow

1. Equivocation detected during block validation
2. `EquivocationRecord` created and stored persistently
3. Honest validators emit a `SlashDeploy` from durable objective sibling pairs or authorized unary invalid-block evidence (`prepare_slashing_deploys` in `block_creator.rs`); only positively bonded validators in the canonical merged pre-state can be slashed
4. `SlashDeploy` executes PoS contract to remove equivocator from bonds
5. Equivocator loses its stake; the system contract remains idempotent as defense in depth, while proposer and receiver authorization reject a new slash when the target's canonical merged-pre-state bond is non-positive

### Multi-Parent Merge and Canonical Slash Reconstruction

A `SlashDeploy` issued in one parent can be rejected by cost-optimal merge resolution. The slash effect must remain live without treating a rejected deploy as new authority. The proposer therefore reconstructs authorized slash work from canonical state:

1. Before block assembly, the proposer runs `compute_parents_post_state` on its selected parents. The merge engine returns the canonical `pre_state` and rejected user-deploy occurrences.
2. `prepare_slashing_deploys` scans durable objective sibling pairs first and then the complete unary invalid-block evidence index. It authorizes candidates against the bond map computed from that exact `pre_state`.
3. `authorized_slash_candidates` selects at most one deterministic evidence item for each `(offender, activation epoch)` target. Hashes are grouped by epoch before the canonical pair is selected. A pair is the ordered hashes of distinct blocks with one sender and sequence number in one validator lifetime; it is independent of local invalid flags and carries both hashes as dependencies.
4. Once a structural sibling group exists, unary fallback from that `(offender, sequence)` is suppressed before epoch and bond filtering. This prevents opposite arrival orders from choosing opposite unary evidence when a cross-epoch collision is not slashable without hiding independent unary faults at other sequences. Only the affected epoch-scoped lifetime is excluded from voting; old evidence does not permanently retire a later same-key lifetime.
5. If a parent slash lost the merge and the offender remains positively bonded, persisted evidence causes the canonical scan to reconstruct one authorized candidate. If the slash effect survived and the offender's bond is zero, the scan emits none.
6. Receive-side validation uses the same parent-derived state and rejects missing evidence, malformed pairs, stale epochs, non-positive target bonds, issuer mismatch, and duplicate targets before replay.

Merge-rejection records are diagnostic evidence about conflict resolution, not an authorization source. This distinction prevents two rejected hashes for the same offender from producing two slash deploys in one block.

### Multi-Slash Blocks

A single block can carry more than one `SlashDeploy`, but only for distinct authorized `(offender, activation epoch)` targets. Multiple unary invalid hashes collapse to one canonical candidate; an objective sibling pair takes precedence for its offender.

Each `SlashDeploy`'s RNG is keyed on the proposer, sequence number, first evidence hash, and optional second evidence hash (`util::rholang::system_deploy_util::generate_slash_evidence_random_seed`). The unary path is byte-identical to its historical encoding. The pair path sorts both hashes first, so arrival order cannot change the seed. Without evidence hashes in the seed, two slashes in the same block from the same proposer would alias the unforgeable channel names allocated by the slash contract, corrupting tuplespace state and per-slash return-channel routing.

### Empty-Block Skip

Heartbeat-disabled proposers (`allow_empty_blocks = false`, the production default) skip block creation when there are no user deploys and no canonically authorized slash candidates. A merge-rejected slash causes work only indirectly: persisted evidence plus a positive target bond in the merged pre-state reconstructs an authorized candidate. A rejected hint by itself cannot force an empty proposal or reauthorize a zero-bond target.

### Two-Level Slashing

- **Level 1**: Direct equivocator — loses entire stake
- **Level 2**: Validator that neglected to report equivocation — also loses stake

This makes collusion economically irrational: both parties get slashed.

---

## 10. Configuration

All consensus parameters are defined in HOCON configuration files:

- **Built-in defaults**: [`node/src/main/resources/defaults.conf`](../../node/src/main/resources/defaults.conf) — every available option and its default value
- **Shard override**: [`docker/conf/default.conf`](../../docker/conf/default.conf) — minimal overrides for multi-validator shard
- **Standalone override**: [`docker/conf/standalone-dev.conf`](../../docker/conf/standalone-dev.conf) — standalone mode with instant finalization

Operator config files are minimal overrides — HOCON's fallback semantics merge them on top of the built-in defaults automatically.

**Genesis-locked parameters** cannot change after network creation:

- `fault-tolerance-threshold` and `synchrony-constraint-threshold` define the on-chain consensus limits.
- `max-cosigners-per-deploy` defines the signer-count limit for deploy admission.
- `initial-phlogiston` and `epoch-phlogiston` define validator fuel credits.
- `client-fuel-allocations` defines additional client SystemVault balances at genesis.
- `native-token-name`, `native-token-symbol`, and `native-token-decimals` define immutable token metadata.

Change these parameters only through a new genesis.

**Native token metadata** is exposed via `/api/status` (`nativeTokenName`, `nativeTokenSymbol`, `nativeTokenDecimals`) and queryable on-chain by any Rholang contract. Joiners verify their config matches the on-chain values at startup; a mismatch causes the node to exit with a structured error event (`native_token_metadata_mismatch`).

See also: [Consensus Configuration Guide](https://github.com/F1R3FLY-io/system-integration/blob/main/docs/consensus-configuration.md) — FTT and synchrony threshold semantics, finalization formula, recommended values per validator set size.

---

## 11. Known Limitations

See [F1R3FLY-io/f1r3node issues](https://github.com/F1R3FLY-io/f1r3node/issues) for current open issues related to consensus.

---

## Source File Map

### Core Consensus
| File | Role |
|------|------|
| `casper/src/rust/casper.rs` | `Casper` and `MultiParentCasper` trait definitions |
| `casper/src/rust/engine/multi_parent_casper/mod.rs` | Main implementation: snapshot, propose, validate, finalize |
| `casper/src/rust/casper_conf.rs` | `CasperConf`, `HeartbeatConf` configuration structs |

### Engine
| File | Role |
|------|------|
| `casper/src/rust/engine/engine.rs` | State machine: GenesisValidator → Initializing → Running |
| `casper/src/rust/engine/running.rs` | Message handling in Running state |
| `casper/src/rust/engine/genesis_ceremony_master.rs` | Genesis ceremony coordination |
| `casper/src/rust/engine/genesis_validator.rs` | Genesis validator participation |
| `casper/src/rust/engine/approve_block_protocol.rs` | Genesis approval collection |

### Block Lifecycle
| File | Role |
|------|------|
| `casper/src/rust/blocks/proposer/proposer.rs` | Proposal orchestration, constraint checks |
| `casper/src/rust/blocks/proposer/block_creator.rs` | Deploy selection, block assembly |
| `casper/src/rust/blocks/block_processor.rs` | 8-step validation pipeline, casper buffer |
| `casper/src/rust/validate.rs` | Individual validation rules |

### Fork Choice & Safety
| File | Role |
|------|------|
| `casper/src/rust/estimator.rs` | LMD GHOST fork choice |
| `casper/src/rust/safety/clique_oracle.rs` | Clique oracle, fault tolerance computation |
| `casper/src/rust/finality/finalizer.rs` | Finalization search with work budgets |
| `casper/src/rust/synchrony_constraint_checker.rs` | Synchrony constraint + recovery bypass |

### Merging
| File | Role |
|------|------|
| `casper/src/rust/merging/dag_merger.rs` | Multi-parent state merge |
| `casper/src/rust/merging/conflict_set_merger.rs` | Deploy conflict resolution |

### Slashing
| File | Role |
|------|------|
| `casper/src/rust/equivocation_detector.rs` | Equivocation types and detection |
| `casper/src/rust/util/rholang/system_deploy_util.rs` | System-deploy RNG seeds (slash seed keyed on `invalid_block_hash`) |
| `casper/src/rust/util/rholang/costacc/slash_deploy.rs` | `SlashDeploy` system-deploy definition |
| `casper/src/rust/slashing_authorization.rs` | Canonical evidence scan, epoch and bond authorization, one-candidate-per-target selection |
| `casper/src/rust/blocks/proposer/block_creator.rs` | Canonical merged-pre-state computation and `prepare_slashing_deploys` |

### Liveness
| File | Role |
|------|------|
| `node/src/rust/instances/heartbeat_proposer.rs` | Heartbeat-driven proposals, stale recovery |
| `node/src/rust/instances/proposer_coalescer.rs` | Single-flight admission and one-bit pending wakeup |
| `node/src/rust/instances/proposer_instance.rs` | Serialized execution against the current Casper engine |

### Storage (consensus-agnostic)
| File | Role |
|------|------|
| `block-storage/src/rust/dag/block_dag_key_value_storage.rs` | DAG structure, latest messages, metadata |
| `block-storage/src/rust/key_value_block_store.rs` | Block persistence |
| `block-storage/src/rust/deploy/key_value_deploy_storage.rs` | Deploy pool |

### Configuration
| File | Role |
|------|------|
| `node/src/main/resources/defaults.conf` | HOCON defaults for all parameters |

---

**See also:** [Casper Module Overview](./README.md) | [Byzantine Fault Tolerance](./BYZANTINE_FAULT_TOLERANCE.md) | [Synchrony Constraint](./SYNC_CONSTRAINT.md) | [Data Flows](../data-flows/)

[← Back to docs index](../README.md)
