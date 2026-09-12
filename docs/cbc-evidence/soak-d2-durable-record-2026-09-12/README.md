# Durable record publication

D2 remains incomplete.
This record retains one matched local repair for B51.
The workload, finalization semantics, and 45-second finalization limit remain unchanged.
No node workload, hosted job, diagnostic virtual machine, or acceptance soak ran for this cycle.

| Cycle | Selected behavior | Production and formal results |
| --- | --- | --- |
| B51 | Publish every minimal record through a synced temporary file, an atomic rename, and a directory sync. A record under its final name is complete or absent. | Matched RED and unchanged-fixture GREEN. The in-place control fails with exit 12. |

The fixture runs the production driver in Docker isolation as uid 65534.
The disk probe drops below the hard floor after the workload starts, so the guardian fires during the first iteration.
A substituted `sync` command logs each call with the target name, the file size, and the content digest.
A poller logs every size change of the six minimal records at ten-millisecond intervals.

The baseline driver and summary writer wrote every record in place with a redirection or `tee`.
The matched RED exits 1 because no record had a synced temporary file, a rename, and a directory sync.
The RED poller also observed one empty summary JSON under its final name.

The corrected driver publishes the guardian breach record, the protection breach record, the early-exit record, the persisted state, and both summaries through one helper.
The matched GREEN exits 0 with the same fixture bytes.
Each record shows its final digest under its temporary name before a directory sync, no temporary file remains, and the poller observed no empty record.
The first GREEN run is retained as an attempt after an external indentation change to the summary writer was reverted.

The corrected sources entered the history in commit `5bb138dfb` from outside the verifying session before this package was bound.
That commit followed a formatting-only rewrite of six driver pipeline continuations, so the matched GREEN and every driver-dependent check were rerun on the committed bytes.
The earlier GREEN runs, suite runs, and verification snapshot are retained as attempts.

The [model note](../../../formal/tlaplus/soak_disk/DurableRecord.md) defines the correspondence table.
The combined checks run against the corrected source.
The formal gate, the classifier, and the emergency suite each ran alone, because the classifier overwrites the gate's shared log names.
The driver suite, the supporting checks, the summary and metrics regressions, and the isolated emergency suite pass.
Rerun outputs remain digest-only in the raw archive.

The substituted `sync` command proves ordering and atomic visibility, not kernel durability.
Storage faults during publication, upload acknowledgment, and the guardian and crash-monitor bounds are not verified here.
No native service manager, real Docker daemon, or disposable runner executed this cycle.
Creation fencing, private Docker containment, reserve bounds, hosted enforcement, maintainer ratification, and acceptance remain pending.
