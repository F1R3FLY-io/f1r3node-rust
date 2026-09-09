# Finalization and Disk Recurrence Prevention Plan

**Status:** Implementation started. The repair and acceptance gates below remain open.

**Progress:** [Gate G0, cycles B1 through B3](../tdd-plans/soak-gates-2026-09-08.md) pass local verification. Hosted evidence confirms B2 formal execution. B3 is committed and pushed at `9310ae2ce`.

Required-check enforcement and complete acceptance identity remain open. Hosted B3 inventory execution still requires confirmation. These cycles do not discharge finalization or disk claims.

The [run 34180346282 inspection](../soak-evidence/34180346282/README.md) preserves one finalization failure, an incomplete duration, disk observations, and separate reporting failures.

**Branch:** `fix/soak-disk-hygiene-stop`

**Reviewed node revision:** `b05b2f00d03d16b32d9d9328c3be868ce565c03c`

**Initial reviewed harness revision:** `022ae6d304ff47a112d7db76be956736cf056e0b`

**Current merged harness pin:** `962effd17708192627bd249362761c0ccb1fd5fa`. The [repin record](../soak-evidence/repin-962effd-2026-09-08/README.md) retains merge verification and local checks.

**Related work:** [Issue #24](https://github.com/F1R3FLY-io/f1r3node-rust/issues/24), [PR #387](https://github.com/F1R3FLY-io/f1r3node-rust/pull/387), [PR #399](https://github.com/F1R3FLY-io/f1r3node-rust/pull/399), and [system-integration PR #137](https://github.com/F1R3FLY-io/system-integration/pull/137).

## Purpose and limits

This plan separates two failures: late finalization and disk exhaustion on the soak runner. Each failure needs its own specification, reproduction, correction, and acceptance evidence.

[Correct by Construction (CbC)](../Glossary.md#correct-by-construction) connects formal claims to production behavior. A passing model does not prove properties that the model does not express.

PR #387 merged into `dev` on September 6. It supplied baseline verification and telemetry, not a demonstrated residual finalization repair.

PR #399 stops the soak earlier when disk hygiene cannot recover sufficient space. It does not yet identify or remove the growing disk consumer.

No finite test campaign guarantees that every future soak will pass. This plan requires explicit operating assumptions and prevents unsupported success claims.

The next diagnostic soak can still expose the finalization defect. The acceptance soak must follow the measured RED/GREEN repair, not precede it.

## Repository and runner status

At 11:30 UTC on September 8, PR #399 and system-integration PR #137 remained open against `dev`. The harness pin matches the current PR #137 head, not a merged revision.

The GitHub comparison confirmed that `master` contains PR #387's merge commit, `1973d116165a5483dbf03b3e37a3d505fd80f9fa`. This establishes ancestry, not a passing instrumented soak.

[Soak run 34180346282](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/34180346282) was active at that observation time. This work did not cancel, restart, or overlap that run.

The run's workflow `head_sha` does not establish the tested node SHA. Gate G0 must verify the actual workload identity from retained evidence.

## Why previous verification did not prevent recurrence

### 1. The claims were narrower than the failure

[CLAIM-FINALITY-001](../claims/settled-effect-probe-equivalence.md) establishes settled-probe reference semantics and batched-walk equivalence. It does not establish a finalization deadline for the complete node pipeline.

[The issue evidence](https://github.com/F1R3FLY-io/f1r3node-rust/issues/24#issuecomment-5526819327) reports healthy settled-probe time after that correction. Other work remained expensive.

The issue reports 95th-percentile finalization latency (p95) for individual load iterations. These measurements differ from the dashboard's passive-series proposal timing.

Run `33707959088` recorded 752.6 ms per merge call and 965 ms per replay block in its worst completed iteration. Finalization p95 reached 96.3 seconds, with 147 deploys outside the existing 45-second finalization limit.

These observations do not demonstrate a false settled-probe theorem. They demonstrate that the local correction did not establish the required system performance.

[ReplayHotLoop.tla](../../formal/tlaplus/replay_liveness/ReplayHotLoop.tla) counts selected replay operations and expresses eventual completion under fairness. Its small configuration does not model arrival pressure, complete merge processing, storage latency, or a 45-second deadline.

Eventual completion is not bounded completion time. A correct component can coexist with growing queues and late finalization.

### 2. The carrier model abstracts away required production behavior

[CarrierIndex.tla](../../formal/tlaplus/carrier_index/CarrierIndex.tla) checks index completeness and absence soundness. Its [baseline configuration](../../formal/tlaplus/carrier_index/MC_CarrierIndex.cfg) uses two signatures and three blocks.

`Crash == UNCHANGED vars` models interruption without losing modeled state. It does not explore volatile writes, durable flushes, process restart, or storage corruption.

The model has no parent-scope traversal, operation counter, validation queue, or finalization deadline. Its reported 222-state pass is useful baseline evidence, not a proof of the residual repair.

[The 512-case production property](../../block-storage/tests/carrier_index_property_test.rs) uses an in-memory index. Its operations are record, prune, and watermark update.

That property does not exercise crash/restart, injected read failures, or complete validation against generated directed acyclic graphs (DAGs). [CLAIM-FINALITY-002](../claims/repeat-deploy-carrier-index-equivalence.md) correctly remains pending.

There is also an unresolved specification conflict. C5 requires equality across storage-availability patterns, including missing ancestry.

However, `repeat_deploy_certified_index_engagement_skips_the_scan` expects reference failure and indexed acceptance on unreadable ancestry. The [test](../../casper/tests/batch2/validate_test.rs) therefore does not establish C5 as written.

This conflict is not evidence of the measured performance cause. It must nevertheless be resolved before claim discharge, without silently weakening the claim.

### 3. A green workflow did not mean all formal checks ran

The continuous integration (CI) workflow does not run every formal job on every event.

[The TLA+ job](../../.github/workflows/slashing-tests.yml) runs on schedule or manual dispatch, not on pull requests. Its earlier PR-gate description does not match that condition.

[Run 33908132670](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/33908132670) completed successfully, but its TLA+ invariant job was `skipped`. The Rocq job passed.

Earlier assistant reports that formal gates passed with that PR overstated the evidence. The recorded TLA+ result must remain `skipped`, not `passed`.

[check-tla-invariants.sh](../../scripts/ci/check-tla-invariants.sh) checks an explicit list of 17 default configurations. It does not discover every model in the repository.

Negative controls run manually. The three additional exhaustive configurations are opt-in. The [Rocq driver](../../scripts/ci/check-formal-invariants.sh) rebuilds three named projects, not every available theory.

A configuration count cannot substitute for a claim-to-check inventory. Missing, skipped, or unrelated checks cannot establish the required property.

### 4. Disk protection had no complete resource claim

[The disk incident record](../work-logs/task-soak-disk-hygiene-stop-2026-09-08T05-06Z.md) documents three runner deaths after the disk guardian fired:

| Run | Free disk at guardian stamp | Outcome |
| --- | ---: | --- |
| `33939315110` | 3992 MB | The runner exhausted disk after the stamp. |
| `33978505238` | 4022 MB | The runner exhausted disk after the stamp. |
| `34056342543` | 3597 MB | The runner exhausted disk after the stamp. |

The old post-hygiene check admitted another iteration above the 4096 MB floor, even below the 8192 MB hygiene threshold. Repeated cleanup attempts reclaimed nothing while space continued to fall.

Stopping node writers did not establish a bound on all remaining disk consumption. The consumer outside the cleanup paths remained unidentified in the retained evidence.

At the inspected revision, the formal tree had no disk or soak model. The driver also had no mandatory CbC attribute.

D1 now adds a bounded admission model. The emergency-response and all-writer resource arguments remain open. Mandatory scope still requires maintainer ratification.

[The current driver test](../../scripts/bench/test-run-merge-recovery-soak.sh) now covers a constant 7000 MB reading and no-op cleanup. It does not establish safety against continued external writes, probe failure, or slow diagnostics.

The local `disk_usage_tag_summary` uses per-root timeouts before the durable tag write. The root count can grow, so per-command bounds do not establish a total diagnostic deadline.

`disk_usage_snapshot` has the same aggregate-bound concern. `bounded` also runs without a deadline when `timeout` is unavailable.

These are visible proof gaps, not evidence that these paths caused the historical deaths. They need focused tests before another long run.

### 5. Telemetry and outcome classification were incomplete

[extend-issue24-metrics.sh](../../scripts/bench/extend-issue24-metrics.sh) extends the summary scraper in `metrics.py`. The raw comma-separated values (CSV) writer in `resource_monitor.py` uses a separate metric-keyword filter.

Both the earlier harness pin `e4a26eb69fcfc611d56d8473f4de2b09960bc173` and the reviewed pin have that separate CSV path. The earlier assertion that missing summary entries necessarily remove raw CSV rows was incorrect.

The extension is not an end-to-end telemetry test. Emission, collection, labels, units, persistence, and artifact retrieval must all be checked.

All three disk-death runs also recorded finalization-limit failures. Disk pressure can affect latency, but a later disk death cannot erase earlier completed product failures.

## Required development sequence

Each behavior uses one RED/GREEN cycle:

```text
observed failure -> production RED -> matching formal RED
-> minimal correction -> production GREEN -> formal GREEN
-> retained evidence -> composed acceptance
```

RED means the specified behavior fails for the intended reason. GREEN means the same obligation passes after the correction.

Existing fixes require historical RED evidence against their actual pre-fix revisions. New repairs require RED against current production.

A compile failure, missing tool, or malformed fixture is not a behavioral RED result. A post-fix baseline pass is not a new repair.

### Gate G0: Bind the evidence and make the checks explicit

**Owners:** The node maintainer and CI maintainer.

1. Record the workflow-control SHA, node SHA, image digest, harness SHA, configuration, workload profile, and run attempt.
2. Preserve prior summaries, raw CSV files, failure counts, logs, and surviving `soak-health` and `pm` tags.
3. Inventory each required claim, its production function, formal configuration, production test, and CI job.
4. Add a bounded PR formal job for the relevant configurations and their expected-failure controls.
5. Keep larger exhaustive checks separate, with explicit completion and timeout status.
6. Reject missing configurations, skipped required jobs, proof errors, and unexpected negative-control results.
7. Record the exact violated invariant for each expected-failure control.

**Exit evidence:** The inventory identifies checked baseline claims and pending new obligations. It cannot report pending obligations as passed.

Add each new configuration to required CI when its RED/GREEN cycle starts. The final acceptance gate requires every applicable check to run successfully.

Each CbC record in `docs/cbc-evidence/` must bind these fields:

- The claim ID, specification digest, production revision, and model digest.
- The verifier version, configuration, assumptions, and finite bounds where applicable.
- The RED counterexample and matching production failure.
- The GREEN verifier log and production test results.
- The exact-candidate workload evidence and unresolved obligations.

Reject stale evidence after a change to the implementation, specification, or model. Reverification must use the changed candidate.

Keep `CLAIM-FINALITY-002` pending. If F1 identifies another stage, create a separate work-bound claim rather than mislabeling carrier evidence.

The existing formal scripts remain useful regression gates. A successful run of those scripts alone does not complete this inventory.

The [claim inventory](../claims/soak-claim-inventory.json) now records baseline checks and pending obligations. The [B3 evidence](../cbc-evidence/scripts-ci-test-soak-claim-inventory-sh.md) retains inventory validation and hosted B2 confirmation.

G0 remains open. Required-check enforcement and the complete acceptance identity are not established.

### Gate D1: Prove the known disk admission correction

**Owner:** The soak maintainer.

**Interface:** The real soak driver inside a disposable container, with external-command shims.

**Behavior:** After hygiene, an enabled disk guard must not admit work while free space remains below the floor-plus-band threshold.

Do not mount host cleanup paths into the container. The pre-fix driver ignores the new root overrides and can delete files outside the fixture.

1. Run the constant-7000-MB scenario against the revision before `ca85cfe3e`.
2. Retain the expected extra-iteration failure as historical RED evidence.
3. Run the same scenario against the branch and retain its GREEN result.
4. Add a small TLA+ model of admission, hygiene, measurements, guardian state, stop, and evidence publication.
5. Confirm that floor-only admission violates the matching invariant.
6. Confirm that the corrected admission transition satisfies the invariant.

The proposed model location is `formal/tlaplus/soak_disk/SoakDisk.tla`, with corrected and expected-failure configurations. The model must permit hygiene to reclaim zero bytes.

The model must not assume that cleanup always succeeds.

**Exit evidence:** The same behavioral counterexample fails the old driver and the old model configuration. The corrected driver and configuration pass.

This gate establishes controlled refusal, not complete soak duration or bounded disk growth.

The [local D1 record](../cbc-evidence/scripts-run-merge-recovery-soak-sh.md) retains the historical admission failure and matching `AdmissionRequiresBand` counterexample. The existing correction passes both local checks.

The corrected model completed with 54 distinct states. Its coverage includes admission after sufficient reclamation and refusal publication. No production driver change was needed.

The new container regression and both model configurations are registered in CI. Hosted execution and maintainer review remain pending. This result does not discharge `CLAIM-SOAK-001`.

### Gate D2: Bound emergency response and retain evidence

**Owners:** The soak maintainer and system-integration maintainer.

**Proposed claim:** `CLAIM-SOAK-001`, pending maintainer ratification and formal discharge.

The claim must cover both admission and mid-iteration failures. It must distinguish an attempted kill, confirmed writer termination, and durable evidence publication.

1. Exercise falling disk samples during an active iteration, including the soft floor and hard floor.
2. Exercise cleanup with no reclamation, partial reclamation, and sufficient reclamation.
3. Exercise malformed or missing disk samples after a successful startup probe.
4. Exercise a failed guardian, hanging Docker commands, delayed `du`, many session roots, and unavailable metadata services.
5. Require one aggregate diagnostic deadline independent of session count.
6. Ensure detailed attribution cannot delay the minimal durable breach record.
7. Verify that cleanup preserves active sessions and the image under test.
8. Verify that a guardian event prevents later iterations even when cleanup subsequently recovers space.
9. Verify that segment restart cannot turn an earlier failure into success.

Run each behavior as a separate RED/GREEN cycle. Use shims or disposable environments, never a full developer filesystem.

The reserve argument must identify these terms:

```text
R = space required for runner operation, minimal evidence, and final upload
G = bounded growth rate of every remaining writer
T = maximum detection, stop, and evidence latency
J = maximum burst not represented by the sampled growth rate
M = explicit safety margin

emergency free-space threshold >= R + G*T + J + M
```

Every term needs a common unit and recorded justification. A sampled average is not a worst-case growth bound.

If a writer has no enforceable bound, the reserve claim remains open. Per-writer retention or quotas can establish bounds only after ownership and safety review.

The formal model must include external writes and stalled optional diagnostics. Fairness alone cannot prove a bounded emergency deadline.

System-integration PR #137 bounds its exit-path attribution walks to 10 seconds in aggregate. That separate correction does not bound this repository's guardian or hygiene paths.

**Diagnostic prerequisite:** Real-driver fault traces match model transitions, and the emergency response meets its declared deadline under enforced diagnostic limits.

**Full exit evidence:** Response time, writer growth, and reserve bounds hold for the acceptance workload. Full discharge depends on D3's disk-growth evidence.

Propose mandatory CbC scope for the driver, verdict/retry logic, and relevant runner exit path. Apply new attributes only after maintainer ratification.

The [missing-sample cycle](../cbc-evidence/soak-d2-probe-2026-09-08/README.md) completes one local D2 behavior. Missing samples before and after hygiene now prevent admission and produce a recorded failure.

Both production traces failed before the correction. The matching formal control violates `AdmissionRequiresSample`. The corrected configuration completed with 22 distinct states.

This result does not establish an emergency deadline, confirmed writer termination, or durable evidence. Hosted execution, model review, and the remaining D2 behaviors stay pending.

The [numeric-prefix cycle](../cbc-evidence/soak-d2-sample-2026-09-09/README.md) completes a second local D2 behavior. The driver now rejects `16384junk` instead of admitting work from its numeric prefix.

Production RED and the exact `AdmissionRequiresValidSample` counterexample precede the one-line correction. Production GREEN records refusal, and formal GREEN completes with 17 distinct states.

This cycle does not verify all malformed values or active-iteration response. The emergency deadline, guardian failure, confirmed termination, durable evidence, and restart obligations remain open.

The [B7–B12 emergency cycles](../cbc-evidence/soak-d2-emergency-2026-09-09/README.md) add six local corrections with matched production and formal counterexamples.

The driver now rejects unavailable active samples, detects active guardian death, and records disk breaches before Docker stop requests. Probe failures invalidate partial output.

Disk attribution has one command-group deadline per invocation. The guardian includes tag preparation and publication in that deadline. This does not bound the complete emergency path.

A retained guardian marker now prevents restart admission. Recovery preserves a failure outcome but does not establish crash durability or exact uncommitted-event counts.

The [B13 stop cycle](../cbc-evidence/soak-d2-stop-2026-09-09/README.md) bounds each disk stop-command group. It tests Docker clients that ignore `TERM`.

The first correction returned but left the clients alive. The corrected wrapper preserves its process group until `KILL` and passes the fixture.

The bounded gate now passes 12 positive configurations and 12 exact controls. These results do not confirm writer termination or establish a composed emergency deadline.

Disk hygiene, other stop paths, cleanup ownership, full admission coverage, durable publication, and the D3 reserve argument remain open.

Hosted confirmation and maintainer review remain pending. D2 is not complete.

### Gate O1: Verify observability before the diagnostic soak

**Owners:** The soak maintainer and Casper maintainer.

1. Exercise carrier absence, carrier hit, fallback, merge, and replay in a short instrumented run.
2. Verify emitted metrics against both the summary collector and the actual raw CSV writer.
3. Verify the disk usage, free-space, inode, protection, and writer-ownership records.
4. Download the artifacts and validate their schema and candidate identity.
5. Exercise evidence retrieval after a controlled writer stop in a disposable runner.
6. Check checkpoint deadlines before dispatch, including late starts and manual dispatches.

**Required raw evidence:**

- `node-metrics-timeseries.csv`, resource time series, and per-iteration failure records.
- Carrier engagement, row-read, fallback, ancestor metadata, and ancestor body counters.
- Merge relation size, branch count, conflict edges, rejection options, state actions, and phase durations.
- Replay spawn, reset, user work, system work, checkpoint calls, and durations.
- Disk usage before and after cleanup, free bytes, free inodes, and surviving protection records.

Use Bash, `jq`, and a CSV-aware reader such as Ruby CSV when quoting requires it. Do not add a Python summarizer.

Preserve metric labels and units. Distinguish cumulative counters, histogram sums, histogram counts, and instantaneous values.

Compute interval deltas per node and label set. Separate restart epochs and reject counter resets or missing intervals as performance evidence.

Do not add overlapping parent timers and child timers. Do not infer finalization p95 from a mean replay duration or a median of iteration summaries.

**Exit evidence:** Retrieved artifacts contain expected exercised metrics. Missing metrics are not interpreted as zero work.

### Gate F1: Identify the remaining finalization work bound

**Owner:** The Casper maintainer.

**Prerequisites:** G0, D1, O1, and D2's tested emergency response.

If the full reserve claim remains open, use an isolated diagnostic runner with enforced writer limits. Do not treat that limited run as acceptance.

1. Confirm that no other soak is active before dispatch.
2. Run the existing issue #24 workload against the immutable instrumented candidate.
3. Preserve the current load profile and finalization assertion without relaxed limits.
4. Compare passing and failing intervals using carrier work, merge work, replay work, queue depth, and disk health.
5. Identify the dominant work stage and its authenticated input-size dimensions.
6. Record the violated bound and the evidence that distinguishes it from competing explanations.

The diagnosis must account for throughput, queue residence, and repeated work. A dominant timer alone does not prove an algorithmic cause.

If telemetry remains insufficient, add narrowly scoped instrumentation first. Do not select an optimization or manufacture a failing property.

**Exit evidence:** A reproducible workload shape and a justified operation-bound obligation.

### Gate F2: Perform the production and formal RED/GREEN repair

**Owner:** The Casper maintainer.

1. Write one generated property through the real production entry point for the measured work bound.
2. Retain its failure against current production, including the seed and minimized fixture.
3. Add the matching formal configuration with explicit work accounting and input-size assumptions.
4. Retain a counterexample that violates the same bound for the same mechanism.
5. Implement the smallest correction without changing consensus decisions.
6. Run the production property and the formal configuration to GREEN.
7. Keep the pre-fix configuration as an automated expected-failure control.

The formal model must charge actual modeled operations. A Boolean that merely assumes optimized work is not sufficient evidence of the production bound.

Use inductive verification for any claimed unbounded relation. Label finite model-checking results with their exact state-space bounds.

**Exit evidence:** Paired RED/GREEN results, a model-to-code map, and unchanged consensus outputs.

### Gate F3: Complete the semantic production bridge

**Owner:** The Casper maintainer, with protocol review for the availability contract.

1. Resolve C5's unreadable-history conflict before claiming full carrier equivalence.
2. Compare forced-index and forced-reference paths on identical generated DAGs.
3. Verify validation results, post-state roots, and rejected-deploy records where the correction affects them.
4. Exercise valid, invalid, and approved carriers across forks and expiration boundaries.
5. Exercise insert, crash, restart, prune, read failure, and validation traces through real storage boundaries.
6. Add a carrier-index fuzz target with an independent oracle and retained failure seeds.
7. Keep forced-path controls test-only and outside consensus inputs.

The availability decision must state when equality applies and when typed deferral is required. Changing that contract needs review, not a narrower assertion chosen to pass tests.

Property tests and fuzzing connect production to the model. They are not proof authority.

**Exit evidence:** Reviewed availability semantics, differential results, crash-boundary evidence, fuzz results, and updated pending claims.

### Gate D3: Remove the measured disk-growth cause

**Owners:** The owner of the growing files and the soak maintainer.

1. Use the diagnostic run to identify the growing filesystem and writer.
2. Include Docker layers, build cache, harness roots, runner logs, and open-deleted files in attribution.
3. Separate bytes and inode exhaustion, including temporary copies during evidence retention.
4. Write one production-facing retention or cleanup property for the identified owner.
5. Confirm its RED result and matching resource-model counterexample.
6. Apply the smallest safe lifecycle correction and verify both GREEN results.
7. Validate storage demand across the full planned duration and failure-retention policy.

Do not delete active node state or required evidence. Do not replace attribution with a larger volume or lower protection threshold.

**Exit evidence:** The measured writer has a verified lifecycle bound. A controlled early stop alone does not satisfy this gate.

### Gate A1: Run the acceptance soak and gate completion

**Owners:** The release maintainer, Casper maintainer, and soak maintainer.

**Prerequisites:** All earlier gates, including the production corrections, are GREEN.

1. Rerun required CI and relevant formal checks on the exact candidate.
2. Run the fixed workload with unchanged finalization limits and protection controls.
3. Complete the required soak duration without overlapping another soak.
4. Retain all iteration outcomes, including failures before any restart or infrastructure event.
5. Compare measured work against the proved bounds and operating assumptions.
6. Verify retained artifacts after the runner terminates normally.
7. Record a separate result for finalization and disk behavior.
8. Discharge applicable CbC claims only after their complete evidence requirements pass.

If this run serves release promotion, use the [60h stability soak](../Glossary.md#60h-stability-soak) and its exact-candidate gates. A short diagnostic run or a Dev integration soak is not a replacement.

| Finalization result | Disk result | Acceptance decision |
| --- | --- | --- |
| The workload passes. | The full duration completes safely. | Accept only if all evidence and formal gates also pass. |
| The workload fails. | Disk remains healthy. | Reject the product repair. |
| The workload passes before early stop. | Protection stops the run. | Record a safe stop, but reject completion. |
| Product failures precede a disk event. | The runner stops or disappears. | Preserve both failures. Do not erase product evidence. |
| The data is incomplete or misbound. | Any outcome | Hold acceptance. Do not infer success. |

## Execution checklist

- [ ] G0: Candidate identity and required-check inventory are complete.
- [ ] D1: The known disk admission fix has paired historical RED/GREEN evidence.
- [ ] D2: Emergency response and evidence publication have verified aggregate bounds.
- [ ] O1: End-to-end telemetry and artifact retrieval pass.
- [ ] F1: Diagnostic evidence identifies the finalization work bound.
- [ ] F2: The measured production property and matching formal configuration pass after the correction.
- [ ] F3: Differential, crash, and fuzz evidence satisfy the reviewed semantic contract.
- [ ] D3: The identified disk consumer has a verified lifecycle correction.
- [ ] A1: The exact-candidate acceptance soak passes both defect gates.

## Branch and review rules

This plan is documentation, not a claim discharge or a production repair. PR #399 can establish disk protection without claiming that issue #24 is fixed.

Keep performance correction commits separate from disk correction commits. Use a follow-up branch if PR #399 merges before finalization attribution completes.

Wait until system-integration PR stack [#139](https://github.com/F1R3FLY-io/system-integration/pull/139) → [#138](https://github.com/F1R3FLY-io/system-integration/pull/138) merges before repinning. Then select the resulting merged revision and repeat cross-repository checks. PR #137 alone does not satisfy this prerequisite.

Keep the current pin unchanged until that prerequisite is met. Do not treat a PR-head pin as merged-candidate evidence.

The prerequisite was verified on September 8. All three pins now identify merged `main` revision `962effd1`. Local cross-repository checks passed. This repin does not close any repair or acceptance gate.

Keep issue #24 open until its acceptance evidence passes. Merge status, coverage, and a green unrelated proof cannot substitute for that evidence.

Do not cancel an active soak or bypass repository gates to obtain a new run. Commit, push, and merge operations require separate explicit authorization.

## Evidence references

- [Finalization failure evidence](https://github.com/F1R3FLY-io/f1r3node-rust/issues/24#issuecomment-5526819327).
- [Disk incident and branch work log](../work-logs/task-soak-disk-hygiene-stop-2026-09-08T05-06Z.md).
- [Casper CbC repair constraints](../casper/design/cbc-repair-plan.md).
- [Formal gate driver](../../scripts/ci/check-formal-invariants.sh) and [TLA+ configuration inventory](../../scripts/ci/check-tla-invariants.sh).
- [Pinned raw CSV collector](https://github.com/F1R3FLY-io/system-integration/blob/022ae6d304ff47a112d7db76be956736cf056e0b/integration-tests/test/infra/resource_monitor.py).
- [Earlier raw CSV collector](https://github.com/F1R3FLY-io/system-integration/blob/e4a26eb69fcfc611d56d8473f4de2b09960bc173/integration-tests/test/infra/resource_monitor.py).

This analysis inspected source files and recorded CI metadata. It did not rerun formal proofs, establish a new RED result, or execute a soak.
