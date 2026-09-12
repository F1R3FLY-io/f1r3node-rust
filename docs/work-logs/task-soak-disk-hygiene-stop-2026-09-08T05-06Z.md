---
task: soak-disk-hygiene-stop
branch: fix/soak-disk-hygiene-stop
claimed_by: claude-session-74f6ecbb
claimed_at: 2026-09-08T01:30:00Z
handoff_status: ready
next_steps:
  - "DONE: hosted inventory execution is recorded for 9310ae2ce and d6aaba962 in commit bc1e16380."
  - Confirm required-check enforcement with a maintainer who can inspect classic protection.
  - Use the retained run 34180346282 inspection to prepare the remaining diagnostic work.
  - "DONE: local D1 production and formal RED/GREEN matched in disposable isolation. Confirm hosted checks and maintainer review."
  - "DONE 2026-09-08T06:20Z: repinned SYSTEM_INTEGRATION_REF x3 to SI 7f488f93 (PR #137 head after main 0fb6337 was merged in at 1e411383; descends per merge-base --is-ancestor). The first SI push c32f1f1d was refused because it was cut from dev and lacked #132 and #134"
  - "DONE 2026-09-08T06:40Z: repinned again to SI 022ae6d3, the #137 head that bounds the post-mortem du walks at 10s in aggregate and covers them in tests; descends from 7f488f93 and 0fb6337"
  - "DONE 2026-09-08: verified PR stack #139 → #138 and repinned all three sites to merged main revision 962effd1."
  - Complete the diagnostic prerequisites before starting a non-overlapping soak.
  - Identify the growing consumer before selecting its lifecycle correction.
---

# Weekend soak disk deaths after the #379 floor

## What happened

Three weekend soak runs died the same way between 2026-09-05 and 2026-09-07.

| Run | Dispatch | Target | Guardian stamp | Runner lost |
| --- | --- | --- | --- | --- |
| 33939315110 | scheduler bot, Fri 02:31Z | master 487a35c29 | `disk-breach` at 3992 MB, 16:38:11Z | 16:38:34Z |
| 33978505238 | automatic restart, Fri 16:38Z | master 487a35c29 | `disk-breach` at 4022 MB, 23:52:25Z | 23:52:45Z |
| 34056342543 | manual, Sat 19:54Z | master be2324661 | `disk-breach` at 3597 MB, Sun 10:01:56Z | 10:02:14Z |

Each soak job ended with the runner worker failing on `No space left on device` while it wrote `/opt/actions-runner/_diag/Worker_*.log`. The workflow post-mortem found each VM terminated. The `soak-health` freeform tag survived on every instance record and proved the PR #379 disk guardian fired and killed the nodes. The runner still ran out of disk about 20 seconds after each stamp.

## Why the floor did not save the run

Hygiene runs at each iteration boundary when free space is inside floor plus band, 8192 MB by default. In run 34056342543 the pass at 05:42Z reclaimed 6.2 GB. The next three passes at 09:33Z, 09:43Z, and 09:52Z reclaimed nothing, while free space fell from 7956 MB to 7355 MB. The post-hygiene check compared against the 4096 MB floor only, so each pass let another iteration start. The last iteration crossed the floor mid-run, the guardian fired, and the runner died before any report.

The earlier inspection reported 0–2-second copies and roughly 20 MB pytest dumps. Those observations did not identify every disk writer or exclude other archive copies. The conclusion that failure-evidence copies could not contribute to disk growth was too strong.

Secondary observations:

- The Saturday manual dispatch received a segment 1 deadline of 20:00Z, six minutes after dispatch. Segment 1 ran zero iterations and the whole run landed in one 11.5 hour final segment with no checkpoint.
- All three runs also had the known test_load `deploys not finalized within 45s` failures, so the verdict would have been red without the disk death.
- The automatic restart fired once and the chain is capped at one. The manual dispatch carried `retry_attempt=1`, so it had no restart.

## What this branch changes

Commit `ca85cfe3e`, pushed, PR pending.

- `scripts/run-merge-recovery-soak.sh`. The post-hygiene check now ends the segment when free space is still inside floor plus band. A new `disk_usage_snapshot` writes `df` and `docker system df`. It also writes `du -sm` for the output dir, the three harness roots, the runner `_diag` and `_work` dirs, and the session dirs. It prints on every hygiene pass and goes into `disk-floor-breach.txt` on both disk stop paths. The guardian appends a compact per-root usage summary to the `soak-health` tag and writes the full snapshot beside the breach marker.
- `SOAK_TMP_ROOT` and `SOAK_RUNNER_ROOT` make the sweep and runner paths overridable, so the driver test sweeps a private tree.
- `scripts/bench/test-run-merge-recovery-soak.sh`. A third scenario shims `df` inside the band and `docker` to a no-op. It asserts that the stale session is swept and no iteration starts. It also asserts that the soak fails closed and the evidence carries `du` lines.

Verification: `bash -n` clean, driver test passed with three scenarios, pre-commit hook passed, full pre-push gate passed with 20 checks.

## Cross-repository half

The initial system-integration pin was `022ae6d3`, a PR #137 head with bounded post-mortem walks. The request remains documented at `docs/discoveries/2026-09-07-soak-disk-post-mortem-request.md` in that repository.

The repin required PR stack #139 → #138 to merge. That prerequisite is now verified. The pin identifies merged `main` revision `962effd1`, and local cross-repository checks passed.

## Attribution before limits

Raising the boot volume alone would move the death later. The next soak with both halves names the consumer in two places that survive the VM. Prune that consumer first, then size the volume if the steady state still needs it.

## Combined prevention plan

The [finalization and disk prevention plan](../plans/soak-recurrence-prevention-2026-09-08.md) defines separate RED/GREEN cycles and acceptance gates for both failures. It also records gaps in previous formal verification and CI reporting.

The plan is documentation only. It does not discharge `CLAIM-FINALITY-002` or establish that the disk consumer has been corrected.

At 11:30 UTC on September 8, node PR #399 and system-integration PR #137 remained open against `dev`. Soak run `34180346282` was active, and this work did not alter that run.

## Gate G0 inventory cycle

The [B3 inventory evidence](../cbc-evidence/scripts-ci-test-soak-claim-inventory-sh.md) records one RED/GREEN cycle against base `43af06da`. The new inventory binds four claims to source digests, baseline checks, evidence, and pending obligations.

The hosted formal job passed on the PR's synthetic checkout `bad72c4c`. The retained observation includes downloaded TLC logs and the verified artifact digest.

The visible `dev` rules omit the TLA+ check, and classic required-check protection remains inaccessible. G0 remains pending until enforcement and complete acceptance identity are established.

The current shell tests are GREEN. They do not discharge the shell implementation, `CLAIM-FINALITY-002`, or the proposed resource claim.

System-integration PR #137 remains open. Soak run `34180346282` was still active at 12:50 UTC, and this cycle did not change that run.

At 13:20 UTC, run `34180346282` had completed with failure. Its three soak segments, report aggregation, and dashboard assembly reported failure.

At that observation, artifact `10057623626`, named `merge-recovery-soak-79200s-34180346282-1`, remained available at 851355201 bytes. Its contents, tested node identity, and failure causes had not yet been inspected.

