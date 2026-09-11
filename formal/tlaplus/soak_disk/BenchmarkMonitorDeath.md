# Benchmark crash-monitor death

B41 tests monitor death during an active benchmark with the optional memory and disk guardians disabled.
The fixture executes the production driver and benchmark script in a restricted container.
The Docker and readiness commands are external substitutes.
A native writer inherits the production ownership marker and closes its output descriptors before detachment.
A second writer has no ownership marker.

The fixture confirms monitor death through a process file descriptor before it evaluates the response.
The baseline benchmark continues its writer after monitor death.
The corrected driver records a breach and uses its existing benchmark interruption path.
The unchanged fixture confirms writer termination, unrelated writer progress, and one retained failure across two refused restarts.
The public counters remain `[0,1,1,1]` for iterations, failures, benchmark segments, and benchmark failures.

| Model action | Production or fixture boundary |
| --- | --- |
| `Init` | The benchmark has one admitted segment and an active native writer. |
| `MonitorDies` | The fixture confirms monitor death before its observation interval. |
| `Respond` | The driver checks its monitor child and follows the existing interruption path. |
| `Restart` | Two real driver restarts preserve the failure and refuse more work. |

The unchecked control disables benchmark observation, as the B40 baseline did.
It fails `BenchmarkMonitorDeathStopsOwnedWriter` with TLC exit 12.
The corrected configuration passes with five distinct states.

The model treats the selected successful response as one atomic action.
It does not establish storage durability, complete writer discovery, concurrent creation fencing, or an aggregate deadline.
The native fixture does not establish Docker writer termination under monitor failure.
Construction is not applicable to this Bash-driver model.
D2 and claim discharge remain pending.
