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

On 2026-10-21, PR #811 changed the owner scan of `scripts/casper-soak/src/host_control.rs`. The change narrows the ownership guarantee, and it needs a fresh acceptance.

- Before the change, one same-user process with an unreadable environment failed every `stop-owned` and `prefer-oom` call. A non-root runner has such processes, so every stop failed even when no owned writer remained (hosted run 39639227494).
- After the change, the scan skips a same-user process when the environment read fails with EACCES or EPERM. Every other scan error still fails the call. A writer that does not stop still fails `stop-owned`.
- **Limitation:** an owned process with an unreadable environment escapes cleanup. `stop-owned` does not kill it, and `prefer-oom` does not change its OOM score. A writer has an unreadable environment only when it is not dumpable, for example after a set-user-ID execution or a `PR_SET_DUMPABLE` change. The soak node, pytest, and the Docker CLI do not do this.
- The unit tests `stop_skips_a_process_whose_environment_is_unreadable` and `oom_preference_skips_a_process_whose_environment_is_unreadable` run as UID 65534 in the binding container. Each requires an unreadable unrelated process, kills or marks the readable owned writer, and leaves the unrelated process unchanged.

On 2026-10-21 the maintainer accepted this change, the narrower guarantee, and its limitation (PR #811, comment 6324960804) at revision `32798242d`. The [acceptance report](../casper/cbc-evidence/runs/casper-soak-writer-stop-acceptance-20261021-01/report.json) binds the accepted sources to the [review package](../casper/cbc-evidence/runs/casper-soak-writer-stop-20261021-01/report.json).

Later on 2026-10-21 the maintainer accepted a fixture fix (PR #811, comment 6326106207) at revision `c3a9a0a28`. Three assertions of `scripts/bench/test-run-merge-recovery-soak.sh` could not fail under `set -e`, and they now fail with a message. The [second acceptance report](../casper/cbc-evidence/runs/casper-soak-writer-stop-acceptance-20261021-02/report.json) binds the fixture to its [review package](../casper/cbc-evidence/runs/casper-soak-writer-stop-20261021-02/report.json).

On 2026-10-22, TASK-021-27 added a residual stop to the end of each segment. The change widens the cleanup guarantee, and it needs a fresh acceptance.

- In daily soak 39711861154, the nodes of two deadline-stopped subprocess sessions kept running and filled the disk after 18.3 h (issue #24, comment 6354292291). The scheduled soak runs the driver of `master`. `master` did not contain PR #811, so the owner stop of PR #811 did not run in that soak. The residual stop is a second, independent stop. It does not depend on the owner marker.
- After the owner stop, `host-control stop-under` kills every same-user process whose arguments or working directory are under `.subprocess-data`, whatever its owner marker. It does not consider its own process and its ancestors. A writer that does not stop fails the segment like a failed owner stop.
- Sometimes the working directory of a same-user process cannot be read (EACCES or EPERM), and its arguments do not name the directory. Such a process is recorded as `uninspected` and is not stopped. After the stop, `stop-under` records the inode, size, modification time, and change time of each entry under the directory, before and after a 2 s wait. A changed, added, or removed entry means that a writer remains, and the stop fails. A process cannot set the change time, so a future-dated file cannot hide a write.
- `residual-writers.txt` records each such process with its segment, name, match reason, environment readability, and owner-marker state. The record shows why the owner stop missed the process.
- Under `.subprocess-data`, the failure-evidence copy takes only the session directories that the failed iteration created. A directory that existed before the iteration started is not copied.
- **Limitation:** a same-user process that names a path under `.subprocess-data` in its arguments is stopped, also when it is not a soak writer. The soak runner is dedicated to one soak, so such a process is part of the soak.
- **Limitation:** an `uninspected` writer that writes less often than once in 2 s can pass the write check. It does not stop, but it also does not fill the disk quickly.
- **Limitation:** after a host protection breach, the soak already fails and must publish that failure promptly. The residual stop then kills and records within the disk stop budget, and it skips the write check.
- Six unit tests and the fixture scenarios 2 and 6 of `scripts/bench/test-run-merge-recovery-soak.sh` cover the stop, the record, the write check, and the evidence scope. The unit tests are `residual_scan_records_why_the_owner_stop_missed_a_writer`, `residual_stop_fails_when_an_uninspectable_writer_keeps_writing`, `residual_stop_detects_writes_beside_a_future_dated_file`, `residual_stop_accepts_an_unchanged_tree_with_a_future_dated_file`, `residual_stop_without_a_quiet_window_still_kills_and_returns_promptly`, and `residual_stop_accepts_an_uninspectable_process_that_writes_elsewhere`.

On 2026-10-23 the maintainer accepted this change, the wider cleanup guarantee, and its three limitations (PR #827, review 5627170022) at revision `c1cf31d7e`. The [acceptance report](../casper/cbc-evidence/runs/casper-soak-deadline-shard-stop-acceptance-20261023-01/report.json) binds the accepted sources to the [review package](../casper/cbc-evidence/runs/casper-soak-deadline-shard-stop-20261022-01/report.json).

On 2026-10-26, PR #863 changed how the driver confirms the writer stop. The change fixes a false stop failure and closes three paths that left writers running. It needs a fresh acceptance.

- In soak 40155878690, the Docker writer stop at the end of a segment failed closed after 14.5 h. A container that exits between `docker ps` and `docker inspect` or `docker kill` made the stop fail, although no writer remained.
- `stop_node_writer_commands` kills each owned container directly after its inspect confirms the owner. A failed inspect or kill is a failure only when a second `docker ps` still lists that container as running.
- The Docker CLI sends one request for each inspected container. A slow daemon therefore delays the first kill by one inspect, not by the inspects of all containers.
- A failed `docker ps`, a failed `host-control stop-owned`, an invalid container ID, or a container with another owner label still fails the stop.
- The end-of-segment stop has its own budget, `SOAK_WRITER_STOP_SECONDS` (5 to 120 s, default 30 s). After a host protection breach or during an emergency, the stop keeps the disk stop budget of 1 to 5 s.
- `writer-stop-failure.txt` names the failed step, the container, and the Docker error. A confirmed stop that wrote output keeps it as `writer-stop.log`.
- An exit of the driver before the end-of-segment stop, for example after a failed state write, now stops the owned writers. A failed stop there records `writer_stop_failed` and a failure count of at least 1.
- After a pytest that a signal ends, the driver stops the owned writers before the evidence copy. The writers then do not run into the next iteration.
- The `target_advanced` early exit record starts with its reason. A later segment of the same soak then keeps `target_advanced` in its summary.
- `host-control stop-owned` and `prefer-oom` write one line for each process that they skip because its environment is unreadable. Their behavior does not change.
- **Limitation:** the driver does not confirm the owner label of a container that exits before its inspect. A container that `docker ps` does not list as running does not write.
- **Limitation:** the end of a segment can take up to `SOAK_WRITER_STOP_SECONDS` plus 1 s longer than before.
- **Limitation:** the stop after a signal-ended pytest records a failure only in the `writer-stop.log` of that iteration. The end-of-segment stop decides the result of the soak.
- **Limitation:** like the end-of-segment stop, the exit stop applies only to a soak without a Casper manifest (`SOAK_INPUT_DIR` is not set). It applies only before the end-of-segment stop has run.
- Nine fixture cases of `scripts/bench/test-run-merge-recovery-soak.sh` cover these rules: slow, vanish, exits, stuck, persist, sigkill, breach, breach-inspect, and a restart after `target_advanced`. The unit test `owner_scan_reports_a_process_whose_environment_is_unreadable` covers the skip report.

On 2026-10-26 the maintainer accepted this change, the changed stop guarantee, and its four limitations (PR #870, comment 6429833007) at revision `6dd029064`. The acceptance includes the fix `85b04c07e` for the inspect-before-kill regression that PR #863 merged. The [acceptance report](../casper/cbc-evidence/runs/casper-soak-writer-stop-race-acceptance-20261026-01/report.json) binds the accepted sources to the [review package](../casper/cbc-evidence/runs/casper-soak-writer-stop-race-20261026-01/report.json).

B44, containment assumptions, and the stated model bounds remain unchanged. This acceptance does not discharge profile claims, node correctness, or post-merge work.

The seven profile claims have separate bounded discharges. Candidate qualification and node soaks remain pending. Construction is not applicable.

The claim auditor checks exact identities, digests, phases, and declared tiers. It does not execute a prover or promote pending claims.

PRs #431 and #432 supply prerequisite containment and disk models. This claim must compose with them, not replace or duplicate them.

The shared CbC ledger stores one status per artifact. Existing waivers and old discharges cannot discharge these new claims.

TASK-017-13 must check claim IDs, source digests, tier applicability, and phase evidence in addition to the generic artifact gate.
