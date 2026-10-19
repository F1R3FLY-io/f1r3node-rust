# H1 Implementation and Verification

**Task:** TASK-021-26.

**Owner:** pi-session-heartbeat-h1-cbc.

**Branch:** `fix/issue-24-deploy-heartbeat-wake`.

**Baseline:** `ed9b3c996`, the F1/F2 implementation.

**Status:** Implementation and final-source targeted tests passed at `ab84edf83`. Claim discharge, maintainer acceptance, and performance qualification remain pending.

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

## Construction argument for review

This argument is not a mechanized proof or maintainer acceptance. It assumes the production request queue and heartbeat loop retain their sequential await behavior.

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

The runtime implementation is commit `ab84edf83`. The maintainer committed and pushed that source while evidence closeout continued.

## Source identities

```text
77328af2cc336e93299f0dbf25100309359b49825e98f3e4dfdfea3bf8f18c5e  node/src/rust/instances/heartbeat_proposer.rs
ccb01fdcf2027f7698066ed5ad166e260fab043846088fc9939c1a3f640c18ac  casper/src/rust/blocks/proposer/proposer.rs
115c1dc774b5ca5f1da37fe1a8dc57730c748098540f467add20736451c8a716  node/src/rust/instances/heartbeat_responsive_tests.rs
21179bcccef9de2b87dfe4c12c9f943295f7d0f6f6218bf9d955cd97aca2b6df  casper/tests/add_block/h1_proposer_spec.rs
```

Current pending evidence records identify the heartbeat, proposer, and API bridge. Historical evidence remains unchanged in separate history files.

The API bridge's older Batch E source and claim digests differ from the current files. Its acceptance is not transferred onto the new bytes.

## Remaining gates

- Create the new proposer compatibility symlink before legacy evidence consumers run.
- Obtain real CbC verifier results or record an unavailable adapter as a gate.
- Review the construction argument and the source bindings against the final implementation.
- Complete any required negative-control, cancellation, or queue-interleaving coverage before discharging the full claim.
- Obtain the required maintainer acceptance without reusing the historical leader-only discharge.
- Renew the separate API display-projection evidence for its current source and claim bytes.
- Approve numeric comparison thresholds before a separately authorized soak against the F1/F2 baseline.

No soak or cloud run was dispatched for this work. H1 does not establish the cause or resolution of the complete 45-second finalization miss.

No commit, push, stage, or branch operation was performed by this agent.
