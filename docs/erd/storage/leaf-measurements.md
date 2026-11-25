# Cold Leaf Measurements

Full scans captured from 2026-11-25T10:35:59Z to 2026-11-25T10:37:45Z across 5 nodes. All chain-state scans use finalized block **1095**. No deploy was submitted and no node was restarted.

- Block hash: `27923d4bbbcbc5d0ff6bbabcab6bd583b80abeca34d19bff41e31d8603dfd722`
- Post-state root: `acc9d534226d1089915238c2e8caab00b8683e073fd2e7273c7c6432d13e6d10`
- [Raw measurements and histograms](leaves-observed.json)
- [Leaf structure diagram](leaf-internals.svg)
- [Earlier sampled overview](../size-overview.md)

## Scope and Method

These are complete scans within the recorded per-environment read transactions, not sampled estimates. The counts include physical copies across nodes and retained state versions. They are not counts of unique logical transactions or channels.

Each stored leaf is decoded as `PersistedData`, then as its source-derived datum, continuation, or join records. The decoder types are generated from the node source declarations and RhoTypes schema. Every encoded record must be consumed completely and reproduce its serialized size. Every cold leaf and traversed radix node must match its content hash.

The cold-store and history transactions are separate, not one atomic cross-environment snapshot. Every referenced leaf must exist in the captured cold-store snapshot. A missing node, missing leaf, decode error, or resource limit fails the measurement instead of producing a partial success.

Encoded record bytes include the physical key, value, and LMDB adapter prefixes. They exclude LMDB page overhead. Distributions use exact frequency counts and nearest-rank percentiles. MiB means 1,048,576 bytes. No contract payloads, channel values, or private keys are included in the report.

## Leaf Variants

Sorted by exact cardinality x mean encoded row size, including replicas.

| Entity | Leaf rows | Mean row bytes | Total encoded MiB | Nested elements | Par occurrences |
| --- | --- | --- | --- | --- | --- |
| rspace / ContinuationsLeaf | 41718 | 5416.47 | 215.496 | 44697 | 1739994 |
| rspace / DataLeaf | 5730 | 1867.00 | 10.202 | 5730 | 55083 |
| rspace / JoinsLeaf | 41688 | 218.54 | 8.688 | 41688 | 41895 |
| eval / ContinuationsLeaf | 0 | n/a | 0.000 | 0 | 0 |
| eval / DataLeaf | 0 | n/a | 0.000 | 0 | 0 |
| eval / JoinsLeaf | 0 | n/a | 0.000 | 0 | 0 |

## Internal Cardinality and Size

For DataLeaf, an element is one datum and the inner count is its top-level Par values. For ContinuationsLeaf, an element is one waiting continuation and the inner count is its BindPatterns. For JoinsLeaf, an element is one channel group and the inner count is its channels.

Each distribution column lists **min / p50 / p95 / max**. Empty populations are n/a, not a measured zero-size object.

| Entity | Elements per leaf | Element bytes | Values, patterns, or channels per element | Row bytes |
| --- | --- | --- | --- | --- |
| rspace / ContinuationsLeaf | 1 / 1 / 1 / 28 | 1057 / 3057 / 12353 / 1417039 | 1 / 1 / 1 / 1 | 1133 / 3141 / 15189 / 1417123 |
| rspace / DataLeaf | 1 / 1 / 1 / 1 | 725 / 740 / 4258 / 4258 | 1 / 1 / 1 / 1 | 801 / 824 / 4342 / 4342 |
| rspace / JoinsLeaf | 1 / 1 / 1 / 1 | 103 / 134 / 134 / 285 | 1 / 1 / 1 / 1 | 179 / 218 / 218 / 369 |
| eval / ContinuationsLeaf | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| eval / DataLeaf | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |
| eval / JoinsLeaf | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a |

## Nested Par Values

A Par occurrence is a typed Par message in a decoded record, including empty Par messages. The walk includes all generated message fields and oneof alternatives. Opaque byte arrays, random states, and replay output bytes are not interpreted as additional Par messages.

A Par size includes its entire encoded subtree. These inclusive sizes overlap for parent and child Par values, so their sum is not an additional storage total. Outermost Par bytes count non-overlapping Par subtrees within each stored record. These bytes are already included in the leaf totals. Neither measure is heap memory usage.

