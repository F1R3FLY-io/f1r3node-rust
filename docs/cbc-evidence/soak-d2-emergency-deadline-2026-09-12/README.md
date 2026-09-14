# Composed emergency deadline

D2 remains incomplete.
This record retains one matched local repair for B49.
The workload, finalization semantics, and 45-second finalization limit remain unchanged.
No node workload, hosted job, diagnostic virtual machine, or acceptance soak ran for this cycle.

| Cycle | Selected behavior | Production and formal results |
| --- | --- | --- |
| B49 | Publish the breach record at once and complete the whole emergency response within one composed deadline, independent of the number of session roots. | Matched RED and unchanged-fixture GREEN. The unbounded control fails with exit 12. |

The fixture runs the production driver in Docker isolation as uid 65534.
Three telemetry roots hold eight session directories each.
The disk probe drops below the hard floor after the workload starts, so the guardian fires during the first iteration.
The evidence copy command ignores the termination signal, records whether the driver's breach record exists, and then stalls.
The unrelated native writer continues in every case.

The baseline driver copied evidence from each telemetry root without any bound and wrote its own breach record only after that copy.
The matched RED exits 1 because the copy started with the record absent and the response missed the fixture budget.
The corrected driver starts one composed deadline at the first breach decision and writes the breach record and early-exit record at once.
It bounds the output drain, each evidence copy, the diagnostics, and the summary writer by the remaining budget, each in its own session.
The matched GREEN exits 0 with the same fixture bytes.
The record is present when the copy starts, two roots are skipped and recorded, and the failure is published within the budget with counters `[1,1,0,0]`.

`SOAK_EMERGENCY_DEADLINE_SECONDS` sets the budget, from 5 through 600 seconds, with a default of 60.
The [model note](../../../formal/tlaplus/soak_disk/EmergencyDeadline.md) defines the setting and the correspondence table.

The combined checks run against the corrected source at the same commit state as the B50 native control-path cycle.
The formal gate, the classifier, and the emergency suite each ran alone, because the classifier overwrites the gate's shared log names.
An earlier gate run before the B50 registration is retained as an attempt.
The driver suite, the supporting checks, the summary and metrics regressions, and the isolated emergency suite pass.
Rerun outputs remain digest-only in the raw archive.

The composed budget bounds the driver's own response.
The guardian's stop and the crash monitor keep their own bounds, and storage durability of the record and the summary is not verified here.
The model counts budget in copy steps, not wall-clock seconds.
No native service manager, real Docker daemon, or disposable runner executed this cycle.
Creation fencing, private Docker containment, reserve bounds, hosted enforcement, maintainer ratification, and acceptance remain pending.
