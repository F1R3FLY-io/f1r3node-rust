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

## FX-14 follow-ups (2026-11-03)

cc3168032 merged at 11c9cb42f. It carries FX-14's follow-ups (2bfaaf942: receipts are spent on use, a validator takes its handle at its own seal, genesis handles; Decisions 84 and 87) and FX-12 at eab30238f (the client's phlo limit counts only its own surfaces). One conflict, in the FX-12 log: the status section now says that eab30238f closes the phlo-limit item.

v6 tests: 88 passed, 0 failed, 7 ignored. `rev_bond_spec`: 3 passed. The cross-host fixture still replays.

**Decision 88 label, known issue:** `FOREIGN_WORK_BUDGET` bounds foreign work in a run with a fixed amount. Decision 86 bounds it by the membrane's wallet instead. FX-15 replaces the fixed amount.

## FX-15-close and the refund-to-payer fix (2026-11-04)

`patch/FX-15-close` at 34e6c842d merged. It carries `patch/FX-15` at 432dc390f (a storage refund at a COMM goes to the payer of the stored item), FX-16 at bee4c599f (the linear close, Decision 107), lane B's epoch fix efb0e3daf and D107, and lane C's test `close_charge_spec::substitution_into_a_continuation_fired_in_the_close_is_not_charged_to_the_proposer`.

