# Decision 107: a bond made while the validator is in quarantine

Lane B, 2026-11-04. Plan r8.23. For Meredith's review. Branch `patch/FX-17-epoch`.

## The case

1. Validator V1 bonds amount b1 (block B1).
2. V1 withdraws (B2). The next epoch close moves V1 to `withdrawers` with b1. Quarantine ends at block E1.
3. While in quarantine, V1 bonds again with amount b2 (B11).
   - `settleOne` admits the new bond only if V1's native bond covers b1 + b2.
   - V1 is in `allBonds` again with b2.
4. V1 withdraws again (B12). The next epoch close moves V1 to `withdrawers` again. Quarantine for b2 ends at block E2, later than E1.

## What PoSV6 does

| Step | Before Decision 107 | After the rewrite |
|---|---|---|
| Second move | `withdrawers[V1] = (b2, E2)`. b1 and E1 are overwritten. | `withdrawers ++ {V1: b2}`, so b2 replaces b1. V1 is removed from the release entry for E1 and kept in the entry for E2. |
| Close at E1 | nothing for V1 | nothing for V1 |
| Close at E2 | releases b2 to V1 | releases b2 to V1 |
| b1 | stays in V1's native bond. PoS no longer tracks it. | the same |

The rewrite keeps the old behaviour. One difference had to be fixed on the way: without the pruning step, the stale entry for E1 would have released b2 at E1, before its own quarantine ended. `withdraw` now removes the key from every release entry before adding it under its new end, when the key is still in quarantine. That is a fold over the keys of `releases`, one per epoch with withdrawals still in quarantine, and it carries only the release map. The withdrawing deployer pays for it, and the close does not.

**Test:** `pos_bond_admin_spec::a_bond_made_during_quarantine_replaces_the_quarantined_amount_and_keeps_the_later_end`, with epoch length 10 and quarantine 15:
- b1 = 1,000, moved at B10, E1 = 25.
- b2 = 500 bonded in B11, withdrawn in B12, moved at B20, E2 = 35.
- The close of B30 releases nothing, and the close of B40 releases 500.
- The native bond still holds 1,000 after B40.

## The issue for review

**b1 is locked for good.** After the second move, PoS has no record of b1, so no close ever releases it. The native bond keeps it, and `RevBondAdmin` will not release it without a PoS call. This was true before Decision 107, and the rewrite keeps it.

Options:

1. **Add the amounts:** `withdrawers[V1] = b1 + b2`, released at the later end E2. Nothing is lost. b1 waits longer than its own quarantine.
2. **Keep two entries:** release b1 at E1 and b2 at E2. This needs a map per key in `withdrawers` (end → amount), which makes the move and release steps a little more complex.
3. **Refuse the re-bond:** a validator in quarantine cannot bond until its release. This is the simplest change, and it stops the case at its source.
4. **Keep the current behaviour**, recorded as a known issue.

Lane B's recommendation is option 1. It is one native expression in `movePendingWithdrawer`, and it loses nothing. The cost is a later release of b1.

## Label (Decision 88)

Known issue, not blocking: it does not diverge from r12 §6.3, and it is not an authority finding. Only the validator's own funds are affected, and only through its own action. Funds are locked, not moved, so the label is Medium for the validator, not High.
