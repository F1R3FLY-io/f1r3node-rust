# FX-15, replay side (lane A2)

Decisions 86 and 89. Base: `patch/FX-8b` at 4a74b0e34. The play side (the meter and the refusal event) is lane C's. The charging side is lane B's.

## What this patch does

- `rholang::accounting::stops`:
  - `Refusal` holds the membrane (`Surface`), the step identity, the drawn amount D and the step cost c.
  - `ReplayStops` holds, for one replay, the stopped membranes, the refused steps and the logged IO hashes. It also holds what replay observes: gross work for each stopped membrane, the refused steps that replay reached with their costs, and the count of refusals.
  - `check_stop` and `check_unstopped` are the Decision 89 checks.
- `CostManager` holds an optional `ReplayStops`. Each charge adds its positive amount to the gross work of a stopped membrane.
- `ChargingRSpace`: in replay, a produce or consume under a stopped membrane runs only if its event hash is logged. Otherwise the space does not store it and does not charge for it. A rho branch ends at an IO step, so this refusal is replay's stop for that branch. A refusal that has the hash of a refused step records the step's storage cost as its cost.
- `v6::stops`:
  - `logged_io` collects the produce and consume hashes of the deploy log.
  - `install` builds the `ReplayStops` for an envelope with refusals.
  - `check` runs `check_stop` for each refusal, against `BlockPurses::available`.
- `BlockPurses::available(surface)` is the free balance at the pre-state, less the gate table, in phlo at the surface's price. For a compound, it is the minimum over the components, because each component pays in full.
- `replay_one` installs the stops before replay and checks them after. It removes the stops before the runtime goes back to the pool. An envelope with a stop does not fail, so the strict unused-COMM check applies.

## Checks

| Check | Condition | Test |
|---|---|---|
| Early stop (Decision 89) | A − D < c | `a_stop_at_a_payable_step_is_refused`, `a_drawn_amount_below_the_stop_point_is_refused` |
| Forged drawn amount (Decision 89) | D ≤ W (gross replayed work) | `a_drawn_amount_above_the_replayed_work_is_refused` |
| Unpayable admission (Decision 89) | D ≤ A. When the membrane did not stop, W ≤ A. | `an_admission_past_the_balance_is_refused` |
| The refused step is real | replay reaches the step, and its cost equals c | `a_refused_step_that_replay_does_not_reach_is_refused`, `a_forged_step_cost_is_refused` |
| A refusal names a foreign membrane | the client is not stopped this way | none yet. The check is in `v6::stops::check`. |
| Same stop | an unlogged IO step under a stopped membrane is refused. Each logged COMM is used. | `under_a_stopped_membrane_only_logged_io_runs` |

## Decisions

| Decision | Alternatives | Reason | Reversible |
|---|---|---|---|
| Replay refuses at the storage boundary, not in the reducer | A reducer stop for each branch | A branch ends at an IO step, so a refused IO step ends the branch. The scheduler does not change. | Yes |
| Replay does not stop a membrane on its own meter | Stop at D | Another interleaving can run more compute before the next IO step. A meter stop could leave a logged event unused and reject an honest block. | Yes |
| A, D and c are in phlo. A is the free balance divided by the origin price. | REV | The meter admits in phlo at the origin price (FX-12). | Yes |
| `refusals_of` returns no refusals until lane C's event type lands | Define the event type here | The event type is lane C's shared interface. | Yes |

## Interface for lane C

- **Event conversion:** the refusal event converts to `stops::Refusal`. `v6::stops::refusals_of` reads it from the deploy log. `rig` skips it.
- **IO step identity and cost:**
  - The identity of a refused IO step is the produce or consume hash (`Produce::create` or `Consume::create`).
  - Its cost c is the storage cost of that produce or consume (`storage_cost_produce` or `storage_cost_consume`).
