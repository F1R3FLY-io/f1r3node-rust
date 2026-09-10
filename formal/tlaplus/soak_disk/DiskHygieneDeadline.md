# Disk Hygiene Deadline Correspondence

## Observed failure

B23 holds the Docker builder cleanup client inside the admission band. The client ignores `TERM` and waits for fixture release.

The independent observer waits five seconds after cleanup starts. It checks client cancellation and failure publication before it releases the fixture.

The baseline leaves the client active and publishes no summary before release. No new iteration starts, but the driver cannot finish its refusal.

## Correction

The driver runs the unchanged cleanup commands in a timed child shell. The shell retains the command group until kill escalation.

`SOAK_DISK_HYGIENE_SECONDS` permits integers from 1 through 30. The default is ten seconds, followed by one second of kill grace.

The fixture uses a one-second command budget. The corrected driver cancels the client, refuses new work, and publishes one protection failure before observation.

A failed timed group preserves an existing guardian breach. Otherwise, the driver records an incomplete cleanup result before its final failure publication.

## Model reuse

The configurations reuse `DiskStopDeadline` without changing that model. `HygieneWithinBudget` names its existing `StopWithinBudget` invariant for the cleanup command group.

The model's `stopping` phase represents an outstanding cleanup command group. `Tick` applies `TERM` and then `KILL` when the deadline is enforced.

The negative control disables enforcement and requires exit 12 on `HygieneWithinBudget`. The corrected configuration has five distinct states.

This correspondence concerns command cancellation. The production fixture separately checks refusal and summary publication, which the model does not represent.

## Current command mapping

B27 removes destructive Docker hygiene. The current timeout fixture stalls `docker system df` inside the same timed command group.

The historical B23 evidence still records the builder cleanup client. The unchanged deadline model applies to cancellation of either selected client, not daemon completion.

## Limits

The configured budget is a component limit, not a measured complete response deadline. The model assumes effective signals and advancing abstract time.

The fixture confirms cancellation of the selected client. It does not confirm that Docker daemon operations or uninterruptible descendants have stopped.

This correction does not change cleanup selectors. Session ownership, image preservation, individual cleanup failures, and reclaimed-space guarantees need separate verification.

Local publication is not durable storage or successful upload. The complete emergency deadline, reserve bounds, hosted checks, maintainer review, and D2 remain pending.
