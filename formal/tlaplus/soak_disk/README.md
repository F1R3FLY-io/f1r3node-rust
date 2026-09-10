# Soak disk admission

## D1 obligation

With disk protection enabled, the driver must refuse a new iteration when cleanup leaves its valid free-space sample below floor plus band.

This model supports D1 in the [prevention plan](../../../docs/plans/soak-recurrence-prevention-2026-09-08.md). It covers one admission decision, not the full resource claim.

`AdmissionRequiresBand` requires every admission sample to reach the floor-plus-band threshold. `RequireBand` selects the actual comparison expression, not an assumed invariant.

## Production correspondence

| Model action | Production behavior |
| --- | --- |
| `CheckGuardian` | Read the guardian marker before admission and after cleanup |
| `ProbeBoundary` | Read `disk_free_mb` before the hygiene decision |
| `Hygiene` | Run `reclaim_disk_space`, including zero reclamation |
| `ProbeAfterHygiene` | Read `disk_free_mb` after the guardian check |
| `Decide` | Compare the new sample against the admission threshold |
| `Admit` | Increment the iteration count and invoke the external workload command |
| `GuardianTrip` | Make a guardian marker available for a later check |
| `PublishRefusal` | Complete local refusal records and the failure summary |

The implementation is [run-merge-recovery-soak.sh](../../../scripts/run-merge-recovery-soak.sh). The mapping is a reviewed abstraction, not a mechanized refinement proof of Bash.

## Configurations

Both configurations start at 7,000 MiB. The floor is 4,096 MiB, and the band is 4,096 MiB.

The finite free-space set is `{7000, 8191, 8192, 8193}`. Cleanup can retain the same value or select a larger value. Cleanup does not always succeed.

- [MC_SoakDisk.cfg](MC_SoakDisk.cfg) requires the floor-plus-band threshold.
- [MC_SoakDisk_floor_only_pre_fix.cfg](MC_SoakDisk_floor_only_pre_fix.cfg) uses the historical floor-only comparison.

The historical counterexample retains 7,000 MiB through cleanup and admits work. The corrected configuration refuses that admission but permits admission after sufficient reclamation.

The corrected model checks `TypeOK`, `AdmissionRequiresBand`, `StopPreventsAdmission`, and `RefusalRecorded`. Weak fairness supports eventual admission or completed local refusal through `Completes`.

## Bounds and exclusions

The model assumes valid samples, no external writes during this decision, and completing commands. These assumptions do not establish production timing or resource bounds.

The guardian transition does not model polling intervals, failed kills, or writer termination. A marker can arrive after its last check.

Local publication does not establish durable storage, crash survival, artifact upload, or retry behavior. D2 must address these obligations separately.

The model ends at admission or completed refusal. It does not model later disk growth, inode exhaustion, finalization, or full-duration acceptance.

The proposed `CLAIM-SOAK-001` remains unratified and pending. No mandatory CbC attributes were added.

## Real-system correspondence

Current driver hygiene performs read-only Docker inspection. A larger later sample in the admission models can represent external space recovery, not proven reclamation.

The additional correspondence documents cover selected real-system behaviors:

- [Docker cleanup ownership](DockerCleanupOwnership.md) covers preservation of three fixture resources.
- [Docker exit stop](DockerExitStop.md) covers one writer after active-iteration `SIGTERM`.
- [Iteration crash recovery](IterationCrashRecovery.md) covers one process crash and two restarts.

These fixtures require a disposable diagnostic virtual machine. They do not run against the developer's Docker daemon.

## Regression commands

Run the production admission regression:

```bash
bash scripts/bench/test-soak-disk-admission.sh
```

The test builds a digest-pinned fixture image. It runs the driver as an unprivileged container user without host mounts, host networking, or a Docker socket.

The `df`, Docker, and workload commands are external boundary fixtures. The workload fixture writes the driver's `finalize` signal after its first invocation. This bounds historical failure inspection.

The test copies the driver and its required summary and metric helpers without changing their bytes. It records image identity, container isolation, source hashes, and results.

Do not execute the historical driver directly on the host. Its cleanup ignores the newer root overrides.

Run the registered formal configurations:

```bash
RUN_EXHAUSTIVE_TLA=0 TLA_TOOLS_JAR=/path/to/tla2tools.jar \
  bash scripts/ci/check-tla-invariants.sh --soak-pr
```

The gate requires exit 12 and the exact `AdmissionRequiresBand` violation for the negative control. Missing files, tool failures, and timeouts fail the gate.

See the [D1 evidence record](../../../docs/cbc-evidence/scripts-run-merge-recovery-soak-sh.md) for revisions, results, and limits.
