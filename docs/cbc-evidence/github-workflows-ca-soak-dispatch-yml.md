# CbC Evidence: .github/workflows/ca-soak-dispatch.yml

jltatbeach accepted CLAIM-SOAK-GATE-001 at revision `7d627df83` on 2026-10-21 (PR #793, comment 6305076788). This record is discharged for the bytes of the file at the tested commit `d1d691d58`. The live gate soaks stay pending.

```json
{
  "artifact": {
    "path": ".github/workflows/ca-soak-dispatch.yml",
    "id": "github-workflows-ca-soak-dispatch-yml",
    "commit": "17b6ecb7cc1b7313438df5ad490fa19dbe3a76a7",
    "commit_is_base": true,
    "sha256": "e0ac1c25a83f358d21b265e1637cbe1c24b5f1435ddd78aa38c50df6ba6016bc",
    "working_tree": true
  },
  "claim": "docs/claims/soak-release-gates.md",
  "claim_ids": [
    "CLAIM-SOAK-GATE-001"
  ],
  "claim_digests": {
    "docs/claims/soak-release-gates.md": "11cb0a78d7a07e171abfdd8161383e803bd27f57cc25f223eb53613d27ad3afd"
  },
  "adapter": null,
  "status": "discharged",
  "scope": "release-train-soak-gate",
  "evidence": {
    "kind": "maintainer-acceptance",
    "ref": "docs/casper/cbc-evidence/runs/soak-release-gates-acceptance-20261021-01/report.json",
    "sha256": "e334fb503eaf78276eee47ce20552c7590735109f072bfd6f83adf7be567022f"
  },
  "tiers": {
    "refutation": "bounded-safety-pass",
    "construction": "not-applicable",
    "binding": "passed"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-10-21T00:39:55Z",
  "previous_ledger": {
    "path": "docs/cbc-evidence/github-workflows-ca-soak-dispatch-yml.md",
    "commit": "a3c8bf175",
    "sha256": "33dcf7563bfe7a538de5c2d7a82e3aacd04656c89cf5e1cfabb82bce59d909b8"
  }
}
```