| Entity | Par count | Par per leaf | Inclusive Par bytes | Mean inclusive bytes | Outermost Par count | Outermost Par MiB | Max typed traversal depth |
| --- | --- | --- | --- | --- | --- | --- | --- |
| rspace / ContinuationsLeaf | 1739994 | 4 / 23 / 135 / 12805 | 81 / 98 / 2310 / 1415793 | 612.86 | 170781 | 183.579 | 121 |
| rspace / DataLeaf | 55083 | 1 / 1 / 30 / 31 | 81 / 110 / 875 / 3614 | 288.97 | 5730 | 6.232 | 26 |
| rspace / JoinsLeaf | 41895 | 1 / 1 / 1 / 2 | 95 / 126 / 126 / 277 | 126.71 | 41688 | 5.038 | 5 |
| eval / ContinuationsLeaf | 0 | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a | 0 | 0.000 | 0 |
| eval / DataLeaf | 0 | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a | 0 | 0.000 | 0 |
| eval / JoinsLeaf | 0 | n/a / n/a / n/a / n/a | n/a / n/a / n/a / n/a | n/a | 0 | 0.000 | 0 |

## Root Reachability

- `selected_root`: the preferred physical cold row used by the common finalized block root.
- `other_recorded_roots_only`: reachable from another recorded root or the current-root pointer, but not from the selected finalized root. This can include older history and newer unresolved state.
- `not_referenced_by_captured_roots`: not used by the traversed roots. This can include in-flight checkpoint writes, retained unreferenced state, or a superseded key-format copy. It is not a proof that deletion is safe.

The reader prefers raw-hash keys, then bincode-hash keys, as the node does. Shared leaves are counted once per physical row in each node, even when multiple trie paths or roots reference them.

| Category | Entity | Leaf rows | Encoded MiB | Nested elements | Par occurrences |
| --- | --- | --- | --- | --- | --- |
| not_referenced_by_captured_roots | ContinuationsLeaf | 933 | 20.505 | 2257 | 183004 |
| not_referenced_by_captured_roots | DataLeaf | 1080 | 0.925 | 1080 | 1748 |
| not_referenced_by_captured_roots | JoinsLeaf | 928 | 0.206 | 928 | 1020 |
| other_recorded_roots_only | ContinuationsLeaf | 26545 | 113.438 | 26545 | 889565 |
| other_recorded_roots_only | DataLeaf | 3300 | 8.125 | 3300 | 51120 |
| other_recorded_roots_only | JoinsLeaf | 26520 | 5.514 | 26520 | 26520 |
| selected_root | ContinuationsLeaf | 14240 | 81.553 | 15895 | 667425 |
| selected_root | DataLeaf | 1350 | 1.152 | 1350 | 2215 |
| selected_root | JoinsLeaf | 14240 | 2.969 | 14240 | 14355 |

## Common Finalized State

These decoded counts, element sizes, and Par distributions match across all scanned nodes. The table shows one copy of the common finalized root, not the sum of replicas. Physical row sizes can differ by eight bytes because raw-hash and bincode-hash keys have different lengths.

| Entity | Reachable leaves | Nested elements | Element bytes | Par occurrences | Outermost Par bytes |
| --- | --- | --- | --- | --- | --- |
| ContinuationsLeaf | 2848 | 3179 | 16862508 | 133485 | 14732560 |
| DataLeaf | 270 | 270 | 220604 | 443 | 46724 |
| JoinsLeaf | 2848 | 2848 | 384938 | 2871 | 362154 |

## Per-Node Chain State

