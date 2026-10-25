# CbC Evidence: .github/actions/soak-segment/action.yml


## Claim CLAIM-SOAK-CLEANUP-001

```json
{
  "artifact": {
    "path": ".github/actions/soak-segment/action.yml",
    "commit": "c894695f42923f2d1b8ba2e24d8b41946b71750b",
    "commit_is_base": true,
    "working_tree": true
  },
  "claim": "# Soak Segment Cleanup Contract\n\n```yaml\nclaim_id: CLAIM-SOAK-CLEANUP-001\nstatus: pending\nscope: bounded-harness-cleanup\nconstruction: not-applicable\nrefutation: pending\nbinding: pending\nacceptance: required\nsoak: pending\n```\n\n## Required behavior\n\nA completed segment stops readable owned writers before its owner marker changes.\nA completed segment also stops visible same-user writers whose arguments or working directory name its subprocess data directory.\nClearing the pytest PID does not remove either cleanup obligation.\nAn unrelated process outside both ownership scopes remains alive.\n\nAn unconfirmed stop writes writer-stop-failure.txt and makes the saved failure count at least one.\nA repeated unconfirmed stop preserves a higher failure count.\nA later segment preserves the earlier stop reason and cannot erase failure information.\nEvidence capture precedes any deletion of evidence sources.\n\nLaunch requires current verification and independent signed acceptance for the actual control sources.\nCandidate node sources cannot establish acceptance for control sources.\nA source, claim, or evidence change invalidates the corresponding acceptance.\nMissing policy, records, trust configuration, or signatures prevents launch.\n\nThe launcher uses the pinned shared CbC runtime from an external cache.\nMissing or modified runtime files prevent verification and launch.\nThe runtime lockfile records an immutable revision and the expected source hashes.\nVerification binds that lockfile and the validated runtime identity into the evidence.\nRepository source contains no shared-runtime copy.\n\n## Bounds and assumptions\n\nThe bounded model covers two segments, one owned writer, one residual writer, and one unrelated process.\nEach segment can end after a deadline, a checkpoint, finalization, or a protection breach.\nThe Linux fixture uses pidfds and runs as an unprivileged user in an isolated container.\nThe host-control implementation can inspect the claimed processes under the stated visibility assumptions.\nThe kernel schedules the cleanup process and delivers supported signals within the configured budgets.\n\nAn owned process with an unreadable environment can escape the owner scan.\nA residual process with unreadable arguments and working directory can escape direct termination.\nThe two-second write check cannot detect every intermittent writer.\nAfter a protection breach, the residual stop skips that write check to preserve the emergency deadline.\nThese accepted limits remain explicit and do not establish complete host containment.\n\n## Evidence and correction\n\nThe verification runner records source hashes before it runs tests.\nThe report records expected failures for the permanent negative controls.\nEach artifact ledger binds this specification and the report to the current source bytes.\nA passing fixture is verification evidence, not maintainer acceptance.\nThe separate acceptance record authenticates the source, claim, and evidence bindings.\n\nThe reusable artifacts are the bounded model, two-segment fixture, negative controls, and launch gate.\n\n## Current verification package\n\nThe [verification report](../casper/cbc-evidence/runs/soak-cleanup-20261025-01/report.json) records the checked source bytes and outcomes.\nThe [status report](../casper/cbc-evidence/runs/soak-cleanup-20261025-01/status.json) records verification-time acceptance and independent claim gaps.\nUnsigned payloads in that package require independent maintainer review and signatures.\n",
  "adapter": "bounded-model-and-linux-fixture",
  "status": "discharged",
  "scope": "bounded-segment-cleanup",
  "evidence": {
    "kind": "bounded-model-and-linux-fixture",
    "ref": "docs/casper/cbc-evidence/runs/soak-cleanup-20261025-01/report.json"
  },
  "waiver": null,
  "verified_at": "2026-10-25T19:55:21Z",
  "claim_id": "CLAIM-SOAK-CLEANUP-001",
  "schema": "cbc-claim/v1",
  "verification_id": "cc1bbbcc-f32b-4d1a-b8de-ec26c5e16ee9",
  "inputs": {
    "artifact": ".github/actions/soak-segment/action.yml",
    "source_sha256": "55d657083eeb8f8a1f793bae4c600b8823d0318780c83dfa983c064e89e8e2ef",
    "claim": {
      "path": "docs/claims/soak-segment-cleanup.md",
      "sha256": "76d21c461e55290ad6abca40ae18b5def403780ce497af5cc36ec0188452ea3a"
    }
  },
  "evidence_files": {
    "docs/casper/cbc-evidence/runs/soak-cleanup-20261025-01/report.json": "0ac45146933c7e3e19f8b95b0acdc4d3540ad6c5fcd4cdf92091ae5a34e2e0d3"
  }
}
```
