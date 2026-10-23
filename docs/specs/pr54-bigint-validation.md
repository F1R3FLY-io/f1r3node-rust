# PR #54 BigInt proposal validation

Branch: `proposal/pr54-bigint-staking-fees`.
Base: PR #54 head `604e1ea82b3b28ba4a12915c92fd2d85c1747a55`.
Purpose: publish the proposed predicate, BigInt staking, and fee safeguards for team review.
The user authorized a new branch, one commit, and push to the existing team repository.

## Scope and compatibility

The proposal adds `isBigInt()`, exact staking amounts, canonical large-stake encoding, metadata schema protection, and fee arithmetic/replay checks.
The protocol constant remains 1, as instructed. No activation mechanism or migration has been approved.
This branch is not suitable for deployment into an existing or mixed-version network.
An unchanged version number does not make the new execution, staking, hashing, storage, or validation rules compatible with old nodes.
The team must decide the protocol and state-transition strategy before rollout.

## Current local validation

These checks ran on the restored proposal on 2026-10-23. Earlier archived results are historical evidence only.

| Check | Result |
|---|---|
| Models unit tests, including canonical bond encoding and hashing | 161 passed |
| Rholang numeric evaluation and predicate tests | 28 passed |
| Casper unit tests | 670 passed, one known WAL test excluded |
| Node configuration tests | 149 passed |
| Node API tests | 95 passed |
| Block storage unit, DAG, and snapshot tests | 103 + 42 + 36 passed |
| Direct/API rejection of invalid fees before storage | Passed |
| Proposer exclusion of stored deploys with invalid fees | Passed |
| Honest failed-precharge replay and rejection of forged cost/status | Passed |
| Large-stake proposal, peer transport, replay, hashes, and metadata | Passed |
| Delegation contract: 100 ASI bond, 40 ASI delegation, 10 ASI withdrawal | Passed |
| Delegation scalability contract | Passed |
| Valid genesis candidate approval | Passed |
| Affected node, Casper, models, storage, and interpreter test compilation | Passed |
| Non-slashable fee verdict regression | 2 passed |
| Soak mapping regressions | 16 passed |
| Soak profile integration tests | 13 passed, one ignored subprocess fixture |
| Exact profile threshold arithmetic, 2,000 small cases plus large products | Passed |
| BigInt live-driver fixture comparison and malformed/mismatched rejection | 2 passed |
| Soak harness, all targets and features | Compilation passed |
| Source-bound B11 snapshot proof | 8 closed theorems, 75 production cases, 6 rejection controls passed |

The unit run excludes `rust::engine::wal_payload_sync::tests::enumerate_reducer_wrong_bytes_falls_back_to_fetch`.
Its unchanged base implementation fails a debug assertion before the test reaches its expected fallback.
This report does not claim that the full workspace test suite passes.

The first concurrent harness run passed the new decimal-stake tests but failed two existing process-restart tests.
The serial harness run had 12 passing tests, three failures, and two ignored process fixtures.
All three failing test names also fail on an untouched archive of PR #54:

- `linux::deadlines_preserve_partial_inventory_and_stop_the_driver`
- `linux::live_restart_enrolls_a_successor_from_the_verified_capture`
- `linux::live_restart_requires_exit_and_the_pinned_successor_capture`

The deadline test expects a PID file that does not exist in this environment.
Restart runs report process-owner rejection or a capture deadline. These are baseline limitations, not passing checks.
The final mapping, profile, and targeted decimal-stake live-driver tests pass. The final source-bound B11 verification passed.

## Independent review

The required Opus review found a decimal-string compatibility defect in the soak harness.
The fix parses monetary witnesses, profile inputs, and display weights with BigInt.
It checks fixture values without narrowing or silently skipping failures.
Regression tests cover values above i128, strict threshold equality, one-unit differences, malformed values, and fixture mismatches.
The fix preserves explicit null values for valid oracle early returns.
Profile-level tests cover request validation and fixture expectations above u64 and i128, including one-unit threshold boundaries.

The review also corrected verdict coverage, misplaced test documentation, and fork-choice test descriptions.
The design now records the remaining team decisions about default limits, stored fault-tolerance bits, metadata format, and wire limits.
The existing slashing models use a historical taxonomy. Documentation and regression headers now state this limit explicitly.
The new verdict has production regression coverage but no matching historical-model proof.
Opus reviewed the proposal and two follow-up corrections. The final review found no remaining actionable defects in those corrections.

The separate slashing fixture profile retains its u64 stake domain. This proposal does not claim BigInt coverage for that profile.

## Delivery boundary

No VPS deployment, image replacement, or live state changes are part of this delivery.
Local tests demonstrate proposal behavior with fresh state. They do not establish deployment compatibility or a completed live economic-pipeline test.

## Reproduction

Set `CARGO_BUILD_JOBS=8` and `CARGO_PROFILE_DEV_DEBUG=0` for the commands below.

```sh
cargo test -p models --lib
cargo test -p rholang --test rholang_numeric_eval_spec
cargo test -p casper --lib -- --skip rust::engine::wal_payload_sync::tests::enumerate_reducer_wrong_bytes_falls_back_to_fetch
cargo test -p node --lib configuration::
cargo test -p node --lib api::
cargo test -p block-storage --lib --test block_dag_storage_test --test soak_snapshot
cargo test -p casper --test mod fee_overflow_is_rejected_before_deploy_storage
cargo test -p casper --test mod invalid_fee_metadata_in_storage_does_not_block_valid_proposals
cargo test -p casper --test mod replay_rejects_forged_cost_and_success_after_precharge_failure
cargo test -p casper --test mod large_stakes_survive_proposal_peer_replay_and_metadata
cargo test -p casper --test mod pos_delegation_review_spec
cargo test -p casper --test mod pos_delegation_scalability_spec
cargo test -p casper --test mod block_approver_protocol_should_successfully_validate_correct_candidate
cargo test -p casper --test mod slashing::prop_t_3_slashable_taxonomy
cargo test -p casper-soak --bin casper-authority-finality --test authority_finality --test authority_mapping
cargo test -p casper-soak --test authority_live
cargo check -p casper-soak --all-targets --all-features
```

The complete harness command includes the unrelated timing and restart failures recorded above.
Run the B11 `export` and `verify` commands from [the canonical-wire verification guide](../../formal/rocq/node_observation/b11/README.md).
The final B11 receipt binds the exported cases to the tested Rust source and dependency manifests.
