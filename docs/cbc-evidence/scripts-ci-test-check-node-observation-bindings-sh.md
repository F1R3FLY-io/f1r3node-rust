# CbC Evidence: scripts/ci/test-check-node-observation-bindings.sh

The isolated Linux regression accepts normalized strip output and refuses twelve executable mutations.
The [report](runs/node-observation-ci-refresh-20260930-01/report.json) records the source identities and verification limits.
The new artifact requires claim-inventory review and named maintainer acceptance.

```json
{
  "artifact": {
    "path": "scripts/ci/test-check-node-observation-bindings.sh",
    "id": "scripts-ci-test-check-node-observation-bindings-sh",
    "commit": "bc226f89b46366ea4cb0c1907d6781050afb7d0c",
    "commit_is_base": true,
    "working_tree": false,
    "sha256": "1841a3861695d32b89f511624af3f4057faed3f82422ed6389230c7fbdc60fa7"
  },
  "claim": "docs/claims/casper-node-observation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-001"
  ],
  "status": "pending",
  "scope": "agent-b-ci-correction-refresh-20260930-01",
  "adapter": null,
  "evidence": {
    "kind": "source-bound-hosted-checks-and-elf-mutation-controls",
    "ref": "docs/cbc-evidence/runs/node-observation-ci-refresh-20260930-01/report.json",
    "sha256": "0dc0d1cabf7a8ed2d8846dbffc8a19e9b4beb117286c8b65540d068bfbb9aeba"
  },
  "verification_status": "recorded",
  "acceptance_status": "pending",
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-09-30T04:44:34.480367+00:00",
  "limits": "The report states the verification limits. Earlier acceptance does not cover the corrected source.",
  "claim_digests": {
    "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38"
  },
  "claim_registration_status": "pending claim-inventory review"
}
```
