# Host Stop Ownership

## Behavior and correspondence

B32 preserves unrelated host writers after driver termination.
B33 preserves those writers after the memory guardian detects a hard-floor breach.
Both cases require termination of a detached workload writer.

The driver gives workload commands a fresh `SOAK_PROCESS_OWNER` value.
The pinned subprocess provider copies its parent environment when it launches nodes and preserves that environment for restart.
The stop helper selects same-user processes whose environment contains the exact owner value.
It does not select processes from their command lines.

The helper opens Linux process descriptors before it reads ownership metadata.
A process descriptor identifies a process independently of later numeric process identifier reuse.
The helper sends signals through those descriptors and checks for termination notifications within a bounded polling interval.
Failed inspection, signaling, or polling causes an unconfirmed stop result.
The existing outer command timeout remains in effect.

The memory path uses the same stop helper instead of separate global process and container selectors.
Its breach record states that termination is unconfirmed.
The record does not claim that the host is already protected.

## Model

`HostStopOwnership.tla` contains one workload writer and two unrelated writers.
The positive configuration checks both preservation and selected termination.
It reaches two distinct states.

The B32 negative control removes all three writers.
The B33 negative control removes the two node writers and preserves the unrelated client.
Both controls violate `UnownedHostWritersPreserved` with exit 12.
`UnsafeSelection` makes the two observed failure states distinct without changing the positive selection rule.

## Tests and limits

The fixtures use real Linux processes in a restricted container with a private process namespace.
They replace external workload, memory-probe, and Docker commands, not production ownership or stop decisions.
The memory test uses controlled probe results, not actual memory exhaustion.
No fixture can signal developer-host processes or access the host Docker socket.

The model assumes stable, cooperative ownership metadata and successful selected termination.
It does not model concurrent forks, hidden processes, privilege changes, cleared environments, or kernel faults.
The fixture does not force process identifier reuse or prove every race in process discovery.
The finite model is not a mechanized implementation proof or a composed response deadline.

Ownership of memory-protection metadata updates remains open.
Current real-Docker revalidation, other launch paths, failed storage, durable publication, and the D3 reserve argument remain pending.
D2, hosted checks, maintainer review, and claim discharge remain pending.

See the [B32–B33 evidence](../../../docs/cbc-evidence/soak-d2-host-stop-2026-09-11/README.md).
