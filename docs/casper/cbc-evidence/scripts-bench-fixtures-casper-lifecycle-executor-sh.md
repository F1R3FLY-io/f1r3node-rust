# CbC Evidence: scripts/bench/fixtures/casper-lifecycle-executor.sh

The user authorized renewal of the existing bounded H01–H10 binding after the workflow inventory repair. This discharge covers only the pre-merge harness.

Previous evidence and containment limitations remain preserved. Node execution and post-merge work remain outside this renewal.

```json
{
  "artifact": {
    "path": "scripts/bench/fixtures/casper-lifecycle-executor.sh",
    "id": "scripts-bench-fixtures-casper-lifecycle-executor-sh",
    "commit": "8dc35ae4a55e1e10cf59cea69f6de87db4da276b",
    "commit_is_base": true,
    "sha256": "a2af2205b89c03b7ab503ca5c312e9b2300040540781ada37c33c849a3b94704",
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
    "member": "previous-ledgers/scripts-bench-fixtures-casper-lifecycle-executor-sh.md",
    "member_sha256": "806f0e0a4908c7ac166c3361a00bc075c67a2f6f0c5e8c3451fb948d3bf5164c"
  },
  "review_candidate": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/candidate-ledgers/scripts-bench-fixtures-casper-lifecycle-executor-sh.md"
}
```
