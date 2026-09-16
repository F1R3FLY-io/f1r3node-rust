# F1 finalization diagnostic methodology

## Purpose and scope

This document specifies the diagnostic run for gate F1.
The run investigates finalization delays and failures under the unchanged workload assertion.
When the evidence identifies a work-bound violation, the run records that bound and its input-size dimensions.
It supplies the work-bound obligation that gate F2 turns into a production and formal counterexample.

The run uses an isolated diagnostic runner with enforced writer limits when the reserve claim is open.
It does not treat a limited run as acceptance and does not overlap another soak.
It preserves the issue #24 load profile and the 45-second finalization assertion without relaxed limits.
A dominant timer alone does not prove an algorithmic cause, so the run separates throughput, queue residence, and repeated work.

## The observed bound

The issue reports finalization p95 at 96.3 seconds, with 147 deploys outside the 45-second limit.
Run `33707959088` recorded 752.6 milliseconds per merge call and 965 milliseconds per replay block in its worst iteration.
These figures name candidate stages but do not attribute the bound to an input-size dimension.
The run must distinguish an algorithmic cause from a backlog and from repeated work.

## Pinned workload contract

The execution source is harness revision `962effd17708192627bd249362761c0ccb1fd5fa`.
`test_deploy_throughput_and_finalization` calls `LifecycleTracker.wait_for_finalization` after each phase submits its deploys.
The selected finalization wait remains 45 seconds.
The test later asserts `total_unfinalized == 0`, not `finalization_p95 <= 45`.
Zero submission failures, node survival, and the existing all-node convergence check remain required.

Reported finalization duration spans submission to the tracker's observation of finalized status.
The phase wait does not impose a separate 45-second submission-to-finalization deadline on each deploy.
The diagnostic must preserve this distinction rather than silently change either the wait or the reported duration.
A sampled status-observation time is not the exact instant of consensus finalization.

The test reads results after the post-wait block-number and metrics probes.
The tracker continues to accept status observations during those probes.
The result is therefore not a frozen snapshot at the wait deadline.
The diagnostic must distinguish wait completion from result capture without changing the pinned behavior.

The percentile input excludes deploys whose finalization time is missing.
Terminal failed or expired deploys can end the wait, but they remain unfinalized in the result.
The percentile helper returns zeros for an empty input list.
The diagnostic must retain missing counts and terminal outcomes instead of interpreting those zeros as measured zero latency.
A low p95 cannot override an unfinalized-deploy failure.

`LOAD_TEST_TELEMETRY_ONLY` must remain unset or empty.
The pinned test treats any nonempty value, including `0`, as permission to skip finalization and convergence assertions.
The diagnostic must record the effective wait and assertion mode without publishing the complete process environment.

## Stage attribution

The run compares stage measurements for successful, failed, deferred, and incomplete block attempts.
A single elapsed time is not attribution.
Restricting the sample to finalized blocks would omit work that can explain the failure.
The diagnostic reports unresolved attribution rather than forcing one dominant stage.

| Stage | What to measure | Input-size dimension |
| --- | --- | --- |
| Carrier index | Engagement, row reads, fallback, ancestor metadata, and ancestor body counters | Ancestor set size and read count |
| Merge | Merge relation size, branch count, conflict adjacency entries, rejection options, and state actions | Adjacency-entry count and branch count |
| Replay | Spawn, reset, user work, system work, checkpoint calls, and phase durations | Deploy count and user-operation count |
| Queue | Proposal queue depth and queue residence time | Arrival rate against service rate |
| Storage | Read and write latency and inode and byte health | Storage pressure during the interval |

The run pairs each stage total with its input-size dimension.
It records per-node and per-label values, and it separates cumulative counters, histogram sums, histogram counts, and instantaneous values.
It computes interval deltas per node and label set.
It separates restart epochs and rejects a counter reset or a missing interval as work.

## Distinguishing the cause

The run compares algorithmic work, queue delay, repeated attempts, storage effects, and scheduling effects before it names a bound.
These causes can occur together.
Elapsed stage time can include asynchronous waits and does not equal processor time or operation count.
Increasing time per input item identifies a candidate explanation, not proof of an algorithmic cause.

The run compares passing and failing intervals with the same input dimensions and measurement definitions.
A work-bound explanation needs counted operations and a justified relation to the measured input size.
A queue explanation needs arrival, service, and residence observations, not depth alone.
Repeated-work attribution needs block and attempt identity rather than similar aggregate counts.
The diagnostic must distinguish expected retries from the work that violates the proposed bound.

