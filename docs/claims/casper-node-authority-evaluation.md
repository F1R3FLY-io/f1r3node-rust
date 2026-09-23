# Casper Node Authority Evaluation

```yaml
claim_id: CLAIM-CASPER-NODE-OBSERVATION-003
status: pending
adapter: null
scope: batch-b2-detached-authority-evaluation
artifacts:
  - node/src/rust/runtime/node_runtime.rs
  - node/src/rust/runtime/setup.rs
  - node/src/rust/soak_observer.rs
  - node/tests/soak_observer.rs
  - casper/src/rust/mod.rs
  - casper/src/rust/soak_observer.rs
  - casper/src/rust/soak_observer/evaluation.rs
  - casper/src/rust/soak_observer/reference.rs
  - casper/src/rust/casper.rs
  - casper/src/rust/engine/engine_cell.rs
  - casper/src/rust/engine/multi_parent_casper/types.rs
  - casper/src/rust/engine/multi_parent_casper/dispatch.rs
  - casper/src/rust/engine/multi_parent_casper/finalization_runner.rs
  - casper/src/rust/finality/floor.rs
  - casper/src/rust/safety/clique_oracle.rs
  - casper/src/rust/util/clique.rs
  - shared/src/rust/dag/mod.rs
  - shared/src/rust/dag/observation_work.rs
  - block-storage/src/rust/dag/block_dag_key_value_storage.rs
  - block-storage/tests/soak_snapshot.rs
  - casper/tests/soak_observer.rs
  - casper/tests/helper/test_node.rs
  - casper/tests/batch1/multi_parent_casper_bonding_spec.rs
  - casper/tests/api/pending_deploys_test.rs
  - casper/tests/api/deploy_finalization_status_test.rs
  - casper/tests/api/last_finalized_api_test.rs
  - casper/tests/api/bonded_status_api_test.rs
refutation: pending
construction: pending
binding: pending
soak: pending
```

## Authorization and scope

The user requested completion of TASK-019-3 on 2026-09-23.
This request authorizes the planned B2 implementation and its mandatory verification records.
It supersedes the earlier restriction to planning steps 1 through 4.
It does not accept the Batch A, B1, or B2 claims.

The [batch plan](../plans/casper-node-observation-batch-b.md) defines the implementation scope.
The implementation starts at `4c0c0dbe7`.
Batch A and B1 acceptance remains pending under TASK-019-4.

## Required properties

1. Ordinary startup installs no observer handle, queue, task, or endpoint.
2. Attachment performs bounded bookkeeping without reading stores or invoking a finalizer, production snapshot, or validator identity.
3. Each instance accepts one attachment. Conflicting attachment changes observer availability without preventing engine installation.
4. Coverage begins at attachment and ends at replacement or shutdown. A request rejects an instance change.
5. Event insertion never waits for capacity. Loss and counter overflow prevent complete coverage.
6. Live derivation, effect attempt, effect return, detached derivation, and observed persisted metadata remain distinct records.
7. Authority requests retain the Batch A identity, permission, challenge, frame, and deadline checks.
8. Evaluation uses one B1 capture. It releases production guards before scratch construction, evaluation, or serialization.
9. Exact, original, and reference paths identify their input scope and authority digest.
10. Adopted public parameters and approved state identity supply authority inputs. Startup defaults cannot replace adopted parameters.
11. The reference uses captured immutable data without production floor, oracle, clique, or traversal helpers.
12. Work, allocation, traversal, clique expansion, recursion, and deadline checks occur before their bounded operations.
13. Exact decisions, original floating-point bits, display projections, and persisted values have separate availability and identity fields.
14. Missing body coverage, restore provenance, display inputs, or counters produce explicit unavailable results.
15. Observation changes no production storage, consensus limits, or ordinary execution result.

## Evidence and acceptance

Tests must cover all constructor routes, attachment states, request limits, replacement, shutdown, record loss, and unchanged production bytes.
Reference controls must expose incorrect threshold, traversal, clique, cache, and containment behavior.
A bounded test instance does not establish the complete unbounded property.
Each property requires the applicability review and verification tiers defined by [the CbC policy](../cbc-verification-tiers.md).

This claim remains pending until source-bound verification and named maintainer acceptance are recorded.
Unavailable display and restore inputs do not qualify a live authority profile.
