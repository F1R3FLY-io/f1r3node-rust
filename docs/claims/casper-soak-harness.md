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
  - scripts/casper-soak/src/manifest.rs
  - scripts/casper-soak/tests/manifest.rs
  - scripts/bench/write-soak-summary.sh
  - scripts/ci/check-tla-invariants.sh
  - .github/workflows/merge-recovery-soak.yml
  - scripts/ci/check-casper-soak-models.sh
  - scripts/ci/check-casper-soak-bindings.sh
  - scripts/casper-soak/Cargo.toml
  - scripts/casper-soak/src/lib.rs
  - scripts/casper-soak/src/main.rs
  - scripts/casper-soak/src/host_control.rs
  - scripts/bench/test-soak-disk-admission.sh
  - scripts/casper-soak/src/models.rs
  - scripts/casper-soak/src/runtime.rs
  - scripts/casper-soak/tests/models.rs
  - scripts/casper-soak/tests/driver.rs
  - scripts/casper-soak/src/bin/check-casper-bindings.rs
  - scripts/casper-soak/tests/bindings.rs
  - scripts/casper-soak/tests/interruption.rs
  - scripts/casper-soak/src/bin/check-casper-claims.rs
  - scripts/casper-soak/tests/claims.rs
  - scripts/casper-soak/task-complete.sh
  - scripts/bench/casper-soak.sh
  - scripts/bench/fixtures/casper-lifecycle-executor.sh
  - .github/workflows/slashing-tests.yml
  - formal/tlaplus/casper_soak/CasperSoakHarness.tla
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_identity_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_resume_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_failure_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_evidence_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_stop_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_cleanup_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_policy_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_samples_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_merge_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_control_unsafe.cfg
mechanization_plan: formal/tlaplus/casper_soak/verification-plan.jsonc
refutation: bounded-safety-pass
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

Node correctness, node runtime changes, and Rocq proofs are outside both epics. New harness code uses Rust and Bash. Existing node claims, including CLAIM-FINALITY-002, remain external and unchanged.

## Inputs, state, and outputs

The [version-1 interface contract](../casper/design/soak-interface-contract.md) specifies field types, fault receipts, fixture IDs, source boundaries, and scenario blockers.

It records the audited external harness revision and distinguishes existing primitives from unimplemented profile adapters.

Inputs are the approved matrix, immutable candidate identities, workload seeds, fault schedule, resource limits, model verdicts, and per-iteration observations.

Required driver state includes phase, run identity, segment, iteration, child state, stop cause, product failures, artifact inventory, and terminal outcome.

The finite model abstracts that state. It does not encode complete manifests, artifact inventories, or specific resource-stop causes.

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

| Model action | Implementation boundary | Required fixture family |
| --- | --- | --- |
| Init, Admit, Resume | DR-START and DR-STATE | Changed identity, history preservation, and actual post-merge gate |
| Launch, Stop | DR-LAUNCH and DR-STOP | Terminal launch refusal and failure retention after resource stop |
| Finish | DR-OBSERVE | Missing samples, observed zero, and duplicate copies |
| Capture, Cleanup | DR-CAPTURE and DR-STOP | Verified evidence capture before artifact deletion |
| Experiment | DR-START and DR-LAUNCH | Isolated policy identity and unchanged baseline configuration |
| JudgeControl | Local runner `classify`, then DR-PUBLISH | Exact negative-control verdict and trace |
| Report | DR-SUMMARY and DR-PUBLISH | Complete required evidence and non-passing incomplete outcomes |

The driver currently consumes the external system-integration harness at `b3d14b27e3c6276b1eb4ab9ccef04e02b0c4e283`.

The interface contract records source-audited primitives at that pin. Each run must qualify its exact adapter and candidate before claiming binding coverage.

Fixtures must execute the real driver with controlled processes and storage responses. A model-only simulation does not satisfy the binding tier.

The inventory gate requires all 48 registered case identities and all 91 invocation records. Each invocation must match its registered exit and command.

Aggregate counts cannot replace case coverage. Inventory acceptance does not discharge this claim or establish semantic binding coverage.

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

