# Disk emergency model correspondence

## Authority

These models cover six local D2 behaviors. They do not discharge `CLAIM-SOAK-001` or prove the Bash implementation.

The production fixtures execute the real driver inside disposable containers. External disk, Docker, and workload commands are fixtures. No node workload runs.

The [evidence package](../../../docs/cbc-evidence/soak-d2-emergency-2026-09-09/README.md) retains each production RED, matching formal RED, correction, and GREEN result.

## Model-to-code map

| Cycle | Model | Production boundary | Independent assertion |
| --- | --- | --- | --- |
| B7 | `ActiveDiskProbe` | The guardian receives no sample after an iteration starts. | `InvalidSampleRequiresInterrupt` requires a breach record and an interruption request. |
| B8 | `DiskEmergencyRecord` | Docker receives a stop command after a hard-floor sample. | `StopRequiresRecord` requires a prior breach record. |
| B9 | `GuardianSupervision` | The iteration watcher observes guardian death. | `DeadGuardianRequiresInterrupt` requires a breach record and an interruption request. |
| B10 | `DiskProbeDeadline` | A disk command prints a valid field and then stalls. | `ProbeWithinDeadline` limits the probe interval. `TimedOutSampleRejected` rejects partial output after timeout. |
| B11 | `DiskDiagnosticDeadline` | A disk attribution command stalls with 32 session roots. | `AttributionWithinBudget` limits the entire attribution interval, not each root separately. |
| B12 | `DiskBreachRestart` | A segment starts with a retained guardian marker and a valid state file. | `RetainedBreachStopsRestart` prohibits admission. `PriorFailuresPreserved` prohibits a lower failure count. |

Each negative configuration disables its correction. Each negative control must produce TLC exit 12 and its specified invariant violation.

## Production limits

The disk probe uses GNU `timeout` with a two-second limit and a one-second kill grace. A failed command invalidates its output.

Disk attribution uses one process-group limit for each invocation. `SOAK_DISK_DIAGNOSTIC_SECONDS` accepts integers from 1 through 10. Its default is 10 seconds.

The attribution limit has a one-second kill grace. Nested attribution timeouts use foreground mode so they remain in the outer process group.

The guardian places tag preparation, metadata access, tag publication, and its disk snapshot inside one attribution invocation. Admission snapshots use the same wrapper separately.

The fixture sets the attribution limit to one second. Its observer allows three seconds after the first stalled command before checking driver progress.

The B11 fixture covers a stalled `du` command with 32 session roots. It does not independently exercise every Docker or metadata failure.

The guardian writes its disk breach marker before requesting termination. The marker reports that termination is unconfirmed. It does not assert that every writer stopped.

On restart, a retained marker prevents new iterations. A zero failure count becomes one. A positive failure count remains unchanged.

This recovery preserves a failure outcome. It does not reconstruct exact counts for multiple uncommitted events. It assumes that the marker and valid state file survive.

## Model assumptions and exclusions

B7 and B9 abstract one completed observation and its response. Their interruption variables do not represent confirmed process termination.

B8 permits external writes and a stop command that does not return. Its invariant concerns record ordering, not elapsed time or record durability.

B10 abstracts the probe limit and kill grace as three clock units. Its command would otherwise return after four units with a valid field.

B11 abstracts one complete command budget as one clock unit. Root counts are 1, 3, and 32. External writes can reduce abstract free space to zero.

Clock transitions assume timer service and effective cancellation. They are not a mechanized proof of Linux scheduling, process groups, or uninterruptible kernel operations.

The models assume completing local record writes. Weak fairness does not establish a wall-clock emergency bound.

The six models are separate abstractions. The combined test run does not establish a composed emergency deadline or an implementation refinement proof.

D2 remains open for stop and cleanup command bounds, admission races, confirmed termination, durable publication, and ownership-safe cleanup. Resource discharge also requires D3.

Hosted execution, maintainer review, mandatory-scope ratification, and the exact-candidate 60-hour acceptance soak remain pending.
