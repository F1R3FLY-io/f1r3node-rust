# Casper Soak Harness Formal Area

**Status:** The bounded safety model and local control runner are implemented. Driver bindings, profile models, shared CI integration, and soaks remain pending.

The [harness claim](../../../docs/claims/casper-soak-harness.md) owns the specification. The [verification plan](./verification-plan.jsonc) records controls and bounds.

This area follows [PR #433](https://github.com/F1R3FLY-io/f1r3node-rust/blob/65f7f6daa832c0acb6fddf2b462db1b9d5461729/docs/cbc-verification-tiers.md). Construction is not applicable. Node correctness and Rocq proofs are outside this area.

## Model correspondence

| Model action | Intended harness boundary | Property | Driver binding |
| --- | --- | --- | --- |
| Init, Admit | Manifest initialization and post-merge admission | PostMergeGate | Pending |
| Resume | State load and persist_soak_state | IdentityPinned, ResumePreservesHistory | Pending |
| Launch, Stop | Workload launch, deadline, and resource stop | StopPreventsLaunch, ProductFailureMonotone | Pending |
| Finish | emit_iteration_metrics and finalization sample extraction | MissingIsUnknown | Pending |
| Experiment | Profile selection and baseline isolation | PolicyIsolation | Pending |
| JudgeControl | Local runner classify function | ControlVerdictExact | Runner unit tests only |
| Capture, Cleanup | snapshot_iteration_monitor_outputs and cleanup | EvidenceBeforeCleanup | Pending |
| Report | Summary and workflow verdict | PassRequiresEvidence | Pending |

The record fields `recorded` and `failureSeen` preserve specification history. They detect loss from the modeled persisted history and failure flag.

The model abstracts two candidates, two segments, four total completed iterations, and one active child. It does not impose a per-segment iteration limit.

Evidence completeness is a Boolean abstraction, not a filesystem model. The `Stop` action abstracts manual, resource, and deadline stops.

The model uses `pre` and `post` for the two PR #216 phases. A verified merge is an input assumption, not an ancestry proof.

Safety checks do not require fairness. This model makes no eventual-termination or unbounded correctness claim.

## Configuration correspondence

Every negative configuration enables one defect knob and checks `TypeOK` plus its named invariant. The clean configuration checks all ten invariants.

| Configuration suffix | Expected result |
| --- | --- |
| MC_CasperSoakHarness | Exit 0 and completed clean search |
| identity_unsafe | Exit 12, IdentityPinned |
| resume_unsafe | Exit 12, ResumePreservesHistory |
| failure_unsafe | Exit 12, ProductFailureMonotone |
| evidence_unsafe | Exit 12, PassRequiresEvidence |
| stop_unsafe | Exit 12, StopPreventsLaunch |
| cleanup_unsafe | Exit 12, EvidenceBeforeCleanup |
| policy_unsafe | Exit 12, PolicyIsolation |
| samples_unsafe | Exit 12, MissingIsUnknown |
| merge_unsafe | Exit 12, PostMergeGate |
| control_unsafe | Exit 12, ControlVerdictExact |

A negative control also requires the exact invariant name and a counterexample trace. Unexpected success, timeout, cancellation, and tool failure cannot pass.

## Local runner

```bash
python3 -m unittest discover -s scripts/ci/tests -p 'test_casper_soak_model_runner.py' -v
python3 scripts/ci/check_casper_soak_models.py --output-dir target/casper-soak-formal/new-run
```

The output directory must not already exist. Set `JAVA` or pass `--java` when the default Java launcher is unsuitable.

Set `TLA_TOOLS_JAR` or pass `--jar` to select the TLC jar. Each report retains its digest and observed TLC version.

The runner uses one worker, seed 1, a 512 MB Java heap, and a 120-second cap per configuration by default.

Logs and the report retain every outcome. Input changes during a run invalidate the result. Configuration constants and digests record the actual model bounds.

The runner is not yet called by shared CI. PRs #431 and #432 remain open prerequisites for that integration.

## Evidence and limits

The [local evidence package](../../../docs/casper/cbc-evidence/runs/casper-harness-controls-20260916-01/report.json) records the bounded run.

The clean model explored 43,424 distinct states. All ten negative controls produced their named violation with exit 12.

Runner unit tests exercise verdict parsing, configuration validation, missing tools, timeout, cancellation, input drift, and evidence-directory preservation.

Those tests do not exercise the live Bash driver. They cannot discharge the driver or profile claims.

## Profile models

The verification plan links seven profile claims with proposed properties, defect knobs, and fixture expectations. Their models and executable bindings remain pending.

## Completion checklist

- [x] State bounded safety properties and model limits.
- [x] Implement the lifecycle model and ten defect knobs.
- [x] Run the clean configuration and ten targeted negative controls.
- [ ] Bind each modeled action to the real driver with fault fixtures.
- [ ] Implement and verify profile generators, collectors, and classifiers.
- [ ] Integrate the checks into the prerequisite-aware workflow.
- [ ] Review the complete evidence package before full claim discharge.

The [cycle checklist](../../../docs/tdd-plans/casper-soak-harness.md) separates model results from pending driver fixtures.
