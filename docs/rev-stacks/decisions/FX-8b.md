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
