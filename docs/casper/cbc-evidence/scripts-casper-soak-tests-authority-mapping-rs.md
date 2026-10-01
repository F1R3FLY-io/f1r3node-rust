# CbC Evidence: scripts/casper-soak/tests/authority_mapping.rs

Controlled verification does not qualify a node candidate. Source-bound acceptance remains pending.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/tests/authority_mapping.rs",
    "id": "scripts-casper-soak-tests-authority-mapping-rs",
    "commit": "25ddb7779f9e0342bd0ca712804fc530bf6464a9",
    "commit_is_base": true,
    "sha256": "565dc792118f6c1932b3fe30212743b15be713fd7cb89741d56bb8879dd221f7",
    "working_tree": true,
    "sha256_before_continuation": "98a31c69949eae51f9913f65ea7700b128dcd5d3e3c437e72bbce098e9fdaa59"
  },
  "claim": "docs/claims/casper-authority-adapter.md",
  "claim_ids": [
    "CLAIM-CASPER-AUTHORITY-ADAPTER-001"
  ],
  "claim_digests": {
    "docs/claims/casper-authority-adapter.md": "09346ab99a44f3c9c3735b34b97dfe433330bdf9b67f1e3ffa8dd8f5a9390b19"
  },
  "adapter": null,
  "status": "pending",
  "scope": "harness-authority-observation-mapping",
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
  "previous_records": [
    {
      "artifact": {
        "path": "scripts/casper-soak/tests/authority_mapping.rs",
        "id": "scripts-casper-soak-tests-authority-mapping-rs",
        "commit": "66c21f26eeceac6027ad618cd96df07482a864c2",
        "commit_is_base": true,
        "sha256": "98a31c69949eae51f9913f65ea7700b128dcd5d3e3c437e72bbce098e9fdaa59",
        "working_tree": true
      },
      "claim": "docs/claims/casper-authority-adapter.md",
      "claim_ids": [
        "CLAIM-CASPER-AUTHORITY-ADAPTER-001"
      ],
      "claim_digests": {
        "docs/claims/casper-authority-adapter.md": "5d13e8d1bd319566fe682bd8e02a32d4b16992222203dfdb1184fc38d3f921b0"
      },
      "adapter": null,
      "status": "pending",
      "scope": "harness-authority-observation-mapping",
      "evidence": {
        "kind": "controlled-verification-not-acceptance",
        "ref": "docs/casper/cbc-evidence/runs/casper-authority-adapter-20260928-01/report.json",
        "sha256": "89cdb6c70c900c5a90dd971bb7840f5122d6146c392e894c0c558a14aa9b84d8"
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
      "verified_at": null
    }
  ],
  "continuation_previous_record": {
    "path": "docs/casper/cbc-evidence/scripts-casper-soak-tests-authority-mapping-rs.md",
    "commit": "25ddb7779f9e0342bd0ca712804fc530bf6464a9",
    "sha256": "47f7ef3b82b25fdbc0f02b97ffce72a502c96ae54b7683ac61870ac55babb9d9"
  },
  "claim_digests_before_continuation": {
    "docs/claims/casper-authority-adapter.md": "5d13e8d1bd319566fe682bd8e02a32d4b16992222203dfdb1184fc38d3f921b0"
  },
  "evidence_before_continuation": {
    "kind": "controlled-live-executor-and-adapter-regression-not-acceptance",
    "ref": "docs/casper/cbc-evidence/runs/casper-authority-live-20260928-01/report.json",
    "sha256": "206d7935f5837645e62165ef51702ad59ceddceb799ff8cf1ef0ccfd90548f4a"
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
