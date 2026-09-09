# Disk sample validation

## D2 sub-obligation

Disk admission must reject an available-space field that contains a numeric prefix followed by text. B6 exercises `16384junk` after a successful startup probe.

The [production regression](../../../scripts/bench/test-soak-disk-sample.sh) executes the real driver in a disposable container. The external `df` fixture returns valid startup data, then the malformed field with exit zero.

At `59430d59b`, `disk_free_mb` applies `int($4)` before its digit check. That conversion produces `16384`, so the driver admits one iteration.

The correction validates `$4` before the existing numeric conversion. The parser rejects the malformed field. The existing B5 refusal then records failure without starting an iteration.

## Model-to-code map

| Model action | Production behavior |
| --- | --- |
| `Probe` | Read the available-space field at an admission boundary |
| `Decode` | Convert the numeric prefix, or validate the original field |
| `Decide` | Apply the known-sample and floor-plus-band admission checks |
| `Admit` | Start an iteration |
| `PublishRefusal` | Complete the local refusal records and failure summary |

The model represents raw fields with `wellFormed` and `numericPrefix`. The malformed record represents `16384junk`, not a valid space measurement.

`RejectMalformed = FALSE` models the old conversion before validation. This transition can produce a known parsed value from malformed input.

`RejectMalformed = TRUE` represents validation of the original field. It produces `Unknown` for malformed input.

`AdmissionRequiresValidSample` checks the raw input, independently of the parsed value. It does not assume that validation succeeds.

The model abstracts one admission decision after successful startup. For a known below-band sample, refusal represents completing hygiene without sufficient reclamation. The model does not reproduce individual cleanup commands.

The existing D1 and B5 models remain unchanged. Their configurations still run in the bounded gate.

## Results

Both configurations use a 4,096 MiB floor and a 4,096 MiB band. Valid samples are `{7000, 8192, 16384}`. The malformed field has numeric prefix `16384`.

The [pre-fix configuration](MC_DiskSampleValidation_numeric_prefix_pre_fix.cfg) violates `AdmissionRequiresValidSample` with TLC exit 12. Its trace parses malformed input as known space and admits work.

The [corrected configuration](MC_DiskSampleValidation.cfg) passes four invariants and `Completes`. It completes with 17 generated states, 17 distinct states, and an empty queue.

Run the production regression:

```bash
bash scripts/bench/test-soak-disk-sample.sh
```

Run the bounded formal gate:

```bash
RUN_EXHAUSTIVE_TLA=0 TLA_TOOLS_JAR=/path/to/tla2tools.jar \
  bash scripts/ci/check-tla-invariants.sh --soak-pr
```

## Limits

This cycle covers one malformed-input behavior at admission. It does not test every numeric format, integer range, probe exit status, or probe location.

The shared parser also serves the guardian. This cycle does not establish a safe guardian response to invalid data during an active iteration.

The model assumes completing probes, cleanup, and local writes. It excludes external writers, stalled commands, aggregate deadlines, and confirmed writer termination.

Local records do not prove durability, upload, restart preservation, inode safety, or the disk reserve. Weak fairness is not a time bound.

The map is a reviewed abstraction, not a mechanized Bash refinement proof. Maintainer review and hosted execution remain pending.

See the [cycle evidence](../../../docs/cbc-evidence/soak-d2-sample-2026-09-09/README.md). D2 remains open, and no claim is discharged.
