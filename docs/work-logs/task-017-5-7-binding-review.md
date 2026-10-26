# TASK-017-5–7 Bounded Binding Review

## Completion notice

Tasks 017-5–7 are complete. The [acceptance record](./task-017-5-7-acceptance.md) records discharged claims 002–004 and strict completion checks.

External commit `d60f54544` separately restored CLAIM-001 discharge. The sections below preserve the review-stage observations, including earlier pending claim states.

## Status and scope

The three controlled-transcript profiles pass fresh verification. The user accepted the bindings and ratified the workflow tags. Tasks 017-5–7 await ledger recording and strict completion checks.

The [review package](../casper/cbc-evidence/runs/casper-profile-binding-review-20260919-01/report.json) identifies the exact sources, executable evidence, model results, retained failure, and pending candidate ledgers.

This review covers CLAIM-CASPER-SOAK-002, CLAIM-CASPER-SOAK-003, and CLAIM-CASPER-SOAK-004 before the PR #216 merge. Construction is not applicable.

No node ran. No live adapter, production policy, post-merge execution, or soak verdict gained approval. Claims 005–008 remain pending.

## Publication repair

The publication collector previously quarantined observations before it compared event copies. A changed cut point could conceal a contradictory copy and produce a passing scenario verdict.

The retained RED run expected exit 2 and received exit 0. The repair compares immutable event content before correlation and deadline quarantine.

An event key contains the declared context, node incarnation, producer, and event identity. Different contexts remain separate events.

Only the transport `record_id` can differ between identical copies. Clock identifiers, monotonic times, and UTC timestamps remain part of the immutable content.

The regression tests add 22 cases:

- Eighteen cases cover nine contradictory fields in both transport orders.
- Three cases keep foreign candidate, scenario, and member events separate.
- One case retains an independent product failure when contradictory evidence makes the overall verdict invalid.

The existing identical-copy case still passes. No node implementation, shared lifecycle source, or model bound changed.

## Verification results

| Profile | Native and Linux tests | Cases per platform | Invocations per platform | Clean TLC states: generated / distinct |
| --- | --- | --- | --- | --- |
| Authority/finality | Eight on each platform | 58 | 65 | 3,281 / 1,681 |
| Publication/restart | Six on each platform | 79 | 83 | 3,281 / 1,681 |
| Recovery/custody | Seven on each platform | 52 | 55 | 841 / 441 |

The authority unit test checks 2,000 arithmetic cases. Each profile also passed three negative model controls with exit 12, the named invariant, and a counterexample.

Eleven shared native tests passed across claim auditing, binding inventories, manifests, and model results. These regression results do not discharge a claim.

Before acceptance recording, strict audits for claims 001–004 returned 4 because their ledgers remained pending. The full claim bundle also returned 4. These are pending results, not discharges.

Before this task, commit `856fae6ad` moved a function in `host_control.rs` and returned CLAIM-001 to pending. Its current source needs separate acceptance.

The historical 67-source acceptance check reported four differences: the prior driver/specification changes and this task's two publication changes. That failed check remains retained.

This review does not restore CLAIM-001 acceptance. TASK-017-4 remains historically complete, but the current lifecycle binding is pending.

Native controlled-transcript tests used optimization level 0. Linux static builds used optimization level 1. Both used the pinned nightly toolchain.

Linux tests ran in an unprivileged disposable container with no network, host mounts, or Docker socket. The container dropped all capabilities and prohibited privilege escalation.

The container had CPU, memory, process, and time limits. The runner captured all four suite outputs before removal. All 21 Linux tests passed.

The retained Linux image digest is `sha256:7d402dd7f1922bcdbdbeb90d14cecf398fdd36bc5105242fa4a4dc1d1cd596c5`.

TLC used the pinned 2.19 JAR with digest `936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`.

No production-driver process fixture ran on the host. No hosted workflow, node campaign, containment campaign, or Rocq proof ran.

## Binding review map

