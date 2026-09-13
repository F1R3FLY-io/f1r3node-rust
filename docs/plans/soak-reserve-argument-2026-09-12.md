# Soak disk reserve argument

## Purpose and scope

This document records the disk reserve argument for gate D2 and gate D3.
It bounds the response-latency term of the reserve inequality.
It marks the growth term, the burst term, and the reserve term as open until the D3 diagnostic run.
It does not discharge any claim and does not authorize an acceptance soak.

The reserve inequality from the recurrence plan is the reference form.

```text
emergency free-space threshold >= R + G*T + J + M

R = space required for runner operation, minimal evidence, and final upload
G = bounded growth rate of every remaining writer
T = maximum detection, stop, and evidence latency
J = maximum burst not represented by the sampled growth rate
M = explicit safety margin
```

Every term needs a common unit and a recorded justification.
This document uses mebibytes for space and seconds for time.
A sampled average is not a worst-case growth bound.

## The response-latency term T

The T term is the time from the free space crossing the trigger to the published failure.
The driver bounds this term in three parts: detection, response, and publication.
The composed emergency deadline from cycle B49 bounds the response part.
The detection part follows from the guardian sample interval and the iteration poll interval.

The guardian samples free disk space on a fixed five-second interval.
A sample below the hard floor fires the breach at once.
A sample below the floor but above the hard floor fires the breach after three consecutive samples.
The iteration loop then observes the breach record within one poll interval.

| T component | Driver source | Worst case with defaults |
| --- | --- | --- |
| Detection below the hard floor | One guardian sample at five seconds, then one poll interval | 7 seconds |
| Detection below the floor | Three guardian samples at five seconds, then one poll interval | 17 seconds |
| Response and publication | `SOAK_EMERGENCY_DEADLINE_SECONDS`, default 60, range 5 through 600 | 60 seconds |

The default hard-floor path gives a worst case of 67 seconds.
The default floor path gives a worst case of 77 seconds.
The general expression uses the settings as symbols.

```text
T <= 5*N + P + D

N = consecutive guardian samples: 1 below the hard floor, 3 below the floor
P = SOAK_GUARDIAN_POLL_SECONDS, default 2
D = SOAK_EMERGENCY_DEADLINE_SECONDS, default 60
```

The composed deadline starts at the first breach decision.
It covers the output drain, the writer stop, each evidence copy, the diagnostics, and the summary writer.
The iteration termination wait is at most fifteen seconds and never exceeds the remaining budget.
Each evidence copy and the summary writer run under the remaining budget in their own sessions.

## The publication dependency

The bound `T <= 5*N + P + D` holds only after the record publication runs inside the deadline.
The current `publish_record` helper calls `sync`, `mv`, and a directory `sync` without the remaining budget.
The guardian waits for publication before it stops the writers, and the iteration controller waits for two publications before it sends the termination signal.
A stalled storage sync can therefore delay the stop and exceed the deadline.

This gap is review finding R1 against the B51 commit.
The native-admission session owns the correction.
Until publication runs inside the composed deadline, the T bound is provisional on that path.
A timeout alone must not become termination confirmation, so the driver must retain an explicit unconfirmed result when publication cannot complete.

## The constraint on the growth term

The trigger free space must cover the reserve inequality at the moment the guardian fires.
The default floor is 4096 mebibytes, and the default hard floor is 2048 mebibytes.
With the floor as the trigger and the default worst-case T of 77 seconds, the inequality bounds the tolerable growth.

```text
G <= (F - R - J - M) / T

F = free space at the trigger, 4096 MiB at the default floor
T = 77 seconds at the default floor path
```

This form closes only after the diagnostic run supplies R, G, and J.
A measured G above this bound requires a higher floor, a shorter deadline, or an enforced writer bound.
A lower protection threshold is not an acceptable way to satisfy the inequality.

## Open terms and the diagnostic requirement

Four terms remain open and depend on the D3 diagnostic run.

| Term | Status | Source needed |
| --- | --- | --- |
| R, reserve for operation, evidence, and upload | Open | The diagnostic run sizes the minimal evidence and the upload demand |
| G, worst-case growth rate of every remaining writer | Open | The diagnostic run identifies the growing writer and its enforced bound |
| J, maximum burst not in the sampled rate | Open | The diagnostic run records the burst above the sampled average |
| M, explicit safety margin | Open | A policy choice recorded with its justification |

The diagnostic run must attribute growth across Docker layers, build cache, harness roots, runner logs, and open-deleted files.
It must separate byte exhaustion from inode exhaustion.
It must include the temporary copies made during evidence retention.
A writer with no enforceable bound leaves the reserve claim open.

## The modeling gap

The `EmergencyDeadline` model bounds the response in copy steps, not in seconds.
The `DurableRecord` model bounds the publication ordering, not its latency.
Neither model represents writer growth during the bounded response.
A D3 resource model must add external writes and stalled optional diagnostics, because fairness alone cannot prove a bounded emergency deadline.

## Status

This argument bounds T for the driver's own response, subject to the R1 publication correction.
It does not bound G, J, or R, which the D3 diagnostic run must supply.
Full reserve discharge depends on the D3 disk-growth evidence and on maintainer review.
D2, D3, and claim discharge remain pending.
