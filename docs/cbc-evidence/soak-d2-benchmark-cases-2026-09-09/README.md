# Additional Opening Benchmark Admission Coverage

## Scope

This package checks four opening benchmark admission paths after B17. All four cases pass without another production correction.

These results add coverage. They do not constitute a new RED/GREEN repair cycle or a claim discharge.

The fixture executes the real driver and benchmark runner. It replaces external commands inside a resource-limited container without host mounts, network, or a Docker socket.

The Docker fixture records startup and returns failure without starting nodes. An admitted benchmark therefore records one benchmark failure by design.

## Results

| Case | Disk protection | Offered space | Benchmark admission | Protection failures |
| --- | --- | --- | --- | --- |
| Equality | Enabled | 8192 MiB | Admitted | 0 |
| Sufficient space | Enabled | 16384 MiB | Admitted | 0 |
| Unavailable sample | Enabled | Missing after startup | Refused | 1 |
| Disabled protection | Disabled | 7000 MiB if queried | Admitted | 0 |

Enabled protection uses a 4096 MiB floor and a 4096 MiB band. The unavailable sample also prevents iteration admission.

Each allowed case admits one subsequent fixture iteration. Disabled protection also disables the memory guardian in this fixture. It does not establish safe operation without protection.

## Retained evidence

The [manifest](manifest.jsonc) binds current source snapshots, original raw files, and complete published logs.

The expanded fixture still detects the B17 defect against its frozen baseline. The [B17 record](../soak-d2-benchmark-monitor-2026-09-09/README.md) remains immutable historical evidence.

Fresh serial verification passes sixteen positive configurations and sixteen exact controls. The archive preserves all 32 actual TLC logs before the classifier executes.

The classifier passes 112 cases. Routing passes six scenarios. The combined emergency suite passes sixteen scenarios. Supporting regressions also pass.

Published logs replace only the local raw-root prefix. The manifest records replacement counts and separate original and published digests.

## Limits

These cases do not check interleaved admission, additional numeric faults, or successful benchmark execution. They do not prove finalization or any full-duration soak result.

Guardian failure, benchmark cancellation, cleanup ownership, confirmed writer termination, durable publication, and the composed response deadline remain open.

Writer-growth bounds, reserve evidence, hosted verification, maintainer review, and complete candidate identity remain pending. D2 and acceptance remain pending.
