# D2 Real-System Evidence: B27–B29

## Status

Three matched local cycles pass. D2 and all claim-discharge and acceptance gates remain pending. No node workload or soak ran during these tests.

The tests use a dedicated virtual machine (VM) with Secure Shell (SSH) access and its real Docker daemon. Each fixture checks the instance identity, provisioning marker, and local daemon endpoint.

The source archive starts at `1e2dc07f4c19c70fb56d9b861be2a0394fcfe721`. Each correction changes the driver in a separate source copy. Helper bytes remain unchanged.

External commits `3b7904ebff5d6341faa95a0eb0dcc2e1bf2a1eec` and `f4111f6b59e6f6e4f9004d6cd224dbc871819bc3` contain the corrections. This session does not attest their hooks.

## Matched results

| Cycle | Production RED | Production GREEN | Exact invariant | Corrected states |
| --- | --- | --- | --- | --- |
| B27 | Hygiene deletes three unrelated Docker resources. | All resources remain, and admission stays refused. | `UnownedDockerResourcesPreserved` | 8 |
| B28 | The writer remains active after driver `SIGTERM`. | The daemon reports termination, and the file stops growing. | `ParentExitStopsFixtureWriter` | 4 |
| B29 | Two restarts admit work and retain zero failures. | Both restarts refuse work with one interruption failure. | `CrashRequiresRefusal` | 4 |

Each production RED returns one. Each production GREEN returns zero. Each formal negative control returns 12 with its exact invariant violation.

The B29 positive model initially had an incomplete successor because a Boolean assignment lacked parentheses. The original log remains separate from the corrected RED/GREEN pair.

B27 uses the historical baseline. B28 uses the B27 correction. B29 uses the B28 correction. All three fixtures also pass against the final corrected driver.

## Corrections and scope

B27 replaces destructive Docker hygiene with read-only inspection. The driver still refuses insufficient space. This correction sacrifices reclamation rather than assuming ownership.

B28 adds writer-stop requests to exit cleanup when an iteration or benchmark client remains active. The fixture verifies active-iteration `SIGTERM`, not every shutdown path.

B29 records an uncommitted iteration before launch. Recovery records one interruption failure and a persistent refusal state. A second restart preserves that result.

The crash test kills the driver process group, not the VM. The fixture supervisor stops the surviving Docker writer before recovery. That stop is not production evidence.

The historical cleanup fixtures retain their scenario labels. Their current command faults target read-only inspection. Simulated later space increases do not demonstrate actual reclamation.

Correspondence documents define each model's assumptions:

- [Docker cleanup ownership](../../../formal/tlaplus/soak_disk/DockerCleanupOwnership.md).
- [Docker exit stop](../../../formal/tlaplus/soak_disk/DockerExitStop.md).
- [Iteration crash recovery](../../../formal/tlaplus/soak_disk/IterationCrashRecovery.md).

## Composed verification

The final gate passes 26 positive configurations and 27 exact negative controls. It retains 53 actual TLC logs before classifier mocks execute.

The classifier passes 189 cases. Routing passes six scenarios. The existing emergency suite passes 38 scenarios, and all three real-system fixtures pass again.

Supporting driver, workflow, release, pin, metric, and summary regressions pass. These results do not establish exact-candidate hosted execution or required-check enforcement.

## Evidence and limits

The [manifest](manifest.jsonc) binds source files, raw evidence, selected complete logs, fixture images, and the diagnostic instance. Published path substitutions have separate counts and hashes.

Raw evidence resides outside Cargo `target/` and Git. Test evidence was retrieved before VM termination. Retrieval does not attest durable artifact upload or power-loss survival.

The reserved image is a small fixture image, not the node image. Stop selection still relies on names and process patterns.

Safe reclamation, ownership-safe stops, other shutdown paths, benchmark crashes, additional crash windows, storage faults, and durable publication remain open.

The complete response deadline, writer-growth bounds, reserve, hosted checks, and maintainer review remain open. The workload and 45-second finalization wait remain unchanged.
