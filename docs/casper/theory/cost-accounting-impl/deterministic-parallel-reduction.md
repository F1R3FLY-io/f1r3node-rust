# Deterministic Parallel Reduction and Checkpoint Ownership

## Scope

This document specifies the consensus-critical boundary between parallel
Rholang reduction, RSpace matching, cost-authority settlement, replay logging,
and checkpoint publication. It refines the concurrency-independent semantics in
[*Cost-Accounted Rho Calculus*](https://github.com/F1R3FLY-io/publications/blob/main/cost-accounting/cost-accounted-rho.tex)
and the cost endofunctor in
[*Continued Interactive GSLTs and the Cost Endofunctor*](https://github.com/F1R3FLY-io/publications/blob/main/cost-accounting-as-monad/continued-gslt-cost-v2.tex)
onto F1R3node's Tokio reducer and RSpace implementation.

The publications require reduction-order-independent cost and conserved linear
authority. The node additionally has to choose one replayable outcome when
several parallel branches compete for the same RSpace datum, continuation,
join, or purse region. Sorting only the candidates already visible to a worker
is insufficient: a worker cannot sort a competing intent that has not yet been
submitted.

The implementation therefore freezes complete communication frontiers,
serializes only transitively conflicting operations in canonical causal order,
and executes disjoint components concurrently. It does not serialize a deploy,
a block, a validator, or a shard globally.

![Deterministic parallel reduction pipeline. Parallel Rholang branches meet at a complete frontier. Exact channel, join, and authority footprints form transitive conflict components. Operations are canonically ordered inside each component while disjoint components execute concurrently. Their events enter one causally ordered log, and checkpoint publication waits for evaluation quiescence.](../diagrams/deterministic-parallel-reduction.svg)

(*Source: [`deterministic-parallel-reduction.puml`](../diagrams/deterministic-parallel-reduction.puml).*)

## Terms

A **participant** is one live causal branch of the current reducer evaluation.
Splitting a parallel composition replaces its parent participant with its child
participants. Rejoining restores the parent only after every child has
completed.

An **intent** is a typed request to perform one RSpace `produce` or `consume`,
including persistence, peek, pattern, continuation, data, and cost-authority
metadata.

A **complete frontier** is reached when every live participant is either
waiting on exactly one intent or has completed. No waiting intent is committed
before that condition holds.

A **causal operation order** is the lexicographic pair of the evaluation
session identity and a tree path. A path segment `(step, 0)` identifies an
operation. Splitting appends `(step, 1)` and a child index. Consequently, an
operation before a split precedes every child operation, and every child
operation precedes the parent's first operation after the join.

An **operation footprint** is the set of mutable semantic resources that an
intent may touch at the frozen frontier. It contains:

- the intent's direct channel or channels;
- every channel in each pre-state join reachable from a produced channel;
- each individual cost-authority region instance that may be debited.

An **authority region** is one linear purse identity carried in a
`CostAuthority`. Compound authority is represented by multiple region keys, not
by one hash of the compound object. Thus authorities `{A, B}` and `{B, C}`
conflict on `B` even when their complete encodings differ.

A **conflict component** is a connected component of the undirected graph whose
vertices are intents and whose edges join intersecting footprints. Connected
components, rather than pairwise batches, are required because conflicts are
transitive.

An **evaluation permit** is a shared permit held for the complete lifetime of
an evaluation. Checkpoint, reset, rollback, and replay-boundary operations take
the exclusive permit.

## Minimal counterexample

Consider:

```rholang
@"x"!(1) | @"x"!(2) | for (@y <- @"x") { @"out"!(y) }
```

If the consumer races with the two producers, local arrival order can choose
either value. That changes the COMM event, the residual datum, the RSpace root,
and any continuation state derived from `y`. Canonically sorting only the
currently stored producers does not solve the defect: the consumer may commit
after seeing value `2` but before value `1` is submitted.

At the complete frontier, all three intents are visible. Their channel
footprints intersect, so they form one component. Canonical order commits
produce `1`, produce `2`, then consume. The consumer receives `1`, value `2`
remains, and every validator records the same exact log and root.

## Production transition

For intent $`i`$, let $`C_i`$ be its direct channels, $`J_i`$ the channels in
pre-state joins reachable from a produced channel, and $`A_i`$ its individual
authority-region identities. Its footprint is

```math
F_i = C_i \cup J_i \cup A_i.
```

Two intents conflict exactly when

```math
F_i \cap F_j \ne \varnothing.
```

The scheduler applies the following transition:

```text
submit(participant, intent):
    bind intent to participant's next causal operation path
    mark participant waiting
    if every live participant is waiting:
        freeze all submitted intents
        expand exact channel, join, and authority footprints
        compute transitive conflict components
        for each component concurrently:
            execute its intents in canonical causal order
        publish completion responses in canonical causal order
        mark their participants running
```

RSpace operations in distinct components may overlap in wall-clock time. The
existing striped channel locks remain the mutation linearization mechanism.
The scheduler adds the proof that any concurrently executed components are
semantically independent; it does not replace RSpace's atomic matching.

Within one component, existing RSpace semantics remain authoritative:
persistent sends remain available, persistent receives may fire repeatedly,
peek receives restore their data, guards and patterns select matches, and joins
commit atomically. The scheduler changes which complete competing set is
linearized first, not what any operation means.

## Causal event log

Every scheduled RSpace operation runs under its `OperationOrder`. RSpace stores
the resulting I/O and COMM events in a map ordered by that key. Checkpoint,
soft checkpoint, and explicit trace extraction drain the map in key order.
External-service metadata updates search both ordinary and causally ordered
events, so replay-visible output and failure metadata cannot be lost merely
because an operation was scheduled.

This ordering is consensus evidence. Tokio wake order, mutex acquisition order,
and component completion order are deliberately excluded. The current Casper
execution path evaluates user deploys sequentially in their block order;
parallelism here is intra-deploy. Duplicate deployment occurrences are rejected
by admission, so a live RSpace does not host two concurrent sessions with the
same consensus deployment identity.

## Cost-authority integration

Channel independence does not imply economic independence. Two operations on
different channels can debit the same located purse. Such operations belong to
one conflict component because the footprint includes every authority region.
Conversely, operations with disjoint channels, disjoint joins, and disjoint
authority regions remain eligible for parallel execution.

When deploy accounting is active, a missing or empty authority uses a
fail-safe common footprint. The downstream authority validation still rejects
malformed or unfunded execution; the common footprint prevents an invalid
operation from racing a valid debit before that rejection. Bootstrap and other
unmetered execution do not introduce an artificial purse conflict because they
have no user settlement authority.

The scheduler does not charge. RSpace's pre-mutation observer remains the
linearization point for compute and quantitative-byte reservations. The
scheduler guarantees that the selected COMM and authority order are canonical;
the observer guarantees that each selected mutation is fully funded before it
changes state.

## Checkpoint, cancellation, and rollback

An evaluation acquires a shared epoch permit before registering its root
participant. The permit is owned by the reduction session, not by the root
future. It is released only when:

```math
participants = \varnothing
\land intents = \varnothing
\land \neg driverActive.
```

This ownership is necessary because cancelling a Tokio root future detaches
already spawned child tasks unless those children are explicitly aborted.
Dropping only the root's permit would allow a checkpoint to publish while a
detached child or frontier driver was still mutating RSpace. Participant guards
remove cancelled waiters, and the session retains the permit until every
detached child and in-flight driver is quiescent.

Checkpoint, soft checkpoint, event-log extraction, reset, rollback, replay
rigging, and administrative state mutation acquire the exclusive epoch permit.
Tokio's read/write lock permits concurrent evaluations and gives boundary
writers a finite queue position. This boundary serialization is not reducer
serialization: all evaluations may hold shared permits concurrently, and all
disjoint components within them remain parallel.

A branch that performs unbounded pure computation can delay its frontier.
Production user execution is bounded by authenticated fuel, so such a branch
must exhaust its finite budget or reach its next intent. Cancellation keeps the
checkpoint boundary closed until already detached work finishes; higher-level
evaluation rollback then restores the captured soft checkpoint if execution
fails.

## Replay and consensus consequences

For a fixed normalized process, random seed, authenticated pre-state,
cost-authority allocation, and protocol version, the reducer now commits one
causal COMM trace. Play records that trace. Replay resets to the original
pre-state, rigs the recorded events, reevaluates under the same causal order,
requires every event to be consumed, and requires the resulting root and cost
to match.

This closes the observed validator disagreement in which two honest nodes
placed the same deployment in different blocks because parallel RSpace arrival
selected different residual state. Casper's majority, clique, fork-choice, and
finalization rules are unchanged. The repair ensures those rules vote over a
deterministic state transition rather than over scheduler-dependent roots.

## Verification and regression matrix

| Obligation | Formal evidence | Executable evidence |
| --- | --- | --- |
| A complete frontier has one terminal state under every submit order | [`DeterministicParallelReduction.tla`](../../../../formal/tlaplus/deterministic_parallel_reduction/DeterministicParallelReduction.tla), with arrival and non-canonical-order controls | competing-producer and competing-consumer exact-root/log tests |
| Transitive channel and authority conflicts form one component | [`DeterministicParallelReduction.v`](../../../../formal/rocq/cost_accounted_rho/theories/DeterministicParallelReduction.v), axiom-free compound-overlap and commutation theorems | component property test and shared-region unit regression |
| Truly disjoint components retain parallel eligibility | safe TLA+ `Inv_DisjointWorkRemainsParallel`; global-serialization control | Loom authority-component interleavings |
| Shared purse regions never execute as disjoint work | safe TLA+ `Inv_SharedAuthorityNeverRunsAsDisjoint`; authority-omission control | compound `{A,B}` / `{B,C}` regression |
| Checkpoints never observe partial frontiers or detached children | [`EvaluationBoundary.tla`](../../../../formal/tlaplus/deterministic_parallel_reduction/EvaluationBoundary.tla), checkpoint and cancellation controls; Rocq epoch theorems | Loom checkpoint/cancellation schedules and cancelled-root production test |
| Event logs use causal rather than arrival order | canonical terminal/log TLA+ state | reversed-arrival log, soft-checkpoint/revert, and external metadata-update tests |
| Existing RSpace constructs refine the same scheduler contract | disjoint-commutation theorem plus legacy operation semantics | joins, persistence, peek, guards, and textual ACI permutation tests |
| Play and replay agree exactly | canonical frontier and causal-segment theorems | interacting-body cost, event-consumption, and post-root equality test |

The safe TLC models and every unsafe counterexample run from
`scripts/check-cost-accounted-rho-tla-invariants.sh`. Apalache independently
checks the safe ten-step reduction horizon, the safe four-step evaluation
boundary, and one exact symbolic counterexample for each defect through
`scripts/check-cost-accounted-rho-apalache.sh --filter deterministic`. The Rocq module is part of
the aggregate axiom and assumption gate. Loom runs from
`scripts/check-cost-accounted-rho-loom.sh`. Rust tests are ordinary workspace
tests and do not require a test-only consensus path.

## Security properties

- A scheduler cannot create funding by running a candidate before its competing
  debit becomes visible.
- Compound authority encoding order cannot hide a shared purse.
- A malformed authority cannot gain concurrency by omitting its regions.
- A cancelled evaluator cannot publish a partial trace or root.
- External-service result metadata remains attached to the exact produce event.
- Arrival order is not part of block identity, cost identity, or replay
  evidence.
- Validator concurrency, shard concurrency, and disjoint intra-deploy work
  remain enabled.

## Operational observability

Reducer call, term, spawn, and cooperative-yield counters remain diagnostic.
They may explain load or starvation but are not consensus inputs. A production
incident involving divergent roots should retain the normalized deployment,
pre-state root, causal event log, protocol version, and authority witness. CPU
timings and Tokio task IDs are useful performance evidence but cannot justify a
different replay result.
