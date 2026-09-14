# D2 numeric-prefix admission evidence

```yaml
artifact: scripts/run-merge-recovery-soak.sh
base_commit: 59430d59b45e4640187fd4b0414b03249c431e8f
claim_id: CLAIM-SOAK-001
scope: B6 numeric-prefix rejection at admission
status: pending
verified_at: null
waiver: null
evidence: manifest.jsonc
```

## Result and sequence

After valid startup data, the external disk fixture returns `16384junk` with exit zero. The pre-fix driver converts the field to `16384` and admits work.

| Source | Test exit | Driver exit | Iterations | Failures |
| --- | ---: | ---: | ---: | ---: |
| `59430d59b` | 1 | 0 | 1 | 0 |
| Source-bound correction | 0 | 1 | 0 | 1 |

The production RED test completed before the formal RED run. TLC then reported `AdmissionRequiresValidSample` with exit 12 before the driver changed.

The correction changes one line in `disk_free_mb`. It validates the original field before applying `int($4)`. The parser rejects the malformed field.

The existing B5 refusal records `host_protection_breach` without starting an iteration. Production GREEN retains `protection-breach.txt`, `early-exit.txt`, and a complete, non-degraded failure summary.

The corrected model passes four invariants and `Completes`. It completes with 17 generated states, 17 distinct states, and an empty queue.

The [model-to-code map](../../../formal/tlaplus/soak_disk/DiskSampleValidation.md) explains the numeric-prefix abstraction and its limits. This map is not a mechanized implementation refinement proof.

## Production isolation

The real driver runs inside a disposable container. The container has no host mounts, networking, or Docker socket.

The container uses UID 65534, a private process namespace, dropped capabilities, and `no-new-privileges`. Its limits are 256 MiB, one CPU, and 128 processes.

The outer limit is 40 seconds with a five-second kill grace. The driver limit is 20 seconds with a two-second grace.

External disk, Docker, and workload commands are fixtures. Admission and sample validation remain production code. No actual node workload runs.

The regression requires valid startup data and the malformed sample. It requires a completed, non-degraded summary before a behavioral verdict.

Fixture errors, tool errors, timeouts, unfinished containers, and out-of-memory termination are not behavioral RED. The production and formal runs completed without tool failures.

## Retained verification

The [manifest](manifest.jsonc) binds tested inputs, baseline Git bytes, raw records, verifier identity, and complete logs. GREEN uses working-tree source hashes.

D1 and both B5 scenarios pass with the corrected parser. The existing three-scenario driver suite passes in a separate disposable container.

That container has the same isolation controls. Its limits are 512 MiB, two CPUs, and 256 processes. Its outer limit is 120 seconds with a five-second kill grace.

The bounded gate passes five positive configurations and five exact negative controls. The classifier passes 35 cases, and routing passes six scenarios.

Workflow, release, repin, collector-extension, and summary regressions pass. The new production regression and formal control are registered in CI. Hosted execution remains pending.

Historical D1, B5, G0, incident, and repin evidence remain unchanged. Their source bindings refer to their historical snapshots, not this correction.

The initial correction removed numeric conversion. Review identified a change to large-integer handling. The final correction preserves the existing conversion and adds validation before it.

The raw record preserves the initial correction and its logs separately. Production GREEN, D1, B5, the driver suite, and bounded TLC passed again with the final correction.

## Remaining obligations

This cycle covers one malformed-field behavior at admission. Other numeric formats, integer limits, probe statuses, and probe locations still require verification.

The shared parser also serves the guardian. This cycle does not establish the guardian response to invalid samples during an active iteration.

The model assumes completing commands, cleanup, and local writes. It excludes external writers, stalled diagnostics, aggregate deadlines, and confirmed writer termination.

Local records do not establish durability, upload, restart preservation, inode safety, or disk reserve. Fairness does not establish an emergency time bound.

D2, maintainer model review, mandatory-scope ratification, and full-duration acceptance remain pending. No claim is discharged.

Raw evidence remains outside Cargo output and Git. The backup is local, not off-host. No commit, push, hosted run, or soak followed this cycle.
