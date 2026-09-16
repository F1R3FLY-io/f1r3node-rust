---
task: TASK-017-1
branch: formal/soak-casper-consensus
claimed_by: pi-casper-ratification-planning
claimed_at: 2026-09-16T20:29:37Z
handoff_status: paused
next_steps:
  - Resolve the recorded EPIC-017 CbC implementation gaps.
  - Define harness/profile fixtures and interface bindings under TASK-017-2.
  - Resolve prerequisite integration under TASK-017-3 with separate Git consent.
---

# Ratified Casper Soak Preparation

## Current scope correction

The maintainer confirmed that both epics verify only the soak harness and profiles. The earlier scaffold incorrectly included node-correctness obligations.

The node is now explicitly the system under test. Runtime repairs, Rust proofs, and Rocq construction are outside EPIC-017 and EPIC-018.

Eight pending claims cover one harness lifecycle contract and seven profile contracts. Profile controls verify generation, fault acknowledgments, collection, correlation, and verdict classification.

Both epics now list seven harness/formal artifacts. Seven pending ledger records cover those artifacts with updated scope and digests.

The correction removes eight newly added runtime attribute lines and fourteen new runtime evidence placeholders. Their compatibility symlinks are removed too.

Pre-existing runtime attributes and evidence remain intact, including the requested Casper evidence relocation. The carrier-index node claim returns to its pre-scaffold content.

CLAIM-CASPER-SOAK-008 now covers carrier-profile correctness without owning CLAIM-FINALITY-002.

Construction is not applicable to any claim in these two epics. TLC controls and executable harness/profile fixtures remain required.

Post-merge work still requires PR #216's actual merge. It adapts and reverifies profiles rather than proving the merged node.

No model, fixture, or soak has run for this correction. No claim is discharged, and no task is marked complete.

Structural checks confirm eight harness/profile claims, seven artifacts per epic, and seven pending ledger records. Source digests, claim digests, links, and task dependencies pass.

All pre-existing runtime attributes and ledger contents match the pre-scaffold baseline. The carrier-index node claim also matches that baseline exactly.

The deterministic STE Check and `git diff --check` pass. These checks validate the reduced scaffold, not the harness implementation.

## CbC check of the reduced scope

The check used the working tree based on `40e2d2d7c409d9f644c04221286bd95a187d1434`, including the uncommitted scope reduction.

The shared skill driver ran these read-only commands with the default mixed-ledger directory:

```bash
cbc.sh identify --scope epic EPIC-017 --json
cbc.sh discharge --scope epic EPIC-017 --strict --json
```

Identification returned seven mandatory harness/formal artifacts and no node artifacts. Strict discharge returned exit 4, with all seven records pending.

The planned model and eleven configurations are absent. No new executable model can run yet.

All ten planned fixture identifiers are absent from the current fixture script. This check is an inventory check, not a behavioral result.

All seven ledger records retain matching artifact and claim digests. Shell syntax checks pass for the driver, fixture script, summary script, and TLC gate.

The TLC jar is present. Missing model and fixture implementations, not a runtime-proof requirement, prevent verification at this stage.

No verifier adapter ran. A generic successful command cannot discharge a bundle that requires exact TLC control verdicts and executable fixture results.

No claim was refuted, discharged, or waived. The gate remains blocked on pending evidence, not a discovered counterexample.

Complete TASK-017-2 interface and fixture contracts and TASK-017-3 prerequisites before implementing TASK-017-4 models, controls, fixtures, and workflow checks.

EPIC-018 remains a separate post-#216 phase. No post-merge campaign or node proof ran during this check.

## Historical record

The sections below record earlier planning and scaffolding. Superseded artifact counts and runtime obligations do not define the current scope.

## Request

Prepare the checked-out branch from the ratification meeting. Review ToDos and epics before implementation. Use CbC and build on PRs #216, #390, and #430 through #433.

## Changes

- Added EPIC-017 with thirteen pre-merge tasks in `docs/ToDos.md`.
- Added blocked EPIC-018 with six tasks for the post-#216 formal-methods harness PR.
- Added the [branch plan](../plans/casper-ratified-soak-2026-09-16.md).
- Recorded all twelve decisions, exact source revisions, evidence profiles, and existing epic overlaps.
- Kept production code, workflow code, existing task claims, and unrelated epic statuses unchanged.

TASK-017-1 remains in progress pending plan review. No task has been marked complete.

## Baseline findings

The branch starts at `a2fe60c7255bf4ba035d41fb65b6d6f1c0f02632`. All six source PRs were open when inspected.

The local ratification commit is newer than the observed PR #390 remote head. The published meeting review remains the accessible decision reference.

The prerequisite order is `#430 -> #431 -> #432 -> #433`. No merge, cherry-pick, commit, push, or workflow dispatch was performed.

The review covered twelve active epic blocks and the completed-epic index. It found related work in EPIC-010, EPIC-012, EPIC-013, EPIC-015, and EPIC-016.

The parser's strict vocabulary rejects seven existing `review` tasks. Compatibility mode restores their counts. Those task states were not changed.

No `docs/handoffs/` directory exists. Existing work logs provide historical coordination context, not new authorization.

## CbC inspection

The repository does not carry `scripts/cbc.sh`. The shared CbC skill driver was used from the repository working directory.

