# Private Docker containment

## Status and scope

This document specifies the next private Docker work for B44 and D2.
It extends the [approved containment proposal](../../formal/tlaplus/soak_disk/ControllerLoss.md#proposed-runner-containment).
It is a design and regression specification, not an implemented correction.
All proposed tests below remain unexecuted.
B44, D2, correctness-claim discharge, hosted enforcement, and acceptance remain open.

The design preserves the workload, finalization semantics, and 45-second finalization wait.
The three harness pins remain `962effd17708192627bd249362761c0ccb1fd5fa`.
The design does not disable benchmarks, container restart policies, collectors, or required workload operations to obtain a passing result.

The integration owner retains the driver, shared gates, shared plan, evidence packages, and claim inventory.
This specification does not reserve a new B-number or change those files.
Runtime implementation must use a separate frozen candidate after coordination with that owner.
The native [work log](../work-logs/task-soak-native-admission-2026-09-12T00-58Z.md) records this handoff.

## Source observations

Commit `9a6dacd7492fb7ae88451bd3536fabce3e143d15` contains the B47 and B48 integration.
The driver has subsequent worktree changes for the emergency deadline.
Those changes are outside this design's ownership and have no test result from this session.
The committed driver is therefore the source reference, not a claim about the changing worktree.

The following observations affect compatibility:

| Source | Observed requirement |
| --- | --- |
| `scripts/bench/run-bench-segment.sh` | Compose starts the shard. The client checks a published port through `localhost`. The script uses logs, stats, and volume removal. |
| `docker/shard.yml` | The shard uses `restart: always`. Other default services use `unless-stopped`. It uses named volumes and source-file mounts. |
| Pinned harness `test/infra/compose.py` | Generated shards use `restart: no`, bridge networks, published ports, named volumes, and a network sysctl. |
| Pinned harness `DockerProvider.create_standalone` | Standalone nodes use root inside containers, named volumes, port publication, and configured log rotation. |
| Pinned harness `DockerNodeHandle` | Restart, pause, unpause, exec, logs, and stats support workload control and telemetry. Log collection itself can create exec processes. |

The reviewed harness files match the pinned revision, although the sibling checkout has a later HEAD.
The operation list is not yet a complete API inventory for every selected test.
A blanket rejection of restart, exec, or sysctls would break required behavior.
Disabling those operations is not a containment correction.

A previous disposable runner reported Docker 29.6.1, API 1.55, containerd 2.2.5, and runc 1.3.6.
That runner is terminated.
These observations do not establish the versions or capabilities of a new runner.
Docker documents embedded containerd from 29.7.0, so this proposal does not depend on that experimental feature.

## Failure contract

The selected fault kills the driver and crash monitor during active work.
The fixture first suspends both controllers to prevent an intervening cooperative shutdown.
The system service manager and kernel must survive that fault.
The containment response must not require either controller to resume.

The trusted launcher, request enforcement, Docker engine, container runtime, and approved runtime helpers require explicit version and source identities.
The design assumes correct behavior from these trusted components under supported requests.
It does not claim resistance to a compromised engine, compromised kernel, or hostile host administrator.
Container workloads must not receive administrative access to these components.

Whole-host suspension, power loss, kernel failure, and storage recovery remain separate obligations.
A missing observation must produce an unconfirmed result, not a successful shutdown result.

## Proposed runtime boundary

The proposed backend uses a private rootful engine within an exclusive [run domain](../Glossary.md#run-domain).
A guarded capability experiment must establish this backend's feasibility before production integration.
A private socket or private directory alone is insufficient.

```text
Trusted system manager and termination observer
  |
  +-- exclusive run-domain cgroup
        +-- driver service and native descendants
        +-- private engine service
        |     +-- dockerd and private containerd
        |     +-- runtime shims and runc processes
        |     +-- containers and container exec processes
        +-- HTTP proxy and owned output helpers
```

The system manager must observe the actual driver as the service main process.
A persistent wrapper must not conceal driver exit.
Manager dependencies must stop the engine and other run services when that driver exits, including successful exit.
Failure of the engine or request enforcement must also stop workload admission and the driver.

`BindsTo=` can propagate unexpected unit deactivation.
`PartOf=` alone does not supply that guarantee.
Startup ordering must permit the trusted launch gate to wait without creating an ordering cycle.
A guarded test must verify the complete dependency transaction, not just individual unit properties.

Every run service must prohibit automatic restart after closure.
The manager must also exclude socket activation, timers, paths, `Upholds=`, and other activation sources for that run identity.
A service's `Restart=no` setting alone does not exclude those sources.
Container restart policies remain unchanged while the run domain is open.
The engine and every restart-capable helper must terminate during closure.

### Placement enforcement

The proposed engine uses the cgroupfs driver within a private cgroup namespace and private mount namespace.
Its cgroup filesystem must expose only its assigned delegated subtree.
The engine must not receive an alternate mount, descriptor, or manager connection that reaches outside that subtree.
A cgroup namespace alone does not prevent migration through an accessible external hierarchy.

The trusted setup must create the cgroup namespace at the assigned subtree root before it moves daemon processes into a leaf.
The leaf layout must satisfy the cgroup v2 internal-process restriction when resource controllers distribute resources to children.
Runtime shims and transient creators must remain below the same root.
The host observer must verify placement from the initial namespace.
A container's relative `0::/` view is not sufficient evidence.

The request boundary must reject conflicting `CgroupParent` settings before execution.
The daemon's `--cgroup-parent` value supplies a default, not authorization.
The boundary must also reject unapproved runtimes, namespace sharing, devices, capabilities, security settings, and mount propagation.
No container may obtain the host manager, host cgroup filesystem, or private runtime socket.

The engine requires more privileges than the current native diagnostic service permits.
The implementation must not remove the native service's restrictions to accommodate Docker.
The engine needs a separate reviewed privilege and namespace policy.
Capability absence or inconsistent placement must refuse work without an unmanaged fallback.

### Sockets, storage, and network

The engine and containerd need separate run-owned sockets, state directories, storage directories, temporary directories, and process identifiers.
The configuration must not inherit `/run/containerd/containerd.sock` or another shared runtime endpoint.
A containerd namespace name does not isolate its process, socket, or storage.

Docker's `data-root` does not relocate containerd image-store data on affected installations.
The launcher must verify containerd's actual root, state, snapshotter, and socket configuration.
Image loading, extraction helpers, snapshot helpers, build services, and log helpers are writers or creators too.
Any required helper outside the run domain prevents admission.
Build services remain unavailable to workload clients unless separately supported and tested.

The private engine must not alter the shared engine's bridge, firewall rules, storage, containers, images, or networks.
The proposed private network namespace must support the driver's native nodes and the engine's published ports.
The driver must retain its existing `localhost` access and node connectivity.
Each service cannot use an unrelated private network namespace and still satisfy that requirement.
Namespace setup and access require guarded verification before workload execution.

The normal workflow's external calls also require an explicit access policy.
Diagnostic network restrictions do not establish normal workflow compatibility.
Workload containers must not receive cloud instance credentials or access to shared control endpoints.
Required external operations must retain their behavior through the reviewed boundary.

Mount authorization requires opened-object identity and a reviewed source set.
String prefixes and a one-time `realpath` check do not authorize later mounts through replaceable paths.
The engine must resolve approved source files within a restricted, stable mount view.
Required writable mounts must retain their existing behavior.
Control files, runtime sockets, manager sockets, and unrelated storage remain inaccessible.

The design must account for daemon logs, container logs, native output, retained open files, and pipe holders.
External logging services cannot silently become uncontained workload writers.
Private storage paths do not establish allocation, inode, or emergency-reserve bounds.
Those bounds remain part of D3.

## Request enforcement

Workload clients receive only a mediated Unix socket through a reviewed HTTP proxy.
The backend Docker socket and containerd socket remain inaccessible to those clients and containers.
An environment variable that selects the private endpoint is not an access control.
Socket aliases, inherited descriptors, alternate client contexts, and reachable network endpoints require explicit exclusion.

The proxy must use a maintained protocol parser rather than an ad hoc parser.
It must recognize only reviewed HTTP methods, route forms, API versions, content types, and upgrade modes.
It must reject ambiguous framing, invalid paths, duplicate JSON members, conflicting field spellings, and unsupported fields before forwarding.
Validation must match the engine's decoding semantics.
A policy for one capitalization of a field must not permit another engine-recognized spelling to change its meaning.

The Docker authorization plugin is not a complete substitute for this boundary.
Docker documents gRPC paths outside plugin authorization, restricted body visibility, and authorization only at the start of upgraded streams.
The proxy must reject native gRPC, `/grpc`, HTTP/2 transitions, and unreviewed upgrades before they reach the backend.
A supplemental plugin may reject requests, but its result cannot establish complete mediation or a creation fence.

| Operation family | Required policy |
| --- | --- |
| Version, ping, information, and inspection | Route requests only to the private engine. Record only approved metadata in diagnostic evidence. |
| Container create, start, run, and Compose create/up | Validate effective configuration before forwarding. Bind resulting resources to the private run identity. |
| Container restart and automatic restart | Preserve required behavior while open. Include every restart mechanism in closure. |
| Exec create/start and attach | Bind exec identity to a contained container. Account for the complete process and connection lifetime. |
| Pause, unpause, stop, kill, wait, and remove | Operate only on resources in the exclusive private engine. Unpause can resume a writer. |
| Logs, stats, events, and archive access | Track connections and output writers. Treat exec-based collection as process creation. |
| Network and volume operations | Permit reviewed private bridge and storage operations. Reject external drivers and storage escapes. |
| Image preparation | Use a separate trusted preparation phase. Verify imported image identity and all preparation writers before release. |
| Build, plugins, swarm, runtime administration, and unknown operations | Refuse workload access until a separate compatibility and containment cycle establishes support. |

Compose and command-line wrappers must use this same request boundary.
Direct API calls must receive the same policy decisions.
A command-name wrapper cannot be the sole enforcement point.

Diagnostic evidence must contain operation names, generated request identities, timestamps, decisions, and approved resource metadata.
It must not contain private keys, complete request bodies, environment values, or full container inspection responses.
The recorder must select safe fields before it writes evidence rather than depend only on later redaction.

## Closure and accepted requests

The lifecycle distinguishes `preparing`, `open`, `closing`, `closed`, and `unconfirmed`.
A trusted run identity must bind the manager units, control objects, endpoint, and resource records.
Names, labels, caller process identifiers, and cooperative environment markers are not sufficient authorization.

The proxy must register each request before it forwards that request to the engine.
Closing must prevent new forwarding and include requests already accepted, buffered, upgraded, or awaiting runtime completion.
An HTTP response does not prove that a writer has stopped.
A completed request can leave a container, exec process, restart policy, or asynchronous helper active.

`closing` begins refusal of new work and requests the manager stop.
Requests accepted earlier can remain active during this interval.
That interval belongs to the emergency response budget and is not a completed creation fence.

The proposed [creation fence](../Glossary.md#creation-fence) requires all of the following:

- The run's request endpoints cannot forward further requests, including through existing connections.
- All accepted requests have no surviving creator or executable deferred effect outside the terminated domain.
- The engine, containerd, runtime helpers, restart mechanisms, and workload processes have terminated.
- The trusted manager cannot reactivate the closed run identity through any configured activation source.
- Independent process and cgroup observations confirm termination within the selected deadline.

Only that conjunction permits `closed`.
Unlinking a socket does not revoke an established connection.
An empty population sample does not exclude later creation.
A `cgroup.kill` operation handles current descendants but does not permanently revoke external creation authority.
Frozen processes remain alive and cannot satisfy termination.

Manager-enforced termination must survive the selected controller loss without a cooperative closure request.
If the proxy also dies, the manager must still stop all creators.
The observer must report `unconfirmed` if it cannot establish the fence, even when the stop request returns success.
A later client must not reuse an old control directory, endpoint, manager invocation, or retained request to reopen the run.

## Regression specification

The identifiers below are local design identifiers, not shared B-cycle registrations.
Every case remains pending.
A supported positive case must accompany refusal tests to exclude an implementation that never permits work.

| ID | Selected case | Required observation before fixture cleanup |
| --- | --- | --- |
| PD-01 | Supported private engine startup and workload launch | Observed manager identity, private endpoints, actual creator placement, and a progressing owned writer precede the fault. |
| PD-02 | Combined driver and crash-monitor loss | Both controller pidfds report exit. The owned Docker writer, engine, and helpers terminate. Unrelated native and Docker writers continue. |
| PD-03 | Missing capability, failed query, or incorrect placement | The launcher refuses before workload code executes. No unmanaged fallback occurs. |
| PD-04 | Conflicting placement or unsupported security request | CLI, Compose, and direct API requests receive refusal before the harmless test payload executes. |
| PD-05 | Request accepted before closure, then delayed | Closure accounts for the queued creator. No payload can execute after `closed`, including after the delay is released. |
| PD-06 | Existing connection, exec, or restart race | Closure includes retained sockets, upgraded streams, exec starts, unpause, and automatic restarts. |
| PD-07 | Successful driver exit or startup interruption | The manager closes the engine lifetime in both cases. No wrapper or preparation helper conceals the driver outcome. |
| PD-08 | Stop error, lost status, observer loss, or deadline expiry | The result remains unconfirmed. Two recovery attempts refuse work and retain one failure through the real summary path. |
| PD-09 | Unrelated resources and shared daemon preservation | Unrelated writers advance. Resource identity, content, configuration, and placement remain unchanged. |
| PD-10 | Protocol, path, body, and descriptor boundary | Unsupported gRPC and upgrade requests never reach the backend. Malformed input and unauthorized object access fail before execution. |
| PD-11 | Required workload compatibility | Restart policies, exec-based telemetry, mount semantics, network access, and finalization settings retain their required behavior. |
| PD-12 | Writer and storage inventory | Actual daemon, snapshot, log, temporary, output, and open-file writers match the declared domain and storage set. |

### First behavioral cycle

PD-02 is the first proposed fault cycle after capability characterization.
The baseline uses the frozen production driver's existing direct-launch path with a real Docker engine on the disposable runner.
The existing native-only launcher's refusal to use Docker is not this cycle's behavioral RED.
The original B44 substitute-based result remains separate from the new real-Docker result.

The fixture must use the same harmless writer, observation rules, and assertions for both launch paths.
Its only declared comparison input selects direct launch or the new contained public launcher.
The execution record must identify that difference explicitly.
The fixture must not emulate production admission, ownership validation, persistence, closure, or summary generation.

1. Verify the disposable runner's identity, exclusive-use guards, expiry timer, and absence of unrelated workloads.
2. Freeze the baseline, fixture, image identities, runtime versions, and observation settings.
3. Start fixture-owned unrelated native and Docker writers outside the proposed run domain.
4. Start the real driver with a harmless external workload that creates the owned Docker writer.
5. Confirm real admission, writer progress, controller identities, and observed cgroup placement.
6. Suspend both controllers through their pidfds.
7. Kill both suspended controllers through those same pidfds.
8. Record termination, progress, retained connections, and pending creators before any fixture cleanup.
9. Confirm behavioral RED from a surviving owned writer, not from setup failure or an outer timeout.
10. Freeze the model and configurations that correspond to the selected runtime defect.
11. Run the frozen formal control and confirm exit 12 with its exact expected invariant.
12. Apply the smallest correction for that behavior after the runtime boundary passes review.
13. Run the unchanged fixture, unchanged formal control, and corrected formal configuration.
14. Run both normal recovery attempts through the real driver and summary writer.
15. Retrieve and audit the source-bound evidence before identity-checked runner termination.

The fixture's observation intervals must remain identical between RED and GREEN.
Its outer cleanup deadline must exceed the observation period and cannot supply the passing termination event.
A missing prerequisite, archive failure, or failed image transfer is not behavioral RED.
A case that already passes unchanged production is characterization, not a new repair.
A revised fixture needs its own frozen baseline run.

The pending-request cases need deterministic evidence that a request entered the real creation path before closure.
An instrumented external runtime boundary may supply a delay, but must retain real engine and runtime execution.
Any instrumentation needs a separate disclosure and unchanged comparison fixture.
The test must not replace the production admission or closure decision with a substitute.

### Formal scope

The proposed finite model separates requests, creators, writers, endpoint state, activation sources, and termination observations.
It must allow accepted requests to create work after client completion.
It must distinguish closure initiation from the completed creation fence.
Possible properties include `ClosedPreventsExecution`, `ConfirmedRequiresTermination`, and `UnrelatedWriterPreserved`.
These names are proposals, not registered or discharged claims.

Each negative control must identify one exact expected invariant.
Actual TLC logs require private paths and retained tool identities.
Classifier and routing substitutes must not overwrite those logs.
Finite model results cannot prove parser correctness, kernel enforcement, storage durability, or a wall-clock bound.

## Implementation gates and remaining work

The following gates prevent premature implementation or integration:

1. Complete the pinned workload's request and mount inventory without recording private keys or request bodies.
2. Verify namespace setup, cgroup delegation, controller availability, and dependency ordering on the guarded disposable runner.
3. Verify the private containerd configuration and every runtime or storage helper's actual placement.
4. Review the HTTP proxy's complete protocol policy and secret-handling rules.
5. Establish the new real-Docker baseline failure before the production correction.
6. Complete the unchanged-fixture runtime and formal comparisons for each selected behavior.
7. Obtain the integration owner's registration and exact-candidate combined verification.

The capability experiment must not change the shared engine or weaken the native service's protections.
An unavailable required feature must stop the experiment with a setup result.
The result must not silently select a different cgroup driver, storage backend, or workload.

The emergency response still needs one deadline across detection, closure, stop, confirmation, and minimal evidence handling.
B48's record ordering does not establish durable publication.
Private Docker containment does not establish the D3 growth and reserve argument.
Storage faults, upload acknowledgment, metadata replacement, source-path trust, and hosted enforcement remain separate verification requirements.
No step in this document authorizes an acceptance soak, a commit, or a push.

## Reference limits

These references describe interfaces, not verified behavior of this candidate:

- [Docker daemon storage](https://docs.docker.com/engine/daemon/): `data-root` does not cover every containerd storage configuration.
- [Multiple Docker daemons](https://docs.docker.com/reference/cli/dockerd/#run-multiple-daemons): Docker describes this arrangement as experimental.
- [Docker cgroup parent](https://docs.docker.com/reference/cli/dockerd/#default-cgroup-parent): Per-container configuration can override the daemon default.
- [Docker authorization plugins](https://docs.docker.com/engine/extend/plugins_authorization/): Protocol, body visibility, and upgraded-stream limits require separate controls.
- [Embedded containerd](https://docs.docker.com/engine/daemon/embedded-containerd/): This experimental feature starts with Docker 29.7.0.
- [systemd unit dependencies](https://www.freedesktop.org/software/systemd/man/latest/systemd.unit.html#BindsTo=): Unexpected deactivation differs from explicit stop propagation.
- [systemd resource delegation](https://www.freedesktop.org/software/systemd/man/latest/systemd.resource-control.html#Delegate=): Delegation requires an explicit ownership boundary.
- [Kernel cgroup v2](https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html): Namespace visibility, migration access, population, and termination have distinct meanings.
