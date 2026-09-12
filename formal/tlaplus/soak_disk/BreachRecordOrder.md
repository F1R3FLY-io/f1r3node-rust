# Breach record before attribution

B48 tests the order of the minimal breach record and the disk usage attribution after a disk floor breach.
The fixture runs the production driver in Docker isolation as uid 65534.
The disk probe reports free space inside the hygiene band, hygiene reclaims nothing useful, and the probe then reports free space below the band.
The attribution probe ignores the termination signal, records whether the breach record exists when it starts, and then stalls.

The baseline driver runs the hygiene-pass attribution before it checks the breach condition.
Its diagnostic deadline sends signals to a process group that the stalled probe's child does not belong to, so the attribution never returns.
The driver never reaches its breach decision, and only the guardian publishes a record.
The matched RED exits 1 because attribution started without the driver's record and the driver was still running at the check.

The correction has two parts.
The driver runs each attribution in its own session and kills that whole session after the diagnostic deadline.
The driver checks the guardian and the disk floor before the hygiene-pass attribution.
It writes the breach record, the early-exit record, and the persisted state before the floor-breach attribution.
The unchanged fixture then observes the record present when attribution starts.
The driver publishes its summary with counters `[0,1,0,0]` while the probe still stalls, and the unrelated native writer continues.

## Correspondence

| Model element | Production or fixture boundary |
| --- | --- |
| `Init` | The driver has decided a disk floor breach. The attribution may stall. |
| `Record` | The driver writes the breach record and early-exit record. |
| `Attribute` | The driver starts the disk usage attribution. |
| `Finish` | A non-stalled attribution returns. A stalled attribution never returns. |
| `RecordFirst = TRUE` | The corrected driver records before it attributes. |
| `RecordFirst = FALSE` | The baseline driver attributes before it records. |
| `AttributionRequiresRecord` | No attribution starts before the record exists. |
| `RecordPublished` | The record is eventually published. The baseline with a stalled attribution never publishes it. |

The attribute-first configuration violates `AttributionRequiresRecord` with TLC exit 12.
The corrected configuration passes with seven distinct states.

## Limits

The fixture tests the hygiene-pass and floor-breach paths after a failed reclaim.
The guardian's own record order was verified earlier and is not changed here.
The model does not represent the session kill or a wall-clock bound.
The diagnostic deadline bounds one attribution, not the composed emergency response.

Storage durability of the record itself is not verified here.
Construction is not applicable to this Bash-driver model.
D2 and claim discharge remain pending.
