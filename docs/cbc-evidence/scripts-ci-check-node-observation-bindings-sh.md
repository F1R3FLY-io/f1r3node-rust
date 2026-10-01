# CbC Evidence: scripts/ci/check-node-observation-bindings.sh

The named maintainer accepted CLAIM-CASPER-NODE-OBSERVATION-004 at revision `3ab092cc5` on 2026-09-30 (PR #447, comment 5918385665). This record is discharged for the Batch D bytes of the file.

```json
{
  "artifact": {
    "path": "scripts/ci/check-node-observation-bindings.sh",
    "id": "scripts-ci-check-node-observation-bindings-sh",
    "commit": "3ab092cc58fb30f4da39e6c8b28b8d25206c661b",
    "commit_is_base": true,
    "working_tree": false,
    "sha256": "e1e11f3ec3318db2f49f7fa73edb143834d801922c1ab37a2e0f18fd3b8d1605"
  },
  "claim": "docs/claims/casper-node-observation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-001",
    "CLAIM-CASPER-NODE-OBSERVATION-004"
  ],
  "status": "discharged",
  "scope": "batch-d-acceptance-01",
  "adapter": null,
  "evidence": {
    "kind": "tiered-evidence-accepted",
    "ref": "docs/cbc-evidence/runs/casper-node-fork-choice-batch-d-3ab092cc5-01/report.json",
    "sha256": "6eb70a7bd3a4833555780e3ebd87104964a45a0c8f518af55869fde4da323297"
  },
  "verification_status": "recorded",
  "acceptance_status": "pending",
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-09-30T19:50:26+00:00",
  "limits": "The report states the verification limits. Earlier acceptance does not cover the corrected source.",
  "claim_digests": {
    "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
    "docs/claims/casper-node-fork-choice-observation.md": "24619b06b7c3c4b3849227cea5126c237a9f45e97c628205003866ee6c3c1940"
  },
  "previous_record": {
    "artifact": "scripts/ci/check-node-observation-bindings.sh",
    "path": "docs/cbc-evidence/scripts-ci-check-node-observation-bindings-sh.md",
    "sha256": "d10ed8a0963398a85b0e330b77b1b321627fcaaa2f83019eb787594e1ab4aadb",
    "commit": "28606f1103343a6d4e1e425a7d9a96c93459c57a"
  },
  "previous_artifact_sha256": "947937192ae8761330cd8b36b437edcd9e3256a53803477e7c076f9cd1c1e5ca",
  "tiers": {
    "refutation": "not-applicable",
    "construction": "not-applicable",
    "binding": "recorded"
  },
  "acceptance": {
    "claim_id": "CLAIM-CASPER-NODE-OBSERVATION-004",
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#issuecomment-5918385665",
    "revision": "3ab092cc58fb30f4da39e6c8b28b8d25206c661b",
    "reviewed_at": "2026-09-30T19:41:35Z"
  }
}
```
