# Soak disk reserve argument

## Purpose and scope

This document records the disk reserve argument for gate D2 and gate D3.
It records the source timing controls and the missing production bounds.
The complete response interval, growth, burst, reserve, and margin remain open.
D3 measurements must support enforced limits rather than replace them.
It does not discharge any claim and does not authorize an acceptance soak.

The reserve inequality from the recurrence plan is the reference form.

```text
emergency free-space threshold >= R + G*T + J + M

R = space required for runner operation, minimal evidence, and final upload
G = bounded growth rate of every remaining writer
T = maximum complete detection, creation closure, confirmed termination, and required evidence interval
J = maximum burst not represented by the sampled growth rate
M = explicit safety margin
```

Every term needs a common unit and a recorded justification.
This document uses mebibytes for space and seconds for time.
Neither a sampled average nor a sampled maximum is a worst-case growth bound.
Byte and inode reserves need separate calculations for each affected filesystem.

## The response-latency term T

T must cover every interval during which an accounted writer can consume the reserve.
Threshold detection, creation closure, independent termination confirmation, and required evidence handling need explicit start and finish events.
Publication of a failure record does not prove that writers have terminated.
Upload acknowledgment and verified retrieval are separate events.
An upload timeout bounds a wait, not successful retrieval.

The following observations refer to source `802bc98d96594e2151afd49b31b8250c86573125`.
They are source-review findings, not new behavioral RED results.

| Component | Source observation | Missing bound |
| --- | --- | --- |
| Guardian cadence | The guardian sleeps five seconds between loop bodies. | Probe time, preference updates, scheduling, and other loop work also affect the interval. |
| Disk probe | `disk_free_mb` uses a two-second timeout and one-second kill grace. | Full probe completion and helper termination under the selected faults need verification. |
| Soft-floor detection | Three consecutive low samples trigger the response. | Three loop bodies do not establish a fifteen-second detection bound. |
| Driver polling | The iteration loop sleeps for `SOAK_GUARDIAN_POLL_SECONDS`, normally two seconds. | Loop work and scheduling precede another observation. |
| Emergency budget | `emergency_start` and `emergency_remaining` use `date +%s`. | Clock changes and all response paths need coverage. The guardian-progress clock is separate. |
| Command termination | `session_bounded` requests TERM, then KILL after one second. | Signal requests do not establish permanent creation closure or independent termination confirmation. |
| Summary | `summary_budget` returns at least five seconds. | The minimum can exceed the remaining emergency budget. |
| Durability reap | `publication_budget` returns at least one second. | The minimum can extend past the emergency budget, and a timeout leaves durability unconfirmed. |

The earlier 7-, 17-, 67-, and 77-second estimates omit required work and are not production upper bounds.
The expression `T <= 5*N + P + D` is therefore not established by these settings.
An end-to-end bound needs explicit assumptions for scheduling, clocks, blocking operations, manager response, creators, and evidence handling.

The iteration path calls `emergency_start` after it observes the breach record.
This point is not the threshold crossing or necessarily the guardian's first breach decision.
The source shares the remaining budget across selected response operations, but that does not prove the complete interval.
A public-production timing cycle must retain missing, late, and unconfirmed outcomes rather than infer success from configured timeouts.

## The production deadline

The workflow sets `SOAK_EMERGENCY_DEADLINE_SECONDS` to 10.
The driver default remains 60, with an accepted range of 5 through 600.
This review does not change either setting.

The three incident runners disappeared 18, 20, and 23 seconds after historical guardian stamps.
These observations motivate earlier intervention but do not guarantee a future survival interval.
Health-tag publication occurs after other response work and is best effort.
The tag timestamp is not a verified threshold-crossing timestamp.

Subtracting the configured ten seconds from the historical eighteen seconds does not establish eight seconds for upload.
The response and upload need their own source-bound observations, resource limits, and failure handling.
No setting alone guarantees successful publication or runner survival.

## The publication dependency

