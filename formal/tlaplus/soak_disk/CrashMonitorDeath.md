# Crash Monitor Death

## Selected behavior

B40 covers monitor death during an active iteration.
The driver must stop its owned host writer and preserve an unrelated host writer.
Two restarts must refuse work and retain one failure.

The production entry point is `scripts/run-merge-recovery-soak.sh`.
The fixture is `scripts/bench/test-soak-crash-monitor-death.sh`.

## Production correspondence

The fixture runs the production driver in a restricted process namespace.
The fixture starts one unrelated writer before the driver starts.
The external workload starts a detached writer with the production ownership marker.
Both writers use real processes and files.
The fixture disables the optional disk and memory guardians.
The Docker boundary returns success without creating Docker resources.

The fixture identifies the actual monitor through its private readiness record.
The fixture checks the process command, user, parent, and session before it sends `SIGKILL` through a process file descriptor.
The same descriptor confirms monitor death.
The fixture does not stop either writer before its verdict.

| Model action or state | Production observation |
| --- | --- |
| `Init` | One admitted iteration and two active writers exist. |
| `MonitorDies` | The fixture confirms death of the identified monitor. |
| `Respond` | The observation checks writer state, continued writes, and the driver result. |
| `Restart` | The production driver starts again with the same output directory. |
| `failures` | The published summary retains one failure. |
| `admissions` | The external workload records exactly one invocation. |
| `unownedRunning` | The unrelated writer stays active and continues writing. |

The correction retains the monitor child PID after startup.
The active iteration loop checks that PID against the driver's running child jobs.
This check uses the Bash job table, not an arbitrary process with the same numeric PID.
A missing running job creates a protection-breach record.
The existing interruption path stops owned writers and retains failure.

The selected failure record uses ordinary file I/O.
The record does not establish durable publication or successful termination of every writer.

## Formal result

`MC_CrashMonitorDeath_startup_only_pre_fix` sets `ObserveMonitorDeath = FALSE`.
TLC exits with code 12 and violates `MonitorDeathStopsOwnedWriter`.
The production baseline also leaves the driver active and its owned writer growing after confirmed monitor death.

`MC_CrashMonitorDeath` sets `ObserveMonitorDeath = TRUE`.
TLC checks five distinct states with these invariants:

- `TypeOK`
- `MonitorDeathStopsOwnedWriter`
- `UnownedWriterPreserved`
- `MonitorFailureRetained`

`Completes` requires two restart observations under weak fairness.
The unchanged production fixture checks stopped writer state, stable writer data, unrelated growth, failure accounting, and refused restarts.

## Limits

This model supplies bounded refutation evidence.
Construction proofs do not apply to this Bash-driver model.
The actions are atomic abstractions, not a refinement proof of Bash, Linux, or Docker.
Weak fairness and fixture observation periods do not establish a wall-clock deadline.

The selected correction checks only active iterations.
Monitor death during benchmarks or at admission boundaries remains open.
Suspension, simultaneous driver failure, storage faults, late creation, and every detached writer remain open.
The external workload closes its output descriptors before it detaches the writer.
This fixture does not establish shutdown when a detached writer retains the workload output pipe.

The test uses cooperative inherited ownership in a private namespace.
It does not establish authorization against forged markers, hidden processes, or container administration.
D2, all claim discharges, hosted enforcement, human review, and acceptance remain pending.

## Evidence

The [B40 evidence package](../../../docs/cbc-evidence/soak-d2-monitor-death-2026-09-11/README.md) records source identities and the verification results.
