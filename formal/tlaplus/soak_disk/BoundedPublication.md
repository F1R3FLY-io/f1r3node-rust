# Bounded publication

R1 keeps the writer stop independent of a stalled storage sync.
The driver publishes each record with an atomic rename first, so the record is visible at once.
Durability is a separate bounded step that never blocks the stop.
A sync that exceeds the budget records an explicit unconfirmed result and is not confirmed termination.

The fixture runs the production driver in Docker isolation as uid 65534.
A substituted `sync` command hangs after the workload starts, which models a storage stall that no signal can interrupt.
The guardian fires during the first iteration from a substituted disk sample.
The fixture measures whether the driver completes its bounded response within the deadline.

The baseline driver synced each record before the rename and inside the stop path.
The stalled sync therefore blocked the guardian breach record and the writer stop without limit.
The matched RED exits 1 because the driver never completed its response within the deadline.

The corrected driver renames the record into place before any sync and runs durability through a bounded reap.
The reap abandons a stalled sync at the budget and records an unconfirmed result.
The matched GREEN exits 0.
The breach record is visible, the writer stop and the failure publication complete within the deadline, and the unconfirmed result is recorded.

## Correspondence

| Model element | Production or fixture boundary |
| --- | --- |
| `MakeVisible` | The driver renames the record into place. |
| `SyncDone` | The bounded reap completes the durability sync. |
| `SyncStall` | The substituted sync hangs and never returns. |
| `Stop` | The driver stops the writer. |
| `Confirm` | The reap confirms durability only when the sync completed. |
| `ShutdownIndependentOfSync` | The stop never waits on a stalled sync. |
| `StalledNeverConfirmed` | A stalled sync never counts as confirmed durability. |
| `StopEventuallyCompletes` | The writer stop always completes. |

The blocking configuration violates `ShutdownIndependentOfSync` with TLC exit 12.
The independent configuration passes with 9 distinct states.

## Limits

The fixture substitutes the sync command with a killable process, while a real stalled sync can stay in uninterruptible kernel state.
The bounded reap abandons such a sync rather than terminating it, so durability remains best effort under a persistent stall.
The model represents one record and one stop and does not represent concurrent publishers.
The composed emergency deadline bounds the reap budget in the emergency path.

Storage hardware behavior, upload acknowledgment, and the guardian's own bounds are not verified here.
No native service manager, real Docker daemon, or disposable runner executed this cycle.
Construction is not applicable to this Bash-driver model.
D2 and claim discharge remain pending.
