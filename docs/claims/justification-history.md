# Justification history during catch-up

Status: pending maintainer review of the specification and Rust binding.

## Scope

The artifact is [Validate::justification_regressions](../../casper/src/rust/validate.rs).
The caller runs `sequence_number` first. That check requires the candidate's sequence number to follow its signed creator justification.

For every permitted arrival order, justification regression means a decrease from the view in that signed predecessor. A later message in the receiver's latest-message table must not change the verdict on an earlier candidate. This is an unbounded claim about history. The finite test and model domains do not bound the protocol.

## Requirements

1. Select the comparison block through the candidate's creator justification. Use no receiver-local latest-message entry for that selection.
2. Compare every validator present in both justification maps, including the candidate sender. Reject a lower sequence number when the new witness is valid. Retain the existing exemption for an invalid new witness.
3. Return `Undecidable` with the missing hash if the predecessor or either required witness is absent. A storage error must remain an error. Missing history must not become an invalid-block verdict or acceptance.
4. Keep the earlier sequence-number validation. A stale self-reference with a sequence gap must still be rejected.

## Evidence and limits

The [investigation](../casper/justification-history-investigation.md) records the failing build, artifact, history, and validation commands.
The [model area](../../formal/tlaplus/justification_history/README.md) records the finite bounds, proof scope, assumptions, and review requirement.

The construction evidence consists of `history_latest_independence`, `history_signed_predecessor`, `history_regression_exact`, and `history_invalid_witness_exemption` in [BugFixSelfRegression.v](../../formal/rocq/slashing/theories/BugFixSelfRegression.v).
These theorems cover predecessor selection and the comparison predicate over unbounded natural numbers and lists. They do not prove the complete Rust DAG, storage implementation, deferral path, or validator lifecycle.
Production Rust regressions bind the selector, comparison, missing-evidence path, and sequence check to representative DAG histories. TLC covers finite arrival orders and incomplete evidence. Neither provides a proof of the complete Rust implementation.

The existing T-9.6 proofs remain intact. Their historical binding to the receiver's latest DAG view does not certify this change. Review must confirm the predecessor-relative specification and the updated Rust binding. No acceptance or waiver is supplied by this record.
