# Casper Soak Harness Formal Area

**Status:** The bounded lifecycle and seven profile bindings are accepted for their recorded pre-merge sources. Live adapter qualification and baseline soaks remain pending.

The [harness claim](../../../docs/claims/casper-soak-harness.md) owns the specification. The [verification plan](./verification-plan.jsonc) records controls and bounds.

This area follows [PR #433](https://github.com/F1R3FLY-io/f1r3node-rust/blob/65f7f6daa832c0acb6fddf2b462db1b9d5461729/docs/cbc-verification-tiers.md). Construction is not applicable. Node correctness and Rocq proofs are outside this area.

## Documentation contract

`CLAIM-CASPER-SOAK-FORMAL-AREA-DOCS` covers this README and the top-level verification plan only. It is separate from the eight executable harness claims.

1. Status descriptions must match the source-bound acceptance records without asserting a passing soak or qualified live adapter.
2. Metadata updates must preserve model inputs, controls, bounds, assumptions, registration policy, and executable source identities.
3. Each profile reference must identify its current claim, verification plan, and accepted report.
4. Evidence must preserve previous document and ledger identities, unsuccessful outcomes, and pending post-merge obligations.

The documentation check verifies these requirements. Fresh lifecycle controls check the executable plan, not node correctness.

## Model correspondence

| Model action | Intended harness boundary | Property | Driver binding |
| --- | --- | --- | --- |
| Init, Admit | Manifest initialization and post-merge admission | PostMergeGate | Accepted bounded fixtures |
| Resume | State load and persist_soak_state | IdentityPinned, ResumePreservesHistory | Accepted bounded fixtures |
| Launch, Stop | Workload launch, deadline, and resource stop | StopPreventsLaunch, ProductFailureMonotone | Accepted bounded fixtures |
| Finish | emit_iteration_metrics and finalization sample extraction | MissingIsUnknown | Accepted bounded fixtures |
| Experiment | Profile selection and baseline isolation | PolicyIsolation | Accepted bounded fixtures |
| JudgeControl | Local runner classify function | ControlVerdictExact | Accepted exact-verdict controls |
| Capture, Cleanup | snapshot_iteration_monitor_outputs and cleanup | EvidenceBeforeCleanup | Accepted bounded fixtures |
| Report | Summary and workflow verdict | PassRequiresEvidence | Accepted bounded fixtures |

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
cargo build --locked -p casper-soak
cargo test --locked -p casper-soak --test models
bash scripts/ci/check-casper-soak-models.sh --output-dir target/casper-soak-formal/new-run
```

The output directory must not already exist. Set `JAVA` or pass `--java` when the default Java launcher is unsuitable.

Set `TLA_TOOLS_JAR` or pass `--jar` to select the TLC jar. Each report retains its digest and observed TLC version.

The runner uses one worker, seed 1, a 512 MB Java heap, and a 120-second cap per configuration by default.

Logs and the report retain every outcome. Input changes during a run invalidate the result. Configuration constants and digests record the actual model bounds.

The workflow runs model checks before the isolated driver fixture job. Local results do not establish hosted-CI completion.

New implementation code uses Rust and Bash. Historical evidence retains the retired Python source names and hashes.

## Evidence and limits

The [initial evidence package](../../../docs/casper/cbc-evidence/runs/casper-harness-controls-20260916-01/report.json) retains the historical bounded run.

The [inventory renewal](../../../docs/casper/cbc-evidence/runs/casper-binding-inventory-renewal-20260919-01/report.json) retains the earlier lifecycle binding and its executable evidence.

The [hosted workflow renewal](../../../docs/casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/report.json) binds the current lifecycle sources after the formal-gate workflow change.

The [earlier formal-area review](../../../docs/casper/cbc-evidence/runs/casper-formal-area-records-20260919-01/report.json) preserves its source and plan identities.

The [renewed documentation review](../../../docs/casper/cbc-evidence/runs/casper-formal-gate-documentation-20260919-01/report.json) checks current metadata and unchanged executable plan inputs. It does not expand the eight claim inventories.

The independent `CLAIM-SOAK-GATE-001` remains pending. This documentation does not establish required-check enforcement or authorize protection-rule changes.

The clean model explored 43,424 distinct states. All ten negative controls produced their named violation with exit 12.

Rust tests exercise verdict parsing, configuration validation, missing tools, timeout, input drift, and evidence-directory preservation.

Model-runner tests alone do not exercise the live Bash driver. The separate accepted lifecycle fixtures cover 48 registered cases and 91 driver invocations.

Production-driver process fixtures run in isolated Linux containers. Accepted profile fixtures use controlled transcripts, not qualified live node observations.

B44, Linux host-control assumptions, cooperating-writer limits, and missing historical evidence remain recorded in the [acceptance log](../../../docs/work-logs/casper-driver-rebind-acceptance.md).

## Profile models

The verification plan links seven implemented profile plans and their accepted evidence. Each plan retains its properties, defect knobs, finite bounds, and fixture expectations.

The [claim index](../../../docs/claims/casper-soak-harness.md) separates their bounded discharges from pending soaks. Construction remains not applicable.

Changed interfaces require a new binding review. The actual PR #216 merge and an accepted handoff still gate post-merge execution.

## Completion checklist

- [x] State bounded safety properties and model limits.
- [x] Implement the lifecycle model and ten defect knobs.
- [x] Run the clean configuration and ten targeted negative controls.
- [x] Bind each modeled action to the real driver with bounded fault fixtures.
- [x] Implement and verify profile generators, collectors, and classifiers with controlled fixtures.
- [x] Integrate the checks into the prerequisite-aware workflow.
- [x] Accept the source-specific bounded lifecycle and profile evidence.
- [ ] Qualify live adapters and review baseline outcomes under TASK-017-12.
- [ ] Close the full changed scope and accept the TASK-017-13 handoff.
- [ ] Reverify changed interfaces after the actual PR #216 merge.

The [cycle checklist](../../../docs/tdd-plans/casper-soak-harness.md) retains implementation history. Current source-bound ledgers control the accepted binding status.
