# D2 missing-sample admission evidence

```yaml
artifact: scripts/run-merge-recovery-soak.sh
base_commit: 80914eedabd3413e174e9fca4fe8bd5c17021cb4
claim_id: CLAIM-SOAK-001
scope: D2 missing post-start samples at admission only
status: pending
verified_at: null
waiver: null
evidence: manifest.json
```

## Result

The pre-fix driver admitted work after a disk probe returned no sample. The corrected driver refuses that admission and records failure.

| Source and case | Test exit | Driver exit | Iterations | Failures |
| --- | ---: | ---: | ---: | ---: |
| `80914eeda`, missing boundary sample | 1 | 0 | 1 | 0 |
| `80914eeda`, missing post-hygiene sample | 1 | 0 | 1 | 0 |
| Source-bound correction, missing boundary sample | 0 | 1 | 0 | 1 |
| Source-bound correction, missing post-hygiene sample | 0 | 1 | 0 | 1 |

Both corrected cases produce `protection-breach.txt`, `early-exit.txt`, and a complete failure summary. The driver uses the existing `host_protection_breach` stop category.

An eight-line common check follows both probe paths. It refuses an empty `DISK_MB` before signal processing or iteration admission.

## Sequence and formal correspondence

The initial production regression failed at both probe locations before the driver changed. TLC then reported the exact `AdmissionRequiresSample` violation with exit 12.

The corrected driver and model passed. The final test also reproduced both failures against the retained pre-fix source snapshot.

The model checks one decision after valid startup. It represents samples with separate validity and quantity fields. A missing sample is not a zero-space measurement.

`RejectMissing = FALSE` omits the new check. The counterexample admits work with `known = FALSE`, matching the production failure.

The corrected configuration completed with 25 generated states and 22 distinct states. Its coverage includes the post-hygiene probe, admission, and refusal publication.

See the [model-to-code map](../../../formal/tlaplus/soak_disk/DiskProbeAdmission.md). This map is an abstraction, not a mechanized refinement proof of Bash.

## Fixture boundaries

The regression runs the real driver and helpers inside disposable containers. The containers have no host mounts, network access, or Docker socket.

Each container uses UID 65534, a private process namespace, dropped capabilities, and `no-new-privileges`. Its limits are 256 MiB, one CPU, and 128 processes.

The outer execution limit is 40 seconds with a five-second kill grace. The driver fixture limit is 20 seconds with a two-second grace.

The first scenario returns 16,384 MiB at startup, then no sample. The second returns 7,000 MiB until the external cleanup fixture completes, then no sample.

Each missing probe returns exit 1 without data. The external workload fixture records admission and requests finalization after its first invocation. No actual node workload runs.

Only external disk, Docker, and workload commands are fixtures. The regression does not replace the admission logic.

The regression requires a completed, non-degraded summary before its behavioral verdict. Tool failures, outer timeouts, unfinished containers, and out-of-memory termination are not behavioral RED.

## Retained verification

The [manifest](manifest.json) binds tested inputs, verifier identity, raw records, and complete text logs. GREEN identifies working-tree source hashes, not a new committed revision.

The real bounded tier passed four positive configurations and four exact negative controls. The classifier passed 28 cases, and routing passed six scenarios.

The existing D1 regression and three-scenario driver suite passed. The driver suite used a separate disposable container with 512 MiB, two CPUs, and 256 processes.

Workflow, release, repin, collector-extension, and summary regressions also passed. No Rust source changed, and no new Rust or Rocq claim evidence is asserted.

The first formal attempt returned exit 75 because TLC could not compare mixed string and numeric samples. It stopped before checking the intended behavior.

The raw record retains that model and error separately. The corrected encoding uses uniform sample records. The invalid attempt is not a formal counterexample.

## Remaining obligations

This cycle does not complete D2 or discharge `CLAIM-SOAK-001`. Hosted execution, model review, and mandatory-scope ratification remain pending.

Other malformed samples, failed guardians, stalled probes, and hanging diagnostics still require their own checks. Active writer termination and emergency deadlines remain open.

The model assumes completing commands and local writes. Fairness does not establish elapsed time. Local records do not prove durability, upload, retry behavior, or crash survival.

The cycle establishes neither all-writer growth bounds nor the reserve inequality. Finalization repair and full-duration acceptance remain separate obligations.

The raw records are outside Cargo output and Git. These local records are not an off-host backup.

The initial implementation cycle included no soak, hosted retry, protection change, commit, or push. Historical D1, B1–B3, repin, and hosted evidence remain unchanged.

## Commit preparation

A later formatting change affected only the test fixture `case` statement. Both production RED cases reproduced the failure with the formatted test. Both production GREEN cases and the D1 regression passed again.

The driver and formal inputs did not change during this revalidation. This revalidation did not repeat TLC.

The manifest preserves the initial source bindings and logs. Its `formatting_revalidation` section binds the formatted test and retains the three repeated test logs. The current claim inventory binds the candidate files.
