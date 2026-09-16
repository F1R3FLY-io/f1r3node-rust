# Casper Soak Harness Formal Area

**Status:** Scaffold only. No TLA+ module or configuration has been implemented or registered here.

The [harness claim](../../../docs/claims/casper-soak-harness.md) owns the specification. The [verification plan](./verification-plan.jsonc) lists proposed bounds and control names.

This area follows [PR #433](https://github.com/F1R3FLY-io/f1r3node-rust/blob/65f7f6daa832c0acb6fddf2b462db1b9d5461729/docs/cbc-verification-tiers.md). Harness construction is not applicable. Runtime semantic claims retain their separate Rocq obligations.

## Model correspondence

| Proposed action | Source boundary | Property | Fixture ID |
| --- | --- | --- | --- |
| PinRun | Driver initialization and workflow image selection | IdentityPinned | resume_changed_identity |
| Resume | State-file load and persist_soak_state | ResumePreservesHistory | resume_overwrites_iteration |
| RecordFailure | Driver failure accounting and summary | ProductFailureMonotone | product_failure_then_resource_stop |
| Classify | Summary and workflow verdict | PassRequiresEvidence | missing_artifact_cannot_pass |
| Stop | Driver deadline, stop markers, and cleanup | StopPreventsLaunch | terminal_marker_prevents_launch |
| Capture | snapshot_iteration_monitor_outputs | EvidenceBeforeCleanup | capture_before_cleanup |
| SelectProfile | Proposed profile validation boundary | PolicyIsolation | experiment_cannot_change_baseline |
| Observe | emit_iteration_metrics and iteration_finalization_latency | MissingIsUnknown | missing_finalization_samples |
| AdmitPostMerge | Proposed manifest validation boundary | PostMergeGate | open_candidate_is_not_merge |
| JudgeControl | Proposed expected-violation registry | ControlVerdictExact | timeout_is_not_counterexample |

Exact function bindings for proposed boundaries remain open under TASK-017-4. The table does not assert that those functions already exist.

## Configuration correspondence

All names below are reserved design items, not runnable configurations. The machine-readable plan contains their knobs and expected properties.

| Proposed configuration suffix | Expected result | Registration |
| --- | --- | --- |
| MC_CasperSoakHarness | TLC exit 0, all invariants clean | Pending positive list |
| identity_unsafe | Exit 12, IdentityPinned | Pending negative registry |
| resume_unsafe | Exit 12, ResumePreservesHistory | Pending negative registry |
| failure_unsafe | Exit 12, ProductFailureMonotone | Pending negative registry |
| evidence_unsafe | Exit 12, PassRequiresEvidence | Pending negative registry |
| stop_unsafe | Exit 12, StopPreventsLaunch | Pending negative registry |
| cleanup_unsafe | Exit 12, EvidenceBeforeCleanup | Pending negative registry |
| policy_unsafe | Exit 12, PolicyIsolation | Pending negative registry |
| samples_unsafe | Exit 12, MissingIsUnknown | Pending negative registry |
| merge_unsafe | Exit 12, PostMergeGate | Pending negative registry |
| control_unsafe | Exit 12, ControlVerdictExact | Pending negative registry |

The initial bounds are two candidates, two segments, two iterations per segment, one child, and three artifact slots.

No clean control may omit a required property merely to fit a time budget. Any reduced bounds need resource evidence and explicit reporting.

## Completion checklist

- [ ] State the safety properties, progress assumptions, and instance bounds.
- [ ] Implement each action and defect knob with source correspondence.
- [ ] Implement a clean configuration and one named negative control per defect class.
- [ ] Add production-driver fixtures that expose the corresponding defect and pass after repair.
- [ ] Register positive and negative controls separately, with exact verdict classification.
- [ ] Record tier applicability, logs, traces, tool versions, digests, and limitations.
- [ ] Require the checks in workflows and review the complete evidence package.

The specification and correspondence are drafted. The unchecked items require implementation and verification, not merely the presence of this README.

The [cycle checklist](../../../docs/tdd-plans/casper-soak-harness.md) keeps formal and fixture results pending until actual runs occur.
