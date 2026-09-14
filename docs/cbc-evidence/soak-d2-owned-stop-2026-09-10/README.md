# D2 Docker Stop Evidence: B30–B31

## Status

B30 and B31 pass their selected local production and formal cycles.
D2 and all claim-discharge and acceptance gates remain pending.
The tests did not run a node workload or soak.
The workload and 45-second finalization wait remain unchanged.

## Matched results

| Cycle | Production RED | Production GREEN | Exact invariant | Corrected states |
| --- | --- | --- | --- | --- |
| B30 | Driver exit kills both workload and unrelated Docker writers. | Only the workload writer stops. | `UnownedWritersPreserved` | 2 |
| B31 | Driver exit loses the rejected stop and records zero failures. | Driver exit records one failure and unconfirmed termination. | `FailedStopRetained` | 2 |

Each production RED returns one, and each production GREEN returns zero.
Each exact formal control returns 12.
B30 includes Docker `run` and Compose launch cases.
B31 starts from the B30 correction.

The source baseline is `52c91cfe4f677b9b9a8b6e718a03e0c29d083d85`.
Separate driver snapshots retain the B30 and B31 corrections.
The original B30 fixture used an absolute Docker path.
The retained replacement uses `PATH` to exercise the production launch contract and repeats the baseline failure.

## Production changes

The driver creates a private Docker wrapper and a fresh owner identifier for workload commands.
The wrapper attaches the Docker owner label during container creation.
The shared stop helper selects that label, validates full identifiers, and checks each label before a destructive command.
The driver does not use fixture labels for selection.

The shared stop helper retains command failures.
After the tested failed exit stop, cleanup records one interrupted-iteration failure and explicit unconfirmed termination.
The fixture's later container removal does not count as production termination.

Model correspondence and assumptions appear in [Docker stop ownership](../../../formal/tlaplus/soak_disk/DockerStopOwnership.md) and [Docker stop failure](../../../formal/tlaplus/soak_disk/DockerStopFailure.md).

## Composed verification

The bounded gate passes 28 positive configurations and 29 exact negative controls.
The evidence retains 57 actual TLC logs before the classifier mocks execute.
The classifier covers 203 cases, and routing covers six scenarios.
All 38 emergency shim scenarios pass.

Final-source real-system verification passes B27, B28, B29, both B30 launch cases, and B31.
Supporting driver, workflow, pin, metric, and summary regressions pass.
The combined supporting command reached its tool limit during release-gate tests.
The partial run remains separate from the subsequent passing release-gate result.

## Evidence and infrastructure

The [manifest](manifest.jsonc) binds source snapshots, raw evidence, complete published streams, fixture images, and the diagnostic instance.
Each published path substitution has a count and separate original and published hashes.
Raw evidence resides outside Cargo `target/` and Git.
Source snapshots identify inputs, not execution coverage for every source file.
External commits `f3366cfab` and `463992ed1` retain the tested source.
This session does not attest those commits' hooks.

The guarded virtual machine used its real local Docker daemon and a digest-pinned fixture image.
All fixture containers had no host mounts, no network, reduced privileges, and resource limits.
Evidence retrieval completed before the observed VM termination.
The termination observation does not attribute the shutdown mechanism.

A later archive check rejected one expected FIFO from the crash fixture.
The original archive remains unchanged, and the evidence records that special entry separately.
That archive check is not behavioral RED or a passed regular-file-only extraction check.

## Remaining obligations

The tested ownership contract requires the workload to use the wrapper and a cooperative Docker environment.
The contract does not authorize operations against an actor who can forge labels or control the daemon.
Pre-existing Compose project adoption, absolute Docker commands, other API clients, and late container creation remain open.

Host-process pattern selection and memory-pressure paths still need ownership-safe corrections and matched tests.
Other shutdown paths, failed storage, benchmark recovery, complete writer termination, and durable publication remain open.
The tests do not establish a composed response deadline or an all-writer disk reserve.

Hosted execution, required-check enforcement, maintainer review, and separately authorized acceptance remain pending.
These records do not discharge any claim or authorize a soak.
