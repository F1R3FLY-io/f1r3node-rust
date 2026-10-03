# CbC Evidence: node/src/rust/soak_observer.rs

The named maintainer accepted CLAIM-CASPER-NODE-OBSERVATION-004 at revision `3ab092cc5` on 2026-09-30 (PR #447, comment 5918385665). This record is discharged for the Batch D bytes of the file. The artifact bytes changed on 2026-10-02 through the PR #447 review remediation `5dacf8d32`: `Observer::bind` streams the executable digest instead of buffering the binary. The acceptance covers the bytes at `3ab092cc5`. The maintainer has not reviewed the remediation.

```json
{
  "artifact": {
    "path": "node/src/rust/soak_observer.rs",
    "id": "node-src-rust-soak-observer-rs",
    "commit": "cf11229eb0b1e49fecd098b4c26782cf624fea9b",
    "commit_is_base": false,
    "working_tree": false,
    "sha256": "297391d51ee8af79a62fcea84887981cd47350ba310fcd286e856948562dc62b",
    "sha256_at_registration": "59f40fd5632955d7fbaae0cd6221e942d3dc7505ad8a096a2d95bce10d2ed878",
    "sha256_at_acceptance": "7067d54f444b0f689984ad77ab34efa5c81113443c12e0e820a66d562edd8e98"
  },
  "claim": "docs/claims/casper-node-fork-choice-observation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-004"
  ],
  "claim_digests": {
    "docs/claims/casper-node-fork-choice-observation.md": "24619b06b7c3c4b3849227cea5126c237a9f45e97c628205003866ee6c3c1940"
  },
  "previous_record": {
    "artifact": "node/src/rust/soak_observer.rs",
    "path": "docs/cbc-evidence/node-src-rust-soak-observer-rs.md",
    "sha256": "40056a192ee92617fda09d8f2953ff76371eb1dabb20e51e89cec46738466d76",
    "commit": "2d4af9134db60ff1393442e844e313be9e907813",
    "status": "discharged",
    "artifact_sha256": "7067d54f444b0f689984ad77ab34efa5c81113443c12e0e820a66d562edd8e98"
  },
  "status": "discharged",
  "scope": "batch-d-acceptance-01",
  "evidence": {
    "kind": "tiered-evidence-accepted",
    "ref": "docs/cbc-evidence/runs/casper-node-fork-choice-batch-d-3ab092cc5-01/report.json",
    "sha256": "6eb70a7bd3a4833555780e3ebd87104964a45a0c8f518af55869fde4da323297"
  },
  "tiers": {
    "refutation": "recorded",
    "construction": "pending",
    "binding": "recorded"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-10-03T14:10:00+00:00",
  "acceptance": {
    "claim_id": "CLAIM-CASPER-NODE-OBSERVATION-004",
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#issuecomment-5918385665",
    "revision": "3ab092cc58fb30f4da39e6c8b28b8d25206c661b",
    "reviewed_at": "2026-09-30T19:41:35Z"
  },
  "refresh": {
    "date": "2026-10-03",
    "reason": "Review remediation 5dacf8d32 replaced the buffered executable read with Sha256Hasher::hash_reader over executable.take(MAX_EXECUTABLE_BYTES + 1). The size bound and the digest value are unchanged. Memory use no longer grows with the binary size.",
    "delta_source": "branch commit 5dacf8d32 (PR #447 review, comment 5965523714)",
    "branch_bytes_unchanged": false,
    "acceptance_covers_branch_change": false
  }
}
```
