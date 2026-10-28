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
