# API Display-Projection Renewal

**Owner:** `pi-session-heartbeat-h1-cbc`.

**Task:** TASK-021-26.

**Claim:** [CLAIM-CASPER-NODE-OBSERVATION-005](../../claims/casper-node-display-projection.md).

**Status:** API-scoped renewal evidence is recorded for current-source review. Full claim discharge and maintainer acceptance remain pending.

## Scope and source

This renewal covers `casper/src/rust/api/block_api.rs` and its display-projection obligations. It does not renew every Batch E artifact.

The source checkpoint is `f41f2a8ab`, with additional test changes in the working tree. The API source itself is unchanged by those tests.

```text
9bf8f175f132f89b373a10af6eba70a9325a6ec9c4fc15cb8ab72a0a71d080f2  casper/src/rust/api/block_api.rs
a5bfd2da4bad84cf727762f4d8834e879279bad4ada7dfe850db3ee5e0717592  casper/src/rust/safety/initial_fault.rs
e86bf8c6df02c037b70df94e1140f72b5d33d55e7806db7859b5669dfa1d2e0b  casper/src/rust/soak_observer/display.rs
1729a5758c702a3c4c006295f5e07ad609f11b9c7d91c875f57199a4097df591  docs/claims/casper-node-display-projection.md
02ff18503f08922eea4a97438c129a194a9b64a812c56e587ddb270cee89e1a8  formal/tlaplus/node_observation/bindings.json
```

## Accepted-to-current comparison

The outside-sandbox runner compared accepted revision `1a9839b0e` with `f41f2a8ab`. The accepted API digest is `fe33e07118d43ab733870ac54840e648d4196a5890188b1b5343b5593d60cd26`.

The API diff contains exactly two changed lines:

- `tp(casper_for_propose.clone(), true)` now supplies `true.into()`.
- `trigger_propose_f(casper, is_async)` now supplies `is_async.into()`.

Both changes belong to proposal dispatch. No display arithmetic, public projection, response shape, or store-read line changed.

The new legacy-request test verifies both boolean values. Conversion retains `is_async`, sets `require_user_deploys` to false, and leaves `expected_empty_self` absent.

Those defaults do not grant H1 authority to a legacy API request. The H1 request guards do not change the legacy display calculation.

The public API still selects persisted finalized values or the oracle path. It still subtracts normalized initial fault through the same shared display helper.

## Current verification

The runner executed the following commands locally on macOS, outside this session's sandbox:

| Check | Result |
| --- | --- |
| `cargo test --release -p casper --test soak_observer` | 53 passed, 0 failed, 0 ignored. |
| `cargo test --release -p casper --lib initial_fault::tests` | 3 passed, 0 failed, 0 ignored. |
| `cargo test --release -p block-storage --test soak_snapshot` | 36 passed, 0 failed, 0 ignored. |
| `cargo test --release -p casper --test mod add_block::h1_proposer_spec` | 7 passed, 0 failed, including legacy boolean conversion. |
| `cargo check -p node -p casper --tests` | Passed. |
| `cargo clippy -p node -p casper --all-targets -- -D warnings` | Passed with no warnings. |
| `bash scripts/ci/check-tla-invariants.sh --soak-pr` | Passed: 18 positive configurations and 96 expected negative-control violations. |

The TLC run used OpenJDK 27 and the runner's existing TLC jar. Raw logs remain in private scratch storage because they contain home paths.

This agent independently compiled the unchanged arithmetic file as a standalone test crate with `rustc 1.95.0-nightly (6efa357bf 2026-02-08)`:

- `rustc --edition=2021 --test`: 3 passed, 0 failed, 0 ignored.
- `rustc --edition=2021 --test -O`: 3 passed, 0 failed, 0 ignored.

These checks exercise debug and optimized arithmetic. They do not replace the Cargo integration checks.

## Binding coverage and limits

The current binding manifest maps 17 properties to 26 unique tests:

- Fifteen tests in `casper/tests/soak_observer.rs`.
- Seven tests in `block-storage/tests/soak_snapshot.rs`.
- Three arithmetic tests in `casper/src/rust/safety/initial_fault.rs`.
- One Linux admission test in `node/tests/soak_observer.rs`.

The successful suites cover the 25 named non-Linux tests. The Linux admission test was not executed because no existing local Linux environment was available.

Property 16 covers frozen arithmetic, record multiplicity, and compiler-profile overflow behavior. Its three named tests passed in the current Cargo run.

The API comparison and legacy-request test bind the two H1 conversions. They do not establish a machine-checked Rust refinement.

The runner could not repeat the Rocq kernel check because Rocq was not installed. `check-formal-invariants.sh --rocq` exited 1 at tool discovery.

The Linux and Rocq limits are not passing results or waivers. A complete fresh platform and kernel verification is not claimed.

## Historical evidence boundary

The display calculator, shared arithmetic, DisplayProjection model and its three negative controls match the historical package byte-for-byte.

The DisplayProjection and MainTheorem Rocq files also match. Capture, dispatch, tests, bindings, and verification scripts differ from the historical package.

The fresh executable and TLC results are recorded above. No fresh Rocq result is inferred from unchanged theorem files.

The earlier acceptance remains bound to its original source, claim, and evidence digests. This renewal does not transfer that acceptance to the current bytes.

The existing tracker-wire and floating-point construction gaps remain explicit. No result establishes equality between a concurrent public response and a detached display value.

## Remaining review and environment gates

- **Maintainer:** Review this API-scoped renewal and record current-source acceptance or required further work.
- **Maintainer:** Authorize any new Linux environment or Rocq setup before the missing checks run.
- **Runner `f1r3node-rust-71`:** Execute the Linux admission and Rocq checks after environment authorization.

The Pi-owned evidence renewal is complete at the recorded scope. The [H1 verification record](heartbeat-h1-verification-2026-10-19.md#remaining-gates) retains the broader discharge and acceptance gates.
