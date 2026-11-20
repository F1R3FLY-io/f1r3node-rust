# FX-8b onto `dev`: rebase dry run (lane B)

Date: 2026-11-03. Nothing in this dry run was committed or pushed. All work is on local scratch branches.

| Item | Value |
|---|---|
| Source | `origin/patch/FX-8b` at `4e20959b8` |
| Target | `origin/dev` at `746a1c368` |
| Merge base | `1a19f1127` |
| `dev` ahead of base | 96 commits |
| FX-8b ahead of base | 235 commits (200 are not merges) |
| Files changed on both sides | 18 |
| Files with a textual conflict in the net diff | 2 |

## 1. Summary

A commit-by-commit rebase is expensive, but the net change is not. Most stops come from the FX-8b history itself (lane merges and duplicate commits), not from `dev`.

| Strategy | Scratch branch | Manual stops |
|---|---|---|
| Linear `git rebase origin/dev` | `scratch/fx8b-rebase-dryrun` | 33 |
| `git rebase --rebase-merges origin/dev` | `scratch/fx8b-rebase-merges-dryrun` | 29 |
| Squash of the net diff onto `dev` | `scratch/fx8b-squash-dryrun` | 2 files, then 1 semantic fix |

**Ruling (Decision 95):** the branch meets `dev` by one merge of `origin/dev` into FX-8b, after FX-15 lands, so the audited commit hashes survive. Lane A2 runs the merge. A merge and the squash give the same tree, so the resolutions in sections 2 and 3 apply unchanged. The resolved files are in the local commit `a918ba35e` on `scratch/fx8b-squash-dryrun` (lane B checkout, not pushed).

**Original recommendation:** rebase as one squashed commit of the net diff onto `dev`, or merge `dev` into FX-8b once. Both give the same tree. The squash tree compiles, and all seven suites pass on it (section 5). The per-patch history stays on the existing `patch/*` branches.

## 2. Textual conflicts in the net diff

`git merge-tree` and `git merge --squash` agree: only two files conflict.

### 2.1 `casper/src/rust/rholang/runtime.rs`

- **Cause.** `dev` adds `parameters` to the `DeployData` of an exploratory deploy. P3-02 adds `cosignatures` and `required_cosigners` to the same struct literal.
- **Resolution.** Keep all three fields:

```rust
parameters,
cosignatures: Vec::new(),
required_cosigners: Vec::new(),
```

### 2.2 `node/src/rust/api/web_api.rs`

- **Cause.** `dev` closes a Rholang injection in `/api/balance` and `/api/registry` with a deploy parameter (`rho:deploy:param:address`, `rho:deploy:param:uri`). P2-12 adds the wallet path (`WalletPath::Legacy` or `Native`) and validates the address before it builds a term.
- **Resolution.**
  - Imports: the union of both sides (`DeployParameter`, `RholangValue`, `native`, `VaultAddress`).
  - `balance_query_term`: keep the address validation. Return `BALANCE_RHOLANG_TEMPLATE` for `Legacy` and `native::balance_term` for `Native`.
  - `get_balance`: send the `address` parameter only on the `Legacy` path. The `Native` term carries the validated address and has no parameter.
  - `mod tests`: the base tests, plus `dev`'s two injection tests and their helpers, plus FX-8b's wallet-path tests.
- **Review point for lane C or the `dev` owner.** On the `Native` path the address goes into source text, not through a deploy parameter. The address validation is the only guard there. The test `balance_query_rejects_addresses_that_are_not_valid` covers an injection payload on both paths. A follow-up can move `native::balance_term` to a deploy parameter too. Label: known issue, not blocking, because the validation refuses every non-address string and the test proves it.

## 3. Semantic fix that has no textual conflict

`dev` adds a fourth argument, `parameters: Vec<DeployParameter>`, to `play_exploratory_deploy`. FX-8b calls it with three arguments in new files, so the compile fails with E0061. The fix adds `Vec::new()` at each call:

- `casper/src/rust/v6/genesis_policy.rs`
- `casper/tests/v6/pos_bond_admin_spec.rs`
- `casper/tests/v6/gated_executor_spec.rs`
- `casper/tests/v6/pos_v6_spec.rs` (2 calls)
- `casper/tests/genesis/v6_genesis_wallets_spec.rs` (2 calls)

## 4. Files changed on both sides without a conflict

Git merged these files automatically. The suites in section 5 cover them, but a reviewer must know them, because a behavior change can pass a textual merge:

