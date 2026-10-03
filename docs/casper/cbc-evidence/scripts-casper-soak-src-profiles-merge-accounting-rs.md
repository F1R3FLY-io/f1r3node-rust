# CbC Evidence: scripts/casper-soak/src/profiles/merge_accounting.rs

The user accepted this bounded pre-merge binding. Node accounting and live execution remain outside this discharge.

The maintainer accepted the PR #436 review remediation of this file on 2026-10-03 (PR #441, comment 5972145164) at revision `1aea3f6b4`.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/profiles/merge_accounting.rs",
    "id": "scripts-casper-soak-src-profiles-merge-accounting-rs",
    "commit": "1aea3f6b4905533e3891b8b497aacb6198ef08e9",
    "commit_is_base": false,
    "sha256": "2e7e96dbfefd77688bb8f6c69ca7a6cd86782d5c00dab40b653ef1bd2884ca8f",
    "sha256_before_review_remediation": "e06024f86e336e22584196908400e69623cbf82a26afe08a3858487b821cd2a5"
  },
  "claim": "docs/claims/casper-soak-merge-accounting.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-005"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-merge-accounting.md": "22a6534ef37b1675b6fe69cd04dbae6473d2fa93d9306efb454499cd04c970f3"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-merge-accounting-profile",
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
    "path": "docs/casper/cbc-evidence/runs/casper-merge-accounting-20260919-01/ledgers/scripts-casper-soak-src-profiles-merge-accounting-rs.md",
    "sha256": "7eacebb233ea2806edefa6fc2b4a690319c7221b94474b0e264dbcb9b4338387"
  },
  "evidence_before_review_remediation": {
    "kind": "bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-merge-accounting-acceptance-20260919-01/report.json",
    "sha256": "ba73b6ff31e85ee528490be184a8b95441977bcaba7ceac6eb905fdb1e0a576b"
  },
  "acceptance": {
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/441#issuecomment-5972145164",
    "revision": "1aea3f6b4905533e3891b8b497aacb6198ef08e9",
    "reviewed_at": "2026-10-03T18:24:22Z"
  }
}
```
