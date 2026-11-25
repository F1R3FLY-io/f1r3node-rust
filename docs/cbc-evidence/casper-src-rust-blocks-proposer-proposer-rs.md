# CbC Evidence: casper/src/rust/blocks/proposer/proposer.rs

- **Status:** pending
- **Adapter:** bounded-regression-tests
- **Commit:** a9abb1a0555ac3e61122071cc7b7017431a97888
- **Verified:** 2026-11-25T22:05:52Z

Claim:

> A bonded signer with an unconfirmed peer restore is held before block creation until its operator confirms first use or the exact available last signed block. An unbonded signer reports NotBonded.

```json
{
  "artifact": {
    "commit": "a9abb1a0555ac3e61122071cc7b7017431a97888",
    "commit_is_base": true,
    "working_tree": true,
    "path": "casper/src/rust/blocks/proposer/proposer.rs"
  },
  "status": "pending",
  "adapter": "bounded-regression-tests",
  "claim": "A bonded signer with an unconfirmed peer restore is held before block creation until its operator confirms first use or the exact available last signed block. An unbonded signer reports NotBonded.",
  "evidence": {
    "kind": "production-proposer-tests",
    "ref": "casper/tests/multi_node/restored_validator_spec.rs",
    "passed": 3,
    "detail": "The production proposer tests cover checkpoint arrival, refusal of first use after signing, peer convergence without approval, stale-generation rejection, first use and unbonded precedence. Two separate stored-DAG engine tests cover pending and ordinary restart. The operator supplies complete signing history and exclusive key use."
  },
  "verified_at": "2026-11-25T22:05:52Z",
  "verification_id": "c869c06f-32e5-45e1-8a73-1d11cf8efbab",
  "inputs": {
    "artifact": "casper/src/rust/blocks/proposer/proposer.rs",
    "source_sha256": "90e044b8736a7e9d32c78506dc50d4b31a513a4c924bad75e3e29859c970e6dc",
    "claim": {
      "path": "docs/claims/restore-proposal-gate.md",
      "sha256": "725b10221c18840d1a07c4a2104d2cb6eeda04692173b0cfb1be9ed12c8f6cc3"
    }
  },
  "evidence_files": {
    "casper/tests/multi_node/restored_validator_spec.rs": "cd45fb6d2ccc710c9ffb2b456490135df01e5941c1591eb7433c0f4ccc2de866"
  }
}
```
