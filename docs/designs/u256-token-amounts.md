# Unsigned 256-bit token amounts across the node

## Scope

Every individual token amount is a BigInt in the unsigned 256-bit range: genesis allocations, bonds, the bond
limits, fee prices, charges, refunds and transfer amounts. Intermediate products, consensus totals and signed deltas
stay arbitrary precision. The minimum bond stays at 1; the default maximum bond is 2^256-1.

## Specification

- Monetary values on peer protobuf messages use exactly 32 unsigned big-endian bytes.
- Public monetary fields are canonical decimal strings, in requests and responses. Successful values are between 0
  and 2^256-1.
- Failed-transfer reports keep the exact attempted value, including negative values.
- Replaced protobuf field numbers are reserved. New fields have no legacy fallback.
- Signature and block-hash encoders use the canonical representation.
- Persisted amounts keep their full precision across storage and restart.
- Block heights, timestamps, counts and phlo units remain bounded integers.
- Reward formulas, withdrawal timing and slashing rules are unchanged.
- Fee validation is the same at admission, execution and replay.
- General Rholang BigInt arithmetic keeps its existing behavior.

## Interfaces

- `BondProto.stake` and `DeployDataProto.phloPrice` hold exactly 32 unsigned big-endian bytes. The deploy signature
  covers that encoding. The bond Merkle leaf hashes the `BondProto` encoding (block hash scheme 3).
- HTTP, WebSocket and public gRPC amount fields are decimal strings: stakes, total stakes, transfers, `phloPrice`
  and `minPhloPrice`. JSON numbers are rejected for these fields.
- HOCON bond limits and the minimum fee price are quoted canonical decimal strings. CLI arguments, bonds files and
  genesis allocation files accept canonical decimal digits and `#` comments. Negative values, leading zeros and
  values above 2^256-1 are rejected.
- A deploy is rejected if its price or maximum charge exceeds 2^256-1. Refunds use exact arithmetic even when a
  reported cost exceeds the signed 64-bit range.
- Reward and finality calculations use arbitrary-precision intermediate values. Display-only fault-tolerance ratios
  are rounded after exact division.
- PoS accepts only BigInt bond amounts (`amount matches BigInt`).

## Validation coverage

Boundary tests cover 0, 1, 2^63-1, 2^63, 2^128, 2^256-1, negative values and 2^256 through the wire codecs, signed
deploys, JSON strings, config parsing, transfer reports and storage reopening. Integration tests cover genesis
allocation at 2^256-1 and peer replay with large validator stakes.

`uint256_fees_transfers_and_rewards_survive_peer_replay` uses a 2^230 payer balance, a 2^150 transfer, a 2^128 fee
price and 2^190 validator stakes. It checks a successful transfer and a failing deploy on two nodes, exact fees and
refunds, payer and recipient balances, and committed rewards. The reward multiplication exceeds 256 bits without
truncation. Peer block acceptance checks replayed events, costs and the resulting state root.

## Weight benchmark

The ignored `casper/tests/weight_bench.rs` target measures fork choice and exact finality at 4, 32 and 100
validators, and recursive clique search. The same source runs against a dev revision and this branch, with their
locked dependencies and the same release profile.

The synthetic DAG has eight rounds. Every block cites all blocks from the preceding round, and the main-parent chain
follows the first validator. The oracle case has unanimous agreement. The clique search uses three disconnected
complete groups so that it recurses. This dense fixture measures these operations, not network throughput or block
production latency.

Narrow stakes cycle through 1 to 7 on both builds. Wide stakes multiply them by 2^252 and run only on this branch.
Individual stakes fit uint256; aggregate stakes exceed it at 32 and 100 validators.

Each configuration runs three processes, each with five warmups and 21 timed samples, pinned to one CPU on a shared
x86_64 host. Setup, input cloning, assertions and printing are outside the timed regions. Peak RSS is Linux
`VmHWM`.

```bash
WEIGHT_BENCH_VALIDATORS=100 WEIGHT_BENCH_WIDE=0 \
  taskset -c 2 cargo test --release --locked -p casper --test weight_bench -- --ignored --nocapture
```

Each timing cell is median / p95, as the median of the three per-process statistics. Memory is the range across
processes.

| Validators | Build / weights | Fork choice ms | Clique oracle ms | Clique search µs | Peak RSS MiB |
| --- | --- | ---: | ---: | ---: | ---: |
| 4 | dev / small | 0.721 / 0.824 | 0.054 / 0.061 | 2.174 / 2.505 | 5.9–5.9 |
| 4 | branch / small | 0.824 / 0.943 | 0.059 / 0.067 | 2.455 / 2.796 | 6.1–6.2 |
| 4 | branch / wide | 1.041 / 1.178 | 0.068 / 0.090 | 2.703 / 2.989 | 6.0–6.2 |
| 32 | dev / small | 100.874 / 106.779 | 3.214 / 3.809 | 183.152 / 226.245 | 13.6–14.0 |
| 32 | branch / small | 111.902 / 119.389 | 3.308 / 3.685 | 174.639 / 246.867 | 14.2–14.7 |
| 32 | branch / wide | 162.329 / 168.513 | 3.774 / 4.082 | 195.532 / 250.567 | 15.3–15.7 |
| 100 | dev / small | 2366.012 / 2421.258 | 44.966 / 50.969 | 1835.876 / 2025.780 | 78.6–78.9 |
| 100 | branch / small | 2599.679 / 2672.657 | 48.425 / 52.305 | 1881.770 / 2049.701 | 82.3–82.4 |
| 100 | branch / wide | 4110.876 / 4209.154 | 52.476 / 61.761 | 1938.662 / 2589.423 | 94.2–94.3 |

At 100 validators, narrow BigInt weights add 9.9% to median fork-choice time and 7.7% to exact-oracle time against
dev. Wide weights add 58.1% to fork-choice time against narrow weights. Clique-search timings are unstable at
microsecond scale, so small differences in that column are noise.
