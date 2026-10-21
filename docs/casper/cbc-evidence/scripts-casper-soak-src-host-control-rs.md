# CbC Evidence: scripts/casper-soak/src/host_control.rs

The user authorized this bounded harness renewal after fresh hosted verification of the formal-gate workflow. Node execution and protection-rule activation remain outside this discharge.

The previous ledger remains in the archive below. Earlier reports retain their execution identities and limitations.

The maintainer accepted the PR #811 owner scan change of this file, its narrower ownership guarantee, and its limitation on 2026-10-21 (PR #811, comment 6324960804) at revision `32798242d`. The owner scan skips a same-user process when its environment read fails with EACCES or EPERM. An owned process with an unreadable environment escapes cleanup and the OOM preference.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/host_control.rs",
    "id": "scripts-casper-soak-src-host-control-rs",
    "commit": "4c38b5489fe35b943d8d6c8db9b0e2103600fcb9",
    "commit_is_base": false,
    "sha256": "3c4c637574f6624f4e1d08518cee22eb8a99e76383f1fc92d0f489db6ed540a8",
    "working_tree": true,
    "sha256_before_change": "933e9cd7d05e763fea581d53f255ddba08fe3737c2046d3bbe6e996a06859aa9"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "6488e5e5eb570f61998281cb2221e63fe6bbe6b0a4b8bde8dc22d80090886c7e"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-harness-only",
  "evidence": {
    "kind": "accepted-change",
    "ref": "docs/casper/cbc-evidence/runs/casper-soak-writer-stop-acceptance-20261021-01/report.json",
    "sha256": "982fc077d124845f51132e013284af25a846e84fca7f3ff0a783a99699434756"
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
  "verified_at": "2026-10-21T19:43:20Z",
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/previous-metadata.tar.gz",
    "sha256": "855516a6685384190c8a4f427ff547fc2c8a0f2b6d894849d26dea9fde91b6d9",
    "member": "prior/ledgers/scripts-casper-soak-src-host-control-rs.md",
    "member_sha256": "972e86d5b951fa278a783a532d06997380cbf5f09783b057c75e7191dcd49158"
  },
  "review_candidate": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/candidate-ledgers/scripts-casper-soak-src-host-control-rs.md",
  "refresh": {
    "accepted_commit": "f9273621c8887947b56d0093a71486338312138e",
    "accepted_sha256": "fb6dafc47a5df2073c4ca618ddf81c4a292229b20e5b08a1f0cfeae306e185bb",
    "production_region_identical": true,
    "reason": "The execute function moved above the test module to satisfy clippy. The production region is byte-identical."
  },
  "evidence_before_change": {
    "kind": "hosted-source-bound-workflow-renewal",
    "ref": "docs/casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/report.json",
    "sha256": "a7397d9c2fa35436452872d3112bdaae755bc0de0f947955e847d1bf9e4b8831"
  },
  "acceptance": {
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/811#issuecomment-6324960804",
    "revision": "32798242d1ab26ffebb2946939d7246bd2899306",
    "reviewed_at": "2026-10-21T19:43:20Z"
  }
}
```
