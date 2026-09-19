# CbC Evidence: scripts/casper-soak/src/host_control.rs

The user accepted the repaired bounded H01–H10 binding review for this file at commit `f9273621c`. The file changed on 2026-09-18 to satisfy clippy, so this record is pending again until a new acceptance binds the current source. The `drift` field keeps the accepted commit and digest.

Profile claims, node soaks, post-merge work, and inherited containment limits remain separate. The linked archive preserves the previous acceptance records.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/host_control.rs",
    "id": "scripts-casper-soak-src-host-control-rs",
    "commit": "6adeb7d38cee6d6e0c580e6b3ea1af3720d2b16a",
    "commit_is_base": false,
    "sha256": "933e9cd7d05e763fea581d53f255ddba08fe3737c2046d3bbe6e996a06859aa9"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "ea23866a9aa860d601dfd3dc4eaccfc7ef56a162022e907ab3619ef34c7dee51"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "bounded-harness-only",
  "evidence": {
    "kind": "accepted-bounded-refutation-and-binding",
    "ref": "docs/casper/cbc-evidence/runs/casper-driver-rebind-acceptance-20260918-01/report.json",
    "sha256": "d5cc58da34bb33dce627c3c40bf055e61fef817657698921359f06a4f8a301b1"
  },
  "tiers": {
    "refutation": "bounded-safety-pass",
    "construction": "not-applicable",
    "binding": "pending"
  },
  "phase_status": {
    "pre_pr216_merge": "pending",
    "post_pr216_merge": "blocked"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": null,
  "drift": {
    "accepted_commit": "f9273621c8887947b56d0093a71486338312138e",
    "accepted_sha256": "fb6dafc47a5df2073c4ca618ddf81c4a292229b20e5b08a1f0cfeae306e185bb",
    "changed_at": "2026-09-18",
    "reason": "The execute function moved above the test module to satisfy clippy. The accepted evidence binds the earlier file only."
  },
  "previous_ledger": null,
  "review_candidate": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/candidate-ledgers/scripts-casper-soak-src-host-control-rs.md"
}
```
