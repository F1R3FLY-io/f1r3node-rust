# F1 and F2 heartbeat wake evidence

The [heartbeat claim](../../../docs/claims/heartbeat-proposal-amplification-bound.md) covers the deploy wake and the cooldown recheck. This run checks the wake and recheck decisions for one admitted deploy or one heartbeat check.

## Refutation and construction

TLC checked 82,945 distinct states with `HeartbeatWake.cfg`. One initial state leads to 82,944 evaluations. The instance includes accepted, duplicate, and failed admissions. It enumerates both values of each Boolean input and all combinations of six time values, including zero. The instance uses milliseconds and a 10 ms recheck floor.

| Configuration | Expected and observed result |
| --- | --- |
| `HeartbeatWake.cfg` | Exit 0, all invariants hold |
| `WakeOnRejected.cfg` | Exit 12, `WakeOnlyOnAdmission` fails |
| `BypassCooldown.cfg` | Exit 12, `CooldownProtectsPending` fails |
| `RecheckDespiteLag.cfg` | Exit 12, `RecheckRequiresPendingCooldown` fails |

The [TLA+ gate](../../../scripts/ci/check-tla-invariants.sh) runs all four configurations in both tiers. Each expected violation has a trace from the initial state to the evaluated check. The [gate fixture](../../../scripts/ci/test-check-tla-invariants.sh) checks that both tiers include all four configurations.

After the merge from `dev`, the gate fixture passed all 101 expected negative controls and verified workflow routing to both tiers. The [registration claim](../../../docs/claims/heartbeat-wake-model-registration.md) records this obligation.

Run TLC from this directory. Replace `HeartbeatWake.cfg` with each control configuration to reproduce its verdict.

```bash
java -Xmx1024m -jar "$TLA_TOOLS_JAR" -workers 1 -config HeartbeatWake.cfg HeartbeatWake.tla
```

Rocq 9.1.1 compiled [MainTheorem](../../rocq/heartbeat_wake/theories/MainTheorem.v). `rocq check` passed, and all seven theorems reported a closed global context. The time theorems quantify over natural numbers. The expiration theorem assumes that the remaining time fits in the millisecond duration conversion.

The [Rocq gate](../../../scripts/ci/check-formal-invariants.sh) builds the project, checks its kernel proof, and checks the assumptions for all seven theorems. The [formal gate receipt](../../../scripts/ci/check-formal-gate.sh) includes the proof sources and project file.

The updated Rocq gate rejected a scratch `Admitted` declaration in each of its eight project scans. It also rejected a missing source directory. The clean run passed all eight builds and kernel checks. Each of the 75 listed theorem checks reported closed assumptions.

Run these commands from `formal/rocq/heartbeat_wake`.

```bash
rocq compile -q -Q theories HeartbeatWake theories/MainTheorem.v
rocq check -silent -Q theories HeartbeatWake HeartbeatWake.MainTheorem
```

## Rust binding

| Model input or result | Rust source and observable effect |
| --- | --- |
| `ApiProposes` | [Casper assembly](../../../node/src/rust/consensus/casper/assembly.rs) provides an API proposer when autopropose is enabled and the node has a proposal trigger. |
| `wake` | [Deploy admission](../../../casper/src/rust/engine/multi_parent_casper/block_admission.rs) signals the heartbeat after a new reservation, when the wake is enabled and a signal exists. |
| `Cooling` | [Heartbeat check](../../../node/src/rust/consensus/casper/instances/heartbeat_proposer.rs) compares the known age of the latest own block with the configured cooldown. |
| `Ready` and `otherLaneDue` | The heartbeat evaluates pending-deploy lag eligibility and the other lanes from the current snapshot before it returns a recheck. |
| `sleep` | The heartbeat clamps the timer delay to the smaller of the check interval and the floored recheck delay. The next check evaluates the current snapshot. |

At `2267799f938e0e5ef8664d1eaa18c26becaadc72`, the 48 heartbeat tests and three launch integration tests passed. The three launch integration tests also passed on the patch applied to `ca-canary` base `7a684d9fd6d88aaaf56203715e2328d8342be6ac`.

The B1 unit test `a_node_wakes_the_heartbeat_on_deploys_only_without_autopropose` passed after the node startup call used the enablement helper. The helper returns the negation of the `ApiProposes` rule. The startup call supplies the configured autopropose flag and the node's proposal trigger.

The B1 RED control enabled the wake for every startup configuration. The test failed for autopropose with a proposal trigger. The pre-cycle source already produced the expected wake decision through its filtered API proposer. The implementing agent reported 66 focused tests passed on the final B1 source. Strict Clippy and formatting checks passed.

The B1 startup polarity test and all three launch integration tests passed on the merged head `61292b2b750b8a3ed00629cf481e0b4c03a5b014`. The manifest records that head's startup and launcher source hashes.