The run does not infer a per-block bound from a mean or a median across iterations.
Finalization percentiles remain descriptive and do not replace the pinned assertion.
Stage percentiles require matched raw observations or a justified estimate with its error and bucket limits.
Histogram sums and counts alone do not provide stage percentiles.
The run avoids overlapping parent and child timers and records missing measurements as unavailable.

## Instrumentation coverage and requirements

The run uses the existing observability metrics first.
It adds instrumentation only for a confirmed measurement or correlation gap.
It does not select an optimization before the measurement.
Metric definitions and emission call sites do not establish complete collection or per-block attribution.

The initial source review uses commit `97e782fa4a7efe84d24cddf1ebd9cd6a35cd651a`.
The September 16 preparation checks node source `802bc98d96594e2151afd49b31b8250c86573125` and the pinned workload.
The reviews inspect [metric definitions](../../casper/src/rust/metrics_constants.rs), validation, replay, merge, driver, and queue code without running the node workload.
The [F1 task log](../work-logs/task-soak-f1-2026-09-16T03-23Z.md) records source bindings and verification limits.
The table uses Rust metric names.
The diagnostic must verify exported names, suffixes, units, labels, and process epochs against actual samples.

| Stage | Measurement | Source metric or probe | Evidence and remaining work |
| --- | --- | --- | --- |
| Merge | Conflict adjacency entries | `dag.merge.conflict.edges` | The merger records the sum of adjacency-set lengths, not unique undirected edges. |
| Merge | Branch count | `dag.merge.relation.branches` | The merger records the branch count. Per-block correlation remains unverified. |
| Merge | Relation size | `dag.merge.relation.items` | The merger records the merge-set size. Per-block correlation remains unverified. |
| Merge | Stage time | `dag.merge.total.time`, `dag.merge.conflict.time`, `dag.merge.branches.time` | Definitions exist. Collection and correlation remain unverified. |
| Replay | Stage time | `block.processing.stage.replay.time` | Checkpoint validation records the timer after `replay_block` returns through `Ok`. An outer `Err` bypasses recording. |
| Parent state | Stage time | `block.processing.stage.parents-post-state.time` | Checkpoint validation records the timer after computation returns, before it matches success or error. |
| Replay | Phase time and reset calls | `block.replay.deploy.evaluate.time`, `block.replay.phase.reset.time`, `block.replay.phase.reset.calls` | Replay code records these metrics. Collection and correlation remain unverified. |
| Replay | Input deploy counts | `block.replay.phase.user-deploys.work`, `block.replay.phase.system-deploys.work` | Histograms record input lengths per replay invocation. They do not identify blocks. |
| Storage | Block-store time | `block.processing.stage.storage.time` | `check_if_well_formed_and_store` records successful store completion. A store error bypasses recording. |
| Storage | DAG insertion time | `dag.insert.time` | The definition exists. Collection, outcome coverage, and correlation require verification. |
| Storage | Pressure | Driver `disk_free_mb` uses `df -Pm` | The probe measures free space. Inode coverage and interval correlation remain unverified. |
| Queue | Pending proposer requests | `proposer.queue.pending` | Setup and proposer paths emit reservation, dequeue, retry, and failed-send updates. Collection remains unverified. |
| Queue | Block-processing channel occupancy | `block-processing.queue.pending` | Setup samples used channel capacity on a five-second target interval. Collection and residence time remain unverified. |
| Queue | Residence time | Coverage not established by this review | Verify arrival-to-service measurement before adding a timer. |
| Carrier index | Watermark eligibility | `block.validation.repeat-deploy.carrier.watermark-engaged`, `block.validation.repeat-deploy.carrier.watermark-not-ready` | Validation increments these counters. Engagement does not establish a complete scan skip. |
| Carrier index | Probe outcomes | `block.validation.repeat-deploy.carrier.index-absence`, `block.validation.repeat-deploy.carrier.index-hit`, `block.validation.repeat-deploy.carrier.index-read-failure` | Validation increments these counters. Read failure includes watermark and index-probe failures. |
| Carrier index | Probe and fallback operations | `block.validation.repeat-deploy.carrier.row-reads`, `block.validation.repeat-deploy.carrier.fallback-scan` | Validation counts probe attempts and fallback scans. These are not physical storage reads. |
| Carrier index | Ancestor operations | `block.validation.repeat-deploy.ancestor.metadata-visits`, `block.validation.repeat-deploy.ancestor.body-reads` | Validation counts traversal operations. Unique ancestor-set size and block correlation remain unverified. |

## Existing carrier and replay instrumentation

The [validation caller](../../casper/src/rust/validate.rs) emits carrier counters around watermark checks, index probes, and fallback traversal.
The absence of metric macros in the storage module does not establish their absence from validation.
The prior draft incorrectly stated that carrier engagement, fallback, and read counters did not exist.

