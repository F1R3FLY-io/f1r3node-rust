# Crash-monitor admission checks

B42 tests confirmed monitor death before a valid admission probe returns.
The fixture runs both benchmark and iteration admission through the production driver.
The external disk probe returns 16384 MiB after it confirms monitor death through a process file descriptor.
The fixture excludes probes from the resource guardian before it injects the fault.

Both baseline cases count an admission after the monitor dies.
The existing active-work checks then stop the clients, but that later response does not undo admission.
The correction checks the crash monitor before either admission counter increases.
Both unchanged fixtures now refuse work and retain one failure across two real restarts.
The public counters remain `[0,1,0,0]` for iterations, failures, benchmark segments, and benchmark failures.
An unrelated native writer continues during each refusal.

| Model action | Production or fixture boundary |
| --- | --- |
| `Init` | The driver enters a benchmark or iteration admission probe. |
| `MonitorDiesDuringProbe` | The external probe confirms monitor death before returning valid free space. |
| `Admission` | The driver checks monitor health before it records admission. |
| `Restart` | A retained breach prevents admission across two restarts. |

Each unchecked configuration violates `MonitorDeathPreventsAdmission` with TLC exit 12.
Each corrected configuration passes with five distinct states.
The `Work` constant selects the benchmark or iteration case.

The finite model describes the tested admission boundary, not atomic creation fencing.
Monitor death after the check, concurrent creation, storage durability, and aggregate deadlines require separate evidence.
Construction is not applicable to this Bash-driver model.
D2 and claim discharge remain pending.
