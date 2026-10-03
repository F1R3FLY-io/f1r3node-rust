# CbC Evidence: casper/src/rust/util/proto_util.rs

The named maintainer accepted CLAIM-CASPER-NODE-OBSERVATION-004 at revision `3ab092cc5` on 2026-09-30 (PR #447, comment 5918385665). This record is discharged for the Batch D bytes of the file. The artifact bytes changed on 2026-10-03 through the dev merge `cf11229eb` (dev PR #496 removed `kept_rejected_records`). The Batch D change of this branch is unchanged, and the acceptance stands for it.

```json
{
  "artifact": {
    "path": "casper/src/rust/util/proto_util.rs",
    "id": "casper-src-rust-util-proto-util-rs",
    "commit": "cf11229eb0b1e49fecd098b4c26782cf624fea9b",
    "commit_is_base": false,
    "working_tree": false,
    "sha256": "d7a75da5e6f52ba0e8873a581041af82eaa0331459ee180428effc97730d3a2c",
    "sha256_at_registration": "4ce3e06c8b5c01561f5470407b7d3e3245982f94e0e1b229fc01d325aadcea77",
    "sha256_at_acceptance": "f14bcb5f9d6f751685c9285b4b3b3cbab213a8d1ffbb8ac56380b5805a7065e7"
  },
  "claim": "docs/claims/casper-node-fork-choice-observation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-004"
  ],
  "claim_digests": {
    "docs/claims/casper-node-fork-choice-observation.md": "24619b06b7c3c4b3849227cea5126c237a9f45e97c628205003866ee6c3c1940"
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
  "previous_record": {
    "path": "docs/cbc-evidence/casper-src-rust-util-proto-util-rs.md",
    "sha256": "46ee3d470029edf755d326117ccb5e76b3d4564a1426a509da1723afcf1a2cdc",
    "commit": "2d4af9134db60ff1393442e844e313be9e907813",
    "status": "discharged",
    "artifact_sha256": "f14bcb5f9d6f751685c9285b4b3b3cbab213a8d1ffbb8ac56380b5805a7065e7"
  },
  "acceptance": {
    "claim_id": "CLAIM-CASPER-NODE-OBSERVATION-004",
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#issuecomment-5918385665",
    "revision": "3ab092cc58fb30f4da39e6c8b28b8d25206c661b",
    "reviewed_at": "2026-09-30T19:41:35Z"
  },
  "refresh": {
    "date": "2026-10-03",
    "reason": "The dev merge cf11229eb brought dev PR #496 (per-block deploy facts cache), which removed kept_rejected_records and the RejectedDeploy import. The Batch D metered entry points of this branch are unchanged.",
    "delta_source": "dev 736b53add..57b1f7a76 (PR #496)",
    "branch_bytes_unchanged": true,
    "acceptance_covers_branch_change": true
  }
}
```
