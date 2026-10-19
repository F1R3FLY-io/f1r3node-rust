# H1 Implementation and Verification

**Task:** TASK-021-26.

**Owner:** pi-session-heartbeat-h1-cbc.

**Branch:** `fix/issue-24-deploy-heartbeat-wake`.

**Baseline:** `ed9b3c996`, the F1/F2 implementation.

**Status:** H1 safety acceptance is recorded for `0a4acca7a179d69e766e65672480390c02b45fe5`, with the limits below. The construction argument is reviewed, not mechanized. Performance qualification remains open.

## Implementation

The heartbeat owns one `ResponsiveMode` value for its task lifetime. Pending work observed at a check enters or refreshes the mode.

The mode expires after twice the configured check interval without another pending observation. Mode transitions do not replenish exemption credit.

Only the normal pending-deploy lane receives the exception after an empty own block. Due stale recovery and convergence retain priority.

The request consumes credit before the callback is awaited. Failures and busy responses therefore cannot replenish credit.

`ProposeRequest` carries the nonempty requirement and expected preceding own-block hash through the queue. Retries retain the same request value.

The proposer compares the preceding hash with its execution snapshot. A changed predecessor returns `NoNewDeploys` without block creation.

The proposer enforces `DeploySelection::Standard` for an exempt request. It also checks the actual created block before publication effects.

Failed user deploys count as user work. System-only and rejection-only blocks remain empty for H1 classification.

Routine cooldown checks include monotonic publication observations. A newly observed own block conservatively restores that clock after a lost result.

Three counters report mode entry, mode exit, and successful exempt publications. The phase-metric extension includes their exported names.

## Reviewed construction argument

The maintainer accepted this reviewed construction argument with its recorded assumptions. It is not a mechanized proof. The production queue and heartbeat loop must retain their sequential await behavior.

Let `C` be the positive configured cooldown. A rolling window is `(t - C, t]`.

The heartbeat records exemption credit at issuance. The next exempt issuance requires at least `C` elapsed time, regardless of the first request's result.

A normal routine request waits for the previous observed publication cooldown. Rounding fractional milliseconds upward cannot shorten that cooldown.

Consider three consecutive successful routine publications, `p1`, `p2`, and `p3`.

- If `p2` is normal, its publication follows `p1` by at least `C`.
- If `p3` is normal, its publication follows `p2` by at least `C`.
- Otherwise, `p2` and `p3` are exempt. Their request issuances are at least `C` apart.
- The sequential loop issues `p2` only after observing `p1`. Thus, `p3` cannot publish within the same rolling window as `p1`.

An exempt publication must contain user work. Two empty routine publications therefore require normal pacing and cannot occupy one rolling cooldown window.

Queue delay does not invalidate the argument when the loop awaits each result. The expected-block hash also prevents a queued exemption from following an intervening own block.

The argument depends on immutable block hashes, consistent snapshot observations, and publication effects preceding successful callback completion. Those existing platform obligations are not proved here.

The argument covers one live heartbeat task. It does not establish persistent credit across process restarts or a global bound on excluded proposal paths.

## Executable bindings

| Obligation        | Production binding                                                | Checks                                                                                            |
| ----------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| H1-1              | `ResponsiveMode::observe`                                         | Entry, refresh, expiry equality, configured intervals, and pending work at expiry.                |
| H1-2              | `can_exempt` and `decide_lanes`                                   | Pending-only exemption, lag refusal, and recovery priority.                                       |
| H1-3              | `issue_exemption`, publication observations, and awaited callback | Re-entry, failed requests, delayed publications, and bounded timed scenarios.                     |
| H1-4              | `ProposeRequest`, queue entries, and `Proposer::do_propose`       | Actual block classification, stale predecessor, no empty publication, and no height escape.       |
| H1-5              | Existing recovery predicates and lane tests                       | Recovery interval boundary, convergence priority, backstop eligibility, and frontier eligibility. |
| Metric collection | Metric constants and harness extension                            | Registration and extension idempotence.                                                           |

The new Rust tests live in `node/src/rust/instances/heartbeat_responsive_tests.rs` and `casper/tests/add_block/h1_proposer_spec.rs`.

The verification dispatcher now selects the full heartbeat module. Its earlier `heartbeat_proposer::tests::` filter omitted the new sibling test module.

## Verification record

