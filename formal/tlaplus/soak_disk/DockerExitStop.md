# Docker Exit Stop Correspondence

B28 sends `SIGTERM` to the driver during an active iteration. The workload fixture starts a bounded Docker writer outside the workload client's process group.

The baseline driver exits while the container remains active and its file grows. The correction requests writer termination from the active-work exit cleanup.

The real daemon then reports `Running=false`, `Pid=0`, and exit code 137. Two later file copies match. This confirms termination of the selected fixture writer.

`DockerExitStop` separates the exit request, successful writer stop, and parent exit. The corrected model has four distinct states.

The negative control omits the writer stop. It requires exit 12 on `ParentExitStopsFixtureWriter`.

The model assumes successful writer termination before `FinishExit`, effective signals, and weak fairness. The fixture checks termination after it observes driver exit.

The fixture does not measure the exact ordering of those production events. The model does not prove termination after a failed daemon operation or a client timeout.

The stop helper retains its existing name and process selectors. Ownership-safe stop selection, late-created writers, other shutdown paths, and uninterruptible descendants remain open.

This fixture covers active-iteration `SIGTERM` only. It does not establish complete shutdown, a complete response deadline, or durable publication.

See the [B27–B29 evidence](../../../docs/cbc-evidence/soak-d2-real-system-2026-09-10/README.md).
