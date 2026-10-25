# CbC Evidence: scripts/bench/test-run-merge-recovery-soak.sh

```json
{
  "artifact": {
    "path": "scripts/bench/test-run-merge-recovery-soak.sh",
    "id": "scripts-bench-test-run-merge-recovery-soak-sh",
    "commit": "f7a2696bd91224ac237f52664b9df406d8420c98",
    "commit_is_base": false,
    "sha256": "d70933e3d37af22aa0a0cd819d909ee86cdc700a43e820ff95153d1babb23a60",
    "working_tree": true,
    "sha256_before_change": "3ff8305f3323e950c9a474b57dd24936de10461d6a5b6318cf129aef6ee953ba"
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
    "member": "prior/ledgers/scripts-bench-test-run-merge-recovery-soak-sh.md",
    "member_sha256": "0e1cbeb535106962fe68f61eb9a2b7b40779fa6d8665b3bfeff6b34d4961b46f"
  },
  "review_candidate": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/candidate-ledgers/scripts-bench-test-run-merge-recovery-soak-sh.md",
  "evidence_before_change": {
    "kind": "accepted-change",
    "ref": "docs/casper/cbc-evidence/runs/casper-soak-writer-stop-acceptance-20261021-02/report.json",
    "sha256": "7d7a5e5f87cbf56716e732c8b6a1ffbd6dc9ce46e47d6ff0a5c3e33da9489522"
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
    "path": "scripts/bench/test-run-merge-recovery-soak.sh",
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
  "verified_at": "2026-10-25T19:06:07Z",
  "claim_id": "CLAIM-SOAK-CLEANUP-001",
  "schema": "cbc-claim/v1",
  "verification_id": "514cd0e6-c64f-4e0b-b7ec-c497ee03ec47",
  "inputs": {
    "artifact": "scripts/bench/test-run-merge-recovery-soak.sh",
    "source_sha256": "d70933e3d37af22aa0a0cd819d909ee86cdc700a43e820ff95153d1babb23a60",
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
