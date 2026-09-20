# CbC Evidence: scripts/casper-soak/tests/campaign_reservation.rs

The local reservation claim remains pending. Fixture checks do not authorize campaign execution.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/tests/campaign_reservation.rs",
    "id": "scripts-casper-soak-tests-campaign-reservation-rs",
    "commit": "9dd92a007b74047a03fa8b96135d6599e7575faf",
    "commit_is_base": true,
    "sha256": "fa3c6b293c79bf19ee1c5ae30a149d8de0370e35158cfeec38f44665a2a5d810",
    "working_tree": true
  },
  "claim": "docs/claims/casper-campaign-reservation.md",
  "claim_ids": [
    "CLAIM-CASPER-CAMPAIGN-002"
  ],
  "claim_digests": {
    "docs/claims/casper-campaign-reservation.md": "4b5f7f23e8bda071056d83ab5878171214ebeccfe465e9aa88b8881eb30f9c0a"
  },
  "adapter": null,
  "status": "pending",
  "scope": "local-baseline-reservation-store",
  "evidence": {
    "kind": "synthetic-fixtures-not-formal-discharge",
    "ref": "docs/casper/cbc-evidence/runs/casper-campaign-reservation-9dd92a007-01/report.json",
    "sha256": "d37b3c7cc0e97aae13bd9b088e0d93d2672bdb53ff7e76b3b592bd43c5f134e4"
  },
  "tiers": {
    "refutation": "pending",
    "construction": "not-applicable",
    "binding": "pending"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": null
}
```
