# Breach record before attribution

D2 remains incomplete.
This record retains one matched local repair for B48.
The workload, finalization semantics, and 45-second finalization limit remain unchanged.
No node workload, hosted job, diagnostic virtual machine, or acceptance soak ran for this cycle.

| Cycle | Selected behavior | Production and formal results |
| --- | --- | --- |
| B48 | Publish the minimal breach record before any disk attribution starts, and keep a stalled attribution from delaying the record or failure publication. | Matched RED and unchanged-fixture GREEN. The attribute-first control fails with exit 12. |

The fixture runs the production driver in Docker isolation as uid 65534.
Its disk probe reports free space inside the hygiene band, and the substituted reclaim recovers nothing useful.
The probe then reports free space below the band.
The attribution probe ignores the termination signal, records whether the driver's breach record exists when it starts, and then stalls.
The unrelated native writer continues in every case.

The planned defect was the attribution before the record in the floor-breach block.
The observed defect was larger.
The baseline driver ran the hygiene-pass attribution before its breach checks.
The probe's child inherited the ignored termination signal and escaped the diagnostic deadline, which signaled a process group the child did not belong to.
The driver hung before its breach decision, and only the guardian published a record.
The matched RED exits 1 because attribution started without the driver's record and the driver was still running at the check.

The corrected driver runs each attribution in its own session and kills that session with a watchdog after the diagnostic deadline.
It checks the guardian and the disk floor before the hygiene-pass attribution.
It writes the breach record, the early-exit record, and the persisted state before the floor-breach attribution.

The matched GREEN exits 0 with the same fixture bytes.
The record is present when attribution starts, and the driver publishes counters `[0,1,0,0]` while the probe still stalls.
One prior-fixture pair is retained with its original identities.
The [model note](../../../formal/tlaplus/soak_disk/BreachRecordOrder.md) defines the correspondence table.

The combined checks run against the corrected source.
The formal gate, the classifier, and the emergency suite each ran alone, because the classifier overwrites the gate's shared log names.
The driver suite passes after its floor-breach scenario stopped expecting the hygiene-pass usage line before the breach.
The supporting checks, the summary and metrics regressions, and the isolated emergency suite pass.
Rerun outputs remain digest-only in the raw archive.

The session kill bounds one attribution, not the composed emergency response.
Storage durability of the record itself is not verified here.
No native service manager, real Docker daemon, or disposable runner executed this cycle.
Creation fencing, private Docker containment, deadline and reserve bounds, hosted enforcement, maintainer ratification, and acceptance remain pending.
