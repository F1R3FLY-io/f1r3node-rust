# ASI amounts across the node

## User story and scope

US-015 supports exact ASI amounts on a fresh test chain. The branch starts at upstream dev commit `88f6b24aee2278c4a28ce403de6ef8ee742c34a4`.

All individual token amounts use BigInt with the unsigned 256-bit range. Intermediate products, consensus totals, and signed deltas remain arbitrary precision.

This change includes the maximum bond increase. The minimum bond stays at 1. Delegation and system-reply peek changes are outside this work.

## Specification

- Genesis allocations, bonds, fee prices, charges, and refunds use BigInt.
- Monetary values on peer protobuf messages use exactly 32 unsigned big-endian bytes.
- Public monetary fields use decimal strings. Successful monetary values are between 0 and 2^256-1.
- Failed-transfer reports retain the exact attempted value, including negative values.
- Replaced protobuf field numbers are reserved. New fields have no legacy fallback.
- Signature and block-hash encoders use the new canonical representation.
- Persisted amounts retain their full precision across storage and restart.
- Block heights, timestamps, counts, and phlo units remain bounded integers.
- Existing reward formulas, withdrawal timing, and slashing rules remain unchanged.
- Fee validation is consistent across admission, execution, and replay.
- General Rholang BigInt arithmetic retains its existing behavior.

## Acceptance

Test 0, 1, 2^63-1, 2^63, 2^128, 2^256-1, negative values, and 2^256. Verify exact encoding, hashing, storage, reporting, and replay. Exercise intermediate products above 256 bits. Compare execution and peer replay costs, events, errors, and state roots.

Run workspace checks and an independent Opus review. Keep commits focused.

## Fresh-chain interfaces

- Casper protocol version remains 1.
- `BondProto.stake` and `DeployDataProto.phloPrice` contain exactly 32 unsigned big-endian bytes. Signatures cover the new fee encoding.
- HTTP, WebSocket, and public gRPC amount fields are decimal strings. This includes stakes, total stakes, transfers, `phloPrice`, and `minPhloPrice`. Consumers must update their generated protobuf bindings and JSON handling before connecting.
- HOCON bond limits and minimum fee prices are quoted canonical decimal strings. CLI arguments, bonds files, and genesis allocation files accept canonical decimal digits. Negative values, leading zeros, and values above 2^256-1 are rejected. Genesis files retain support for # comments.
- Deploy records retain `F1R3DEP` version 1. Observer snapshots retain version 2. No metadata format marker or startup compatibility check is added.
- A deploy is rejected if its price or maximum charge exceeds 2^256-1. Refund calculations use exact arithmetic even when a reported cost exceeds the signed 64-bit range.
- Validator reward and finality calculations use arbitrary-precision intermediate values. Display-only fault-tolerance ratios are rounded after exact division, so their last floating-point bits can differ from dev.

## Validation coverage

The boundary tests cover wire codecs, signed deploys, JSON amount strings, config parsing, transfer reports, and storage reopening. The integration tests cover genesis allocation at 2^256-1 and peer replay with large validator stakes.

`uint256_fees_transfers_and_rewards_survive_peer_replay` uses a 2^230 payer balance, a 2^150 transfer, a 2^128 fee price, and 2^190 validator stakes. It checks a successful transfer and a failing deploy on two nodes, exact fees and refunds, payer and recipient balances, and committed rewards. The reward multiplication exceeds 256 bits without truncation. Peer block acceptance checks replayed events, costs, and the resulting state root.

Commands:

```bash
cargo check --workspace --all-targets
cargo test -p shared -p models -p block-storage --lib
cargo test -p node --lib
cargo test --release -p casper --test mod uint256_fees_transfers_and_rewards_survive_peer_replay
cargo test --release -p casper -- --test-threads=4
```

The release suites also cover node, block-storage, rholang, rspace_plus_plus, models, shared, comm, crypto, graphz, and casper-soak.

## Validation results: 2026-10-27

- Workspace checks and the listed crate suites passed.
- The main Casper integration target passed all 1,122 enabled cases. Five diagnostic or long-running cases remain explicitly ignored by the repository.
- The initial Cargo run timed out. Remaining cases ran in separate processes, with targeted reruns for corrected fixtures.
- Casper unit tests, the parent-state regression target, 54 observer tests, the isolated PoS contract test, and documentation tests passed.
- Process-control tests passed in a disposable container with a separate PID namespace. This validates the test suite, not a live network soak.
- Boundary regressions cover maximum genesis allocations, bonds-file comments, metadata snapshot capture, transfer reports, fee admission, refunds, and peer replay.
- Independent Opus review completed. Confirmed parser, snapshot, configuration, and test-fixture findings were corrected and tested.

