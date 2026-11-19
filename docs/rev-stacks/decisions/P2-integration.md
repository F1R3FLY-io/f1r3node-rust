# P2-integration decisions (Decision 38)

Branch `patch/P2-integration`, lane A2, cut from `patch/P1-12` (abba9ff2a).

## Merges

| Order | Source | Tip | Carries |
|---|---|---|---|
| 1 | `patch/P1-13` (lane A1) | 9c2e2afd0 | Offer status, receipts, the rejection log |
| 2 | `patch/P2-21` (lane B) | 9d43b2349 | P2-02 to P2-11, P2-14, P2-15, P2-20, P2-21 |
| 3 | `patch/P2-16` (lane A2) | 9a71bf770 | Storage of unmatched sends and receives |
| 4 | `patch/P3-07` (lane C) | e38938b99 | P2-17 to P2-19, P3-01 to P3-03, P3-07 |

Merges 2 to 4 are done in two steps. First, lanes B and C are merged on a local branch (`tmp/lanes-bc`, commits a7076a01a and e25f02265). These branches share their history from dev through P2-15, so the merge is an ordinary three-way merge. Second, that branch is merged into this one (954272af0).

## Conflict resolutions

| # | Where | Decision | Alternatives | Reason | Reversible |
|---|-------|----------|--------------|--------|------------|
| 1 | Merge base of step 2 | The merge uses 6354984f6 (the original `patch/P1-04b` that lane B merged) as its base, not the git merge base (dev). | Merge with the dev base. | Lanes B and C start from dev and the original P1-01 to P1-04b. This branch starts from A1's `patch/P1-rebased`, which changes only commit hashes. With the dev base, 18 files give add/add conflicts that hide the real changes. With 6354984f6, 4 files conflict, and each side's diff is its real work. | Yes |
| 2 | `v6/core.rs`, `v6/store.rs` (P1-13) | `CoreParams` gets P1-13's `retained_records`. It bounds the memory record store (`Retention::LastBlocks`). `store::DEFAULT_RETAINED_RECORDS` is `casper::V6_RETAINED_RECORDS_DEFAULT` (1024). P1-13's separate retained map in the core is not kept. | Keep P1-13's map beside the store. | P1-07 and P1-11 already keep the fixed records in the record store, with the same bound and the same source tests. Two copies would hold the same records. | Yes |
| 3 | `V6Engine::new` (P1-13) | `retained_records` is the last argument, after the lag threshold, the fallback distance and the policy. The persistent LMDB store keeps each fixed record as a pointer to its citing block (P1-11, Q-7), so the bound does not limit it. A record is `notRetained` only when its citing block is pruned. | Apply the bound to the LMDB store. | The LMDB store has no position index by block. Q-7 already decides how long a record is kept. | Yes |
| 4 | `v6/mod.rs`, `tests/v6/mod.rs` | Union of the modules of both sides. | — | Both sides add modules. | Yes |
| 5 | `v6/executor.rs` | `BlockExecutor` keeps both builders: `with_slash_source` (P1-05) and `with_boxcar` (P2-05). | — | Independent. | Yes |
| 6 | `v6/engine.rs` | The propose path keeps the lag gate (P1-08) and record packing (P1-12), and uses lane B's effective price (P2-07) as the pool's minimum price. The engine keeps both `lag_gate` and `price_hook`. | — | Independent. | Yes |
| 7 | `v6/engine.rs` | The pool is read with this branch's `deploy_storage.read_all()`. Lane B's `.lock()` comes from an older dev. | — | M0's deploy storage needs no lock. | Yes |
| 8 | `p_new_normalizer.rs` (M0 against P2-18) | M0's generated `ReceiveBind` gets `cost_signature: None`. | — | Lane C added the field after M0 wrote the bind. A generated bind has no annotation. | Yes |
| 9 | `tests/v6/gated_executor_spec.rs` (P1-06 against P2-05) | The three `Body` literals get `v6: None`. | — | P1-06 added `Body::v6`. | Yes |
| 10 | Lanes B and C, `rev_stack/mod.rs` and `native.rs` | Union of the imports. `transfer` checks that the payer signed (P3-02) before it journals (P2-21). | Journal first. | A transfer that the payer did not sign must not enter the journal. | Yes |
| 11 | Lanes B and C, `reduce.rs` | Git's three-way merge, unchanged. | — | No conflict. | Yes |

