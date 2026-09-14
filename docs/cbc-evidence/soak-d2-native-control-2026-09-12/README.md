# Native control-path refusal

D2 remains incomplete.
This record retains one native cycle that the second session produced and the integration session packaged.
The workload, finalization semantics, and 45-second finalization limit remain unchanged.
No node workload, hosted job, or acceptance soak ran for this cycle.

| Cycle | Selected behavior | Production and formal results |
| --- | --- | --- |
| B50 | The launcher refuses work below an untrusted control-directory ancestor, before it creates the control directory or starts the manager. | Matched native RED and GREEN with the unchanged fixture. The parent-only control fails with exit 12. |

One guarded diagnostic runner ran seven native invocations.
The runner passed the existing identity, expiration, and exclusive-use checks and did not register with GitHub.
The retrieval archive was verified and audited, and the runner was then observed `TERMINATED`.
The [handoff](./b50-HANDOFF.md) lists each invocation, source snapshot, result, archive check, and termination record.

The baseline launcher checked only the immediate parent of its control directory.
Below an untrusted ancestor, the workload initialization hook executed as uid 65534 before the driver refused its run-domain record.
The matched RED exits 1 on that premature workload initialization.
The corrected launcher checks every component of the control-directory path from the filesystem root before creation or manager launch.
The matched GREEN exits 0.
The unavailable-query and controller-loss regressions pass with both the committed B46 driver and the B48 driver.

The corrected launcher and the B48 driver entered the history in commit `9a6dacd74` before this cycle was registered.
The index was staged as a whole at that commit.
The committed launcher digest matches the tested helper, and the committed driver matches the tested B48 driver.
The [model note](../../../formal/tlaplus/soak_disk/NativeControlPath.md) defines the correspondence table.

The combined checks run at the same commit state as the B49 emergency deadline cycle.
The retained formal gate run is the isolated run with the B49 and B50 configurations registered.
The classifier and routing regressions passed with those configurations registered.
The emergency suite was not rerun for this integration because this cycle did not change the driver or the local fixtures.

Concurrent directory replacement, mount changes, and inherited descriptors remain unverified.
The second session's [private Docker containment specification](../../plans/soak-private-docker-containment-2026-09-12.md) is bound as a design input with its twelve regression cases unexecuted.
Private Docker containment, creation fencing, durable publication, D3 reserve bounds, hosted enforcement, maintainer ratification, and acceptance remain pending.
