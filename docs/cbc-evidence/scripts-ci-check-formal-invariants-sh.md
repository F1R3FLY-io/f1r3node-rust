# CbC Evidence: scripts/ci/check-formal-invariants.sh

The combined package records current source hashes and scoped verification results. Both node claims remain pending named maintainer acceptance.

Model proofs and finite Rust correspondence retain the limits stated in the package. This record does not qualify a live campaign.

```json
{
  "artifact": {
    "path": "scripts/ci/check-formal-invariants.sh",
    "id": "scripts-ci-check-formal-invariants-sh",
    "commit": "38e57604187feab97cb45f000f95270b12a9f8bf",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "cd1261a9ae66708a3434f9cab41ce12439ccee0e46bd087bb7e430585a28d747"
  },
  "claim": "docs/claims/casper-node-observation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-001",
    "CLAIM-CASPER-NODE-OBSERVATION-002"
  ],
  "claim_digests": {
    "docs/claims/casper-node-observation.md": "d02013bb25aac9d9d16795651cbcbacd2227eae3873661f0258c6b6bd35ff2c6",
    "docs/claims/casper-node-authority-snapshot.md": "3130ca377c849facee112379fa656b34f63ab8858f01f3f8f2a6c5447a72c807"
  },
  "adapter": null,
  "status": "pending",
  "scope": "task-019-4-combined-b11-cycle-03",
  "evidence": {
    "kind": "tiered-evidence-not-discharge",
    "ref": "docs/cbc-evidence/runs/casper-node-claim-gate-38e576041-01/report.json",
    "sha256": "785a7188828a9aa0688dc28cfdac0303937f592f47d47e082d1ba752131c2e3e"
  },
  "tiers": {
    "refutation": "recorded",
    "construction": "recorded-partial",
    "binding": "recorded-partial"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": null,
  "previous_record": {
    "artifact": "scripts/ci/check-formal-invariants.sh",
    "path": "docs/cbc-evidence/scripts-ci-check-formal-invariants-sh.md",
    "sha256": "686ae226ff2b05714d804b1b921ecf97b9c9d1687dbf46a4ef94aa5ecf7af46d",
    "archive_member": "previous-records/scripts-ci-check-formal-invariants-sh.md"
  }
}
```
