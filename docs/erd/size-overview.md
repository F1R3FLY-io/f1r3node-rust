# Entity Size Overview

Sorted in descending order by **cardinality x average encoded record size**.

Snapshot: 2026-11-25T10:01:46Z to 2026-11-25T10:01:58Z. This uses the saved observations, not a new live measurement.

[Raw observations](storage/observed.json) | [Measurement details](storage/observed.md) | [Storage formulas](storage/sizing.md) | [Memory assessments](memory/sizing.md)

## Calculation

For each database instance:

```text
average_record_bytes = sampled_key_value_bytes / sampled_records
estimated_total_bytes = exact_record_count * average_record_bytes
```

Sizes include encoded keys, encoded values, and adapter prefixes. They exclude LMDB page overhead.
If every record was sampled, the total is exact. Otherwise it is an estimate from the first keys in LMDB order, not a random sample.
Empty measured tables have an exact total of zero, but no measured average record size. Missing counts or sizes are not treated as zero.

For each whole-shard entity, add the per-instance products. The displayed average is that sum divided by the combined cardinality. It is not an unweighted average of node samples.
All sort comparisons use unrounded values. Display units use powers of 1024.

**Sampling warning:** an estimated payload larger than its live page usage signals a nonrepresentative sample. Such rows are marked `exceeds pages`. The estimate is not capped or presented as exact. The ranking is therefore approximate where samples are incomplete.

Live page usage is measured separately. It includes branch, leaf, and overflow pages, but excludes free pages and environment metadata. It is not heap usage, file length, or reserved map size.

## Whole Shard, Including Replicas

Cardinality counts stored copies across nodes and queues, not unique chain entities. DB instances include empty tables. Different environments were read at different times.

The later [complete cold-leaf scan](storage/leaf-measurements.md) gives exact totals for DataLeaf, ContinuationsLeaf, and JoinsLeaf. It also measures their internal records and root reachability. Its capture time differs from this sampled overview; the totals must not be combined.