| Check                                                                   | Result at report creation                                                                                |
| ----------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `cargo check -p node -p casper --tests`                                 | Passed before the final lint-only edits. Final targeted tests rebuilt both crates.                       |
| `cargo clippy -p node -p casper --all-targets`                          | Passed on `ab84edf83`, with no warnings.                                                                 |
| Final formatting                                                        | The runner reports the commit hook passed formatting on `ab84edf83`.                                     |
| `cargo test --release -p node -p casper --no-fail-fast`                 | Exit 0 on pre-final source. Supporting evidence only. The largest binary had 1092 passed and 12 ignored. |
| `cargo test --release -p node heartbeat_proposer`                       | Exit 0 on `ab84edf83`: 61 passed, 0 failed, 0 ignored, 341 filtered out.                                 |
| `cargo test --release -p casper --test mod add_block::h1_proposer_spec` | Exit 0 on `ab84edf83`: 3 passed, 0 failed, 0 ignored, 1101 filtered out.                                 |
| `bash scripts/bench/test-extend-issue24-metrics.sh`                     | Passed locally, exit 0. Expected malformed-fixture errors remain visible.                                |
| `bash scripts/test-verify-heartbeat-proposal-bound.sh`                  | Passed locally, exit 0. This checks dispatch, not Rust behavior.                                         |
| Shell syntax checks                                                     | Passed locally for the changed verification and metric-extension scripts.                                |

Direct cargo execution in this session failed before compilation because the sandbox denied the Cargo home configuration. That attempt supplied no test evidence.

The cooperating runner executed cargo outside the sandbox and posted the final-source counts in the shared coordination channel. Raw logs remain in private scratch storage.

This agent independently matched the four source digests below against the checkout. It did not execute cargo successfully inside the sandbox.

The initial implementation is `ab84edf83`. The hash-binding refinement is `e7638bbd5`, and the current heartbeat source matches `da1c12ed1`.

The runner reported 61 passed and 0 failed for `cargo test --release -p node heartbeat_proposer` after the refinement.
The runner also reported no warnings from `cargo clippy -p node --all-targets`. Those results identify the heartbeat bytes below.

The earlier adapter run and Casper test results remain bound to their recorded source. This digest refresh does not establish new verifier results.

## Source identities at `da1c12ed1`

```text
3c62915518a13bf20d8c79d0b1da106a3db5e26de6141412ef1f4fe287500576  node/src/rust/instances/heartbeat_proposer.rs
ccb01fdcf2027f7698066ed5ad166e260fab043846088fc9939c1a3f640c18ac  casper/src/rust/blocks/proposer/proposer.rs
115c1dc774b5ca5f1da37fe1a8dc57730c748098540f467add20736451c8a716  node/src/rust/instances/heartbeat_responsive_tests.rs
21179bcccef9de2b87dfe4c12c9f943295f7d0f6f6218bf9d955cd97aca2b6df  casper/tests/add_block/h1_proposer_spec.rs
```

Current evidence records identify the heartbeat, proposer, and API bridge. The two replaced `dev` records remain unchanged in the history directories.

The 13 intermediate PR snapshots remain in checkpoint `b87d1dfe29ee6d06d1894aec3db872c63ff6829f`. Their removal changes no runtime or test source.

Historical JSON entries use `git_object` and retain the original SHA-256. Retrieve a historical object with `git show <git_object>`.

A Git object identifies checkpoint bytes, not a file in the current working tree.

The API bridge's older Batch E source and claim digests differ from the current files. Its acceptance is not transferred onto the new bytes.

## Additional verification on 2026-10-19

The runner verified the additional tests against `f41f2a8ab` plus the test changes in the working tree.

- Formatting and `cargo check -p node -p casper --tests` passed.
- `cargo clippy -p node -p casper --all-targets -- -D warnings` passed with no warnings.
- The heartbeat filter passed 65 tests, with no failures or ignored tests.
- The bounded queue-entry test passed 1 test, with no failures.
- The H1 proposer filter passed 7 tests, with no failures.

Nine tests were added across these suites. They cover cancellation after issuance, delayed completion, queue rejection and re-enqueue, changed predecessors, depleted selection, and legacy requests.

Two controls deliberately alter test state to expose rolling-bound violations. They do not mutate production source or establish a mutation-testing score.

The queue-entry test uses the production entry type and a real bounded channel. It does not execute the production worker or activate disabled retries.

The queued proposer tests execute the real proposer with controlled snapshots, block creation, validation, and effect counters. They are not network integration tests.

### Test-file consolidation

The four additional tests now reside in the inline `additional_tests` module of `heartbeat_responsive_tests.rs`. Their test bodies and full test names are unchanged.

The runner verified the merged working tree on `33c0ac7c4`. Formatting passed, strict node clippy reported no warnings, and all 65 heartbeat tests passed.

The earlier two-file inputs remain in the previous-review Git objects at `33c0ac7c4af184573977b6b5184b81f4d5da4740`. No new history files were added.

The current merged test inputs are:

```text
034ce24f396ee3719013095e10970d3327cf4df5092daf3942dae1f284473ef3  node/src/rust/instances/heartbeat_responsive_tests.rs
c76ea6698d479db9617bba7cad5f7868a74f217d7582b28ae7ec8c6309aa9431  node/src/rust/instances/proposer_instance.rs
b9f54549857fa29b0f008210c7e9b685a4304fd88a9eea1a79ec0b00d702b1c9  casper/tests/add_block/h1_proposer_spec.rs
```

