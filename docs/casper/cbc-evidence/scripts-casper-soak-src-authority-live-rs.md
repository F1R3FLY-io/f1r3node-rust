# CbC Evidence: scripts/casper-soak/src/authority_live.rs

Controlled verification does not qualify a node candidate. Source-bound acceptance remains pending.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/authority_live.rs",
    "id": "scripts-casper-soak-src-authority-live-rs",
    "commit": "25ddb7779f9e0342bd0ca712804fc530bf6464a9",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "b1b83e9cd5a862fbbd55d71b8ac258c1507d63fc34eea1c0564454b0c2ddd005",
    "sha256_before_continuation": "efd1ed4d2e868889bf56070e8c0d203d62bf84e2813981222c4244452f5609c2",
    "sha256_before_parameter_rename": "e63d620db4586c96b48b8ed6b643bedfcefb372c64275ccb4ad90c6fc9f3b125",
    "parameter_rename": "2026-10-03: the capture parameter nonce became request_key, so CodeQL rust/hardcoded-crypto-value no longer matches a value that only derives the request correlation identifier. Behavior is unchanged. The record status stays pending."
  },
  "claim": "docs/claims/casper-authority-live-executor.md",
  "claim_ids": [
    "CLAIM-CASPER-AUTHORITY-LIVE-001"
  ],
  "claim_digests": {
    "docs/claims/casper-authority-live-executor.md": "71894bfb29abab60eae0f81432a4c4aa1fa3bdb83a730830f28b2cca91da5d5e"
  },
  "adapter": null,
  "status": "pending",
  "scope": "harness-live-authority-executor",
  "evidence": {
    "kind": "controlled-node-interface-verification-not-acceptance",
    "ref": "docs/casper/cbc-evidence/runs/casper-node-interface-adapter-20261001-01/report.json",
    "sha256": "9ef54687272869f604b9cf66bff9a5c5ae80efff7c653f8a05136bd094ea4dde"
  },
  "tiers": {
    "refutation": "pending",
    "construction": "pending",
    "binding": "pending"
  },
  "phase_status": {
    "pre_pr216_merge": "pending",
    "post_pr216_merge": "blocked"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": null,
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/runs/casper-authority-provider-20260928-01/previous-records/scripts-casper-soak-src-authority-live-rs.md",
    "sha256": "b4e20ed820818ed9fb02790ac370cf48122382402ba96590044b8999d1fc8a80",
    "commit": "63b9ace2b2c300b9dc605903d12bbb767566ae4b"
  },
  "continuation_previous_record": {
    "path": "docs/casper/cbc-evidence/scripts-casper-soak-src-authority-live-rs.md",
    "commit": "25ddb7779f9e0342bd0ca712804fc530bf6464a9",
    "sha256": "1bb8da559a5cc19240cdffbc6522131f9997d119313a5d559729c178b1b41b26"
  },
  "claim_digests_before_continuation": {
    "docs/claims/casper-authority-live-executor.md": "925c1cbb2b99215d17a0223ab7c8bdb3554fe6549309021e0f99b5526ca84d33"
  },
  "evidence_before_continuation": {
    "kind": "controlled-provider-verification-not-acceptance",
    "ref": "docs/casper/cbc-evidence/runs/casper-authority-provider-20260928-01/report.json",
    "sha256": "d85f9d4ce21cc13205e44f0ef94ab6ead8ca2a192a094febb128198135810470"
  },
  "continuation_registration": {
    "scope": "batch-d-e-harness-mapping",
    "date": "2026-10-01",
    "before_implementation": true,
    "ref": "docs/work-logs/task-017-12-node-interface-20261001.md"
  },
  "verification_observation": {
    "ref": "docs/casper/cbc-evidence/runs/casper-node-interface-adapter-20261001-01/report.json",
    "selected_tests": 47,
    "linux_selected_tests": 35,
    "clippy": "passed",
    "adapter_discharge": false,
    "live_qualification": false
  }
}
```