`identify --scope epic EPIC-017 --json` found six mandatory artifacts in the initial candidate scope.

`discharge --scope epic EPIC-017 --strict --json` returned exit 4, with four missing records:

- `block-storage/src/rust/dag/block_dag_key_value_storage.rs`
- `block-storage/src/rust/dag/carrier_index.rs`
- `casper/src/rust/finality/floor.rs`
- `casper/src/rust/validate.rs`

The driver also reported an existing block-creator waiver and heartbeat discharge. Neither result discharges the new epic's claims.

The scope is preliminary. TASK-017-2 must expand it to the actual implementation files and audit each claim before code changes.

The initial probe found no CbC attribute for `estimator.rs`, `dag_operations.rs`, the soak driver, or the formal gate script.

The draft repair plan conflicts with ratified certificate and stale-recovery policy. The cross-view leader claim needs an explicit lane scope.

The driver status output reports multiple entries for `interpreter_util.rs`. Do not interpret that output as a new discharge.

## Tool availability

Java, the TLC jar, and Z3 are present. Rocq and Coq are not on the current PATH.

No prover, runtime test, or soak was run during planning. No claim was waived or discharged.

## Pre/post amendment

The maintainer requested a separate follow-on formal-methods harness PR after PR #216 merges.

EPIC-017 now owns the pre-merge models, oracles, baseline bindings, profiles, and baseline evidence. Its completion does not depend on PR #216 merging.

EPIC-018 starts only after the accepted pre-merge handoff and the actual PR #216 merge into `dev`. The proposed follow-on branch is `formal/soak-casper-post-cost-accounting`.

The post-merge tasks rebind claims and adapt the harness to actual merged APIs, fields, metrics, storage, and lifecycle behavior. They require new runs and evidence.

The handoff records genuine pending production obligations without waiving them. It cannot excuse an incomplete claim required by the pre-merge scope.

No follow-on branch or PR was created. This amendment changes planning documents only.

## Claim scaffolding amendment

The maintainer clarified that this branch must build a formally verifiable soak harness, following PR #433.

The generic scaffold command found no new default-scanner candidates. It did not create the claim-specific verification structure.

This amendment adds seven pending claim specifications and extends the existing carrier-index claim with phase ownership and typed-identity obligations.

The harness specification defines ten invariants, proposed finite bounds, negative controls, driver correspondences, and a RED/GREEN cycle checklist.

The scaffold tags thirteen additional existing artifact paths and the new formal area. Both epics now list the twenty-six claim-owned artifact paths.

Twenty-one new evidence records have pending status, source and claim digests, null verification timestamps, and explicit tier applicability.

Five existing evidence records remain unchanged. Their statuses cannot discharge the new claims without a property, revision, and assumption audit.

The harness uses TLC refutation and real-driver fixtures. Shell-driver construction is not applicable, as specified by PR #433.

Unbounded runtime claims retain pending Rocq construction and production-binding obligations. The post-merge phase still requires the actual #216 merge and new evidence.

No model, fixture, or workflow implementation was added. No prover, soak, or CbC discharge command was run for this amendment.

## Planning validation

- YAML parsing reports thirteen TASK-017 entries and six TASK-018 entries.
- Dependency checks confirm unique task identifiers, acyclic task dependencies, and explicit merge and handoff gates.
- Relative file links and all twelve decision rows pass the local checks.
- The deterministic STE Check passes for the new plan, work log, and changed ToDos sections.
- `git diff --check` passes.

These checks validate planning artifacts only. The strict CbC scope check remains non-passing, as recorded above.

## Scaffold validation

- All eight claim specifications parse and retain pending status.
- Both epic file lists cover the same twenty-six mandatory artifact paths.
- All ten proposed negative controls have unique knobs and fixtures, named properties, and expected TLC exit 12.
- All twenty-one new ledger records parse and retain pending status with null verification timestamps.
- Source and claim digests match the scaffold inputs.
- Five legacy evidence records remain byte-identical to the branch baseline.
- Claim/task references and local document links resolve.
- The deterministic STE Check passes for the new prose.
- `git diff --check` passes.

The JSONC plan also passes JSON parsing. Its language-server probe is inconclusive, so no clean LSP result is claimed.

These checks do not execute the harness, model, or prover. The next CbC run must inspect the explicit epic scope and all pending claim obligations.

## Modular evidence layout

Twenty Casper-owned records now live in `docs/casper/cbc-evidence/`. This set includes Casper runtime evidence, heartbeat evidence, and Casper soak-area evidence.

Shared storage, execution, protobuf, and harness records remain in `docs/cbc-evidence/`. Claim specifications remain in their existing directory.

Relative symlinks preserve the old paths for the shared driver's flat lookup and writes. They do not create independent evidence records.

The migration changes relative document links only. Artifact identities, embedded evidence, statuses, and verification timestamps remain unchanged.

The Casper documentation index describes directory ownership and the temporary compatibility links. New Casper records need the same link until the driver supports module routing.

## Next step

Resolve the recorded CbC gaps in the reduced scope. Define TASK-017-2 profile fixture expectations before implementing harness models and fixtures.

Keep deferred policies experimental. Retain both `phloLimit` and `phloPrice` in every protocol-7 acceptance contract.
