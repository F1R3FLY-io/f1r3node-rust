# Disk Stop Command Deadline

## Scope

B13 checks stalled stop commands after an active disk breach. The production fixture runs the driver with external disk, Docker, and workload commands replaced.

The Docker clients ignore `TERM`. The fixture checks failure publication and client exit five seconds after the first stop command starts.

The fixture does not run nodes. Client exit does not confirm that the Docker daemon stopped a container or that every writer stopped.

## Model correspondence

| Model element | Production operation |
| --- | --- |
| `phase = "stopping"` | A `stop_node_writers` invocation has not returned. |
| `CommandsReturn` | The stop-command group returns before the deadline. |
| First `Tick` | The timeout sends `TERM` after the configured command budget. |
| Second `Tick` | The timeout sends `KILL` after its one-second grace period. |
| `EnforceDeadline = FALSE` | The historical command group has no timeout. |
| `StopWithinBudget` | A stalled command group cannot remain active after the modeled cancellation point. |

The positive configuration uses `EnforceDeadline = TRUE`. The negative control uses `FALSE` and violates `StopWithinBudget` with TLC exit 12.

The production wrapper accepts `SOAK_DISK_STOP_SECONDS` values from one through five. The default is two seconds. Each invocation has a separate one-second kill grace.

The fixture uses a one-second command budget. Its five-second observation permits the guardian and parent stop paths to complete.

The wrapper exports the command function to its child shell. Its command line does not contain the `/tmp/rnode` match expression.

The child shell ignores `TERM` so an early shell exit cannot cancel the later group `KILL` while a stalled client remains alive.

## Limits

The model assumes timer service, effective process-group cancellation, and returning local operations. Abstract ticks do not establish Linux scheduling or wall-clock bounds.

The fixture clients do not detach. The model does not cover uninterruptible processes, daemon-side cancellation, process identifier reuse, or every writer.

The correction preserves the existing process and container selectors. It does not establish their ownership contract.

The memory guardian still has a separate stop path. Disk hygiene, iteration shutdown, diagnostics, filesystem writes, and upload do not share this deadline.

Local summary publication does not establish durability. B13 does not complete D2, discharge a claim, or authorize another soak.
