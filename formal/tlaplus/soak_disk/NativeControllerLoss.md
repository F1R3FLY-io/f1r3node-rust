# Native controller-loss subcycle

This B44 subcycle tests a native-only launcher through its command-line interface.
The [evidence package](../../../docs/cbc-evidence/soak-d2-native-containment-2026-09-11/README.md) records matched production and formal RED/GREEN results.
The original direct-launch B44 counterexample remains RED.
The normal soak workflow does not use this launcher.
This prototype does not complete B44 or D2.

## Selected behavior

The fixture starts one owned native writer and one unrelated native writer.
Both writers ignore the termination signal and use separate process sessions.
The fixture verifies controller identities, suspends both controllers, and kills both through process file descriptors.
Neither controller can complete shutdown between the deaths.
The fixture observes writer termination independently before its cleanup starts.

The baseline adapter launches the unchanged production driver without service-manager containment.
The matched RED exits 1 because the owned writer continues after both controllers die.
Its owned writer grows from 376 to 396 bytes during the final progress check.
The unrelated writer grows from 380 to 400 bytes.
The baseline adapter is retained evidence, not an unmanaged fallback in the current launcher.

The corrected launcher uses the system service manager outside both failed controllers.
Its service contains the driver and its native descendants, including detached descendants.
The service uses `KillMode=control-group`, `KillSignal=SIGKILL`, and `Restart=no`.
The selected GREEN confirms both controller deaths and owned-writer termination.
The reviewed owned writer remains at 2 bytes while the unrelated writer grows from 4 to 14 bytes.
Two actual restarts refuse admission and retain counters `[1,1,0,0]`.

The counters identify iterations, failures, benchmark segments, and benchmark failures, in that order.
The reviewed observation takes approximately 0.551 seconds, including the final 0.5-second progress check.
This measurement is not a worst-case or composed deadline.
The independently observed cgroups put the owned writer with the driver and the unrelated writer outside that service.

## Correspondence

| Model element | Executed behavior | Boundary |
| --- | --- | --- |
| `Init` | The fixture reaches active work with both writers alive. | Startup, placement checks, and admission races are not modeled. |
| `ControllersCrash` | The fixture suspends both controllers, then confirms both deaths through process descriptors. | The system manager, kernel, and external observer survive. |
| `ManagerResponse` with `ManagedContainment=FALSE` | The baseline leaves the detached owned writer active. | The later observation is not an instantaneous stop requirement. |
| `ManagerResponse` with `ManagedContainment=TRUE` | The service manager stops the selected owned native writer after driver death. | This transition assumes a successful manager response. |
| `NativeControllerLossStopsOwnedWriter` | The owned process descriptor reports death and its output remains stable. | This is not Docker containment or durable publication. |
| `UnownedWriterPreserved` | The unrelated writer remains alive and produces additional output. | No claim covers every unrelated host resource. |
| `Completes` | The selected fault reaches the observation state. | Weak fairness supplies eventual response, not a wall-clock bound. |

Both finite configurations contain three states.
The unmanaged configuration violates exactly `NativeControllerLossStopsOwnedWriter` with TLC exit 12.
The managed configuration exits 0.
The model does not represent restart accounting.
The production fixture separately checks two refused restarts through the existing summary interface.
Construction is not applicable to this soak-driver subcycle under the [verification-tier policy](../../../docs/cbc-verification-tiers.md).

## Execution and retention

Each matched pair uses identical fixture bytes in its baseline and corrected source snapshots.
The matched pair uses the same bounded outer service settings.
The outer service exists only for fixture cleanup and stops after the fixture returns its verdict.
The production native service is separate from that outer service.
The matched RED demonstrates that the outer service does not provide the required shutdown before the verdict.

An earlier correction attempt failed setup because the installed service manager rejected its environment arguments.
A later attempt passed its behavioral assertions but reported an outer-service cleanup timeout.
Neither attempt is the matched GREEN result.
The retained matched pair uses immediate outer-service cleanup after the verdict and has invocation exits 1 and 0.
The evidence retains all five initial stages, including their source identities and failures.

Evidence review found that their copied shell files lacked execute permissions.
Their restarts used the production driver's fallback summary, not its normal summary writer.
The reviewed fixture restores shell execute permissions without changing the fault or its assertions.
A new matched RED/GREEN pair passes on a second guarded runner.
Both reviewed restarts execute the driver and use the normal summary writer.

The diagnostic virtual machine had an expiry timer and no GitHub runner registration.
The evidence archive was retrieved and verified before identity-checked termination.
Both runners have final infrastructure observations of `TERMINATED`.
The virtual machine's termination does not establish production writer termination.

## Open obligations

The launcher is a native-only diagnostic prototype, not the complete proposed run domain.
Its current interface requires a trusted root caller, a non-root workload, private paths, and a duration from 1 through 3600 seconds.
The service has fixed diagnostic resource limits and no external network.
Docker and OCI commands in the fixture are substitutes.
No real Docker containment result exists for this subcycle.

The launcher checks process placement after the service starts.
It does not implement a verified pre-admission handshake or a creation fence.
The selected case does not verify inaccessible metadata, missing cgroup files, unsuccessful stops, successful driver exits, or loss of the external observer.
Cleanup before invocation identity becomes available also needs a separate failure test.
The current missing-file handling must not support a general termination guarantee.

Private Docker engine and runtime containment must cover supported CLI, Compose, direct API, restart, and exec operations.
Pending creators, conflicting placement, alternate host sockets, automatic relaunch, and unsupported operations require explicit enforcement and separate tests.
The normal workflow must not switch to this prototype before those requirements pass.
Storage durability, minimal evidence reserve, upload acknowledgment, and one composed deadline remain open.
Claim discharge, hosted enforcement, maintainer ratification, and acceptance remain pending.
