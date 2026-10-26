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

## Preserved policy

- The default bond minimum remains 1 and maximum remains 9223372036854775807.
- Bond limits use BigInt without changing their configured values.
- Explicit test configuration can supply values above the signed 64-bit range.
- Fee arithmetic, admission, and failed-precharge replay behavior remain as in PR #54.
- The separate fee-hardening proposal is outside this scope.
- The system-reply peek fix remains a required pipeline dependency.

## Upgrade and replay validation

- The protocol constant remains 1. Genesis and proposal headers use that existing value.
- Team review must choose activation, versioning, and historical-state handling before deployment.
- Tests use fresh local state. They do not establish compatibility with an existing or mixed-version network.
- The production LMDB mapping registers the metadata schema store.
- Startup checks the schema before legacy LFB migration can decode or change metadata.
- Regression tests exercise production LMDB restart, peer replay with large stakes, and system-reply peek behavior.
