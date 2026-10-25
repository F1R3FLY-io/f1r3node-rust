# CbC Evidence: scripts/run-merge-recovery-soak.sh

```json
{
  "artifact": {
    "path": "scripts/run-merge-recovery-soak.sh",
    "id": "scripts-run-merge-recovery-soak-sh",
    "commit": "f7a2696bd91224ac237f52664b9df406d8420c98",
    "commit_is_base": false,
    "sha256": "431094a0d64c5d2923474948e22112ef27292889b4c91d08bb7343d2be9fd12e",
    "working_tree": true,
    "sha256_before_change": "8486972f4b7ea2c2ba3433d0dc1c3b2cdd1efa58de01feebbe7e6726548b2758"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "670e2867bf40fb941a7e762b8624550c0feb8cdc45fe7efcfc7b69f3cc0cabc8"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-harness-only",
  "evidence": {
    "kind": "accepted-change",
    "ref": "docs/casper/cbc-evidence/runs/casper-soak-deadline-shard-stop-acceptance-20261023-01/report.json",
    "sha256": "c24f610020098e953c6a6ca9194b062fb87c9ccf97b8eb1604dab9c872cde76a"
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
  "verified_at": "2026-10-23T10:45:54Z",
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/previous-metadata.tar.gz",
    "sha256": "855516a6685384190c8a4f427ff547fc2c8a0f2b6d894849d26dea9fde91b6d9",
    "member": "prior/ledgers/scripts-run-merge-recovery-soak-sh.md",
    "member_sha256": "168a029dd86166e24d1fce93e366a47d3ab9fb7127b23fced7fc6e0652ecf0ff"
  },
  "review_candidate": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/candidate-ledgers/scripts-run-merge-recovery-soak-sh.md",
  "evidence_before_change": {
    "kind": "accepted-change",
    "ref": "docs/casper/cbc-evidence/runs/casper-soak-writer-stop-acceptance-20261021-01/report.json",
    "sha256": "982fc077d124845f51132e013284af25a846e84fca7f3ff0a783a99699434756"
  },
  "acceptance": {
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/827#pullrequestreview-5627170022",
    "revision": "c1cf31d7ed9224ccb83ebdd8de9d9ea8cd9b9c88",
    "reviewed_at": "2026-10-23T10:45:54Z"
  },
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-CASPER-SOAK-001"
}
```


## Claim CLAIM-SOAK-CLEANUP-001

```json
{
  "artifact": {
    "path": "scripts/run-merge-recovery-soak.sh",
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
  "verified_at": "2026-10-25T19:55:45Z",
  "claim_id": "CLAIM-SOAK-CLEANUP-001",
  "schema": "cbc-claim/v1",
  "verification_id": "1e4576be-eeb7-4352-b07a-eaa590325cf7",
  "inputs": {
    "artifact": "scripts/run-merge-recovery-soak.sh",
    "source_sha256": "431094a0d64c5d2923474948e22112ef27292889b4c91d08bb7343d2be9fd12e",
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
