# Runtime Cardinality and Size

Captured from 2026-11-25T10:01:46Z to 2026-11-25T10:01:58Z. Base revision: `7a2ee6456e3cf4b11fd4147a6136a62233f9a62d`.

[Raw observations](observed.json) | [Storage formulas](sizing.md) | [Memory limits](../memory/sizing.md)

This is a read-only sample of five nodes and three queues, not a load test or heap capture. No node was restarted. Each environment has its own read transaction. Counts across environments and replicas are not one atomic shard snapshot.

The probe uses normal LMDB reader locking. It can update reader slots in lock.mdb, but opens data.mdb read-only. A temporary executable is copied into each container and removed after use.

Exact table counts and live page bytes come from LMDB statistics. Record-size samples visit at most 256 first keys per table within a two-second budget per environment. They are not random samples. Sample totals cover the whole table only when complete is true in the JSON.

## Process Resources

Docker memory accounting is not a sum of entity heap sizes. CPU is a short sample, not a sustained-load result.

| Container | Memory / limit | CPU | PIDs |
| --- | --- | --- | --- |
| rnode.bootstrap | 167.9MiB / 6GiB | 0.75% | 26 |
| rnode.validator1 | 162.1MiB / 6GiB | 0.25% | 26 |
| rnode.validator2 | 161.5MiB / 6GiB | 0.30% | 25 |
| rnode.validator3 | 155.4MiB / 6GiB | 0.26% | 24 |
| rnode.readonly | 212.9MiB / 6GiB | 0.36% | 27 |
| rnode.deploy-queue1 | 10.73MiB / 1GiB | 0.00% | 2 |
| rnode.deploy-queue2 | 10.43MiB / 1GiB | 0.00% | 2 |
| rnode.deploy-queue3 | 10.48MiB / 1GiB | 0.00% | 2 |

## Node Status

latestBlockNumber is the next block number, not the current tip.

| Node | Ready | Peers | Next block | Finalized block |
| --- | --- | --- | --- | --- |
| rnode.bootstrap | true | 4 | 783 | 780 |
| rnode.validator1 | true | 4 | 783 | 780 |
| rnode.validator2 | true | 4 | 783 | 780 |
| rnode.validator3 | true | 4 | 783 | 780 |
| rnode.readonly | true | 4 | 784 | 781 |

## Resident Metrics

### rnode.bootstrap

```text
dag_resident_child_keys 2344
parents_post_state_cache_size{source="f1r3fly.casper"} 64
dag_lfb_height 780
dag_held_blocks 2344
dag_max_height 782
dag_resident_blocks 2344
dag_resident_edges 7023
block_index_cache_size{source="f1r3fly.casper"} 8
dag_resident_cutoff 0
```

### rnode.validator1

```text
dag_max_height 782
dag_resident_blocks 2344
dag_resident_cutoff 0
block_index_cache_size{source="f1r3fly.casper"} 8
dag_held_blocks 2344
dag_resident_child_keys 2344
dag_resident_edges 7023
dag_lfb_height 780
parents_post_state_cache_size{source="f1r3fly.casper"} 64
```

### rnode.validator2

```text
block_index_cache_size{source="f1r3fly.casper"} 8
dag_resident_cutoff 0
dag_held_blocks 2344
dag_max_height 782
parents_post_state_cache_size{source="f1r3fly.casper"} 64
dag_resident_child_keys 2344
dag_lfb_height 780
dag_resident_blocks 2344
dag_resident_edges 7023
```

### rnode.validator3

```text
dag_resident_edges 7023
dag_held_blocks 2344
dag_resident_child_keys 2344
parents_post_state_cache_size{source="f1r3fly.casper"} 64
block_index_cache_size{source="f1r3fly.casper"} 9
dag_resident_blocks 2344
dag_lfb_height 780
dag_resident_cutoff 0
dag_max_height 782
```

### rnode.readonly

