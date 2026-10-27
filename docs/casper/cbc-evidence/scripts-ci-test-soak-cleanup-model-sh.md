# CbC Evidence: scripts/ci/test-soak-cleanup-model.sh


## Claim CLAIM-SOAK-CLEANUP-001

```json
{
  "artifact": {
    "path": "scripts/ci/test-soak-cleanup-model.sh",
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
  "verified_at": "2026-10-27T06:04:17Z",
  "claim_id": "CLAIM-SOAK-CLEANUP-001",
  "schema": "cbc-claim/v1",
  "verification_id": "f2fb4f90-2117-4c5c-92a9-696c752812a3",
  "inputs": {
    "artifact": "scripts/ci/test-soak-cleanup-model.sh",
    "source_sha256": "619b1d7ba46247e88b1d5c31ebf1797a2f47cc735eaad4a7a3a0e02bc5f3488b",
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
