---
task: TASK-017-1
branch: formal/soak-casper-consensus
claimed_by: pi-casper-ratification-planning
claimed_at: 2026-09-16T20:29:37Z
handoff_status: paused
next_steps:
  - Review the EPIC-017 pre-merge and EPIC-018 post-merge plan with the maintainer.
  - Define claim-specific reference oracles under TASK-017-2.
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

## Planning validation

- YAML parsing reports thirteen TASK-017 entries and six TASK-018 entries.
- Dependency checks confirm unique task identifiers, acyclic task dependencies, and explicit merge and handoff gates.
- Relative file links and all twelve decision rows pass the local checks.
- The deterministic STE Check passes for the new plan, work log, and changed ToDos sections.
- `git diff --check` passes.

These checks validate planning artifacts only. The strict CbC scope check remains non-passing, as recorded above.

## Next step

Review the two-phase plan before implementation. Then start TASK-017-2 with claim statements, negative controls, source bridges, phase ownership, and a precise artifact scope.

Keep deferred policies experimental. Retain both `phloLimit` and `phloPrice` in every protocol-7 acceptance contract.