```text
dag_resident_blocks 2347
dag_resident_edges 7032
dag_resident_child_keys 2347
dag_lfb_height 781
dag_resident_cutoff 0
dag_held_blocks 2347
dag_max_height 783
block_index_cache_size{source="f1r3fly.casper"} 8
parents_post_state_cache_size{source="f1r3fly.casper"} 64
```

## Environment Files

All byte columns are bytes, not KiB. File allocation can differ from file length. Map size is address space, not RAM use. Absent environments have no measured table count.

| Node | Environment | File length | Allocated bytes | Map bytes | Read transaction |
| --- | --- | --- | --- | --- | --- |
| rnode.bootstrap | /var/lib/rnode/blockstorage | 13258752 | 13262848 | 1099511627776 | 3656 |
| rnode.bootstrap | /var/lib/rnode/dagstorage | 9887744 | 9891840 | 107374182400 | 12974 |
| rnode.bootstrap | /var/lib/rnode/deploystorage | 32768 | 32768 | 1073741824 | 6 |
| rnode.bootstrap | /var/lib/rnode/rspace/history | 15544320 | 15548416 | 1099511627776 | 5111 |
| rnode.bootstrap | /var/lib/rnode/rspace/cold | 45834240 | 45838336 | 1099511627776 | 237 |
| rnode.bootstrap | /var/lib/rnode/eval/history | 32768 | 32768 | 1099511627776 | 3 |
| rnode.bootstrap | /var/lib/rnode/eval/cold | 12288 | 12288 | 1099511627776 | 1 |
| rnode.bootstrap | /var/lib/rnode/reporting | 12288 | 12288 | 10995116277760 | 1 |
| rnode.bootstrap | /var/lib/rnode/transaction | environment absent | n/a | n/a | n/a |
| rnode.bootstrap | /var/lib/rnode/rspace/casper/v2/channels | environment absent | n/a | n/a | n/a |
| rnode.validator1 | /var/lib/rnode/blockstorage | 13299712 | 13303808 | 1099511627776 | 3659 |
| rnode.validator1 | /var/lib/rnode/dagstorage | 9744384 | 9748480 | 107374182400 | 12959 |
| rnode.validator1 | /var/lib/rnode/deploystorage | 32768 | 32768 | 1073741824 | 6 |
| rnode.validator1 | /var/lib/rnode/rspace/history | 15552512 | 15556608 | 1099511627776 | 5971 |
| rnode.validator1 | /var/lib/rnode/rspace/cold | 51998720 | 52002816 | 1099511627776 | 238 |
| rnode.validator1 | /var/lib/rnode/eval/history | 32768 | 32768 | 1099511627776 | 3 |
| rnode.validator1 | /var/lib/rnode/eval/cold | 12288 | 12288 | 1099511627776 | 1 |
| rnode.validator1 | /var/lib/rnode/reporting | 12288 | 12288 | 10995116277760 | 1 |
| rnode.validator1 | /var/lib/rnode/transaction | environment absent | n/a | n/a | n/a |
| rnode.validator1 | /var/lib/rnode/rspace/casper/v2/channels | environment absent | n/a | n/a | n/a |
| rnode.validator2 | /var/lib/rnode/blockstorage | 13320192 | 13324288 | 1099511627776 | 3659 |
| rnode.validator2 | /var/lib/rnode/dagstorage | 9949184 | 9953280 | 107374182400 | 12955 |
| rnode.validator2 | /var/lib/rnode/deploystorage | 32768 | 32768 | 1073741824 | 6 |
| rnode.validator2 | /var/lib/rnode/rspace/history | 15437824 | 15441920 | 1099511627776 | 5973 |
| rnode.validator2 | /var/lib/rnode/rspace/cold | 52023296 | 52027392 | 1099511627776 | 238 |
| rnode.validator2 | /var/lib/rnode/eval/history | 32768 | 32768 | 1099511627776 | 3 |
| rnode.validator2 | /var/lib/rnode/eval/cold | 12288 | 12288 | 1099511627776 | 1 |
| rnode.validator2 | /var/lib/rnode/reporting | 12288 | 12288 | 10995116277760 | 1 |
| rnode.validator2 | /var/lib/rnode/transaction | environment absent | n/a | n/a | n/a |
| rnode.validator2 | /var/lib/rnode/rspace/casper/v2/channels | environment absent | n/a | n/a | n/a |
| rnode.validator3 | /var/lib/rnode/blockstorage | 13352960 | 13357056 | 1099511627776 | 3659 |
| rnode.validator3 | /var/lib/rnode/dagstorage | 9265152 | 9269248 | 107374182400 | 12955 |
| rnode.validator3 | /var/lib/rnode/deploystorage | 32768 | 32768 | 1073741824 | 6 |
| rnode.validator3 | /var/lib/rnode/rspace/history | 15347712 | 15351808 | 1099511627776 | 5970 |
| rnode.validator3 | /var/lib/rnode/rspace/cold | 51990528 | 51994624 | 1099511627776 | 238 |
| rnode.validator3 | /var/lib/rnode/eval/history | 32768 | 32768 | 1099511627776 | 3 |
| rnode.validator3 | /var/lib/rnode/eval/cold | 12288 | 12288 | 1099511627776 | 1 |
| rnode.validator3 | /var/lib/rnode/reporting | 12288 | 12288 | 10995116277760 | 1 |
| rnode.validator3 | /var/lib/rnode/transaction | environment absent | n/a | n/a | n/a |
| rnode.validator3 | /var/lib/rnode/rspace/casper/v2/channels | environment absent | n/a | n/a | n/a |
| rnode.readonly | /var/lib/rnode/blockstorage | 13307904 | 13312000 | 1099511627776 | 3659 |
| rnode.readonly | /var/lib/rnode/dagstorage | 9555968 | 9560064 | 107374182400 | 12973 |
| rnode.readonly | /var/lib/rnode/deploystorage | 32768 | 32768 | 1073741824 | 6 |
| rnode.readonly | /var/lib/rnode/rspace/history | 15470592 | 15474688 | 1099511627776 | 7456 |
| rnode.readonly | /var/lib/rnode/rspace/cold | 52125696 | 52129792 | 1099511627776 | 240 |
| rnode.readonly | /var/lib/rnode/eval/history | 32768 | 32768 | 1099511627776 | 3 |
| rnode.readonly | /var/lib/rnode/eval/cold | 12288 | 12288 | 1099511627776 | 1 |
| rnode.readonly | /var/lib/rnode/reporting | 35680256 | 35684352 | 10995116277760 | 2339 |
| rnode.readonly | /var/lib/rnode/transaction | environment absent | n/a | n/a | n/a |
| rnode.readonly | /var/lib/rnode/rspace/casper/v2/channels | environment absent | n/a | n/a | n/a |
| rnode.deploy-queue1 | /var/lib/deploy-queue | 16384 | 16384 | 17179869184 | 1 |
| rnode.deploy-queue2 | /var/lib/deploy-queue | 16384 | 16384 | 17179869184 | 1 |
| rnode.deploy-queue3 | /var/lib/deploy-queue | 16384 | 16384 | 17179869184 | 1 |

