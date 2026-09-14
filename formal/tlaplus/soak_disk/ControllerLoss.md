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
The original direct-launch configuration still has no production or formal GREEN result.
A separate [native-only subcycle](NativeControllerLoss.md) passes without completing B44.
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
A native-only service-manager prototype now has selected fault evidence.
The complete containment design remains unimplemented and unverified.

The fixture uses a private process namespace, UID 65534, no mounts, no network, and no Docker socket.
Docker commands inside the fixture are external substitutes.
The outer fixture container is cleanup infrastructure, not production shutdown evidence.
Construction is not applicable to this Bash-driver model.
D2 and claim discharge remain pending.

## Proposed runner containment

The user approved this implementation scope on 2026-09-11.
The complete proposal is not an implemented correction or a ratified correctness claim.
The native-only prototype does not satisfy the complete contract.
The proposed boundary uses the system service manager and cgroup v2 outside the two failed controllers.
The workload and its launch services must remain inside an exclusive [run domain](../../../docs/Glossary.md#run-domain).
A controller check inside that domain cannot provide the independent response.

### Failure contract

The selected failure includes simultaneous driver and crash-monitor death after active work begins.
The correction must preserve the workload, finalization semantics, and 45-second finalization wait.
The system service manager and kernel must remain available to enforce containment.
The user confirmed this survival assumption for implementation scope.
Correctness-claim ratification and acceptance remain separate obligations.

Whole-host suspension, kernel failure, and power loss need separate recovery and reserve evidence.
They must not produce a successful shutdown verdict by assumption.

The trusted manager must establish domain ownership before it permits work.
Names, environment markers, labels, and caller-supplied process identifiers are not sufficient authorization.
Workload processes must not change domain ownership, escape placement, or access an unrestricted host Docker socket.
The design does not claim protection against a hostile host administrator or a compromised kernel.

### Proposed structure

The proposed launcher creates a private run domain through the trusted service manager.
The driver, native descendants, private Docker engine, container runtime, and runtime helpers must enter their assigned containment before workload execution.
The engine must use private control sockets and storage paths.
It must not use the shared Docker engine or a shared container runtime for workload creation.
This structure needs runner integration and actual daemon tests before it can replace the current launcher.

The service manager must couple domain shutdown to driver termination, including termination before normal exit handling.
The design must prevent automatic engine restart after closure.
A bounded stop must include the private launch services and their detached descendants.
The manager must verify actual cgroup placement before it permits the first admission.
Missing support or inconsistent placement must refuse work rather than select the unmanaged driver.

The launch boundary must enforce Docker placement rather than supply a caller-overridable default.
Conflicting parent settings and escape-capable API requests must fail before execution.
The same rule applies to command-line clients, Compose, direct API calls, container restart, and container execution.
The design must identify each supported operation and reject unsupported operations.
A command-name wrapper alone does not establish this boundary.

### Closure and observation

Closure must revoke launch admission and account for requests already accepted by the engine.
The shutdown path must contain pending creators before it reports complete termination.
The design must identify the exact operation that prevents later workload execution.
A single process enumeration or cgroup kill does not establish a [creation fence](../../../docs/Glossary.md#creation-fence).
An empty cgroup observation alone does not prove that an external creator cannot populate it later.

The shutdown observer must distinguish a signal request from confirmed process termination.
A failed stop, inaccessible status, or expired deadline must retain an unconfirmed result and prevent restart admission.
The observer must preserve unrelated native writers, Docker writers, images, networks, and storage.
Frozen processes are not terminated processes.
The outer test container must not supply the production shutdown mechanism.

The response needs one deadline across detection, closure, stop, confirmation, and minimal evidence handling.
Separate command timeouts do not establish that deadline.
A stalled storage operation must not become a successful publication result.
Preallocated storage, allocation limits, and upload acknowledgment need separate fault tests and the D3 reserve argument.
This proposal does not supply those bounds.

### Documented constraints

The [systemd kill procedure](https://www.freedesktop.org/software/systemd/man/latest/systemd.kill.html) applies `KillMode=control-group` to processes in the unit's cgroup.
It does not establish ownership or placement of containers created by an external Docker engine.
A service wrapper around the driver is therefore insufficient by itself.

The [Docker daemon reference](https://docs.docker.com/reference/cli/dockerd/#default-cgroup-parent) distinguishes path-based cgroupfs parents from systemd slice parents.
A per-container parent overrides the daemon default.
The implementation must select and verify a supported hierarchy rather than assume identical behavior between the two drivers.
Changing the shared engine's cgroup driver is outside this proposal.

The [kernel cgroup v2 reference](https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html) documents concurrent-fork and migration handling for `cgroup.kill`.
That operation is not a permanent creation fence.
The same reference defines `cgroup.events` population as the presence of live processes in a subtree.
A population sample does not establish storage durability, absence of future creators, or a wall-clock bound.
These references describe interfaces, not verification of this repository's implementation.

### Proposed implementation scope

The user confirmed the following implementation scope before the native subcycle began.
Another agent retains commit and push ownership.

| File or area | Proposed method |
| --- | --- |
| New `scripts/bench/run-soak-contained.sh` and `scripts/bench/soak-containment.py` | Add the trusted launch, placement, closure, and termination interface. |
| `scripts/run-merge-recovery-soak.sh` | Require containment before admission and preserve existing failure accounting. |
| `.github/workflows/merge-recovery-soak.yml` | Integrate the managed launcher and retrieve independent shutdown evidence. |
| `scripts/bench/test-soak-controller-loss.sh` and new isolated runner fixtures | Exercise actual controller loss, native descendants, and real Docker workloads. |
| `formal/tlaplus/soak_disk/ControllerLoss*` and matching configurations | Model explicit closure, pending creation, stop, and observation transitions. |
| Formal and emergency gate scripts | Register successful cases only after matched production and formal GREEN results. |
| Claim inventory and existing tracking files | Bind the new inputs without changing historical execution identities. |

The existing B44 counterexample remains required evidence.
Any necessary fixture change must preserve the fault, termination, unrelated-writer, and recovery assertions.
The revised fixture must establish behavioral RED against the uncorrected source before the correction.
Missing deployment prerequisites are setup failures, not behavioral RED.

The verification sequence starts with the native B44 case and then adds real Docker creation and shutdown cases.
Later cases must cover concurrent creation, lost metadata, failed stops, output holders, successful exits, and two refused restarts.
Each behavior needs its own unchanged-fixture RED/GREEN comparison and matching formal result.
Real daemon tests require guarded disposable infrastructure rather than additional privileges on the developer host.
Full D2 completion still requires storage faults, the composed deadline, D3 reserve bounds, hosted execution, and maintainer ratification.
