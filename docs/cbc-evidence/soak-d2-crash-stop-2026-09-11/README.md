# B38 and B39 crash monitor evidence

B38 adds a crash monitor outside the driver process group.
The monitor waits on the driver's process descriptor and invokes the existing ownership-checked writer stop helper.
The real-system fixture confirms owned Docker termination without fixture intervention and preserves an unrelated writer.

B39 corrects a regression found during composed verification.
The monitor started another stop after the driver completed exit handling.
The correction acknowledges handled exits without treating that acknowledgment as successful writer termination.

## Results

| Check | Result |
| --- | --- |
| B38 production | RED exits 1. The corrected fixture exits zero. |
| B38 formal | The exact `DriverCrashStopsOwnedWriter` control exits 12. GREEN has three distinct states. |
| B39 production | The existing stop-deadline regression exits 1 before correction and zero afterward. |
| B39 formal | The exact `HandledExitHasNoExtraStop` control exits 12. GREEN has three distinct states. |
| Final real-system batch | All 11 cases pass against the final driver. |
| Final formal gate | 34 positive configurations and 36 exact controls pass. |
| Classification and routing | 252 classifier cases and six routing scenarios pass. |
| Emergency regressions | All 41 cases pass. |
| Supporting regressions | The isolated driver, admission, workflow, release, pin, metrics, and summary checks pass. |

The [manifest](manifest.jsonc) binds source snapshots, original evidence, complete selected streams, substitution counts, tools, and archive identities.
All 228 verification input snapshots match the final source.
Snapshots identify bytes, not execution coverage for every captured file.
The package preserves the failed intermediate emergency regression and the successful final regression.

## Diagnostic lifecycle

The first archive contains 524 unique regular files and 2,221,867 bytes.
The final archive contains 418 unique regular files and 2,107,157 bytes.
Both archives passed validation before extraction, and every extracted digest matched.
Both diagnostic virtual machines were observed terminated after retrieval.
All eleven diagnostic virtual machines used so far have termination observations.

The bootstrap archive retains the earlier `980285d4c` identity.
Separate hashed runtime payloads identify the initial and final tested drivers.
The bootstrap archive is not a current-source execution claim.

## Retained distinctions

The unchanged B37 fixture failed because production now stops its previously surviving writer.
Its adapted external Docker boundary rejects kill requests explicitly.
The fixture still requires refused restarts and one retained interruption failure.

The initial formal batch stopped at a changed-HEAD guard before TLC ran.
The next batch passed after the current source bytes matched the retained runtime payloads.
The guard result is not a behavioral RED.

External commits `5a211fb5c` and `bef79ba48` contain the production changes.
This session does not attest their commit hooks.
No assistant commit, push, hosted dispatch, node workload, or acceptance soak occurred in these cycles.

## Limits

The [B38 correspondence](../../../formal/tlaplus/soak_disk/DriverCrashStop.md) and [B39 correspondence](../../../formal/tlaplus/soak_disk/CrashMonitorExit.md) define the separate contracts.
Both models remain bounded refutation evidence, and the construction tier does not apply.
The fixtures do not prove full process discovery, late-creation fencing, monitor survival, storage durability, or an aggregate emergency deadline.
The exit-handling acknowledgment is not termination evidence.

Storage faults, remaining ownership and crash windows, safe reclamation, reserve bounds, hosted verification, enforcement, and maintainer review remain open.
The workload and 45-second finalization limit remain unchanged.
D2, claim discharge, and acceptance remain pending.
