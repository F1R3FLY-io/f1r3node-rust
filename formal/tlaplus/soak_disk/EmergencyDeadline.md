# Composed emergency deadline

B49 tests the whole emergency response after a guardian breach during an iteration.
The fixture runs the production driver in Docker isolation as uid 65534.
The disk probe reports free space above the floor until the workload starts and then reports free space below the hard floor.
The guardian fires during the first iteration.
Three telemetry roots hold eight session directories each, and the evidence copy command ignores the termination signal and stalls.
The fixture measures the time from the guardian record to the driver's exit with a published summary.

The baseline driver stops the iteration, drains its output, stops the writers, emits metrics, and then copies failure evidence from each telemetry root.
It writes its own breach record and early-exit record only after that copy, and no bound covers the copy.
One stalled root therefore delays the breach record and the summary without limit.
The matched RED exits 1 because the copy started before the record and the response exceeded the fixture budget.

The corrected driver starts one composed deadline when it detects a breach.
It writes the breach record and the early-exit record at once, before it stops the iteration.
It bounds the output drain, each evidence copy, the diagnostics, and the summary writer by the remaining budget.
Each bounded step runs in its own session with a watchdog.
When the budget is spent, remaining copies are skipped and recorded, and the summary is still published.
The matched GREEN exits 0: the record precedes the copy, two roots are skipped, and the failure is published within the budget with counters `[1,1,0,0]`.

## Setting

`SOAK_EMERGENCY_DEADLINE_SECONDS` sets the composed budget in seconds, from 5 through 600, with a default of 60.
The budget starts at the first breach decision and covers every later response step in the driver.
The per-command bounds still apply inside the budget.
The summary writer keeps a minimum of five seconds so the fallback summary can still be produced.

## Correspondence

| Model element | Production or fixture boundary |
| --- | --- |
| `Init` | The driver has decided a breach. The evidence copy may stall. |
| `Record` | The driver writes the breach record and early-exit record. |
| `CopyRoot` | One telemetry root copies within one unit of budget. |
| `StallRoot` | A stalled copy consumes budget without progress. With the composed deadline it stops at the deadline. |
| `SkipRoot` | The driver skips a root after the deadline and records the skip. |
| `Publish` | The driver publishes the summary once every root is copied or skipped. |
| `ResponseWithinDeadline` | The elapsed response never exceeds the deadline. |
| `RecordBeforeCopy` | No copy starts before the record. |
| `Publishes` | The response eventually publishes. The unbounded baseline with a stalled copy never does. |

The unbounded configuration violates `ResponseWithinDeadline` with TLC exit 12.
The composed configuration passes with 16 distinct states.

## Limits

The fixture stalls the evidence copy and uses substituted stop and probe commands.
The model counts budget in copy steps, not wall-clock seconds, and does not represent the session kill.
The budget bounds the driver's own response.
It does not bound the guardian's stop or the crash monitor, which keep their own bounds.

Storage durability of the record and the summary is not verified here.
No native service manager, real Docker daemon, or disposable runner executed this cycle.
Construction is not applicable to this Bash-driver model.
D2 and claim discharge remain pending.
