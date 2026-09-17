# Casper Soak Harness Verification Cycles

**Status:** Bounded model controls pass. Real-driver fixture cycles, profile verification, and soaks remain pending.

**Owner:** TASK-017-4. Post-merge rebinding belongs to TASK-018-2.

The [claim](../claims/casper-soak-harness.md) defines the contract. The [formal plan](../../formal/tlaplus/casper_soak/verification-plan.jsonc) names the controls and bounds.

## Behavior checklist

| Cycle | Behavior | Formal property | State |
| --- | --- | --- | --- |
| H01 | Reject changed candidate identity on resume. | IdentityPinned | Pending |
| H02 | Preserve iterations and failure history across segments. | ResumePreservesHistory | Pending |
| H03 | Preserve product failures after resource termination. | ProductFailureMonotone | Pending |
| H04 | Refuse passing reports with incomplete evidence. | PassRequiresEvidence | Pending |
| H05 | Prevent workload launch after a terminal condition. | StopPreventsLaunch | Pending |
| H06 | Capture durable evidence before cleanup. | EvidenceBeforeCleanup | Pending |
| H07 | Keep deferred experiments outside baseline authority. | PolicyIsolation | Pending |
| H08 | Keep missing measurements distinct from zero values. | MissingIsUnknown | Pending |
| H09 | Require the actual merge for a post-#216 profile. | PostMergeGate | Pending |
| H10 | Accept only the named negative-control violation. | ControlVerdictExact | Pending |

## First bounded model result

All H01 through H10 negative configurations produced their named invariant violation with exit 12. The clean configuration passed with 43,424 distinct states.

The local runner's twelve unit tests passed. They check the verifier runner, not the live driver behavior required by this checklist.

No RED/GREEN driver cycle is marked complete. The [implementation log](../work-logs/task-017-4-harness-model-2026-09-16.md) links the retained results and limits.

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