## Database Records

Page bytes include live branch, leaf, and overflow pages. They exclude environment metadata and free pages. Min, mean, and max below describe sampled encoded key plus value bytes, including adapter prefixes. A missing sample is n/a, not zero.

| Node / environment | Database | Exact rows | Live page bytes | Sample rows | Min / mean / max record bytes |
| --- | --- | --- | --- | --- | --- |
| rnode.bootstrap / rnode/blockstorage | blocks | 1033 | 11190272 | 256 | 6071 / 7737.36328125 / 24792 |
| rnode.bootstrap / rnode/dagstorage | blocks-approved | 1 | 1564672 | 1 | 1560008 / 1560008 / 1560008 |
| rnode.bootstrap / rnode/dagstorage | block-metadata | 2344 | 3403776 | 256 | 966 / 966 / 966 |
| rnode.bootstrap / rnode/dagstorage | latest-messages | 3 | 4096 | 3 | 129 / 129 / 129 |
| rnode.bootstrap / rnode/dagstorage | dag-membership-v1 | 2344 | 274432 | 256 | 65 / 65 / 65 |
| rnode.bootstrap / rnode/dagstorage | dag-height-v1 | 2344 | 184320 | 256 | 64 / 64 / 64 |
| rnode.bootstrap / rnode/dagstorage | dag-children-v1 | 7023 | 1081344 | 256 | 88 / 88 / 88 |
| rnode.bootstrap / rnode/dagstorage | dag-sequence-v1 | 2344 | 544768 | 256 | 61 / 125.74609375 / 126 |
| rnode.bootstrap / rnode/dagstorage | dag-equivocations-v1 | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.bootstrap / rnode/dagstorage | genesis-hash | 1 | 4096 | 1 | 71 / 71 / 71 |
| rnode.bootstrap / rnode/dagstorage | dag-history-state-v1 | 3 | 4096 | 3 | 43 / 66.66666666666667 / 110 |
| rnode.bootstrap / rnode/dagstorage | dag-gc-retry-v1 | 3 | 4096 | 3 | 56 / 56 / 56 |
| rnode.bootstrap / rnode/dagstorage | floor-index | 2344 | 380928 | 256 | 96 / 96 / 96 |
| rnode.bootstrap / rnode/dagstorage | frontier-index | 2343 | 376832 | 256 | 96 / 96 / 96 |
| rnode.bootstrap / rnode/dagstorage | mergeable-channel-cache | 1032 | 1175552 | 256 | 581 / 585.96875 / 634 |
| rnode.bootstrap / rnode/dagstorage | deploy-lifecycle-events | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.bootstrap / rnode/dagstorage | deploy-lifecycle-terminal | 14 | 4096 | 14 | 150 / 150.57142857142858 / 151 |
| rnode.bootstrap / rnode/dagstorage | carrier-index | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.bootstrap / rnode/dagstorage | carrier-index-seeds | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.bootstrap / rnode/dagstorage | carrier-index-meta | 4 | 4096 | 4 | 41 / 43.25 / 46 |
| rnode.bootstrap / rnode/deploystorage | deploy_storage | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.bootstrap / rnode/deploystorage | rejected_deploy_buffer | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.bootstrap / rnode/deploystorage | deploy_storage_seeds | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.bootstrap / rnode/deploystorage | rejected_deploy_buffer_seeds | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.bootstrap / rnode/deploystorage | deploy_storage_quarantine | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.bootstrap / rnode/deploystorage | rejected_deploy_buffer_quarantine | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.bootstrap / rnode/rspace/history | rspace-history | 11722 | 14819328 | 256 | 150 / 605.53515625 / 8809 |
| rnode.bootstrap / rnode/rspace/history | rspace-roots | 238 | 24576 | 238 | 51 / 51.03781512605042 / 60 |
| rnode.bootstrap / rnode/rspace/cold | rspace-cold | 12439 | 45268992 | 256 | 218 / 2577.56640625 / 40968 |
| rnode.bootstrap / rnode/eval/history | eval-history | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.bootstrap / rnode/eval/history | eval-roots | 2 | 4096 | 2 | 51 / 55.5 / 60 |
| rnode.bootstrap / rnode/eval/cold | eval-cold | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.bootstrap / rnode/reporting | reporting-cache | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator1 / rnode/blockstorage | blocks | 1030 | 11157504 | 256 | 6071 / 7737.53125 / 24792 |
| rnode.validator1 / rnode/dagstorage | blocks-approved | 1 | 1564672 | 1 | 1560008 / 1560008 / 1560008 |
| rnode.validator1 / rnode/dagstorage | block-metadata | 2344 | 3399680 | 256 | 966 / 966 / 966 |
| rnode.validator1 / rnode/dagstorage | latest-messages | 3 | 4096 | 3 | 129 / 129 / 129 |
| rnode.validator1 / rnode/dagstorage | dag-membership-v1 | 2344 | 278528 | 256 | 65 / 65 / 65 |
| rnode.validator1 / rnode/dagstorage | dag-height-v1 | 2344 | 184320 | 256 | 64 / 64 / 64 |
| rnode.validator1 / rnode/dagstorage | dag-children-v1 | 7023 | 1089536 | 256 | 88 / 88 / 88 |
| rnode.validator1 / rnode/dagstorage | dag-sequence-v1 | 2344 | 536576 | 256 | 61 / 125.74609375 / 126 |
| rnode.validator1 / rnode/dagstorage | dag-equivocations-v1 | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator1 / rnode/dagstorage | genesis-hash | 1 | 4096 | 1 | 71 / 71 / 71 |
| rnode.validator1 / rnode/dagstorage | dag-history-state-v1 | 4 | 4096 | 4 | 40 / 60 / 110 |
| rnode.validator1 / rnode/dagstorage | dag-gc-retry-v1 | 3 | 4096 | 3 | 56 / 56 / 56 |
| rnode.validator1 / rnode/dagstorage | floor-index | 2344 | 372736 | 256 | 96 / 96 / 96 |
| rnode.validator1 / rnode/dagstorage | frontier-index | 2343 | 372736 | 256 | 96 / 96 / 96 |
| rnode.validator1 / rnode/dagstorage | mergeable-channel-cache | 1029 | 1167360 | 256 | 581 / 585.96875 / 634 |
| rnode.validator1 / rnode/dagstorage | deploy-lifecycle-events | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator1 / rnode/dagstorage | deploy-lifecycle-terminal | 14 | 4096 | 14 | 150 / 150.57142857142858 / 151 |
| rnode.validator1 / rnode/dagstorage | carrier-index | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator1 / rnode/dagstorage | carrier-index-seeds | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator1 / rnode/dagstorage | carrier-index-meta | 4 | 4096 | 4 | 41 / 43.25 / 46 |
| rnode.validator1 / rnode/deploystorage | deploy_storage | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator1 / rnode/deploystorage | rejected_deploy_buffer | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator1 / rnode/deploystorage | deploy_storage_seeds | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator1 / rnode/deploystorage | rejected_deploy_buffer_seeds | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator1 / rnode/deploystorage | deploy_storage_quarantine | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator1 / rnode/deploystorage | rejected_deploy_buffer_quarantine | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator1 / rnode/rspace/history | rspace-history | 11722 | 14815232 | 256 | 150 / 605.53515625 / 8809 |
| rnode.validator1 / rnode/rspace/history | rspace-roots | 238 | 24576 | 238 | 51 / 51.03781512605042 / 60 |
| rnode.validator1 / rnode/rspace/cold | rspace-cold | 13173 | 51437568 | 256 | 179 / 5344.8359375 / 62293 |
| rnode.validator1 / rnode/eval/history | eval-history | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator1 / rnode/eval/history | eval-roots | 2 | 4096 | 2 | 51 / 55.5 / 60 |
| rnode.validator1 / rnode/eval/cold | eval-cold | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator1 / rnode/reporting | reporting-cache | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator2 / rnode/blockstorage | blocks | 1030 | 11165696 | 256 | 6071 / 7737.53125 / 24792 |
| rnode.validator2 / rnode/dagstorage | blocks-approved | 1 | 1564672 | 1 | 1560008 / 1560008 / 1560008 |
| rnode.validator2 / rnode/dagstorage | block-metadata | 2344 | 3403776 | 256 | 966 / 966 / 966 |
| rnode.validator2 / rnode/dagstorage | latest-messages | 3 | 4096 | 3 | 129 / 129 / 129 |
| rnode.validator2 / rnode/dagstorage | dag-membership-v1 | 2344 | 270336 | 256 | 65 / 65 / 65 |
| rnode.validator2 / rnode/dagstorage | dag-height-v1 | 2344 | 184320 | 256 | 64 / 64 / 64 |
| rnode.validator2 / rnode/dagstorage | dag-children-v1 | 7023 | 1089536 | 256 | 88 / 88 / 88 |
| rnode.validator2 / rnode/dagstorage | dag-sequence-v1 | 2344 | 540672 | 256 | 61 / 125.74609375 / 126 |
| rnode.validator2 / rnode/dagstorage | dag-equivocations-v1 | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator2 / rnode/dagstorage | genesis-hash | 1 | 4096 | 1 | 71 / 71 / 71 |
| rnode.validator2 / rnode/dagstorage | dag-history-state-v1 | 4 | 4096 | 4 | 40 / 60 / 110 |
| rnode.validator2 / rnode/dagstorage | dag-gc-retry-v1 | 3 | 4096 | 3 | 56 / 56 / 56 |
| rnode.validator2 / rnode/dagstorage | floor-index | 2344 | 393216 | 256 | 96 / 96 / 96 |
| rnode.validator2 / rnode/dagstorage | frontier-index | 2343 | 385024 | 256 | 96 / 96 / 96 |
| rnode.validator2 / rnode/dagstorage | mergeable-channel-cache | 1029 | 1204224 | 256 | 581 / 585.96875 / 634 |
| rnode.validator2 / rnode/dagstorage | deploy-lifecycle-events | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator2 / rnode/dagstorage | deploy-lifecycle-terminal | 14 | 4096 | 14 | 150 / 150.57142857142858 / 151 |
| rnode.validator2 / rnode/dagstorage | carrier-index | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator2 / rnode/dagstorage | carrier-index-seeds | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator2 / rnode/dagstorage | carrier-index-meta | 4 | 4096 | 4 | 41 / 43.25 / 46 |
| rnode.validator2 / rnode/deploystorage | deploy_storage | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator2 / rnode/deploystorage | rejected_deploy_buffer | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator2 / rnode/deploystorage | deploy_storage_seeds | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator2 / rnode/deploystorage | rejected_deploy_buffer_seeds | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator2 / rnode/deploystorage | deploy_storage_quarantine | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator2 / rnode/deploystorage | rejected_deploy_buffer_quarantine | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator2 / rnode/rspace/history | rspace-history | 11722 | 14716928 | 256 | 150 / 605.53515625 / 8809 |
| rnode.validator2 / rnode/rspace/history | rspace-roots | 238 | 24576 | 238 | 51 / 51.03781512605042 / 60 |
| rnode.validator2 / rnode/rspace/cold | rspace-cold | 13173 | 51449856 | 256 | 179 / 5344.8359375 / 62293 |
| rnode.validator2 / rnode/eval/history | eval-history | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator2 / rnode/eval/history | eval-roots | 2 | 4096 | 2 | 51 / 55.5 / 60 |
| rnode.validator2 / rnode/eval/cold | eval-cold | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator2 / rnode/reporting | reporting-cache | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator3 / rnode/blockstorage | blocks | 1030 | 11169792 | 256 | 6071 / 7737.53125 / 24792 |
| rnode.validator3 / rnode/dagstorage | blocks-approved | 1 | 1564672 | 1 | 1560008 / 1560008 / 1560008 |
| rnode.validator3 / rnode/dagstorage | block-metadata | 2344 | 3407872 | 256 | 966 / 966 / 966 |
| rnode.validator3 / rnode/dagstorage | latest-messages | 3 | 4096 | 3 | 129 / 129 / 129 |
| rnode.validator3 / rnode/dagstorage | dag-membership-v1 | 2344 | 274432 | 256 | 65 / 65 / 65 |
| rnode.validator3 / rnode/dagstorage | dag-height-v1 | 2344 | 184320 | 256 | 64 / 64 / 64 |
| rnode.validator3 / rnode/dagstorage | dag-children-v1 | 7023 | 1085440 | 256 | 88 / 88 / 88 |
| rnode.validator3 / rnode/dagstorage | dag-sequence-v1 | 2344 | 540672 | 256 | 61 / 125.74609375 / 126 |
| rnode.validator3 / rnode/dagstorage | dag-equivocations-v1 | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator3 / rnode/dagstorage | genesis-hash | 1 | 4096 | 1 | 71 / 71 / 71 |
| rnode.validator3 / rnode/dagstorage | dag-history-state-v1 | 4 | 4096 | 4 | 40 / 60 / 110 |
| rnode.validator3 / rnode/dagstorage | dag-gc-retry-v1 | 3 | 4096 | 3 | 56 / 56 / 56 |
| rnode.validator3 / rnode/dagstorage | floor-index | 2344 | 380928 | 256 | 96 / 96 / 96 |
| rnode.validator3 / rnode/dagstorage | frontier-index | 2343 | 380928 | 256 | 96 / 96 / 96 |
| rnode.validator3 / rnode/dagstorage | mergeable-channel-cache | 1029 | 1142784 | 256 | 581 / 585.96875 / 634 |
| rnode.validator3 / rnode/dagstorage | deploy-lifecycle-events | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator3 / rnode/dagstorage | deploy-lifecycle-terminal | 14 | 4096 | 14 | 150 / 150.57142857142858 / 151 |
| rnode.validator3 / rnode/dagstorage | carrier-index | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator3 / rnode/dagstorage | carrier-index-seeds | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator3 / rnode/dagstorage | carrier-index-meta | 4 | 4096 | 4 | 41 / 43.25 / 46 |
| rnode.validator3 / rnode/deploystorage | deploy_storage | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator3 / rnode/deploystorage | rejected_deploy_buffer | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator3 / rnode/deploystorage | deploy_storage_seeds | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator3 / rnode/deploystorage | rejected_deploy_buffer_seeds | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator3 / rnode/deploystorage | deploy_storage_quarantine | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator3 / rnode/deploystorage | rejected_deploy_buffer_quarantine | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator3 / rnode/rspace/history | rspace-history | 11722 | 14819328 | 256 | 150 / 605.53515625 / 8809 |
| rnode.validator3 / rnode/rspace/history | rspace-roots | 238 | 24576 | 238 | 51 / 51.03781512605042 / 60 |
| rnode.validator3 / rnode/rspace/cold | rspace-cold | 13173 | 51412992 | 256 | 179 / 5344.8359375 / 62293 |
| rnode.validator3 / rnode/eval/history | eval-history | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator3 / rnode/eval/history | eval-roots | 2 | 4096 | 2 | 51 / 55.5 / 60 |
| rnode.validator3 / rnode/eval/cold | eval-cold | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.validator3 / rnode/reporting | reporting-cache | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.readonly / rnode/blockstorage | blocks | 1036 | 11206656 | 256 | 6071 / 7737.36328125 / 24792 |
| rnode.readonly / rnode/dagstorage | blocks-approved | 1 | 1564672 | 1 | 1560008 / 1560008 / 1560008 |
| rnode.readonly / rnode/dagstorage | block-metadata | 2347 | 3411968 | 256 | 966 / 966 / 966 |
| rnode.readonly / rnode/dagstorage | latest-messages | 3 | 4096 | 3 | 129 / 129 / 129 |
| rnode.readonly / rnode/dagstorage | dag-membership-v1 | 2347 | 266240 | 256 | 65 / 65 / 65 |
| rnode.readonly / rnode/dagstorage | dag-height-v1 | 2347 | 184320 | 256 | 64 / 64 / 64 |
| rnode.readonly / rnode/dagstorage | dag-children-v1 | 7032 | 1089536 | 256 | 88 / 88 / 88 |
| rnode.readonly / rnode/dagstorage | dag-sequence-v1 | 2347 | 536576 | 256 | 61 / 125.74609375 / 126 |
| rnode.readonly / rnode/dagstorage | dag-equivocations-v1 | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.readonly / rnode/dagstorage | genesis-hash | 1 | 4096 | 1 | 71 / 71 / 71 |
| rnode.readonly / rnode/dagstorage | dag-history-state-v1 | 4 | 4096 | 4 | 40 / 60 / 110 |
| rnode.readonly / rnode/dagstorage | dag-gc-retry-v1 | 3 | 4096 | 3 | 56 / 56 / 56 |
| rnode.readonly / rnode/dagstorage | floor-index | 2347 | 380928 | 256 | 96 / 96 / 96 |
| rnode.readonly / rnode/dagstorage | frontier-index | 2346 | 376832 | 256 | 96 / 96 / 96 |
| rnode.readonly / rnode/dagstorage | mergeable-channel-cache | 1035 | 1191936 | 256 | 581 / 585.96875 / 634 |
| rnode.readonly / rnode/dagstorage | deploy-lifecycle-events | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.readonly / rnode/dagstorage | deploy-lifecycle-terminal | 14 | 4096 | 14 | 150 / 150.57142857142858 / 151 |
| rnode.readonly / rnode/dagstorage | carrier-index | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.readonly / rnode/dagstorage | carrier-index-seeds | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.readonly / rnode/dagstorage | carrier-index-meta | 4 | 4096 | 4 | 41 / 43.25 / 46 |
| rnode.readonly / rnode/deploystorage | deploy_storage | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.readonly / rnode/deploystorage | rejected_deploy_buffer | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.readonly / rnode/deploystorage | deploy_storage_seeds | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.readonly / rnode/deploystorage | rejected_deploy_buffer_seeds | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.readonly / rnode/deploystorage | deploy_storage_quarantine | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.readonly / rnode/deploystorage | rejected_deploy_buffer_quarantine | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.readonly / rnode/rspace/history | rspace-history | 11722 | 14749696 | 256 | 150 / 605.53515625 / 8809 |
| rnode.readonly / rnode/rspace/history | rspace-roots | 238 | 24576 | 238 | 51 / 51.03781512605042 / 60 |
| rnode.readonly / rnode/rspace/cold | rspace-cold | 13178 | 51539968 | 256 | 179 / 5296.46484375 / 62293 |
| rnode.readonly / rnode/eval/history | eval-history | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.readonly / rnode/eval/history | eval-roots | 2 | 4096 | 2 | 51 / 55.5 / 60 |
| rnode.readonly / rnode/eval/cold | eval-cold | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.readonly / rnode/reporting | reporting-cache | 2338 | 35631104 | 256 | 9100 / 12169.96875 / 39173 |
| rnode.deploy-queue1 / deploy-queue | records | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.deploy-queue1 / deploy-queue | payloads | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.deploy-queue1 / deploy-queue | meta | 1 | 4096 | 1 | 10 / 10 / 10 |
| rnode.deploy-queue2 / deploy-queue | records | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.deploy-queue2 / deploy-queue | payloads | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.deploy-queue2 / deploy-queue | meta | 1 | 4096 | 1 | 10 / 10 / 10 |
| rnode.deploy-queue3 / deploy-queue | records | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.deploy-queue3 / deploy-queue | payloads | 0 | 0 | 0 | n/a / n/a / n/a |
| rnode.deploy-queue3 / deploy-queue | meta | 1 | 4096 | 1 | 10 / 10 / 10 |

## Reproduce

Run `bash docs/erd/collect-sizes.sh` from this checkout. It writes generated observations under target/erd-tools/observed by default. It requires the named local containers, Docker, curl in each node image, jq, Rust, and Cargo.

Review the output before replacing this recorded sample. The script does not submit deploys, start containers, stop containers, or dump database keys and values.