- [x] TASK-017-4: Implement the finite model and ten defect knobs.
- [x] TASK-017-4: Register clean and expected-violation configurations in the local runner.
- [x] TASK-017-4: Integrate those controls into the reviewed shared CI gate.
- [x] TASK-017-4: Bind each action to the real driver with deterministic fault fixtures.
- [x] TASK-017-4: Validate manifests and refuse missing required evidence in the workflow.
- [ ] TASK-017-12: Run approved baseline profiles and retain all terminal outcomes.
- [ ] TASK-017-13: Check every linked claim and artifact, not just one artifact-level status.
- [ ] TASK-018-2: Rebind changed interfaces after the actual #216 merge.
- [ ] TASK-018-5: Produce new merged-runtime evidence and compare compatible profiles.

## Current gaps

The formal-gate implementation changed `.github/workflows/slashing-tests.yml` and reopened this claim. Hosted run `35473280388`, attempt 1, now verifies that workflow and the isolated driver fixtures.

The user requested fresh hosted verification and Claim001/documentation renewal. The [hosted renewal record](../work-logs/soak-formal-gate-hosted-renewal.md) binds that request to the verified sources.

The [renewed report](../casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/report.json) preserves the previous binding and records the new execution identities.

This renewal restores only the bounded harness discharge. The independent governance claim remains pending until its protection and acceptance requirements pass.

The local bounded model and control runner are implemented. One clean configuration and ten named negative controls pass their expected verdict checks.

The model checks safety only, not eventual termination. Its bounds are two candidates, two segments, four total iterations, and one active child.

The real-driver fixtures now cover lifecycle admission, history, terminal transitions, capture, and publication. The shared workflow runs the model and fixture gates.

The user accepted the bounded H01–H10 [binding review](../work-logs/task-017-4-final-checks.md) for TASK-017-4.

The [acceptance record](../work-logs/task-017-4-acceptance.md) binds that approval to the earlier verified source and retained evidence.

The user accepted the repaired driver's bounded binding review. Its [acceptance record](../work-logs/casper-driver-rebind-acceptance.md) identifies the approved source and preserves earlier evidence.

CLAIM-CASPER-SOAK-001 was discharged for the repaired pre-merge harness on 2026-09-18. TASK-017-4 remains complete. The source-specific ledger records control this discharge.

Later on 2026-09-18, `scripts/casper-soak/src/host_control.rs` changed after that acceptance. The change moves one function above the test module to satisfy clippy and does not change behavior. The accepted binding covers the file at commit `f9273621c`, not the current file. The claim returned to pending until a new acceptance bound the current source. The [drift record](../work-logs/casper-driver-rebind-acceptance.md#drift-after-acceptance-2026-09-18) states the scope.

On 2026-09-19 the user accepted the refreshed binding for the current source. The [refresh acceptance record](../work-logs/casper-driver-rebind-acceptance.md#refresh-acceptance-2026-09-19) identifies the approved source and the retained evidence.

Later on 2026-09-19 the binding inventory in `scripts/ci/check-casper-soak-bindings.sh` changed again. The isolated container copied no profile workflow, so the claim audit inside the container could not read seven declared artifacts. The hosted binding job failed from 2026-09-19T05:46Z. The repair adds one glob line that copies every `casper-*` profile workflow.

That repair invalidated the earlier source binding and returned this claim to pending. The [inventory repair record](../work-logs/casper-driver-rebind-acceptance.md#inventory-repair-drift-2026-09-19) preserves that historical state.

The user subsequently requested: `it is commited. Complete Claim001 renewal`.

The [renewal report](../casper/cbc-evidence/runs/casper-binding-inventory-renewal-20260919-01/report.json) binds the repaired inventory to the existing bounded H01–H10 contract.

Only the workflow-copy line differs among the 39 accepted artifacts. Fresh isolated execution and exact omission controls verify the repair without changing runtime behavior or assertions.

The renewal preserves the previous ledgers and unsuccessful inventory controls. It restores this bounded pre-merge discharge without authorizing node execution.

B44, containment assumptions, and the stated model bounds remain unchanged. This acceptance does not discharge profile claims, node correctness, or post-merge work.

The seven profile claims have separate bounded discharges. Candidate qualification and node soaks remain pending. Construction is not applicable.

The claim auditor checks exact identities, digests, phases, and declared tiers. It does not execute a prover or promote pending claims.

PRs #431 and #432 supply prerequisite containment and disk models. This claim must compose with them, not replace or duplicate them.

The shared CbC ledger stores one status per artifact. Existing waivers and old discharges cannot discharge these new claims.

TASK-017-13 must check claim IDs, source digests, tier applicability, and phase evidence in addition to the generic artifact gate.
