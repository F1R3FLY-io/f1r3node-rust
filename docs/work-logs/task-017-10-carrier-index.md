# TASK-017-10: Carrier Index Profile

## Ownership

- Task: TASK-017-10.
- Epic: EPIC-017.
- Implementer: `pi-soak-carrier-index-linux`.
- Started: 2026-09-19T06:11:36Z.
- Branch: `formal/soak-casper-consensus`.
- Starting revision: `442e93faa`.
- Claim: `CLAIM-CASPER-SOAK-008`.

## Scope

This task implements and verifies the carrier-index harness profile. It does not change node behavior or discharge `CLAIM-FINALITY-002`.

The profile must compare matched candidate, DAG, scan-window, and availability inputs. Missing path receipts and counters must remain unknown.

Unavailable node interfaces block affected live scenarios. Controlled transcripts do not supply node evidence or authorize a soak run.

## Plan

- [x] Record and publish task ownership before implementation.
- [ ] Implement the profile generator, collector, classifier, and executable fixtures.
- [ ] Run bounded model controls and matching implementation tests.
- [ ] Record source-specific evidence and remaining interface limits.
- [ ] Obtain the required binding acceptance before task completion.

## Coordination

The remote machine owns TASK-017-8. The proposed assignment for TASK-017-11 remains unconfirmed and unclaimed.

Profile-specific files belong to this task. Shared inventory and workflow changes require coordination before editing.

Exchange published commit IDs between machines. Use fast-forward-only pulls from clean checkpoints. Each new commit requires separate user approval.

## Current state

The initial profile module, command-line binary, and 18 test functions now exist. This is an implementation checkpoint, not task completion.

The profile checks paired inputs, path engagement, counters, artifact identities, fault receipts, and restart links. Live, post-merge, and typed-identity execution remain blocked.

The first compile found an unsupported `Result` method. The replacement compiled successfully. The subsequent fixture run exceeded its 180-second tool limit before the final test completed.

That interrupted run is not a passing test result. Its temporary log is `/tmp/carrier-index-checks/first-fixtures.log`.

The commit hook rejected unformatted files. Targeted formatting corrected the three new Rust files without changing shared sources.

The bounded model, model configurations, final verification, claim inventory, and evidence records remain unfinished. The `models` command cannot run until those model files exist.

The correctness claim remains pending. No node campaign or claim discharge has occurred.
