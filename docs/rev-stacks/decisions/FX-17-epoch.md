# FX-17, the epoch change after a void boundary (lane B)

Decision 105. Plan r8.16. Base: `patch/FX-8b` at 4a74b0e34. Lane A2 owns the void mechanism in FX-17. This patch is lane B's PoS part.

## Problem

PoSV6's `closeBlock` started an epoch only when `blockNumber % epochLength == 0`. A void block runs no close. So when the boundary block was void, the epoch change was skipped until the next multiple of the epoch length. The active set, the release of quarantined withdrawers and the move of pending withdrawers waited a whole epoch.

## What this patch does

- PoSV6 keeps an epoch counter on a new private channel, `lastEpochCh`. Genesis sends `0`, the epoch of block 0.
- `closeBlock` reads the counter with a peek (`<<-`). The epoch change runs when `blockNumber / epochLength` is greater than the counter. The same close then sets the counter to `blockNumber / epochLength`.
- A close inside an epoch that has already started only reads the counter. The peek does not remove the datum, so these closes do not write the counter.
- `settlePendingBonds` still runs at every close, before the epoch test, as before.
- The quarantine formula in `withdraw` is unchanged. `removeQuarantinedWithdrawers` compares the quarantine end with the current block number, so a late epoch change still releases each withdrawer whose quarantine has ended.

## Test

`pos_bond_admin_spec::an_epoch_starts_at_the_first_close_at_or_past_its_boundary`, with an epoch length of 3:

1. Block 1: Alice bonds. She is bonded but not active (epoch 0).
2. Block 3 is void, so no close runs. Block 4 runs on the post-state of block 1. Its close starts epoch 1, and Alice is active.
3. Block 5: Bob bonds. Epoch 1 has started already, so Bob is bonded but not active.
4. Block 6: the boundary of epoch 2. Bob is active.

Each block replays to the same post-state root.

## Fixtures and roots

- The cross-host fixture (`casper/tests/fixtures/v6/cross_host/`) is regenerated with `exit_spec::write_cross_host_fixture`. The PoSV6 term changes the v6 genesis root.
- No golden root in the repository pins a v6 genesis. The legacy PoS contract does not change, so `genesis_test` and `LEGACY_GOLDEN_ROOT` keep their values.

## Decisions

| Decision | Alternatives | Reason | Reversible |
|---|---|---|---|
| A separate private channel for the counter | A key in the PoS state map | Decision 105 asks for a peek. The state map is taken and put back through `runMVar`. A separate datum can be peeked, so a close inside an epoch does not rewrite it. | Yes |
| The counter holds the epoch index (`blockNumber / epochLength`) | The block number of the last epoch change | The index gives the test directly and does not depend on which block started the epoch. | Yes |
| Genesis sets the counter to 0 | Leave it empty until the first boundary | Block 0 is in epoch 0, and genesis computes the active set for it. | Yes |

## Effects on other parts

- **Slashing:** `slashing_authorization` derives the epoch of a block by arithmetic (`block_number / epoch_length`). After a void boundary, PoS can still be in the earlier epoch while that arithmetic gives the later one. A v6 block runs no slashing today, so nothing reads the difference yet. **Known issue, not blocking:** it must be settled before v6 enables slashing.
- **The close bound (FX-16):** a close now reads one more datum. Re-measured with 100 bonds: inside an epoch 182,040 to 186,339 phlo (+4,299), at a boundary 4,400,917 to 4,405,649 (+4,732). The full sweep is in the FX-16 log, revision 2.

## Verification

- casper `v6::` (release): 89 passed, 0 failed, 7 ignored. This includes the cross-host replay on the new fixture.
- casper `genesis::` (release): 118 passed, 0 failed.
- casper lib (release): 851 passed, 0 failed, 1 ignored.
- `cargo fmt --check`: not verified.

## Revision 2: Decision 107, a linear PoS close (flagged for review)