| Rank | Entity | DB instances | Cardinality | Average B/record | Cardinality x size | Live page bytes | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `rspace/cold / rspace-cold` | 5 | 65136 | 4806.59 | 298.58 MiB | 239.48 MiB | Sampled, 1280 records; exceeds pages |
| 2 | `blockstorage / blocks` | 5 | 5159 | 7737.46 | 38.07 MiB | 53.30 MiB | Sampled, 1280 records |
| 3 | `rspace/history / rspace-history` | 5 | 58610 | 605.54 | 33.85 MiB | 70.50 MiB | Sampled, 1280 records |
| 4 | `reporting / reporting-cache` | 5 | 2338 | 12169.97 | 27.14 MiB | 33.98 MiB | Sampled, 256 records |
| 5 | `dagstorage / block-metadata` | 5 | 11723 | 966.00 | 10.80 MiB | 16.24 MiB | Sampled, 1280 records |
| 6 | `dagstorage / blocks-approved` | 5 | 5 | 1560008.00 | 7.44 MiB | 7.46 MiB | Exact, all 5 records |
| 7 | `dagstorage / dag-children-v1` | 5 | 35124 | 88.00 | 2.95 MiB | 5.18 MiB | Sampled, 1280 records |
| 8 | `dagstorage / mergeable-channel-cache` | 5 | 5154 | 585.97 | 2.88 MiB | 5.61 MiB | Sampled, 1280 records |
| 9 | `dagstorage / dag-sequence-v1` | 5 | 11723 | 125.75 | 1.41 MiB | 2.57 MiB | Sampled, 1280 records |
| 10 | `dagstorage / floor-index` | 5 | 11723 | 96.00 | 1.07 MiB | 1.82 MiB | Sampled, 1280 records |
| 11 | `dagstorage / frontier-index` | 5 | 11718 | 96.00 | 1.07 MiB | 1.80 MiB | Sampled, 1280 records |
| 12 | `dagstorage / dag-membership-v1` | 5 | 11723 | 65.00 | 744.14 KiB | 1.30 MiB | Sampled, 1280 records |
| 13 | `dagstorage / dag-height-v1` | 5 | 11723 | 64.00 | 732.69 KiB | 900.00 KiB | Sampled, 1280 records |
| 14 | `rspace/history / rspace-roots` | 5 | 1190 | 51.04 | 59.31 KiB | 120.00 KiB | Exact, all 1190 records |
| 15 | `dagstorage / deploy-lifecycle-terminal` | 5 | 70 | 150.57 | 10.29 KiB | 20.00 KiB | Exact, all 70 records |
| 16 | `dagstorage / latest-messages` | 5 | 15 | 129.00 | 1.89 KiB | 20.00 KiB | Exact, all 15 records |
| 17 | `dagstorage / dag-history-state-v1` | 5 | 19 | 61.05 | 1.13 KiB | 20.00 KiB | Exact, all 19 records |
| 18 | `dagstorage / carrier-index-meta` | 5 | 20 | 43.25 | 865 B | 20.00 KiB | Exact, all 20 records |
| 19 | `dagstorage / dag-gc-retry-v1` | 5 | 15 | 56.00 | 840 B | 20.00 KiB | Exact, all 15 records |
| 20 | `eval/history / eval-roots` | 5 | 10 | 55.50 | 555 B | 20.00 KiB | Exact, all 10 records |
| 21 | `dagstorage / genesis-hash` | 5 | 5 | 71.00 | 355 B | 20.00 KiB | Exact, all 5 records |
| 22 | `deploy-queue / meta` | 3 | 3 | 10.00 | 30 B | 12.00 KiB | Exact, all 3 records |
| 23 | `dagstorage / carrier-index` | 5 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 24 | `dagstorage / carrier-index-seeds` | 5 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 25 | `dagstorage / dag-equivocations-v1` | 5 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 26 | `dagstorage / deploy-lifecycle-events` | 5 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 27 | `deploy-queue / payloads` | 3 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 28 | `deploy-queue / records` | 3 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 29 | `deploystorage / deploy_storage` | 5 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 30 | `deploystorage / deploy_storage_quarantine` | 5 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 31 | `deploystorage / deploy_storage_seeds` | 5 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 32 | `deploystorage / rejected_deploy_buffer` | 5 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 33 | `deploystorage / rejected_deploy_buffer_quarantine` | 5 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 34 | `deploystorage / rejected_deploy_buffer_seeds` | 5 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 35 | `eval/cold / eval-cold` | 5 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 36 | `eval/history / eval-history` | 5 | 0 | n/a | 0 B | 0 B | Exact, empty |

## Per Node

Each table below has its own descending ranking. These views avoid combining replica copies.

### rnode.bootstrap