| Node | Entity | All rows | Encoded MiB | Finalized-root rows | Other-root rows | Unreferenced rows |
| --- | --- | --- | --- | --- | --- | --- |
| rnode.bootstrap | ContinuationsLeaf | 8085 | 38.692 | 2848 | 5237 | 0 |
| rnode.bootstrap | DataLeaf | 924 | 1.842 | 270 | 654 | 0 |
| rnode.bootstrap | JoinsLeaf | 8080 | 1.683 | 2848 | 5232 | 0 |
| rnode.validator1 | ContinuationsLeaf | 8389 | 44.123 | 2848 | 5309 | 232 |
| rnode.validator1 | DataLeaf | 1200 | 2.086 | 270 | 660 | 270 |
| rnode.validator1 | JoinsLeaf | 8384 | 1.748 | 2848 | 5304 | 232 |
| rnode.validator2 | ContinuationsLeaf | 8389 | 44.123 | 2848 | 5309 | 232 |
| rnode.validator2 | DataLeaf | 1200 | 2.086 | 270 | 660 | 270 |
| rnode.validator2 | JoinsLeaf | 8384 | 1.748 | 2848 | 5304 | 232 |
| rnode.validator3 | ContinuationsLeaf | 8389 | 44.123 | 2848 | 5309 | 232 |
| rnode.validator3 | DataLeaf | 1200 | 2.086 | 270 | 660 | 270 |
| rnode.validator3 | JoinsLeaf | 8384 | 1.748 | 2848 | 5304 | 232 |
| rnode.readonly | ContinuationsLeaf | 8466 | 44.436 | 2848 | 5381 | 237 |
| rnode.readonly | DataLeaf | 1206 | 2.101 | 270 | 666 | 270 |
| rnode.readonly | JoinsLeaf | 8456 | 1.763 | 2848 | 5376 | 232 |

## Capture Evidence

| Node / store | History txn | Cold txn | Recorded roots | Selected-root leaf hashes | All-root leaf hashes | Reader seconds | Decode seconds | Live cold-page MiB |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| rnode.bootstrap / rspace | 7160 | 330 | 330 | 5966 | 17089 | 1.243 | 17.358 | 58.059 |
| rnode.bootstrap / eval | 3 | 1 | 1 | 0 | 0 | 0.000 | 0.000 | 0.000 |
| rnode.validator1 / rspace | 8393 | 334 | 333 | 5966 | 17239 | 1.255 | 19.541 | 64.465 |
| rnode.validator1 / eval | 3 | 1 | 1 | 0 | 0 | 0.000 | 0.000 | 0.000 |
| rnode.validator2 / rspace | 8412 | 334 | 333 | 5966 | 17239 | 1.333 | 19.561 | 64.488 |
| rnode.validator2 / eval | 3 | 1 | 1 | 0 | 0 | 0.000 | 0.000 | 0.000 |
| rnode.validator3 / rspace | 8448 | 334 | 333 | 5966 | 17239 | 1.275 | 19.825 | 64.445 |
| rnode.validator3 / eval | 3 | 1 | 1 | 0 | 0 | 0.000 | 0.000 | 0.000 |
| rnode.readonly / rspace | 10569 | 339 | 336 | 5966 | 17389 | 1.267 | 19.699 | 65.027 |
| rnode.readonly / eval | 3 | 1 | 1 | 0 | 0 | 0.000 | 0.000 | 0.000 |

## Safety and Limits

The probe opens LMDB data read-only and uses normal reader locking. Reader slots in lock.mdb can change. It releases the transactions before nested decoding. It does not use NO_LOCK and does not copy live LMDB files without a transaction.

Each probe runs in a temporary container with a separate 512 MiB memory-limited cgroup, one CPU, no network, dropped capabilities, and a read-only root filesystem. It shares the node PID namespace for correct LMDB PID locking, but does not signal or restart node processes. The data volume permits reader-lock updates. The host kernel cannot enforce swap limits.

Bounds per environment: 10 seconds for capture, 256 MiB of encoded rows, one million rows or traversed nodes, and 60 seconds for decoding. Each decoded record is limited to 16 MiB. Nested traversal is limited to 256 typed levels and five million message visits per leaf. A limit failure produces no successful complete report.

The stored counts describe this capture interval. They do not establish long-term storage growth, retention correctness, or consensus correctness.

## Reproduce

Requirements: Docker, jq, Rust, Cargo, and protoc with proto3 support. Set PROTOC when a non-default compiler is required.

```bash
bash docs/erd/collect-leaves.sh docs/erd/storage
CARGO_TARGET_DIR=target/erd-tools/build \
  cargo run --locked --manifest-path docs/erd/Cargo.toml --bin erd-leaf-stats -- \
  --report docs/erd/storage/leaves-observed.json docs/erd/storage/leaf-measurements.md
```

Add `--check` to the report command to verify the Markdown against the saved measurements without accessing the shard.