The workflow API reported no active soaks at that observation. No new soak was dispatched.

## Failed-run inspection

G0/B3 is committed and pushed at `9310ae2ceda65d1944e1235336fd12df489e4197`. Its historical evidence remains unchanged.

The [run 34180346282 report](../soak-evidence/34180346282/README.md) identifies tested node `0f5d2b74`, control revision `be232466`, and harness `0fb63372`. It also records the collector overlay.

The artifact preserves 51 iterations and one completed finalization failure. Iteration 42 left five deploys unfinalized after the 45-second wait. Its sustained finalization p95 was 64.9 seconds.

The run stopped after `dev` advanced. Earlier cleanup increased free space, but the evidence does not establish a full-duration disk guarantee.

The failure archive contains 8,813,121,294 bytes from accumulated earlier Docker sessions. This identifies retained data, not every disk writer.

Aggregation failed with `Argument list too long`. The dashboard then failed because its summary was missing. Neither reporting failure removes the finalization failure.

Verified archive copies reside outside `target/`. The inspection preserves every available iteration outcome and raw CSV file. The dominant work bound remains unestablished.

The current inventory now binds these reviewed documentation changes. The B3 manifest still describes its historical commit, not this inspection. All claims and acceptance gates retain their pending obligations.

No repin, soak dispatch, production repair, formal discharge, commit, or push occurred during this inspection.

## Merged harness repin

The user then confirmed that the system-integration changes had merged. The [repin record](../soak-evidence/repin-962effd-2026-09-08/README.md) retains API observations and ancestry checks.

The selected revision is `962effd17708192627bd249362761c0ccb1fd5fa` on `main`. It contains the previous pin, PR #139's head, and the observed `dev` revision.

The repository helper updated all three pin sites without bypass flags. The merged harness passed 313 unit tests. The initial missing-Poetry failures were environment failures, not behavioral RED results.

Pin, collector, release, and formal-routing regression checks passed. The collector overlay applied to the merged harness, compiled, and remained byte-identical on repeat application.

The workload file and both collectors are unchanged across the repin. The finalization wait remains 45 seconds. The failed-run evidence remains separate from future acceptance evidence.

No new soak, production correction, formal discharge, commit, or push followed the repin during that inspection.

## D1 admission verification

The [D1 record](../cbc-evidence/scripts-run-merge-recovery-soak-sh.md) retains the historical production failure, matching formal counterexample, and GREEN results.

The pre-fix driver at `0f5d2b74` admitted one iteration after zero reclamation at 7,000 MiB. The corrected driver at `ca85cfe3e` refused admission and recorded failure.

The current driver has the same bytes as that correction. No production change was needed. The corrected model completed with 54 distinct states.

The tests used disposable containers without host mounts or network access. The first fixture attempt failed on file permissions before driver execution and is not RED evidence.

The final fixture includes the required metric and summary helpers. The existing three-scenario driver suite also passed inside a disposable container.

The bounded formal tier now runs three positive configurations and three expected-failure controls. All six passed local classification and actual TLC verification.

The inventory retains pending candidate status. Hosted D1 execution, model review, mandatory-scope ratification, G0 enforcement, and the remaining repair gates stay open.

## D2 missing-sample admission

The user authorized one local D2 cycle after confirming the harness pin and additional cloud quota. No soak or hosted retry followed that authorization.

At `80914eeda`, missing samples before and after hygiene each admitted one iteration. Both traces ended with driver exit zero and no recorded failures.

An eight-line common check now refuses both admissions. Each corrected trace ends with zero iterations, one failure, driver exit one, and local refusal evidence.

The [cycle evidence](../cbc-evidence/soak-d2-probe-2026-09-08/README.md) retains both production traces and the matching `AdmissionRequiresSample` counterexample. The corrected model completed with 22 distinct states.

The first formal attempt failed on a sample encoding error before checking behavior. The record excludes that exit-75 result from behavioral RED.

The existing D1 and three-scenario driver regressions passed in disposable containers. All four positive configurations and four exact controls passed actual bounded TLC verification.

The classifier passed 28 cases, and routing passed six scenarios. Workflow, release, repin, collector-extension, and summary regressions also passed.

D2 remains open. This cycle does not prove emergency timing, writer termination, durable evidence, all-writer bounds, or acceptance. No claim was discharged.

## D2 numeric-prefix rejection

The user directed local D2 work before O1. The next cycle reproduces admission after a valid startup sample and a later `16384junk` field.

At `59430d59b`, the production test fails because the driver admits one iteration without recording failure. TLC then reports `AdmissionRequiresValidSample` with exit 12.

The one-line correction adds a digit check before the existing numeric conversion. The corrected trace records zero iterations, one failure, and local refusal evidence. The corrected model completes with 17 distinct states.

The [cycle evidence](../cbc-evidence/soak-d2-sample-2026-09-09/README.md) binds both traces, model inputs, and complete verification logs. D1, B5, and the three-scenario driver suite pass in disposable containers.

The composed bounded gate passes five positive configurations and five exact negative controls. The classifier passes 35 cases, and routing passes six scenarios.

This result does not complete D2 or discharge a claim. The model assumes completing commands and local writes. Active-iteration response, guardian failure, aggregate deadlines, termination, durability, and restart preservation remain pending.

No commit, push, hosted run, or soak followed this cycle.

## D2 emergency cycles B7 through B12

The user requested completion of D2. Six further local cycles started from `ac94c1755` and retained each intermediate correction separately.

The corrections cover active sample loss, record ordering, guardian death, probe timeout, aggregate attribution, and retained-breach restart handling.

Each production regression failed before its formal counterexample. Each formal RED reported the required invariant with exit 12 before the corresponding production correction.

All six corrected local behaviors pass. The [evidence package](../cbc-evidence/soak-d2-emergency-2026-09-09/README.md) retains the source identities, traces, and limitations.

A driver-suite failure exposed a fixture readiness race. The corrected fixture waits for every required telemetry file. Thirty repeated first-scenario checks and the full suite pass.

Two combined verification commands reached tool limits. Separate completed runs supply the accepted results. The retained records do not classify those tool limits as behavioral RED.

The combined bounded gate passes 11 positive configurations and 11 exact negative controls. The classifier covers 77 cases, and routing covers six scenarios.

D2 remains incomplete. Stop and cleanup command bounds, confirmed termination, durable publication, full admission coverage, and cleanup ownership still require work.

A composed emergency deadline and the D3-dependent reserve argument remain open. No claim is discharged. No commit, push, hosted run, or soak followed these cycles.

## D2 stop deadline cycle B13

The next cycle starts from `3d2aa7904`. A stalled Docker fixture reproduces delayed failure publication before the matching `StopWithinBudget` formal counterexample.

The first timeout correction returns but leaves two clients alive. The retained process snapshot shows both clients after local summary publication.

The corrected wrapper keeps its shell alive until process-group cancellation. The fixture clients exit, and the driver publishes one protection failure.

The [B13 evidence package](../cbc-evidence/soak-d2-stop-2026-09-09/README.md) retains both correction attempts and separate production and formal results.