- `pickActiveValidators` is one expression: `allBonds.keys().toList().take(numberOfActiveValidators).toSet()`. Its callers pass the `allBonds` map instead of a list. The unused `fst` helper is removed. The order is the same: the map's keys in their sorted order.
- An epoch close releases at most 2 quarantined withdrawers (`quarantined.take(2)` before `unorderedParMap`). The others stay in `withdrawers` and are released at the next epoch close. The cap is temporary. `docs/rev-stacks/memos/D107-loops.md` gives the loops that still pass state by value and estimates the proper rewrite.
- New test: `pos_bond_admin_spec::an_epoch_close_releases_at_most_two_withdrawers_and_the_next_epoch_takes_the_rest`. Three validators withdraw in one epoch. The close of block 10 releases two of them, and the close of block 20 releases the third.
- The cross-host fixture is regenerated.
- Verification (release): casper `v6::` 90 passed, 0 failed. `genesis::` 118 passed. casper lib 851 passed.

## Revision 3: the withdrawer rewrite (Decision 107, for review)

The release cap of revision 2 is removed. The withdrawer loops no longer carry the PoS state through a channel.

### State

- New key `releases`: a map from the block number at which quarantine ends to a map `{pk: true}` of the keys that end then.
- `withdrawers` maps a key to its bond amount. It was `(bond, quarantine end)`. The quarantine end now lives in `releases`. `getWithdrawers` returns the new shape. No Rust code and no v6 test reads the old shape. Only the legacy `PoSTest.rho` uses the old PoS contract.

### Changes

| Part | Before | Now |
|---|---|---|
| `withdraw` | sets `pendingWithdrawers[pk] = end` | also adds `pk` to `releases[end]`. All withdrawals of one epoch share one `end`. |
| `movePendingWithdrawer` | `ListOps.fold` over the pending withdrawers, carrying the state | three native map operations: `withdrawers ++ allBonds∩pending`, `allBonds.diff(pending)`, `pendingWithdrawers = {}` |
| quarantine filter | `ListOps.filter` over all withdrawers | a fold over the keys of `releases` that carries two small maps. There is one key for each epoch with withdrawals still in quarantine. |
| releases | `unorderedParMap` (capped at 2), then a fold over the results carrying the state | the due entries are split in halves with `slice` until each part has one entry. Each part calls `RevBondAdmin!("release", …)`. A counter loop collects the failures, and it carries only the failure map. |
| state update | one `delete` per key inside the fold | one `diff` for `withdrawers` and one for `committedRewards` |
| failed release | the key stayed in `withdrawers` and was tried again at each epoch | the key stays in `withdrawers` and is put back in `releases` under the current block, so the next epoch close tries it again |
| `settleOne` | matched `(withdrawing, _)` | reads `withdrawers.getOrElse(pk, 0)` |

No contract body that fires once for each withdrawer refers to the PoS state, so the state is not copied once for each withdrawer.

### Edge case to review

`movePendingWithdrawer` joins the new withdrawers to `withdrawers` with `++`. If a key is still in quarantine and withdraws again after a new bond, the right side (the new bond) replaces the old amount. That is the same as the old `set`. The old quarantined bond is not added to the new one in either version.

### Tests

- `pos_bond_admin_spec::an_epoch_close_releases_every_withdrawer_whose_quarantine_has_ended` replaces the cap test:
  - Validators V1, V2 and V3 bond in B1 to B3 and withdraw in B4 to B6, with an epoch length of 10 and quarantine 0.
  - B9 releases none.
  - The close of B10 moves all three and releases all three. None of them is bonded after it.
- The release, re-bond and pending-bond tests of revision 1 pass unchanged (14 PoS tests).
- No test makes a release fail. `RevBondAdmin` release fails only when the native bond is missing, and no test can set that up from outside.
- The cross-host fixture is regenerated.
- Verification (release): casper `v6::` 90 passed, 0 failed. `genesis::` 118 passed. casper lib 851 passed.
