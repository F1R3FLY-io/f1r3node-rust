# CbC Evidence: scripts/ci/check-casper-soak-bindings.sh

The user authorized this bounded harness renewal after fresh hosted verification of the formal-gate workflow. Node execution and protection-rule activation remain outside this discharge.

The previous ledger remains in the archive below. Earlier reports retain their execution identities and limitations.

The maintainer accepted the R8 named-claim change of this file on 2026-10-23 (PR #700, comment 6363192996) at revision `c49e736c2`.

```json
{
  "artifact": {
    "path": "scripts/ci/check-casper-soak-bindings.sh",
    "id": "scripts-ci-check-casper-soak-bindings-sh",
    "commit": "c49e736c2b75ecf7aa4a9977c631233ce1de551e",
    "commit_is_base": false,
    "sha256": "5e5f5c90edd18e6ec380d513b970ab8cac4a82bb7b84ec9d7f0ad4c2f897493a",
    "working_tree": true,
    "sha256_before_change": "2077865b8f68e8dfde219c54548b528fbf318eeb24143765294f890aa4432a76"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "d195ca6afe951e6806ccd9724a63bc5171374d3e0809196827661e2cb70bc855"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-harness-only",
  "evidence": {
    "kind": "accepted-change",
    "ref": "docs/casper/cbc-evidence/runs/casper-r8-named-claims-acceptance-20261023-01/report.json",
    "sha256": "0f0a8551b06cf729fea54ad95737ee6f5ee1d50770a28d7e79056a65640084c8"
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
  "verified_at": "2026-10-23T10:50:00Z",
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/previous-metadata.tar.gz",
    "sha256": "855516a6685384190c8a4f427ff547fc2c8a0f2b6d894849d26dea9fde91b6d9",
    "member": "prior/ledgers/scripts-ci-check-casper-soak-bindings-sh.md",
    "member_sha256": "fd4afea9536b3431d5377d6ff2a43fd72e8436c84ceb6200631f4bc88d824a7d"
  },
  "review_candidate": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/candidate-ledgers/scripts-ci-check-casper-soak-bindings-sh.md",
  "evidence_before_change": {
    "kind": "hosted-source-bound-workflow-renewal",
    "ref": "docs/casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/report.json",
    "sha256": "a7397d9c2fa35436452872d3112bdaae755bc0de0f947955e847d1bf9e4b8831"
  },
  "acceptance": {
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/700#issuecomment-6363192996",
    "revision": "c49e736c2b75ecf7aa4a9977c631233ce1de551e",
    "reviewed_at": "2026-10-23T10:50:00Z"
  }
}
```
