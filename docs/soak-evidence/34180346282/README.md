# Soak Run 34180346282 Inspection

[Run 34180346282](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/34180346282), attempt 1, failed on 2026-09-08. This record preserves the incident facts. It does not discharge a claim or establish soak acceptance.

| Obligation | Observation | Verdict |
| --- | --- | --- |
| Finalization | Iteration 42 left five deploys unfinalized after the 45-second wait. | Failed |
| Disk protection | Cleanup increased free space. No disk breach marker was found. | Not established for full duration |
| Full duration | The daily run stopped after `dev` advanced. | Incomplete |
| Report publication | Aggregation failed before dashboard assembly. | Failed |

## Tested candidate

| Identity | Value |
| --- | --- |
| Workflow and control checkout | `master`, `be2324661e474bba0cd0f44a8c06864f69f7d0fb` |
| Tested node checkout | `dev`, `0f5d2b7414786cd27b8a686ca636485c35a5edce`, version 0.4.46 |
| Integration harness pin | `0fb633729a22f600d2c2de4c8fdc3b1f57e9ecb6` |
| Exported image manifest | `sha256:3b10aa41e4adebdf2fc7efe6315f962a8a7d08e87d51c1f53d2d6861aeef0d3e` |
| Exported manifest list | `sha256:483727742825364a9a943b780db9c87ffec3f8c8f63e7a4a079108df8b4e5820` |

The workflow applied `scripts/bench/extend-issue24-metrics.sh` to the harness collector before execution. The subprocess provider used the node's `defaults.conf` with the harness CLI overrides. Neither is an immutable runtime attestation.

## Workload and outcomes

The run alternated Docker and subprocess providers with four validators, a bootstrap node, and a read-only node. Docker passed 26 of 26 iterations. The subprocess provider passed 24 of 25. The Docker integration preflight passed all 124 tests in 8,750 seconds.

Iteration 42 (subprocess, 11:35:23 to 11:44:36 UTC, 553 s) submitted 1,200 sustained deploys without submission errors. Five remained unfinalized after the wait.

| Measurement | Value |
| --- | --- |
| Sustained inclusion p95 | 25.4 s |
| Sustained finalization p50 / p95 | 26.7 s / 64.9 s |
| Tip minus finalized height before drain | 8 blocks |
| Finalized height spread after drain | 19 blocks |
| Peak node RSS | 19,549 MB |

```text
E   AssertionError: 5 deploy(s) not finalized within 45s
```

The final summary's `finalization_p95_ms` of 40.7 s is a median of iteration records, not a percentile across deploys, and cannot replace the failed phase result. The read-only block queue reached 18 pending items in iteration 42 and 19 in passing iteration 44. This inspection does not establish the dominant finalization work bound (gate F1).

## Disk evidence

The driver ran with a 4,096 MB floor, a 4,096 MB band, and a 2,048 MB hard floor. After iteration 46 at 12:25:40 UTC, cleanup raised free space from 7,964 MB to 14,371 MB. The guardian could not apply `oom_score_adj` to some workload processes at 08:04:11 UTC.

The artifact expands to 14,083,145,744 bytes across 652 files. The failure's `log-archive/` subtree holds 8,813,121,294 bytes, of which node logs are 8,116,018,608. All 21 nested node-metrics CSV files match earlier Docker iterations 1 through 41 (odd numbers) by SHA-256, so the failure archive includes accumulated earlier sessions. Archive retention is a D3 input. These byte counts do not identify every disk writer.

## Early stop and report failures

At 13:12:18 UTC segment 1 recorded `early_exit_reason=target_advanced` after iteration 51. Later segments did no work and wrote `early_exit_reason=none` into the final summary. Elapsed time was 29,277 s of a 70,448 s budget.

Report aggregation failed at 13:12:31 UTC:

```text
ci-control/scripts/bench/aggregate-perf-report.sh: line 174: /usr/bin/jq: Argument list too long
```

The script passed the 164,253-byte passive summary through `--argjson`. The dashboard then failed because `report/weekly-summary.json` was missing (gate O1).

## Evidence preservation

| Archive | SHA-256 |
| --- | --- |
| Artifact 10057623626 (851,355,201 bytes, expires 2026-09-22T13:12:31Z) | `3b25041120055e32a49810ec81f762cdbf570af9b2bdba8bf93c14569489a4cc` (matches GitHub's digest) |
| Workflow-log ZIP | `ec82240393dcfa9f28854207928f32c191374ffe593ab14fcede2ad4dc58571f` (locally computed) |
| Inspection `manifest.json` (retained outside Git) | `76434f810484fa842b0c772297b513cac034348dd9878c234c453f62b3b6a10e` |

The raw archives, the per-member digest list, the phase and iteration CSV extracts, and the derivation script stay outside Git on the inspecting host. This inspection dispatched, canceled, or restarted no soak and changed no pin, workload, or source.
