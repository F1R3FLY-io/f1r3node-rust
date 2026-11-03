# FX-8b decisions: the remaining fixes merged onto FX-8

Branch `patch/FX-8b`, lane A2, cut from `patch/FX-8`. The tip is the candidate for the testing preview. Lane A1 re-audits the merge commits (A-8).

## Merges

| Order | Branch | Commit | Conflicts |
|---|---|---|---|
| 1 | FX-7 | d1c1a5df0 | `executor.rs`, `reduce.rs`, `gated_executor_spec.rs`, `slot_spec.rs` |
| 2 | FX-6 (route-hole fix) | ead0663ef | None |
| 3 | FX-2-refunds | 38c8ca21c | `storage_rule.rs`, `charging_rspace.rs`, `P2-16.md` |
| 4 | FX-9 (Decisions 68, 69) | d6b2b3c40 | `gated_executor_spec.rs` |
| 5 | FX-10 | 972f8faa1 | None |
| 6 | P3-01 | 4c0ea0a03 | None |
| 7 | FX-11 | — | Not pushed yet. Merged last, when lane B pushes it. |

## Decisions

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 1 | `executor.rs` (FX-7): FX-5's envelope classification (`Unnormalized`, `Sealed`, `Named`) is kept, and each case carries FX-7's `bare_stack` flag. An unnormalized term has no bare stack. | Take FX-7's tuple and drop the classification. | FX-5 decision 1 (an unnormalized term names nothing, so the deployer alone pays) needs the classification. FX-7's gate needs the flag. | Yes |
| 2 | `check_region_signature` (FX-7): FX-7 resolved the same F30 conflict the same way. Only a variable name differs, and FX-7's name is kept. The order is FX-4's hash-seal check on the substituted forms, then FX-2's origin signers with FX-6's data refusal and `held_or_signed`, then FX-2's owner and kind. | — | Lane A1's first requirement. | Yes |
| 3 | `gated_executor_spec.rs` (FX-7): FX-7 added a check to a shared test (a cosigned envelope with no seal moves nothing). It is kept and written with FX-3's cosignature API (required cosigners in the signed data, a cosignature over the whole envelope). The other hunks were tests that both sides appended, and both are kept. | — | FX-7 was written before FX-3's API. | Yes |
| 4 | `storage_rule.rs`, `charging_rspace.rs` (FX-2-refunds): the refunds side. Since the P2-16 race fix, the merged history has not otherwise changed these files, so the refunds side is a superset. | — | — | Yes |
| 5 | `rho:rev:bondAdmin` refuses every command under the `from_data` mark, as FX-9 does for `rho:rev:wallet` and `rho:rev:bond`. | Leave it, because only PoS holds the name. | Lane A1's third requirement: native draws are refused under the mark. A release or a slash is a move. | Yes |

## Lane A1's merge requirements

| Requirement | Where it holds | Test |
|---|---|---|
| Keep the F30 check order | `check_region_signature`, decision 2 | `hash_seal_spec`: FX-4's F18 tests and this lane's two records-on F18 tests |
| FX-3's cosigner verification for origin signers | The origin's signers are the deployer and `DeployData.cosigners`. `DeployData::from_deploy`, which play (`runtime.rs`) and replay (`replay_runtime.rs`) both use, fills that field with `Cosignature::verified_cosigners`. The gate uses the same function. | `region_spec` cosignature tests |
| Native draws refused under the `from_data` mark | FX-9 (`rho:rev:wallet`, all commands but the reads, which includes `spendCompute` and `replaceHook`; `rho:rev:bond`, all but `balance`) and decision 5 (`rho:rev:bondAdmin`) | See below |
| The mark kept on refund records | The storage rule keeps the whole `SealRecord`, with `from_data`, for each item | `storage_rule::tests::a_refund_record_keeps_the_mark_of_data_received_at_run_time` |
| Run the named spec files on the merged code | — | See "Verification" |

### Native draws on the routes of revision 6

