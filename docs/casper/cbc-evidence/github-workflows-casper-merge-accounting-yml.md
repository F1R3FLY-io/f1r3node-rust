# CbC Evidence: .github/workflows/casper-merge-accounting.yml

The user accepted this bounded pre-merge binding. Node accounting and live execution remain outside this discharge.

The maintainer accepted the PR #829 change of the Java install step of this file on 2026-10-23 (PR #829, review 5628129141) at revision `f2f3645dd`.

```json
{
  "artifact": {
    "path": ".github/workflows/casper-merge-accounting.yml",
    "id": "github-workflows-casper-merge-accounting-yml",
    "commit": "f2f3645ddedae8d612e55141cfbd15e9c4ef884b",
    "commit_is_base": false,
    "sha256": "36d4b339cee219f8fb798ccdd9bb82bac7cac2706467bfc60d9599f056e44402",
    "sha256_before_change": "69478722b065d0bebc3c2d8dd6d43a129ee7296ebf58a3ff910e43347997b23d"
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
    "kind": "accepted-change",
    "ref": "docs/casper/cbc-evidence/runs/ci-preinstalled-jdk-acceptance-20261023-01/report.json",
    "sha256": "987eb5ae70d8b738c7adaa7619ff71a713a5f0e804cf067d136c44de873c7bd9"
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
  "verified_at": "2026-10-23T12:21:29Z",
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/runs/casper-merge-accounting-20260919-01/ledgers/github-workflows-casper-merge-accounting-yml.md",
    "sha256": "9115af0995f99db39c62869954e3cc98d6c6320fd62db94b441133bee0bb1c75"
  },
  "evidence_before_change": {
    "kind": "bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-merge-accounting-acceptance-20260919-01/report.json",
    "sha256": "ba73b6ff31e85ee528490be184a8b95441977bcaba7ceac6eb905fdb1e0a576b"
  },
  "acceptance": {
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/829#pullrequestreview-5628129141",
    "revision": "f2f3645ddedae8d612e55141cfbd15e9c4ef884b",
    "reviewed_at": "2026-10-23T12:21:29Z"
  }
}
```
