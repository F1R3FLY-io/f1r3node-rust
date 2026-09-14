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

## The instrumentation rule

The run uses the existing observability metrics first.
The gate O1 telemetry already emits the carrier, merge, and replay counters this run needs.
If the telemetry cannot separate the dominant stage from its input-size dimension, the run adds narrowly scoped instrumentation.
It adds one counter or one timer for the missing dimension, and it does not select an optimization before the measurement.

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