The heartbeat and proposer runtime digests remain unchanged. The [API renewal report](api-display-projection-renewal-2026-10-19.md) records the source comparison, fresh tests, and declared Linux and Rocq limits.

## H1 maintainer acceptance

The shared maintainer handoff records [acceptance on PR #768](https://github.com/F1R3FLY-io/f1r3node-rust/pull/768#issuecomment-6265326408) by `jeffrey-l-turner` at `2026-10-19T10:40:35Z`.

The acceptance covers H1-1 through H1-5 at revision `0a4acca7a179d69e766e65672480390c02b45fe5`. The accepted claim digest remains `0f62182a84d4f83eae00e6dab4647d1299ffddedb0e98d5840b42df7e57db44b`.

The exact accepted evidence remains immutable:

```text
37a9a5b80b2b5f2ceb5f003915acf438192cef771939bdffaba40f2efb21065e  0a4acca7a179d69e766e65672480390c02b45fe5:docs/casper/validation/heartbeat-h1-verification-2026-10-19.md
acfd02d5a6b477d453767a9b6b7871eb3c163b660f41f4c89b8ea613a2d46701  0a4acca7a179d69e766e65672480390c02b45fe5:docs/casper/validation/api-display-projection-renewal-2026-10-19.md
```

This updated report records that acceptance. It does not replace the accepted inputs with a newly verified proof.

The accepted construction assumptions are:

- One live heartbeat task owns the state.
- The queue and heartbeat loop await requests sequentially.
- Block hashes are immutable, and snapshots are consistent.
- Publication effects precede a successful callback.

The accepted H1 limits remain explicit:

- No bound applies across process restarts or globally across the excluded proposal paths.
- Linux display admission and the Rocq recheck were not executed. No pass is recorded for either check.
- The agentic verifier supplies executable-check evidence only. The construction argument is reviewed, not mechanized.

The acceptance closes H1 discharge, construction and binding review, and H1 claim acceptance at that scope. It does not accept performance or the separate display-projection claim.

The acceptance provenance is the shared maintainer handoff and its linked record. This sandbox could not authenticate GitHub or verify an SSH approval.

## Remaining gates

Each open gate has one accountable owner. A runner executes an authorized operation but does not grant acceptance.

| Open gate | Owner | Required result |
| --- | --- | --- |
| Separate display-projection acceptance | Maintainer | Review CLAIM-CASPER-NODE-OBSERVATION-005 independently. The H1 acceptance does not accept that claim or establish fresh Linux or Rocq results. |
| Comparison execution and performance acceptance | Maintainer | Complete the authorized comparison and evaluate every threshold in design section 5.1. `f1r3node-rust-71` is the runner. |

### Completed prerequisites

- **H1 discharge, construction and binding review, and claim acceptance:** The maintainer accepted H1-1 through H1-5 at the revision and evidence digests above. The recorded assumptions and limits remain applicable.
- **API-scoped evidence renewal:** `pi-session-heartbeat-h1-cbc` recorded the two-line API comparison, 53 display tests, 3 arithmetic tests, and 36 capture tests. TLC passed 18 positive configurations and detected 96 expected violations. Separate display-projection acceptance remains open. Linux and Rocq limits are not passing results.
- **Additional test coverage:** `pi-session-heartbeat-h1-cbc` added nine tests. The runner verified the 65-test, 1-test, and 7-test selections. Broader claim review remains open.
- **Current-source evidence digests:** `pi-session-heartbeat-h1-cbc` refreshed the heartbeat binding to `da1c12ed1` and updated the affected report digests. Intermediate records remain available in immutable checkpoint `b87d1dfe29ee6d06d1894aec3db872c63ff6829f`.
- **Proposer compatibility symlink:** Closed by `14c4c3546`. The maintainer authorized the creation, and `f1r3node-rust-71` created the symlink.
- **Real adapter execution:** The runner completed both artifact commands on `ab84edf83`. Both commands exited 0. This closes the execution prerequisite only.
- **Numeric thresholds:** The maintainer approved the thresholds in design section 5.1. Performance thresholds do not discharge the safety claim.
- **Comparison authorization:** The maintainer authorized the baseline and H1 runs in sequence. The maintainer owns authorization for any additional soak.

The adapter recorded a proposal from a large language model. It did not apply or check that proposal. The tool status depended on the targeted test command's exit status.

The adapter's `discharged` label supplies executable-check evidence, not a mechanized proof of H1-1 through H1-5. H1 acceptance comes from the separate maintainer review recorded above.

The runner reported baseline run `39194089405` at `ed9b3c996` with profile `weekend-60h`. The H1 run is planned for `da1c12ed1` after that baseline.

Comparison results are not recorded here. H1 does not establish the cause or resolution of the complete 45-second finalization miss.

The additional implementation changes are test-only. This agent did not create a commit, stage files, push, or switch branches.
