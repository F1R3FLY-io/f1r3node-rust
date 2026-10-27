# CbC Evidence: scripts/casper-soak/src/host_control.rs

The user authorized this bounded harness renewal after fresh hosted verification of the formal-gate workflow. Node execution and protection-rule activation remain outside this discharge.

The previous ledger remains in the archive below. Earlier reports retain their execution identities and limitations.

The maintainer accepted the PR #811 owner scan change of this file, its narrower ownership guarantee, and its limitation on 2026-10-21 (PR #811, comment 6324960804) at revision `32798242d`. The owner scan skips a same-user process when its environment read fails with EACCES or EPERM. An owned process with an unreadable environment escapes cleanup and the OOM preference.

The maintainer accepted the TASK-021-27 `stop-under` action of this file, its write check, and its limitations on 2026-10-23 (PR #827, review 5627170022) at revision `c1cf31d7e`.

The maintainer accepted the PR #863 skip report of `stop-owned` and `prefer-oom` in this file on 2026-10-26 (PR #870, comment 6429833007) at revision `6dd029064`.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/host_control.rs",
    "id": "scripts-casper-soak-src-host-control-rs",
    "commit": "85b04c07e512a09d5fc87239294ff111f4e88563",
    "commit_is_base": false,
    "sha256": "427e0a7a550156cfc821e37df4fe452a03aae3b9ed8807bd897be0c80b76e6be",
    "working_tree": true,
    "sha256_before_change": "31c99688f2475b8d9756cd6fdd2f49af54981e50a07160c8a3f479ad8e80cfed"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "92c82d3eb42331aa2fda2e6c53dd4b74dd3a1cd166855dd0c2db4b38cdbbda2c"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-harness-only",
  "evidence": {
    "kind": "accepted-change",
    "ref": "docs/casper/cbc-evidence/runs/casper-soak-writer-stop-race-acceptance-20261026-01/report.json",
    "sha256": "a0d27b8e73a7e9b798639203ad84e00739daa94d9e6e8b5f1374a2e3e1f82c8f"
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
  "verified_at": "2026-10-26T23:32:01Z",
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/previous-metadata.tar.gz",
    "sha256": "855516a6685384190c8a4f427ff547fc2c8a0f2b6d894849d26dea9fde91b6d9",
    "member": "prior/ledgers/scripts-casper-soak-src-host-control-rs.md",
    "member_sha256": "972e86d5b951fa278a783a532d06997380cbf5f09783b057c75e7191dcd49158"
  },
  "review_candidate": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/candidate-ledgers/scripts-casper-soak-src-host-control-rs.md",
  "refresh": {
    "accepted_commit": "f9273621c8887947b56d0093a71486338312138e",
    "accepted_sha256": "fb6dafc47a5df2073c4ca618ddf81c4a292229b20e5b08a1f0cfeae306e185bb",
    "production_region_identical": true,
    "reason": "The execute function moved above the test module to satisfy clippy. The production region is byte-identical."
  },
  "evidence_before_change": {
    "kind": "accepted-change",
    "ref": "docs/casper/cbc-evidence/runs/casper-soak-deadline-shard-stop-acceptance-20261023-01/report.json",
    "sha256": "c24f610020098e953c6a6ca9194b062fb87c9ccf97b8eb1604dab9c872cde76a"
  },
  "acceptance": {
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/870#issuecomment-6429833007",
    "revision": "6dd0290647f420567b958c7eb783ca9806383bb5",
    "reviewed_at": "2026-10-26T23:32:01Z"
  }
}
```
