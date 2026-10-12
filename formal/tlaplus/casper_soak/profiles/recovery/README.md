# Recovery and Custody Profile

This controlled-transcript profile implements [CLAIM-CASPER-SOAK-004](../../../../../docs/claims/casper-soak-recovery.md). It launches no node and grants no policy authority.

## Commands

Build the separate binary:

```bash
cargo build --locked -p casper-soak --bin casper-recovery
```

Run the fixtures and model controls:

```bash
TLA_TOOLS_JAR="$HOME/.tla/tla2tools.jar" \
  bash scripts/casper-soak/check-recovery.sh target/recovery-checks
```

Set `SOAK_RECOVERY_JAVA` to select Java. Set `CARGO_PROFILE_TEST_OPT_LEVEL=0` for the local macOS toolchain when its native-CPU optimizer fails.

The runner records tool versions, optimization level, source digests, fixture results, and model results. Its output directory must be new.

Replay a retained fixture with the same executable:

```bash
target/debug/casper-recovery run \
  --manifest target/recovery-checks/fixtures/recovery_complete/manifest.json \
  --request target/recovery-checks/fixtures/recovery_complete/request.json \
  --artifacts target/recovery-checks/fixtures/recovery_complete \
  --output target/recovery-replay
```

The `identity` command reports compiled source and executable identities. A changed executable requires new manifest and qualification pins.

Exit codes are `0` for passed, `1` for incomplete or product failure, `2` for invalid input, and `3` for blocked.

Every scenario retains `soak_verdict: non_passing` and `harness_verification: pending`. Fixture success is not node evidence or claim acceptance.

## Authority and schema limits

D-06 preserves all-eligible stale recovery, leader-only convergence, frontier follow, readiness, and backstop behavior. The profile keeps these lane labels separate.

D-07 identifies uncertainty about exact-occurrence and reason-join support on the selected `dev` line. This profile does not resolve that ratification question.

Tests use `synthetic-source-occurrence-v1`. The manifest, request, qualification proof, fixture, and snapshot must bind the same occurrence schema.

An adapter must preserve source occurrence identities. Observation position, deploy signature, or execution order cannot replace an occurrence identity.

All live and post-merge requests remain blocked. Experimental leadership, clock, frontier, and coverage requests also remain blocked, even with qualified-looking metadata.

The generator retains requested experimental dimensions as blocked context. It does not generate executable workload or fault requests for those variants.

No current node baseline, comparative soak result, supported experimental build, or adapter qualification is claimed.

## Input and generation contract

The [common contract](../../../../../docs/casper/design/soak-interface-contract.md) defines identity, provenance, measurement presence, and outcome separation.

The request binds candidate, binary, scenario, pair, member, node, incarnation, segment, iteration, seed, policy, lane, occurrence schema, frontier, objective height, and lifespan.

Supported baseline lanes are `stale_recovery`, `convergence`, `frontier_follow`, `pending_deploy`, `readiness`, and `backstop`.

Stale recovery requires `all_eligible` leadership. Convergence requires `leader_only`. Other lanes retain the declared `baseline` leadership label.

Baseline requests require `one_parent_b1` coverage and the `baseline` clock policy. These labels do not redefine production configuration.

Configuration, fixture, and expectation artifacts must match manifest hashes. The separate frontier artifact must match `frontier_digest`.

Frontier artifacts are opaque synthetic topology descriptions, not valid node blocks. Expected one-parent and collective coverage are separately pinned Boolean values.

The generator returns this schedule:

```mermaid
flowchart LR
  F[Load pinned fixture] --> S[Apply requested fault schedule]
  S --> O[Observe recovery]
  O --> C[Correlate sources and classify]
```

Fault requests support pause and delayed delivery. Each request identifies its target incarnation, trigger, deadline, and prior fault dependencies.

A dependency must name an earlier scheduled request. Generation does not invent acknowledgments, occurrence identities, or delivery outcomes.

## Collection and fault evidence

The collector retains at most 64 immutable transport records. Each record has a unique transport identity and an exact artifact digest.

Repeated copies of one event count once. Conflicting copies invalidate evidence, including contradictions that change payload lane or frontier labels.