The B2 test uses a pending deploy and an own block about 1 s old. The cooldown is 3 s and the check interval is 60 s. The production heartbeat check makes zero proposals and computes a delay above 1.5 s and at most 2 s. The test covers the delay calculation. The accounting smoke below exercises the live loop.

The B2 RED control made `cooldown_recheck_delay` return `None` in a scratch copy. The test failed with a 60 s delay. The existing F2 code passed the test before any production edit. An independent run passed all 57 heartbeat tests. The implementing agent also reported that strict Clippy and formatting checks passed.

The B3 test runs the heartbeat task with a 200 ms check interval and a 1.5 s cooldown. It sends four deploy wake signals 250 ms apart. The test records proposal times and compares each recorded time with the deadline from the own-block timestamp. It also requires a proposal within the test wait limit.

The original B3 RED control set the cooldown to zero only on a signal wake. It failed with three proposals at the first sample of the earlier test. All 58 heartbeat tests passed independently on the earlier `dev` candidate and the isolated `ca-canary` candidate with the same heartbeat source.

The revised B3 test failed when a scratch control shortened the cooldown to 1.2 s. It recorded a proposal 288 ms before the deadline. The earlier test passed that control. A control that removed the cooldown failed with six early proposals.

The cooldown recheck counter records a timer that expires after a shortened sleep. The new unit test checks that signal wakes and ordinary timer intervals leave the counter unchanged. The revised B3 test and the counter test passed in the 60-test heartbeat suite. This run used an isolated build target on the merged `dev` candidate.

The B4 fixture sets both pending-deploy lag caps to one block. The validator is eight blocks ahead of the latest finalized block. At an own-block age of 1 s, the check must produce zero proposals and keep its 60 s interval. At 4 s, after the 3 s cooldown, the check must still produce zero proposals. The test supplies each age with a block timestamp and calls the production check directly.

The implementing agent's B4 RED controls each failed on a scratch copy. `RecheckDespiteLag` scheduled a 2 s recheck. A control that removed the lag cap from pending-deploy eligibility scheduled a 1.999 s recheck. A control that removed the lag cap only from the proposal decision made one proposal after the cooldown. This last control failed the proposal-count assertion independently of the timer assertion.

All 59 heartbeat tests passed on the `dev` candidate and the isolated `ca-canary` candidate with the same B4 source.

All eight proposer integration tests passed. The [proposer source](../../../casper/src/rust/blocks/proposer/proposer.rs) has the same bytes as `dev` at `71d435ea6f6a7d766b7e1bda29b96cc0017563ca`. The tests cover proposal constraints, publication effects, and empty recovery under the height constraint.

The [lane binding check](check-lane-binding.sh) compiled the candidate `decide_lanes` function and its version on `dev` at `3c6a46d2821fc22553f189c696cc7a42fb25c3b9`. The check passed 221,184 cases. It covered all 13 Boolean inputs and three relations to each lag cap. The existing lane decisions matched in every case. The new flag was true exactly when an active cooldown was the sole reason the pending-deploy lane was ineligible.

The same check compiled the production `cooldown_recheck_delay` and `next_heartbeat_sleep` functions. It compared the delay and selected sleep against the model's timing rule in 504 cases. It uses the model's time values and minimum recheck delay, both values of the cooldown flag, and an unknown own-block age.

```bash
bash formal/tlaplus/heartbeat_wake/check-lane-binding.sh 3c6a46d2821fc22553f189c696cc7a42fb25c3b9
cargo test --release -p node --lib heartbeat_proposer::
cargo nextest run --locked --release -p node -E 'test(/heartbeat/)'
cargo test --release -p casper --lib a_node_wakes_the_heartbeat_on_deploys_only_without_autopropose
cargo test --release -p casper --test mod engine::casper_launch_spec
cargo test --release -p casper --test mod add_block::proposer_spec
```

The checks assume that deploy admission supplies the reservation verdict and that the heartbeat supplies a consistent age and lane decision for each check. The formal models cover the local rules. The maintainer approved the F1 and F2 scope on 2026-11-27 at revision `60853a719`. Scheduler progress, multiple live tasks, accounting execution, and legacy recovery remain open proof obligations.

The F1 and F2 patch adds a counter to `add_deploy` in `block_admission.rs`. The block-ingest sources match `dev`. Commit `2cc05cedf` removed the legacy buffer wait verdict before this branch. The [buffer release claim](../../../docs/claims/casper-buffer-release.md) still has no discharge for this artifact.

## Local timing comparison

Both runs used `test_heartbeat_timing` with the native subprocess provider, `HEARTBEAT_TIMING=1`, and `HEARTBEAT_TIMING_REPS=5`. Each run used one warm-up followed by five high-load phases. The base was `dev` at `71d435ea6f6a7d766b7e1bda29b96cc0017563ca`. The candidate included F1 and F2 at `2267799f938e0e5ef8664d1eaa18c26becaadc72`.

