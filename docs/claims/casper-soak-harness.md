# Casper Soak Harness Verification Contract

```yaml
claim_id: CLAIM-CASPER-SOAK-001
status: pending
adapter: embedded
pre_merge_tasks: [TASK-017-2, TASK-017-4, TASK-017-12, TASK-017-13]
post_merge_tasks: [TASK-018-1, TASK-018-2, TASK-018-5, TASK-018-6]
artifacts:
  - scripts/run-merge-recovery-soak.sh
  - scripts/bench/test-run-merge-recovery-soak.sh
  - scripts/bench/write-soak-summary.sh
  - scripts/ci/check-tla-invariants.sh
  - .github/workflows/merge-recovery-soak.yml
mechanization_plan: formal/tlaplus/casper_soak/verification-plan.jsonc
refutation: pending
construction: not-applicable
construction_assumptions: null
binding: pending
soak: pending
```

## Authority and boundary

This claim covers the harness, not the correctness of Casper consensus. It follows the [PR #433 tiers](https://github.com/F1R3FLY-io/f1r3node-rust/blob/65f7f6daa832c0acb6fddf2b462db1b9d5461729/docs/cbc-verification-tiers.md).

TLC checks the finite driver model. Shell and container fixtures bind the driver to that model. A Rocq theorem is not required for this shell-driver claim.

The [branch plan](../plans/casper-ratified-soak-2026-09-16.md) controls phase ownership. The following claims verify profile generation, observation, and classification only:

| Claim | Profile boundary | Pre-merge task | Post-merge task |
| --- | --- | --- | --- |
| [002](./casper-soak-authority-finality.md) | Committee, fork choice, finality | TASK-017-5 | TASK-018-3 |
| [003](./casper-soak-publication.md) | Publication and terminal eviction | TASK-017-6 | TASK-018-3 |
| [004](./casper-soak-recovery.md) | Heartbeat and deploy custody | TASK-017-7 | TASK-018-3, TASK-018-5 |
| [005](./casper-soak-merge-accounting.md) | Merge and accounting | TASK-017-8 | TASK-018-4 |
| [006](./casper-soak-slashing.md) | Slash authorization | TASK-017-9 | TASK-018-3 |
| [007](./casper-soak-version-phlo.md) | Version and signed Phlo fields | TASK-017-11 | TASK-018-4 |
| [008](./casper-soak-carrier-index.md) | Carrier comparison inputs and telemetry | TASK-017-10 | TASK-018-4 |

Each profile needs executable fixtures against the actual generator, collector, and classifier. A correct collector can faithfully report a consensus failure.

Node correctness, Rust changes, and Rocq proofs are outside both epics. Existing node claims, including CLAIM-FINALITY-002, remain external and unchanged.

## Inputs, state, and outputs

Inputs are the approved matrix, immutable candidate identities, workload seeds, fault schedule, resource limits, model verdicts, and per-iteration observations.

The model state contains the phase, run identity, segment, iteration, child state, resource-stop flag, product-failure set, artifact set, and terminal outcome.

Outputs are an immutable manifest, event trace, per-iteration measurements, model logs, fixture results, and final report with artifact digests.

The finite exploration bounds and proposed controls are in the [verification plan](../../formal/tlaplus/casper_soak/verification-plan.jsonc). They are not observed test results.

## Required invariants

1. `IdentityPinned`: Every observation belongs to the selected node, image, harness, configuration, seed, and run identity.
2. `ResumePreservesHistory`: Resume cannot overwrite an earlier iteration or decrease accumulated failure information.
3. `ProductFailureMonotone`: Resource termination cannot erase an observed product failure.
4. `PassRequiresEvidence`: Passing requires completed required profiles, valid evidence, successful conformance, and no unresolved product failure.
5. `StopPreventsLaunch`: A terminal marker, exhausted deadline, or resource stop prevents another workload launch.
6. `EvidenceBeforeCleanup`: Cleanup cannot delete required artifacts before durable capture and digest validation.
7. `PolicyIsolation`: Experimental policy controls cannot alter the production baseline or authorize deployment.
8. `MissingIsUnknown`: Missing finalization samples or work counters cannot become zero latency, zero work, or a passing verdict.
9. `PostMergeGate`: A post-merge run requires the actual #216 merge revision within the selected `dev` history.
10. `ControlVerdictExact`: A negative control passes only with TLC exit 12 and its named invariant violation.

Safety checks require no fairness assumption. Termination requires eventual child exit or enforced termination, available artifact storage, and fair scheduling of enabled cleanup actions.

The harness cannot guarantee host survival after external power loss. Such a run has incomplete evidence and cannot pass.

## Binding contract

| Model action | Existing implementation boundary | Required fixture |
| --- | --- | --- |
| PinRun | Driver environment initialization and workflow image selection | Reject changed image, node, or harness identity on resume. |
| Resume | State-file load and `persist_soak_state` | Preserve segment history and iteration numbering after restart. |
| Observe | `emit_iteration_metrics` and `iteration_finalization_latency` | Separate missing samples, duplicate log copies, and completed finalization. |
| Stop | Deadline handling, resource guards, and `cleanup_soak_processes` | Stop children and prevent new launch after each terminal cause. |
| Capture | `snapshot_iteration_monitor_outputs` and summary generation | Preserve logs and product failures through resource termination. |
| Publish | Workflow artifact upload and summary generation | Refuse missing, mismatched, or incomplete evidence. |

The driver currently consumes an external system-integration harness. Each run must pin that repository and audit its interface before claiming binding coverage.

Fixtures must execute the real driver with controlled processes and storage responses. A model-only simulation does not satisfy the binding tier.

## Evidence package

Each cycle needs the following fields. Null values mean pending, not zero assumptions or successful execution.

```yaml
claim_ids: [CLAIM-CASPER-SOAK-001]
phase: pre_pr216_merge
node_revision: null
harness_revision: null
image_digest: null
configuration_digest: null
model_digest: null
fixture_digest: null
seed: null
run_id: null
bounds: null
assumptions: null
tool_versions: null
red_exit: null
formal_red_exit: null
green_exit: null
formal_green_exit: null
construction: not-applicable
construction_assumptions: null
artifact_digests: []
product_failures: []
terminal_outcome: pending
```

A failing product observation and an infrastructure termination remain separate fields. Timeouts, cancellation, missing tools, and unexpected control success are non-passing outcomes.

## Implementation checklist

- [ ] TASK-017-4: Implement the finite model and one defect knob for each registered control.
- [ ] TASK-017-4: Register clean and expected-violation configurations separately after the files exist.
- [ ] TASK-017-4: Bind each action to the real driver with deterministic fault fixtures.
- [ ] TASK-017-4: Validate manifests and refuse missing required evidence in the workflow.
- [ ] TASK-017-12: Run approved baseline profiles and retain all terminal outcomes.
- [ ] TASK-017-13: Check every linked claim and artifact, not just one artifact-level status.
- [ ] TASK-018-2: Rebind changed interfaces after the actual #216 merge.
- [ ] TASK-018-5: Produce new merged-runtime evidence and compare compatible profiles.

## Current gaps

No new model, fixture, or workflow check has been implemented by this scaffold. Refutation, executable fixtures, and soak observations remain pending. Construction is not applicable.

PRs #431 and #432 supply prerequisite containment and disk models. This claim must compose with them, not replace or duplicate them.

The shared CbC ledger stores one status per artifact. Existing waivers and old discharges cannot discharge these new claims.

TASK-017-13 must check claim IDs, source digests, tier applicability, and phase evidence in addition to the generic artifact gate.