The first combined verifier runs overlap on fixed temporary log paths. Those results cannot provide isolated verification evidence. Fresh serial runs pass.

The current bounded gate passes 12 positive configurations and 12 exact controls. The classifier covers 84 cases. All eight emergency scenarios and supporting regressions pass.

D2 remains pending. Client exit does not confirm writer termination, durable publication, or the complete emergency deadline. Cleanup, admission coverage, reserve evidence, and maintainer review remain open.

No commit, push, hosted dispatch, or soak occurs in this cycle.

## B13 commit and D2 expansion

The user authorized the B13 commit after staging its final evidence bindings. Commit `e6fdd343b` contains 34 files and preserves the pending gate statuses.

All commit hooks pass. Post-commit verification checks the source, evidence, executable modes, parent, and clean working tree. The local archive also passes verification.

The user then directs continued D2 expansion under the original plan. The other agent handles refactoring. No push or soak follows this commit.

## D2 guardian admission cycle B14

B14 starts from `e6fdd343b`. The fixture kills the guardian while the parent waits for a boundary probe that returns a valid sample.

Production RED admits an iteration. The matching formal control violates `AdmissionRequiresGuardian` with exit 12 before the correction.

The corrected driver checks guardian liveness after the probe and refuses admission. Production GREEN records zero iterations and one protection failure.

Formal GREEN completes with four distinct states. The [evidence package](../cbc-evidence/soak-d2-boundary-2026-09-09/README.md) retains the separate source snapshots and complete logs.

The combined gate passes 13 positive configurations and 13 exact controls. All nine emergency scenarios and supporting regressions pass.

B14 does not complete D2. Benchmark admission, live but stalled guardians, late crashes, cleanup, termination, durability, and reserve evidence remain open.

## D2 benchmark restart cycle B15

The user pushed `37ec7f71d` and requested the remaining branch work. This cycle starts from that revision and preserves the D2-first order.

Production RED requests the opening benchmark before checking a retained breach. The state file is absent, and the startup sample is valid.

The driver eventually records failure, but that later result does not make benchmark admission safe. The matching formal control violates `RetainedBreachPreventsBenchmark` with exit 12.

The correction adds the retained-marker check to opening benchmark admission. Production GREEN records no benchmarks, no iterations, and one failure.

The corrected model completes with six distinct states. The [B15 package](../cbc-evidence/soak-d2-benchmark-2026-09-09/README.md) retains the matched cycle and source snapshots.

The bounded gate passes 14 positive configurations and 14 exact controls. The classifier covers 98 cases. Ten emergency scenarios and supporting regressions pass.

The first metadata audit detects a hash of its own active build log. The raw archive retains that invalid record separately from behavioral evidence.

D2 remains in progress. Other benchmark admission paths, active benchmark supervision, stalled guardians, cleanup, termination, durability, and reserve evidence remain open.

No commit, push, hosted dispatch, or soak occurs in this cycle.

## D2 benchmark disk-admission cycle B16

The user confirmed B16 after publication of `7f0f46923`. The cycle uses that baseline and preserves the D2-first order.

Production RED requests the opening benchmark with 7000 MiB free, below the 8192 MiB admission threshold. The Docker fixture starts no node writers.

The formal control violates `BenchmarkRequiresBand` with exit 12 for the same sample and threshold. Both RED results precede the production correction.

The shared benchmark function now checks the disk sample before admission. Refusal sets the protection reason, ends further execution, and records one failure.

Production GREEN records no benchmarks and no iterations. Formal GREEN completes with 12 distinct states. The [B16 package](../cbc-evidence/soak-d2-benchmark-band-2026-09-09/README.md) retains the evidence.

Fifteen positive configurations, fifteen exact controls, 105 classifier cases, six routing scenarios, and eleven emergency scenarios pass. The supporting regressions also pass.

The first read-only inventory inspection used an absent key and raised `KeyError`. The raw archive records that helper error separately from behavioral evidence.

Published logs replace the local raw-root prefix with a documented placeholder. The original logs retain their bytes and separate digests.

B16 covers low-space opening admission only. Other sample cases, interleaved admission, active supervision, guardian progress, cleanup, termination, durability, and reserve evidence remain open.

D2 and acceptance remain pending. The workload and 45-second finalization wait remain unchanged. No commit, push, hosted dispatch, or soak occurs in B16.

## Remaining D2 scope and cycle B17

The user confirmed the remaining D2 implementation scope after commit `6e0b50f26`. This approval does not authorize a commit or soak dispatch.

No peer was available for coordination. The other agent retains refactoring ownership. This work uses narrow behavior changes in the existing driver and fixtures.

B17 checks disk monitoring during the opening benchmark. The baseline driver starts that benchmark before it creates the guardian.

The isolated fixture will change available space from 16384 MiB to 1024 MiB while benchmark startup remains active. It starts no nodes.

Production RED returned without a breach record or stop request during benchmark execution. It then admitted one iteration and recorded no protection failure.

The matching formal control violated `BenchmarkBreachObserved` with exit 12. Both RED results preceded the production correction.

The correction moves the existing opening benchmark block after guardian startup. Production GREEN records the breach and stop request before benchmark return.

The corrected driver admits no later iteration and records one protection failure. Formal GREEN completes with five distinct states.

Sixteen positive configurations, sixteen exact controls, 112 classifier cases, six routing scenarios, and twelve emergency scenarios pass. Supporting regressions also pass.

An initial fixture edit had an ambiguous text match. The first fixture run lacked executable snapshot modes and returned a degraded summary.

Both setup failures remain in the raw archive. Neither failure supplies behavioral RED. Corrected baseline permissions match the Git modes without changing source bytes.

The [B17 package](../cbc-evidence/soak-d2-benchmark-monitor-2026-09-09/README.md) retains source snapshots, original logs, and published digests. Verification runs execute serially.

The model assumes a completing guardian sample before benchmark return. It does not prove guardian progress, benchmark cancellation, writer termination, or durable publication.

D2 and acceptance remain pending. No commit, push, hosted dispatch, or soak occurs in B17.

## Additional opening benchmark admission coverage

Four additional production cases pass without another driver correction. They check 8192 MiB equality, 16384 MiB sufficiency, an unavailable sample, and disabled disk protection.

The unavailable sample refuses all work and records one protection failure. The other cases reach the benchmark Docker boundary and preserve subsequent iteration admission.

The Docker fixture deliberately returns failure without starting nodes. These cases do not assert successful benchmarks or finalization.

The expanded fixture still detects B17 on its frozen baseline. Sixteen positive configurations, sixteen exact controls, 112 classifier cases, and sixteen emergency scenarios pass.

Routing and supporting regressions also pass. The [coverage package](../cbc-evidence/soak-d2-benchmark-cases-2026-09-09/README.md) retains fresh source snapshots and original logs.

B17 remains immutable historical evidence. Its earlier fixture and driver bindings refer to its retained snapshot, not the expanded fixture.

This coverage is not a new RED/GREEN repair cycle. Interleaved admission, guardian failure, benchmark cancellation, cleanup, termination, durability, and reserve obligations remain open.