| Rank | Entity | DB instances | Cardinality | Average B/record | Cardinality x size | Live page bytes | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `rspace/cold / rspace-cold` | 1 | 12439 | 2577.57 | 30.58 MiB | 43.17 MiB | Sampled, 256 records |
| 2 | `blockstorage / blocks` | 1 | 1033 | 7737.36 | 7.62 MiB | 10.67 MiB | Sampled, 256 records |
| 3 | `rspace/history / rspace-history` | 1 | 11722 | 605.54 | 6.77 MiB | 14.13 MiB | Sampled, 256 records |
| 4 | `dagstorage / block-metadata` | 1 | 2344 | 966.00 | 2.16 MiB | 3.25 MiB | Sampled, 256 records |
| 5 | `dagstorage / blocks-approved` | 1 | 1 | 1560008.00 | 1.49 MiB | 1.49 MiB | Exact, all 1 records |
| 6 | `dagstorage / dag-children-v1` | 1 | 7023 | 88.00 | 603.54 KiB | 1.03 MiB | Sampled, 256 records |
| 7 | `dagstorage / mergeable-channel-cache` | 1 | 1032 | 585.97 | 590.55 KiB | 1.12 MiB | Sampled, 256 records |
| 8 | `dagstorage / dag-sequence-v1` | 1 | 2344 | 125.75 | 287.84 KiB | 532.00 KiB | Sampled, 256 records |
| 9 | `dagstorage / floor-index` | 1 | 2344 | 96.00 | 219.75 KiB | 372.00 KiB | Sampled, 256 records |
| 10 | `dagstorage / frontier-index` | 1 | 2343 | 96.00 | 219.66 KiB | 368.00 KiB | Sampled, 256 records |
| 11 | `dagstorage / dag-membership-v1` | 1 | 2344 | 65.00 | 148.79 KiB | 268.00 KiB | Sampled, 256 records |
| 12 | `dagstorage / dag-height-v1` | 1 | 2344 | 64.00 | 146.50 KiB | 180.00 KiB | Sampled, 256 records |
| 13 | `rspace/history / rspace-roots` | 1 | 238 | 51.04 | 11.86 KiB | 24.00 KiB | Exact, all 238 records |
| 14 | `dagstorage / deploy-lifecycle-terminal` | 1 | 14 | 150.57 | 2.06 KiB | 4.00 KiB | Exact, all 14 records |
| 15 | `dagstorage / latest-messages` | 1 | 3 | 129.00 | 387 B | 4.00 KiB | Exact, all 3 records |
| 16 | `dagstorage / dag-history-state-v1` | 1 | 3 | 66.67 | 200 B | 4.00 KiB | Exact, all 3 records |
| 17 | `dagstorage / carrier-index-meta` | 1 | 4 | 43.25 | 173 B | 4.00 KiB | Exact, all 4 records |
| 18 | `dagstorage / dag-gc-retry-v1` | 1 | 3 | 56.00 | 168 B | 4.00 KiB | Exact, all 3 records |
| 19 | `eval/history / eval-roots` | 1 | 2 | 55.50 | 111 B | 4.00 KiB | Exact, all 2 records |
| 20 | `dagstorage / genesis-hash` | 1 | 1 | 71.00 | 71 B | 4.00 KiB | Exact, all 1 records |
| 21 | `dagstorage / carrier-index` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 22 | `dagstorage / carrier-index-seeds` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 23 | `dagstorage / dag-equivocations-v1` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 24 | `dagstorage / deploy-lifecycle-events` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 25 | `deploystorage / deploy_storage` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 26 | `deploystorage / deploy_storage_quarantine` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 27 | `deploystorage / deploy_storage_seeds` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 28 | `deploystorage / rejected_deploy_buffer` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 29 | `deploystorage / rejected_deploy_buffer_quarantine` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 30 | `deploystorage / rejected_deploy_buffer_seeds` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 31 | `eval/cold / eval-cold` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 32 | `eval/history / eval-history` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 33 | `reporting / reporting-cache` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |

### rnode.deploy-queue1

| Rank | Entity | DB instances | Cardinality | Average B/record | Cardinality x size | Live page bytes | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `deploy-queue / meta` | 1 | 1 | 10.00 | 10 B | 4.00 KiB | Exact, all 1 records |
| 2 | `deploy-queue / payloads` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 3 | `deploy-queue / records` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |

### rnode.deploy-queue2

| Rank | Entity | DB instances | Cardinality | Average B/record | Cardinality x size | Live page bytes | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `deploy-queue / meta` | 1 | 1 | 10.00 | 10 B | 4.00 KiB | Exact, all 1 records |
| 2 | `deploy-queue / payloads` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 3 | `deploy-queue / records` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |

### rnode.deploy-queue3

| Rank | Entity | DB instances | Cardinality | Average B/record | Cardinality x size | Live page bytes | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `deploy-queue / meta` | 1 | 1 | 10.00 | 10 B | 4.00 KiB | Exact, all 1 records |
| 2 | `deploy-queue / payloads` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 3 | `deploy-queue / records` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |

### rnode.readonly

