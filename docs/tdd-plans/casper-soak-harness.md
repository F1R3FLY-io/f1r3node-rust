# Casper Soak Harness Verification Cycles

**Status:** The user accepted bounded H01–H10 harness binding twice, most recently for the repaired driver on 2026-09-18. The harness source changed again after that acceptance, and the user accepted the refreshed binding on 2026-09-19. CLAIM-CASPER-SOAK-001 is discharged for that scope. Profile verification and node soaks remain pending.

The [acceptance record](../work-logs/task-017-4-acceptance.md) supersedes the historical pending labels below. It preserves all stated containment limits.

**Owner:** TASK-017-4. Post-merge rebinding belongs to TASK-018-2.

The [claim](../claims/casper-soak-harness.md) defines the contract. The [formal plan](../../formal/tlaplus/casper_soak/verification-plan.jsonc) names the controls and bounds.

## Behavior checklist

| Cycle | Behavior | Formal property | State |
| --- | --- | --- | --- |
| H01 | Reject changed candidate identity on resume. | IdentityPinned | Bounded binding is accepted. |
| H02 | Preserve iterations and failure history across segments. | ResumePreservesHistory | Bounded binding is accepted. |
| H03 | Preserve product failures after resource termination. | ProductFailureMonotone | Bounded binding is accepted. |
| H04 | Refuse passing reports with incomplete evidence. | PassRequiresEvidence | Bounded binding is accepted. |
| H05 | Prevent workload launch after a terminal condition. | StopPreventsLaunch | Bounded binding is accepted. The lock protocol remains required. |
| H06 | Capture durable evidence before cleanup. | EvidenceBeforeCleanup | Bounded binding is accepted. Containment limits remain. |
| H07 | Keep deferred experiments outside baseline authority. | PolicyIsolation | Bounded binding is accepted. |
| H08 | Keep missing measurements distinct from zero values. | MissingIsUnknown | Bounded binding is accepted. |
| H09 | Require the actual merge for a post-#216 profile. | PostMergeGate | Bounded gate binding is accepted. No merged-node run occurred. |
| H10 | Accept only the named negative-control violation. | ControlVerdictExact | Bounded binding is accepted. |

## First bounded model result

All H01 through H10 negative configurations produced their named invariant violation with exit 12. The clean configuration passed with 43,424 distinct states.

The local runner's twelve unit tests passed. They check the verifier runner, not the live driver behavior required by this checklist.

No RED/GREEN driver cycle is marked complete. The [implementation log](../work-logs/task-017-4-harness-model-2026-09-16.md) links the retained results and limits.

## H10 trace validation

The shared gate now requires a first state after the counterexample header. The RED fixture reproduced acceptance of a truncated trace.

The GREEN fixture passed 61 acceptance controls and 21 rejection cases across three registered areas. The bounded shared tier passed 13 positives and 61 negatives.

The [cycle log](../work-logs/task-017-4-control-trace-2026-09-17.md) links the source digests and transcripts. This partial binding does not complete H10 or the harness claim.

## H10 positive-search validation

The shared gate now requires exit zero and the exact completed-search marker for positive configurations. RED reproduced acceptance of an incomplete positive search.

GREEN rejected that result through the pull-request workflow command. The shared bounded tier passed all 13 positives and 61 negatives.

The [cycle log](../work-logs/task-017-4-positive-search-2026-09-17.md) records the evidence. The following cycle covers contradictory markers and multiple invariant violations.

## H10 exact-result classification

The shared gate rejects contradictory success/error output, additional invariant violations, duplicate expected violations, and additional verifier errors.

RED reproduced acceptance of contradictory positive output. GREEN rejected all 13 new ambiguity cases through the pull-request workflow command.

The bounded shared tier passed 13 positives and 61 negatives. Existing fixture checks, Casper model controls, and twelve runner tests also passed.

The [cycle log](../work-logs/task-017-4-exact-result-2026-09-17.md) links the retained results. That cycle left shared registration and driver/evidence bindings pending.

## Shared registration and restart-state loading

The shared gate now runs the Casper clean configuration and ten controls in both tiers. Casper retains one worker, seed 1, and a two-minute configuration cap.

