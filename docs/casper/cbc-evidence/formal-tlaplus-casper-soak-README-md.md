# CbC Evidence: formal/tlaplus/casper_soak/README.md

The historical documentation review passed for its recorded inputs. The contract is now pending because the formal-gate workflow change reopens Claim001.

The retained report is historical evidence. Recheck documentation consistency after the new workflow binding is accepted.

The eight executable claim inventories remain unchanged. Soaks remain pending, and post-merge verification remains blocked.

The previous pending record remains available at the exact Git revision and digest below.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/README.md",
    "id": "formal-tlaplus-casper-soak-README-md",
    "commit": "1f749aa831f54f2c5b3a7c79be27581c89e55f46",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "7745bc75532483aa730d7a2610297a3fb0e9b37e29d53b278f3f1ea637b22b07"
  },
  "claim": "formal/tlaplus/casper_soak/README.md",
  "claim_anchor": "documentation-contract",
  "claim_ids": ["CLAIM-CASPER-SOAK-FORMAL-AREA-DOCS"],
  "claim_digests": {
    "formal/tlaplus/casper_soak/README.md": "7745bc75532483aa730d7a2610297a3fb0e9b37e29d53b278f3f1ea637b22b07"
  },
  "adapter": "embedded",
  "status": "pending",
  "pending_reason": "Claim001 requires renewal after the approved formal-gate workflow change.",
  "scope": "formal-area-documentation-and-plan-consistency-only",
  "evidence": {
    "kind": "bounded-controls-and-documentation-consistency",
    "ref": "docs/casper/cbc-evidence/runs/casper-formal-area-records-20260919-01/report.json",
    "sha256": "e30a9f39bac88560a4a9671436250ff4fc2d98c1d7cf709acfad514d3c927c33"
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
  "verified_at": "2026-09-19T21:09:36Z",
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/formal-tlaplus-casper-soak-README-md.md",
    "commit": "1f749aa831f54f2c5b3a7c79be27581c89e55f46",
    "sha256": "e56cb71dfdbf3c29509fba01550134fbd73fa2577135b8a846c926ae7483da5d"
  },
  "inventory_scope": "Separate documentation contract. The eight executable claim inventories remain unchanged."
}
```
