# Finding: space replay when a deploy runs out of phlo

**Status.** Fixed by decision 36 on 2026-10-05. The user approved the fix of section 6 as [decision 36](../decision-register.md#decision-36), and the code, the models, and the tests of section 6 implement it. Section 8 keeps the questions that stay open for their owners.

**Scope.** This document records one defect of the Reified RSpaces substrate, its fix, and two related observations about the host code. Decision 36 amends [decision 19](../decision-register.md#decision-19), and the [design](../design.md#s53-verify-fr-3) describes the fixed rule.

## Contents

1. [Summary](#1-summary)
2. [Terms](#2-terms)
3. [Reproduction and measurements](#3-reproduction-and-measurements)
4. [Mechanism](#4-mechanism)
5. [Reach and effect](#5-reach-and-effect)
6. [The fix (decision 36)](#6-the-fix-decision-36)
7. [Related observations outside the substrate](#7-related-observations-outside-the-substrate)
8. [Open questions](#8-open-questions)
9. [References](#9-references)

## 1. Summary

A deploy could run out of phlo while the substrate committed a [space transaction](../glossary.md#space-transaction).
The [replay](../glossary.md#replay) of that deploy then reached a different error and a lower cost than play.
Casper compares the failed flag and the cost of each replayed deploy, so validators rejected an honest block that held such a deploy.

The defect was in the replay half of the P1 substrate ([decision 19](../decision-register.md#decision-19)).
No Casper file and no host file caused it.
The P4 test `space_queue_replay_equal` in `rholang/tests/reified_queue_replay.rs` failed because of it.

Decision 36 fixed the defect on 2026-10-05 (section [6](#6-the-fix-decision-36)).
Replay now stages a fire without a recorded COMM as an [unpaired fire](../glossary.md#unpaired-fire), with no error.
The commit resolves the fire after the observer calls of play, so replay fails with the error and the cost of play.
The P4 test now passes, and a new sweep test checks equal outcomes and costs over many phlo limits.

## 2. Terms

| Term | Meaning |
|---|---|
| Phlo limit $`L`$ | The phlo that a deploy buys. Play stops the deploy when a charge would exceed it. |
| Charge | One call of the [accounting observer](../glossary.md#accounting-observer) that can reject an operation: an introduction charge for a produce or a consume, or a COMM charge for a fire. |
| Play cost $`c_p`$ | The cost that play records in the block for the deploy. |
| Replay cost $`c_r`$ | The cost that replay computes for the same deploy from the same log. |
| [Pairing](../glossary.md#pairing) | The match of a staged fire with a recorded COMM of the replay log. |
| [Prefix atomicity](../glossary.md#prefix-atomicity) | Commit rule D6: when operation $`i`$ fails, operations $`0`$ to $`i-1`$ stay applied. |
| [Unpaired fire](../glossary.md#unpaired-fire) | A staged replay fire whose COMM the replay log does not hold. |

## 3. Reproduction and measurements

On the code before the fix, at commit `a0bcaf90e`, run the P4 test with the local parser `[patch]` of P3b:

```sh
cargo test -p rholang --test reified_queue_replay space_queue_replay_equal
```

The test plays the deploy `for (@x <= b) { Nil } | b!!(1)` in a `hashmap:queue` space with $`L = 100000`$.
The persistent receive and the persistent send form a divergent pair, so the deploy runs until its phlo is spent.
The test keeps the event log of play and reverts, as the play of Casper does.
Then it rigs the log on a replay runtime and evaluates the deploy again with the same limit.

Batch 4E-T2 measured these results on 2026-10-05, on the code before the fix:

| Deploy | Phlo limit | Play | Replay |
|---|---|---|---|
| Divergent pair in a queue space | 100000 | `OutOfPhlogistonsError`, cost 100000 | `ReplayDivergence`, cost 98166 |
| The same deploy | 50000 | `OutOfPhlogistonsError`, cost 50000 | `ReplayDivergence`, cost 47250 |
| A deploy that aborts with `ParamsUnsupported` after space operations | the test default | `ParamsUnsupported`, cost 17997 | `ParamsUnsupported`, cost 17997 |

A sweep of the phlo limit over one space program found 166 mismatches among the 357 limits at which play fails.
A host-only twin of the program found 25 mismatches among 203 such limits (section [7.1](#71-host-replay-skips-a-rejected-comm-charge)).
The abort row shows that a failure without a rejected charge replays equally.

A temporary diagnostic test made that sweep.
Its logs were under `target/` and were lost on 2026-10-05, when an external process deleted that directory.
The regression test `space_queue_out_of_phlo_sweep_replays_equal` of the fix replaces it (section [6.5](#65-the-tests)).
On the code before the fix, 143 of its 232 deploys replayed to another outcome than play, and with the fix none does.

## 4. Mechanism

Figure F1 shows the three runs side by side.

![Sequence of one space transaction with three operations: a counter take fire, a counter put, and a data fire. Play stages the three operations with no host call, then commits them in order. The charges of operations 0 and 1 are accepted and their events are logged. A charge of operation 2 is rejected because the phlo is spent, so play fails with OutOfPhlogistonsError, keeps operations 0 and 1 applied, and Casper records the deploy as failed with cost L. Replay before the fix rigs the log, stages the same operations, finds the COMM of operation 0, finds no COMM for operation 2, and fails at staging with ReplayDivergence. The whole transaction drops, so replay makes no charge of the three operations, and its cost is lower than L. Casper reports a cost mismatch and rejects the honest block. Replay with the fix of decision 36 rigs the same log, stages operations 0 and 1 paired and operation 2 as an unpaired fire with no error, and commits. It makes the charges of play in the same order, logs the events of operations 0 and 1 and removes their bindings, and the same charge of operation 2 rejects. So replay fails with OutOfPhlogistonsError at cost L, equal to play, and Casper accepts the block.](../diagrams/16-space-replay-out-of-phlo.svg)

*Figure F1. Source: [diagrams/16-space-replay-out-of-phlo.puml](../diagrams/16-space-replay-out-of-phlo.puml), rendered by [diagrams/render.sh](../diagrams/render.sh).*

### 4.1 Play

Staging makes no host call.
The commit runs the staged operations in order (`commit_staged`, `rspace++/src/rspace/space_store_handle.rs:3353-3397`).
For each operation, play makes its charges first and logs its events only after the observer accepts them (`rspace++/src/rspace/rspace/space_play.rs:143-149` and `183-193`).
When a charge of operation $`k`$ rejects, the commit stops at operation $`k`$.
The operations before $`k`$ stay applied and logged (D6), and operation $`k`$ leaves no event.

### 4.2 Replay before the fix

Before the fix, replay checked each fire against the log while it staged the fire.
At commit `a0bcaf90e`, `TxnCore::verify` returned the error `ReplayDivergence` when the log held no free occurrence of the COMM.
The two staging calls of `verify` passed the error on with the operator `?`.
The code was lines 1571 to 1595 of `space_store_handle.rs`, with the calls at lines 1757 and 1946 of that commit.

Play never logged the COMM of operation $`k`$, so `verify` failed at staging.
The agent propagated the error, and the whole transaction dropped before its commit.
So replay made none of the charges of the transaction.
It missed the charges of the operations before $`k`$, which play accepted, and it never reached the rejected charge of operation $`k`$.

The replay commit itself made the calls of play in the same order, as it still does (section [6.3](#63-the-code)).
The defect was only that the check at staging stopped the transaction before that commit.

### 4.3 The parity requirement

Casper accepts a replayed deploy only when its failed flag and its cost equal the values of play (`casper/src/rust/rholang/replay_runtime.rs:619-632`):

```math
(\mathit{failed}_r,\ c_r) = (\mathit{failed}_p,\ c_p).
```

Let the transaction $`T`$ hold the charges $`w_a, \ldots, w_k`$ in commit order, where play rejects $`w_k`$.
Play accepts $`w_a`$ to $`w_{k-1}`$ and then fails with $`c_p = L`$, as the measured rows show.
Replay before the fix made no charge $`w_j`$ with $`a \le j \le k`$, so it failed earlier with $`c_r < L`$.
The failed flags agreed and the costs differed, so the block failed the check.
With the fix, replay makes the charges $`w_a`$ to $`w_k`$ in the same order, and $`w_k`$ rejects again, so $`c_r = L = c_p`$.

## 5. Reach and effect

Every collection operation starts with a counter fire (`rholang/src/rust/interpreter/spaces/counter.rs:249-307`, [decision 31](../decision-register.md#decision-31)).
Every deploy pays introduction charges: the interpreter enters the COMM accounting scope for each deploy (`rholang/src/rust/interpreter/interpreter.rs:229`), and the introduction charge applies in that scope (`rholang/src/rust/interpreter/accounting/mod.rs:1191-1203`).
So a plain deploy can run out of phlo inside any space transaction, and no cost-signed syntax is necessary.

Before the fix, any user could make validators reject an honest block that held a space deploy.
The user only had to choose a phlo limit that ran out inside a space transaction.
All honest validators computed the same result, so the defect caused no fork.

## 6. The fix (decision 36)

### 6.1 The rule

The fix moves the divergence check from staging to commit.
In replay, `verify` reports an unpaired fire instead of an error, so replay stages exactly what play stages.
At commit, each operation makes the charges of play in the same order.
An unpaired fire returns the rejection of the observer when one of its charges rejects.
Otherwise it fails with `ReplayDivergence` after its charges, and the prefix stays applied (D6).
Sections S5.1 to S5.3 and S6 of the [design](../design.md#s52-commit) state the rule in full.

### 6.2 The changed steps

The pseudocode states the two changed steps.
The first step pairs a fire, or marks it as unpaired.
It never fails, so staging no longer depends on the log.

```text
verify(comm, trigger) -> Option<COMM>
    if the mode is Play: return None
    (recorded, count) <- recorded_binding(trigger, comm), or return None
    available <- count / binding_multiplicity(trigger, comm), or 0 when the multiplicity is 0
    if paired(comm) < available: return recorded
    return None                          -- an unpaired fire
```

The second step commits a replayed operation.
The unpaired check comes after the charges, so a rejected charge gives the error of play.

```text
commit_replay(operation)
    refuse a native replay directive    -- NativeReplayDirectiveUnsupported
    observe the introduction            -- a rejection returns the error of play
    if the operation is a fire:
        observe the COMM                -- a rejection returns the error of play
        if the fire is unpaired:
            roll back the counter step of a produce
            fail with ReplayDivergence  -- no event, no store change, no binding change
    log the events, apply the effects, remove the bindings of the COMM
```

### 6.3 The code

The fix changes only two substrate files. Casper, the host, and the P4 agents do not change.

| Part | Change | Code |
|---|---|---|
| `TxnCore::verify` | It returns `Option<COMM>` and never fails: the recorded COMM of a paired fire, or `None` in play and for an unpaired fire. | `rspace++/src/rspace/space_store_handle.rs:2287-2315` |
| `TxnCore::stage_produce`, `TxnCore::stage_consume` | The two calls of `verify` no longer pass an error on, and only a paired fire adds its COMM to `paired`. | `rspace++/src/rspace/space_store_handle.rs:2479`, `2490-2492`, `2669`, and `1957-1959` |
| `StagedFire`, `StagedConsumeFire` | The field `recorded` keeps the result of `verify`. | `rspace++/src/rspace/space_store_handle.rs:2036-2038` and `2067-2069` |
| `ReplayBackend::produce_fire` | After the `observe_comm` call, an unpaired fire fails with `ReplayDivergence` before it logs anything. | `rspace++/src/rspace/replay_rspace/space_replay.rs:254-263` |
| `ReplayBackend::produce_observed` | A failed fire rolls the counter step back, also for an unpaired fire. | `rspace++/src/rspace/replay_rspace/space_replay.rs:209-211` |
| `ReplayBackend::consume_observed` | After the `observe_comm` call, an unpaired fire fails with `ReplayDivergence` before it logs anything. | `rspace++/src/rspace/replay_rspace/space_replay.rs:406-415` |
| `SpaceTxnError::ReplayDivergence`, `commit_staged` | The documentation states that the commit raises the error after the calls, with the prefix of play. | `rspace++/src/rspace/space_store_handle.rs:107-111` and `3353-3360` |

### 6.4 The proofs and the models

The [verification dossier](../verification.md#42-headlines-of-eventcorrelationv) records the headlines and the invariants of the fix:

- Headline H4 `verify_sound_complete` (`EventCorrelation.v:6261`) is restated. Its conjunct 3 `stage_without_trigger_unpaired` (`EventCorrelation.v:6197`) stages a fire without its trigger event unpaired.
- Its conjunct 6 `unpaired_fire_inert` (`EventCorrelation.v:6227`) shows that an unpaired fire never commits, and that it emits no event and no effect.
- The new headline H23 `replay_reproduces_play_run` (`EventCorrelation.v:7805`) covers replay with the commit log of play. That replay reproduces the calls, the events, the status, and the store of play.
- The new invariant T6 `ReplayOutcomeEqual` of `CheckpointReplay.tla` states that validators never reject an honest block. Its replayed deploy ends with the failure and the spending of play.
- The Unsafe control `StagingDivergenceUnsafe.cfg` restores the rule of decision 19 and refutes exactly T6. The control `AcceptUnpairedUnsafe.cfg` commits an unpaired fire and refutes exactly T1.
- Three Rocq mutants fail in the expected declarations: `ec-replay-fire-fails-at-staging`, `ec-unpaired-skips-observe-comm`, and `ec-unpaired-fire-commits`.

### 6.5 The tests

| Test | Location | What it checks |
|---|---|---|
| `prop_space_txn_replay_divergence_typed_at_commit` | `rspace++/tests/reified_space_store_handle_props.rs:2409` | Property 5, renamed (H4, T3): an unpaired fire stages with no host call, and the commit fails at it with `ReplayDivergence` after its calls. |
| `prop_space_txn_replay_requires_trigger_event` | `rspace++/tests/reified_space_store_handle_props.rs:2475` | Property 16 (H4): without its trigger event, a fire stages unpaired and fails at commit. |
| `prop_space_txn_replay_reproduces_play_rejection` | `rspace++/tests/reified_space_store_handle_props.rs:2483` | Property 25, new (H23, T6): replay rejects the call that play rejected, with the outcome, the calls, and the state of play. |
| `space_store_handle_replay_divergence_is_typed_at_commit` | `rspace++/tests/reified_space_store_handle.rs:1507` | The integration form of property 5, renamed. |
| `space_store_handle_replay_reproduces_play_rejection` | `rspace++/tests/reified_space_store_handle.rs:1997` | A sweep over every position of a fallible call (H23, T6). |
| `space_queue_out_of_phlo_sweep_replays_equal` | `rholang/tests/reified_queue_replay.rs:850` | The control of the fix: a queue deploy at many phlo limits replays to the error codes and the cost of play. |

The tests `space_store_handle_replay_cross_channel_comm_pairs_once`, `space_store_handle_replay_requires_produce_trigger_event`, and `space_store_handle_replay_requires_consume_trigger_event` now expect the error at commit.
The P4 test `space_queue_replay_equal` passes with no change.

### 6.6 Consequences

- A real divergence charges the introduction and the COMM of the unpaired fire before it fails. These charges never exceed the charges of an honest run.
- An agent cannot continue a transaction after an unpaired fire, because the commit always fails at it.
- The report of a failed deploy now holds the committed prefix, as play logged it.

The design pass compared other options:

| Option | Equal error and cost | Fails closed | Footprint |
|---|---|---|---|
| Unpaired at staging, resolved at commit (built) | Yes | Yes, at commit | 2 substrate files |
| Pairing moved to commit | Yes | Yes | Larger: H4 and H5 change |
| Store or park the fire, as host replay does | No: it copies the defect of section 7.1 | Late | P4 agent only |
| The agent catches the staging error | Yes, if every agent does it | Yes | A new `SpaceTxn` method |

## 7. Related observations outside the substrate

### 7.1 Host replay skips a rejected COMM charge

Host replay has the same mismatch when play rejects a COMM charge.
It stores the produce, or parks the consume, and it skips the rejected charge (`rspace++/src/rspace/replay_rspace.rs:1214-1219`).
A COMM is charged only when its cost authority has a non-Unit signature (`rholang/src/rust/interpreter/accounting/mod.rs:1794-1813`).
So only deploys with cost-signed syntax reach this case.
The pgmcp finding 9921 records it for the RSpace and cost-accounting owners.

### 7.2 A log that omits a COMM

Replay follows the log as the schedule of the deploy.
Host replay stores a datum whose COMM the log omits, and it does not look for a waiting match (`rspace++/src/rspace/replay_rspace.rs:1214-1219`).
Space replay instead fails the deploy with `ReplayDivergence`, at commit since decision 36.
Casper compares only the failed flag and the cost, so in both cases validators accept a log that omits a COMM.
At the baseline, a proposer can thus suppress a COMM, and with spaces it can make a space deploy fail.
Neither case forks honest validators.

This observation is risk R1 of the fix, and the fix does not change it.
Decision 36 leaves it to its owners, and the campaign does not pursue it.

## 8. Open questions

The fix closes the first question of the earlier list. Two questions stay open, and neither belongs to this campaign ([decision 36](../decision-register.md#decision-36)):

1. The forged omission of section 7.2. Decide whether a space replay divergence should fail only the deploy, as now, or the whole block. The question belongs to the owners of Casper and RSpace.
2. Host replay for a rejected COMM charge (finding 9921). The question belongs to the owners of RSpace and cost accounting. The native-replay directives `RejectedIntroduction` and `RejectedComm` are a model, because they make replay repeat the rejected call.

## 9. References

- [Decision register, decision 36](../decision-register.md#decision-36), [decision 19](../decision-register.md#decision-19), and [decision 31](../decision-register.md#decision-31).
- [Design, section S5.1 (staging)](../design.md#s51-staging), [section S5.2 (commit)](../design.md#s52-commit), [section S5.3 (verify)](../design.md#s53-verify-fr-3), [section S6 (errors)](../design.md#s6-errors), and [section S10 (limits)](../design.md#s10-limits).
- [Verification dossier](../verification.md): the [headlines of EventCorrelation.v](../verification.md#42-headlines-of-eventcorrelationv), the [invariants of CheckpointReplay.tla](../verification.md#52-checkpointreplaytla), and the [witness tests](../verification.md#73-the-19-witness-tests).
- [Glossary entry "Unpaired fire"](../glossary.md#unpaired-fire).
