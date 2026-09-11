# Combined controller loss

B44 remains an open production defect.
The fixture suspends the driver and crash monitor before it kills both through their process file descriptors.
This sequence prevents either controller from completing a shutdown between the two deaths.
The fixture does not stop the workload writer before its verdict.
The owned writer continues after both controllers exit.
The unrelated writer also continues.

The final fixture permits 120 observation intervals of 0.1 seconds for an independent response.
It then checks writer progress again after 0.5 seconds.
Process inspection adds time, so these intervals are not an aggregate deadline proof.
The initial fixture returned from its observation loop when the driver exited.
Its revised observation loop avoids requiring an instantaneous external response.
Both versions retain their original source bindings and RED results.

`ControllersCrash` models the combined loss of both controllers.
`Observe` models the later observation without assuming an instantaneous stop at driver death.
The current configuration violates `ControllerLossStopsOwnedWriter` with TLC exit 12.
No production or formal GREEN result exists for B44.
Recovery assertions use two actual restarts because a killed driver cannot write its initial summary.
This configuration is not registered as a passing gate or an accepted pre-fix control.

The current driver relies on a surviving controller to enumerate and stop writers.
The ownership marker identifies cooperative writers but does not make their lifetime depend on either controller.
Adding another check inside the driver cannot execute after driver death.
Another ordinary watchdog does not establish a complete failure-containment contract.

Completion requires a reviewed ownership and containment design that survives the specified controller failures.
The design must include native descendants, Docker writers, late creation, inaccessible metadata, and failed stops.
A managed process group alone does not contain detached descendants or Docker daemon workloads.
The tests must preserve unrelated writers and retain actual termination observations.
Kernel-managed containment and an independent runner supervisor are design candidates, not implemented or verified solutions.

The fixture uses a private process namespace, UID 65534, no mounts, no network, and no Docker socket.
Docker commands inside the fixture are external substitutes.
The outer fixture container is cleanup infrastructure, not production shutdown evidence.
Construction is not applicable to this Bash-driver model.
D2 and claim discharge remain pending.