| Rank | Entity | DB instances | Cardinality | Average B/record | Cardinality x size | Live page bytes | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `rspace/cold / rspace-cold` | 1 | 13178 | 5296.46 | 66.56 MiB | 49.15 MiB | Sampled, 256 records; exceeds pages |
| 2 | `reporting / reporting-cache` | 1 | 2338 | 12169.97 | 27.14 MiB | 33.98 MiB | Sampled, 256 records |
| 3 | `blockstorage / blocks` | 1 | 1036 | 7737.36 | 7.64 MiB | 10.69 MiB | Sampled, 256 records |
| 4 | `rspace/history / rspace-history` | 1 | 11722 | 605.54 | 6.77 MiB | 14.07 MiB | Sampled, 256 records |
| 5 | `dagstorage / block-metadata` | 1 | 2347 | 966.00 | 2.16 MiB | 3.25 MiB | Sampled, 256 records |
| 6 | `dagstorage / blocks-approved` | 1 | 1 | 1560008.00 | 1.49 MiB | 1.49 MiB | Exact, all 1 records |
| 7 | `dagstorage / dag-children-v1` | 1 | 7032 | 88.00 | 604.31 KiB | 1.04 MiB | Sampled, 256 records |
| 8 | `dagstorage / mergeable-channel-cache` | 1 | 1035 | 585.97 | 592.26 KiB | 1.14 MiB | Sampled, 256 records |
| 9 | `dagstorage / dag-sequence-v1` | 1 | 2347 | 125.75 | 288.21 KiB | 524.00 KiB | Sampled, 256 records |
| 10 | `dagstorage / floor-index` | 1 | 2347 | 96.00 | 220.03 KiB | 372.00 KiB | Sampled, 256 records |
| 11 | `dagstorage / frontier-index` | 1 | 2346 | 96.00 | 219.94 KiB | 368.00 KiB | Sampled, 256 records |
| 12 | `dagstorage / dag-membership-v1` | 1 | 2347 | 65.00 | 148.98 KiB | 260.00 KiB | Sampled, 256 records |
| 13 | `dagstorage / dag-height-v1` | 1 | 2347 | 64.00 | 146.69 KiB | 180.00 KiB | Sampled, 256 records |
| 14 | `rspace/history / rspace-roots` | 1 | 238 | 51.04 | 11.86 KiB | 24.00 KiB | Exact, all 238 records |
| 15 | `dagstorage / deploy-lifecycle-terminal` | 1 | 14 | 150.57 | 2.06 KiB | 4.00 KiB | Exact, all 14 records |
| 16 | `dagstorage / latest-messages` | 1 | 3 | 129.00 | 387 B | 4.00 KiB | Exact, all 3 records |
| 17 | `dagstorage / dag-history-state-v1` | 1 | 4 | 60.00 | 240 B | 4.00 KiB | Exact, all 4 records |
| 18 | `dagstorage / carrier-index-meta` | 1 | 4 | 43.25 | 173 B | 4.00 KiB | Exact, all 4 records |
| 19 | `dagstorage / dag-gc-retry-v1` | 1 | 3 | 56.00 | 168 B | 4.00 KiB | Exact, all 3 records |
| 20 | `eval/history / eval-roots` | 1 | 2 | 55.50 | 111 B | 4.00 KiB | Exact, all 2 records |
| 21 | `dagstorage / genesis-hash` | 1 | 1 | 71.00 | 71 B | 4.00 KiB | Exact, all 1 records |
| 22 | `dagstorage / carrier-index` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 23 | `dagstorage / carrier-index-seeds` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 24 | `dagstorage / dag-equivocations-v1` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 25 | `dagstorage / deploy-lifecycle-events` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 26 | `deploystorage / deploy_storage` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 27 | `deploystorage / deploy_storage_quarantine` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 28 | `deploystorage / deploy_storage_seeds` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 29 | `deploystorage / rejected_deploy_buffer` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 30 | `deploystorage / rejected_deploy_buffer_quarantine` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 31 | `deploystorage / rejected_deploy_buffer_seeds` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 32 | `eval/cold / eval-cold` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 33 | `eval/history / eval-history` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |

### rnode.validator1

