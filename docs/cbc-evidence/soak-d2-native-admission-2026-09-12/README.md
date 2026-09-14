# Native launch admission

D2 remains incomplete.
This record retains one native cycle that the second session produced and the integration session packaged.
The workload, finalization semantics, and 45-second finalization limit remain unchanged.
No node workload, hosted job, or acceptance soak ran for this cycle.

| Cycle | Selected behavior | Production and formal results |
| --- | --- | --- |
| B47 | The launcher verifies manager placement and gate identity before it releases the driver, so a stalled service-status query prevents native admission. | Matched native RED and GREEN with the unchanged fixture. The start-first control fails with exit 12. |

One guarded diagnostic runner ran eleven native invocations.
The runner had an expiration timer, no GitHub registration, and a source archive that matched the frozen committed baseline.
The retrieval archive was verified, independently audited, and then the runner was observed `TERMINATED`.
The [handoff](./b47-HANDOFF.md) lists each invocation, source snapshot, result, archive check, and termination record.

The baseline launcher starts the driver before the service-status query returns.
The stalled query lets the driver admit one iteration even though the launcher later refuses with exit 2.
The correction starts trusted root gate code without the workload environment.
The launcher verifies the manager observation and gate identity, then publishes the run-domain record and a private release record.
The gate then drops privilege and executes the driver.
The native controller-loss regression passes with both the committed B45 driver and the frozen B46 driver.

Eleven invocations retain their original identities.
They include one integration setup failure, where control directory mode 0711 prevented the B46 directory walk.
They also include an integer-parsing refactor with its own results.
The final launcher uses mode 0755 for the control directory and keeps the environment, gate identity, and release files private to root.
The [model note](../../../formal/tlaplus/soak_disk/NativeLaunchAdmission.md) defines the correspondence table.

The combined checks run at the same commit state as the B48 breach record cycle.
The classifier and routing regressions passed with the native configurations registered.
The retained formal gate run is the isolated run at that commit state.
It ran after the B46 pathname control was narrowed and the B48 configurations were registered.
Two earlier gate attempts are retained: one collided with the classifier on the shared log names, and one hit the nondeterministic B46 control.
The emergency suite was not rerun for this integration because the driver and the local fixtures are unchanged since the base commit.

The root launcher requires a trusted administrative startup context.
Adversarial release access, metadata failures, failed stops, and observer-loss windows need separate fault evidence.
The second session's review found that the launcher checks only the immediate control parent, and that control-path fault cycle stays with that session.
Private Docker containment, creation fencing, durable publication, aggregate deadlines, D3 reserve bounds, hosted enforcement, maintainer ratification, and acceptance remain pending.
