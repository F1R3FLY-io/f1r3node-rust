# Publication and Restart Profile

This profile implements controlled-transcript checks for [CLAIM-CASPER-SOAK-003](../../../../../docs/claims/casper-soak-publication.md). It does not execute or repair the node.

## Commands

Build the separate profile binary:

```bash
cargo build --locked -p casper-soak --bin casper-publication
```

Run the executable fixtures and bounded model controls:

```bash
TLA_TOOLS_JAR="$HOME/.tla/tla2tools.jar" \
  bash scripts/casper-soak/check-publication.sh target/publication-checks
```

Set `SOAK_PUBLICATION_JAVA` to select another Java executable. The runner verifies the pinned TLC JAR digest.

The runner records `CARGO_PROFILE_TEST_OPT_LEVEL`. An explicit value of `0` avoids the observed native-CPU optimizer failure on the local macOS toolchain.

Inspect the compiled identity:

```bash
target/debug/casper-publication identity
```

Process a retained fixture with the same executable:

```bash
target/debug/casper-publication run \
  --manifest target/publication-checks/fixtures/publication_complete/manifest.json \
  --request target/publication-checks/fixtures/publication_complete/request.json \
  --artifacts target/publication-checks/fixtures/publication_complete \
  --output target/publication-replay
```

Each output path must be new. Exit codes are `0` for passed, `1` for incomplete or product failure, `2` for invalid input, and `3` for blocked.

Outputs retain exact input bytes, qualification records, raw observations, generation requests, collection results, and the scenario report.

A synthetic scenario cannot become node evidence or a passing soak. Model and fixture checks do not discharge the claim without binding acceptance.

## Request and observation contract

The [common contract](../../../../../docs/casper/design/soak-interface-contract.md) defines provenance, capture, presence states, and phase boundaries.

The request binds the manifest, candidate, node, segment, iteration, seed, scenario, pair, member, and predecessor/successor incarnations.

It also supplies `publication_id`, `generation`, `minimum_generation`, `cut_point`, `expected_tuple`, `unresolved_occurrences`, and an observation deadline.

The publication identifier scopes the pinned scenario across its pre-crash and recovered generations. Node-specific identifier mappings require adapter qualification.

Generation values and monotonic times use canonical unsigned decimal strings. Digests use lowercase hexadecimal strings.

The generator returns an ordered schedule and separate workload and fault requests:

```mermaid
flowchart LR
  F[Prepare pinned fixture] --> B[Capture prior snapshot]
  B --> C[Observe publication boundary and exit]
  C --> R[Observe linked restart and readiness]
  R --> A[Capture recovered snapshot]
```

The `configuration`, `fixture`, and `expectation` input references must match manifest digests. Inline expectations cannot override those artifacts.

Each complete transcript has three unique records:

| Record | Required content |
| --- | --- |
| `before_snapshot` | Prior incarnation, complete tuple, retained-work inventory, and terminal-verdict inventory. |
| `fault_ack` | Requested cut point and generation, observed boundary and exit, linked successor, and readiness. |
| `recovered_snapshot` | Matching successor and predecessor, complete tuple, retained work, and terminal verdicts. |

Snapshots carry an exact Boolean `atomic` flag and the fixture digest. Tuple, retained-work, and terminal-verdict measurements use explicit presence states.

An observed empty inventory is valid. Missing or error inventories have null values and nonempty reasons. They are not empty inventories.

The atomic flag records an adapter assertion. It does not prove that an unqualified node interface produces atomic snapshots.

A tuple contains exactly `publication_id`, `generation`, `block_hash`, `state_root`, and `effect_digest`. The classifier compares complete tuples and never assembles components across records.

Before-publication cuts permit the pinned old or new tuple. After-publication cuts permit only the pinned new tuple. A generation below the declared minimum remains a failure.

Occurrence identities remain distinct even when deploy signatures match. An occurrence can leave retained work only when its matching durable terminal verdict is observed.

Previously observed durable terminal verdicts must survive restart without changes. Missing work measurements cannot erase an independently observed lost verdict.

Unknown prior verdicts cannot erase independently observed work loss. If the initial occurrence was not observed, that occurrence comparison remains incomplete.

## Fault acknowledgment and correlation

A compound fault receipt contains `boundary`, `exit`, and `restart` events. Each event has an identity, producer, exact observation flag, sequence, clock, and monotonic time.

All receipt events belong to one observer producer and clock. Their sequence numbers strictly increase, and their monotonic times do not decrease.

The receipt also requires `status: applied`, Boolean exit and readiness evidence, and the exact requested predecessor and successor.

The prior snapshot must precede the boundary. The recovered snapshot must follow readiness in that same observer clock domain.

A method return, generic restart, requested crash, or wall-clock timestamp cannot replace these receipts. Missing receipt evidence gives zero acknowledged fault coverage.

Foreign identities and late observations remain quarantined with their source references. They cannot fill a required slot. An unrelated restart produces an incomplete scenario.

Identical event copies count once. Conflicting copies, duplicated transport identities, malformed records, or corrupt artifacts prevent a passed result.

The transport bound is 64 records. Work and terminal inventories each have a bound of 64 entries. Repeated snapshots do not become an arbitrary history comparison.

## Bounded verification

The model explores two scenarios with three collection slots each. It abstracts fault acknowledgment, restart matching, and tuple completeness.

| Property | Defect knob | Executable fixture |
| --- | --- | --- |
| `FaultAcknowledged` | `AssumeCrashApplied` | `publication_fault_unacknowledged` |
| `RestartIdentityMatched` | `MixRestartObservations` | `publication_restart_mismatch` |
| `TornTupleReported` | `HideTupleMismatch` | `publication_torn_tuple` |

The clean configuration requires a completed error-free search. Each negative configuration requires exit 12, its named invariant violation, and a counterexample trace.

The model assumes other fields are valid and excludes independent earlier failures. Executable fixtures separately check work loss, durable verdicts, malformed inputs, and failure retention.

The wrapper reuses the unchanged model runner. It rebases only the staged plan's model path and retains both plans with their digests.

Model and configuration bytes remain unchanged. Original source inputs must retain their digests after execution.

The tests cover every old/new block/root/effect combination at both cut points. These controlled cases do not establish valid node execution or an extended property campaign.

## Remaining boundaries

Live boundary, exit, restart, atomic-snapshot, and durable-work adapters remain unqualified. All `node_observation` requests remain blocked.

Parallel-policy and post-merge requests remain blocked. They cannot modify baseline configuration or substitute for TASK-017-12 approval.

The new workflow runs on relevant pull requests and nightly. Its proposed mandatory tag requires human ratification before commit.

Existing broad tags cover the Rust, Bash, and formal files. Existing lifecycle and authority/finality artifacts remain unchanged.