- `Cargo.toml`, `Cargo.lock` (supply-chain updates on `dev`)
- `casper/src/rust/api/block_api.rs`, `casper/src/rust/casper.rs`, `casper/src/rust/engine/approve_block_protocol.rs`
- `casper/src/rust/util/mod.rs`, `casper/src/rust/util/rholang/interpreter_util.rs`
- `casper/tests/add_block/multi_parent_casper_add_block_spec.rs`, `casper/tests/soak_observer.rs`, `casper/tests/util/rholang/runtime_manager_test.rs`
- `models/src/rust/rholang/sorter/score_tree.rs` (the SortedParMap change and its revert on `dev`)
- `node/src/rust/api/deploy_grpc_service_v1.rs`, `node/src/rust/runtime/setup.rs`, `node/src/rust/web/web_api_routes.rs`
- `rholang/src/rust/interpreter/reduce.rs` (the `dev` join rule that refuses channels that resolve to the same name; FX-2 and FX-4 change the same file)
- `rspace++/src/rspace/space_matcher.rs`

## 5. Results on the squash tree

Command: `RUSTC_BOOTSTRAP=1 cargo +stable test --release`.

| Suite | Passed | Failed | Ignored |
|---|---|---|---|
| `-p rholang` (all targets) | 2451 | 0 | 70 |
| `-p casper --lib` | 860 | 0 | 1 |
| `-p casper --test mod -- v6::` | 85 | 0 | 7 |
| `-p casper --test mod -- genesis::` | 118 | 0 | 1 |
| `-p casper --test mod -- util::` | 134 | 0 | 3 |
| `-p node --lib` | 429 | 0 | 0 |
| `-p casper --test mod -- add_block::` | 44 | 0 | 0 |

- `cargo check -p node -p casper -p rholang --tests` passes.
- `cargo fmt --check` is not verified.
- Clippy did not run, because the dry run adds no code for review.
- The `dev` tests `get_balance_template_binds_address_through_ast_not_source` and `get_registry_template_binds_uri_through_ast_not_source` pass next to the FX-8b wallet-path tests.
- The full casper integration modules (`multi_node`, `batch1`, `batch2`, `slashing` and others) did not run. Run them in CI on the real rebase.

## 6. Stops in the commit-by-commit rebases

These stops are why the commit-by-commit rebase is not recommended. Most of them replay conflicts that the lanes already resolved in merge commits.

### 6.1 Linear rebase (33 stops)

The causes fall into four groups:

1. **Add/add (`AA`) on the v6 module.** The lane merges brought the same new files in twice: `casper/src/rust/v6/{backend,body,core,engine,linearizer,mod,propose,validity,resource_policy,executor,record,surface}.rs`, `casper/tests/v6/{mod,block_executor_spec}.rs`, `casper/tests/genesis/v6_genesis_spec.rs`, `docs/rev-stacks/decisions/{P1-04b,D76}.md`.
2. **Duplicate commits.** D76 appears twice (`3b6d77537` and `33b40ef29`). Both stop.
3. **Content (`UU`) replays of lane-merge resolutions.** Examples: `executor.rs` (P2-19, P3-07b, P3-04, FX-7, D76), `reduce.rs` (P3-04, FX-4, FX-2), `system_processes.rs` (FX-11, FX-13), `region_spec.rs`, `gated_executor_spec.rs`, `seal_wiring_spec.rs`, `journal.rs`, `settle.rs`.
4. **Binary fixtures.** `casper/tests/fixtures/v6/cross_host/{block,genesis,record}.bin` stop at FX-12 and at the FX-8b fixture commit.

Seven stops touch a file that `dev` also changed:

- `casper/src/rust/casper.rs`: `6e758814e`, `f6009a39a`
- `rholang/src/rust/interpreter/reduce.rs`: `afbbf1834`, `def89b562`, `4aed4d852`
- `casper/src/rust/rholang/runtime.rs`: `c59b01727` (P3-02)
- `node/src/rust/api/web_api.rs`: `ace1187df` (P2-12)

In the net diff, only `runtime.rs` and `web_api.rs` still conflict. These are the two conflicts in section 2. The `casper.rs` and `reduce.rs` stops conflict only with intermediate FX-8b states.

### 6.2 `--rebase-merges` (29 stops)

Most stops are at the lane merge commits, for example `3fc601e6b`, `954272af0`, `45bfb2246`, `67104a363`, `e82192f70`, `692db527c`, `92bcf18ea`, the FX-8 merges of FX-3, FX-4 and FX-5, the FX-8b merges of FX-7, FX-2-refunds, FX-11, FX-13 and FX-12, and `e51906722`. Git cannot reuse the earlier resolutions without `rerere` data from the original merges, so each merge stops again.

## 7. Procedure (Decision 95)

1. Check out FX-8b after FX-15 lands.
2. Run `git merge origin/dev`.
3. Apply the resolutions in section 2 and the fix in section 3.
4. Run the suites in section 5 and the CI integration pipeline.
5. If `dev` moves before Friday, repeat the dry run. The cost is low, because only the net diff conflicts.

## 8. Local scratch state

- `scratch/fx8b-rebase-dryrun`, `scratch/fx8b-rebase-merges-dryrun`: the aborted rebase attempts. Delete them after the real rebase.
- `scratch/fx8b-squash-dryrun`: the squash is staged and the fixes are unstaged. Nothing is committed.
- The pre-push hook still refuses every ref outside the lane B list, and no scratch ref is on that list.
