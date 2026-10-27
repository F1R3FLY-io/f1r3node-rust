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
- Fresh databases use versioned formats. Incompatible old data is rejected without migration.
- Block heights, timestamps, counts, and phlo units remain bounded integers.
- Existing reward formulas, withdrawal timing, and slashing rules remain unchanged.
- Fee validation is consistent across admission, execution, and replay.
- General Rholang BigInt arithmetic retains its existing behavior.

## Acceptance

Test 0, 1, 2^63-1, 2^63, 2^128, 2^256-1, negative values, and 2^256. Verify exact encoding, hashing, storage, reporting, and replay. Exercise intermediate products above 256 bits. Compare execution and peer replay costs, events, errors, and state roots.

Run workspace checks and an independent Opus review. Keep commits focused. Do not push this branch.
