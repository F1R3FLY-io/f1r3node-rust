---
task: TASK-017-1
branch: formal/soak-casper-consensus
claimed_by: pi-casper-ratification-planning
claimed_at: 2026-09-16T20:29:37Z
handoff_status: paused
next_steps:
  - Run the requested CbC review against the scaffolded epic scope.
  - Complete source bindings and claim reconciliation under TASK-017-2.
  - Resolve prerequisite integration under TASK-017-3 with separate Git consent.
---

# Ratified Casper Soak Preparation

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

Run the requested CbC review next. Then complete TASK-017-2 source bindings and legacy-claim reconciliation before implementing the TASK-017-4 model and fixtures.

Keep deferred policies experimental. Retain both `phloLimit` and `phloPrice` in every protocol-7 acceptance contract.
