# Durable publication: bounded response and atomic records

D2 remains incomplete.
This record retains the corrected publication for the B51 review findings R1 and R3.
The workload, finalization semantics, and 45-second finalization limit remain unchanged.
No node workload, hosted job, diagnostic virtual machine, or acceptance soak ran for this cycle.

| Finding | Selected behavior | Production and formal results |
| --- | --- | --- |
| R1 | Publish each record with an atomic rename first. Keep the writer stop independent of a stalled sync. Record an unconfirmed result when durability cannot complete. | Matched RED and unchanged-fixture GREEN. The blocking control fails with exit 12. |
| R3 | Prove the atomic rename for every published record and reject in-place publication. | Matched GREEN with an in-place negative control. The in-place model control fails with exit 12. |

The R1 fixture runs the production driver in Docker isolation as uid 65534.
A substituted `sync` command hangs after the workload starts, which models a storage stall that no signal can interrupt.
The guardian fires during the first iteration.
The baseline driver synced each record before the rename and inside the stop path.
The stall therefore blocked the guardian breach record and the writer stop without limit.
The matched RED exits 1 because the driver never completed its bounded response within the deadline.

The corrected driver renames each record into place before any sync and runs durability through a bounded reap.
The reap abandons a stalled sync at the budget and records `publication-unconfirmed.txt`.
The driver also detects a breach recorded after the iteration process exits, so a missed poll-loop check no longer skips the emergency response.
The matched GREEN exits 0.
The breach record is visible, the writer stop and the failure publication complete within the deadline, and the unconfirmed result is recorded.

The R3 fixture observes each rename through a substituted `mv` command.
The verdict requires an atomic rename from a temporary name to the final path for every record.
It rejects an in-place control that publishes the same records without a rename.
The corrected verdict passes the atomic driver and rejects the in-place control.

The [bounded publication model](../../../formal/tlaplus/soak_disk/BoundedPublication.md) refutes a stop that waits on a stalled sync.
The [atomic record model](../../../formal/tlaplus/soak_disk/DurableRecord.md) refutes in-place publication.
The formal gate passes 52 positive configurations and 52 exact controls, including both new models.

The combined checks run against the corrected working tree above base commit `b32a2e9e8`.
The classifier and routing regressions pass with the new registrations.
The driver suite and the summary writer regression pass.
Every emergency sub-fixture passes in an individual run.
The full single-invocation emergency suite exceeds the memory available on this host, so the kernel stops it.
The disk-admission scenarios passed in memory-limited partial suite runs with no failures.

The `sync` substitution uses a killable process.
A real stalled sync can stay in uninterruptible kernel state, which the bounded reap abandons rather than terminates.
The `mv` substitution observes the rename and does not prove filesystem rename atomicity.

Durability under power loss, upload acknowledgment, and the guardian and crash-monitor bounds are not verified here.
Producer failure is verified separately by the record-producer cycle.
No native service manager, real Docker daemon, or disposable runner executed this cycle.
Creation fencing, private Docker containment, reserve bounds, hosted enforcement, ratification, and acceptance remain pending.
