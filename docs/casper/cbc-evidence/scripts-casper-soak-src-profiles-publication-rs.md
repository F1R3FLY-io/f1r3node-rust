# CbC Evidence: scripts/casper-soak/src/profiles/publication.rs

The user accepted this bounded pre-merge profile binding. Node correctness and live execution remain outside this discharge.

The maintainer accepted the PR #436 review remediation of this file on 2026-10-03 (PR #441, comment 5972145164) at revision `1aea3f6b4`.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/profiles/publication.rs",
    "id": "scripts-casper-soak-src-profiles-publication-rs",
    "commit": "1aea3f6b4905533e3891b8b497aacb6198ef08e9",
    "commit_is_base": false,
    "sha256": "21848ac903ddf4cdcacfbc4ce3b31b1bd1033f64dc034c1bc11604cc544502c1",
    "sha256_before_review_remediation": "7cde33e0be93a9b4a655e4053adcad01ef7e3f755cf4afea0d0c3ab36eaa7aee"
  },
  "claim": "docs/claims/casper-soak-publication.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-003"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-publication.md": "f49995b85ec66fe5da179efeadef28559eaec3df2a55fa69479c7688492b00db"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-publication-profile",
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
  "previous_ledger": {
    "archive": "docs/casper/cbc-evidence/runs/casper-profile-binding-review-20260919-01/ledgers.tar.gz",
    "sha256": "52d9f45f0cff756cc13da2b5770023fd57072c4c5ba984d3822bb75a26acd5db",
    "member": "docs/casper/cbc-evidence/scripts-casper-soak-src-profiles-publication-rs.md"
  },
  "evidence_before_review_remediation": {
    "kind": "bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-profile-acceptance-20260919-01/report.json",
    "sha256": "614d13c3e52ed11afb84a9a0b5f6345db42bc3a684518f9ce7060b15fc08c4da"
  },
  "acceptance": {
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/441#issuecomment-5972145164",
    "revision": "1aea3f6b4905533e3891b8b497aacb6198ef08e9",
    "reviewed_at": "2026-10-03T18:24:22Z"
  }
}
```
