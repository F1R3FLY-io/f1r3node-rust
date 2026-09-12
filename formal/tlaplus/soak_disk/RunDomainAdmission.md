# Run-domain admission checks

B45 tests admission when containment is required but the driver cannot verify its own run domain.
The fixture runs both benchmark and iteration admission through the production driver.
It sets `SOAK_CONTAINMENT=required` and points `SOAK_RUN_DOMAIN_RECORD` at three records.
The absent record does not exist.
The mismatched record names a different cgroup.
The matching record names the fixture container's own cgroup and the driver's uid.

Both baseline cases count an admission with an absent record.
The baseline driver ignores the containment setting and selects its unmanaged path.
The correction verifies the record before either admission counter increases.

An absent or mismatched record refuses work and retains one failure across two real restarts.
The public counters remain `[0,1,0,0]` for iterations, failures, benchmark segments, and benchmark failures.
A matching record admits exactly one unit of work.
An unrelated native writer continues during each refusal.

## Run-domain record

A run-domain record is a JSON object with the fields `unit`, `cgroup`, and `uid`.
The trusted launcher writes the record before it starts the driver.
The record and its directory must belong to uid 0 and must not be writable by group or other users.
The driver rejects a symbolic link, an unreadable file, or a record without those fields.

The driver compares the `cgroup` field with its own single `/proc/self/cgroup` line and the `uid` field with its own uid.
A failed comparison writes the breach record and refuses admission.
The driver never selects the unmanaged path from a failed check.

The native launcher does not write this record yet.
That change belongs to the native launch barrier, which another session owns.

## Correspondence

| Model action | Production or fixture boundary |
| --- | --- |
| `Init` | The driver enters a benchmark or iteration admission probe with a record that matches or does not match. |
| `Admission` with `VerifyPlacement = TRUE` | The driver compares the trusted record with its kernel cgroup view and uid before it records admission. |
| `Admission` with `VerifyPlacement = FALSE` | The baseline driver admits work without a comparison. |
| `Restart` | A retained breach prevents admission across two restarts. |
| `UnverifiedPlacementPreventsAdmission` | No admission counter increases without a matching record. |
| `MatchingRecordAdmits` | A matching record is never refused. The fixture observes one admitted workload. |
| `RefusalRetainsFailure` | Each refusal records one failure. |
| `Completes` | Each case reaches admission or two refused restarts. |

Each unchecked configuration violates `UnverifiedPlacementPreventsAdmission` with TLC exit 12.
Each corrected configuration passes with six distinct states.
The `Work` constant selects the benchmark or iteration case.

## Limits

The fixture container shares one cgroup between the driver, the owned workload, and the unrelated writer.
The matching case therefore tests record comparison, not exclusive run-domain ownership.
The driver reads its cgroup from `/proc/self/cgroup`, which is the kernel view, but it does not verify that the cgroup is a systemd unit.
The fixture fabricates the records with a root-owned directory under `/run`.

No native service manager, real Docker daemon, or disposable runner executed this cycle.
The finite model does not represent time, storage, creation fencing, or Docker containment.
Construction is not applicable to this Bash-driver model.
D2 and claim discharge remain pending.