## Wiring

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 12 | `v6::inclusion::GateScreen` plugs lane B's gate into the `InclusionScreen` seam (P1-09). It runs `gate_block` with `PhloLimitDemand` at the attested root, using the executor's own gate inputs (named signatures, joint wallets). `V6Engine::new` installs it. `AdmitAll` stays for `with_parts`. | A separate gate copy in the engine. | The inclusion check and the block's own gate must agree. | Yes |
| 13 | Decision 44: after selection, `create_block` screens the selected envelopes with the same screen at the attested root. For each refused envelope it records a rejection (signature, attested root, next block number, reason) through the deploy storage, which also removes the offer from the pool. The block leaves the envelope out. | Leave refused envelopes in the block as `NotRun`. | Decision 44. A refused envelope does not reach a block. | Yes |
| 14 | If the screen cannot run, the proposal keeps its envelopes, and the block's own gate decides. A warning is logged. | Fail the proposal. | A transient read error must not stop proposals. The block's gate gives the same verdicts. | Yes |
| 15 | The reason text comes from `inclusion::refusal_reason`, and it is cut to `offer-rejection-reason-bytes` by the log. | Debug format. | It is readable through the offer status route. | Yes |
| 16 | The screen is a free function, `inclusion::screen_offers`, so that it can be tested without a v6 test network. | A method on the engine. | P1-14's harness is not on this base. | Yes |

## Journal against payer-must-sign (P2-21 and P3-02)

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 17 | `rho:rev:wallet transfer` first refuses a payer that did not sign, then journals when a journal is open. Both rules hold for a journaled transfer. | — | Decision 10 above. | Yes |
| 18 | Lane B's `rev_reserve_spec` sets the deployment's signers before each run (lane C's `sign_as`). A new assertion shows that, with the journal open, a transfer that names another deployer's id is refused and leaves no journal entry. | — | Without signers, every transfer in that spec was refused by lane C's check. | Yes |
| 19 | A REV stack literal (P2-19) journals its moves when a journal is open: `journal::journaled_transfers` checks the whole stack against `balance + credits − debits − held` and records every cell, or none. | Leave stack literals native. | Without the journal, the v6 merge drops the stack's moves (P2-19 open item 3). | Yes |
| 20 | A kind stack (P3-07), whether a mint or a transfer, still writes natively. | Journal kind transfers. | A journal entry cannot carry a mint, and the conservation check would see a kind transfer as a REV move. This needs a ruling (memo H). | Yes |
| 21 | `WalletOwner::from_canonical_bytes` (P2-21, moved from `hooks.rs`) decodes a compound owner (P3-02). | — | The journal stores owners in canonical bytes. A transfer from a joint wallet put a compound in the journal, and closing the journal failed with "a journal entry is malformed". | Yes |
| 22 | A stack cell that names the writer's own wallet counts toward the amount that the writer must cover, but it records no journal entry. | Record a self-move. | The native path moves nothing for such a cell either, and settlement gets no empty moves. | Yes |

## Tests

- Un-ignored: `v6::region_spec::native_rev_writes_of_a_run_survive_the_merge`. It now reads Bob's balance at the settled root only. With the journal, a run's post-root holds no move.
- `v6::region_spec::a_stack_literal_moves_one_unit_per_cell_and_replays`: the settlement carries 2 units of moves (the cell on the writer's own signature records none), and Alice's settled balance is 100,000 − 2 − her charges. The check of Alice's post-root balance was removed for the same reason.
- `v6::region_spec::a_joint_wallet_is_spent_only_by_all_its_signers`: both parts read the settled root. The joint wallet holds 50,000 − 700 − its charge, and Bob holds 700. After the leak, the joint wallet loses only its charge. P3-03 read the post-root because of P2-19 open item 3.
- New: `rev_stack::owner::tests::a_compound_owner_survives_its_canonical_bytes`.
- New: `v6::offer_screen_spec::a_refused_offer_reports_rejected_and_leaves_the_pool`. On a v6 genesis, an unfunded offer is refused, leaves the pool and is in the rejection log with the attested root, the block number and the reason. This is the state that `offered_pool_status` reports as `rejected`. A funded offer stays.
- `rholang/tests/rev_reserve_spec.rs::an_open_journal_holds_transfers_out_of_the_state` (decision 18).
- `casper/tests/util/rholang/system_vault_v6_test.rs` (P2-11): the user runs set their signer first (`eval_signed`), for the same reason as decision 18. The facade transfer failed with "the payer must be a signer of this deployment" in the full run.

## Suites (2026-10-31, release, stable 1.97 with `RUSTC_BOOTSTRAP=1`)

- rholang, all targets: 2,373 passed, 0 failed, 70 ignored.
- casper lib: 826 passed, 0 failed, 1 ignored.
- casper `--test mod`: 1,194 passed, 1 failed (`system_vault_v6_test::the_facade_transfers_natively_and_reads_balances`, fixed as above; its spec then passes), 13 ignored.
- clippy (casper, rholang, with tests): no warning in a file this branch touches.