D2 and acceptance remain pending. No commit, push, hosted dispatch, or soak occurs.

## D2 active benchmark cancellation B18

The user requested continued D2 completion from `59b90568c`. The baseline benchmark call waits synchronously without parent supervision.

B18 checks guardian death and a disk breach while a benchmark client ignores termination requests. An external fixture observer releases the client after eight seconds.

Both production RED results matched separate formal controls before the correction. Each control violated `BenchmarkCancellationObserved` with exit 12.

The corrected driver supervises a timed benchmark command group. It cancels that group on a guardian fault and records one protection failure.

Both production cases publish failure before fixture release and leave no active benchmark client. Formal GREEN completes with fourteen distinct states.

No peer was available, and the other agent retains refactoring ownership. These results do not confirm node termination or complete D2.

## D2 benchmark guardian admission B19

B19 uses the retained B18 correction as its source-bound baseline. Both opening and interleaved admission initially incremented the benchmark counter after guardian death.

The unchanged `GuardianAdmission` control supplies the matching `AdmissionRequiresGuardian` counterexample. Both production RED results precede the new admission guard.

The shared benchmark function now checks guardian liveness and the breach marker after the disk probe. It refuses admission before the benchmark counter changes.

Production GREEN preserves one protection failure in both cases. The interleaved case preserves its completed iteration. Formal GREEN has four distinct states.

The [combined package](../cbc-evidence/soak-d2-benchmark-supervision-2026-09-09/README.md) retains each cycle's source snapshots and RED/GREEN logs.

Seventeen positive configurations, eighteen exact controls, 126 classifier cases, six routing scenarios, and twenty emergency scenarios pass.

The verification command reached its 600-second tool limit during release regressions. The original partial run remains unchanged, and the remaining checks passed separately.

A source navigation tool could not parse the Bash file. Direct source reads supplied the required inspection. Neither tool limitation supplies behavioral RED.

D2 and acceptance remain pending. Guardian progress, cleanup ownership, confirmed writers, durability, aggregate deadlines, and reserve evidence remain open.

No commit, push, hosted dispatch, or soak occurs in these cycles.

## D2 guardian progress B20

The user requested the remaining tasks after publishing `8ab599e5c`. The branch and worktree were current and clean at the start.

B20 suspends a live guardian during an active benchmark. The existing liveness check cannot detect missing progress.

The fixture checks client cancellation and failure publication before releasing the suspended guardian after twelve seconds. It starts no nodes.

No peer was available. The other agent retains refactoring ownership. D2 and acceptance remain pending.

Production RED left the benchmark client active and published no summary before release. The guardian remained alive in a suspended state.

The exact formal RED violated `StaleGuardianRequiresInterrupt` before production changes. The corrected driver checks an atomically replaced progress timestamp from elapsed host time.

Production GREEN cancels the client and publishes one protection failure before release. Formal GREEN has five distinct states.

ShellCheck reported an unused second uptime field during the correction. The field now uses `_unused`. This lint finding is not behavioral RED.

## D2 active iteration progress B21

B21 starts from the source-bound B20 correction. Its suspended guardian initially allowed the iteration client to remain active until fixture release.

The unchanged progress model supplied the matching RED. The production correction adds the progress check to active iteration supervision.

Production GREEN cancels the client and publishes one protection failure before release. Formal GREEN has five distinct states.

## D2 admission progress B22

B22 starts from the source-bound B21 correction. Its fixtures pause the driver and guardian during a valid disk probe.

After nine seconds, the observer verifies expired progress and resumes the driver first. Both baseline paths admit work before active supervision can stop it.

The matching formal control violates `StaleProgressPreventsAdmission`. The correction checks progress before benchmark and iteration counters change.

Both production GREEN cases refuse all work and record one protection failure. Formal GREEN has six distinct states.

The [combined package](../cbc-evidence/soak-d2-guardian-progress-2026-09-09/README.md) retains all three source-bound cycles and four production scenarios.

Nineteen positive configurations, twenty exact controls, 140 classifier cases, six routing scenarios, and twenty-four emergency scenarios pass. Supporting regressions also pass.

The silence limit defaults to ten seconds and permits 8 through 30 seconds. The fixtures use eight seconds. This component limit is not the complete response deadline.

D2 and acceptance remain pending. These cycles do not prove writer termination, durable publication, reserve bounds, or every scheduling and metadata fault.

External commit `706b11b6e` appeared during evidence preparation. Its parent is the B20 baseline, and its executable source matches the tested snapshots.

The evidence builder stopped on the changed HEAD before publishing its manifest. The original error remains retained as a verification interruption, not behavioral RED.

Hooks for the external commit remain unattested by this session. The assistant creates no replacement commit or amendment.

## D2 disk hygiene deadline B23

B23 starts from `5549561e1`. The fixture holds a cleanup client inside the admission band and observes it before release after five seconds.

Production RED leaves the client active without a summary at observation. The formal control violates `HygieneWithinBudget` with exit 12 before the production correction.

The correction places the unchanged cleanup commands under one deadline and one second of kill grace. Failed cleanup now refuses admission and records one protection failure.

Production GREEN cancels the client and publishes refusal before release. The formal configurations reuse the unchanged `DiskStopDeadline` model with an explicit invariant alias.

The positive configuration has five distinct states. The model covers cancellation, while the production fixture separately checks local failure publication.

The [evidence package](../cbc-evidence/soak-d2-hygiene-2026-09-09/README.md) retains the matched cycle and the complete regression results.

Twenty positive configurations, twenty-one exact controls, 147 classifier cases, six routing scenarios, and twenty-five emergency scenarios pass. All supporting regressions pass.

Executable inputs match the snapshot taken before the combined verification. The actual TLC logs were retained before the classifier and routing tests ran.

The user committed and pushed the correction as `9c99de84e`. Its executable source matches the tested snapshots. This session does not attest its hooks.

The configured hygiene budget defaults to ten seconds. It is not a measured complete response deadline or a reserve justification.

D2 remains pending. Cleanup ownership, daemon operations, detached descendants, durable publication, other faults, hosted checks, and maintainer review remain open.

## D2 disk settings B24

B24 starts from `9c99de84e`. Three digit-only configurations exceed individual or combined signed arithmetic limits and still permit workload admission.

Separate production RED cases retain the oversized floor, oversized band, and overflowing sum. The initial formal control violates `AdmissionRequiresValidDiskSettings` with exit 12.

The correction normalizes decimal strings and checks their ranges before arithmetic. It also checks the band against the maximum minus the floor.

All three production GREEN cases return configuration error 2 before admission. They produce no soak summary because configuration validation precedes the run.

ShellCheck flagged the initial numeric-looking string comparison. An equal text prefix makes the lexical comparison explicit, and C collation keeps that comparison stable.

The initial recursive model reached its 120-second positive verification limit. That result is not behavioral RED. Its original source and logs remain unchanged.

The replacement model uses explicit decimal column steps. Its fresh negative control returns 12 on the same invariant, and its positive run has 88 distinct states.

Two valid maximum-value cases pass on both baseline and corrected source. The expanded fixture also reproduces all three original faults against the frozen baseline.

