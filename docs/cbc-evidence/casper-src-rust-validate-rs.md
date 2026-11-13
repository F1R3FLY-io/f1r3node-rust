# CbC Evidence: casper/src/rust/validate.rs

- **Status:** pending
- **Adapter:** manual
- **Commit:** 52763ce3f4a91adb4429d04b901f415b1aae41d1
- **Verified:** 2026-11-13T00:44:00Z

Claim:

> # Justification history during catch-up
>
> Status: pending maintainer review of the specification and Rust binding.
>
> ## Scope
>
> The artifact is [Validate::justification_regressions](../../casper/src/rust/validate.rs).
> The caller runs `sequence_number` first. That check requires the candidate's sequence number to follow its signed creator justification.
>
> For every permitted arrival order, justification regression means a decrease from the view in that signed predecessor. A later message in the receiver's latest-message table must not change the verdict on an earlier candidate. This is an unbounded claim about history. The finite test and model domains do not bound the protocol.
>
> ## Requirements
>
> 1. Select the comparison block through the candidate's creator justification. Use no receiver-local latest-message entry for that selection.
> 2. Compare every validator present in both justification maps, including the candidate sender. Reject a lower sequence number when the new witness is valid. Retain the existing exemption for an invalid new witness.
> 3. Return `Undecidable` with the missing hash if the predecessor or either required witness is absent. A storage error must remain an error. Missing history must not become an invalid-block verdict or acceptance.
> 4. Keep the earlier sequence-number validation. A stale self-reference with a sequence gap must still be rejected.
>
> ## Evidence and limits
>
> The [investigation](../casper/justification-history-investigation.md) records the failing build, artifact, history, and validation commands.
> The [model area](../../formal/tlaplus/justification_history/README.md) records the finite bounds, proof scope, assumptions, and review requirement.
>
> The construction evidence consists of `history_latest_independence`, `history_signed_predecessor`, `history_regression_exact`, and `history_invalid_witness_exemption` in [BugFixSelfRegression.v](../../formal/rocq/slashing/theories/BugFixSelfRegression.v).
> These theorems cover predecessor selection and the comparison predicate over unbounded natural numbers and lists. They do not prove the complete Rust DAG, storage implementation, deferral path, or validator lifecycle.
> Production Rust regressions bind the selector, comparison, missing-evidence path, and sequence check to representative DAG histories. TLC covers finite arrival orders and incomplete evidence. Neither provides a proof of the complete Rust implementation.
>
> The existing T-9.6 proofs remain intact. Their historical binding to the receiver's latest DAG view does not certify this change. Review must confirm the predecessor-relative specification and the updated Rust binding. No acceptance or waiver is supplied by this record.

```json
{
  "artifact": {
    "path": "casper/src/rust/validate.rs",
    "commit": "52763ce3f4a91adb4429d04b901f415b1aae41d1",
    "working_tree": true,
    "commit_is_base": true
  },
  "claim": "# Justification history during catch-up\n\nStatus: pending maintainer review of the specification and Rust binding.\n\n## Scope\n\nThe artifact is [Validate::justification_regressions](../../casper/src/rust/validate.rs).\nThe caller runs `sequence_number` first. That check requires the candidate's sequence number to follow its signed creator justification.\n\nFor every permitted arrival order, justification regression means a decrease from the view in that signed predecessor. A later message in the receiver's latest-message table must not change the verdict on an earlier candidate. This is an unbounded claim about history. The finite test and model domains do not bound the protocol.\n\n## Requirements\n\n1. Select the comparison block through the candidate's creator justification. Use no receiver-local latest-message entry for that selection.\n2. Compare every validator present in both justification maps, including the candidate sender. Reject a lower sequence number when the new witness is valid. Retain the existing exemption for an invalid new witness.\n3. Return `Undecidable` with the missing hash if the predecessor or either required witness is absent. A storage error must remain an error. Missing history must not become an invalid-block verdict or acceptance.\n4. Keep the earlier sequence-number validation. A stale self-reference with a sequence gap must still be rejected.\n\n## Evidence and limits\n\nThe [investigation](../casper/justification-history-investigation.md) records the failing build, artifact, history, and validation commands.\nThe [model area](../../formal/tlaplus/justification_history/README.md) records the finite bounds, proof scope, assumptions, and review requirement.\n\nThe construction evidence consists of `history_latest_independence`, `history_signed_predecessor`, `history_regression_exact`, and `history_invalid_witness_exemption` in [BugFixSelfRegression.v](../../formal/rocq/slashing/theories/BugFixSelfRegression.v).\nThese theorems cover predecessor selection and the comparison predicate over unbounded natural numbers and lists. They do not prove the complete Rust DAG, storage implementation, deferral path, or validator lifecycle.\nProduction Rust regressions bind the selector, comparison, missing-evidence path, and sequence check to representative DAG histories. TLC covers finite arrival orders and incomplete evidence. Neither provides a proof of the complete Rust implementation.\n\nThe existing T-9.6 proofs remain intact. Their historical binding to the receiver's latest DAG view does not certify this change. Review must confirm the predecessor-relative specification and the updated Rust binding. No acceptance or waiver is supplied by this record.\n",
  "adapter": "manual",
  "status": "pending",
  "verified_at": "2026-11-13T00:44:00Z",
  "evidence": {
    "kind": "rust-regression-tlc-and-rocq",
    "ref": "docs/cbc-evidence/formal/justification-history.json",
    "counterexample": null,
    "detail": "Rust, TLC, Rocq and coqchk checks passed; specification and Rust-binding review remain pending. The unsafe TLC configuration violates HistoryRelative as expected."
  },
  "verification_id": "1bce9cb4-2b24-4dfa-9f28-e095ff261459",
  "inputs": {
    "artifact": "casper/src/rust/validate.rs",
    "source_sha256": "87a0b27a552881051737acb8d8f017404faaf70ff22b785810e36774c5481949",
    "claim": {
      "path": "docs/claims/justification-history.md",
      "sha256": "5f98cecacf073237033012ea289ef4c8a700de575ebe04e240a5f735315cccdb"
    }
  },
  "evidence_files": {
    "docs/cbc-evidence/formal/justification-history.json": "76c9db3513cbeea227c6fc8c20fbcd8ea99ae6de04067d62da6c3f25dc8ef35f"
  }
}
```
