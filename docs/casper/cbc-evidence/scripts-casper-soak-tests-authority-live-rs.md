# CbC Evidence: scripts/casper-soak/tests/authority_live.rs

Controlled verification does not qualify a node candidate. Source-bound acceptance remains pending.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/tests/authority_live.rs",
    "id": "scripts-casper-soak-tests-authority-live-rs",
    "commit": "25ddb7779f9e0342bd0ca712804fc530bf6464a9",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "dca1c888fb35799626c413b833ce695dd908dfc2dd30c2d3df4556f4947e9476",
    "sha256_before_continuation": "9e1153f8b2c3249913794a2dcf443a67757376a7b1691f3daf7397a1240e5d92"
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
    "path": "docs/casper/cbc-evidence/runs/casper-authority-provider-20260928-01/previous-records/scripts-casper-soak-tests-authority-live-rs.md",
    "sha256": "f050fb8800b9c8437a53d03ebe2921eaf2033e61c683a5e1bb50840fa08507a9",
    "commit": "63b9ace2b2c300b9dc605903d12bbb767566ae4b"
  },
  "continuation_previous_record": {
    "path": "docs/casper/cbc-evidence/scripts-casper-soak-tests-authority-live-rs.md",
    "commit": "25ddb7779f9e0342bd0ca712804fc530bf6464a9",
    "sha256": "3295430dffcdea6ea2542476b9c7f1ca500b4c442a1e6433031edc756977b4d6"
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
