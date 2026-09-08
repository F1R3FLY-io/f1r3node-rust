# Soak Run 34180346282 Inspection

## Verdicts

[Run 34180346282](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/34180346282), attempt 1, failed. This inspection preserves incident evidence. It does not discharge a claim or establish soak acceptance.

| Obligation | Observation | Verdict |
| --- | --- | --- |
| Finalization | Iteration 42 left five deploys unfinalized after the 45-second wait. | Failed |
| Disk protection | Cleanup increased free space. No disk breach marker was found in the retained artifact. | Not established for full duration |
| Full duration | The daily run stopped after `dev` advanced. | Incomplete |
| Report publication | Aggregation failed before dashboard assembly. | Failed |

All nine prevention gates remain pending. The exact-candidate 60-hour acceptance soak remains required.

## Tested candidate

| Identity | Value |
| --- | --- |
| Workflow and control checkout | `master`, `be2324661e474bba0cd0f44a8c06864f69f7d0fb` |
| Tested node checkout | `dev`, `0f5d2b7414786cd27b8a686ca636485c35a5edce` |
| Reported node version | `0.4.46` |
| Integration harness pin | `0fb633729a22f600d2c2de4c8fdc3b1f57e9ecb6` |
| Exported image manifest | `sha256:3b10aa41e4adebdf2fc7efe6315f962a8a7d08e87d51c1f53d2d6861aeef0d3e` |
| Exported manifest list | `sha256:483727742825364a9a943b780db9c87ffec3f8c8f63e7a4a079108df8b4e5820` |
| Subprocess binary | `/tmp/rnode`, copied from `/opt/docker/bin/node` in the locally built image |

The checkout step compared the resolved node revision with the expected revision. The workflow revision is not the tested node revision.

The image used a local `latest` tag. The logs record build digests, but no separate subprocess binary checksum. These observations do not constitute immutable runtime attestation.

The workflow also changed the harness collector before execution. It ran `node-under-test/scripts/bench/extend-issue24-metrics.sh` against `metrics.py`.

The retained sources identify the harness pin and the overlay from the tested node revision. The reconstructed collector digest is not a captured runtime checksum.

The subprocess provider used `node-under-test/node/src/main/resources/defaults.conf`. The harness test added its documented CLI overrides. The retained source digests identify both inputs.

## Workload and outcomes

The run alternated Docker and subprocess providers. Docker passed 26 of 26 iterations. The subprocess provider passed 24 of 25 iterations.

The preceding Docker integration preflight passed all 124 collected tests. It took 8,750 seconds.

Each soak iteration used four validators, a bootstrap node, and a read-only node. Three validators received deploy submissions.

| Phase | Requested load |
| --- | --- |
| Low | 1 deploy/second for 30 seconds |
| Medium | 5 deploys/second for 20 seconds |
| High | 10 deploys/second for 15 seconds |
| Burst | 32 deploys |
| Sustained | 4 deploys/second for 300 seconds |

The finalization wait remained 45 seconds. The test did not enable telemetry-only acceptance. Active benchmark segments were disabled.

[iterations.csv](iterations.csv) preserves all 51 outcomes. [phases.csv](phases.csv) preserves all 255 distinct phase reports with source line numbers.

The parser excludes the duplicate captured-log section from phase counts. It does not change the original logs or their reported sample counts.

## Finalization failure

Iteration 42 ran from 11:35:23 to 11:44:36 UTC on September 8, 2026. It used the subprocess provider and lasted 553 seconds.

The sustained phase submitted 1,200 deploys without submission errors. Five deploys remained unfinalized after the finalization wait.

| Measurement | Reported value |
| --- | --- |
| Sustained inclusion p95 | 25.4 seconds |
| Sustained finalization p50 | 26.7 seconds |
| Sustained finalization p95 | 64.9 seconds |
| Tip minus finalized block height, before drain | 8 blocks |
| Finalized block height spread, after drain | 19 blocks |
| Peak node RSS in the iteration record | 19,549 MB |

The assertion at `iteration-00042-subprocess/pytest.log:2766` states:

```text
E   AssertionError: 5 deploy(s) not finalized within 45s
```

The earlier four phases reported zero unfinalized deploys. The finalization percentiles describe completed observations. They exclude deploys that remained unfinalized.

The test applies a finalization wait after submission. Therefore, a successful iteration can report a finalization percentile above 45 seconds.

The final summary reports 40.7 seconds for `finalization_p95_ms`. That value is a median of iteration records, not a percentile across all deploys. It cannot replace the failed phase result.

## Raw telemetry limits

The artifact retains 51 primary node-metrics CSV files. Their combined length is 5,241,345,094 bytes. The original metric labels remain unchanged.

[raw-metrics-inspection.json](raw-metrics-inspection.json) records complete CSV parses for subprocess iterations 40, 42, and 44. These parses cover 3,834,849 rows.

The inspection also checked all 153 primary CSV headers. It did not fully parse every other iteration's raw metrics.

Iteration 42 contains 1,177,470 raw metric rows and 292 metric names across six node labels. Its metric names include carrier counters and merge/replay work histograms.

The first retained metric sample occurs at elapsed second 39.0. The last sample occurs at second 540.9 or 544.0, depending on the node.