`seal_record_spec`'s route tests send each payload into Alice's code by each of the eleven routes. Two payloads are added: a `rho:rev:wallet` transfer from Alice to Bob, and a `rho:rev:bond` bond of Alice's funds. On every route, each native call must run and answer with a refusal, and no wallet changes. A route that never runs the payload therefore fails the test. With FX-9's check switched off, all eleven route tests fail.

`seal_wiring_spec::a_bond_admin_call_inside_data_received_at_run_time_moves_nothing`: PoS's name, held at genesis, is used inside data sent to an evaluator. The slash is refused and the bond stays. The same call in the holder's own code slashes.

## Post-merge fixes (separate commit)

- `hash_seal_spec`: this lane's records-on F20 test from FX-8 is removed. FX-7 decision 6 admits a `# P` that binds `deployerId` (an author-bound name). FX-7's own test, `a_hash_sealed_region_may_not_read_the_running_deployment`, already checks the current rule (runner-context names refused under a `Process` record) with records on. The rule now sits in URN resolution, not in `check_region_signature`.
- `region_spec` (FX-3 tests): two terms drew on the cosigner's wallet at the top level. Under FX-7's seal reading (F6), a draw must sit in a region that the wallet's owner sealed. The transfers are now in `{% … %}[c]`, as FX-7 changed its own tests. The cosigner now also pays the region's phlo (Decision 59), so the balance check uses the settled charge.
- `gated_executor_spec`: one more FX-7 test used the older cosignature API.
- The cross-host fixture is written again. FX-7 and FX-10 change the v6 genesis contracts, so the earlier genesis does not replay.
- The runbook gets two preconditions: clear `/tmp/casper-shared-lmdb-*` before each full run, and `touch models/build.rs` when checkouts share a target directory.

## Verification (release, stable 1.97 with `RUSTC_BOOTSTRAP=1`, at 13e0abd85)

Named spec files on the merged code: rholang `seal_record_spec` 21, `seal_wiring_spec` 7, `hash_seal_spec` 11, `rev_bond_spec` 2, `rev_genesis_spec` 10, `signed_region_spec` 13, `slot_spec` 4, `storage_left_in_space_spec` 5. casper `region_spec` and `gated_executor_spec` in `v6::`. All pass.

Full suites, `/tmp` cleared of LMDB directories before the run:

- rholang lib: 1,447 passed, 0 failed.
- rspace_plus_plus: all pass.
- casper lib: 847 passed, 0 failed.
- casper `--test mod`: 1,243 passed, 0 failed, 20 ignored.
- node: 466 passed, 0 failed.

## FX-11 (2026-11-02)

FX-11 (1b2d387e1, Decisions 70, 72 and 73) merged last, at 2e9339435. Two conflicts:

- `system_processes.rs`: FX-11 adds the same `rho:rev:bondAdmin` refusal under the data mark as this branch's decision 5, with a message for each command. FX-11's form is kept, and the check appears once.
- `seal_wiring_spec.rs`: both sides appended a test. Both are kept.

The PoSV6 tests and the cross-host fixture merged with no textual conflict, and the fixture still replays. One post-merge fix (c41ccb87b): FX-11's records-on genesis tests read a run's post-root, which P3-06 removed. They now read the merged state.

## Verification of the tip (c41ccb87b, `/tmp` cleared before the run)

- rholang lib: 1,447 passed, 0 failed.
- rspace_plus_plus: all pass.
- casper lib: 847 passed, 0 failed.
- casper `--test mod`: 1,250 passed, 0 failed, 19 ignored.
- node: 466 passed, 0 failed.

## FX-13 (2026-11-03, plan r8.10)

FX-13 (ecd2b0169, Decision 77) merged at e17e3ed32. Decision 77 removes the `from_data` mark. One conflict, in `system_processes.rs`:

- FX-11's refusals under the old mark are dropped: `received_refusal`, `received_at_run_time` and the `rho:rev:bondAdmin` check.
- The no-record rule in `wallet_authority` stays (Decision 70). A command with no seal record, when records are on, has no wallet authority.

