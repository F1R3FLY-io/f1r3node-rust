# D2 Disk Hygiene Deadline Evidence

## Scope and result

B23 starts from `5549561e1027438945dedabd1b7e08244883f62d`. The fixture uses 7000 MiB free space and an 8192 MiB admission threshold.

The production driver enters disk hygiene. The Docker fixture then holds the cleanup client, which ignores `TERM` until fixture release.

An independent observer checks cancellation and summary publication after five seconds, before release. Production RED leaves the client active without a summary at observation.

The exact formal RED violates `HygieneWithinBudget` with exit 12. The configurations reuse the unchanged `DiskStopDeadline` model through an explicit invariant alias.

The corrected driver times the cleanup command group and refuses admission when that group fails. Production GREEN cancels the client and publishes one protection failure before release.

The positive configuration has five distinct states. The model covers cancellation, not summary publication. The production fixture checks publication separately.

## Verification

Twenty positive configurations and twenty-one exact controls pass. The raw archive retains all 41 actual TLC logs before mock-based tests execute.

The classifier passes 147 cases. Routing passes six scenarios. The emergency suite passes twenty-five scenarios. All supporting regressions pass.

The executable inputs match a snapshot taken before the combined verification. The [manifest](manifest.jsonc) binds that snapshot, final sources, original raw digests, and complete logs.

Published logs replace only the local evidence-root prefix. The manifest retains separate digests and replacement counts. Earlier evidence packages remain unchanged.

The user committed and pushed the source as `9c99de84e`. The committed executable inputs match the tested snapshots. This session does not attest the commit hooks.

## Isolation and limits

The container has no host mounts, network, Docker socket, extra capabilities, or real nodes. UID 65534 and resource limits contain the fixture.

The fixture starts a cleanup client, not a Docker daemon operation. Cancellation does not prove that every descendant or daemon operation has stopped.

Cleanup selectors remain unchanged. Active-session ownership, image preservation, individual cleanup failures, and reclaimed-space guarantees require separate verification.

The command budget defaults to ten seconds with one second of kill grace. This component budget is not a measured complete response deadline.

Local summary publication is not durable storage or successful upload. Reserve bounds, other fault cases, hosted checks, maintainer review, D2, and acceptance remain pending.

The [model correspondence](../../../formal/tlaplus/soak_disk/DiskHygieneDeadline.md) gives the model mapping and its assumptions. The workload and 45-second finalization wait remain unchanged.