| Rank | Entity | DB instances | Cardinality | Average B/record | Cardinality x size | Live page bytes | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `rspace/cold / rspace-cold` | 1 | 13173 | 5344.84 | 67.15 MiB | 49.05 MiB | Sampled, 256 records; exceeds pages |
| 2 | `blockstorage / blocks` | 1 | 1030 | 7737.53 | 7.60 MiB | 10.64 MiB | Sampled, 256 records |
| 3 | `rspace/history / rspace-history` | 1 | 11722 | 605.54 | 6.77 MiB | 14.13 MiB | Sampled, 256 records |
| 4 | `dagstorage / block-metadata` | 1 | 2344 | 966.00 | 2.16 MiB | 3.24 MiB | Sampled, 256 records |
| 5 | `dagstorage / blocks-approved` | 1 | 1 | 1560008.00 | 1.49 MiB | 1.49 MiB | Exact, all 1 records |
| 6 | `dagstorage / dag-children-v1` | 1 | 7023 | 88.00 | 603.54 KiB | 1.04 MiB | Sampled, 256 records |
| 7 | `dagstorage / mergeable-channel-cache` | 1 | 1029 | 585.97 | 588.83 KiB | 1.11 MiB | Sampled, 256 records |
| 8 | `dagstorage / dag-sequence-v1` | 1 | 2344 | 125.75 | 287.84 KiB | 524.00 KiB | Sampled, 256 records |
| 9 | `dagstorage / floor-index` | 1 | 2344 | 96.00 | 219.75 KiB | 364.00 KiB | Sampled, 256 records |
| 10 | `dagstorage / frontier-index` | 1 | 2343 | 96.00 | 219.66 KiB | 364.00 KiB | Sampled, 256 records |
| 11 | `dagstorage / dag-membership-v1` | 1 | 2344 | 65.00 | 148.79 KiB | 272.00 KiB | Sampled, 256 records |
| 12 | `dagstorage / dag-height-v1` | 1 | 2344 | 64.00 | 146.50 KiB | 180.00 KiB | Sampled, 256 records |
| 13 | `rspace/history / rspace-roots` | 1 | 238 | 51.04 | 11.86 KiB | 24.00 KiB | Exact, all 238 records |
| 14 | `dagstorage / deploy-lifecycle-terminal` | 1 | 14 | 150.57 | 2.06 KiB | 4.00 KiB | Exact, all 14 records |
| 15 | `dagstorage / latest-messages` | 1 | 3 | 129.00 | 387 B | 4.00 KiB | Exact, all 3 records |
| 16 | `dagstorage / dag-history-state-v1` | 1 | 4 | 60.00 | 240 B | 4.00 KiB | Exact, all 4 records |
| 17 | `dagstorage / carrier-index-meta` | 1 | 4 | 43.25 | 173 B | 4.00 KiB | Exact, all 4 records |
| 18 | `dagstorage / dag-gc-retry-v1` | 1 | 3 | 56.00 | 168 B | 4.00 KiB | Exact, all 3 records |
| 19 | `eval/history / eval-roots` | 1 | 2 | 55.50 | 111 B | 4.00 KiB | Exact, all 2 records |
| 20 | `dagstorage / genesis-hash` | 1 | 1 | 71.00 | 71 B | 4.00 KiB | Exact, all 1 records |
| 21 | `dagstorage / carrier-index` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 22 | `dagstorage / carrier-index-seeds` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 23 | `dagstorage / dag-equivocations-v1` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 24 | `dagstorage / deploy-lifecycle-events` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 25 | `deploystorage / deploy_storage` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 26 | `deploystorage / deploy_storage_quarantine` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 27 | `deploystorage / deploy_storage_seeds` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 28 | `deploystorage / rejected_deploy_buffer` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 29 | `deploystorage / rejected_deploy_buffer_quarantine` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 30 | `deploystorage / rejected_deploy_buffer_seeds` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 31 | `eval/cold / eval-cold` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 32 | `eval/history / eval-history` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 33 | `reporting / reporting-cache` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |

### rnode.validator2

