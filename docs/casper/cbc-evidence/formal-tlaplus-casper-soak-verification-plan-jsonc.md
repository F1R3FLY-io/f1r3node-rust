# CbC Evidence: formal/tlaplus/casper_soak/verification-plan.jsonc

The renewed plan review passes after the hosted Claim001 renewal. Fresh lifecycle controls verify unchanged executable inputs after the metadata update.

The independent governance claim remains pending. This documentation discharge does not establish required-check enforcement.

The eight executable claim inventories remain unchanged. Soaks remain pending, and post-merge verification remains blocked.

The previous pending record remains available at the exact Git revision and digest below.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/verification-plan.jsonc",
    "id": "formal-tlaplus-casper-soak-verification-plan-jsonc",
    "commit": "191e184be556c1f190748143377ab369586c53b6",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "ae5b5e2b434e132d18a7a19107d18f3732e65ba33bc20c4fd3351b409dceaa8b"
  },
  "claim": "formal/tlaplus/casper_soak/README.md",
  "claim_anchor": "documentation-contract",
  "claim_ids": ["CLAIM-CASPER-SOAK-FORMAL-AREA-DOCS"],
  "claim_digests": {
    "formal/tlaplus/casper_soak/README.md": "ba430ad08847614f95d3ff965d671ec75cbe77ac88ae5330548923fe5198f9ab"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "formal-area-documentation-and-plan-consistency-only",
  "evidence": {
    "kind": "bounded-controls-and-documentation-consistency",
    "ref": "docs/casper/cbc-evidence/runs/casper-formal-gate-documentation-20260919-01/report.json",
    "sha256": "662fa837af9027b4ace38ae0b9338fa26472fe2f3d3180d6a24569e6b3329f06"
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
  "verified_at": "2026-09-19T22:49:08Z",
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/formal-tlaplus-casper-soak-verification-plan-jsonc.md",
    "commit": "191e184be556c1f190748143377ab369586c53b6",
    "sha256": "0621a0bf071aa9d623c1ec525c0253243863a134303ff0b01b2da8b4a27fbafd"
  },
  "inventory_scope": "Separate documentation contract. The eight executable claim inventories remain unchanged."
}
```