| Decision | Alternatives | Reason | Reversible |
|---|---|---|---|
| `CostManager::charge` stays as on efa511e93 (lane B's c345b400d): the meter decides gated charges first, and there is no foreign-work limit | Lane C's resolution, which the auto-merge brought back with `own_origin` and the `foreign` budget | Decisions 91 and 103: gated work is bounded by its wallet, and ungated work counts against the client's limit | Yes |
| `close_charge_spec`: keep `no_deployment_makes_the_close_fail` and lane C's substitution test | Take one side | The tests are independent | Yes |

v6 tests: 104 passed, 0 failed, 10 ignored. `seal_wiring_spec`: 3 passed. `rev_bond_spec`: 9 passed.

## Merge of `origin/dev` (Decision 95, 2026-11-04)

`origin/dev` at 657156f98 merged into FX-8b after FX-15 and FX-15-close (f7af68ca8). `dev` moved 37 commits past the dry run's target (746a1c368). Issue #730 (trusted deploy-store reads) adds five conflicts that the dry run did not see. The dry run's sections 2.1, 2.2 and 3 apply as written.

| Decision | Alternatives | Reason | Reversible |
|---|---|---|---|
| Take `dev`'s deploy codec (signature checked at write, Blake2b fingerprint checked at read) and remove A4's verify-once cache, `from_stored_proto` and the `deploy-verified-encodings-cap` setting | Keep the cache beside the fingerprint | Both remove the per-read signature check. `dev`'s form needs no process-wide set and also covers restore and quarantine. Two mechanisms for one job is slop. | Yes |
| Check named cosigners in the codec's `verify_signature`, which the write path and the restore path both call | Check them only in `from_proto` | `dev`'s read path no longer calls `from_proto`. Without this, a stored deploy that lacks a named cosigner is read back as valid. FX-3 refused it on read. A missing cosigner maps to `signature_verification_failed` at admission. | Yes |
| `from_proto` = `from_proto_unverified`, then the signature check, then `with_complete_cosignatures` | `dev`'s `from_proto` without the cosigner check | Network input keeps FX-3's rule | Yes |
| `PreparedDeploy` carries the signature, and every insert path clears the offer-rejection entry | Clear only in `add` and `add_if_absent` | `dev`'s admission inserts through `add_prepared_if_unchanged`. FX-8b's rejection log must clear there too. | Yes |
| `reserve_deploy`: `dev`'s snapshot loop, with A3's pool cap checked under the rejected-buffer guard just before the insert. `Reservation::{Added, Refused, PoolFull}` keeps the `pool_full` lifecycle event. | `dev`'s `Option<DeployError>` and no cap | P0-00 A4 keeps the cap in the admission critical section. `dev`'s guard is that section. | Yes |
| Seven dry-run calls plus two that FX-15-close added in `pos_bond_admin_spec.rs` pass `Vec::new()` as the fourth argument of `play_exploratory_deploy` | — | Dry run section 3 | Yes |
| `web_api.rs` per section 2.2: `balance_query_term` validates the address, then returns `BALANCE_RHOLANG_TEMPLATE` with the `address` parameter (Legacy) or `native::balance_term` with no parameter (Native) | — | Ruled in the dry run | Yes |

**Decision 88 labels:**

- Known issue, not blocking: the Native balance path puts the validated address in source text (dry run section 2.2). The validation refuses every string that is not an address, and `balance_query_rejects_addresses_that_are_not_valid` proves it.
- Known issue, not blocking: an operator config that still sets `deploy-verified-encodings-cap` is ignored, because the field is gone and the casper config does not deny unknown keys.

### Fixes found by the full suites on the merged tree

| Decision | Alternatives | Reason | Reversible |
|---|---|---|---|
| Remove `cost.record_comm()` from `handle_result` and remove `CostManager::record_comm` | Add a surfaces-only ledger call for D8b | D8b's `on_comm_between` (02f70434e) already counts the COMM, so play counted each COMM twice. `rholang/tests/comm_ledger_spec.rs` found it. Production reads only the surfaces and the work, so no root or charge changes. | Yes |
| Add `cosignatures` and `required_cosigners` to `dev`'s new `DeployData` fixtures in `block-storage` and `web_api.rs` tests, and close the `web_api.rs` test function that the conflict split | — | Compile fixes | Yes |

**Decision 88 label, known issue:** the double count was on FX-8b since efa511e93. It is fixed here. `rholang --test` targets were not in the suites that ran on FX-15-replay, which is why it was not seen there.

`cargo fmt --check` did not run faithfully: the pinned nightly cannot be downloaded in this environment, and stable rustfmt ignores the repository's unstable options, so it reports diffs across untouched files. The lines added in this merge are under 100 columns.

## Last merge conditions (2026-11-04)

| Decision | Alternatives | Reason | Reversible |
|---|---|---|---|
| Take `region_spec::the_gate_refuses_a_seal_that_resolves_only_at_run_time` from dea48f13d. Its settle change is already on FX-8b. | Merge the whole commit | Only the test is missing | Yes |
| Write `pos_v6_spec::the_one_expression_active_set_pick_agrees_with_the_list_ops_pick` here. It runs the old pick (`toList`, `ListOps` `map` with `fst`, `take`, `toSet`) and the new one (`keys().toList().take().toSet()`) on 35 cases: 0 to 30 bonds with random 65-byte keys, and 1 to 100 active validators. Both give the same set in every case. | Wait for lane B | No branch on `origin` has lane B's test. Lane B can replace this one. | Yes |

Both tests pass.

**Finding for F39 (lane A2's confirmation that lane B asked for). Known issue, not blocking:**
- The gate does **not** refuse every seal that resolves only at run time. A pure stack body under a seal bound at run time is admitted: `for (s <- ch) { {% slot :: () %}[s] }`. `region::stack_payer` reads an unresolved bound seal on a stack body as a token kind, so it names no wallet.
- At run time, `split_kind` finds no kind, and `check_region_signature` refuses the seal unless its owner is a signer of the deployment or is held (`held_or_signed`). So no wallet outside the deployment's signers is reached, and authority holds.
- What is lost: the gate makes no commitment for that signer's wallet. Work under the stack is still bounded by the meter's per-block ledger, which keeps each draw within the wallet's free balance.
- Fix options, for a ruling: (1) the gate names an unresolved bound seal on a stack body as an unresolved owner, which refuses it. This also refuses a token kind passed in at run time. (2) Keep the run-time check as the only guard and record it under F39. F39 and F33 stay open until this is ruled.

**Ruling (Decision 112, 2026-11-04):** option 2. The run-time check (`check_region_signature`, `held_or_signed`) is the guard. The gate does not refuse a seal whose owner is bound only at run time. F39 and F33 are closed. `region_spec::the_gate_refuses_a_seal_that_resolves_only_at_run_time` records the shapes that the gate does refuse. The pure stack body above is admitted by design.

Lane B's own equivalence test with random keys is kept beside `pos_v6_spec::the_one_expression_active_set_pick_agrees_with_the_list_ops_pick`.

## FX-17 merged (2026-11-05)

`patch/FX-17` at 0b01b1663 is merged: D10 and D11 (P11, the order floor and P5), the void records and Decision 109, F52, the traced (d) close, lane B's withdrawer rewrite and Decision 114, and lane C's FX-15 follow-ups (the deterministic storage reservation).

Full suites at 0b01b1663 (release, `/tmp` cleared before each crate): rholang 2,463 passed, 69 ignored; rspace++ 423, 7 ignored; block-storage 320; models 338; shared 130 and 1 failed; crypto 118; comm 437 (rerun; see below); graphz 48; casper lib 900, 1 ignored; casper `--test mod` 1,314 passed, 1 failed, 21 ignored; node 469, 1 ignored.

**Labels (Decision 88):**
- Known issue, environmental: `shared::bounded_file::a_failed_rotation_keeps_the_active_file_writable`, as at 6c43a2c8f (root).
- Known issue, timing: `comm::f1r3fly_server::resource_tests::dropping_incoming_releases_listener_and_stalled_handshakes` failed once in the full run. It passes 3 of 3 alone, and comm is unchanged since 6c43a2c8f. The full comm rerun gives 437 passed.
- Known issue, timing under load: `multi_node::v6_engine_state::a_long_run_keeps_executor_memory_flat_and_old_receipts_available` fell below its floor of 25 in the full parallel run. Alone it fixes 34 and passes. Its count depends on machine load, not on the code.

## FX-20 and FX-19 merged (2026-11-05, Decisions 119, 120, 124 to 126)

- `patch/FX-20` at 91e3a02b7 is merged (275eccde7): κ in record field 20, no close charge, tag 1 reserved.
- Lane C's `patch/FX-19` at 10e218add is merged: the declared per-firing bound, and the refusal machinery removed from the membrane. FX-19 was cut from FX-17, so `executor.rs` had six conflicts. The resolution:
  - `close_gate` takes FX-19's `replay` flag and returns only the `SystemGate`. `CloseFunding` stays for stored code that the close fires. The gate carries FX-20's κ cell, and not FX-17's `CloseCharge`.
  - `replay_after` keeps FX-20's κ check and drops `accept_metered` on the close, as FX-19 does.
  - `replay_recorded` takes FX-19's signature without the pre-charge argument.
- **Cross-host fixture.** FX-19's regenerated fixture has no κ, so its replay fails with "the close's cost". The fixture is written again on the merged tree. This is the same cause as FX-20 D4.
- Results on the merged tree: casper lib `v6::` 126 passed; `--test mod v6::` 123 passed, 9 ignored; `multi_node::v6` 11 passed.

## FX-19-format and FX-22 merged (2026-11-05)

- `patch/FX-19-format` at e1073055d is merged (a61d45834): `Event::Refusal` and `RefusalEventProto` are removed, and `EventProto` tag 4 is reserved.
- `patch/FX-22` at 369cad52b is merged: the gate commits min(`phloLimit`, the static estimate), and each run is limited to it. The loose estimate stays for the handoff (Decision 128). `executor.rs` had six conflicts with FX-19. The resolution keeps FX-19's metering (`EnvelopeFunding` with the admitted-run count, no refusal log, no `BlockLedger`) and FX-22's `committed` argument to `run` and `replay_one`.
- **Two FX-19 tests changed.** `region_spec::an_installer_that_cannot_hold_the_bound_is_not_fired_and_pays_nothing` and `each_run_holds_at_most_its_share_of_the_installers_free_balance` held the installer's balance with a `Nil` deployment and a large `phloLimit`. Under FX-22, `Nil` commits only its static estimate, so the balance stayed free and the calls fired. The tests now hold the balance with `@"hold"!(0)`. That deployment sends on a public channel, so it has no estimate and commits its `phloLimit`.
- **Interaction, known issue (Decision 88).** FX-19 seals stored code with the installing deployment's `phloLimit`. The run evaluates a copy limited to Δ, so for a deployment in the estimated fragment the seal carries Δ. Reason this has no effect now: in the fragment every channel is a private name that cannot leave the term, so its stored code can never fire.
- Results on the merged tree: casper lib `v6::` 127 passed; `--test mod v6::` 126 passed, 9 ignored (with the two tests above changed); `multi_node::v6` 11 passed; `forms_spec` 5 passed; `rholang --test delta_sigma_spec` 8 passed.

## FX-19 (Decisions 132, 133) and FX-21 (Decisions 130, 131) merged (2026-11-06)

- **Order.** The `dev` merge waits on lane B's parser (Q-35, `memos/Q-35-parser-master.md`): `dev` 3b5daabb4 needs rholang-rs master 4e22339, and `rev-stacks/syntax-dev` at 743752b has master only to dd6f611. FX-19 and FX-21 are merged first. The resolved `dev` merge is kept on the local branch `wip/dev-merge`. Reversible: the end tree does not depend on the order.
- `patch/FX-19` at 59cb1f0c5 (Decision 132) is merged (3ce70bd40). Then `patch/FX-21` at 0991d5768 is merged (a8d1d0f57). Then `patch/FX-19` at 6214a1053 (Decision 133, which includes FX-21) is merged.
- **FX-22 under FX-21.** FX-22's `limited()` copy set the deployment's `phloLimit` to Δ, so the default seal limit became Δ. Play now calls `RuntimeOps::process_deploy_limited(envelope, Δ)`, and replay sets `run_limit` to Δ around `replay_deploy_e`. The record keeps the signed envelope, and the seal keeps the signed `phloLimit`. This withdraws the interaction noted at the FX-22 merge.
- **Decision 134 test.** `close_charge_spec::a_firing_set_off_by_the_close_over_its_bound_fails_the_close_and_the_void_record_replays`. A bond deployment (limit 200,000) installs a handle continuation that the close fires. The body doubles a string 18 times, so its own evaluation passes the bound without a COMM. The close fails with `FiringExhausted`, the block is void for reason (d) with κ = 0, the deployment's own entry stays `Completed`, and the record replays under 1, 2 and 4 workers.
- **What a failed run is charged (for lane A1).** Two numbers differ:
  - The record's `compute_phlo` for a failed run is the run's evaluation limit, min(Δ, `phloLimit`) = Δ, for every error kind that dev charges in full (`InterpreterImpl::handle_error`). Parser errors and `OperatorNotDefined`/`OperatorExpectedError` record the parsing cost, and user aborts and `if` type errors record the phlo used. Test: `gated_executor_spec::a_run_never_spends_more_than_its_commitment` (`1 / 0`, Δ = 5,014, `phloLimit` 1,000,000: `compute_phlo` = 5,014).
  - The wallet debit comes from the settlement, which charges the cells and the work that the run's membrane metered, capped at Δ. In the same test the failed run is debited 102, not 5,014.
  - *Known issue (Decision 88):* for a failed run the record's `compute_phlo` and the settled debit disagree. Reason it is not blocking: the debit never passes the commitment, and replay checks both numbers. Which number a failed run should pay is a charging decision for Meredith.
- **Cross-host fixture.** Both FX-19 and FX-21 wrote it again. It is written again on the merged tree.
- Results on the merged tree: casper `--test mod v6::` 143 passed, 9 ignored; `multi_node::v6` 11 passed; `forms_spec` 15 passed; rholang `delta_sigma_spec` 8, `elaboration_spec` 20, `seal_record_spec` 7 passed (1 ignored).

## Full suites at e3edaf623 (2026-11-06, `/tmp` cleared before each crate)

| Crate | Passed | Failed | Ignored |
|---|---|---|---|
| rholang | 2,475 | 0 | 70 |
| rspace++ | 423 | 0 | 7 |
| block-storage | 320 | 0 | 0 |
| models | 339 | 0 | 0 |
| shared | 149 | 1 | 0 |
| crypto | 118 | 0 | 0 |
| comm | 437 | 0 | 0 |
| graphz | 48 | 0 | 0 |
| casper lib | 890 | 0 | 1 |
| casper `--test mod` (nextest, one process per test) | 1,336 | 0 | — |
| node | 469 | 0 | 1 |

- *Known issue, environmental:* `shared::bounded_file::a_failed_rotation_keeps_the_active_file_writable` fails because the tests run as root, as at 6c43a2c8f.
- **The SIGSEGV hunt.** casper `--test mod` ran under `cargo-nextest`, which runs each test in its own process and names any test that a signal ends. 1,120 tests passed before an environment restart, and the other 216 passed in a second run. No test ended on a signal. An earlier nextest run on 818e72d6c passed 345 tests with no signal, before the disk filled. A `--test-threads=1` run takes about 16 hours here, and each environment restart ends it, so it was replaced by the nextest runs. *Known issue:* the one SIGSEGV seen in lane C's environment is not reproduced.
- **`cargo fmt --check`.** The pinned nightly (`nightly-2026-02-09`) cannot be downloaded here, and stable rustfmt ignores the unstable options in `rustfmt.toml`, so its result is not authoritative. With stable rustfmt, `dev` at 3b5daabb4 gives diffs in 524 files and FX-8b in 609 files. 89 files have diffs on FX-8b that `dev` does not have. CI's formatting job decides.

## Decision 136: a failed run pays its commitment (2026-11-06)

- `patch/FX-19` at 821c4bee0 is merged (7e8743b53). It adds Decision 133 to the log and updates `region_spec`.
- **Settlement.** In `settle::charges`, a failed run no longer pays the cells and work that its membrane metered. It pays its recorded cost, `run.compute_phlo`, capped at Δ, from the client's wallets (the payers that pay a deployment's own unsealed work).
  - For the errors that `dev` charges in full, `compute_phlo` is the run's evaluation limit, min(Δ, `phloLimit`) = Δ. So the debit is Δ × price.
  - Parser errors and `OperatorNotDefined`/`OperatorExpectedError` keep the parsing cost. User aborts and `if` type errors keep the phlo used. These come from `dev`'s `InterpreterImpl::handle_error`, unchanged.
  - The record's `compute_phlo` and the debit are now the same number. This withdraws the known issue recorded at e3edaf623.
- **Test.** `gated_executor_spec::a_failed_run_pays_its_commitment_and_a_run_never_spends_more`: `1 / 0` with `phloLimit` 1,000,000 and Δ = 5,014 records 5,014 and is debited 5,014. The replay gives the same settlement, and a record whose cost is changed to Δ − 1, Δ + 1 or 1,000,000 does not replay.
- Results: casper lib `v6::` 127 passed; `--test mod v6::` 143 passed, 9 ignored; `multi_node::v6` 11 passed; `forms_spec` 15 passed.

## `dev` merged (2026-11-06)

- `dev` at 3b5daabb4 is merged. The parser pin is lane B's `rev-stacks/syntax-dev` at ad15ad2b10c2e9aeb2857f5e27f7de8f7c22ebdd, which merges rholang-rs master 4e22339 into 743752b (Q-35, Q-37). CLAUDE.md's parser note names the pin and the branch.
- Conflicts: `Cargo.toml` (the pin), `Cargo.lock`, the import lists of `matcher/exports.rs` and `substitute.rs` (both sides' types), and `p_simple_type_normalizer.rs`, where `dev`'s `SimpleType::Type` connective replaces the branch's refusal. The branch's refusal of `Proc::FixedPointPattern` in `normalize.rs` is removed, because `dev` normalizes it.
- Results: rholang 2,549 passed, 71 ignored; casper `--test mod v6::` 143 passed, 9 ignored.

## Full suites on the final tip 9b54135f3 (2026-11-07, `/tmp` cleared before each crate)

The tip holds FX-17 to FX-22, FX-19-format, Decisions 119 to 136, and `dev` 3b5daabb4 with rholang-rs at ad15ad2.

| Crate | Passed | Failed | Ignored |
|---|---|---|---|
| rholang | 2,550 | 0 | 69 |
| rspace++ | 424 | 0 | 7 |
| block-storage | 320 | 0 | 0 |
| models | 339 | 0 | 0 |
| shared | 149 | 1 | 0 |
| crypto | 118 | 0 | 0 |
| comm | 437 | 0 | 0 |
| graphz | 48 | 0 | 0 |
| casper lib | 890 | 0 | 1 |
| casper `--test mod` (nextest, one process per test) | 1,338 | 0 | 21 skipped |
| node | 469 | 0 | 1 |

- *Known issue, environmental:* `shared::bounded_file::a_failed_rotation_keeps_the_active_file_writable` fails because the tests run as root.
- No casper test ended on a signal.

## Handoff notes for lane A1's final read

- **Formatting.** `cargo fmt --check` is not verified here: the pinned nightly cannot be downloaded, and stable rustfmt ignores the unstable options in `rustfmt.toml`. CI's formatting job decides.
- **Known issues (Decision 88):**
  - The one SIGSEGV seen in lane C's environment is not reproduced (nextest, no signal in 1,338 tests on this tip and in 1,336 on e3edaf623).
  - The `shared` test above fails only as root.
  - FX-22's static estimate is loose (Decision 128).
  - Terms outside the estimated fragment commit their `phloLimit` (Decision 122).
- **Parser.** rholang-rs is pinned to `rev-stacks/syntax-dev` at ad15ad2 (lane B, Q-35). The branch is not yet merged into rholang-rs master.
- **Open for P12.** FX-18 stays parked (F53).

## `dev` merge resolution (2026-11-12)

The merge target is `d57a3f4501513c699887af6e457607bc928c8faa`. The parser remains pinned to `ad15ad2b10c2e9aeb2857f5e27f7de8f7c22ebdd`.

- Keep `dev`'s block-ingest architecture and persistent DAG history. Keep protocol-6 execution, floor pacing, commit markers, and executor state.
- Write finalization metadata, history progress, commit markers, and the LFB pointer in one storage transaction. Keep the resident finalized cache bounded.
- Keep U256 stake weights and deploy prices. Native wallet amounts retain their protocol-6 bound, with checked conversions at each boundary.
- Export BigInt bonds and the four consensus parameters from PoSV6. Keep its native bond accounting in Int values.
- Keep Protobuf field 15 for `dev`'s U256 phlo price. Move the unreleased co-signature field to 17 and retain required signers at 16.
- Check named co-signers when records enter storage and when storage reads them. Retain fingerprint checks and the current record format.
- Keep the shared instructions in AGENTS.md and link the parser guidance from CLAUDE.md. Preserve existing evidence ledgers and signed acceptance packages.

The existing `exit_spec::write_cross_host_fixture` generator rewrites the three binary replay fixtures for the merged wire format and PoSV6 term.

Validation:

- `cargo check --offline --workspace --tests` passes.
- The initial release suite passed 6,010 of 6,023 tests. The final 294-test regression run passes, including all 13 previous failures and the added transaction-rollback check.
- The regenerated cross-host fixture replays successfully.
- `cargo clippy --offline --workspace --all-targets -- -D warnings` passes.
- Formatting checks pass for the resolution files. The full formatter reports 52 unchanged branch files, so this merge uses the documented `SKIP_FMT=1` hook option.
- References pass for the edited instructions and merge notes. The full Markdown check reports 149 anchor findings in the imported API and slashing documents.
- The new prose adds no STE Check findings. Human STE Review remains pending.

## CI follow-up (2026-11-13)

The full supply-chain check rejected `fuzz/Cargo.lock` under `--locked`. Its three parser packages still referenced `4e22339eca493c5ce3d14386c2af67af74bfffa2`.
The fuzz lockfile now uses the workspace pin, `ad15ad2b10c2e9aeb2857f5e27f7de8f7c22ebdd`, for those packages. Dependency versions and policy exceptions remain unchanged.

The repository formatter corrects the 52 remaining branch files. Workspace and soak-chart formatting checks now pass without a skip.
The full `scripts/ci/check-supply-chain.sh check` command passes for the workspace, fuzz crate, and soak charts.

Commit Policy rejects imported Mergify commits because the `ca-canary` base lacks the source validator. No sync PR was open during this check.
The existing `just ca-sync` workflow must transfer accepted `dev` history and policy before this ordinary PR can use them.
The [Commit Policy guide](../../ci.md#commit-policy) describes the trusted transfer route.
