# Disk probe admission

## D2 sub-obligation

With disk protection enabled, admission requires a known sample after startup. This requirement applies before hygiene and after hygiene.

The [production regression](../../../scripts/bench/test-soak-disk-probe.sh) exercises both locations through the real driver. A missing sample must prevent admission and produce a failure result with local evidence.

This cycle covers one D2 behavior in the [prevention plan](../../../docs/plans/soak-recurrence-prevention-2026-09-08.md). It does not complete D2 or discharge `CLAIM-SOAK-001`.

## Production correspondence

| Model action | Driver behavior |
| --- | --- |
| `ProbeBoundary` | Read `disk_free_mb` at the iteration boundary after startup succeeds |
| `DecideHygiene` | Run hygiene only when the sample is known and below floor plus band |
| `Hygiene` | Complete cleanup and optional diagnostics |
| `ProbeAfterHygiene` | Obtain the post-hygiene sample |
| `DecideAfterHygiene` | Refuse a known sample below floor plus band |
| `CheckAdmission` | Refuse a missing sample before starting work |
| `Admit` | Increment the iteration count and invoke the workload |
| `PublishRefusal` | Complete local failure records and the summary |

The implementation is [run-merge-recovery-soak.sh](../../../scripts/run-merge-recovery-soak.sh). The new common check follows both probe paths and precedes signal processing and admission.

`RejectMissing` selects whether the model executes that new check. It does not assume the required invariant.

## Sample representation

Each sample has `known` and `freeMiB` fields. Valid samples have `known = TRUE`. The missing sample has `known = FALSE`.

The missing record contains a numeric placeholder for representation only. The model never uses that placeholder as a free-space measurement.

Both numeric comparisons require a known sample. `AdmissionRequiresSample` and `AdmissionRequiresBand` together require a known admission sample at or above floor plus band.

## Configurations and results

Both configurations use a 4,096 MiB floor and a 4,096 MiB band. The valid sample set is `{7000, 8192, 16384}`.

The model also permits a missing sample at either probe. Cleanup can leave the known sample unchanged.

- [MC_DiskProbeAdmission.cfg](MC_DiskProbeAdmission.cfg) enables rejection of missing samples.
- [MC_DiskProbeAdmission_missing_sample_pre_fix.cfg](MC_DiskProbeAdmission_missing_sample_pre_fix.cfg) omits that rejection, as the pre-fix driver does.

The pre-fix configuration violates `AdmissionRequiresSample` with TLC exit 12. Its counterexample admits work with `known = FALSE`.

The corrected configuration completed with 25 generated states and 22 distinct states. Coverage includes the post-hygiene probe, admission, and refusal publication.

The corrected configuration checks five invariants and `Completes`. Weak fairness supports eventual completion, not a wall-clock deadline.

## Scope and exclusions

Startup has already obtained a valid sample. The model covers one admission decision and assumes that commands and local writes complete.

The production fixtures return no data and exit 1 at the selected disk probe. They do not establish handling of every malformed value or command failure.

The model does not represent a stalled probe, a failed guardian, active writers, or emergency response timing. Those D2 behaviors remain pending.

Local publication does not prove durability, crash survival, upload, retry behavior, or inode safety. No external-write growth bound or disk reserve follows from this cycle.

The model-to-code map is not a mechanized refinement proof of Bash. Maintainer review and hosted execution remain pending.

The existing D1 model and historical evidence remain unchanged.

## Verification

Run the production regression:

```bash
bash scripts/bench/test-soak-disk-probe.sh
```

The regression uses disposable containers without host mounts, networking, or a Docker socket. External disk, Docker, and workload fixtures do not replace admission logic.

Run the bounded formal gate:

```bash
RUN_EXHAUSTIVE_TLA=0 TLA_TOOLS_JAR=/path/to/tla2tools.jar \
  bash scripts/ci/check-tla-invariants.sh --soak-pr
```

The gate requires exit 12 and the exact `AdmissionRequiresSample` violation for the new control. Tool errors and timeouts cannot satisfy that requirement.

See the [D2 cycle evidence](../../../docs/cbc-evidence/soak-d2-probe-2026-09-08/README.md) for source identities, results, and exclusions.
