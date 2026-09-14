# D2 Stop Deadline Evidence

## Status

B13 completes one local production and formal RED/GREEN cycle from `3d2aa79048c9da0af09c7771b3f5c3eb3c369414`.

D2, all claims, hosted confirmation, maintainer review, and acceptance remain pending. This package does not authorize a soak.

## Observed behavior

The fixture supplies a valid startup sample. After workload admission, the sample falls to 1,024 MiB. Both Docker stop clients ignore `TERM`.

Five seconds after the first stop request, the fixture checks the local summary and client processes. The historical driver fails that check.

The matching formal control violates `StopWithinBudget` with TLC exit 12. The first production correction returns, but leaves both clients alive.

The retained diagnostic snapshot shows the two live clients and the published summary. The corrected child shell ignores `TERM` until process-group cancellation.

The corrected production fixture passes. The positive model completes with five distinct states. The original failure and both correction attempts remain separate.

The fixture adds process and summary snapshots during investigation. Those observations do not change its deadline or verdict. The historical driver also fails the final fixture.

## Verification

The actual bounded gate passes 12 positive configurations and 12 exact negative controls. The classifier passes 84 cases. Routing passes six scenarios.

The combined emergency fixture passes eight scenarios. D1, B5, B6, the driver suite, and supporting workflow, release, repin, metrics, and summary checks pass.

The first combined verifier and classifier runs shared fixed temporary log paths. Their results cannot provide isolated verification evidence. Subsequent serial runs pass.

The manifest retains complete selected logs, source bindings, and raw digests. Historical evidence packages remain unchanged.

## Boundaries

The containers use an immutable fixture image, UID 65534, private process namespaces, dropped capabilities, resource limits, and `no-new-privileges`.

The containers have no host mounts, network, or Docker socket. No node workload runs. No developer-filesystem cleanup occurs.

The fixture replaces external commands, not production parsing, guardian decisions, timeout handling, or summary publication.

The [model correspondence](../../../formal/tlaplus/soak_disk/DiskStopDeadline.md) states the cancellation and scheduling assumptions.

Client exit does not confirm daemon-side cancellation or writer termination. Local publication does not establish durability or upload success.

Disk hygiene, other stop paths, full admission coverage, ownership, crash recovery, the composed emergency deadline, and the D3 reserve argument remain open.

## Raw evidence

The raw directory is `/home/bf_spark/soak-evidence/f1r3node-rust/d2-stop-LVbwcGyM`.

The local backup is not off-host. The manifest excludes TLC state directories and unselected temporary verifier logs from its raw digest map.

The initial fixture source is reconstructed from the retained correction snapshot. Its digest must match the digest recorded before the first production RED.

The reconstruction does not replace the original digest or execution records. The final source snapshot is separate from both intermediate correction snapshots.