Twenty-one positive configurations, twenty-two exact controls, 154 classifier cases, six routing scenarios, and thirty emergency scenarios pass. All supporting regressions pass.

The [evidence package](../cbc-evidence/soak-d2-settings-2026-09-10/README.md) separates invalid configuration refusal from maximum-value characterization and retains the model timeout.

External commits `4e9dd432b` and `556b944f3` contain the correction and final verification inputs. Those inputs match the pre-verification snapshot. Their hooks remain unattested by this session.

B24 does not complete D2. Other input faults, cleanup ownership, complete shutdown, durable publication, reserve bounds, hosted checks, and maintainer review remain pending.

## D2 temporary session preservation B25

B25 starts from `8eba1e4a7`. That evidence commit formatted the B24 fixture and left its current inventory binding stale.

The stale binding is not behavioral RED. The B25 combined suite verifies the current fixture, including all five B24 cases.

The new ownership fixture creates an old unowned directory with an active writer. Production RED deletes the directory while the writer retains its open file.

The exact formal control exits 12 on `UnownedSessionPreserved`. The correction removes age-only deletion and preserves the temporary session.

Production GREEN retains the session data and records disk refusal without admitting work. The corrected model has four distinct states.

The first supporting driver regression still required deletion of an old unowned directory. A retained diagnostic trace identifies that obsolete assertion.

The updated assertion requires preservation. The complete supporting suite passes without a further production correction.

Twenty-two positive configurations, twenty-three exact controls, 161 classifier cases, six routing scenarios, and thirty-one emergency scenarios pass.

The [evidence package](../cbc-evidence/soak-d2-cleanup-session-2026-09-10/README.md) retains both behavioral counterexamples and the regression correction.

External commit `d64ae3bbf` contains the tested executable inputs. Their hashes match the verification snapshot. Its hooks remain unattested by this session.

Preserving unowned data can cause earlier disk refusal. Safe reclamation requires ownership and termination evidence, not a longer age threshold.

Other D2 faults, Docker ownership, complete shutdown, durability, deadline bounds, reserve bounds, hosted checks, and maintainer review remain pending.

## D2 cleanup failure preservation B26

B26 starts from `d64ae3bbf`. Five external command faults expose suppressed cleanup errors followed by sufficient disk samples.

The baseline admits work after each failed container listing, container removal, network prune, image prune, or builder prune. The formal control violates `CleanupFailurePreventsAdmission`.

The correction retains each failure across the cleanup group and enables pipeline failure detection. The existing failed-hygiene path refuses admission and records one protection failure.

All five production GREEN cases pass. The corrected formal configuration has 42 distinct states.

Partial and sufficient reclamation cases pass on both baseline and corrected source. These successful-cleanup cases provide characterization, not additional repairs.

Twenty-three positive configurations, twenty-four exact controls, 168 classifier cases, six routing scenarios, and thirty-eight emergency scenarios pass.

The [evidence package](../cbc-evidence/soak-d2-cleanup-outcome-2026-09-10/README.md) retains the matched failures, characterization cases, and complete regression logs.

External commit `14ffb4d3a` contains the tested executable inputs. This session does not attest its hooks.

D2 remains pending. Other faults, Docker ownership, image preservation, complete shutdown, durable publication, deadline and reserve bounds, hosted checks, and maintainer review remain open.

## D2 real-system cycles B27–B29

The [evidence package](../cbc-evidence/soak-d2-real-system-2026-09-10/README.md) records three separate failures and corrections on a disposable diagnostic virtual machine.

B27 reproduces deletion of an unrelated stopped container, unused network, and reserved fixture image. Read-only hygiene preserves all three and retains disk admission refusal.

B28 reproduces a Docker writer that survives driver `SIGTERM`. Exit cleanup now requests writer termination. The post-exit check confirms termination and stable file contents.

B29 kills the driver process group with `SIGKILL`. The fixture supervisor stops the surviving Docker writer before recovery. This stop is not production crash-time shutdown evidence.

The baseline resumes work with zero failures. The correction records the uncommitted iteration and retains one interruption failure across two refused restarts.

The first B29 positive model had an incomplete successor because a Boolean assignment lacked parentheses. The corrected model passes both repeated controls.

The final gate passes 26 positive configurations and 27 exact controls. The classifier passes 189 cases, and routing passes six scenarios.

All 38 emergency scenarios, three composed real-system fixtures, and supporting regressions pass. Actual TLC logs were retained before classifier mocks ran.

External commits `3b7904eb` and `f4111f6b` contain the tested corrections. This session does not attest their hooks or change Git history.

Test evidence was retrieved before the diagnostic machine's termination request. Other workflows and machines were left unchanged. No node workload or soak ran.

D2 and acceptance remain pending. Ownership-safe stops, safe reclamation, other shutdown paths, additional crash windows, power-loss durability, deadline and reserve bounds remain open.

Hosted verification, required-check enforcement, and maintainer review remain open. The workload and 45-second finalization wait remain unchanged.

## D2 Docker stop cycles B30–B31

The [evidence package](../cbc-evidence/soak-d2-owned-stop-2026-09-10/README.md) retains two separate production and formal RED/GREEN cycles.
B30 reproduces termination of an unrelated Docker writer with a matching name prefix.
The production wrapper attaches an owner label at creation, and the shared stop helper verifies that label before selecting full identifiers.
Both Docker `run` and Compose launch cases pass.

B31 rejects the Docker stop command with exit 42 while the workload writer remains active.
The baseline records zero failures and loses the stop outcome.
The correction retains one interruption failure and explicit unconfirmed termination.
This result does not prove writer termination.

The composed gate passes 28 positive configurations and 29 exact controls.
The classifier covers 203 cases, routing covers six scenarios, and all 38 emergency shim scenarios pass.
B27–B29, both B30 launch cases, B31, and the supporting regressions also pass against the corrected source.
The combined supporting command reached its tool limit, and the remaining checks passed separately.

The real-system evidence was retrieved before the diagnostic VM reached `TERMINATED`.
A later regular-file-only archive check rejected one expected crash-fixture FIFO.
The original archive and special-entry record remain retained.
The termination observation does not identify the shutdown mechanism.

External commits `f3366cfab` and `463992ed1` contain the tested source and verification registrations.
Current inputs match the retained runtime snapshot.
This session does not attest those commits' hooks or create another commit.

Host-process ownership, memory-pressure paths, other launch forms, late creation, failed storage, and complete shutdown remain open.
D2, hosted verification, maintainer review, claim discharge, and acceptance remain pending.
No node workload or soak ran, and the finalization wait remains 45 seconds.

## D2 host stop cycles B32–B33

The user requested full D2 completion.
The [host stop evidence](../cbc-evidence/soak-d2-host-stop-2026-09-11/README.md) records two additional local corrections.
The broader gate remains pending.

B32 reproduces unrelated node and client termination during driver exit.
The correction gives workload processes an owner value and uses Linux process descriptors for selected termination.
B33 reproduces the memory path's separate unsafe selector.
The memory path now uses the shared stop helper and reports unconfirmed termination.