| Metric | `dev` | F1 and F2 |
| --- | ---: | ---: |
| Mean inclusion p95 | 6.36 s | 4.16 s |
| Mean finalization p95 | 42.28 s | 38.94 s |
| Mean blocks created per validator and phase | 9.8 | 12.0 |
| Phases above 9 s inclusion p95 | 0 of 5 | 0 of 5 |
| Unfinalized deploys | 0 | 0 |

Both tests passed and the last finalized block advanced in every phase. This comparison covers the local five-phase profile. The shared merge plan also requires accounting-shard qualification. [Qualification plan](../../../docs/designs/deploy-heartbeat-wake.md#7-shared-merge-plan)

## Accounting-shard smoke and finality fixture

The standalone protocol-6 smoke applied F1 and F2, including the B1 startup change from `fe64a7fe84eaaf633138cea93ba2b9b4441e5477`, to `ca-canary` base `7a684d9fd6d88aaaf56203715e2328d8342be6ac`. With autopropose off, the validator recorded two deploy wakes, one scheduled cooldown recheck, and zero A-LIN holds. Both submitted offers executed with outcome `Completed`.

The stored timestamps of the two deploy blocks were 3,008 ms apart with a 3,000 ms cooldown. Blocks 3 and 4 each carried one execution record and zero deploys. The order floor reached block 4. The standalone state-finality floor stayed at genesis during the smoke.

On the same accounting candidate, the three-validator `the_lfb_follows_the_attested_canonical_roots_below_the_order_floor` fixture passed. It checks state-finality progress and canonical execution roots on all validators. The run logged one cancelled replay warning. The fixture uses direct proposals. B2 adds test coverage on `dev` while preserving the runtime behavior measured here.

## Evidence hashes

Run `shasum -a 256 -c` from the repository root with this manifest.

```text
e3fe9b556664cebe3552971e65f92cc0feb101ff947642705dd6fec31558b519  formal/tlaplus/heartbeat_wake/HeartbeatWake.tla
4fce31bcd33762bd91fbbfb1e5e80adb487634827b6536c5296798fca50d329b  formal/tlaplus/heartbeat_wake/HeartbeatWake.cfg
814fbb3c699fc1f2b6cfe1e1b50e5545cd0a830a713842e2504fa3e47be1c2f7  formal/tlaplus/heartbeat_wake/WakeOnRejected.cfg
94846d945a56f3c520921a4f84224b37f69da98a7a99df328d338dadcc6a470e  formal/tlaplus/heartbeat_wake/BypassCooldown.cfg
aea3b843d992f1ebc31f9b6a0852b1d5c61a432ad89c67c04d949ab37b1120cc  formal/tlaplus/heartbeat_wake/RecheckDespiteLag.cfg
0c9429936c975ea52ec6fb3d6f3344bf958b9b9fd39325094e5576de033aff6b  formal/tlaplus/heartbeat_wake/check-lane-binding.sh
a738ece1229e36459e6a80b8c88a65b9a629b80d427b7f42255f38f4f5d990a9  formal/rocq/heartbeat_wake/_CoqProject
b51339ed1416a0dc79e5c75dc065259e08e812da6988676dc48cefc0f176417a  formal/rocq/heartbeat_wake/theories/MainTheorem.v
acb8bcab18784bb02b8e5d3e3207d3131eac874de02aec1756d68061dba688e6  scripts/ci/check-formal-gate.sh
902dd2a717231b4867e521fb371caacf069e87232ce120a37dde12792692b6e6  scripts/ci/check-formal-invariants.sh
54b10f2ba7e4667ab5ebb6f59b017802bd532cfeba2d6239f85c99469a1a3292  scripts/ci/check-tla-invariants.sh
98830ea9f2b08b237b718d39a438db3fd182646dacf0371609c726a7ae926f26  scripts/ci/test-check-tla-invariants.sh
4b63cce0a392eb39d259e0a801479b665d9099b63a5a50bc85703e90bb1da92b  node/src/rust/instances/heartbeat_proposer.rs
985036670d95f0c67cd2669d6b6d050f053e2295b1496b49cded5e3c568cd107  node/src/rust/runtime/setup.rs
ac93273ac3edcfcc0e682621f28921d7653c216d0b2c2ec06c5abe0a52a819c9  casper/src/rust/engine/casper_launch.rs
ebfdb11d68d0e49de763caca5a3dbf2f85581d2eda8fb29099621421dcf4aa2e  casper/src/rust/blocks/proposer/proposer.rs
cbed7c15e71e8cfc3989fc0f9bdaf6daf88f6af199767c11c053fc35d9805512  casper/src/rust/engine/multi_parent_casper/block_admission.rs
d86302c7a1f1977f383f59f83767181ad1766fbfcf45d930d7a24cc55c331570  casper/tests/engine/casper_launch_spec.rs
```
