# Decisions — P1-rebased (lane A on M0)

Each entry gives the decision, the alternatives, the reason and whether it is reversible.

## D-P1-reb-1 — Base and replay order

- **Decision:** `patch/P1-rebased` starts at `fe6ad0400` (tag `rev-stacks/m0-track-a`). It replays, in order, `patch/P1-01`, `patch/P1-01-hooks`, `patch/P1-03` and `patch/P1-04` with `git cherry-pick`.
- **Detail:** `patch/P1-01-hooks` already holds its own replay of the three P1-01 commits (`cfad0b328`, `c4224777e`, `7527525d3`). Their patch-ids equal those of `35be2d164`, `555207c2c` and `4cf8c77c8`, so the replay takes the P1-01 commits once, from `patch/P1-01`, and then the four commits that are new on the hooks branch.
- **Reversible:** yes.

## Commit map

| Source | Rebased | Diff |
|---|---|---|
| P1-01 `35be2d164` | `391503a5b` | same |
| P1-01 `555207c2c` | `b5d08af18` | same |
| P1-01 `4cf8c77c8` | `7743c2336` | same |
| P1-01-hooks `a44c467e7` | `729786261` | resolved, see D-P1-reb-2 |
| P1-01-hooks `1326b1ddd` | `678c85380` | same |
| P1-01-hooks `226b3538e` | `38230e647` | same |
| P1-01-hooks `6acaeb679` | `cf5b3642d` | same |
| P1-03 `0436846a3` | `9c53c24b0` | same |
| P1-03 `700638aef` | `8250ddaa3` | same |
| P1-03 `465f214e0` | `b1c9f6922` | same |
| P1-03 `c08f47346` | `f9ff1c1e0` | same |
| P1-04 `a8aaf8670` | `4fe2a4115` | same |
| P1-04 `dd0734e57` | `96ab78221` | same |
| P1-04 `051c9346d` | `2cdf394b1` | same |

"Same" means the same patch-id (`git patch-id --stable`): the diff did not change, only the parent and the hash.

## D-P1-reb-2 — casper.rs and casper_launch.rs: keep both field sets

- **Conflict:** `a44c467e7` adds `v6_order_then_execute` to `CasperShardConf`, to its `new()` defaults and to the `casper_launch` literal. P0-12 added `rejection_search_cap_from_height` and `rejection_search_group_branch_cap` at the same three places. Three hunks conflicted.
- **Decision:** Keep both. The P0-12 fields come first, then `v6_order_then_execute`.
- **Reason:** The fields are independent. Neither side changes the other's lines.
- **Reversible:** yes.

## D-P1-reb-3 — validity.rs: no conflict in this replay; the conflict is in P1-04b

- **Finding:** In this replay, `casper/src/rust/v6/validity.rs` changes only in P1-01 (the module) and P1-03 (tests), and no commit conflicts there. The hooks do not edit `validity.rs`. They call it through `V6Engine::check_block` in `validation_dispatcher.rs`.
- **Where the conflict is:** `patch/P1-04b` `cf8d33d6e` changes `check_structure` and `V6Engine::check_block` to return `(V6Body, Vec<R>)`. A trial cherry-pick of `cf8d33d6e` onto this branch (local, not pushed) applies without a text conflict, but the hook does not compile:

  ```
  validation_dispatcher.rs:132  expected `&V6Body`, found `&(V6Body, Vec<BlockRecord>)`
  ```

- **Resolution for lane A2:** change the hook's match arm to

  ```rust
  Ok((body, _records)) => engine.observe_block(block, &body),
  ```

  With this change, the trial branch compiles, `casper --lib v6` gives 48 passes and `casper --test mod v6::` gives 3 passes.
- **Decision:** This branch does not take the change, because the new return type arrives with P1-04b. Lane A2 applies it when it rebases.
- **Reversible:** yes.

## Verification (2026-10-30, stable 1.97 with `RUSTC_BOOTSTRAP=1`, release build)

`cargo test --release -p casper -p rholang --no-fail-fast` passes.

| Package | Pass | Fail | Ignored |
|---|---|---|---|
| casper (lib 771, `tests/mod.rs` 1157, other test targets) | 1985 | 0 | 16 |
| rholang (with doc tests) | 2283 | 0 | 70 |

`cargo check -p casper -p node --tests` is clean.
