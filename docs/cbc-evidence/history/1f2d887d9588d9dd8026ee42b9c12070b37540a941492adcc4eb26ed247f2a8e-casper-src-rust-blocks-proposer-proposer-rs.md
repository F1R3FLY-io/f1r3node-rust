# CbC Evidence: casper/src/rust/blocks/proposer/proposer.rs

- **Status:** pending
- **Adapter:** bounded-regression-tests
- **Commit:** 197a2f9952e235ce5130f8b1a1a4940b7ae07277
- **Verified:** 2026-11-25T20:18:40Z

Claim:

> A bonded validator that awaits its own history returns HistoryIncomplete before block creation. An unbonded validator returns NotBonded.

```json
{
  "artifact": {
    "commit": "197a2f9952e235ce5130f8b1a1a4940b7ae07277",
    "path": "casper/src/rust/blocks/proposer/proposer.rs"
  },
  "status": "pending",
  "adapter": "bounded-regression-tests",
  "claim": "A bonded validator that awaits its own history returns HistoryIncomplete before block creation. An unbonded validator returns NotBonded.",
  "evidence": {
    "kind": "production-proposer-tests",
    "ref": "casper/tests/multi_node/restored_validator_spec.rs",
    "tests": [
      "a_restored_validator_does_not_reuse_the_sequence_number_of_its_lost_block",
      "a_held_node_that_is_not_bonded_reports_that_it_is_not_bonded"
    ],
    "passed": 2,
    "detail": "The restore fixture delivers the last signed block before the gate opens. The tests cover this delivery order and refusal precedence. Safety under arbitrary delivery order and timeout remains open."
  },
  "verified_at": "2026-11-25T20:18:40Z",
  "verification_id": "32a7088e-73be-487f-941b-44d235cf2912",
  "inputs": {
    "artifact": "casper/src/rust/blocks/proposer/proposer.rs",
    "source_sha256": "0028ea78a97a9244960673c52cd4337f0f5ccbc084556cfc5ba0730e28aa7ecb",
    "claim": {
      "path": "docs/claims/restore-proposal-gate.md",
      "sha256": "428b877de17bc239f6ca3b77f196686512c249a020f57443ea78c76fee403048"
    }
  },
  "evidence_files": {
    "casper/tests/multi_node/restored_validator_spec.rs": "ff21b90ff5e71d94f0699148b27bcebf9fe3e8d70be703afa4ebba5ea44a57a5"
  }
}
```