Both production controls return one, and their exact formal controls return 12.
The common corrected model has two states.
The final gate passes 29 positive configurations and 31 exact controls.
All 40 emergency cases, 217 classifier cases, six routing scenarios, and supporting regressions pass.

The first B32 composed command reached its tool limit during classifier tests.
The actual model logs remained intact, and the separate classifier run passed.
An older benchmark fixture required a return callback after client termination.
The corrected fixture accepts observed termination while retaining its breach, stop-request, failure, and refusal checks.

The first memory fixture armed its fault before it observed a healthy sample.
That setup result returned two and is not behavioral RED.
The corrected fixture waits for a healthy sample and retains matched RED/GREEN results.

The tests use restricted containers with private process namespaces and no host Docker socket.
No new VM, real node workload, or soak ran.
The historical B30–B31 evidence remains unchanged despite staged formatting changes and the later documentation commit.

Full D2 discharge requires D3 all-writer growth and reserve evidence.
The prior diagnostic scope excludes real node workloads.
I requested authorization for bounded D3 node diagnostics, without a soak.
Observability prerequisites, remaining local faults, durable evidence, the complete deadline, and maintainer review remain required.

A separate export check found that `e821517e0` omits 57 B30–B31 TLC streams because of the global `*.log` ignore rule.
The exported inventory rejects the missing stream.
Package-local ignore exceptions preserve the existing evidence bytes and permit later authorized staging.
No commit or push occurred in this session.

## D2 native memory preference B34

The user requested continuation after the bounded D3 scope question.
Local D2 faults remain first, and node diagnostics still require observability prerequisites and bounded safety controls.
A soak remains outside this authorization.

External commit `e2321eafe7ccb6efcf9ce29866b690ff84853ab0` contains the prior source and evidence.
The current input export validates, and all inventory inputs are tracked.
This session does not attest the external commit hooks.

The [B34 evidence](../cbc-evidence/soak-d2-oom-ownership-2026-09-11/README.md) reproduces an unrelated native-process preference change from zero to 1000.
The exact formal RED violates `UnownedPreferencesPreserved` with exit 12 before the correction.
The correction checks inherited ownership through an opened process directory and writes through the same directory descriptor.
Production GREEN preserves unrelated preferences and still sets the workload preference to 1000.

The positive model has two distinct states.
Thirty positive configurations, 32 exact controls, 224 classifier cases, six routing scenarios, and 41 emergency cases pass.
Supporting regressions also pass.
No real node workload or new VM ran.

The unchanged Docker preference loop remains outside this native-process correction.
B35 benchmark recovery and B36 Docker preference ownership were unchecked at this stage.
D2, reserve bounds, durability, complete response deadlines, hosted checks, and maintainer review remain pending.

## B35 characterization and B36 Docker preference completion

The user disabled the confirmation extension and requested continuation.
External commits `b56a408a2` and `980285d4c` contain the Docker correction and formatting changes.
This session does not attest those commits or their hooks.
The current source archive includes the committed source and a current inventory overlay without changing claim or gate statuses.

The [B35–B36 evidence](../cbc-evidence/soak-d2-container-preference-2026-09-11/README.md) records 12 matched real-system outcomes.
B35 passes unchanged-production characterization and current-driver revalidation.
Two refused restarts preserve one failed benchmark stop while the writer remains running.
B35 has no production repair or formal RED.
A crash before any benchmark outcome or stop record remains a separate gap.

The B36 baseline changes unrelated preferences from zero to 1000 with both `run` and Compose.
The corrected driver preserves unrelated preferences at zero and still prefers the workload at 1000.
Both GREEN cases stop the workload writer and preserve the unrelated writer and its file growth.
The batch also revalidates B27–B31 against the current driver.

The composed gate passes 31 positive configurations and 33 exact controls.
All 64 actual TLC logs were saved before classifier and routing substitutes ran.
All 231 classifier cases, six routing scenarios, 41 emergency cases, and supporting regressions pass.
Seven primary language-server checks report no diagnostics.
The runtime audit compares 211 executed inputs with the current source.

The final archive contains 421 unique regular files with matching digests after extraction.
Archive format and repeated-preparation failures occurred before VM launch.
The first new VM lacked Ruby and stopped before behavior tests.
Both new diagnostic VMs were observed terminated after evidence retrieval.
The original expired-VM transfer and B35 working-directory failure remain setup evidence, not behavioral RED.

No Git commit, push, hosted dispatch, real node workload, or soak occurred in this continuation.
Remaining work includes storage faults, crash windows, full launch ownership, durable publication, aggregate deadlines, and D3 growth and reserve bounds.
Conflicting caller preference options and legacy runner-survival comments remain review limitations.
D2 and acceptance remain pending.

## B37 benchmark crash recovery

The user requested D2 completion.
B37 reproduces new work after a driver crash during an active benchmark, before any outcome or stop record exists.
The production baseline exits 1, and the matching formal control exits 12 for `BenchmarkCrashRequiresRefusal`.
The correction records benchmark interruption state before launch and retains one failure across two refused restarts.
A failed exit stop sets the retained state immediately, so B35 does not count the interruption twice.

The [B37 evidence](../cbc-evidence/soak-d2-benchmark-crash-2026-09-11/README.md) records the corrected production pass and four-state model pass.
Ten real-system GREEN cases revalidate B35, Docker preferences, ownership, rejected stops, shutdown, and iteration recovery.
The composed gate passes 32 positive configurations and 34 exact controls.
All 66 actual model logs were saved before substitutes ran.
All 238 classifier cases, six routing scenarios, 41 emergency cases, and supporting regressions pass.
Five primary language-server checks report no diagnostics.

The input audit compares 217 verification inputs with the current source without a mismatch.
Archive validation precedes extraction and verifies 376 unique regular files with matching extracted digests.
The ninth diagnostic virtual machine was observed terminated after retrieval.
The fixture stops its recorded benchmark client group after the crash, but the Docker writer remains active through both refused restarts.
Fixture cleanup is not production shutdown evidence.

External commits `69332f716` and `64b02472c` contain earlier evidence and the B37 correction.
This session does not attest those commits or their hooks.
The bootstrap archive retains the earlier source snapshot, while separate hashed payloads identify the executed driver versions.
No assistant commit, push, hosted dispatch, real node workload, or soak occurred in this cycle.

Storage faults, other crash windows, full creation ownership, writer termination, durable publication, aggregate deadlines, and reserve bounds remain open.
Hosted verification, enforcement, maintainer review, and claim ratification also remain open.
D2 and acceptance remain pending.

## B38 and B39 crash monitor work

The user authorized implementation in the proposed priority order.
B38 addresses the observed Docker writer that survives a driver process-group crash.
The production fixture exits 1 before correction, and the exact `DriverCrashStopsOwnedWriter` control exits 12.
An independent crash monitor waits on the parent process descriptor and invokes the existing ownership-checked stop helper.
The corrected fixture stops the owned Docker writer and preserves the unrelated writer before fixture cleanup.

The composed emergency suite then finds a separate regression.
The B13 failure summary already exists, but a new stalled Docker client remains below the crash monitor.
B39 acknowledges completed exit handling and avoids that duplicate stop.
The exact `HandledExitHasNoExtraStop` control exits 12 before correction.
The unchanged production regression and both three-state models pass after correction.
An acknowledgment does not establish successful writer termination.