| Rank | Entity | DB instances | Cardinality | Average B/record | Cardinality x size | Live page bytes | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `rspace/cold / rspace-cold` | 1 | 13173 | 5344.84 | 67.15 MiB | 49.07 MiB | Sampled, 256 records; exceeds pages |
| 2 | `blockstorage / blocks` | 1 | 1030 | 7737.53 | 7.60 MiB | 10.65 MiB | Sampled, 256 records |
| 3 | `rspace/history / rspace-history` | 1 | 11722 | 605.54 | 6.77 MiB | 14.04 MiB | Sampled, 256 records |
| 4 | `dagstorage / block-metadata` | 1 | 2344 | 966.00 | 2.16 MiB | 3.25 MiB | Sampled, 256 records |
| 5 | `dagstorage / blocks-approved` | 1 | 1 | 1560008.00 | 1.49 MiB | 1.49 MiB | Exact, all 1 records |
| 6 | `dagstorage / dag-children-v1` | 1 | 7023 | 88.00 | 603.54 KiB | 1.04 MiB | Sampled, 256 records |
| 7 | `dagstorage / mergeable-channel-cache` | 1 | 1029 | 585.97 | 588.83 KiB | 1.15 MiB | Sampled, 256 records |
| 8 | `dagstorage / dag-sequence-v1` | 1 | 2344 | 125.75 | 287.84 KiB | 528.00 KiB | Sampled, 256 records |
| 9 | `dagstorage / floor-index` | 1 | 2344 | 96.00 | 219.75 KiB | 384.00 KiB | Sampled, 256 records |
| 10 | `dagstorage / frontier-index` | 1 | 2343 | 96.00 | 219.66 KiB | 376.00 KiB | Sampled, 256 records |
| 11 | `dagstorage / dag-membership-v1` | 1 | 2344 | 65.00 | 148.79 KiB | 264.00 KiB | Sampled, 256 records |
| 12 | `dagstorage / dag-height-v1` | 1 | 2344 | 64.00 | 146.50 KiB | 180.00 KiB | Sampled, 256 records |
| 13 | `rspace/history / rspace-roots` | 1 | 238 | 51.04 | 11.86 KiB | 24.00 KiB | Exact, all 238 records |
| 14 | `dagstorage / deploy-lifecycle-terminal` | 1 | 14 | 150.57 | 2.06 KiB | 4.00 KiB | Exact, all 14 records |
| 15 | `dagstorage / latest-messages` | 1 | 3 | 129.00 | 387 B | 4.00 KiB | Exact, all 3 records |
| 16 | `dagstorage / dag-history-state-v1` | 1 | 4 | 60.00 | 240 B | 4.00 KiB | Exact, all 4 records |
| 17 | `dagstorage / carrier-index-meta` | 1 | 4 | 43.25 | 173 B | 4.00 KiB | Exact, all 4 records |
| 18 | `dagstorage / dag-gc-retry-v1` | 1 | 3 | 56.00 | 168 B | 4.00 KiB | Exact, all 3 records |
| 19 | `eval/history / eval-roots` | 1 | 2 | 55.50 | 111 B | 4.00 KiB | Exact, all 2 records |
| 20 | `dagstorage / genesis-hash` | 1 | 1 | 71.00 | 71 B | 4.00 KiB | Exact, all 1 records |
| 21 | `dagstorage / carrier-index` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 22 | `dagstorage / carrier-index-seeds` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 23 | `dagstorage / dag-equivocations-v1` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 24 | `dagstorage / deploy-lifecycle-events` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 25 | `deploystorage / deploy_storage` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 26 | `deploystorage / deploy_storage_quarantine` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 27 | `deploystorage / deploy_storage_seeds` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 28 | `deploystorage / rejected_deploy_buffer` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 29 | `deploystorage / rejected_deploy_buffer_quarantine` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 30 | `deploystorage / rejected_deploy_buffer_seeds` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 31 | `eval/cold / eval-cold` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 32 | `eval/history / eval-history` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 33 | `reporting / reporting-cache` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |

### rnode.validator3

