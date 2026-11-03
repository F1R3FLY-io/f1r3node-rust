# FX-15 refusal event: lane A2 review

Lane A2, 2026-11-03. Lane C's `FX-15-refusal-event.md` is not on the remote (no `patch/FX-15` branch at review time). This review checks the four points that the coordinator gave for it, against the Q-32 memo and Decision 89.

## Points

1. **The refused step is a fourth event type in the deploy log.** Confirmed, with two conditions:
   - `rig` must skip refusal events. They are not COMMs, and they must not go into the replay-data multimap.
   - The event carries the membrane key: `Surface` (owner, origin envelope, price), as D76 and Decision 80 use it.
2. **The step identity comes from the step's source.** Confirmed for IO steps: the produce or consume hash, as the other events use. **Correction for a compute charge:** the term alone does not identify the step, because two branches can reduce equal terms. The identity is the hash of the charged term and the branch's random state. Both are the same in play and replay.
3. **The drawn amount is the gross total admitted under t.** Confirmed, with one condition: the admission test of Decision 86 must use the same gross figure. A refund does not raise t's available balance during the run. Settlement still charges net work, as now.
4. **The Q-32 acceptance check applies.** Confirmed. Each refusal event i for t gives (D_i, c_i). A_t is t's pre-state balance less what the gate holds. W_t is the gross work that replay meters under t without a stop.

## Replay checks (Decision 89)

| Check | Condition | Rejects |
|---|---|---|
| Early stop | A_t − D_i < c_i | a refusal of a payable step |
| Forged drawn amount | D_i ≤ W_t, with the early-stop check | an amount above the work that ran (W_t), or below the stop point |
| Unpayable admitted step | D_i ≤ A_t when t stopped. W_t ≤ A_t when t did not stop. | an admission past the balance |
| Same stop | Under a stopped t, a produce or consume that is not logged is refused. Each logged event is used (strict, not the failed-evaluation tolerance). | different stored items |
| The refused step is real | Replay finds the step with the event's identity and recomputes c_i | a forged step cost |

**Replay does not stop t on its own meter.** With a different interleaving, replay can run more compute in a branch before that branch reaches its next IO step. A meter stop in replay could leave a logged event unused, so replay would reject an honest block. W_t ≤ A_t is therefore a check only when t did not stop. When t did not stop, W_t is the same in any order.

## For lane C

The meter can start on these terms. Points 2 and 3 above are the only changes.
