# Parallel Runtime, Validator, and Shard Isolation

## Scope

This document specifies the concurrency boundary connecting cost-accounted
Rholang execution, RSpace history, Casper validation, finalization, and
multi-shard resource scheduling. It is the production refinement of the local
cost semantics in [*Cost-Accounted Rho Calculus*](https://github.com/F1R3FLY-io/publications/blob/main/cost-accounting/cost-accounted-rho.tex)
and the compositional accounting construction in
[*Continued Interactive GSLTs and the Cost Endofunctor*](https://github.com/F1R3FLY-io/publications/blob/main/cost-accounting-as-monad/continued-gslt-cost-v2.tex).
Those papers define local resource sufficiency, composition, and conservation.
The node must additionally ensure that concurrent evaluators and validators do
not exchange authority through mutable implementation state.

The contract applies simultaneously to three kinds of concurrency:

- evaluations executing against different authenticated pre-state roots;
- validators receiving, replaying, and certifying blocks in different orders;
- shards sharing process, allocator, or storage infrastructure while retaining
  independent ledgers and semantic state.

Concurrency is not reduced to a serial global phase. Every participant owns a
local phase and may interleave each transition with every other participant.

## Terms

An **evaluation transaction** begins when the node captures the authenticated
pre-state and payer authority for one candidate deployment. It ends when the
node atomically accepts its resulting state and evidence or rejects it and
restores the appropriate base state.

A **recorded root** is an immutable RSpace history root present in the shared
history repository. A spawned runtime carries a local history object whose root
selects its semantic state.

The **current-root pointer** is the repository's mutable convenience pointer.
It supports opening the repository's most recently selected root. It is not an
authority source for an evaluation already in progress, a replay result, a
certificate, or a finalized-floor publication.

A **captured floor tuple** is the block, RSpace root, and committed effect set
read by one validator before replay. These components describe one state and
must remain paired throughout validation.

A **state certificate** records sufficient validator support for one exact
replayed state. It complements the causal certificate; it does not replace or
redefine Casper's weighted finalization vote.

A **promotion** atomically replaces one validator's local finalized block,
published root, and published committed effects after both certification and
local replay succeed.

A **shard frame** is the portion of the global implementation state belonging
to one shard: balances, charges, semantic commits, and admitted tasks. Actions
for another shard must leave that frame unchanged.

A **shard runtime boundary** is the production ownership boundary formed by one
Casper instance, its `RuntimeManager`, its ordinary and replay RSpace stores,
its mergeable store, and its runtime-local caches. A shard identifier on a
block identifies the consensus domain; it is not a substitute for isolating
the stateful services that execute that domain.

A **reduction frontier** is the complete set of next RSpace intents from every
live causal branch of one Rholang evaluation. It is distinct from a Casper DAG
frontier and from a finalized-floor frontier.

## Intra-deploy deterministic parallelism

Parallel Rholang branches retain independent Tokio tasks. Before any competing
RSpace intent commits, every live branch must either submit its next intent or
complete. The frozen frontier is partitioned by transitive overlap of direct
channels, pre-state join channels, and individual linear cost-authority
regions. Operations within one component commit in canonical causal order;
components with disjoint semantic and economic footprints execute
concurrently.

This prevents validator state from depending on task wake order without
turning the reducer into one global critical section. In particular, different
channels are not sufficient evidence of independence when two operations can
draw from the same RevVault purse. Compound authorities are indexed by each
region, so `{A, B}` conflicts with `{B, C}` on `B`.

RSpace logs scheduled I/O and COMM events by causal operation order and drains
them in that order at checkpoint. An evaluation owns a shared epoch permit
until every participant and frontier driver is quiescent. Checkpoint, reset,
rollback, and replay setup require the exclusive permit. Cancelling a root
future therefore cannot expose a partial state while detached children still
run. The complete algorithm, proof boundary, and regression matrix are in
[Deterministic Parallel Reduction and Checkpoint Ownership](deterministic-parallel-reduction.md).

## Required execution transaction

For transaction $`t`$, the node captures the tuple

```math
C_t = (F_t, R_t, E_t, A_t),
```

where $`F_t`$ is the block-structural floor, $`R_t`$ is its exact RSpace root,
$`E_t`$ is its committed effect set, and $`A_t`$ is the authenticated authority
and purse snapshot. No later write to a shared current-root pointer may alter
$`C_t`$.

Parser, reducer, play, and replay failures have distinct evidence projections
but one state-atomic boundary:

1. A parser failure occurs before execution and therefore publishes no witness.
2. A reducer failure retains attempted compute and byte cost but rolls back
   candidate-created linear custody.
3. A play failure restores the play transaction's captured pre-state.
4. A replay failure restores the authenticated block pre-state, including when
   replay had already created a candidate checkpoint.
5. Only accepted replay may publish mergeable evidence or validator support.

The accepted result binds its canonical payload, captured pre-state, resulting
post-state, complete authority witness, compute trace, byte trace, physical
stack operations, fee allocation, and settlement. An evaluator must not reuse
a parser witness, erase reducer work, retain a rejected candidate root, or
publish evidence before final-state validation.

## RSpace root authority

RSpace history records are append-only for this contract. Concurrent branches
may checkpoint different roots into the same repository. Each spawned ordinary
or replay runtime then resets through its own history object and materializes a
hot store from the explicitly requested root.

The following rules are normative:

1. A runtime reads semantic state from its local history root.
2. `reset(root)` must fail if `root` is not recorded.
3. A successful checkpoint records its root before the runtime publishes it.
4. Resetting or checkpointing one runtime may change the shared convenience
   pointer but cannot change another runtime's local root or hot store.
5. Validation captures the finalized root explicitly; it never consults the
   shared pointer after capture.
6. Promotion publishes the candidate's verified root, not the repository's
   current pointer.
7. Garbage collection cannot delete a root reachable from an active runtime,
   replay transaction, block, finalized floor, or retained authenticated
   evidence.

The shared pointer is safe only because it is excluded from semantic authority.
Turning it into an authority source would make validator results depend on which
unrelated runtime reset most recently.

## Validator pipeline

Each validator/candidate pair advances independently through

```text
receive -> capture floor -> replay -> validate -> emit support
```

Support messages are delivered independently to every observer. An observer may
promote only after all of the following hold:

- the candidate has the unchanged exact weighted Casper certificate;
- the candidate has a state certificate for the exact replayed state;
- the observer locally replayed and accepted that candidate;
- the replayed state contains the observer's currently committed floor effects;
- the candidate root remains recorded locally;
- the block, root, and effect set are published in one promotion transition.

The causal and state conditions serve different purposes. The causal
certificate establishes the existing protocol's vote. The state certificate
prevents a causally certified but effect-dropping state from becoming a state
floor. Local replay ensures that received support cannot substitute for the
observer's own deterministic validation.

For distinct validators $`i`$ and $`j`$, promotion updates commute pointwise:

```math
promote_j(promote_i(W, B_i), B_j)
=
promote_i(promote_j(W, B_j), B_i).
```

This theorem is a frame property, not an ordering preference. Validators may
promote at different wall-clock times. If they promote the same candidate, they
publish the same root and committed effect set because both are functions of the
candidate, not of a shared mutable pointer.

## Crash and restart

A crash may interrupt a validator after floor capture or replay. Restart clears
the incomplete local phase and re-enters at candidate receipt. It does not
publish partial support, change the finalized floor, or discard the recorded
root. The new attempt captures the then-current local floor as one tuple.

The deductive refinement makes the durability premise explicit. Replay first
adds the candidate root to the validator-local recorded-root set. Promotion
requires that exact root already to be present and preserves the complete set;
it cannot manufacture storage authority as a side effect of voting. Restart is
identity on the durable finalized tuple and recorded-root set while discarding
only transient task state.

Recovery therefore retries work; it does not continue a transaction whose
captured block, root, and state may no longer correspond. This separates local
storage or replay faults from objective block invalidity and prevents a local
unknown-root event from creating slash evidence against a remote validator.

## Multi-shard resource isolation

Let $`L_s`$ be total authenticated deposits for shard $`s`$, $`B_s`$ its
remaining balance, and $`C_s`$ its committed charges. Every admitted shard
trace must preserve

```math
L_s = B_s + C_s.
```

For any action belonging to shard $`r`$ with $`r \ne s`$, the complete frame of
$`s`$ is unchanged. A funded charge on $`s`$ atomically changes

```math
(B_s, C_s) \mapsto (B_s - q, C_s + q)
```

for $`q \le B_s`$. An underfunded attempt changes neither component. Concurrent
top-ups and charges use an atomic compare-and-swap or equivalent serialized
commit against the shard-local ledger version; a stale attempt retries from the
new committed version rather than overwriting it.

Shared worker capacity is global operational state, but worker ownership is
unique. Acquiring a worker creates one owner entry only when capacity is
available. Releasing it removes exactly that owner. Worker contention may delay
a shard but cannot transfer funds, cost evidence, roots, or semantic commits
between shards.

The current production topology gives each node process one shard runtime
boundary. Deploying several shards therefore creates distinct Casper and
`RuntimeManager` instances even when their genesis state has the same
content-addressed root. If a future host places several shard instances in one
process, it must retain the same boundary: each shard receives a distinct
ordinary RSpace store, replay store, mergeable store, cache set, and SystemVault
state. Merely adding a shard identifier to keys in one mutable runtime would not
satisfy the frame theorem.

The production transition corresponding to the model is:

```text
ApplyShardExecution(shard, block, authenticated_payers):
    runtime := runtime_boundary(shard)
    captured := capture(block.pre_state, authenticated_payers)
    candidate := runtime.execute(captured)
    if candidate is rejected:
        restore runtime to captured.pre_state
        publish no candidate root or settlement
    else:
        atomically publish candidate.root and candidate.settlement
    release every worker and runtime owned by this execution
```

The selected runtime remains fixed for the entire transition. Neither a worker
retry nor an underfunded purse lookup may select another shard's runtime or
SystemVault.

The arbitrary-shard theorem is obtained by frame preservation over traces, not
by enumerating hundreds of literal shards. Finite-state model checking then
exhausts representative competing schedules, while the parameterized Rocq proof
quantifies over arbitrary shard and task types.

Actions on distinct shards commute pointwise. For shard actions $`a`$ and
$`b`$ with different owners and any observed shard $`s`$,

```math
apply_b(apply_a(W))(s) = apply_a(apply_b(W))(s).
```

This is stronger than checking two fixed execution orders: the proof quantifies
over arbitrary shard and action types, while TLC and Loom enumerate the
intermediate capture, worker, retry, crash, top-up, commit, and release races.

## Formal correspondence

| Obligation | TLA+ | Rocq | Production evidence |
|---|---|---|---|
| Split evaluation phases and rollback | `ConcurrentEvaluationTransactionIsolation.tla` | `EvaluationTransactionIsolation.v` | parser/reducer/play/replay failure regressions |
| Explicit root authority under concurrent resets | `ConcurrentEvaluationTransactionIsolation.tla`, `ParallelValidatorConsensus.tla` | captured-root clauses in `ParallelValidatorConsensus.v` | ordinary and replay RSpace branch-isolation tests |
| Support only after exact local replay | `ParallelValidatorConsensus.tla` | `local_support_requires_accepted_replay` | validation-dispatch and replay tests |
| State-preserving promotion | `StateLineageFinality.tla`, `ParallelValidatorConsensus.tla` | `StateLineageFinality.v`, `ParallelValidatorConsensus.v` | floor-rebase and certified-promotion regressions |
| Independent validator update commutativity | `ParallelValidatorConsensus.tla` | `distinct_validator_promotions_commute_pointwise` | generated validator-order tests |
| Atomic block/root/effect publication | `ParallelValidatorConsensus.tla` | `eligible_validator_promotion_is_atomic_and_lineage_preserving` | serialized block and deploy lookup agreement tests |
| Replay-root durability across promotion and restart | `ParallelValidatorConsensus.tla` | `replay_root_recording_preserves_consistency`, `promotion_retains_every_recorded_root`, `restart_preserves_consistency_and_recorded_roots` | storage-backed runtime reopen and Loom restart tests |
| Accountable certificate and promotion composition | `AccountableFinality.tla`, `ParallelValidatorConsensus.tla` | `finalized_floor_parallel_accountable_promotion_correct` | exact-threshold and validator-order properties |
| Per-shard conservation, root ownership, foreign-action framing, and distinct-shard commutation | `MultiShardResourceIsolation.tla` | `MultiShardConcurrency.v`, including `distinct_shard_actions_commute_pointwise` | located-settlement properties and concurrent independently stored `RuntimeManager`/SystemVault integration |
| Shared worker uniqueness and capacity | `MultiShardResourceIsolation.tla` | `shared_worker_capstone` | bounded admission/backpressure concurrency tests |
| Concrete interleaving refinement | all three concurrency models | corresponding capstones | Loom validator-publication and multi-shard ledger/worker models |

The Rocq capstones are axiom-free. `ParallelValidatorConsensus.v` is
parameterized by arbitrary node, block, root, and effect types.
`MultiShardConcurrency.v` is parameterized by arbitrary shard and task types.
These proofs establish the algebraic and frame arguments beyond the finite
instances explored by TLC and Apalache.

## Finite-state search coverage

`ParallelValidatorConsensus.tla` uses three validators with stakes
$`40/35/25`$, two sequential effect-bearing candidates, independently delivered
support, node-local floors and roots, and split replay phases. No validator has
enough weight to certify a candidate alone. TLC exhausts the baseline schedule
graph through eight non-stuttering actions: 12,877 generated states, 3,411
distinct states, and search depth 9. A second safe configuration enables crashes
after capture or replay and restart from a fresh capture. A third starts from a
history-consistent cut in which a candidate was accepted before a newer floor was
published; its 150 generated / 58 distinct states prove the old candidate cannot
erase the new floor's effect. Eight
single-defect models must violate their named invariant:

- causal-only acceptance;
- support before local validation;
- promotion without local replay;
- shared-current-root capture authority;
- shared-current-root publication;
- non-atomic floor publication;
- stale effect-dropping floor promotion;
- deletion of a locally replayed root during task failure.

Apalache independently checks every baseline and crash-enabled safe invariant
through bound 6 in the routine gate, the baseline through bound 8 in the deep
gate, and the stale-window pre-state through bound 2. Removing only the
current-floor preservation guard loses a committed effect in one step. Its
crash-root-deletion negative control must independently violate
local root retention through bound 5. The depth-8 symbolic run is intentionally
a deep gate because it explores the same split transition system with SMT
rather than TLC's explicit finite-state representation.

`ConcurrentEvaluationTransactionIsolation.tla` separates parser, capture,
reset, execution, checkpoint, validation, rejection, acceptance, crash, and
restart for two transactions. Its safe TLC instance reaches 511 distinct states
at depth 21. Eight negative controls cover witness reuse, reducer erasure, play
and replay rollback omission, early evidence, shared root authority, shared root
publication, and foreign-root deletion.

`MultiShardResourceIsolation.tla` schedules four tasks across two shards with a
shared two-worker pool, concurrent top-ups, stale compare-and-swap retries, and
crashes. The non-crash safe TLC instance reaches 4,048 distinct states at depth
19; `MultiShardResourceIsolationCrash.cfg` separately checks every safety
invariant with crash/restart enabled rather than assuming successful task
completion. Five negative controls cover blind commit, foreign state write,
cross-shard root publication, cross-shard debit, and worker/resource leakage.

`multi_shard_runtime_isolation_test.rs` binds these abstractions to the node's
production storage and vault path. Two independently scoped `RuntimeManager`
instances replay the same genesis, concurrently mint and burn different values
at the same canonical public vault address, and then verify their distinct
balances and mutually unreachable post-state roots. A failed overdraw on one
shard remains failed while the other shard receives a concurrent top-up. This
proves that identical public purse identity does not collapse storage authority
across shard runtime boundaries. The test then drops both runtime managers,
reopens each independently scoped persistent store, and rechecks root ownership
and balances, binding the crash/restart model to actual RSpace history.

Casper's shared test LMDB is created under `target/casper-test-scratch`, never
the host's `/tmp`. The test harness registers exact-path normal-exit cleanup
because a `lazy_static` `TempDir` is not dropped by Rust. A subprocess regression
initializes the environment, exits, and proves that the registered directory is
removed. An abnormal kill may leave disk-backed scratch for inspection, but it
cannot consume tmpfs RAM.

These state counts describe the checked finite configurations. They are not a
claim that only three validators or two shards are supported.

## Executable gates

The canonical commands are:

```bash
scripts/check-parallel-validator-consensus.sh
scripts/check-finalized-floor-ALL.sh
scripts/check-cost-accounted-rho-proofs.sh
scripts/check-cost-accounted-rho-tla-invariants.sh
scripts/check-cost-accounted-rho-apalache.sh
scripts/check-cost-accounted-rho-loom.sh
cargo test -p rspace_plus_plus --test concurrent_rspace_test
cargo test -p casper concurrent_shards_keep_vault_balances_roots_and_failures_isolated
```

The TLC runners use one bounded worker, a bounded JVM heap, a hard resident or
address-space ceiling, and an on-disk metadir. They never place TLC state graphs
in `/tmp`.

## Diagnostic interpretation

Different block hashes for one deploy are not by themselves proof of conflicting
Casper certificates: a deploy may appear in competing blocks before one becomes
canonical. They become a protocol defect when nodes disagree after the relevant
finalized state certificate or when a promoted floor omits an already committed
effect.

The following combinations identify violations of this contract:

- `UnknownRootError` after another runtime resets indicates shared-pointer
  authority or premature root deletion.
- A support message without successful local replay indicates early support or
  peer-evidence trust.
- Equal candidate hashes with different published roots indicate non-atomic or
  shared-pointer publication.
- A promoted floor lacking a previously committed effect indicates stale-floor
  admission.
- Correct per-shard totals with another shard's balance changed indicate a frame
  violation even if global conservation happens to hold.
- Growing validation backlog with no bounded admission or retirement indicates
  a liveness/resource-lifetime failure, not a reason to weaken state checks.

The repair rule is to restore the violated authority, atomicity, lineage, or
frame invariant. Tests must not normalize divergent outputs, relax consensus
assertions, hide invalid-block logs, or increase resource ceilings to make a
failure disappear.
