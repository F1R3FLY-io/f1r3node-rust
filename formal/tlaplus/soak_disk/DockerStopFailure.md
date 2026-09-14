# Docker Stop Failure

## Behavior

B31 retains a failed Docker stop and states that writer termination is unconfirmed after driver exit.
The workload writer remains active because the external stop command returns 42 without contacting the daemon.

The negative control violates `FailedStopRetained` with exit 12.
The positive configuration preserves the active writer and reaches two distinct states.
A failed command does not imply writer termination.

## Correspondence

The stop helper propagates failed listing, identifier validation, ownership inspection, and destructive-command results.
The outer timeout also returns failure when its command budget expires.

Active-exit cleanup records `writer-stop-failure.txt` and `writer_stop_failed` in `early-exit.txt` after a failed stop.
For the tested uncommitted iteration, cleanup records one failure and the retained interruption state.
Existing restart handling refuses that interruption state without adding another interruption failure.

The fixture uses the real driver and Docker daemon.
An external Docker command substitute rejects only the stop command.
The fixture requires a nonzero driver exit, a surviving writer, one recorded failure, and explicit unconfirmed-termination text.
The fixture supervisor removes its exact container identifier after the observation.
That removal is not production shutdown evidence.

## Assumptions and limits

The model assumes one rejected stop, an existing uncommitted iteration, and completing local record writes.
Its abstract transition does not prove atomic production publication or power-loss durability.
The fixture does not inject storage faults or all Docker errors.

A successful client result still does not independently confirm every writer's termination.
Other shutdown paths, benchmark recovery, concurrent record updates, late daemon operations, and complete deadline bounds remain open.
D2, hosted checks, maintainer review, claim discharge, and acceptance remain pending.

See the [B30–B31 evidence](../../../docs/cbc-evidence/soak-d2-owned-stop-2026-09-10/README.md).
