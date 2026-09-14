# D3 disk-growth diagnostic methodology

## Purpose and scope

This document specifies the diagnostic run for gate D3.
The run identifies the growing filesystem and the writer that consumes it.
It supplies the growth, burst, and reserve terms that the disk reserve argument leaves open.
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
The run records each class separately at every sample.

| Writer class | What to measure | Command family |
| --- | --- | --- |
| Docker images and layers | Per-image and per-layer bytes, shared versus unique | `docker system df -v`, image inspect |
| Docker container writable layers | Per-container upper-directory bytes | `docker ps -s`, container inspect |
| Docker build cache | Build-cache bytes and reclaimable bytes | `docker system df -v` build-cache rows |
| Harness telemetry roots | Bytes and inode counts under the integration-tests roots | Bounded `du` and `find` with a deadline |
| Runner logs | Journal and runner diagnostic bytes | Journal disk usage, log directory size |
| Open-deleted files | Bytes held by unlinked but still-open files | `lsof +L1` for size and holder |
| Evidence retention copies | Temporary bytes during the failure-evidence copy | The driver's bounded snapshot |

## The open-deleted hazard

A file that a process unlinks but still holds open keeps its blocks until the process closes it.
Such a file has no path, so a path-based `du` walk cannot see its bytes.
The free-space drop with a near-empty `du` result is the signature of this hazard.
The run must test this hypothesis first with `lsof +L1`, because it explains a silent, fast exhaustion.

## Sampling procedure

The run captures a baseline before the workload starts.
It then samples every writer class at a fixed interval through the run.
It records free bytes, free inodes, and each class total at every sample.
It separates byte exhaustion from inode exhaustion, because either one ends the runner.

The run computes the per-interval delta for each writer class.
It attributes the dominant, monotonic growth to one owner.
It records restart epochs and rejects a counter reset or a missing interval as growth.
It keeps the raw samples, so a later reader can recompute every delta.

## Deriving the reserve terms

The run supplies the three open terms of the reserve inequality `Reserve >= Required + Growth*Deadline + Burst + Margin`.
Each term needs a recorded justification and a common unit.
A sampled average is not a worst-case bound.

| Term | Definition | Derivation from the run |
| --- | --- | --- |
| G, growth | The worst-case growth rate of every writer still active after the workload stop | The maximum per-interval delta of the surviving writers, not their average |
| J, burst | The maximum single-interval jump above the sampled rate | The largest one-sample delta minus the modeled rate |
| R, reserve | The space for runner operation, the minimal evidence, and the final upload | The measured minimal evidence bytes plus the retention-copy peak plus the upload size |

The run reports these terms for the surviving writers during the emergency response window.
The response window is the composed emergency deadline that the reserve argument bounds.
These terms then close the reserve inequality against the disk floor.

## Output and exit

The run produces one named growing writer with its worst-case growth rate.
It produces the reserve terms with their justifications.
It records byte growth and inode growth as separate results.
It retains the raw per-interval samples and the open-deleted inventory.

The named writer and its rate feed the D3 correction that follows.
That correction writes one production retention or cleanup property for the owner.
The property gets a matching resource-model counterexample, which extends the `ReserveBound` model with the measured writer.
The exit is a verified lifecycle bound for the measured writer, because a controlled early stop alone does not satisfy D3.

## Safety controls

Every disk-walk command carries a deadline, because an unbounded `du` on a full filesystem stalls.
The run reuses the driver's bounded snapshot rather than a fresh recursive walk during the response.
The run never deletes active node state or required evidence.
The run never replaces attribution with a larger volume or a lower protection threshold.

## What this document does not do

This document specifies the run and does not execute it.
The run needs the runner, the O1 observability prerequisites, and separate authorization.
It does not name the writer, which only the executed run can do.
The D3 correction, its property, and the resource-model counterexample follow the run and need the driver.

D2, D3, and claim discharge remain pending.