Watermark engagement records eligibility to use the index.
An index absence can remove a signature from the scan set.
An index hit keeps the signature in that set.
Index read failures also retain scan work.
The fallback counter therefore does not mean only an unreadable-read refusal.
These counters measure validation operations, not physical storage reads or unique ancestors.

The [replay caller](../../casper/src/rust/rholang/replay_runtime.rs) records user and system input lengths before reset or deploy execution.
The tracing field `n_user` is not the only count instrumentation.
Histogram sums represent recorded input deploy totals, while histogram counts represent replay invocations that reach those recording sites.
These values do not count successfully completed deploys.
A failure can leave input-count observations without corresponding phase-completion observations.

The reviewed carrier and replay call sites specify a `source` label, but no block identifier.
Collection must preserve node identity, labels, and process epochs.
A scrape interval can contain several blocks or repeated attempts.
Separate cumulative totals cannot identify which input count belongs to which block duration.
Per-block attribution remains open despite the existing instrumentation.

## Queue and timer interpretation

[Node setup](../../node/src/rust/runtime/setup.rs) increments the proposer pending count before it awaits the channel send.
The [proposer instance](../../node/src/rust/instances/proposer_instance.rs) decrements the count after receive, before interval delay and proposal execution.
Retry and failed-send paths also update the gauge.
A zero pending count therefore does not establish an idle proposer.
The count is not a queue-residence measurement.

The block-processing queue gauge uses channel capacity rather than per-message enqueue timestamps.
Its configured sampling interval does not establish complete observations of short peaks.
Per-attempt queue residence still requires verified timestamps and identity.

[Block processing](../../casper/src/rust/blocks/block_processor.rs) records setup, validation, and storage timers only after their respective paths reach the recording site.
[Checkpoint validation](../../casper/src/rust/util/rholang/interpreter_util.rs) records parent-state duration before it matches the returned computation result.
The outer replay timer instead follows a fallible replay call.
A recorded replay duration does not establish a valid block, and an absent duration does not establish zero work.
Input counts, phase durations, outcomes, and attempts need matching coverage before comparison.

## Requirements before per-block attribution

The diagnostic must verify collection and correlation before it introduces duplicate metrics.
The following requirements do not authorize a diagnostic run or complete O1:

1. Verify collection of the existing carrier counters and replay work histograms.
2. Preserve units, node identities, label sets, process epochs, and missing intervals.
3. Correlate block identity, input counts, stage durations, and attempt outcomes through bounded diagnostic records.
4. Distinguish watermark eligibility, index absence, index hit, read failure, and fallback traversal.
5. Establish ancestor-set size separately from traversal-operation counts.
6. Verify queue residence, inode coverage, and byte-health correlation before adding instrumentation for confirmed gaps.
7. Measure collection overhead and retained-record growth before the guarded diagnostic.

Use bounded records for block correlation rather than unbounded block-hash labels on exported metrics.
Each record needs a node, process epoch, block identity, attempt identity, stage, outcome, and measurement boundary.
Monotonic timestamps from different processes or hosts cannot be subtracted without a verified clock relation.
Record and byte limits need explicit truncation counts and an incomplete result when required records are lost.
Do not record deploy bodies, private keys, or complete process environments.

The diagnostic must retain the distinction between missing data and zero work.
It must not change finalization behavior or relax the 45-second assertion.
A current O1 pass supplies evidence only for its recorded source, coverage, and exercised paths.
Per-block attribution requires the additional correlation evidence above.

## Output and exit

A conclusive run identifies the responsible work and its measured input-size dimensions.
It records the violated operation bound and the workload shape that reproduces the failure.
It retains raw observations, attempt outcomes, missing records, and competing explanations.
A resource stop, insufficient correlation, or failure to reproduce yields an incomplete or inconclusive result, not a chosen optimization.

The dominant stage and its bound become the gate F2 obligation.
Gate F2 writes one generated property through the real production entry point for that bound.
Gate F2 retains the failing seed and the minimized fixture, then adds the matching formal configuration with explicit work accounting.
The exit for F1 is a reproducible workload shape and a justified operation-bound obligation, not a chosen optimization.

## What this document does not do

This document specifies the run and does not execute it.
The run needs the isolated diagnostic runner, the O1 observability prerequisites, and separate authorization.
It does not name the stage, which only the executed run can do.
The repair, the formal counterexample, and the semantic bridge are gates F2 and F3, and they belong to the Casper maintainer.

D2, D3, F1, F2, F3, and claim discharge remain pending.