The other files merged with no textual conflict. FX-13's versions of `seal_record_spec` and `seal_wiring_spec` are kept.

| Decision | Alternatives | Reason | Reversible |
|---|---|---|---|
| Keep FX-13's spec files over FX-11's mark tests | Keep both | The mark does not exist after Decision 77, so FX-11's mark tests have no subject | Yes |

## Verification of the tip (e17e3ed32, `/tmp` cleared before the run)

- rholang lib: 1,446 passed, 0 failed. One test less than at c41ccb87b: FX-13 removes `a_refund_record_keeps_the_mark_of_data_received_at_run_time` in `storage_rule.rs`.
- rspace_plus_plus: 421 passed, 0 failed.
- casper lib: 847 passed, 0 failed.
- casper `--test mod`: 1,250 passed, 0 failed, 19 ignored.
- node: 466 passed, 0 failed.

## D76, FX-12 and FX-14 (2026-11-03, plan r8.11 and r8.12)

Merged in this order: D76 at 1502a4e98 (with lane C's D79), FX-12 at d7537c771, FX-14 at e5c574656. Checked against lane A1's revision 12 and 13 merge requirements.

| Requirement | Result |
|---|---|
| 1. The gate table and D79's `BlockPurses` | D79's two `region_spec` tests and the grant test pass after the merge. |
| 2. `comm_ledger.rs` | D79's `comm_cells` call site and FX-12's work map are both kept. `reset` clears the surfaces and the work. |
| 3. `executor.rs` | `run` and `replay_one` keep the work tally and the funding oracle. `set_funding(None)` runs before each runtime goes back to the pool. |
| 4. Cross-host fixture | Written again after the FX-14 merge with `write_cross_host_fixture`. |
| 5. FX-14 after FX-12 and D79 | Done. The FX-14 merge had no conflict. |
| 6. `for` form in a self-sealed envelope | Known issue under Decision 88 (not a §6.3 divergence, not High). |
| 7. Suite results | Below. |
| 8. F41 | `charge_recorded_work` calls `seal::purse_of`. |
| 9. Funding oracle in the close | Not installed. The close has no match filter. FX-15 (Decision 86) decides it. |
| 10. FX-12 open list | Updated in the FX-12 log. |

FX-12 was cut from D76 before the rebase onto e17e3ed32. For each conflicted file, the rebased D76 is kept, and FX-12's own metering diff (aa47795d4) is applied on it. One assertion block from FX-12's FX-13 merge (the split of the run's work between the sender and the owner) is added again to `region_spec`.

`admits_waiting` (lane C) is accepted as the hook. It runs in `extract_first_match`, which play produce, play consume and replay produce use. Replay consume (`locked_consume` → `run_matcher_consume`) matches the incoming receive against stored data, so it has no stored receive to filter. The incoming receive belongs to the running envelope, and `EnvelopeFunding` funds it without a test. The gap in the D79 hook memo is therefore closed.

One fix in lane C's code: `admits_waiting` was put between the guard's doc comment and `check_commit`, so the comment was on the wrong function. The function now comes before the comment.

| Decision | Alternatives | Reason | Reversible |
|---|---|---|---|
| Keep the rebased D76 in conflicts and apply FX-12's own diff | Take FX-12's side, or resolve hunk by hunk | FX-12 carries the pre-rebase D76. Its own changes are one commit. | Yes |
| Accept `admits_waiting` | Replace it with a `FundedReceive` trait | It is at the hook point, and play and replay both use it | Yes |

**Decision 88 labels:** no new finding is blocking. The misplaced doc comment was fixed in the merge.

v6 tests on the merged tree: 85 passed, 0 failed, 7 ignored.

## Verification of the tip (aea9b529d, `/tmp` cleared before the run)

- rholang lib: 1,451 passed, 0 failed, 3 ignored.
- rspace_plus_plus: 421 passed, 0 failed.
- casper lib: 851 passed, 0 failed, 1 ignored.
- casper `--test mod`: 1,260 passed, 0 failed, 19 ignored.
- node: 466 passed, 0 failed.
