# F1 finalization diagnostic methodology

## Purpose and scope

This document specifies the diagnostic run for gate F1.
The run identifies the dominant work stage that pushes finalization past the 45-second limit.
It records the violated operation bound and its input-size dimensions.
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

## Stage attribution

The run attributes finalization latency to one dominant stage across every candidate block.
A single elapsed time is not attribution.
The run records each stage separately at every finalized block.

| Stage | What to measure | Input-size dimension |
| --- | --- | --- |
| Carrier index | Engagement, row reads, fallback, ancestor metadata, and ancestor body counters | Ancestor set size and read count |
| Merge | Merge relation size, branch count, conflict edges, rejection options, and state actions | Conflict-edge count and branch count |
| Replay | Spawn, reset, user work, system work, checkpoint calls, and phase durations | Deploy count and user-operation count |
| Queue | Proposal queue depth and queue residence time | Arrival rate against service rate |
| Storage | Read and write latency and inode and byte health | Storage pressure during the interval |

The run pairs each stage total with its input-size dimension.
It records per-node and per-label values, and it separates cumulative counters, histogram sums, histogram counts, and instantaneous values.
It computes interval deltas per node and label set.
It separates restart epochs and rejects a counter reset or a missing interval as work.

## Distinguishing the cause

The run must separate three explanations before it names a bound.
An algorithmic cause shows a stage time that grows faster than its input-size dimension.
A throughput cause shows a stable per-item time with a rising queue depth and residence time.
A repeated-work cause shows the same input processed more than once across iterations.

The run compares passing and failing intervals on the same axes.
It plots each stage time against its input-size dimension across both intervals.
A stage whose time per unit of input rises with input size is the algorithmic candidate.
A stage whose time per unit stays flat while the queue grows is a throughput candidate, not an algorithmic one.

The run does not infer a per-block bound from a mean or a median across iterations.
It uses the 95th percentile for the finalization assertion and reports the exact percentile for each stage.
It avoids overlapping parent and child timers, which would double-count work.
It treats a missing metric as missing, not as zero work.

## Instrumentation coverage and requirements

The run uses the existing observability metrics first.
It adds instrumentation only for a confirmed measurement or correlation gap.
It does not select an optimization before the measurement.
Metric definitions and emission call sites do not establish complete collection or per-block attribution.

The source review uses commit `97e782fa4a7efe84d24cddf1ebd9cd6a35cd651a`.
It checks [metric definitions](../../casper/src/rust/metrics_constants.rs), validation, replay, merge, and driver code without running the workload.
The table uses Rust metric names.
The diagnostic must verify exported names, suffixes, units, labels, and process epochs against actual samples.

| Stage | Measurement | Source metric or probe | Evidence and remaining work |
| --- | --- | --- | --- |
| Merge | Conflict adjacency entries | `dag.merge.conflict.edges` | The merger records the sum of adjacency-set lengths, not unique undirected edges. |
| Merge | Branch count | `dag.merge.relation.branches` | The merger records the branch count. Per-block correlation remains unverified. |
| Merge | Relation size | `dag.merge.relation.items` | The merger records the merge-set size. Per-block correlation remains unverified. |
| Merge | Stage time | `dag.merge.total.time`, `dag.merge.conflict.time`, `dag.merge.branches.time` | Definitions exist. Collection and correlation remain unverified. |
| Replay | Stage time | `block.processing.stage.replay.time` | The definition exists. Collection and correlation remain unverified. |
| Replay | Phase time and reset calls | `block.replay.deploy.evaluate.time`, `block.replay.phase.reset.time`, `block.replay.phase.reset.calls` | Replay code records these metrics. Collection and correlation remain unverified. |
| Replay | Input deploy counts | `block.replay.phase.user-deploys.work`, `block.replay.phase.system-deploys.work` | Histograms record input lengths per replay invocation. They do not identify blocks. |
| Storage | Stage time | `block.processing.stage.storage.time`, `dag.insert.time` | Definitions exist. Collection and correlation remain unverified. |
| Storage | Pressure | Driver `disk_free_mb` uses `df -Pm` | The probe measures free space. Inode coverage and interval correlation remain unverified. |
| Queue | Queue depth | `proposer.queue.pending` | The definition exists. Emission and collection remain unverified. |
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
The diagnostic must retain the distinction between missing data and zero work.
It must not change finalization behavior or relax the 45-second assertion.

## Output and exit

The run produces one dominant work stage with its measured time distribution.
It produces the input-size dimension that the stage time grows with.
It records the violated bound as an operation count or a time as a function of that dimension.
It retains the raw per-interval samples and the workload shape that reproduces the failure.

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
