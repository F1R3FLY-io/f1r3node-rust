# Driver crash stop

## Selected contract

After the driver process group crashes, production stops the fixture's owned Docker writer and preserves its unrelated Docker writer.
The contract assumes healthy storage, a responsive Docker daemon, cooperative ownership metadata, and a surviving crash monitor.

## Correspondence

| Model element | Production operation |
| --- | --- |
| `Init` | Start the crash monitor before workload admission and wait for its readiness record. |
| `Crash` | Send `SIGKILL` to the driver process group during an active benchmark. |
| `SurvivesDriverCrash` | The monitor creates a separate session with `os.setsid()`. |
| `Respond` | The parent process descriptor becomes readable, and the monitor calls the existing ownership-checked stop helper. |
| `ownedRunning` | The workload Docker container remains active. |
| `unownedRunning` | The unrelated Docker container remains active. |

The [B38 record](../../../docs/cbc-evidence/soak-d2-crash-stop-2026-09-11/README.md) retains production RED/GREEN and the exact `DriverCrashStopsOwnedWriter` control.
The control exits 12, and the corrected model exits zero with three distinct states.
The production fixture records driver exit 137, container state, and file growth before cleanup.
GREEN requires the owned container to stop, its file to remain unchanged, and the unrelated writer to continue.
Fixture cleanup occurs after this verdict.

## Composition

The [B39 model](CrashMonitorExit.md) covers handled exits separately.
The crash monitor does not repeat a stop after a valid exit-handling acknowledgment.
That acknowledgment does not establish writer termination.

The B37 fixture previously required a surviving writer without a daemon fault.
B38 invalidates that assumption.
The current B37 fixture explicitly rejects Docker kill requests and verifies the rejected request before checking retained recovery failures.
Its admission and failure-count assertions remain intact.

## Limits

The model uses atomic transitions, stuttering, and weak fairness.
It does not prove a wall-clock deadline or implementation refinement.
The construction tier does not apply to this bounded Bash-driver model.

The fixture uses restricted containers on a guarded disposable virtual machine, not a node workload.
It does not cover monitor death, all admission races, late writer creation, forged metadata, every launch path, or uninterruptible writers.
Storage faults, power loss, durable publication, aggregate deadlines, reserve bounds, hosted verification, and maintainer review remain open.
D2 and claim discharge remain pending.
