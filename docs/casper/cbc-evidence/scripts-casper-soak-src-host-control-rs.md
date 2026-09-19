# CbC Evidence: scripts/casper-soak/src/host_control.rs

The user authorized renewal of the existing bounded H01–H10 binding after the workflow inventory repair. This discharge covers only the pre-merge harness.

Previous evidence and containment limitations remain preserved. Node execution and post-merge work remain outside this renewal.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/host_control.rs",
    "id": "scripts-casper-soak-src-host-control-rs",
    "commit": "8dc35ae4a55e1e10cf59cea69f6de87db4da276b",
    "commit_is_base": true,
    "sha256": "933e9cd7d05e763fea581d53f255ddba08fe3737c2046d3bbe6e996a06859aa9",
    "working_tree": true
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "61e085da51b1f69759c579d8fea92e192541ec28bf0b8905d6b3054ae26d015b"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-harness-only",
  "evidence": {
    "kind": "renewed-bounded-inventory-binding",
    "ref": "docs/casper/cbc-evidence/runs/casper-binding-inventory-renewal-20260919-01/report.json",
    "sha256": "6a0cd52a9153f128cadef74c5469b02a3580e25a53e44fb45cf15d678186e889"
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
  "verified_at": "2026-09-19T19:41:31Z",
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/runs/casper-binding-inventory-renewal-20260919-01/previous-metadata.tar.gz",
    "sha256": "9d7e06983cf0c357adba8408c1fd716099cf39b4d6e179547162e05738e0026d",
    "member": "previous-ledgers/scripts-casper-soak-src-host-control-rs.md",
    "member_sha256": "afd7a9bf5e5612d9a24612264d4c7b625d2b03e5d861c11a53d2d19942c947d3"
  },
  "review_candidate": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/candidate-ledgers/scripts-casper-soak-src-host-control-rs.md",
  "refresh": {
    "accepted_commit": "f9273621c8887947b56d0093a71486338312138e",
    "accepted_sha256": "fb6dafc47a5df2073c4ca618ddf81c4a292229b20e5b08a1f0cfeae306e185bb",
    "production_region_identical": true,
    "reason": "The execute function moved above the test module to satisfy clippy. The production region is byte-identical."
  }
}
```