| Rank | Entity | DB instances | Cardinality | Average B/record | Cardinality x size | Live page bytes | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `rspace/cold / rspace-cold` | 1 | 13173 | 5344.84 | 67.15 MiB | 49.03 MiB | Sampled, 256 records; exceeds pages |
| 2 | `blockstorage / blocks` | 1 | 1030 | 7737.53 | 7.60 MiB | 10.65 MiB | Sampled, 256 records |
| 3 | `rspace/history / rspace-history` | 1 | 11722 | 605.54 | 6.77 MiB | 14.13 MiB | Sampled, 256 records |
| 4 | `dagstorage / block-metadata` | 1 | 2344 | 966.00 | 2.16 MiB | 3.25 MiB | Sampled, 256 records |
| 5 | `dagstorage / blocks-approved` | 1 | 1 | 1560008.00 | 1.49 MiB | 1.49 MiB | Exact, all 1 records |
| 6 | `dagstorage / dag-children-v1` | 1 | 7023 | 88.00 | 603.54 KiB | 1.04 MiB | Sampled, 256 records |
| 7 | `dagstorage / mergeable-channel-cache` | 1 | 1029 | 585.97 | 588.83 KiB | 1.09 MiB | Sampled, 256 records |
| 8 | `dagstorage / dag-sequence-v1` | 1 | 2344 | 125.75 | 287.84 KiB | 528.00 KiB | Sampled, 256 records |
| 9 | `dagstorage / floor-index` | 1 | 2344 | 96.00 | 219.75 KiB | 372.00 KiB | Sampled, 256 records |
| 10 | `dagstorage / frontier-index` | 1 | 2343 | 96.00 | 219.66 KiB | 372.00 KiB | Sampled, 256 records |
| 11 | `dagstorage / dag-membership-v1` | 1 | 2344 | 65.00 | 148.79 KiB | 268.00 KiB | Sampled, 256 records |
| 12 | `dagstorage / dag-height-v1` | 1 | 2344 | 64.00 | 146.50 KiB | 180.00 KiB | Sampled, 256 records |
| 13 | `rspace/history / rspace-roots` | 1 | 238 | 51.04 | 11.86 KiB | 24.00 KiB | Exact, all 238 records |
| 14 | `dagstorage / deploy-lifecycle-terminal` | 1 | 14 | 150.57 | 2.06 KiB | 4.00 KiB | Exact, all 14 records |
| 15 | `dagstorage / latest-messages` | 1 | 3 | 129.00 | 387 B | 4.00 KiB | Exact, all 3 records |
| 16 | `dagstorage / dag-history-state-v1` | 1 | 4 | 60.00 | 240 B | 4.00 KiB | Exact, all 4 records |
| 17 | `dagstorage / carrier-index-meta` | 1 | 4 | 43.25 | 173 B | 4.00 KiB | Exact, all 4 records |
| 18 | `dagstorage / dag-gc-retry-v1` | 1 | 3 | 56.00 | 168 B | 4.00 KiB | Exact, all 3 records |
| 19 | `eval/history / eval-roots` | 1 | 2 | 55.50 | 111 B | 4.00 KiB | Exact, all 2 records |
| 20 | `dagstorage / genesis-hash` | 1 | 1 | 71.00 | 71 B | 4.00 KiB | Exact, all 1 records |
| 21 | `dagstorage / carrier-index` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 22 | `dagstorage / carrier-index-seeds` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 23 | `dagstorage / dag-equivocations-v1` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 24 | `dagstorage / deploy-lifecycle-events` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 25 | `deploystorage / deploy_storage` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 26 | `deploystorage / deploy_storage_quarantine` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 27 | `deploystorage / deploy_storage_seeds` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 28 | `deploystorage / rejected_deploy_buffer` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 29 | `deploystorage / rejected_deploy_buffer_quarantine` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 30 | `deploystorage / rejected_deploy_buffer_seeds` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 31 | `eval/cold / eval-cold` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 32 | `eval/history / eval-history` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |
| 33 | `reporting / reporting-cache` | 1 | 0 | n/a | 0 B | 0 B | Exact, empty |

## Not Ranked

The following environments or databases were absent from the sample. Absence is not a measured zero-row table.

- `rspace/casper/v2/channels: environment absent`: rnode.bootstrap, rnode.validator1, rnode.validator2, rnode.validator3, rnode.readonly.
- `transaction: environment absent`: rnode.bootstrap, rnode.validator1, rnode.validator2, rnode.validator3, rnode.readonly.

The 1,868 in-memory declarations cannot be ranked by total retained bytes from this snapshot. Per-type live counts and owned allocation sizes are not both available.
Do not multiply resident DAG counts by encoded disk record sizes and call the result RAM usage.
Filesystem-only entities and logical contract entities also lack separate measured products. Contract values are already represented within tuple-space storage. Adding them again would double-count data.

## Regenerate

This command reads the saved snapshot and does not access Docker or the running nodes:

```bash
CARGO_TARGET_DIR=target/erd-tools/build \
  cargo run --locked --manifest-path docs/erd/Cargo.toml -- --rank-sizes
```

Add `--check` to verify the saved overview without changing it. The ERD render check also validates this overview.
