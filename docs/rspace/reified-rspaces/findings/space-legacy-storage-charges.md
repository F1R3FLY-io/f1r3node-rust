# Finding: space operations paid no legacy storage charges

**Status.** Fixed by Option P of decision 45 on 2026-10-06. The user decided the charging rule in [decision 45](../decision-register.md#decision-45), and section 5 describes the fix as built. Section 6 records the limits that stay.

**Scope.** This document records one gap of the Reified RSpaces agent, its reach, and its fix. The [design](../design.md#n10-legacy-charges) describes the fixed rule, and [Rule SM-8a](../specification.md#13-entries-and-operations) states it.

## Contents

1. [Summary](#1-summary)
2. [Terms](#2-terms)
3. [Mechanism and measurement](#3-mechanism-and-measurement)
4. [Reach and effect](#4-reach-and-effect)
5. [The fix (decision 45, Option P)](#5-the-fix-decision-45-option-p)
6. [Limits that stay](#6-limits-that-stay)
7. [References](#7-references)

## 1. Summary

A legacy deploy pays the storage of each host operation through `ChargingRSpace`.
A space operation reached the host store through the substrate, and the substrate does not pass through `ChargingRSpace`.
So a space operation in a legacy deploy paid no storage charge and no event fee, while the equivalent host program paid both.
Play and replay agreed, so no block was rejected, but a deploy paid less than its host program.

Option P of decision 45 fixed the gap on 2026-10-06.
Each staged space operation now pays the [legacy charges](../glossary.md#legacy-charge) that `ChargingRSpace` makes for its equivalent host call, in the order of the host program.

## 2. Terms

- A **legacy deploy** runs inside the legacy scope of its budget (`RuntimeBudget::enter_legacy_scope`). It pays phlo through `RuntimeBudget::charge_legacy`.
- A **legacy charge** is a charge of `ChargingRSpace`. Before a host call it charges the storage of a record or a continuation. After the call it refunds the removed storage and charges the event fees.
- A [space transaction](../glossary.md#space-transaction) stages host-equivalent operations and commits them through the substrate of P1.

## 3. Mechanism and measurement

The runtime wraps the host RSpace twice: in `DeterministicRSpace`, and then in `ChargingRSpace` (`rholang/src/rust/interpreter/rho_runtime.rs:2010-2013`).
The reducer calls the outer wrapper, so every host operation pays its legacy charges.
The space runtime takes the concrete RSpace before these wrappers (`rholang/src/rust/interpreter/rho_runtime.rs:2661`), as decision 19 requires.
So the commit of a space transaction reaches the host store directly, and no legacy charge runs.

The test `space_queue_legacy_replay_equal` measures the gap on the deploy `a!(1) | a!(2) | b!(3)`, which sends to two queue names.
With Option P, the deploy makes 31 storage charges, which total 2313 phlo: the counter bumps and the stores of the three sends.
Before Option P it made none of them, and its legacy cost was only the 305 phlo of the evaluation steps of the reducer.

## 4. Reach and effect

Casper evaluates deploys in the legacy scope through `RuntimeOps::evaluate` (`casper/src/rust/rholang/runtime.rs:1594-1600`).
Its callers include `process_deploy`, which plays a user deploy, and `run_user_deploy`, which replays one (`casper/src/rust/rholang/replay_runtime.rs:597`).
So the gap reached the user deploys of blocks.
Play and replay made the same charges, so the gap caused no replay mismatch and no fork.
The effect was economic: a space operation cost almost nothing.
A deploy could store data in a space for less than on an ordinary channel.

Deploys outside the legacy scope pay through the accounting observer, which the substrate already drives ([M-3](../decision-register.md#m-3)).
They were not affected.

## 5. The fix (decision 45, Option P)

### 5.1 The rule

Each staged operation of a space transaction pays what `ChargingRSpace` charges for its equivalent host call.
The pre-charge of an operation runs in its callback `before_observe`, before its observer calls.
Its post-charges run in the callback of the next operation, and the post-charges of the last operation run after the commit.
So the legacy log of a space job is the log of its host program.
A charge that runs out of phlo stops the job where the host program stops.
Table N10.1 of the [design](../design.md#n10-legacy-charges) gave the charges of each staged shape, and [decision 90](../decision-register.md#decision-90) retired that table with the charges.

### 5.2 The code

- `rholang/src/rust/interpreter/reified/legacy_charges.rs` is a new file. `produce_charges` and `consume_charges` restate `ChargingRSpace` and its `handle_result`, and `LegacyChain` puts the charges into the callbacks.
- `spaces/generic.rs` stages every operation of a plan through the chain, and the finish step of each prepared job charges the tail.
- `spaces/counter.rs` stages the counter bump through the chain.
- No file of `accounting/`, no host file, and no Casper file changes.

### 5.3 The tests

- `prop_legacy_charges_equal_charging_rspace` runs generated host programs through the real `ChargingRSpace`. For each host call, the charges of the equivalent staged operation equal the new entries of the legacy log, value and label.
- `prop_legacy_chain_log_is_host_order` stages generated transactions with generated budgets. The legacy log and the stop point equal those of the host program.
- `prop_legacy_replay_stops_with_play` replays each such transaction with the log of play. Replay makes the legacy log, the stop and the store of play.
- The unit tests of `legacy_charges.rs` pin the charges of each shape and the order of the chain.
- `space_queue_legacy_replay_equal` and `space_queue_legacy_out_of_phlo_sweep_replays_equal` run legacy deploys on a queue in play and replay.

### 5.4 The proofs and the models

[Section 11.14](../verification.md#1114-legacy-charges-of-decision-45) of the verification dossier lists the Rocq module `LegacyCharges.v`, the TLA+ invariant of `CheckpointReplay.tla`, and their negative controls.

## 6. Limits that stay

- In a legacy deploy, the reducer runs host operations outside the deterministic scheduler. So the evaluation charges of parallel branches follow the schedule, as for host programs. A space job is sequential inside, so its charges keep their order.
- If an operation fails a read-only check of the commit, the post-charges of the operation before it are not made. Only a defect, or a produce counter at `i32::MAX`, fails such a check.

## 7. References

- [Decision 45](../decision-register.md#decision-45) and the [design, section N10](../design.md#n10-legacy-charges).
- `rholang/src/rust/interpreter/storage/charging_rspace.rs`: the host charges.
- `rholang/src/rust/interpreter/accounting/costs.rs`: the cost functions.
