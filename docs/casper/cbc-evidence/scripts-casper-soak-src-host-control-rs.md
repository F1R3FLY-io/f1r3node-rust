# CbC Evidence: scripts/casper-soak/src/host_control.rs

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
  },
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-CASPER-SOAK-001"
}
```


## Claim CLAIM-SOAK-CLEANUP-001

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/host_control.rs",
    "commit": "99545bf7c3a3a63c651cf1a06a36018322094ac0",
    "commit_is_base": true,
    "working_tree": true
  },
  "claim": "# Soak Segment Cleanup Contract\n\n```yaml\nclaim_id: CLAIM-SOAK-CLEANUP-001\nstatus: pending\nscope: bounded-harness-cleanup\nconstruction: not-applicable\nrefutation: pending\nbinding: pending\nacceptance: required\nsoak: pending\n```\n\n## Required behavior\n\nA completed segment stops readable owned writers before its owner marker changes.\nA completed segment also stops visible same-user writers whose arguments or working directory name its subprocess data directory.\nClearing the pytest PID does not remove either cleanup obligation.\n\nA completed segment also kills each Docker container that carries its owner label, directly after an inspect confirms that label.\nA Docker container that `docker ps` no longer lists as running counts as stopped.\nA container that stays listed as running after its kill is an unconfirmed stop.\nAn unrelated process outside both ownership scopes remains alive.\n\nAn unconfirmed stop writes writer-stop-failure.txt and makes the saved failure count at least one.\nA repeated unconfirmed stop preserves a higher failure count.\nA later segment preserves the earlier stop reason and cannot erase failure information.\nEvidence capture precedes any deletion of evidence sources.\n\nLaunch requires current verification and independent signed acceptance for the actual control sources.\nCandidate node sources cannot establish acceptance for control sources.\nA source, claim, or evidence change invalidates the corresponding acceptance.\nMissing policy, records, trust configuration, or signatures prevents launch.\n\nThe launcher uses the pinned shared CbC runtime from an external cache.\nMissing or modified runtime files prevent verification and launch.\nThe runtime lockfile records an immutable revision and the expected source hashes.\nVerification binds that lockfile and the validated runtime identity into the evidence.\nRepository source contains no shared-runtime copy.\n\n## Bounds and assumptions\n\nThe bounded model covers two segments, one owned writer, one residual writer, and one unrelated process.\nThe model does not separate subprocess writers from Docker writers.\nThe writer stop cases of `scripts/bench/test-run-merge-recovery-soak.sh` cover the Docker path.\nThese cases are slow, vanish, exits, stuck, breach, and breach-inspect.\nThe verification runs that fixture as its fourth check.\n\nThe launch gate covers the soak control path.\nThis path is the driver, `host_control.rs`, the soak workflow, the segment action, the claims, the model, the soak tests, and the gate scripts.\nThe gate does not cover `.github/workflows/ci.yml` or the `.gitattributes` files.\nA change to those files therefore does not stop a soak.\n\nThe verification binds the dependency closure of the harness, not the workspace `Cargo.lock`.\n`scripts/ci/soak-cleanup-lock-closure.sh` computes this closure from `Cargo.lock`.\nIts roots are the non-optional dependencies of `scripts/casper-soak/Cargo.toml`.\nA dependency change outside the closure therefore does not stop a soak.\n\nThe closure digest also binds the workspace `Cargo.toml` parts that reach the harness build.\nThese parts are the inherited `[workspace.dependencies]` entries of the harness, and the `[patch.*]` and `[profile.*]` tables.\nOther workspace members and their dependencies stay unbound.\nThe walk follows every edge that `Cargo.lock` records, also optional edges that the harness features do not activate.\nA change to such a package therefore stops a soak until a new verification and acceptance exist.\n\nEach segment can end after a deadline, a checkpoint, finalization, or a protection breach.\nThe Linux fixture uses pidfds and runs as an unprivileged user in an isolated container.\nThe host-control implementation can inspect the claimed processes under the stated visibility assumptions.\nThe kernel schedules the cleanup process and delivers supported signals within the configured budgets.\n\nAn owned process with an unreadable environment can escape the owner scan.\nA residual process with unreadable arguments and working directory can escape direct termination.\nThe two-second write check cannot detect every intermittent writer.\nAfter a protection breach, the residual stop skips that write check to preserve the emergency deadline.\nThe owner label of a container that exits before its inspect is not confirmed.\nThese accepted limits remain explicit and do not establish complete host containment.\n\n## Evidence and correction\n\nThe verification runner records source hashes before it runs tests.\nThe report records expected failures for the permanent negative controls.\nEach artifact ledger binds this specification and the report to the current source bytes.\nA passing fixture is verification evidence, not maintainer acceptance.\nThe separate acceptance record authenticates the source, claim, and evidence bindings.\n\nThe reusable artifacts are the bounded model, two-segment fixture, negative controls, and launch gate.\n\n## Current verification package\n\nThe [verification report](../casper/cbc-evidence/runs/soak-cleanup-20261027-02/report.json) records the checked source bytes and outcomes.\nThe [status report](../casper/cbc-evidence/runs/soak-cleanup-20261027-02/status.json) records verification-time acceptance and independent claim gaps.\nThe earlier packages `soak-cleanup-20261025-01`, `soak-cleanup-20261026-01`, and `soak-cleanup-20261027-01` stay as history.\nThe packages `soak-cleanup-20261026-01` and `soak-cleanup-20261027-01` did not bind the workspace `Cargo.toml` parts of the harness build.\nUnsigned payloads in that package require independent maintainer review and signatures.\n",
  "adapter": "bounded-model-and-linux-fixture",
  "status": "discharged",
  "scope": "bounded-segment-cleanup",
  "evidence": {
    "kind": "bounded-model-and-linux-fixture",
    "ref": "docs/casper/cbc-evidence/runs/soak-cleanup-20261027-02/report.json"
  },
  "waiver": null,
  "verified_at": "2026-10-27T06:04:11Z",
  "claim_id": "CLAIM-SOAK-CLEANUP-001",
  "schema": "cbc-claim/v1",
  "verification_id": "b5f709df-2cfe-4ba0-bd1a-cdcad14cbbf1",
  "inputs": {
    "artifact": "scripts/casper-soak/src/host_control.rs",
    "source_sha256": "427e0a7a550156cfc821e37df4fe452a03aae3b9ed8807bd897be0c80b76e6be",
    "claim": {
      "path": "docs/claims/soak-segment-cleanup.md",
      "sha256": "9fe7a53590d2fb82378b7630e099442a5ee1477930367925d0e8f9d6e4e25c1a"
    }
  },
  "evidence_files": {
    "docs/casper/cbc-evidence/runs/soak-cleanup-20261027-02/report.json": "e57297ba6eafe30a2d07eff512824fb06a2bd393eec635c61ef29159e30f6db7"
  }
}
```