The [combined evidence](../cbc-evidence/soak-d2-crash-stop-2026-09-11/README.md) retains both RED/GREEN cycles and the intermediate regression.
The final source passes 11 real-system cases, 41 emergency cases, 34 positive configurations, 36 exact controls, and 252 classifier cases.
Six routing scenarios and the supporting regressions also pass.
All 70 actual model logs were saved before verifier substitutes ran.
The 228 verification input snapshots match the final source.

The two diagnostic archives contain 524 and 418 verified regular files.
Each archive passed validation before extraction, and every extracted digest matched.
Both diagnostic virtual machines were observed terminated after retrieval.
All eleven diagnostic virtual machines used so far have termination observations.
The earlier bootstrap archive and separate runtime payloads retain distinct source roles.

The unchanged B37 fixture fails its old surviving-writer assumption after B38.
Its adapted Docker boundary rejects kill requests explicitly and retains the same restart-admission and failure-count assertions.
The initial formal batch stopped at a changed-HEAD guard before TLC ran.
That guard result is not a behavioral RED.
External commits `5a211fb5c` and `bef79ba48` contain the production changes, and this session does not attest their hooks.
No assistant commit, push, hosted dispatch, node workload, or acceptance soak occurred in these cycles.

Storage faults, monitor health, late creation, other launch and crash paths, safe reclamation, aggregate deadlines, and reserve bounds remain open.
Hosted verification, enforcement, maintainer review, and claim ratification remain open.
D2 and acceptance remain pending.

## B40 monitor death during an iteration

The user requested continued D2 work and shutdown evidence.
The native fixture confirms monitor death through a process file descriptor.
The baseline driver stays active, and its owned writer continues writing.
The production fixture exits 1, and the exact `MonitorDeathStopsOwnedWriter` control exits 12.
This failure is not a timeout-only result.

The correction retains the monitor child PID and checks the driver's running child jobs during an active iteration.
The existing interruption path stops the owned writer and records one failure.
The unchanged fixture preserves an unrelated writer and refuses two restarts with retained counters `[1, 1, 0, 0]`.
The five-state model passes, and construction proofs remain not applicable.
The [B40 package](../cbc-evidence/soak-d2-monitor-death-2026-09-11/README.md) retains the cycle evidence.

The combined verification passes 35 positive configurations, 37 exact controls, 259 classifier cases, and six routing scenarios.
All 72 actual TLC logs were saved before verifier substitutes ran.
All 42 emergency cases, 11 real-system cases, and supporting regressions pass.
The real-system archive contains 418 unique regular files and 2,107,137 bytes.
The archive passed validation before extraction, and every extracted digest matches.

The diagnostic virtual machine was observed terminated after retrieval.
All twelve recorded diagnostic virtual machines now have termination observations.
External commit `ab9519a5d` contains the correction, and this session does not attest its hooks.
This session did not commit, push, dispatch hosted checks, run a node workload, or start an acceptance soak.
The workload and 45-second finalization wait remain unchanged.

Monitor failure during benchmarks and admission boundaries remains open.
Inherited output pipes, simultaneous failures, late creation, storage faults, failed stops, and complete writer discovery remain open.
Aggregate deadlines, reserve bounds, hosted enforcement, human review, and acceptance remain open.
D2 and all claim discharges remain pending.

## B40 evidence packaging change

The user required digest-only reruns before the B40 manifest was built.
The builder no longer creates a package-local `.gitignore`.
The new package retains whitespace attributes without overriding the root ignore rules.
The packaging work does not modify historical packages, manifests, published sets, or commits.

Rerun patterns are `composed-*`, `final-*`, `supporting-*`, `emergency-*`, and `tlc-*.log`.
Only the exact filename `final-real.txt` is an exception to the `final-*` rule.
Current-cycle real-system results use the cycle prefix.
Prior-cycle retrievals remain digest-only, even when the current batch repeats their tests.

Published cycle logs use `.txt` filenames without changing their content.
The builder does not copy reruns into the package or add published entries for reruns.
The claim inventory binds only the new published files and source inputs.

The raw archive is `[EVIDENCE_ROOT]/raw-streams.tar.gz`.
Its manifest inventory retains every rerun stream with `raw_path`, `raw_sha256`, and `raw_bytes`.
The export check accepts those raw records without package files.
A missing listed published file still fails the export check.
The archive-inventory validator checks all raw members, including digest-only reruns.

The shared validator is `scripts/ci/check-soak-evidence-package.rb`.
All 19 packaging regression cases pass, including missing-file, digest, byte-count, JSONC, and publication-rule checks.
The tests reject the broad `final-real*` exception and prior-cycle retrievals that use the current-cycle prefix.
The existing claim-inventory validator and TLA+ gate code remain unchanged by the packaging work.

The historical-byte audit found 878 concurrent staged deletions under `docs/cbc-evidence/` before the new package was built.
This session did not create or reverse those deletions.
The workspace inventory requires the missing historical files.
The new package checks remain separate from this unresolved workspace condition.

The user clarified the exact exception after the first B40 draft was built.
This revision preserves that draft outside Git before changing the current package.
The 59 current-cycle batch and lifecycle streams use the `b40-real-system--` prefix.
The 405 retrieved streams for B27 through B38 now have raw records only.
The raw archive and its recorded member bytes remain unchanged.
The revision records new packaging-source hashes without changing the original build-input records.

## Authorized rerun removal and build checks

Commit `5e1842688` removed 878 rerun files and their candidate bindings.
The user then authorized removal of the remaining files and an update to the build.
The review found 187 remaining `final-real-*` files in the B38/B39 package.
A package-local ignore rule allowed 17 of those logs.
The other 170 files matched the root ignore rules.

The removal preserves all 187 published files in `[EVIDENCE_ROOT]/published-reruns.tar.gz` before deletion.
The archive passed a complete member, digest, and byte-count check.
The [retention record](../claims/soak-evidence-retention.jsonc) records the archive identity and each removed file.
The archive contains previously published bytes, not a new execution.
The original historical manifests and `docker/.env` remain unchanged.

The repository check rejects rerun files and rerun bindings regardless of package-local ignore rules.
The repository check requires registration and current publication digests for each new-format package.
The Lint job runs that check and the packaging regression suite.
The existing claim-inventory validator and TLA+ gate code remain unchanged.

The inventory removes the 187 obsolete bindings and registers the B40 package as historical evidence.
The B40 manifest retains the original executed-source hashes.
Commit `5e1842688` changes whitespace in the native fixture, so the historical run does not attest the current fixture bytes.
Current publication bindings do not establish a new production test result.
D2, all claim discharges, hosted verification, and acceptance remain pending.

The 28 packaging regressions, inventory validation, ten JSONC regressions, and workflow checks pass locally.
The repository check confirms that no rerun files or candidate bindings remain.
The fresh formal run passes 35 positive configurations and 37 exact controls.
All 72 actual TLC logs remain outside Git.
The routing regressions also pass.
These local checks do not establish hosted execution or D2 completion.

