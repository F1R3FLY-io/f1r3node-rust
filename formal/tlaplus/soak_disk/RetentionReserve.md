# Retention reserve under disk pressure

This model supports gate D3 and the disk reserve argument.
The emergency response copies failure evidence from each telemetry root during the response.
Each copy consumes temporary disk space, which the reserve argument counts in the R term.
The model shows that the response must skip a copy that would breach the operating reserve.

The response holds the operating reserve only when it skips a copy under disk pressure.
A copy that runs while the free space is near the floor pushes the free space below the reserve.
The response then worsens the exhaustion that it is trying to survive.
Skipping the copy and recording the skip keeps the reserve for the runner and the final upload.

## Finding

The current driver bounds each evidence copy by the emergency deadline, not by free space.
It skips a copy only when the composed deadline has expired, through `emergency_remaining`.
Under disk pressure the copy still runs while time remains, so it can breach the operating reserve.
A future D3 cycle should add a space-aware skip that uses the copy footprint from the diagnostic run.

## Correspondence

| Model element | Production boundary |
| --- | --- |
| `Reserve` | The free space at the guardian trigger |
| `Required` | The operating reserve for the runner and the final upload |
| `Copies` | The failure-evidence copies, one per telemetry root |
| `CopySize` | The temporary space that one copy consumes |
| `Copy` with `SkipUnderPressure` | The response skips a copy that would breach the reserve |
| `ReserveHeld` | The free space never falls below the operating reserve |

The skip-under-pressure configuration passes.
The unskipped configuration violates `ReserveHeld` with TLC exit 12.

## Limits

The model uses small concrete constants and one copy size.
It represents the space bound only, not the emergency-deadline time bound that the driver already applies.
It does not represent the driver, the stall, or the summary writer.
The copy footprint and the operating reserve still need the D3 diagnostic run.

This model is not registered in the formal gate yet, because the admission-check cleanup edits the same gate lists.
The space-aware skip is a driver change and waits for that cleanup and for the diagnostic reserve values.
D2, D3, and claim discharge remain pending.
