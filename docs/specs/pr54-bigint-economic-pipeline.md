# PR #54 economic amount proposal specification

## Type predicate

- `1n.isBigInt()`, `0n.isBigInt()`, and `(-1n).isBigInt()` return true.
- Int, string, Boolean, float, Nil, collection, and unforgeable values return false.
- A parallel process containing a BigInt is not a BigInt scalar.
- Variables and expressions are evaluated before the predicate checks the value.
- Any arguments cause `MethodArgumentNumberMismatch`.

## Staking

- Bond, delegate, and undelegate amounts use exact native base units.
- Balances, limits, totals, pending withdrawals, and reward calculations preserve BigInt precision.
- Genesis and consensus bond serialization preserve values above `i64::MAX`.
- Consensus weight operations do not convert monetary values to 64-bit integers.
- JSON monetary fields contain decimal strings.
- The authority-finality harness parses stake strings without a fixed integer limit and rejects malformed or mismatched witnesses.
- Tests include a 100 ASI amount at 18 decimals and invalid negative amounts.

## Fees

- Fee metadata requires a positive limit and a nonnegative price. Existing minimum-price policy still applies.
- A precharge must fit the existing nonnegative signed 64-bit fee range.
- Invalid metadata fails at admission, execution, and replay.
- Refunds equal the remaining allowed gas multiplied by price, using checked operations.
- Costs above the gas limit yield zero refund without signed narrowing.
- Tests cover the exact boundary, overflow, negative metadata, and costs above `i64::MAX`.

## Upgrade and replay validation

- The protocol constant remains 1. Genesis and proposal headers use that existing value.
- Team review must choose activation, versioning, and historical-state handling before deployment.
- Tests use fresh local state. They do not establish compatibility with an existing or mixed-version network.
- The production LMDB mapping registers the metadata schema store.
- Startup checks the schema before legacy LFB migration can decode or change metadata.
- Failed precharges require `is_failed = true` and `cost = 0` in block validation and replay.
- Regression tests exercise production LMDB restart, peer replay with large stakes, and forged precharge metadata.
