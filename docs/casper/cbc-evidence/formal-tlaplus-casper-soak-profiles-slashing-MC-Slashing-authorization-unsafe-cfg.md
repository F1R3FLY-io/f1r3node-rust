# CbC Evidence: formal/tlaplus/casper_soak/profiles/slashing/MC_Slashing_authorization_unsafe.cfg

The user accepted this bounded pre-merge binding. Workflow-tag ratification and live execution remain outside this discharge.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/profiles/slashing/MC_Slashing_authorization_unsafe.cfg",
    "id": "formal-tlaplus-casper-soak-profiles-slashing-MC-Slashing-authorization-unsafe-cfg",
    "commit": "137b74fdb903969d186aef9241380eef24dd4833",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "938955b64080a1049dd7d57da178228dec0f4ac4779557fb4415af8c8f14044f"
  },
  "claim": "docs/claims/casper-soak-slashing.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-006"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-slashing.md": "79e37bb2c481d95d9a2548f41e44bfc6e642a866263c665e6cef63fdcfd68a48"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-slashing-profile",
  "evidence": {
    "kind": "accepted-bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-slashing-acceptance-20260919-01/report.json",
    "sha256": "3840449eb7f63f4b0fa7ec853c37a6fe9f91195d83e30d9b156898b4f9597871"
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
  "verified_at": "2026-09-19T16:00:28Z",
  "pending": [
    "workflow-tag-ratification",
    "external-evidence-publication"
  ],
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/runs/casper-slashing-acceptance-20260919-01/previous-ledgers.tar.gz",
    "sha256": "a112156258c8407d2b5b498c3cc338085e706f9c743df29abb7eb5aefecb3189",
    "member": "formal-tlaplus-casper-soak-profiles-slashing-MC-Slashing-authorization-unsafe-cfg.md",
    "member_sha256": "05406182edd0c33d877646b07a361957163721a36db2e5b9744dd0e0bc3d3ee6"
  }
}

```