Event keys include immutable context and encoded producer identity. Identifier punctuation cannot merge separate producer keys.

Foreign runs, lanes, policies, schemas, frontiers, or process incarnations cannot fill required observation slots. Their raw references remain in the rejection inventory.

Fault coverage requires `status: applied`, exact Boolean observation flags, matching trigger identity, observed trigger evidence, and an on-time receipt.

Pause receipts require an observed paused or stopped state. Delivery receipts require the exact message identity and observed delivery state.

Ordered receipts require the same observer producer and clock, increasing producer sequence numbers, and nondecreasing monotonic times.

The recovery snapshot must follow acknowledged fault completion. A command return or a snapshot captured before the fault cannot establish exercised coverage.

The snapshot requires an applied evaluation receipt with the pinned fixture digest and ordered `load_fixture` and `observe_recovery` steps.

## Occurrences and measurements

A source occurrence has candidate-scoped `occurrence_id`, deploy signature, carrier block, and sender. Sample identities remain distinct from occurrence identities.

Two observations of A and one of B produce two occurrences and one duplicate observation. Equal deploy signatures do not merge A and B.

Each sample retains measured custodian, reason inputs, joined reason, causal references, tombstone, lease, terminal outcome, retry authorization, body availability, objective height, lifespan, and retry count.

The classifier compares these fields with pinned fixture expectations. It does not calculate node retry authority or select a new reason-join rule.

Reason-input and causal-reference comparisons use sets. Their raw arrays remain retained, including ordering and repeated entries.

Joined reasons remain independent pinned values. Set normalization does not prove a node semilattice implementation or resolve D-07.

An expired lease cannot override a pinned refusal. Tests retain refused-retry evidence even when the observed lease has expired.

Counts distinguish source occurrences, sample observations, duplicate occurrence observations, custody disagreements, completed retries, and expiry.

Missing terminal or custody measurements produce null aggregate values with reasons. They do not become zero. Observed empty inventories remain distinct from missing inventories.

The classifier retains block count, cadence, latency, resident memory, and CPU time with declared units. Missing telemetry cannot erase an independently observed state mismatch.

Missing occurrence samples cannot erase an independently observed telemetry mismatch. Invalid-input precedence does not convert a failure into a passed result.

Recovery duration uses canonical monotonic endpoints in one clock domain. Different clock domains produce an unknown duration.

Cross-clock normalization is not implemented, even when a transcript supplies additional synchronization metadata. The profile makes no synchronized latency claim.

Inventory flags and applied receipts remain provider assertions. Synthetic fixtures do not independently prove node execution, containment, or complete telemetry collection.

## Bounded model and bindings

The model explores two scenarios with three occurrence sample slots. The slots represent A, another observation of A, and B.

| Property | Defect knob | Executable fixture |
| --- | --- | --- |
| `LaneLabelsPreserved` | `ConflateRecoveryLanes` | `recovery_lane_mismatch` |
| `OccurrenceCountsPreserved` | `CollapseOccurrenceIdentity` | `recovery_occurrence_counts` |
| `PauseCoverageAcknowledged` | `AssumePauseApplied` | `recovery_pause_unacknowledged` |

The count fixture is valid input. It passes only when the binary reports two occurrences and one duplicate observation.

The model assumes other identity fields, qualification records, receipts, and measurements are valid. It excludes independent product failures and unbounded histories.

The wrapper preserves model and configuration bytes. It changes only the staged plan's model path for the unchanged shared runner.

A clean result requires exit 0 and an error-free completed search. Each negative control requires exit 12, its named invariant violation, and a counterexample trace.

Model success, fixture success, source-ledger validation, and human binding acceptance remain separate checks. Construction is not applicable.

## Remaining gates

The PR/nightly workflow retains its evidence. Its proposed mandatory tag requires human ratification before commit.

CLAIM-004 remains pending. Candidate qualification, workload pins, resource approval, and real baseline campaigns remain under TASK-017-12.

Post-merge adaptation requires the actual merge and accepted handoff. Missing interfaces do not authorize node repairs or policy changes.