- **Compute step identity:** for a refused compute step, the meter calls `ReplayStops::found_step(identity, cost)` at the charge point in replay. The identity is the hash of the charged term and the branch's random state.
- **Unstopped membranes:** `check_unstopped` (W ≤ A) is not yet called in replay. Today a foreign purse can be short, and FX-12 caps its draw. Turn the check on in the same commit as the meter, or replay rejects the blocks that FX-12 accepts today.

## Open

- **Settlement for a stopped membrane (lane B):** replay cannot reproduce the net work of a stopped membrane exactly, because the interleaving can differ. Settlement must take that work from the record, and replay must check that it is at most D. Lane B's FX-15 charging side.
- An integration test needs lane C's meter. Play stops a membrane, replay agrees under 1, 2 and 4 workers, and each forged trace fails.

## Verification

- `rholang --lib stops`: 10 passed, 0 failed.
- casper `v6::`: 88 passed, 0 failed, 7 ignored. Without refusals, replay is unchanged.

## The close (Decisions 94 and 97)

- `RuntimeManager::compute_state_gated` and `replay_compute_state_gated` take a `SystemGate`. The plain functions call them with no gate, so the legacy paths do not change. A gate has three parts:
  - `funding`: the oracle that lane C installs.
  - `charge`: a `SystemCharge` with the payer (the proposing validator) and the amount held for the close.
  - `stops`: the replay check for refusals.
- `CostManager::system_charge` gives the `SystemCharge` to the runtime that runs the close. Lane B's FX-16 reads the payer and the hold from it. FX-16 then takes the actual charge from the hold and returns the rest.
- `BlockExecutor::close_gate` builds the close's gate.
  - The stop check uses `BlockPurses` at the settled root with an empty commitment table.
  - `funding` is `None`. Lane C sets it to `EnvelopeFunding::new(purses, &[])` on the same purses.
- In replay, the refusals of a system deploy install `ReplayStops` and are checked like an envelope's. A system deploy with refusals and no stop check fails replay.

### Decision 97: pre-charge the close

- **Configuration:** `close-precharge` in the shard resource policy, which is on chain in the genesis block. It is policy encoding 3, 65 bytes. Encoding 2 still decodes, with the default value.
- **Acceptance:** `V6Engine::check_close_cover` reads the proposer's balance at the block's attested root. A block whose proposer holds less than the pre-charge is invalid (`InvalidFormat`). The validation dispatcher calls the check after `check_block`.
- **Proposal:** `create_block` makes the same check on its own wallet and proposes nothing when the check fails.
- **The gate:** `gate_block_with_close` holds the pre-charge on the proposer's wallet before any envelope is admitted. `check_recorded_with_close` holds it the same way in replay. Envelopes that draw on the proposer's wallet see the hold.
- **After the close:** the actual charge comes from the hold, and the rest goes back. This is lane B's (FX-16).
- **Removed:** the Decision 96 check after the close, which would have stopped the chain.

| Decision | Alternatives | Reason | Reversible |
|---|---|---|---|
| The default `close-precharge` is 0, which turns the check and the hold off | A nonzero default | The amount must cover the close charge that FX-16 measures. The value is set with FX-16. | Yes |
| The pre-charge is a policy field on chain | Node configuration | Every validator must apply the same acceptance rule and the same hold. | Yes |
| Acceptance reads the attested root | The execution pre-state | The execution pre-state is not known when the block is accepted. | Yes |
| If the proposer holds less than the pre-charge at execution, the gate holds what is free | Refuse every envelope | Execution must not stop the chain. See the open item below. | Yes |
| A failed acceptance check gives `InvalidFormat` | A new `InvalidBlock` variant | No new variant is necessary, and the log gives the reason. | Yes |

**Open item (Decision 88: known issue, needs a ruling):** the proposer's balance can fall between the attested root and the execution pre-state, for example through blocks that execute between them. Then the hold is less than the pre-charge, and FX-16 can take less than the close charge. The shortfall is at most the pre-charge.
