# Decision 107: loops that pass an accumulated value by value, and the withdrawer rewrite

Lane B, 2026-11-04. Plan r8.23, design r21. For Meredith's review.

## Why these loops matter

In Rholang, a loop that sends its accumulated list or map on a channel at each step copies that value into the COMM. The cost meter charges the size of each COMM's data. If the value grows by one element at each step, a loop over n elements costs about n²/2 element-copies. A loop that only sends the remaining tail of a list also copies n, n−1, …, 1 elements. All `ListOps` operations below are of this kind, because `ListOps` itself is a recursive loop that sends `(accumulatedValue, tail)` at each step.

The measurement in the FX-16 log shows the effect: before Decision 107, the epoch close grew about as 325·n² phlo in the number of bonds n.

## Inventory

Method: a reading of every contract that genesis deploys (`embedded_rho.rs` and `fs_genesis.rs`), with a search for `ListOps!` calls and for recursive contracts that take an accumulator or a list tail as a parameter. "Grows with" names the quantity that sets the loop's length.

### PoSV6 (runs in every close)

| Line | Where | Loop | Carries by value | Grows with |
|---|---|---|---|---|
| 34 | `settlePendingBonds` | `ListOps.fold` over `pendingBonds` | the **whole PoS state map** | pending bonds (at most 1 merges per block) |
| 58, 61 | `settleOne` | `ListOps.fold` over one key's pending registrations | `(state, total)` | registrations on one native bond |
| 85 | `getCurrentEpochRewards` | `ListOps.fold` over `allBonds` | a growing map | bonds (exploratory read only, not in the close) |
| 276 | `movePendingWithdrawer` (epoch close) | `ListOps.fold` over `pendingWithdrawers` | the **whole PoS state map** | withdrawals since the last epoch |
| 287 | `removeQuarantinedWithdrawers` (epoch close) | `ListOps.filter` over `withdrawers` (a `reverse` then a `fold`) | a growing list, and the list tail | all withdrawers in quarantine |
| 289 | same | `ListOps.unorderedParMap` over the released withdrawers | a growing result list | released withdrawers (now at most 2) |
| 306 | `computeRemove` | `ListOps.fold` over the release results | the **whole PoS state map** | released withdrawers (now at most 2) |
| — | `pickActiveValidators` | was `ListOps.map` over `allBonds` | a growing list | bonds. **Removed by Decision 107**: now one expression. |

### ListOps (the loops that the calls above use)

Every operation is a recursive `loop` that sends the accumulated value and the list tail on each step: `fold`, `foldClean`, `partialFold`, `partialFoldClean`, `map`, `mapClean`, `filter`, `filterClean`, `reverse`, `sum`, `forall`, `exists`, `indexOf`, `unorderedParMap` (its `collect` keeps the result list on a channel), and `parMap`. Each is quadratic in the list length.

### Other genesis contracts

| Contract | Loop | Carries by value | Grows with | In the close? |
|---|---|---|---|---|
| `Either.rho` 291, 302 | `ListOps.foldClean` over a function list | the function list | the caller's pipeline length | No |
| `MultiSigSystemVault.rho` 87, 111, 113 | `ListOps.map`, `indexOf`, `partialFold` | key and unsealer lists | the signers of one vault | No |
| `Registry.rho` 465 | `loopTreeMapNodes` | the nibble list tail | key length (fixed) | No |
| `File.rho` 7482 | `readLinesIntoLoop` | the rows read so far | lines read by one call | No |
| `File.rho` 7690 | `concatStringsLoop` | the list tail and the string so far | strings joined by one call | No |
| `File.rho` 7058, 7145, 7244, 7340, 7402 | write loops | counters only | — | No (not by-value accumulators) |
| `Buffer.rho` 1347 | `allocInnersLoop` | the list of inner buffers so far | buffer capacity / unit | No |
| `Buffer.rho` 1390, 1407, 1425 | park, clear, close loops | indices only | — | No (not by-value accumulators) |
| `Stream.rho` 584, 633 | `foldLoop`, `foldChunksLoop` | the caller's accumulator | stream length | No |
| `Stream.rho` 771 | `loop(@partial)` | a list accumulator | stream length | No |

The search found no other accumulator loops in `Registry.rho`, `VersionedRegistry.rho`, `NonNegativeNumber.rho`, `MakeMint.rho`, `AuthKey.rho`, `SystemVaultV6.rho`, `Stack.rho`, `TokenMetadata.rhox`, `Fs.rho`, `Dir.rho`, `Stdin.rho` or `Stdout.rho`. A search by name pattern can miss a loop with an unusual name, so a second reading of these files is advisable.

The contracts outside PoSV6 run only when a deployer calls them, and that deployer pays. They do not affect the close bound.

## Rewriting the withdrawer loops

Decision 107 caps the releases of one close at 2 (`quarantined.take(2)`). The cap bounds lines 289 and 306. It does not bound lines 276 and 287, which still walk all pending withdrawers and all withdrawers in quarantine, carrying the state or a growing list.

**Proposed rewrite:**

1. Keep a release schedule in the PoS state: a map from the epoch index at which quarantine ends to the set of keys that end then. `withdraw` adds the key to its epoch's set.
2. `movePendingWithdrawer`: move the pending set into `withdrawers` and the schedule with native map and set operations (`union`, `delete`, `diff`), not a fold over the state.
3. `removeQuarantinedWithdrawers`: read the schedule entries for the epochs that have ended. Their number is small, one for each epoch since the last close. Pay each key with `RevBondAdmin!("release", …)` in parallel, and join the results with a counter on one channel. Remove the paid keys with one native `diff`. A failed release stays in the schedule for the next epoch.
4. `settlePendingBonds` (line 34) gets the same treatment: one map `union` instead of a fold over the state.

**Estimate:** about 2.5 days.
- 1 day for the PoS change.
- 1 day for tests: ordering, a failed release, many withdrawers in one epoch, and play-and-replay agreement.
- 0.5 day to re-pin the fixtures, re-measure, and remove the cap.

**Risk:** medium. The change touches bond custody (`RevBondAdmin` release), so it needs lane A1's authority audit. One point needs checking: whether every native operation it uses (`union` and `diff` on sets and maps) exists in the Rust interpreter with the expected cost.

## Open

- **Lines 276 and 287 stay unbounded under the cap.** A shard where many validators withdraw in one epoch can still make one epoch close exceed the pre-charge. The bound test in FX-16 measures this case.
- The pre-charge counts the bonds at the attested root. A withdrawer in quarantine has left `allBonds`, so it is not counted, although lines 276 and 287 still walk it. Two remedies: the rewrite above, or counting withdrawers in quarantine as bonds for the pre-charge.
