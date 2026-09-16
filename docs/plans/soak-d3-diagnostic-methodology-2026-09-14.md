# D3 disk-growth diagnostic methodology

## Purpose and scope

This document specifies the diagnostic run for gate D3.
The run identifies the growing filesystem and the writer that consumes it.
It supplies observations for the open growth, burst, and reserve terms.
Measurements alone do not establish enforceable upper bounds.
It is a measurement methodology, not a fix and not an acceptance soak.

The run does not delete active node state or required evidence.
It does not cancel an active soak and does not lower any protection threshold.
It runs under the gate O1 observability prerequisites and the diagnostic safety controls.
A separate authorization governs any node soak.

## The open question

The three incident runners lost free disk space while the guardian stamp showed only a few gigabytes free.
The [work log](../work-logs/task-soak-disk-hygiene-stop-2026-09-08T05-06Z.md) records the deaths within 18 to 23 seconds of the stamp.
The recurrence plan and PR 399 add protection and an earlier stop, but neither names the growing writer.
D3 closes only when the run names that writer and gives it an enforceable lifecycle bound.

## Attribution targets

The run attributes disk growth to a specific owner across every writer class.
A single free-space number is not attribution.
The run records each class separately with its filesystem, owner, process epoch, and measurement interval.
The run distinguishes logical file sizes from allocated blocks and avoids double-counting shared layers or hard links.
The run records only approved inspection fields, not environment values, private keys, or complete Docker inspection responses.

| Writer class | What to measure | Command family |
| --- | --- | --- |
| Docker images and layers | Per-image and per-layer allocated bytes, shared versus unique | Bounded private-engine accounting and selected image fields |
| Docker container writable layers | Per-container allocated bytes and inode demand | Bounded private-engine accounting and selected container fields |
| Containerd storage and helpers | Actual image-store, snapshot, state, and temporary allocations | Verified runtime paths and bounded filesystem accounting |
| Docker build cache | Build-cache bytes and reclaimable bytes | `docker system df -v` build-cache rows |
| Harness telemetry roots | Bytes and inode counts under the integration-tests roots | Bounded `du` and `find` with a deadline |
| Runner logs | Journal and runner diagnostic bytes | Journal disk usage, log directory size |
| Open-deleted files | Allocated blocks, logical size, device, inode, and holder epoch | Bounded descriptor inspection with file identity checks |
| Evidence retention copies | Copy, archive, metadata, and upload-staging allocation peaks | Bounded measurements before, during, and after retention |
| Filesystem and runner overhead | Journal, metadata, reservation, and unattributed allocation | Filesystem identity, free-space observations, and reconciliation |

## The open-deleted hazard

A file that a process unlinks but still holds open keeps its blocks until the process closes it.
A path-based `du` walk cannot attribute an unlinked file that remains open.
A difference between `df` and `du` does not identify this cause by itself.
Metadata, reserved space, inaccessible paths, and measurement timing can also explain a difference.

The run checks open-deleted files as one hypothesis, not as an established cause.
A bounded `lsof +L1` result identifies candidate holders, not a complete allocated-block inventory.
Logical size can differ from allocated space.
Repeated descriptors for one device and inode must not multiply its allocation.
Missing permissions, process exits, and incomplete scans remain explicit gaps.

## Sampling procedure

The run captures a baseline before the workload starts.
A configured sampling interval is a target, not proof of the actual interval.
Each probe records monotonic start and finish times, its outcome, and its source identity.
Sequential probes must not appear as one simultaneous sample.
The run records free bytes and free inodes separately for every affected filesystem.

The run computes valid deltas within each filesystem, owner, and process epoch.
Filesystem usage is a gauge, so a decrease can represent reclamation rather than a counter reset.
Cumulative allocation counters require separate reset handling.
Missing data remains unavailable, not zero, and invalid intervals do not supply rates.
The run retains raw observations and reconciles attributed growth against filesystem changes.

An interval delta describes net change, not all allocations within that interval.
Allocation followed by reclamation can leave a zero delta despite a high temporary peak.
The run reports unresolved growth and measurement overlap rather than forcing every change onto one owner.

## Deriving the reserve terms

The reference inequality is `F >= R + G*T + J + M`.
`F` is a conservative free-space value at a defined start time, not an assumed sample equal to the configured floor.
Each term needs consistent units, explicit assumptions, and a recorded justification.
Neither a sampled average nor a sampled maximum establishes a worst-case bound.

| Term | Required bound | Role of the diagnostic |
| --- | --- | --- |
| G, growth | Enforced aggregate allocation rate for every writer not charged to R | Identify writers and test their limits across the complete response interval. |
| J, burst | Maximum allocation outside the rate envelope, including unresolved threshold overshoot | Measure candidate bursts and verify the mechanism that limits them. |
| R, reserve | Bounded runner operation, required evidence, metadata, archive, and upload-staging demand | Measure footprints and test the declared retention and staging limits. |
| T, interval | Complete detection, closure, confirmed termination, and required evidence interval | Record each stage and test its limits under the selected faults. |
| M, margin | Explicit allowance for bounded uncertainty | Record the policy, units, assumptions, and justification. |

The maximum observed interval rate is evidence about that run only.
Subtracting that rate from the largest observed delta does not establish an unsampled burst bound.
A quota can bound total allocation but does not establish a rate limit by itself.
A quota failure must also preserve required evidence and workload outcomes.

The [reserve argument](soak-reserve-argument-2026-09-12.md) keeps the complete interval open.
A stop request does not remove writers or accepted deferred work from the accounting.
Copies, compression, and upload staging need simultaneous-allocation analysis, not just the final archive size.
Byte and inode reserves require separate calculations for each filesystem.
Unattributed writers or unenforced limits keep the reserve claim open.

## Output and exit

The run identifies growing writers and reports their observed rates and allocation peaks.
It distinguishes measured values, proposed limits, verified enforcement, and unresolved assumptions.
It records byte growth and inode growth as separate results.
It retains the raw per-interval samples and the open-deleted inventory.

Each selected writer supplies a production retention or cleanup obligation for its owner.
The correction requires a frozen public-production RED, a matching resource-model counterexample, and unchanged-fixture GREEN results.
A measured rate must not enter the model as an enforced bound without separate justification.

D3 requires a verified lifecycle correction and storage-demand validation across the full planned duration and failure-retention policy.
Repeated failures, restarts, preparation, and required retained evidence belong in that demand.
A controlled early stop alone does not satisfy D3.

## Safety controls

Every probe needs declared time, output, and resource limits with an explicit incomplete result.
A command timeout alone does not prove helper termination or bound bytes written during that interval.
The run must verify probe termination and collect independent observations before fixture cleanup.
The existing driver snapshot does not establish a byte or inode limit.
The run never deletes active node state or required evidence.
The run never replaces attribution with a larger volume or a lower protection threshold.

## What this document does not do

This document specifies the run and does not execute it.
The run needs the runner, the O1 observability prerequisites, and separate authorization.
It does not name the writer, which only the executed run can do.
The D3 correction, its property, and the resource-model counterexample follow the run and need the driver.

D2, D3, and claim discharge remain pending.