The [bounded publication package](../cbc-evidence/soak-d2-durable-record-2026-09-12/README.md) records the selected publication corrections.
The driver no longer waits for record synchronization before it requests writer shutdown.
`publish_record` checks producer success before rename, and `reap_publication` limits its synchronization wait.
A stalled reap records an unconfirmed durability result.

These corrections do not bound all producer, write, rename, filesystem, or upload operations.
Atomic visibility, file synchronization, directory synchronization, power-loss survival, upload acknowledgment, and verified retrieval remain distinct obligations.
Historical finite-model results and selected runtime tests retain their original scope.
They do not establish the complete T term.

## The constraint on the growth term

The calculation needs a conservative free-space value F at a defined start time.
The configured floor is not the free space observed when the guardian reacts.
Threshold overshoot, sampling latency, rounding, and concurrent allocation need explicit accounting.
The hard-floor path needs its own accounting rather than the soft floor's larger reserve.

For positive T, the algebraic constraint is:

```text
F >= R + G*T + J + M
G <= (F - R - J - M) / T
```

This algebra does not establish any input value.
G must include every writer that can consume space during the interval unless R already accounts for that allocation.
A stop request cannot remove a writer or deferred creator from the calculation.
A negative available-growth budget means that the proposed reserve is insufficient even before continuing growth.

A valid result requires enforced growth and burst bounds, a complete interval bound, and justified reserve and margin values.
Measured rates can refute proposed bounds but cannot prove their enforcement.
A lower protection threshold is not an acceptable way to satisfy the inequality.

## Open terms and the diagnostic requirement

Every term remains open until its production assumptions and evidence are established.

| Term | Status | Required evidence |
| --- | --- | --- |
| R, reserve for operation, evidence, and upload | Open | Verified limits for simultaneous data, metadata, copies, archives, and upload staging. |
| G, aggregate allocation rate | Open | Complete writer attribution and tested enforcement throughout the interval. |
| T, complete interval | Open | Source-bound detection, closure, termination, and evidence observations under the selected faults. |
| J, maximum allocation outside the rate envelope | Open | Tested burst and threshold-overshoot limits, not a sampled residual. |
| M, explicit safety margin | Open | A reviewed policy with consistent units and justified bounded uncertainty. |

The diagnostic run must attribute growth across Docker layers, build cache, harness roots, runner logs, and open-deleted files.
It must separate byte exhaustion from inode exhaustion.
It must include the temporary copies made during evidence retention.
A writer with no enforceable bound leaves the reserve claim open.

## The modeling gap

The `EmergencyDeadline` model bounds the response in copy steps, not in seconds.
The `DurableRecord` model bounds the publication ordering, not its latency.
Neither model represents writer growth during the bounded response.
A D3 resource model must add external writes and stalled optional diagnostics, because fairness alone cannot prove a bounded emergency deadline.

The [reserve bound model](../../formal/tlaplus/soak_disk/ReserveBound.md) limits `Grow` transitions through a finite step guard.
Its passing configuration assumes concrete growth, burst, reserve, and step limits.
The model does not implement or verify a production stop mechanism, scheduling bound, or conversion from steps to seconds.

The [retention reserve model](../../formal/tlaplus/soak_disk/RetentionReserve.md) assumes one fixed copy size and no concurrent external allocation.
Its space-aware choice preserves the modeled reserve under those assumptions.
A production correction must address changing source sizes, concurrent writers, allocation overhead, inode demand, and failure reporting.
A separate free-space check before an unrestricted copy would not establish that bound.
Skipping required evidence must leave an explicit incomplete result, not successful acceptance.

## Status

This argument identifies open proof obligations and does not establish a production bound for T, G, J, R, or M.
The September 16 review corrects earlier timing and reserve claims without changing runtime behavior or historical evidence.
Full reserve discharge needs D2 containment, D3 lifecycle enforcement, complete evidence, and maintainer review.
D2, D3, and claim discharge remain pending.