The largest observed sample gap is approximately 3.6 seconds. The selected cumulative series show no decreases. These checks do not prove uninterrupted collection or exclude process restarts.

Each iteration and node label stays separate. The collector omits metric type declarations and explicit process-start metrics. No cross-epoch rate or duration bound is derived.

Histogram sums, counts, plain counters, and queue gauges remain distinct. Missing metrics do not mean zero. Printed internal averages do not establish finalization percentiles.

The read-only block queue reached 18 pending items in iteration 42. It reached 19 in passing iteration 44. Queue peaks alone do not identify the failure mechanism.

This inspection does not establish the dominant finalization work bound. F1 must connect authenticated work dimensions, repeated work, throughput, and queue growth before F2 selects a correction.

## Disk evidence

The driver reported a disk floor of 4,096 MB and a hygiene band of 4,096 MB. Its guardian reported a hard disk floor of 2,048 MB.

At 12:25:40 UTC, after iteration 46, cleanup increased free space from 7,964 MB to 14,371 MB. The finalization failure occurred earlier.

The retained logs also contain a guardian warning at 08:04:11 UTC. The guardian could not apply `oom_score_adj` to some workload processes.

No retained disk-breach marker or executed `No space left on device` failure was identified. This is not evidence of a complete disk reserve guarantee.

[storage.json](storage.json) records exact file lengths and content matches. The artifact expands to 14,083,145,744 bytes across 652 files.

The failure's `log-archive/` subtree contains 8,813,121,294 bytes. Node log files account for 8,116,018,608 bytes of that subtree.

All 21 nested node-metrics CSV files match earlier Docker iterations by SHA-256. These are iterations 1 through 41 with odd iteration numbers.

Thus, the failure archive includes accumulated earlier sessions, not only the failed subprocess session. Archive retention requires further investigation in D3.

These byte counts measure retained file lengths. They do not establish filesystem allocation, inode usage, open-deleted files, or growth from every writer.

The earlier conclusion that failure-evidence copies could not contribute to disk growth was too strong. This run does not identify every consumer in the earlier disk incidents.

## Early stop and report failures

At 13:12:18 UTC, segment 1 recorded that `dev` advanced past the tested revision. The driver stopped after iteration 51.

`early-exit.txt` preserves that reason. Segment 1 reported `early_exit_reason=target_advanced`. Later segments did no work but wrote `early_exit_reason=none` into the final summary.

The final summary reports 29,277 elapsed seconds against a 70,448-second soak budget. The daily window was 79,200 seconds. This was not a completed 60-hour acceptance run.

Report aggregation failed at 13:12:31 UTC:

```text
ci-control/scripts/bench/aggregate-perf-report.sh: line 174: /usr/bin/jq: Argument list too long
```

The script passed the complete passive summary through `--argjson`. That argument requires 164,253 bytes, including its terminating byte.

The artifact contains an empty checkpoint summary. Dashboard assembly then failed because `report/weekly-summary.json` was missing. Neither reporting failure explains away the finalization failure.

## Evidence preservation

Artifact `10057623626` contains 851,355,201 compressed bytes. Its ZIP checksum matches the GitHub artifact digest:

```text
3b25041120055e32a49810ec81f762cdbf570af9b2bdba8bf93c14569489a4cc
```

The complete workflow-log ZIP is also retained:

```text
ec82240393dcfa9f28854207928f32c191374ffe593ab14fcede2ad4dc58571f
```

The workflow-log checksum is locally computed. GitHub did not provide an independent checksum for that download.

Verified archive copies reside outside Cargo's build directory:

```text
/home/bf_spark/soak-evidence/f1r3node-rust/34180346282/attempt-1/
```

The extraction and analysis inputs reside at:

```text
target/soak-evidence/34180346282/attempt-1/
```

The backup is local, not an off-host backup. GitHub reports artifact expiration at `2026-09-22T13:12:31Z`.

[manifest.json](manifest.json) binds the archives, metadata, inspected sources, and derived files. [archive-members-sha256.json](archive-members-sha256.json) binds every extracted artifact file.

[observations.json](observations.json) retains selected source lines. The complete archives retain all available iteration logs and raw telemetry.

The raw archives remain outside Git because they contain large logs and potentially sensitive fixture data. Only compact inspection records belong in this change.

To repeat the derivation, run:

```bash
ruby docs/soak-evidence/34180346282/inspect.rb \
  target/soak-evidence/34180346282/attempt-1 \
  /tmp/soak-34180346282-review
```

## Remaining work

D1 still requires isolated historical disk-admission RED/GREEN evidence. D2 and D3 must establish emergency-response bounds and identify all disk writers.

O1 must address retrieval and reporting failures. F1 must establish the dominant finalization work bound. No repair was selected during this inspection.

The repin prerequisite remains the merge of system-integration PR stack #139 → #138. The repin must use the resulting merged revision and repeat cross-repository checks.

This inspection did not dispatch, cancel, or restart a soak. It did not change the workload, finalization limit, harness pin, production source, or formal model.

The subsequent [merged harness repin](../repin-962effd-2026-09-08/README.md) has a separate record. No commit or push was made.
