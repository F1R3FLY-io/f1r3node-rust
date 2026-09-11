# Crash monitor exit handling

## Selected contract

The crash monitor does not start another stop after the driver completes its existing exit handling.
This contract concerns duplicate stop requests, not successful writer termination.

## Correspondence

| Model element | Production operation |
| --- | --- |
| `HandleExit` | Complete the existing cleanup path, including its failure accounting. |
| `RememberHandledExit` | Write `handled` and a newline to the monitor's private acknowledgment file. |
| `ObserveExit` | Wait for the parent process descriptor, then read at most nine acknowledgment bytes. |
| `stopRequests` | Count the modeled driver stop and any additional monitor stop. |
| `HandledExitHasNoExtraStop` | A handled exit does not start another monitor stop. |

A missing, unreadable, incomplete, or different acknowledgment does not suppress the monitor's stop request.
A failed stop can still have completed exit handling if the driver retained its failure accounting.
The acknowledgment is not a successful-stop record.

## Evidence

The [B39 record](../../../docs/cbc-evidence/soak-d2-crash-stop-2026-09-11/README.md) retains the failed existing stop-deadline regression.
The failure summary already existed at the observation deadline.
The process snapshot identifies a new stalled Docker client below the crash monitor.
Thus, the regression concerns an extra stop process, not missing failure publication.

The exact control violates `HandledExitHasNoExtraStop` with exit 12.
The corrected model passes with three distinct states.
The production regression passes without changing its fixture or extending its deadline.
The final real-system batch also rechecks abrupt crash response against the corrected driver.

## Limits

The model assumes a readable, intact acknowledgment and atomic transitions.
It does not model storage exhaustion, synchronization failures, power loss, or the complete emergency timeline.
Weak fairness does not establish a wall-clock bound.
The construction tier does not apply to this bounded Bash-driver model.

The acknowledgment adds another storage boundary for later fault testing.
Monitor health, full ownership, remaining crash windows, successful-exit stop failures, durable publication, and reserve bounds remain open.
D2 and claim discharge remain pending.
