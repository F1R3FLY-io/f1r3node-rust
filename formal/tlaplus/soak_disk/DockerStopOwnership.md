# Docker Stop Ownership

## Behavior

B30 stops the workload Docker writer and preserves an unrelated Docker writer after driver termination.
Both container names match the historical name selector.

The negative control violates `UnownedWritersPreserved` with exit 12.
The positive configuration also checks `OwnedWriterStopped` and reaches two distinct states.
Stopping neither writer cannot satisfy the positive configuration.

## Correspondence

The driver creates a fresh owner identifier and a private Docker command wrapper before workload launch.
Iteration and benchmark commands receive the wrapper through `PATH`.
The wrapper adds `io.f1r3fly.soak.owner` during `docker run` or `docker create`.
For Compose `up` and `create`, the wrapper adds a service-label override to the original configuration.

The stop helper selects the exact owner label and requests full container identifiers.
The stop helper validates each identifier and checks the owner label again before the destructive command.
Fixture-only labels do not control production selection.

The real-system fixture replaces the external workload, not Docker creation or the driver's selection decisions.
Its workload uses the Docker command from `PATH`, as the pinned harness does.
The unrelated writer starts outside that wrapper before the driver starts.
Both Docker `run` and Compose tests retain matched RED and GREEN observations.

## Assumptions and limits

The model assumes two existing containers, authentic creation metadata, a completing stop, and no concurrent container creation.
The fixture observes writer state and file growth after driver exit, not exact termination ordering.
The two-state model does not establish a mechanized Bash refinement or a response deadline.

The label identifies resources created through the wrapper in a cooperative runner environment.
It is not authorization against an actor who can control Docker or forge labels.
Absolute Docker paths, Docker API clients, unsupported launch forms, and pre-existing Compose project adoption need separate verification.

Host-process pattern selection, memory-pressure paths, late daemon creation, concurrent drivers, and every shutdown path remain open.
The fixture does not run nodes or establish durable publication, complete writer termination, or reserve bounds.
D2, maintainer review, hosted checks, claim discharge, and acceptance remain pending.

See the [B30–B31 evidence](../../../docs/cbc-evidence/soak-d2-owned-stop-2026-09-10/README.md).