The bounded shared tier passed 14 positives and 71 negatives. The fixture also rejects an unregistered Casper unsafe configuration.

The real driver now reads numeric checkpoint data without executing shell input. Six restart-state cases and all 42 isolated disk scenarios passed.

These checks are partial prerequisites for H01 and H02, not full identity or history bindings. Manifest validation, correlated profile observations, and conformance publication remain unimplemented.

The [continuation log](../work-logs/task-017-4-driver-integration-2026-09-17.md) records the results and the approved dependency change. Profile implementation may proceed alongside TASK-017-4, but all incomplete verification cycles remain pending.

## H01 manifest identity slice

The driver rejects changed manifest bytes before saved-state writes. Matching identities resume across an expired window without workload launch.

The final fixture passes two positive invocations and 45 rejection invocations. Fresh model, driver, disk, and runner checks also pass.

The [resume report](../casper/cbc-evidence/runs/casper-manifest-resume-20260917-01/report.json) records the limits and packed evidence. Capability qualification, observation identity, and individual successful invocation transcripts remain pending.

The helper and fixture are mandatory-scope additions. No task, claim, or full verification cycle is marked complete.

## Binding inventory and terminal entry

The [gate continuation](../work-logs/task-017-4-binding-inventory-gate.md) records three additional RED/GREEN checks.

- The inventory gate rejects unrelated replacements for required cases.
- The execution entry point rejects a terminal marker set after Bash admission.
- The wrapper cannot publish success after an interrupted build.

Current host tests, three focused Linux tests, and eleven TLC controls pass. Two full-suite attempts remain incomplete or invalid.

The complete current fixture suite, interrupted-container capture, and semantic binding review remain pending. No H01–H10 cycle receives full discharge from these partial checks.

## Current terminal and claim checks

The [final-check log](../work-logs/task-017-4-final-checks.md) maps H01–H10 to the current implementation and records its limits.

Three more RED/GREEN cycles pass:

- A terminal-transition lock excludes concurrent workload admission.
- An active stop drains the current iteration without creating another iteration.
- A changed ledger cannot reuse evidence for different source bytes.

The current isolated suite passes 22 Rust tests and all 91 invocations across 48 registered cases. Claim auditing remains distinct from claim discharge.

The current shared tier passes 14 positive configurations and 71 negative controls. The legacy driver suite and all 42 disk scenarios also pass.

Interrupted-capture ordering now has a controlled Docker-boundary fixture. It does not verify the Docker daemon or resolve B44.

The completion adapter accepts TASK-017-4 and checks its claim independently of structural links. Pending claim evidence still blocks completion.

Earlier incomplete runs and invalid reports remain historical evidence. Their results do not describe the current source snapshot.

## Per-cycle record template

```yaml
cycle: null
claim: CLAIM-CASPER-SOAK-001
status: pending
phase: pre_pr216_merge
source_revision: null
model_digest: null
fixture_digest: null
red_exit: null
formal_red_exit: null
green_exit: null
formal_green_exit: null
construction: not-applicable
construction_assumptions: null
expected_violation: null
observed_violation: null
tool_versions: null
artifact_digests: []
```

A null field is not a successful run. RED requires the expected defect, not an import error, timeout, or unrelated failure.

Each implemented cycle retains both the clean configuration and its negative control. Fixture tests must invoke the production driver rather than a duplicate implementation.

A passing fixture proves only the stated binding behavior. It does not prove an unbounded Casper semantic claim.

## Profile verification checklist

TASK-017-5 through TASK-017-11 own profile generation, fault acknowledgments, collectors, and classifiers. They do not own node-correctness proofs.

- [ ] Bind each profile's clean transcript and three defect controls to its real harness implementation.
- [ ] Check missing observations, mismatched identities, and planted product failures without modifying node behavior.
- [ ] Keep fixture outcomes separate from real-node soak outcomes.

Record `construction: not-applicable` for these infrastructure claims. No Rocq promotion belongs to either epic.

For post-merge evidence, create new profile cycle records with the actual #216 merge and updated harness digests. Keep the pre-merge records intact.