| Claim and property | Executable binding | Limits |
| --- | --- | --- |
| 002: `MismatchedInputDetected` | Request validation and preparation bind the paired DAG, electorate, justifications, candidate, manifest, and executable. | Opaque synthetic inputs do not prove valid node graphs or signatures. |
| 002: `MissingFinalityDetected` | Presence validation and classification distinguish an observed hold from an absent finality response. | Actual finality adapters remain unqualified. |
| 002: `HeadMismatchReported` | Classification compares correlated heads and retains failures despite missing work counters. | The comparison does not prove fork-choice correctness or traversal bounds. |
| 003: `FaultAcknowledged` | Compound receipts require the observed cut point, process exit, linked restart, readiness, and clock order. | Synthetic receipts do not establish actual crash injection or containment. |
| 003: `RestartIdentityMatched` | Correlation requires the declared predecessor, successor, node, context, and deadline. Contradictory copies produce invalid evidence before quarantine. | Producer and incarnation identities are declared evidence identities, not cryptographic proofs. |
| 003: `TornTupleReported` | Classification compares complete pinned tuples and retains stale-generation, lost-work, and durable-verdict failures. | The atomic flag is an adapter assertion, not a storage proof. |
| 004: `LaneLabelsPreserved` | Request validation and collection keep the declared recovery lane and policy separate. | Unsupported policies remain blocked. |
| 004: `OccurrenceCountsPreserved` | Qualified synthetic schemas and exact occurrence keys preserve multiplicity, duplicate counts, and explicit unknown measurements. | The schema does not establish actual `dev` occurrence support. |
| 004: `PauseCoverageAcknowledged` | Correlated state receipts, dependency order, and snapshot timing establish controlled pause and delivery coverage. | Requested faults and provider return values do not establish observed node state. |

Each model has two scenarios and three observation slots. Models assume valid auxiliary fields and exclude unbounded histories and independent earlier product failures.

Executable fixtures separately test malformed evidence, qualification pins, deterministic generation, blocked requests, unknown values, receipts, and independent failures.

The collector review checked all three implementations. Authority rejects invalid context as invalid input. Recovery already compares copies before payload quarantine. Publication required the recorded repair.

This review does not prove that every possible implementation defect is absent. Passing bounded checks still require source-specific human binding acceptance.

## D-07 and live qualification

D-07 interpretation remains unresolved. Ratifiers must clarify exact-occurrence, tombstone, and reason-join expectations for the baseline node.

Recovery fixtures use an explicit synthetic schema. Pinned expectations support diagnostic comparisons without granting retry authority or establishing a node reason algebra.

One-parent B1 remains the baseline label. Collective coverage, rotating leadership, alternate clocks, and leader-free custody remain blocked experiments.

Unknown durations remain unknown across clock domains. Leases cannot authorize recovery or bypass body, lifespan, or objective-height gates.

TASK-017-12 retains live qualification, actual baseline results, final workload identities, resource approval, and node campaigns. This review does not satisfy those obligations.

## Decisions requested at review

1. Accept or reject the source-bound, bounded pre-merge bindings for claims 002, 003, and 004.
2. Ratify or reject `cbc=mandatory` for the three workflow paths below.

```text
.github/workflows/casper-authority-finality.yml
.github/workflows/casper-publication.yml
.github/workflows/casper-recovery.yml
```

The ratified tags now apply. Approval of these bindings does not resolve D-07, authorize live execution, or accept node correctness.

The candidate ledgers remain pending. Existing canonical profile records retain their historical source and evidence identities until acceptance. No approval transferred through a digest change.

After approval, acceptance must bind the reviewed sources and updated claim specifications. Strict claim audits and task-integrity checks must pass before tracker closure.

## Human decisions (2026-09-19)

The user accepted the source-bound, bounded pre-merge bindings for claims 002, 003, and 004. The user ratified `cbc=mandatory` for the three workflow paths.

An external edit added those tags and the acceptance notice. This assistant paused and requested confirmation. The user replied, “yes - I authorized acceptance”.

This acceptance applies to the reviewed sources in this package. Acceptance must bind those sources and the updated claim specifications before tracker closure.

The acceptance does not resolve D-07, authorize live execution, or accept node correctness.

## Provenance and retained history

Work started at `ba32d5c98eeeec366b2fa790d4f85926c20faddb`. An external merge advanced HEAD to `ab682eea1760c50867dcf7416e37f155b63e5dbc` during verification.

The merge changed ancestry without changing the tracked tree from the starting revision. An external process staged the two publication files. This assistant preserved the index.

The package preserves the initial source snapshot, RED source snapshot, failed assertion, GREEN check, final native outputs, isolated Linux outputs, model traces, and source digests.

Archive ownership is numeric. Evidence logs replace exact workspace, home, and temporary-directory prefixes with placeholders. The package records each changed file and both hashes.

The first packaging attempt stopped at the STE Check. Whole-tracker checking reported 19 legacy findings outside the changed metadata fields.

The retained tracker patch changes only evidence paths and review links. The corrected check covers all four changed prose documents without changing legacy tracker prose.

Original profile packages and the lifecycle acceptance package remain unchanged. Historical unavailable-Linux results do not describe these fresh Linux executions.

The earlier driver-repair failure is historical. The later function-move drift is a current, separate source-acceptance gate.

This assistant did not stage, commit, push, activate tags, waive a claim, repin a candidate, or close a task.