The branch `feature/asi-u256-amounts` is published in [PR 881](https://github.com/F1R3FLY-io/f1r3node-rust/pull/881). Client changes and deployments remain outside this task.

## Review follow-up: 2026-10-30

The candidate incorporates dev `f48e0a6cdc18436fdaf4e2b5a54f52c5eb123a16` into PR head `ab0f7c150818d58a961036546985e3f5e1e08463`. The settled-admission fixture uses dev's citer identity with the BigInt stake representation.

Fork-choice sorting borrows scores instead of cloning each score for each comparison. Clique search borrows candidate weights and clones the largest initial weight once. These changes preserve tie-breaking, exact arithmetic, and work-meter charges. Fault and synchrony ratios retain their denominator guards. An impossible conversion failure now panics with an invariant message instead of silently supplying infinity or zero.

### API scope and overlapping work

The full fresh-chain migration remains in this PR. It includes stakes, consensus weights, fees, native transfer reporting, persistence, and maximum bond. Individual amounts remain uint256. Aggregates and intermediate products can be larger. Existing reward and slashing policies, protocol version, and storage version numbers remain unchanged.

[PR 896](https://github.com/F1R3FLY-io/f1r3node-rust/pull/896) overlaps `TransferInfo`, transaction extraction, enrichment, and event types. Its legacy int64 fields plus exact-string interface differ from this PR's replacement fields. The changes need explicit reconciliation before both can merge. This PR does not incorporate that compatibility interface. [Issue 806](https://github.com/F1R3FLY-io/f1r3node-rust/issues/806) provides the transfer-reporting context, but this fresh-chain interface does not satisfy its legacy-client compatibility contract.

The exact fee arithmetic also relates to [issue 894](https://github.com/F1R3FLY-io/f1r3node-rust/issues/894). This work makes no historical replay compatibility claim.

### Integration prerequisites

Following the dependency sequence used for [PR 497](https://github.com/F1R3FLY-io/f1r3node-rust/pull/497), compatible client bindings and the integration harness must land before repinning and running heavy CI. Those repositories are outside this node-only follow-up.

The node still pins system-integration `e1e00ee503ea3071c03abb84245cc0d48e441e82`, which selects pyf1r3fly `34ff0419456f`. That client sends the old int64 `phloPrice` field 7, whereas this node decodes a 32-byte value at field 15. Its bonding template also sends an Int literal instead of the BigInt required by PoS. A wide-amount client release is insufficient unless its generated messages and signing bytes match this node schema.

After compatible client and harness changes merge, review the immutable harness revision and run `just repin-system-integration <full-sha>` so all node pin sites move together. Validate deployment, bonding, transfers, fees/refunds, rewards, and peer replay with wide amounts. The final published head needs successful amd64 and arm64 Integration gates. Local unit tests and synthetic benchmarks do not replace these runs.

The cleanup admission gate also needs current verification evidence and acceptance signed by an allowed maintainer. Regenerating evidence alone cannot supply that signature. No acceptance waiver or CI bypass is part of this change.

### Local validation of the candidate

Workspace all-target compilation, strict Clippy, and formatting checks passed. Casper unit tests passed 712 cases with one existing ignored case. The release library suites passed 130 block-storage, 162 models, 419 node, and 129 shared tests. All 1,146 enabled main Casper integration cases passed. Twelve existing cases remain ignored by the repository.

The first runner completed 294 cases. Nextest ran the remaining 852 cases with the repository scheduling rules. A test-name inventory check confirmed complete coverage without duplicate passes. This includes the large-amount fee, transfer, reward, and peer-replay regression.

Opus reviewed the changes and corrected benchmark in two read-only passes, with no blocking correctness or consensus findings. The benchmark smoke test caught malformed fixture keys before measurement. The corrected fixture uses valid deterministic secp256k1 public keys.

Cleanup verification passed all four checks, including the model and process-control negative cases in a disposable container. Every source digest still matches the existing [verification package](../casper/cbc-evidence/runs/soak-cleanup-20261028-pr881/report.json). Its ledgers and unsigned payloads remain unchanged. Cleanup admission still reports 25 entries requiring maintainer acceptance. This is not a live network soak.

### Weight benchmark method

The ignored `casper/tests/weight_bench.rs` target measures fork choice and exact finality at 4, 32, and 100 validators. It also measures recursive clique search. The same source runs against dev `f48e0a6cdc18436fdaf4e2b5a54f52c5eb123a16` and this candidate, with their respective locked dependencies and the same release profile.

The synthetic DAG has eight rounds. Every block cites all blocks from the preceding round, and the main-parent chain follows the first validator. The oracle case has unanimous agreement. The separate clique search uses three disconnected complete groups so that it exercises recursive search. This dense fixture measures these operations and does not represent network throughput or typical block-production latency.

Narrow stakes cycle through 1 to 7 on both builds. Wide stakes multiply them by 2^252 and run only on the PR build. Individual stakes fit uint256. Aggregate stakes exceed uint256 at 32 and 100 validators, but not at 4. Each run checks stable fork choice, exact clique weight, and successful finality decisions.

Measurements use three separate processes per configuration, each with five warmups and 21 timed samples. The dev/PR order reverses in the middle run. Each process is pinned to CPU 2 on an x86_64 Intel Xeon E5-2680 v4 host. The host also runs the Casper integration suite, so these are shared-host measurements.

Setup, input-map cloning, assertions, and output printing are outside the timed regions. Peak RSS comes from Linux `VmHWM` and includes fixture setup and all three operations. It does not measure allocations per call.

To reproduce one configuration:

```bash
WEIGHT_BENCH_VALIDATORS=100 WEIGHT_BENCH_WIDE=0 \
  taskset -c 2 cargo test --release --locked -p casper --test weight_bench -- --ignored --nocapture
```

Repeat for 4 and 32 validators. Run wide mode with `WEIGHT_BENCH_WIDE=1` only on the PR build. Copy the benchmark source unchanged into a clean archive of the pinned dev revision for the narrow baseline. Repeat processes to assess timing variability.

### Weight benchmark results: 2026-10-30

Each timing cell shows median / p95. These are the medians of the three per-process statistics, not pooled sample quantiles. The memory column shows the range across those processes.

| Validators | Build / weights | Fork choice ms | Clique oracle ms | Clique search µs | Peak RSS MiB |
| --- | --- | ---: | ---: | ---: | ---: |
| 4 | dev / small | 0.721 / 0.824 | 0.054 / 0.061 | 2.174 / 2.505 | 5.9–5.9 |
| 4 | pr / small | 0.824 / 0.943 | 0.059 / 0.067 | 2.455 / 2.796 | 6.1–6.2 |
| 4 | pr / wide | 1.041 / 1.178 | 0.068 / 0.090 | 2.703 / 2.989 | 6.0–6.2 |
| 32 | dev / small | 100.874 / 106.779 | 3.214 / 3.809 | 183.152 / 226.245 | 13.6–14.0 |
| 32 | pr / small | 111.902 / 119.389 | 3.308 / 3.685 | 174.639 / 246.867 | 14.2–14.7 |
| 32 | pr / wide | 162.329 / 168.513 | 3.774 / 4.082 | 195.532 / 250.567 | 15.3–15.7 |
| 100 | dev / small | 2366.012 / 2421.258 | 44.966 / 50.969 | 1835.876 / 2025.780 | 78.6–78.9 |
| 100 | pr / small | 2599.679 / 2672.657 | 48.425 / 52.305 | 1881.770 / 2049.701 | 82.3–82.4 |
| 100 | pr / wide | 4110.876 / 4209.154 | 52.476 / 61.761 | 1938.662 / 2589.423 | 94.2–94.3 |

All 27 processes passed their assertions. Cross-build checks confirmed identical fork-choice heads, equal narrow clique weights, and wide clique weights scaled by exactly 2^252.

At 100 validators, narrow BigInt weights increased median fork-choice time by 9.9% and exact-oracle time by 7.7% against dev. Wide weights increased fork-choice time by 58.1% against the PR's narrow weights. The 100-validator fork-choice medians varied by at most 2.1% across repeated runs of each configuration.

Recursive clique-search timings are less stable. The four-validator wide case had a 53.4% spread between run medians at microsecond scale. Small differences in that column are not evidence of improvement.

These measurements compare the whole BigInt migration plus the borrowing changes against dev. They do not isolate the benefit of removing clones. The dense 100-validator fixture costs seconds even on dev, so these figures must not be presented as normal block-production latency.
