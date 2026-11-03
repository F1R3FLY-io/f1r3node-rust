# FX-17: order-floor storage and transaction layout (on paper)

Lane A2, 2026-11-03. Decision 99 (Q-33), design r19. No code until lane A1's review. Base: `patch/FX-15-replay` (this branch has the CWG finalizer: `floor_runner`, `commit_lfb`, `finalization_commit`).

## 1. Registers

The order floor uses the existing `lfb-pointer` store, with a second key. No new LMDB store is necessary, and the store is already in the env that `commit_lfb_is_transactional` checks.

| Store | Key | Value | Meaning |
|---|---|---|---|
| `lfb-pointer` | `"lfb"` | `LfbPointer { hash, height, commit_epoch }` | Unchanged. The state-final block (P11). |
| `lfb-pointer` | `"order"` (new) | `LfbPointer { hash, height, commit_epoch }` | The order floor: the oracle's `Advance(F)` after P1 to P4. |
| `finalization-commit-marker` | `commit_epoch` | `FinalizationCommitMarker` | Unchanged. Written only for a state commit. |
| block metadata | block hash | `BlockMetadata` | Its "finalized" flags keep dev's meaning: state-final. Order finality sets no flag. |

- `LfbPointer` is reused for the order value, so no new serialized type is added. The bincode note in CWG §3.6 does not apply.
- `commit_epoch` is one counter for both keys. Each transaction that writes either key increments it.

## 2. One transaction, two commits

`commit_lfb` becomes `commit_floors`. Under `global_lock.write`, in one `batched_put`:

1. **Order part:** run `validate` (P1 to P4) on the candidate F. If F passes and is higher than the order pointer, write `"order" → F`. If not, keep the order pointer.
2. **State part:** compute F' = `state_floor(dag, order)`. This is the highest main-chain block M at or below the order pointer (after step 1) that has `pos(M) ≤ p*` (section 3) and is higher than the current LFB.
   - If F' exists, write the same rows that `commit_lfb` writes today: metadata rows for F' and its held ancestors, the marker, and `"lfb" → F'`.
3. If neither part writes, the transaction is empty and the call returns `Refused`, as today.

A state commit must also run when the order floor does not move, because attestations arrive after order. The finalization runner calls `commit_floors(None, …)` (state part only) once in each cycle, after the Compute step and whatever its outcome. The call is a lookup, so it does not use the derivation budget (CWG §3.2).

`CommitOutcome::Committed` carries `{ order: Option<LfbPointer>, state: Option<FinalizationCommitMarker> }`. Event publication and `mark_events_published` see only the state marker, so dev's events keep their meaning.

## 3. P5: the attested position p*

- `pos(B)` is B's index in the linear order (`ExecutorCore` index).
- Each v6 block carries an attestation `(block_hash, root)`.
- The tally keeps the latest attestation of each bonded validator, as `(j, root)`.

**Rule:**
- An attestation supports every position p ≤ j iff `root` equals the local canonical `post_root(j)`. The canonical records form a chain (pre-root of j+1 = post-root of j), so one match covers the prefix.
- An attestation whose root differs from the canonical root at j is a `Misattestation` (evidence, existing path).
- An attestation at a j with no local canonical record waits.
- p* is the highest p at which the supporting stake is more than 2/3 of the bonded stake at the current LFB.
- Under θ ≤ 0, p* is undefined and the state part never writes (CWG P3 regime).

**The tally:**
- It is in memory, in `ExecutorCore`, next to `pending_attestations`.
- On startup it is rebuilt from the bodies of the blocks above the LFB. The `max_unattested` gate keeps that range bounded.
- The finalizer reads p* through a narrow trait (`StateFloor::attested_position()`), so `block-storage` does not depend on v6.

## 4. Linearizer and startup

- `V6Engine::tick` reads `lfb_pointer("order")`. If the key is absent (a store from before FX-17), it reads `"lfb"`. That is today's behavior and the upgrade path.
- The A-LIN hold stays on the order pointer.
- Reconciliation (CWG §3.6) does not change: markers are state commits only. One new invariant is checked at startup: `height(order) ≥ height(lfb)`. If it is false, the node panics, as for a stale metadata flag.

## 5. Crash cases

| Crash point | State after restart | Effect |
|---|---|---|
| Before `batched_put` | old order, old LFB | The next cycle derives the same result again |
| After `batched_put`, before the in-memory swap | new registers on disk | `try_new` reads the KV store, as today |
| After the state commit, before events | marker present | Events are re-emitted (existing path) |

LMDB commits the transaction or does not, so a torn write cannot give "order moved, LFB did not" when both should move.

## 6. Interfaces that change

- `commit_lfb` → `commit_floors(order: Option<(BlockHash, f32, V)>, state: S)`.
- `CommitOutcome::Committed` → `Committed { order, state }`.
- A new `CommitRefusal::StateUnattested` (for metrics only. It is not an error).
- `KeyValueDagRepresentation::last_finalized_block()` keeps dev's meaning. A new `order_floor()` gives the order pointer.

These are shared interfaces between block storage, the finalizer and the v6 seam. Lane A1 should review them before code.

## 7. Questions for lane A1

1. Must the order floor run P2 (state containment) and the A-LIN check, or only the LFB? This paper runs all of P1 to P4 on the order floor and P5 only on the LFB. The order floor is then as safe as today's LFB, and the LFB is stricter.
2. The bonds for the 2/3 come from the current LFB, which lags the order floor. Is the bond latency of one attestation round acceptable (Q-33 section 4)?
3. Is a "finalized" deploy status for clients state-final only, as here, or does it also report order-final?
