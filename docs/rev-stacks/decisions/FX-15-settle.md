# FX-15, settlement of a stopped membrane (lane B)

Decisions 86, 89. Plan r8.16. Base: `patch/FX-15-replay` (lane A2) at 8bee6cdde. Lane C writes the meter and the refusal event. Lane A2 wrote the replay checks. This patch is lane B's settlement part.

## Problem

Replay cannot reproduce the net work of a stopped membrane exactly. Another interleaving can run more compute in a branch before the branch reaches its next IO step. The record entry (field 19) holds the net work of each surface, and replay compares its own tally with the record. For a stopped membrane, an honest block could therefore fail the comparison, and settlement could charge a different amount in replay than in play.

## What this patch does

- `v6::stops::settled_work(refusals, replayed, recorded)`:
  - With no refusals, the replayed work stands, as before.
  - For each stopped membrane, the work is the recorded net work, not the replayed work.
  - The recorded net work must be at most the drawn amount D of the membrane. If a membrane has more than one refusal, the least D bounds it.
  - A stopped membrane with no recorded entry pays nothing.
  - A stop without the recorded work is refused (`NoRecordedWork`).
- `Runs::replay_one` takes the recorded work of the envelope and applies `settled_work` after the Decision 89 checks. The record comparison and `settle_block` both use the result, so settlement takes the stopped membrane's work from the record.
- `EnvelopeExecutor::replay_recorded` takes the recorded work for each envelope. `EnvelopeExecutor::replay` keeps its signature and passes no recorded work. A stop then fails with `NoRecordedWork`. `BlockExecutor::replay` passes the work of each record entry.
- Play does not change. Play's net work under a stopped membrane is at most its gross draw, so an honest record passes.

## Checks

| Check | Condition | Test |
|---|---|---|
| Recorded work is bounded | recorded net work ≤ D | `recorded_work_above_the_drawn_amount_is_refused`, `recorded_work_up_to_the_drawn_amount_passes` |
| Several refusals | the least D bounds the membrane | `the_least_drawn_amount_of_a_membrane_bounds_its_work` |
| Settlement takes the record | the stopped membrane's work is the recorded value, other membranes keep the replayed value | `a_stopped_membrane_settles_the_recorded_work` |
| No stop | replayed work stands | `without_a_stop_the_replayed_work_stands` |
| Missing record | refused | `a_stop_without_the_recorded_work_is_refused` |

## Decisions

| Decision | Alternatives | Reason | Reversible |
|---|---|---|---|
| The bound is the least D of the membrane's refusals | The greatest D, or the D of the first refusal | Play stops the membrane at its first refusal, so no draw follows it, and every D of the membrane is equal in an honest record. The least D is the strict choice if they differ. | Yes |
| Replace the replayed entry, not add to it | Take the maximum of the two | Settlement must charge exactly what the record says, so play and replay give the same settlement. | Yes |
| A stopped membrane with no recorded entry pays nothing | Refuse the block | The work map keeps only positive net work, so an honest record can omit an entry whose net work is 0 after refunds. | Yes |
| `replay` keeps its signature | Change every caller | The test callers replay envelopes with no stops. A stop on that path is refused, not settled at 0. | Yes |

## Known issues (Decision 88 labels)

- **No lower bound on the recorded work. Known issue, not blocking.** A proposer can record less net work than ran under a stopped membrane, down to 0. Nobody is overcharged, and no authority changes. The loss is fee income to the compute wallet, at most D per stopped membrane. Replay cannot compute the exact net work, so a lower bound needs a separate rule, for example net work ≥ D − (refunds under the membrane in replay).
- **No end-to-end test yet.** `refusals_of` returns no refusals until lane C's event type lands. The integration test (play stops a membrane, replay agrees under 1, 2 and 4 workers, a forged recorded work fails) goes in with the meter.

## Verification

- `casper --lib v6::stops`: 7 passed, 0 failed.
- casper `v6::` (release): 88 passed, 0 failed, 7 ignored.
- clippy on the touched code: no new warnings.
- `cargo fmt --check`: not verified.
