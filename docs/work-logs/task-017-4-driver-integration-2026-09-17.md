---
task: TASK-017-4
claimed_by: pi-casper-harness
handoff_status: ready
dependency_approval_date: 2026-09-17
construction: not-applicable
---

# TASK-017-4 Integration Continuation

The user requested completion of TASK-017-4. The starting revision is `c9faa7d2f4c1cf4564b544a6cf3aa4c95f869d26`, with a clean working tree.

## Execution Plan

- Register the Casper clean configuration and ten controls in the shared gate.
- Verify shared routing, missing-control detection, finite bounds, and exact result classification.
- Replace executable restart-state loading with validated data loading.
- Run real-driver restart fixtures and prerequisite regression tests.
- Review manifest, correlation, classification, and publication obligations against available profile interfaces.
- Run the task completion checks without waiving missing obligations.

## Dependencies Before Approval

TASK-017-2 had a complete contract but remained open because the completion helper rejects TASK identifiers. TASK-017-3 awaited executable workload configurations.

The shared claim spans TASK-017-4, TASK-017-12, and TASK-017-13. Passing a local model cannot discharge the future soak obligations.

Profile dispatch and observation adapters remain unimplemented under TASK-017-5 through TASK-017-11. Those tasks previously waited for TASK-017-4.

No node campaign, external repin, Git publication, or runtime repair belongs to this continuation.

## Verified Changes

The shared gate now registers the Casper clean configuration and all ten controls. Casper uses one worker, seed 1, a 512 MB heap cap, and a two-minute configuration cap in both tiers.

RED reproduced the missing registration. GREEN passed 71 negative acceptance controls, 28 basic negative rejections, 17 ambiguity rejections, and two additional positive rejections.

The fixture checks routing and rejects unregistered Casper unsafe configurations. Actual TLC passed 14 positives and 71 negatives in 73.982 seconds, including container setup.

The driver no longer executes `.soak-state` as shell code. It accepts bounded, whitelisted numeric fields and rejects executable input, duplicate fields, unknown fields, missing fields, and overflow.

RED reproduced shell execution from saved state. GREEN passed six restart-state cases, the three existing driver scenarios, and all 42 isolated disk scenarios.

All twelve local runner tests passed. Active language-server checks passed for all four changed shell files.

The [evidence report](../casper/cbc-evidence/runs/casper-driver-integration-20260917-01/report.json) identifies 1,692 retained records and 228 source digests. The bundle includes raw transcripts, fixture sources, tool versions, seeds, state counts, and immutable container image identities.

The final registration fixture adds the unsafe-configuration check after the TLC run. Its source digest therefore differs from the earlier fixture snapshot, not from the tested gate.

The strict CbC gate returned exit 4. The completion helper rejected TASK-017-4 with exit 2 before changing its status.

## Acceptance Review

| Requirement | Result |
| --- | --- |
| Shared clean and negative model registration | Implemented and verified locally. |
| Exact control verdicts and finite bounds | Verified for the registered configurations and fixture cases. |
| Saved state cannot execute shell commands | Verified through the real driver. |
| Full immutable manifest validation | Not implemented. |
| Profile request dispatch and correlated observation records | Not implemented. Required adapters belong to the profile tasks. |
| Complete inventory and conformance publication | Not implemented. Legacy summaries do not establish conformance. |
| All H01 through H10 real-driver bindings | Incomplete. Numeric state validation does not establish complete identity or history binding. |
| Workflow claim-discharge integration | Incomplete. The shared full claim still has pending obligations. |
| Runtime proofs and construction | Not applicable. |

## Approved Dependency Decision

On 2026-09-17, the user approved profile implementation alongside unfinished common-driver bindings. The approval resolves the sequencing decision, not the missing implementation.

TASK-017-4 and TASK-017-5 through TASK-017-11 may proceed against the completed contract and applied prerequisites. Their task-closure dependencies no longer prevent implementation.

TASK-017-3 retains prerequisite integration and initial identity recording. TASK-017-12 now owns final executable workload pinning and qualification before dispatch.

TASK-017-12 explicitly waits for TASK-017-2 through TASK-017-11. Required verification, task closure, capability checks, and approved resource budgets remain dispatch gates.

The candidate matrix stays unchanged and non-dispatchable. No task or claim is marked complete. The completion helper still needs TASK identifier support.

## Planned Stack Integration

The user plans to merge `docs/consensus-neutral-execution` next to add this branch after PR #433. No merge, PR retarget, commit, or push occurred during this update.

The actual parent and merge revisions remain unset. After integration, changed shared sources need fresh fixture and model results before their evidence can support current claims.

The [branch plan](../plans/casper-ratified-soak-2026-09-16.md#planned-stack-integration) records the stack checks. The [task records](../ToDos.md#epic-017-ratified-casper-conformance-and-soak-evidence) retain the incomplete implementation obligations.

## Dependency Update Checks

The YAML and dependency checks passed. The revised graph has no cycle. Task statuses, profile acceptance criteria, inventories, and unrelated epics remain unchanged.

Protected source and evidence hashes still match. The candidate matrix and external dependency pin remain unchanged. Relative links, STE Check, and whitespace checks passed.

The strict CbC gate still returned exit 4. This planning update neither discharges claims nor replaces the retained implementation results.

## Stack Verification After Merge

Merge `0f1ccdf38f9ab3b056e7601b93961cb56c0a51e9` contains the selected PR #433 revision. PR #436 now targets `docs/consensus-neutral-execution`.

The merged tree equals first parent `f72337662039befc77162328a997556719a7062e`. All 228 source hashes and 1,692 earlier evidence records still match.

The shared-gate fixture passed in 18.151 seconds. The real-driver fixture passed its three existing scenarios and six restart-state cases in 14.068 seconds.

Both measurements include container setup. The containers used the previously pinned image, no network, no host mounts, and no Docker socket.

Twelve runner tests, shell syntax, and Linux-targeted Pyright passed. The strict CbC gate returned exit 4 because full claims remain pending.

The [stack report](../casper/cbc-evidence/runs/casper-stack-integration-20260917-01/report.json) retains 132 records and the source-manifest reference. Its SHA-256 is `76852e6cb027dfb81583bade9fdf710cf26a964f362c705c4fe64cf83d006358`.

Actual TLC and the 42-case disk suite were not rerun. Their earlier evidence remains applicable to unchanged source bytes, without extending its coverage.

The confirmed stack-parent diff has 813 files, 94,281 insertions, and 802 deletions. The current-dev merge-base comparison has 980 files, 104,443 insertions, and 513 deletions.

TASK-017-14 still requires diff reduction. This check removed no files and changed no claim or task status.

Current `dev` is `bc23c8667ebef0f3fb7c3310caf85ce106df25fa`. The blocked candidate matrix still selects `a2fe60c7255bf4ba035d41fb65b6d6f1c0f02632` and requires qualification review before dispatch.

No candidate repin, node run, commit, merge, push, or PR change occurred during this verification. The next implementation cycle is H01 manifest/resume identity binding alongside approved profile work.

## Diagnostic and Language Review

Fresh Linux-targeted Pyright passed for the unchanged native fixtures. Their host API findings and previously reviewed exception-handler findings remain false positives.

The Boolean identity check deliberately requires JSON false. No native fixture changed to silence those findings.

The deterministic STE Check does not replace human STE Review.
