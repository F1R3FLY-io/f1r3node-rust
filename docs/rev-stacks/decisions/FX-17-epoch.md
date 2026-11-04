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
- **The close bound (FX-16):** a close inside an epoch now reads one more datum. The re-measured close is in the FX-16 log.

## Verification

- casper `v6::` (release): 89 passed, 0 failed, 7 ignored. This includes the cross-host replay on the new fixture.
- casper `genesis::` (release): 118 passed, 0 failed.
- casper lib (release): 851 passed, 0 failed, 1 ignored.
- `cargo fmt --check`: not verified.
