# Decisions — P1-01, second round (v6 flag and hook points)

Each entry gives the decision, the alternatives, the reason and whether it is reversible. The first round (the v6 module) is in `P1-01.md`, by another session.

## D-P1-01h-1 — A separate branch, patch/P1-01-hooks

- **Decision:** This round is on `patch/P1-01-hooks`. It starts at `patch/P0-08` and carries the three commits of `patch/P1-01` (cherry-picked with `-x`) under the hook commit.
- **Alternatives:** Push to `patch/P1-01`.
- **Reason:** Another session owns `patch/P1-01`, and `patch/P1-03` starts from it. The hooks need P0-07, which is on `patch/P0-08` only.
- **Reversible:** yes. The hook commit applies onto `patch/P1-01` after the phase 0 patches land.

## D-P1-01h-2 — The v6 flag has no per-node key

- **Decision:** `CasperShardConf::v6_order_then_execute` defaults to false, and `casper_launch` sets it to false. There is no `v6-order-then-execute` key in `CasperConf`, unlike `8fff49669`.
- **Alternatives:** Port the per-node key.
- **Reason:** Meredith decided on 2026-10-22 that v6 mode is a shard-wide parameter that the shard fixes at launch. P1-03 brings the protocol-6 genesis and sets the flag from it. Until P1-03, every hook is inactive and a node behaves as on dev.
- **Reversible:** yes.

## D-P1-01h-3 — Where the engine lives

- **Decision:** The engine is a field of `MultiParentCasperImpl` and of `FinalizationContext`. `hash_set_casper` starts it when the flag is set.
- **Reason:** The proposer, validation and the finalizer use the same engine, so no one task owns it. The two test-harness literals get `v6: None`.
- **Reversible:** yes.

## D-P1-01h-4 — Hook points on dev's structure

- **Decision:** The proposer calls `V6Engine::create_block` when the engine exists. Validation runs `check_block` and `observe_block`, skips the checkpoint and bonds step, and skips the mergeable cache. The snapshot reads active validators from the block's bonds when the post-state root is not held. The finalizer wakes the engine in `adopt_derived` after a committed advance.
- **Reason:** These are the hook points of `8fff49669`. The finalizer hook moves from `compute_last_finalized_block` (removed by P0-06) to the commit step.
- **Reversible:** yes.

## D-P1-01h-5 — The engine takes dev's lock-free deploy storage

- **Decision:** `V6Engine` holds `Arc<KeyValueDeployStorage>` in place of `Arc<parking_lot::Mutex<KeyValueDeployStorage>>`.
- **Reason:** dev removed the deploy pool mutex in `f1bc683d5`. The engine reads the pool once per proposal with `read_all`.
- **Reversible:** yes.

## Verification (2026-10-30, stable 1.97 with `RUSTC_BOOTSTRAP=1`)

- `casper --lib`: 743 tests pass, including the v6 module tests. The one failure is the WAL test that also fails in debug builds on dev.
- Clippy (`--all-targets`, `casper`) gives no warning in a file that this round changes.
- `casper --test mod` with the flag off: 95 tests pass in `add_block`, `blocks`, `multi_parent_casper_finalization_spec`, `multi_parent_casper_deploy_spec` and `last_finalized_api_test`.
