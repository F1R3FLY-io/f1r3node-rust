# CbC Evidence: node/src/rust/instances/heartbeat_proposer.rs

- **Status:** pending
- **Task:** TASK-021-26
- **Owner:** pi-session-heartbeat-h1-cbc
- **Phase:** H1 claim registration before implementation

The revised claim is `docs/claims/heartbeat-proposal-amplification-bound.md`. Registration is not verification, discharge, or maintainer acceptance.

The previous record remains byte-identical at `docs/casper/cbc-evidence/history/node-src-rust-instances-heartbeat-proposer-rs-20260821.md`.
Its historical leader-only discharge does not establish H1 correctness or current recovery correctness.

The source digests below identify the registration snapshot with F1/F2 edits. They do not identify a verified H1 implementation.

The nonempty-request enforcement in `casper/src/rust/blocks/proposer/proposer.rs` also requires source-bound evidence before completion.

```json
{
  "artifact": {
    "path": "node/src/rust/instances/heartbeat_proposer.rs",
    "id": "node-src-rust-instances-heartbeat-proposer-rs",
    "commit": "21ad594a7",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "c693cdbc1e9f10325bd35aa3deb187372bcb4cba0a3902067258b1fb7aecbadb"
  },
  "claim": "docs/claims/heartbeat-proposal-amplification-bound.md",
  "claim_digests": {
    "docs/claims/heartbeat-proposal-amplification-bound.md": "8b70ed654c39dc924a3a947d8a91ce873010781973eb6dcbbb78b8bfe5f03881"
  },
  "adapter": null,
  "status": "pending",
  "scope": "heartbeat-h1-bounded-nonempty-cooldown-exemption",
  "evidence": {
    "kind": "claim-registration-only",
    "ref": "docs/claims/heartbeat-proposal-amplification-bound.md",
    "sha256": "8b70ed654c39dc924a3a947d8a91ce873010781973eb6dcbbb78b8bfe5f03881"
  },
  "tiers": {
    "refutation": "pending",
    "construction": "pending",
    "binding": "pending"
  },
  "registered_at": "2026-10-18T21:34:04Z",
  "waiver": null,
  "verified_at": null,
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/history/node-src-rust-instances-heartbeat-proposer-rs-20260821.md",
    "sha256": "e7ee126e11bf5a83a029bda23216afa7553887462cf1307430eac98d1b9eca7b",
    "commit": "3fd707102"
  },
  "companion_registration_snapshot": {
    "path": "casper/src/rust/blocks/proposer/proposer.rs",
    "sha256": "cc098b4893ff97e2a5c9b54db27107ac84f49d4e11ab811b3279b1f7d548265d",
    "status": "pending"
  }
}
```
