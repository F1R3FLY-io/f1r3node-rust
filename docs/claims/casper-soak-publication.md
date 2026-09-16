# Casper Publication Soak Claim

```yaml
claim_id: CLAIM-CASPER-SOAK-003
status: pending
adapter: embedded
decisions: [D-05]
pre_merge_tasks: [TASK-017-6]
post_merge_tasks: [TASK-018-3]
artifacts:
  - casper/src/rust/engine/engine_cell.rs
  - casper/src/rust/finality/floor.rs
  - casper/src/rust/blocks/proposer/block_creator.rs
  - block-storage/src/rust/dag/block_dag_key_value_storage.rs
refutation: pending
construction: pending
construction_assumptions: null
binding: pending
soak: pending
```

## Contract

Inputs are versioned evaluation snapshots, candidate blocks, state roots, effect sets, durable terminal verdicts, and crash points.

Outputs are the published block/root/effect tuple, publication order, durable verdict, and remaining unresolved work.

Each publication must expose one complete tuple from one accepted evaluation. Stale evaluations cannot publish after their source context changes.

Restart must recover the same committed prefix. It must not expose a torn tuple or discard unresolved work without a durable terminal verdict.

Original direct-finalization fault-tolerance evidence remains distinct from later projections. A projection cannot replace the original evidence.

Single-flight evaluation remains the default. An optional parallel experiment must preserve publication order, peer-visible order, and finalized results.

## Model and oracle

Audit `formal/tlaplus/finalized_floor/ParallelValidatorConsensus.tla` for reusable publication actions without importing rejected certificate assumptions.

The proposed finite instance uses two evaluators, three candidate tuples, two context versions, and a crash at each durable-write boundary.

The oracle is an append-only sequence of atomic accepted tuples. Restart reconstructs the committed prefix from durable state only.

The bridge covers engine coordination, floor publication, block creation, and DAG storage. TASK-017-6 must identify exact write boundaries and source functions.

## Positive and negative controls

| Control | Required observation |
| --- | --- |
| Clean publication and restart | Recover the same complete committed prefix. |
| Publish an old context | Violate stale-result refusal. |
| Persist root before corresponding effects become durable | Expose a torn tuple and violate atomic publication. |
| Evict on a local finality marker | Lose unresolved work and violate durable-verdict authority. |
| Replace original fault-tolerance evidence with a projection | Violate evidence provenance. |
| Reorder parallel publication | Diverge from the single-flight reference sequence. |

## Tiers and assumptions

TLC checks the finite interleavings. Unbounded publication histories require a Rocq theorem and production bindings.

The candidate construction project is `formal/rocq/finalized_floor/`. Audit its statements and assumptions before naming a matching theorem.

Crash tests assume the documented storage durability contract. They must distinguish process termination from loss of acknowledged durable writes.

Fair scheduling is required only for progress claims. Atomic publication and stale-result refusal are safety properties.

## Phase obligations

Pre-merge fixtures cover current publication paths and preserve baseline evidence. Post-merge fixtures cover the actual #216 publication and restart paths.

The complete publication-ledger architecture remains optional. This claim constrains observable behavior, not a mandatory internal design.

The [harness contract](./casper-soak-harness.md) defines evidence fields and phase closure. All new proof and binding obligations remain pending.
