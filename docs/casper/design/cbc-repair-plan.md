# Casper CbC Repair Plan

**Status:** Draft. Production implementation is pending.

## Purpose

This plan defines non-cost-accounting repairs for the F1R3FLY Casper implementation.

Correct by Construction (CbC) evidence must connect each design invariant to production Rust behavior.

The formal models define the required behavior. A model result alone does not prove that production Rust implements that behavior.

## Scope

This plan covers these Casper areas:

- finalized-floor selection and promotion
- finality arithmetic
- fork choice and parent selection
- multi-parent state merge
- deploy occurrence and recovery
- historical replay
- validator and shard root ownership
- recovery leadership and local failure handling
- slashing authorization
- bounded traversal and runtime resource ownership

This plan does not cover execution cost, metering, purses, fees, or cost-accounted Rho semantics.

## Design constraints

The implementation must retain weighted clique voting, equivocation handling, and Latest Message Driven Greedy Heaviest Observed Subtree fork choice.

The implementation must make each consensus decision a deterministic function of authenticated consensus data.

The implementation must not use collection order, local cache state, arrival order, or ambient runtime state as consensus inputs.

The implementation must preserve certified state effects across parent selection, replay, merge, and floor promotion.

The implementation must isolate provisional roots until validation commits an authenticated state transition.

## Required invariants

### Certified floor

A selected parent set must descend from the certified floor.

The selected parent state must retain every effect in the certified floor state.

Floor promotion must materialize the candidate state before later execution uses that state.

A state-retention certificate must use the same frozen latest-message view as the finality decision.

### Finality arithmetic

Finality decisions must use checked integer arithmetic.

Fixed-point display values must not change the exact decision boundary.

All multiplication and addition must reject overflow before a finality comparison.

### Fork choice and parents

Fork-choice scoring must use checked arithmetic and a strict total order.

Lowest-universal-common-ancestor selection must terminate and produce one result for one authenticated DAG.

Proposer and receiver paths must apply identical parent depth, lineage, floor, and state-retention rules.

### Merge and deploy lifecycle

Deploy occurrences must include their source identity.

Merge survivor order must be total and independent of input order.

Merge operations must authenticate repeated effects and reject inconsistent duplicates.

Each occurrence must have one canonical disposition in one merge scope.

A terminal rejection must not become executable because local state later changes.

### Replay

Each execution record must bind its pre-state root and post-state root.

Replay must start from the recorded pre-state root.

Replay must reproduce the recorded post-state root before validation accepts the block.

A node must not replace historical replay context with its current local context.

### Runtime isolation

Each evaluation must own its base root, provisional roots, and publication boundary.

A rollback must remove only provisional roots that the same evaluation owns.

Each shard must update only its own ledger, root index, and version.

Shared worker ownership must remain unique and bounded.

Block-heap reclamation must not change consensus semantics.

### Recovery and failures

Recovery input must have a fixed bound.

Recovery leadership must derive from authenticated round data.

Leader unavailability must cause a deterministic transition to a later recovery round.

A local root failure must not become objective invalid-block evidence.

Retry and quarantine rules must terminate or reach a terminal disposition.

### Slashing

Slash authorization must use the committee and evidence bound to the relevant parent or certified floor.

Proposer and receiver authorization must produce the same result.

Later bonding events must not change the classification of historical evidence.

Missing local evidence must cause dependency recovery rather than objective rejection.

### Resource bounds

Floor and frontier traversals must terminate under explicit bounds.

Cached results must be write-once values of deterministic functions.

Runtime, block, evaluation, validator, and shard owners must have explicit cleanup boundaries.

Reclamation must preserve every root that another live owner can reach.

## Development method

Use one test-driven development cycle for one behavior.

```mermaid
flowchart LR
    D[Specify one invariant] --> R[RED: reproduce the missing behavior]
    R --> M[Add or refine the formal model]
    M --> I[Implement the Rust repair]
    I --> B[Add a source bridge test]
    B --> G[GREEN: run the focused proof gate]
    G --> C[Record CbC evidence]
    C --> S[Run integration and soak checks]
```

The RED result must reproduce a production failure, a proof gap, or an expected model counterexample.

The GREEN result must include the production test and the related formal check.

A proof claim remains pending until its source bridge identifies the production function and authenticated inputs.

## Verification layers

Use Rocq for unbounded algebraic and inductive claims.

Use TLA+ and TLC for bounded concurrency, lifecycle, crash, and liveness state spaces.

Use Apalache for symbolic bounded checks and independent negative controls.

Use Lean for small independent validator and slashing kernels.

Use Z3 and Sage for arithmetic witnesses and minimized counterexamples.

Use Loom for shared-memory ownership and publication interleavings.

Use Rust property tests, integration tests, and fuzz targets for production bridges.

Use scheduled soak preflight runs for composed multi-node and resource checks.

## Completion rule

Do not mark a repair complete until all applicable evidence layers pass.

Record unavailable tools and deferred exhaustive checks as open evidence gaps.

Do not treat a local-only model pass as proof of production conformance.

Keep this plan in draft status until every repair has an implementation reference and a discharged CbC claim.
