# CbC Evidence: scripts/ci/verify-soak-cleanup.sh


## Claim CLAIM-SOAK-CLEANUP-001

```json
{
  "artifact": {
    "path": "scripts/ci/verify-soak-cleanup.sh",
    "commit": "1b16819729e5aef31e7632d5b4d450fcd53464d2",
    "commit_is_base": true,
    "working_tree": true
  },
  "claim": "# Soak Segment Cleanup Contract\n\n```yaml\nclaim_id: CLAIM-SOAK-CLEANUP-001\nstatus: pending\nscope: bounded-harness-cleanup\nconstruction: not-applicable\nrefutation: pending\nbinding: pending\nacceptance: required\nsoak: pending\n```\n\n## Required behavior\n\nA completed segment stops readable owned writers before its owner marker changes.\nA completed segment also stops visible same-user writers whose arguments or working directory name its subprocess data directory.\nClearing the pytest PID does not remove either cleanup obligation.\nAn unrelated process outside both ownership scopes remains alive.\n\nAn unconfirmed stop writes writer-stop-failure.txt and makes the saved failure count at least one.\nA repeated unconfirmed stop preserves a higher failure count.\nA later segment preserves the earlier stop reason and cannot erase failure information.\nEvidence capture precedes any deletion of evidence sources.\n\nLaunch requires current verification and independent signed acceptance for the actual control sources.\nCandidate node sources cannot establish acceptance for control sources.\nA source, claim, or evidence change invalidates the corresponding acceptance.\nMissing policy, records, trust configuration, or signatures prevents launch.\n\n## Bounds and assumptions\n\nThe bounded model covers two segments, one owned writer, one residual writer, and one unrelated process.\nEach segment can end after a deadline, a checkpoint, finalization, or a protection breach.\nThe Linux fixture uses pidfds and runs as an unprivileged user in an isolated container.\nThe host-control implementation can inspect the claimed processes under the stated visibility assumptions.\nThe kernel schedules the cleanup process and delivers supported signals within the configured budgets.\n\nAn owned process with an unreadable environment can escape the owner scan.\nA residual process with unreadable arguments and working directory can escape direct termination.\nThe two-second write check cannot detect every intermittent writer.\nAfter a protection breach, the residual stop skips that write check to preserve the emergency deadline.\nThese accepted limits remain explicit and do not establish complete host containment.\n\n## Evidence and correction\n\nThe verification runner records source hashes before it runs tests.\nThe report records expected failures for the permanent negative controls.\nEach artifact ledger binds this specification and the report to the current source bytes.\nA passing fixture is verification evidence, not maintainer acceptance.\nThe separate acceptance record authenticates the source, claim, and evidence bindings.\n\nThe reusable artifacts are the bounded model, two-segment fixture, negative controls, and launch gate.\n\n## Current verification package\n\nThe [verification report](../casper/cbc-evidence/runs/soak-cleanup-20261025-01/report.json) records the checked source bytes and outcomes.\nThe [status report](../casper/cbc-evidence/runs/soak-cleanup-20261025-01/status.json) records verification-time acceptance and independent claim gaps.\nUnsigned payloads in that package require independent maintainer review and signatures.\n",
  "adapter": "bounded-model-and-linux-fixture",
  "status": "discharged",
  "scope": "bounded-segment-cleanup",
  "evidence": {
    "kind": "bounded-model-and-linux-fixture",
    "ref": "docs/casper/cbc-evidence/runs/soak-cleanup-20261025-01/report.json"
  },
  "waiver": null,
  "verified_at": "2026-10-25T19:06:15Z",
  "claim_id": "CLAIM-SOAK-CLEANUP-001",
  "schema": "cbc-claim/v1",
  "verification_id": "1da20f04-e7c1-4dd9-bc69-8c93511aa540",
  "inputs": {
    "artifact": "scripts/ci/verify-soak-cleanup.sh",
    "source_sha256": "1aad730d9259bf776f6a211b98855c1ebff91e30a79d48dce066e009e1c98415",
    "claim": {
      "path": "docs/claims/soak-segment-cleanup.md",
      "sha256": "edd3ab8b4429376dca9e27b165a7791c67a0a405003a1e1e9ffe22c260b1a0ff"
    }
  },
  "evidence_files": {
    "docs/casper/cbc-evidence/runs/soak-cleanup-20261025-01/report.json": "75e758dbc0a5927c0d4e59c5708e4c7a9bd80a64feab5b369708e2e12786e526"
  }
}
```
