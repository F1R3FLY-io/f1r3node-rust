# B44 native containment evidence

This package records one native-only B44 subcycle from source `1f9427958634516ab04f757688b0e2ad3376b718`.
The user approved the containment implementation scope before this subcycle.
The selected native case passes, but B44 and D2 remain incomplete.
The original direct-launch counterexample remains RED.
The normal workflow does not use the new prototype.

## Results

| Check | Result |
| --- | --- |
| Matched unmanaged production baseline | The fixture exits 1 because the owned writer continues after both controllers die. |
| Matched native service correction | The unchanged fixture exits 0 and preserves the unrelated writer. |
| Two actual restarts | Both refuse admission and retain counters `[1,1,0,0]`. |
| Unmanaged finite model | TLC exits 12 on exactly `NativeControllerLossStopsOwnedWriter`. |
| Managed finite model | TLC exits 0 after three distinct states. |
| Combined formal checks | All 40 positive configurations and 42 exact controls pass. |
| Classifier and routing | All 294 classifier cases and six routing scenarios pass. |
| Existing emergency suite | All 46 cases pass. |
| Supporting regressions | Admission, driver, metrics, and summary checks pass. |
| Infrastructure cleanup | Evidence retrieval precedes observed virtual-machine state `TERMINATED`. |

The matched RED owned writer grows from 376 to 396 bytes during the final progress check.
The unrelated writer grows from 380 to 400 bytes.
The reviewed GREEN owned writer remains at 2 bytes while the unrelated writer grows from 4 to 14 bytes.
The reviewed GREEN observation takes approximately 0.551 seconds, including its 0.5-second progress check.
This observation is not a worst-case response bound.

The production native service is separate from the outer diagnostic service.
The outer service cleans up only after the fixture returns its verdict.
The matched baseline demonstrates that this cleanup does not supply production shutdown.
The fixture confirms controller and writer deaths through process file descriptors.
Independent cgroup observations place the owned writer with the driver and the unrelated writer outside that service.

## Evidence identities

The [manifest](manifest.jsonc) identifies the original and published bytes, source snapshots, tool identities, replacement counts, and archive members.
The baseline adapter launches the unchanged production driver without containment.
The corrected helper has no unmanaged fallback.
Each matched pair uses identical fixture bytes and outer-service settings.

An earlier corrected invocation failed setup because the service manager rejected its environment arguments.
Another invocation passed its behavioral assertions but failed outer-service cleanup.
Neither invocation is the matched GREEN result.
All five initial stages retain separate identities and results.
An earlier unexecuted correction snapshot remains distinct from executed source.

Evidence review found that the initial fixture copied shell files without execute permissions.
Its restarts used the driver's fallback summary.
The reviewed fixture restores execute permissions without changing the fault or its assertions.
A second guarded runner establishes a new matched RED/GREEN pair with the normal summary writer.
Both runners have verified evidence retrieval and observed `TERMINATED` state.
The manifest distinguishes all seven native invocations.

The combined formal, classifier, routing, emergency, and supporting reruns remain digest-only in the archive.
The manifest records their checked counts and results.
Actual TLC logs were saved before the verifier-process substitutes ran.
Published current-cycle results use the `b44` prefix.
No prior-cycle result is renamed into current-cycle evidence.

The raw archive remains outside Git and Cargo build output.
The manifest records its location as `[EVIDENCE_ROOT]/raw-streams.tar.gz`.
The handoff identifies the corresponding external evidence directory.
Explicit exclusions and later audit outputs mean the archive is not a complete filesystem image.
Archive availability to reviewers remains a separate obligation.

## Limits and next work

The [correspondence](../../../formal/tlaplus/soak_disk/NativeControllerLoss.md) states the model assumptions and prototype limits.
The system manager, kernel, and external observer survive the selected fault.
The model assumes successful native termination and uses weak fairness for eventual response.
It does not model restart accounting, pending creation, failed stops, storage faults, or elapsed time.
Construction is not applicable to this soak-driver subcycle.

The prototype checks placement after service start, not through a verified pre-admission handshake.
It does not implement a creation fence or private Docker engine containment.
Docker and OCI commands in the native fixture are substitutes.
Metadata loss, missing cgroup files, observer loss, unsuccessful stops, successful exits, and unrestricted alternate sockets require further work.
The prototype must not replace the normal workflow launcher before the complete containment contract passes.

The workload, finalization semantics, and 45-second finalization wait remain unchanged.
All four claim discharges, all nine gates, hosted enforcement, maintainer ratification, and acceptance remain pending.
This package is partial verification, not a deployment or acceptance authorization.
