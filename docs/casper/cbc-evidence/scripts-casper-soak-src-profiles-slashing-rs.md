# CbC Evidence: scripts/casper-soak/src/profiles/slashing.rs

The user accepted this bounded pre-merge binding and ratified the workflow tag. Live execution remains outside this discharge.

The maintainer accepted the PR #436 review remediation of this file on 2026-10-03 (PR #441, comment 5972145164) at revision `1aea3f6b4`.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/profiles/slashing.rs",
    "id": "scripts-casper-soak-src-profiles-slashing-rs",
    "commit": "1aea3f6b4905533e3891b8b497aacb6198ef08e9",
    "commit_is_base": false,
    "working_tree": true,
    "sha256": "00203010bcad782b9c9631aa72e5052f4d49830ca9ba08b0f7fcaf78c5dfd4c2",
    "sha256_before_review_remediation": "694491c0d9f4ac986424a0c4cc278ae9918674c4e27fbc4ddf0983e3c2b00665"
  },
  "claim": "docs/claims/casper-soak-slashing.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-006"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-slashing.md": "6954ff5fe9edb29909f88f94b328aced5229ac5c7387b67d02c185c4b717845b"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-slashing-profile",
  "evidence": {
    "kind": "accepted-review-remediation",
    "ref": "docs/casper/cbc-evidence/runs/casper-profile-review-fixes-acceptance-20261003-01/report.json",
    "sha256": "5c1f90f31886844d58b9c57992ced1905eea30b62555c31d7429ee5dea0ae633"
  },
  "tiers": {
    "refutation": "bounded-safety-pass",
    "construction": "not-applicable",
    "binding": "passed"
  },
  "phase_status": {
    "pre_pr216_merge": "discharged",
    "post_pr216_merge": "blocked"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-10-03T18:30:00Z",
  "pending": [
    "external-evidence-publication"
  ],
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/runs/casper-slashing-ratification-20260919-01/previous-ledgers.tar.gz",
    "sha256": "084343234fa3ce8b30c87a0de1143cbd32f91b7685873ade6dacce906ddfb998",
    "member": "scripts-casper-soak-src-profiles-slashing-rs.md",
    "member_sha256": "04a8a02d576479ae56fe936ae74b420b67745cd95f4f6db0fde300c6016aada4"
  },
  "evidence_before_review_remediation": {
    "kind": "ratified-bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-slashing-ratification-20260919-01/report.json",
    "sha256": "b47dc09e4d3f901503b88ae0ce346c94aecc4a0fc60d3b76079c8b19f54070ca"
  },
  "acceptance": {
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/441#issuecomment-5972145164",
    "revision": "1aea3f6b4905533e3891b8b497aacb6198ef08e9",
    "reviewed_at": "2026-10-03T18:24:22Z"
  }
}
```
