# FX-19 memo (lane C): the declared per-firing bound (Decision 120)

Base: `patch/FX-17` at dbc3dca9b. Names: u1…um deployers, d1…di deployments, B1…Bj blocks. W is the installer's wallet.

**Recommendation:** confirm the working reading of Decision 120, with the meaning of "the work under the firing" that this memo gives. Choose option (i) for several firings that draw on one wallet.

## 1. The rule

1. **The seal record** gets `phlo_limit`, the `phloLimit` of the installing deployment d1. The bound is b = `phlo_limit` × `phlo_price`, in REV.
2. **The match filter.** A sealed stored receive is a match candidate only if W can hold b (section 3). A receive that is not a candidate stays stored, and the datum is stored too, as with Decision 79's filter.
3. **The firing.** The body of the fired continuation runs with its tuple-space operations (sends, receives, contract installs) held in a buffer. When the body's evaluation ends:
   - **Cost ≤ `phlo_limit`:** the buffered operations go to the tuple space in order. W pays the actual cost. The rest of b is not charged.
   - **Cost > `phlo_limit`:** the buffer is discarded, and W pays b. The firing COMM stays, so the datum is consumed. The client's own work is not affected.
4. **The work under a firing** is the evaluation of that body only. A COMM that the body causes is dispatched after the flush, so it is a new firing. If the fired code is sealed, it has its own bound. Each sealed firing has its own bound, also when the installer is the same.

## 2. Why the rule is deterministic without a trace

- **The buffer prevents a cascade.** No other process sees a buffered operation before the flush, so a discarded firing has no effects to undo. The reducer already evaluates a body as one future: `eval` joins its spawned parts, and COMM bodies are deferred.
- **The cost of a body does not depend on the order of evaluation.** The cost is the substitution, the expressions, and the storage reserved when each operation is issued (c97c2e4b3), plus the COMM charge. So play and replay agree on whether a firing passes its bound. The point where the bound is passed can differ between play and replay, but nothing from the firing stays, so that point has no effect.
- **The check of several firings does not depend on their order** (section 3). A bound that a run holds is not released before the run ends.

## 3. Several firings in one block on one wallet

In one run, a firing is a candidate if the bounds that the run already holds on W, plus b, are not more than the run's share of W. The total of the bounds does not depend on the order of the firings. So replay checks each logged firing in its own order and gets the same result.

| Option | The run's share of W | Effect |
|---|---|---|
| **(i), recommended** | free(W) / n. free(W) is the pre-state balance less the gate commitments. n is the number of envelopes that the gate admitted in the block. | No two runs can overdraw W. A client is never affected. A popular contract with a small wallet can skip firings in a full block. |
| (ii) | free(W) for each run. The merge settles W in gate order and marks the run that overdraws W `Overdrawn`. | No firing is skipped. A client's envelope can be dropped, and it counts as included. |

**The close** runs after the merge, as one run. Its share is free(W) less the amounts that the block's runs drew.

## 4. What is removed and what replay checks

**Removed:**
- From `membrane.rs`: `StepScope`, the step identities, `Refusal`, `ReplayRefusals`, `Admission`, and the `BlockLedger` that is shared across runs.
- From `surface.rs`: `accept_metered`, and the funding meter in the close.
- From the deploy log and the record format: the refusal event. Lane A2 owns that format.

**Kept:** the per-wallet totals that each run draws, for the settlement.

**Replay checks, from the logged COMMs only:**
- each sealed firing is covered;
- each firing that passes its bound is discarded;
- the amounts that W is charged.

## 5. Decisions for Meredith

1. Confirm section 1 item 4: the work under a firing is its own body. Each later firing is bounded again.
2. Choose option (i) or (ii).
3. A bound is held until its run ends. It is not released when its firing ends, because a release would make the check depend on order.

**Build after the ruling:** about 2.5 days with tests. Lane A2 does the change to the record format.
