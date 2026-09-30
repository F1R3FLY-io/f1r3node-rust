# CbC Evidence: scripts/ci/check-node-observation-bindings.sh

Batch D added the claim 004 file to the hash list of the driver. The driver passed on revision `3ab092cc5` in a Linux container. Named maintainer acceptance of the changed source remains pending.

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
  "status": "pending",
  "scope": "batch-d-verification-01",
  "adapter": null,
  "evidence": {
    "kind": "tiered-evidence-recorded-pending-acceptance",
    "ref": "docs/cbc-evidence/runs/casper-node-fork-choice-batch-d-3ab092cc5-01/report.json",
    "sha256": "6eb70a7bd3a4833555780e3ebd87104964a45a0c8f518af55869fde4da323297"
  },
  "verification_status": "recorded",
  "acceptance_status": "pending",
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-09-30T17:50:15+00:00",
  "limits": "The report states the verification limits. Earlier acceptance does not cover the corrected source.",
  "claim_digests": {
    "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
    "docs/claims/casper-node-fork-choice-observation.md": "24619b06b7c3c4b3849227cea5126c237a9f45e97c628205003866ee6c3c1940"
  },
  "previous_record": {
    "path": "docs/cbc-evidence/scripts-ci-check-node-observation-bindings-sh.md",
    "sha256": "7222ee6f13ff4a417af3e4d48d5a40dc90498da364562c217273ad04f5b01013",
    "commit": "3ab092cc58fb30f4da39e6c8b28b8d25206c661b"
  },
  "previous_artifact_sha256": "947937192ae8761330cd8b36b437edcd9e3256a53803477e7c076f9cd1c1e5ca",
  "tiers": {
    "refutation": "not-applicable",
    "construction": "not-applicable",
    "binding": "recorded"
  }
}
```