## B41–B44 shutdown coverage

The shutdown work starts from `84a9c8fbde09639c88085afa21e61770bc4a022d`.
The workload, finalization semantics, and 45-second finalization limit remain unchanged.
The [shutdown package](../cbc-evidence/soak-d2-shutdown-2026-09-11/README.md) retains the selected results and source identities.
No virtual machine, hosted workload, or acceptance soak was launched for these cases.

B41 establishes a benchmark monitor-death defect before correction.
The owned native writer continues after confirmed monitor death while the unrelated writer also progresses.
The correction adds monitor health to the existing active-benchmark interruption path.
The unchanged fixture and matching five-state model pass.
Two refused restarts retain counters `[0,1,1,1]` for iterations, failures, benchmark segments, and benchmark failures.

B42 establishes an admission defect in opening benchmark and iteration probes.
The external probe confirms monitor death and then returns 16384 MiB.
The baseline admits work before its active-work check detects the dead monitor.
The correction checks the monitor before admission and honors an existing breach at the iteration boundary.
Both unchanged fixtures and five-state configurations pass with no admission and one retained failure.

B43 establishes an output-drain defect after interruption.
Device and inode observations confirm that the detached owned writer holds the iteration output FIFO.
The baseline waits for output completion before it stops that writer.
Moving the existing owned-writer stop before the drain wait corrects the selected case.
The unchanged fixture and four-state model pass while the unrelated writer continues.
Failed stops and other descriptor holders remain outside this correction.

B44 remains an open controller-loss defect.
The fixture suspends both controllers before it kills them through process descriptors.
Neither controller can complete shutdown between the two deaths.
The final fixture allows 120 observation intervals before its writer-stability check.
The owned writer grows from 492 to 512 bytes after that observation, and the unrelated writer grows from 532 to 552 bytes.
The current model violates `ControllerLossStopsOwnedWriter` with TLC exit 12.

The B44 fixture revisions avoid two incorrect future assumptions.
Driver death must not require instantaneous termination by an independent supervisor.
A killed driver cannot write its initial summary, so recovery assertions use two actual restarts.
The initial sources and RED results remain intact.
The revised fixture and model retain a separate RED result with no production correction or GREEN result.
The passing combined suite does not register B44 as an accepted negative control.

The B41-only snapshot passes 36 positive configurations, 38 exact controls, 266 classifier cases, six routing scenarios, and 43 emergency cases.
Its unused chart build artifacts remain in the external snapshot rather than the regular-file evidence archive.
The later combined snapshot includes the B41–B43 production and gate changes.
The three later B44 diagnostic-file changes are recorded separately because the combined suite does not execute B44.
No executed production or passing-gate input was changed by those diagnostic revisions.

The combined snapshot passes 39 positive configurations, 41 exact controls, 287 classifier cases, and six routing scenarios.
All 46 emergency cases and the selected supporting regressions pass.
All 80 actual TLC logs identify the frozen source and expected result.
Language-server checks report no diagnostics for nine files, but five checks remain inconclusive.

Another actor created commit `cdc0a57f5` during verification.
The source audit finds only the three declared B44 diagnostic changes relative to the combined snapshot.
This session did not run or attest the commit hooks.

D2 shutdown completion requires a reviewed containment design that survives the specified controller failures.
The design must cover native descendants, Docker writers, late creation, inaccessible metadata, and failed stops without acting on unrelated writers.
Storage durability, aggregate deadlines, D3 reserve evidence, hosted verification, maintainer review, and acceptance remain pending.
Construction is not applicable to these Bash-driver models.
No claim is discharged.

### B44 containment proposal, 2026-09-11

This review starts from a clean tree at `1f9427958634516ab04f757688b0e2ad3376b718`.
The [controller-loss correspondence](../../formal/tlaplus/soak_disk/ControllerLoss.md#proposed-runner-containment) now contains a proposed runner containment contract.
The proposal includes a private Docker engine, explicit placement, pending creation, independent shutdown observation, and failure retention.
The glossary defines the run domain and creation fence.
These definitions do not establish an implemented protection mechanism.

The local host reports systemd 255, cgroup v2, and Docker's systemd cgroup driver.
These read-only observations do not verify a deployed containment boundary.
The Docker reference permits per-container overrides of daemon placement defaults.
The kernel reference does not establish a permanent creation fence from one cgroup kill.
A simple service wrapper or daemon default is therefore insufficient.

GitHub CLI access still fails because authentication is unavailable.
No new behavioral result, hosted execution, virtual machine, or acceptance run exists for this proposal.
Production and formal source files remain unchanged.
The next implementation requires confirmation of the proposed files, method, and service-manager survival assumption.
B44 remains RED, and D2 remains incomplete.

### B44 native subcycle, 2026-09-11

The user confirmed the proposed implementation scope and assigned commit and push work to another agent.
The [native subcycle](../../formal/tlaplus/soak_disk/NativeControllerLoss.md) starts from committed source `1f9427958634516ab04f757688b0e2ad3376b718`.
The new baseline adapter launches the unchanged driver without containment.
The matched RED confirms both controller deaths while the owned writer grows from 376 to 396 bytes.
The unrelated writer also continues.
The matching model violates `NativeControllerLossStopsOwnedWriter` with TLC exit 12.

The corrected native launcher uses a separate system service with control-group termination and no automatic restart.
The unchanged fixture confirms owned-writer termination while the unrelated writer continues.
Two actual restarts refuse admission and retain `[1,1,0,0]`.
The managed three-state model passes.
The original direct-launch B44 model and fixture remain separate, unresolved evidence.

The initial service environment arguments caused a setup failure, not behavioral RED.
A later invocation passed its behavioral assertions but failed outer-service cleanup.
The final matched pair uses identical outer-service settings and has invocation exits 1 and 0.
All five initial stages retain separate source and result identities.
The evidence archive was verified before the diagnostic virtual machine reached observed `TERMINATED` state.

Evidence review found that the initial fixture copied shell files without execute permissions.
Its restarts used the driver's fallback summary.
The reviewed fixture restores execute permissions without changing the fault or its assertions.
A second guarded runner establishes matched RED/GREEN results with normal summary publication on both actual restarts.
The reviewed observation records stable owned output at 2 bytes and unrelated output from 4 to 14 bytes.
Verified retrieval precedes observed termination of the second runner.

The combined snapshot passes 40 positive configurations, 42 exact controls, 294 classifier cases, six routing scenarios, and 46 emergency cases.
All 82 actual TLC logs identify the frozen source and expected result.
The supporting admission, driver, metrics, and summary regressions pass.
The combined snapshot does not execute the privileged native fixture.
Its later permission correction has separate reviewed native execution.
All combined reruns remain digest-only.

This native-only prototype is not connected to the normal soak workflow.
It checks placement after service start and does not establish a verified pre-admission handshake.
Private Docker containment, creation fencing, metadata failure handling, and general termination guarantees remain open.
The 45-second finalization wait and workload remain unchanged.
The [native evidence package](../cbc-evidence/soak-d2-native-containment-2026-09-11/README.md) records the bounded verification and its limitations.
B44, D2, all claim discharges, and acceptance remain pending.
