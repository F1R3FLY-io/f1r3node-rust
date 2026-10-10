# Reified RSpaces design

This document describes how the campaign builds Reified RSpaces in the node.
Each phase of the campaign adds a section, and the [README](README.md#campaign-phases) lists the phases.
Phase P1 writes the [substrate](#substrate) section, which describes the storage layer of spaces in `rspace++`.
Phase P4 writes the section [Native spaces](#native-spaces), which describes the first working space, of the family `hashmap:queue`.
Later phases add the other disciplines and outer stores, the theory seam, and the other parts.

Read the [glossary](glossary.md) first, because this document uses its terms and links to its entries.
The [decision register](decision-register.md) is normative, and this document follows it.

## Substrate

The substrate is the part of `rspace++` through which a [space agent](glossary.md#space-agent) reads and changes the state of a [space](glossary.md#space).
It consists of three new source files and four one-line edits of existing `rspace++` files ([decision 19](decision-register.md#decision-19)).
This section describes the substrate as phase P1 delivers it, after three fix rounds that followed an independent review.

The fix rounds introduced the fix rules FR-1 to FR-10.
The second round amended four of them, as FR-1', FR-5', FR-7', and FR-9'.
The comments of the code and the model maps cite the rules by these names, and Table S0 lists them.

| Rule | Subject | Section |
|---|---|---|
| FR-1, FR-1' | One canonical order for each channel group. FR-1' allows a repeated channel ([decision 28](decision-register.md#decision-28)). | [S5.5](#s55-canonical-order-fr-1) |
| FR-2 | Every map and set of the substrate uses the store identity of a channel. | [S2](#s2-defined-terms) |
| FR-3 | Replay pairs a fire only under its trigger key. | [S5.3](#s53-verify-fr-3) |
| FR-4 | A transaction holds the lock stripe of every channel that it touches. | [S5.6](#s56-lock-coverage-fr-4) |
| FR-5, FR-5' | At most one transaction commits under each parent order in one evaluation scope. | [S5.7](#s57-parent-tracking-fr-5) |
| FR-6 | The counter arithmetic is checked. | [S4](#s4-counter-timing) |
| FR-7, FR-7' | The commit checks run in every build profile, and their cost grows only with the changed entries. | [S5.2](#s52-commit), [S10](#s10-limits) |
| FR-8 | The sweep counts what it removes and keeps the replay bookkeeping. | [S5.4](#s54-sweep) |
| FR-9, FR-9' | A nested transaction or sweep in one task fails closed. | [S7](#s7-locking-and-ordering) |
| FR-10 | A peek fire does not store the sent value, and the agent re-offers it ([decision 30](decision-register.md#decision-30)). Since [decision 45](decision-register.md#decision-45), a peek fire also removes its stored records as the host does, and the agent stores them again. | [S5.8](#s58-peek-re-offer-fr-10) |

### S1. Purpose and the rule of no new ISpace surface

A space is a tuple space in the sense of Linda [[Gel85](#ref-gel85)], which a Rholang program creates and uses through sends and receives [[MR05](#ref-mr05)].
It persists as ordinary data and continuations at [storage channels](glossary.md#storage-channel) in the one [RSpace](glossary.md#rspace) of the node ([decision M-2](decision-register.md#m-2)).
So each space operation must act as a [host operation](glossary.md#host-operation) of an equivalent Rholang program.
It must make the same [accounting observer](glossary.md#accounting-observer) calls, log the same events, and make the same store changes, in the same order.
Then the existing checkpoint, replay, merge, and charging machinery covers spaces, and the consensus code does not change ([decision 18](decision-register.md#decision-18), [M-3](decision-register.md#m-3)).

The substrate gives a space agent this equivalence.
The agent reads a [staged view](glossary.md#staged-view), decides the [outcome](glossary.md#outcome) of each operation, and stages whole operations in a [space transaction](glossary.md#space-transaction).
The substrate validates each operation when the agent stages it.
At commit, it applies the operations with the calls of the host operations, in the host order ([S3](#s3-host-equivalence)).
It owns these call sequences, so an agent cannot break the equivalence with a raw write or a raw log call ([decision 19](decision-register.md#decision-19), D2).

The public surface has three parts, all in [space_store_handle.rs](../../../rspace++/src/rspace/space_store_handle.rs):

- `SpaceTxnHost<C, P, A, K>` is a trait that each concrete host implements. `RSpace` implements it for play, `ReplayRSpace` for replay, and `ReportingRspace` for [reporting](glossary.md#reporting). Its methods are `mode`, `matcher`, `begin`, `root_leaf_record_count`, and `sweep_volatile`.
- `SpaceStoreHandle<C, P, A, K>` is a struct over a shared host. From P4, the runtime keeps one handle for each host. Every transaction and every sweep starts through the handle, which refuses nested use and records the committed parent orders.
- `SpaceTxn<C, P, A, K>` is a trait for one open transaction. It offers the reads of the staged view, the lookups of replay, the staging methods `produce` and `consume`, and `commit`.

The type parameters are the channel `C`, the pattern `P`, the datum `A`, and the continuation `K` of the host.
The traits stay generic, because `rspace++` cannot name the `Par` type of the `models` crate (decision 19, D2).
Rholang uses the instance over `Par`, `BindPattern`, `ListParWithRandom`, and `TaggedContinuation`.
The test `reified_space_txn_host_bounds_hold_for_par_hosts` builds a handle over each host of this instance and commits an empty transaction.

**No new `ISpace` surface.** Spaces are not `ISpace` implementors ([I-1](decision-register.md#i-1)).
The substrate adds no method to `ISpace`, and it changes no `ISpace` implementor.
The three hosts implement `SpaceTxnHost` in two new child modules, `space_play` and `space_replay`.
A child module can use the private items of its parent module, so no host item changes its visibility ([M-4](decision-register.md#m-4)).
The one exception is the field `replay_rspace` of `ReportingRspace`, which becomes `pub(crate)`, so that the reporting host can delegate to its inner replay space.

Table S1.1 lists every P1 edit of an existing file.
The footprint policy `p1` checks that these edits are the only changes of the pinned files ([S8](#s8-what-is-unchanged)).

| File | Edit |
|---|---|
| `rspace++/src/rspace/mod.rs` | Adds `pub mod space_store_handle;`. |
| `rspace++/src/rspace/rspace.rs` | Adds `mod space_play;`. |
| `rspace++/src/rspace/replay_rspace.rs` | Adds `mod space_replay;` after line 49. |
| `rspace++/src/rspace/reporting_rspace.rs` | Makes the field `replay_rspace` `pub(crate)`. |
| `scripts/audit/check-rocq-line-refs.sh` | Replaces the dead validator tree with the Reified theories ([decision 14](decision-register.md#decision-14)). |

Table S1.2 lists the new source files.

| File | Content |
|---|---|
| `rspace++/src/rspace/space_store_handle.rs` | The public API, the staged view `TxnView`, the staging core `TxnCore`, the commit loop, the event correlation, and the sweep. |
| `rspace++/src/rspace/rspace/space_play.rs` | The play backend `PlayBackend`, and `SpaceTxnHost` for `RSpace`. |
| `rspace++/src/rspace/replay_rspace/space_replay.rs` | The replay backend `ReplayBackend`, and `SpaceTxnHost` for `ReplayRSpace` and `ReportingRspace`. |

Figure S1 shows the components of the substrate and their links to the host.

![Substrate components. Top: the callers outside P1, the space agent (P4) and the reducer hooks begin_evaluation_scope and sweep_volatile (P4 and P7). Middle: the substrate of P1. The SpaceStoreHandle holds the shared HandleState, which records the open contexts (FR-9) and the committed parent orders (FR-5'). Through the SpaceTxnHost trait the handle opens a HostTxn, which holds the TxnCore (the staged view TxnView keyed by the store identity Blake2b256 of the bincode of a channel, the counter view, the lock coverage, the installed guard, the paired COMM events and the staged operations with their staged effects). The trait also runs sweep_volatile_channels. The commit, commit_staged, runs operation i under parent ++ [(i, 2)] through a mode backend, PlayBackend or ReplayBackend. RSpace, ReplayRSpace and ReportingRspace implement the trait, and ReportingRspace delegates to its inner replay space. Bottom: the baseline host that the substrate calls and never changes: the lock stripes, the accounting observer, the event log, the replay table with its bindings, and the reporting logger, above the hot store with history read-through and the history trie. A note lists the frozen modules: ISpace, ispace_impl.rs, hot_store.rs, trace, serializers, history, merger and casper/src.](diagrams/01-substrate-components.svg)

*Figure S1. Source: [diagrams/01-substrate-components.puml](diagrams/01-substrate-components.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

**Callers.** In P1, no production path calls the substrate, so ordinary execution does not change ([S8](#s8-what-is-unchanged)).
Phase P4 captures a handle in the runtime, and its space agents call the handle.
The reducer calls `begin_evaluation_scope` at the entry of each root evaluation (P4), and `sweep_volatile` at its end (P7).
Both calls go in `ReducerCore::inj_with_observation` and `ReducerCore::inj` (decision 19, D1).

### S2. Defined terms

Table S2.1 gives the terms of this section in short form.
Each term links to its glossary entry, which gives the full definition.

| Term | Meaning in the substrate |
|---|---|
| [Host operation](glossary.md#host-operation) | A produce, a consume, or a COMM event that the host RSpace performs for ordinary Rholang. |
| [Outcome](glossary.md#outcome) | Store or Fire for a produce, and Park or Fire for a consume. The agent decides it, and the substrate checks it. |
| [Staged view](glossary.md#staged-view) | The hot store with the effects of the staged operations applied. Each map of the view uses the store identity as its key. |
| [COMM](glossary.md#comm-event) | The event of one synchronization: the consume identity, the sorted produce identities, the peeks, and the times-repeated map. |
| [Counter](glossary.md#produce-counter) $`\kappa`$ | The produce counter: the non-persistent produces of one identity since the node last took the event log. |
| [Times-repeated](glossary.md#times-repeated) | The map from each produce of a COMM to its counter value at the COMM. |
| [Operation order](glossary.md#operation-order) and [sub-order](glossary.md#sub-order) | The scheduler key of an RSpace operation, and the key $`\mathit{parent} \mathbin{+\!\!+} [(i, 2)]`$ of operation $`i`$ of a transaction. |
| [Binding](glossary.md#binding) | An entry of the replay table that links an IO event of the play log to a COMM of that log. |
| [Pairing](glossary.md#pairing) | The replay step that matches a fire with one free recorded occurrence of its COMM, under the trigger key. |
| [Retention](glossary.md#retention) | Superseded by [decision 45](decision-register.md#decision-45). A peek fire kept the stored records that it binds in place. |
| [Re-offer](glossary.md#re-offer) | The second produce of a datum that a peek fire did not store. |
| [Re-store](glossary.md#re-store) | The second store of a stored record that a peek fire removed, in the same transaction ([decision 45](decision-register.md#decision-45)). |
| [Volatile channel](glossary.md#volatile-channel) | A storage channel with a lowercase KIND, which lives for one deploy. |
| [Sweep](glossary.md#sweep) | The removal of the volatile channels of a deploy at its end. |
| [Canonical group](glossary.md#canonical-group) | A channel group whose store identities are non-decreasing. |
| [Store identity](glossary.md#store-identity) | The hash $`\mathrm{Blake2b256}(\mathrm{bincode}(c))`$ of a channel $`c`$, and the list of these hashes for a group. |
| [Lock coverage](glossary.md#lock-coverage) | The transaction holds the stripe of each touched channel in each phase that the host operation locks. |
| [Evaluation scope](glossary.md#evaluation-scope) | One root evaluation of a deploy. The tracker of committed parent orders resets at its start. |

Table S2.2 gives the related terms that this section also uses.

| Term | Meaning in the substrate |
|---|---|
| [Trigger event](glossary.md#trigger-event) | The IO event of the operation that fires. Its key in the replay table is the trigger key. |
| [Unpaired fire](glossary.md#unpaired-fire) | A replay fire without a free recorded occurrence of its COMM. It stages with no error, and its commit fails after the observer calls of play. |
| [Prefix atomicity](glossary.md#prefix-atomicity) | A failure of operation $`i`$ at commit leaves the operations $`0`$ to $`i - 1`$ applied. |
| [Lock stripe](glossary.md#lock-stripe) | One of the 4096 mutexes of phase A or of phase B. |
| [Accounting observer](glossary.md#accounting-observer) | The charging interface: start, produce or consume, COMM, and finish. |
| [Reporting](glossary.md#reporting) | A replay that records each event in a report. |

The section also uses two terms of its own.
A *staged effect* is one entry change that a staged operation makes in the staged view.
It is a stored or removed datum, a parked or removed continuation, or an added or removed join.
A *receipt* is the result of one committed operation: `Stored`, `Parked`, or `Fired` with the COMM, the continuation, and the results.

Table S2.3 defines the symbols of the formulas and of the pseudocode.

| Symbol | Meaning |
|---|---|
| $`p`$ | A produce identity, `Produce::create(channel, data, persist)`. |
| $`\mathit{src}`$, $`\mathit{persist}`$ | The identity and the persist flag of the produce that is staged or committed. |
| $`\mathit{side}`$ | The side of a fire: produce for a produce-side fire, and consume for a consume-side fire. |
| $`n(p)`$ | The number of non-persistent produces of $`p`$ that the deploy performed before the current operation, in commit order. |
| $`\kappa_{\mathrm{host}}(p)`$ | The produce counter of the host. |
| $`\delta(p)`$ | The number of non-persistent produces of $`p`$ that the open transaction staged before the current operation. |
| $`\kappa_v(p)`$ | The counter that the staged view shows, $`\kappa_{\mathrm{host}}(p) + \delta(p)`$. |
| $`\mathit{times}(p)`$ | The times-repeated entry of $`p`$ in a COMM. |
| $`[\varphi]`$ | The Iverson bracket: 1 when the condition $`\varphi`$ holds, and 0 otherwise. |
| $`b`$, $`m`$ | The number of bindings of a COMM under a key, and the binding multiplicity of that key for the COMM. |
| $`\mathit{parent}`$, $`i`$ | The operation order under which a transaction commits, and the index of an operation in the transaction. |

### S3. Host equivalence

The commit applies each staged operation through the backend of its mode.
The backend makes the calls of the equivalent host operation, in the host order.
Tables S3.1, S3.2, and S3.3 give each step for play, replay, and reporting, with the baseline line that it mirrors.

The line numbers refer to the baseline commit `e748426`, which the plan calls HEAD.
The P1 hunk of `replay_rspace.rs` adds one line after line 49, so a later line of that file is one higher in the P1 tree.
A path without a crate prefix is relative to `rspace++/src/rspace/`.

Every mode first runs the read-only commit checks of [S5.2](#s52-commit).
The host has no such step, because it matches under its own locks at the moment of the operation.
A failed check leaves the operation without any effect.

Table S3.1 gives the play steps of `PlayBackend` in `rspace/space_play.rs`, over the host `RSpace`.

| Step | Produce | HEAD | Consume | HEAD |
|---|---|---|---|---|
| 1 | Run the callback `before_observe` of the agent with the produce identity. It takes the place of `register_introduction_authority` in the reducer. | `rholang/src/rust/interpreter/reduce.rs:548-552` | The same, with the consume identity. | `rholang/src/rust/interpreter/reduce.rs:660-664` |
| 2 | `start_accounting_operation` with the channel and its join groups. | `rspace/ops_produce.rs:44-53` | `start_accounting_operation` with the channels and no joins. | `rspace/ops_consume.rs:44-45` |
| 3 | `observe_produce`. | `rspace/ops_produce.rs:78` | `observe_consume`. | `rspace/ops_consume.rs:73-81` |
| 4, Store or Park | `log_produce`, which logs the event and steps the counter of a non-persistent produce. Then `store_data`, which calls `put_datum`. | `rspace/ops_produce.rs:105-106`, `212-226`, `rspace/trace_log.rs:158-172` | `log_consume`, then `store_waiting_continuation`: `put_continuation`, and then `put_join` for each channel. | `rspace/ops_consume.rs:146-147`, `179-190` |
| 5, Fire | `observe_comm` with the COMM whose counter closure counts the produce itself. | `rspace/ops_produce.rs:185-198` | `observe_comm` with the COMM of the plain counters. | `rspace/ops_consume.rs:126-135` |
| 6, Fire | `log_produce`, then `log_comm` with `PRODUCE_COMM_LABEL`. | `rspace/ops_produce.rs:199-200` | `log_consume`, then `log_comm` with `CONSUME_COMM_LABEL`. | `rspace/ops_consume.rs:136-137` |
| 7, Fire | Remove a non-persistent continuation at its live index. | `rspace/ops_produce.rs:202-205` | None. | |
| 8, Fire | `remove_matched_datum_and_join`: in descending index order, remove each stored non-persistent record, and then the fired join of its channel. A peek fire does the same ([decision 45](decision-register.md#decision-45)). | `rspace/ops_produce.rs:207`, `228-248` | `store_persistent_data`: in descending index order, remove each stored non-persistent record. A peek fire does the same. | `rspace/ops_consume.rs:138`, `192-202` |
| 9, Fire | The receipt from `wrap_result`. Each result keeps the persist flag of the host ([S5.8](#s58-peek-re-offer-fr-10)). | `rspace/ops_consume.rs:204-229` | The same. | `rspace/ops_consume.rs:204-229` |
| 10 | `observe_operation_finish` with `Stored`, `Matched`, or `Rejected`. | `rspace/ops_produce.rs:62-65` | The same. | `rspace/ops_consume.rs:55-58` |

The play store steps `store_data`, `remove_matched_datum_and_join`, `store_waiting_continuation`, and `store_persistent_data` are private to their sibling modules.
So `space_play.rs` restates them statement for statement over the public `HotStore` API (decision 19, D7).
The host-parity tests compare the results with the host.

Table S3.2 gives the replay steps of `ReplayBackend` in `replay_rspace/space_replay.rs`, over the host `ReplayRSpace`.
Its HEAD columns cite `replay_rspace.rs`.

| Step | Produce | HEAD | Consume | HEAD |
|---|---|---|---|---|
| 1 | Run `before_observe`, as in Table S3.1. | `rholang/src/rust/interpreter/reduce.rs:548-552` | The same, with the consume identity. | `rholang/src/rust/interpreter/reduce.rs:660-664` |
| 2 | `start_accounting_operation` with the channel and its join groups. | `1053-1059` | `start_accounting_operation` with the channels and no joins. | `760-761` |
| 3 | A replay directive of the observer fails the operation with `NativeReplayDirectiveUnsupported`. The host would run the operation natively instead. | `1084-1100` | The same. | `788-805` |
| 4 | `observe_produce`. | `1101` | `observe_consume`. | `806-814` |
| 5 | `increment_produce_counter`, before any match. | `1103`, `1488-1494` | None. | |
| 6, Store or Park | `log_produce` to the logger, then `store_data`. | `1126-1127`, `1170-1171`, `1579-1593` | `log_consume` to the logger, then `store_waiting_continuation`, which also raises the estimate of waiting continuations. | `852-853`, `896-904`, `914-922`, `1561-1577` |
| 7, Fire | `observe_comm`. | `1312` | `observe_comm`. | `944-950` |
| 7a, unpaired Fire | Stop with `ReplayDivergence` after `observe_comm` accepts ([decision 36](decision-register.md#decision-36)). The fire logs no event, changes no store entry or binding, and step 13 rolls its counter step back. | None | The same, with no counter step. | None |
| 8, Fire | `log_produce`, then `log_comm` with the label `comm.produce`. | `1314-1321` | `log_consume`, then `log_comm` with the label `comm.consume`. | `952-966` |
| 9, Fire | Remove a non-persistent continuation. In both cases, call `mark_replay_waiting_continuation_match`. | `1323-1329` | None. | |
| 10, Fire | `remove_matched_datum_and_join`, also for a peek fire. | `1331`, `1707-1727` | `store_persistent_data`, also for a peek fire. | `968`, `1595-1605` |
| 11, Fire | `remove_bindings_for` the COMM. | `1332`, `1407-1435` | The same. | `969` |
| 12, Fire | The receipt from `wrap_result`. Its produce is the copy that the paired COMM recorded. | `1150-1161`, `1679-1705` | The receipt from `wrap_result`. | `971` |
| 13 | After a failed fire, `rollback_produce_counter`. | `1163-1165`, `1496-1509` | None. | |
| 14 | `observe_operation_finish`. | `1068-1071` | The same. | `771-774` |

Host replay decides the outcome of an operation from the log.
It fires only when the log holds a matching COMM, and otherwise it stores the datum or parks the continuation (`replay_rspace.rs:1215-1219`, `985-996`).
It also asserts that the COMM it computes is in the recorded list, and it panics otherwise (`replay_rspace.rs:1026-1033`, `1396-1401`).

A space agent decides the outcome from the staged view, as in play, so it can stage a fire that the log does not hold.
Staging then pairs nothing for the fire, and it reports no error ([S5.3](#s53-verify-fr-3)).
At commit, a rejected observer call fails the fire with the error of play.
Otherwise step 7a fails the fire with the typed error `ReplayDivergence`.

Table S3.3 gives the reporting steps.

| Step | Reporting | HEAD |
|---|---|---|
| Host | `ReportingRspace` implements `SpaceTxnHost` with the mode `Reporting`. It delegates `begin`, `root_leaf_record_count`, and `sweep_volatile` to its inner replay space, the field `replay_rspace`. | `reporting_rspace.rs:121` |
| Calls | The steps of Table S3.2, through `ReplayBackend`. | `replay_rspace.rs`, as Table S3.2 cites it |
| Report | The inner replay space carries the `ReportingLogger`. So the calls `log_produce`, `log_consume`, and `log_comm` of steps 6 and 8 add the events `ReportingProduce`, `ReportingConsume`, and `ReportingComm` to the report, in commit order. | `reporting_rspace.rs:162-185`, `481-569` |

**Differences by decision.** The substrate differs from the host in four places, and a decision covers each one.

1. Directives. In replay and reporting, a native replay directive of the observer fails the operation closed ([decision 19](decision-register.md#decision-19), D4). Native replay itself runs a space operation through the native backend ([N14](#n14-decision-47-spaces-in-native-execution)), which [I-13](decision-register.md#i-13) excluded until decision 47.
2. Divergence. Replay pairs each fire at staging, and it resolves an [unpaired fire](glossary.md#unpaired-fire) at commit ([decision 36](decision-register.md#decision-36)). The commit makes the calls of play first, so a rejected call gives the error and the cost of play. Otherwise the fire fails with the typed error `ReplayDivergence`, not with a panic, and it stores and parks nothing.
3. Candidate choice. The agent chooses the continuation and the candidates, and the substrate validates any choice. The host takes the first match of its deterministic shuffle.
4. Consume-side pairing. A consume-side fire pairs only under its trigger key, which is stricter than host replay ([S10](#s10-limits)).

[Retention](glossary.md#retention) was a fifth difference until [decision 45](decision-register.md#decision-45).
A peek fire kept its stored records in place, while the host removes them and the reducer produces them again.
Since decision 45, a peek fire removes them as the host does, and the agent stores each one again ([S5.8](#s58-peek-re-offer-fr-10)).

The peek results are not a difference.
Neither the host nor the substrate stores the incoming datum of a produce-side peek fire.
The host reducer produces it again, and the agent re-offers it.
Both remove the stored non-persistent records that a peek fire binds, and the reducer or the agent sends them again.

### S4. Counter timing

Replay must reproduce the times-repeated map of each COMM, so play and replay must count a fired produce at the same point.
The host does this with two different rules.
Play computes a produce-side COMM before `log_produce` steps the counter, so its closure adds one for the produce itself (`rspace/ops_produce.rs:185-191`).
Replay steps the counter before it matches (`replay_rspace.rs:1194`), so its plain counters already count the produce.
With the symbols of Table S2.3, both rules give the same value:

```math
\mathit{times}(p) \;=\; n(p) + \bigl[\, \mathit{side} = \mathrm{produce} \;\wedge\; \neg\,\mathit{persist} \;\wedge\; p = \mathit{src} \,\bigr]
```

Inside an open transaction, the host counter does not yet count the staged produces.
The staged view adds them, so that the view counter equals $`n(p)`$ at each staged operation:

```math
\kappa_v(p) \;=\; \kappa_{\mathrm{host}}(p) + \delta(p) \;=\; n(p)
```

Staging computes $`\mathit{times}(p)`$ from $`\kappa_v(p)`$ by the first equation, with the play rule.
After a non-persistent produce, staging records the step $`\delta(\mathit{src}) \leftarrow \delta(\mathit{src}) + 1`$.
A consume steps no counter.
Each step uses checked arithmetic, and a counter above $`2^{31} - 1`$ fails with `CounterOverflow` before any effect (FR-6).

At commit, `live_comm` computes the COMM again from the live host counters, by the play rule.
The operations $`0`$ to $`i - 1`$ are then applied, so the live counter equals $`n(p)`$ for operation $`i`$.
A different COMM fails the operation with `Inconsistent` before any effect ([S5.2](#s52-commit)).
Lock coverage keeps every other operation away from the counted channels until the commit ends ([S5.6](#s56-lock-coverage-fr-4)).

For example, the queue scenario of [S5.8](#s58-peek-re-offer-fr-10) sends one datum $`d`$ twice in one transaction, under one identity.
The peek fire records $`\mathit{times}(d) = 1`$, and the re-offer records $`\mathit{times}(d) = 2`$.

### S5. Algorithms

The algorithms below follow the literate form of Knuth [[Knu84](#ref-knu84)].
Each algorithm starts with its purpose and an outline.
The outline names its steps in angle brackets, and the text then defines each named step, with its explanation before its code.
The pseudocode follows the Rust code of `space_store_handle.rs` and of the two backends.

Table S5.0 gives the notation of the pseudocode.

| Notation | Meaning |
|---|---|
| `x ← e` | Assign the value of `e` to `x`. |
| `require c   ▷ E` | Fail with the error `E` when the condition `c` is false. The failure stops the algorithm. |
| `fail E` | Stop with the error `E`. |
| `▷ text` | A comment. After a check, it names the error of the check. |
| `[a] ++ l` | The list `l` with `a` at its head. |
| `\|l\|` | The length of the list `l`. |
| `⟨Name⟩` | A named step, which the text defines after the outline. |
| `κ_v`, `δ`, `src` | The symbols of Table S2.3. |

#### S5.1. Staging

Staging validates one operation and applies its effects to the staged view at once.
Later reads and operations of the same transaction then see these effects.
Nothing touches the host before commit.
Every check of a group runs before the first hot-store access under the key of that group.

Staging never fails for pairing ([decision 36](decision-register.md#decision-36)).
In replay, a fire without a free recorded occurrence of its COMM stages as play stages it, and it stays an [unpaired fire](glossary.md#unpaired-fire).
So replay stages exactly the operations of play, for every log, and the commit decides the outcome of an unpaired fire ([S5.2](#s52-commit)).

The outline of a produce has five named steps.

```text
STAGE-PRODUCE(channel, data, persist, outcome, before_observe):
    require the transaction is open                              ▷ Finished
    ⟨Admit the produce channel⟩
    src ← Produce::create(channel, data, persist)
    if outcome is Fire(channels, wk, candidates):
        ⟨Admit the fired group and the candidates⟩
    ⟨Load the joins and the data of the produce channel⟩
    if outcome is Store:
        ⟨Stage a store⟩
    else:
        ⟨Stage a produce-side fire⟩
```

⟨Admit the produce channel⟩.
The produce channel needs its phase-A stripe ([S5.6](#s56-lock-coverage-fr-4)).
It must not belong to an installed system process.

```text
    key ← STORE-IDENTITY(channel)
    REQUIRE-COVERAGE(channel, Produced)                          ▷ UnlockedChannel
    require key ∉ installed                                      ▷ InstalledChannel
```

⟨Admit the fired group and the candidates⟩.
The fired group passes the group checks of [S5.5](#s55-canonical-order-fr-1) with phase-B coverage, and it must contain the produce channel.
The peeks of the waiting continuation must be empty, or they must cover every bind.
Candidate $`j`$ must bind channel $`j`$ of the group.
Exactly one candidate must have the index $`-1`$ and carry the produced record: its channel, its source, its data, and its persist flag.

```text
        group ← ADMIT-GROUP(channels, |wk.patterns|, Joined)     ▷ S5.5
        require key ∈ group                                      ▷ InvalidOperation
        CHECK-PEEKS(|channels|, wk.peeks)                         ▷ MixedArrowJoin
        require |candidates| = |group|, and candidate j binds group[j]    ▷ InvalidOperation
        require no index is below −1, and exactly one candidate has the index −1
            and carries (channel, src, data, persist)            ▷ InvalidOperation
```

⟨Load the joins and the data of the produce channel⟩.
A host produce reads the join groups of its channel, to lock them and to match.
The substrate loads them through the staged view, checks that each stored group is canonical, and requires the phase-B stripe of every joined channel.
It keeps the groups as `joins_at_start`, which the commit compares with the live groups.

```text
    joins_at_start ← LOAD-JOINS(key)                             ▷ NonCanonicalChannelOrder
    for each group g in joins_at_start, and each channel c of g:
        REQUIRE-COVERAGE(c, Joined)                              ▷ UnlockedChannel
    LOAD-DATA(key)
```

⟨Stage a store⟩.
A store inserts the record at the head of the data list, as `put_datum` does.
A non-persistent produce also steps its staged counter ([S4](#s4-counter-timing)).

```text
        δ′ ← NEXT-DELTA(src, persist)                            ▷ CounterOverflow
        view.data[key] ← [Datum(data, persist, src)] ++ view.data[key]
        if δ′ ≠ none: δ(src) ← δ′
        append (the operation, before_observe, effects = [PutDatum(src)])
        return (index, Produce(src), no COMM)
```

⟨Stage a produce-side fire⟩.
A fire loads the data and the joins of every channel of the group, and the continuations of the group.
The waiting continuation must be in the view, and each stored candidate must bind the record at its index.
The substrate then computes the COMM by the play rule of [S4](#s4-counter-timing), and it verifies the COMM in replay ([S5.3](#s53-verify-fr-3)).

Verification returns the recorded COMM of a paired fire, and none in play or for an unpaired fire.
It never fails, and only a paired fire adds its COMM to `paired`.
The staged fire keeps the result as its field `recorded`, which the commit reads.
Last, staging applies the effects to the view in the host order.
The fire never stores the incoming datum ([S5.8](#s58-peek-re-offer-fr-10)).

```text
        LOAD-GROUP(group)                                        ▷ NonCanonicalChannelOrder
        require a continuation of view.conts[group] is byte-equal to wk   ▷ NotInView
        CHECK-STORED-CANDIDATES(group, candidates)               ▷ NotInView
        positions ← CLAIM-POSITIONS(group, candidates)
        comm ← COMM::new(candidates, wk.source, wk.peeks, t)
            with t(p) = κ_v(p) + [¬persist ∧ p = src]            ▷ CounterOverflow
        recorded ← VERIFY(comm, Produce(src))    ▷ none in play, and for an unpaired fire
        δ′ ← NEXT-DELTA(src, persist)                            ▷ CounterOverflow
        removed ← REMOVED-RECORDS(candidates, positions)
        effects ← APPLY-FIRE-TO-VIEW(group, wk, candidates, removed)
        if δ′ ≠ none: δ(src) ← δ′
        if recorded ≠ none: paired ← paired ++ [comm]
        append (the operation with its fire and recorded, before_observe, effects)
        return (index, Produce(src), comm)
```

The outline of a consume has two named steps.

```text
STAGE-CONSUME(channels, patterns, k, persist, peeks, outcome, before_observe):
    require the transaction is open                              ▷ Finished
    group ← ADMIT-GROUP(channels, |patterns|, Consumed)          ▷ S5.5
    CHECK-PEEKS(|channels|, peeks)                               ▷ MixedArrowJoin
    src ← Consume::create(channels, patterns, k, persist)
    if outcome is Park:
        ⟨Stage a park⟩
    else:
        ⟨Stage a consume-side fire⟩
```

⟨Stage a park⟩.
A park loads the continuations of the group and the joins of every channel.
It inserts the continuation at the head of the list, unless the list holds one with the same dedup identity.
Then it adds the group to the joins of each channel that lacks it.
These are the list rules of `put_continuation` and `put_join` (`hot_store.rs:720-783`, `1017-1040`).

```text
        LOAD-CONTINUATIONS(group)
        for each key ck of group: LOAD-JOINS(ck)                 ▷ NonCanonicalChannelOrder
        wk ← WaitingContinuation(patterns, k, persist, peeks, src)
        effects ← []
        if no continuation of view.conts[group] has the dedup identity of wk:
            view.conts[group] ← [wk] ++ view.conts[group]
            effects ← effects ++ [PutContinuation(group, src)]
        for each key ck of group:
            if group ∉ view.joins[ck]:
                view.joins[ck] ← [group] ++ view.joins[ck]
                effects ← effects ++ [PutJoin(ck, group)]
        append (the operation, before_observe, effects)
        return (index, Consume(src), no COMM)
```

⟨Stage a consume-side fire⟩.
A consume-side fire binds stored records only.
The COMM uses the plain view counters, because a consume steps no counter.
Verification follows the rule of a produce-side fire, under the trigger key of the consume.
A peek fire removes its stored non-persistent records too, as the host does ([decision 45](decision-register.md#decision-45)).

```text
        require |candidates| = |group|, and candidate j binds group[j]    ▷ InvalidOperation
        require every candidate has an index of 0 or more         ▷ InvalidOperation
        for each key ck of group: LOAD-DATA(ck)
        CHECK-STORED-CANDIDATES(group, candidates)               ▷ NotInView
        positions ← CLAIM-POSITIONS(group, candidates)
        comm ← COMM::new(candidates, src, peeks, κ_v)            ▷ CounterOverflow
        recorded ← VERIFY(comm, Consume(src))    ▷ none in play, and for an unpaired fire
        removed ← REMOVED-RECORDS(candidates, positions)
        effects ← REMOVE-RECORDS(group, candidates, removed)
        if recorded ≠ none: paired ← paired ++ [comm]
        append (the operation with its fire and recorded, before_observe, effects)
        return (index, Consume(src), comm)
```

The helpers apply the list rules of the hot store.
`CHECK-STORED-CANDIDATES` requires that each stored candidate names the record at its index, byte for byte.
It lets a fire bind a non-persistent record at most once, also when the group repeats its channel, and a persistent record more than once.
`CLAIM-POSITIONS` finds, for each stored candidate, the first byte-equal record of its channel that no earlier non-persistent candidate claimed.
The staged view and the commit locate records with this one rule.

```text
NEXT-DELTA(src, persist):
    if persist: return none
    require κ_v(src) + 1 ≤ 2^31 − 1                              ▷ CounterOverflow
    return δ(src) + 1

REMOVED-RECORDS(candidates, positions):
    return [(j, positions[j]) for each stored non-persistent candidate j]

APPLY-FIRE-TO-VIEW(group, wk, candidates, removed):
    effects ← []
    if not wk.persist:
        remove the first continuation of view.conts[group] that is byte-equal to wk
        effects ← effects ++ [RemoveContinuation(group, wk.source)]
    effects ← effects ++ REMOVE-RECORDS(group, candidates, removed)
    for each key ck of group:
        if view.conts[group] = [] and group ∈ view.joins[ck]:
            remove group from view.joins[ck]
            effects ← effects ++ [RemoveJoin(ck, group)]
    return effects                       ▷ the incoming datum is never stored

REMOVE-RECORDS(group, candidates, removed):
    for each (j, q) in removed, the highest q first:
        remove the record at q from view.data[group[j]]
        add RemoveDatum(candidates[j].source) to the effects
    return the effects
```

A view change that fails after validation leaves the view unusable.
So the transaction closes at once and releases its locks, and the error names the failed step, for example `Inconsistent`.
Such a failure is a defect of the substrate.

The peeks rule admits a peek set. It decides nothing about the removal, because every fire removes its stored non-persistent records ([decision 45](decision-register.md#decision-45)):

```rust
/// Peeks rule of an operation with `binds` channels: no peeks, or
/// `{0..binds-1}` (every bind peeks). Any other set is `MixedArrowJoin`. A peek
/// fire removes the same records as any other fire, because the host COMM
/// removes every stored non-persistent candidate whatever its peeks (decision
/// 45, Rule E).
pub(crate) fn check_peeks(binds: usize, peeks: &BTreeSet<i32>) -> Result<(), SpaceTxnError> {
    let all_binds = || i32::try_from(binds).is_ok_and(|binds| peeks.iter().copied().eq(0..binds));
    match peeks.is_empty() {
        true => Ok(()),
        false if all_binds() => Ok(()),
        false => Err(SpaceTxnError::MixedArrowJoin {
            binds,
            peeks: peeks.clone(),
        }),
    }
}
```

#### S5.2. Commit

Commit is the single mutation point of a transaction.
It applies the staged operations in staging order, each under its own sub-order, with the host calls of [S3](#s3-host-equivalence).
It returns one receipt for each operation, and it releases the locks before it returns.
In every mode, an operation makes its observer calls before its events and its store effects.
So the commit of replay makes the calls of play first, also for an [unpaired fire](glossary.md#unpaired-fire) ([decision 36](decision-register.md#decision-36)).

```text
COMMIT():
    require the transaction is open                              ▷ Finished
    take the locks and the registration, and mark the transaction finished
    staged ← the staged operations
    clear paired and δ        ▷ the host counters and bindings decide from now on
    result ← COMMIT-STAGED(staged)
    release the locks, and then the registration
    return result

COMMIT-STAGED(staged):
    require the captured store is the current store of the host  ▷ StoreSwapped
    parent ← the current operation order, or none
    if parent ≠ none: ADMIT-PARENT(parent, store)                ▷ ParentOrderReused, S5.7
    for i ← 0 to |staged| − 1:
        if parent ≠ none:
            receipt[i] ← in the scope of sub_order(parent, i): COMMIT-OPERATION(i, staged[i])
        else:
            receipt[i] ← COMMIT-OPERATION(i, staged[i])
        on an error: return the error        ▷ operations 0 to i − 1 stay applied
    return the receipts

COMMIT-OPERATION(i, op):
    events ← []
    receipt ← APPLY-OP(op, events)           ▷ the backend of the mode: Table S3.1 or S3.2
    ⟨Correlate the effects with the events⟩                      ▷ EventCoverage
    return receipt

APPLY-OP(op, events):
    ⟨Run the read-only commit checks⟩        ▷ Inconsistent, CounterOverflow
    run op.before_observe with the source of op                  ▷ Host error
    start the accounting operation                               ▷ Host error
    result ← ⟨Run the observed body⟩         ▷ Host error, ReplayDivergence
    finish the accounting operation with Stored, Matched, or Rejected, after result
    return result
```

A commit without an operation context is not tracked, and it emits no sub-orders.
A failure of operation $`i`$ leaves the operations $`0`$ to $`i - 1`$ applied, as the equivalent host program would ([prefix atomicity](glossary.md#prefix-atomicity)).
The deploy then fails, and the soft checkpoint of the deploy removes the applied operations.
An unpaired fire fails under the same rule, so a replay divergence leaves the prefix that play leaves.
A transaction that is dropped before commit has no effect: it makes no event, no observer call, and no write.

⟨Run the read-only commit checks⟩.
Before any effect of the operation, the commit checks the live store at every entry that the operation changes (FR-7').
For a produce, the join groups of its channel must equal `joins_at_start`.
A non-persistent produce needs room in its host counter, because the host steps the counter without a check.
For a fire, `live_comm` computes the COMM again from the live counters, and the result must equal the staged COMM.
Then the commit finds the fired continuation and the bound records in the live store, and each removed entry must be at its staged position.

```text
    if op is a produce:
        require get_joins(op.channel) = op.joins_at_start, as bytes    ▷ Inconsistent
        if not op.persist: require κ_host(op.src) < 2^31 − 1           ▷ CounterOverflow
    if op fires:
        require live_comm(the live counters) = the staged COMM         ▷ Inconsistent
        find the continuation and the records with CLAIM-POSITIONS     ▷ Inconsistent
        require each removed entry at its staged position             ▷ Inconsistent
```

The read-only checks of FR-7' run for an unpaired fire too, before any call, and its staged effects equal those of play.
The checks read no binding, so the pairing of a fire never changes them.
The event correlation never runs for an unpaired fire, because `APPLY-OP` fails first.

⟨Run the observed body⟩.
The observed body makes the remaining calls of the host operation, in the order of Table S3.1 or Table S3.2.
Each observer call precedes the events and the store effects of the operation, so a rejected call leaves no event of the operation.
A fire then observes its COMM, and in replay an unpaired fire stops right after that call ([decision 36](decision-register.md#decision-36)).

So replay makes every observer call that play makes, in the same order, and its only extra call is the stateless directive query.
The same call then rejects in both modes, after the same prefix.
A replay produce steps its host counter before it matches (Table S3.2, step 5).
It rolls the step back when its fire fails (step 13).

```text
    if mode ≠ Play:
        require the observer returns no replay directive          ▷ NativeReplayDirectiveUnsupported
    observe the produce or the consume of op                      ▷ Host error
    if mode ≠ Play and op is a produce: step the host counter of op.src
    if op stores or parks:
        log the IO event, then store the datum or park the continuation
        return Stored or Parked
    observe the COMM of the fire                                  ▷ Host error
    if mode ≠ Play and op.fire.recorded = none:
        fail ReplayDivergence(op.fire.comm)   ▷ no event, no store change, no binding change
    log the IO event, then the COMM event
    remove the fired continuation and the bound non-persistent records
    if mode ≠ Play: remove the bindings of the COMM
    return Fired
```

⟨Correlate the effects with the events⟩.
Each staged effect must have its event among the events of the operation.
A join change must have the continuation change of its group.
Last, each COMM must follow its trigger event in the list.
The cost is $`O(e \cdot v)`$ for $`e`$ staged effects and $`v`$ events of one operation, which are small numbers.

```text
    for each effect in op.effects:
        PutDatum(s):               require a Produce event of s     ▷ EventCoverage
        RemoveDatum(s):            require a COMM that lists s      ▷ EventCoverage
        PutContinuation(g, s):     require a Consume event of s     ▷ EventCoverage
        RemoveContinuation(g, s):  require a COMM of the consume s  ▷ EventCoverage
        PutJoin(k, g):             require a PutContinuation on g   ▷ EventCoverage
        RemoveJoin(k, g):          require a RemoveContinuation on g    ▷ EventCoverage
    for each COMM x in events:
        require an earlier event is the consume of x or a produce of x  ▷ EventCoverage
```

Figure S5.2 shows the commit as an activity diagram, with the steps of the backends and the place of each error.

![Commit of a space transaction as an activity. The commit refuses a finished transaction with Finished, takes the locks and the registration, and clears the paired COMM events and the counter delta. It refuses a swapped hot store with StoreSwapped and a reused parent order with ParentOrderReused, both with no effect. Then a loop runs each staged operation i in staging order under sub_order(parent, i): the read-only checks (joins unchanged, counter room, live COMM equal to the staged COMM, removed entries at their staged positions), the before_observe callback, the observer start, in replay the directive check, the observe call, in replay the counter step, and then the outcome: Store, Park, or Fire. A fire makes the observe_comm call. In replay, an unpaired fire then fails with ReplayDivergence, after the observer calls of play and with no event, no store change, and no binding change (decision 36). Any other fire logs the produce or consume event and the COMM event, removes the continuation and the bound non-persistent records, also for a peek fire (decision 45), and in replay removes the bindings. The observer finish follows, and then the event correlation. In native replay (decision 47), the observer start is replaced: operation i takes the ticket of its journal row with begin_row, and a row that is not ready, a footprint that differs from the locked footprint, an outcome that does not fit the staged kind, or a staged COMM that is not the recorded candidate fails closed with a mismatch and closes the epoch, so no operation waits for the row. A recorded denial publishes its row, fails with OutOfPhlogistons as play fails, and keeps the epoch open. An error in operation i leaves the operations before it applied. At the end the commit releases the locks and then the registration and returns the receipts.](diagrams/06-space-txn-commit.svg)

*Figure S5.2. Source: [diagrams/06-space-txn-commit.puml](diagrams/06-space-txn-commit.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

Figure S5.2a shows the lifecycle of a transaction as a state diagram.
It is the figure 06a of the diagram plan.

![Lifecycle of a space transaction as a state machine. A call of SpaceStoreHandle::begin first registers the current task, and a second open transaction or sweep of the same task fails with NestedTransaction and takes no lock. Empty lock keys fail with InvalidOperation. The transaction then acquires the phase-A stripes and then the phase-B stripes, captures the hot store and the installed set, and becomes Open. In the Open state the agent reads the staged view and stages operations. A rejected staging leaves the transaction open with no effect. A failed view change after validation closes it with Inconsistent and releases its locks. A drop releases the locks and the registration with no effect. A commit moves to Committing, which ends in Committed with the receipts, or in Failed with no effect for StoreSwapped and ParentOrderReused, or with the operations before the failing one applied. Every later call on a closed transaction fails with Finished.](diagrams/06a-space-txn-lifecycle.svg)

*Figure S5.2a. Source: [diagrams/06a-space-txn-lifecycle.puml](diagrams/06a-space-txn-lifecycle.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

#### S5.3. Verify (FR-3)

In replay and in reporting, staging verifies each fire against the recorded bindings before it applies the fire to the view.
Play has no recorded log, so it skips this step.
Verification never fails: it returns the recorded COMM that the fire pairs, or none for an [unpaired fire](glossary.md#unpaired-fire) ([decision 36](decision-register.md#decision-36)).
The lookup uses only the trigger key of the fire.
The trigger key is `Produce(src)` for a produce-side fire and `Consume(src)` for a consume-side fire.
Host replay reads the same key for a produce (`replay_rspace.rs:1202-1213`), and for a consume it also reads an index of all COMM events ([S10](#s10-limits)).

Before replay, `rig` binds one recorded occurrence of a COMM under each IO event of the COMM that the log holds (`replay_rspace.rs:480-540`).
A produce that occurs twice in the produce list of the COMM gives two bindings under its key.
So a key holds $`\lfloor b / m \rfloor`$ recorded occurrences of the COMM, where $`m`$ is the [binding multiplicity](glossary.md#binding) of the key.
An occurrence is free while the transaction has not paired it:

```math
\mathit{free}(\mathit{comm}, \mathit{key}) \;=\; \Bigl\lfloor \frac{b}{m} \Bigr\rfloor - \bigl\lvert \{\, c \in \mathit{paired} : c = \mathit{comm} \,\} \bigr\rvert
```

```text
VERIFY(comm, trigger):
    if mode = Play: return none
    (recorded, b) ← the binding of comm under trigger in the replay table, or return none when absent
    m ← BINDING-MULTIPLICITY(trigger, comm)
    available ← b div m, or 0 when m = 0
    if the copies of comm in paired < available: return recorded
    return none                              ▷ an unpaired fire
```

The multiplicity counts the occurrences of the key in the COMM:

```rust
/// Bindings that one recorded occurrence of `comm` adds under `key` in
/// `rig` (replay_rspace.rs), and that `remove_bindings_for` removes: one for
/// the consume, and one per occurrence of a produce in the produce list.
pub(crate) fn binding_multiplicity(key: &IOEvent, comm: &COMM) -> usize {
    match key {
        IOEvent::Consume(consume) => usize::from(*consume == comm.consume),
        IOEvent::Produce(produce) => comm
            .produces
            .iter()
            .filter(|listed| *listed == produce)
            .count(),
    }
}
```

The staging core applies the rule as follows:

```rust
    // Replay pairing under the trigger key only, as host replay looks it up:
    // `Produce(source)` for a produce-side fire and `Consume(source)` for a
    // consume-side fire. The key holds one binding per occurrence of the key
    // in the COMM (`binding_multiplicity`), so a recorded occurrence is
    // available while the paired copies use fewer bindings than it holds.
    // Returns the recorded copy, or `None` in play and for an unpaired fire.
    // An unpaired fire stages as in play, never pairs, and fails at commit
    // after the observer calls of play (decision 36), so replay staging
    // equals play staging for every log.
    fn verify<B: TxnBackend<C, P, A, K>>(
        &self,
        backend: &B,
        comm: &COMM,
        trigger: &IOEvent,
    ) -> Option<COMM> {
        match self.mode {
            SpaceMode::Play => None,
            SpaceMode::Replay | SpaceMode::Reporting => {
                let (recorded, count) = backend.recorded_binding(trigger, comm)?;
                let available = match binding_multiplicity(trigger, comm) {
                    0 => 0,
                    multiplicity => count / multiplicity,
                };
                let paired = self.paired.iter().filter(|paired| *paired == comm).count();
                (paired < available).then_some(recorded)
            }
        }
    }
```

The literal rule of the design pass compared $`b`$ with the paired copies, without the division.
For a COMM that lists one produce twice, that rule pairs one recorded occurrence two times.
The Rocq lemma `design_verify_overpairs` (`EventCorrelation.v:5164`) proves this over-pairing, and the test `space_store_handle_replay_cross_channel_comm_pairs_once` pins the corrected rule.

The commit removes all bindings of a paired COMM with `remove_bindings_for` (Table S3.2, step 11).
A binding that no operation pairs fails the deploy at `check_replay_data` with `UnusedCommEvent`, as in host replay (`replay_rspace.rs:542-558`).
So a park where the log recorded a fire is not an error at once, because the binding may belong to a later produce.

**Unpaired fires.** A fire that does not pair stages as play stages it ([decision 36](decision-register.md#decision-36)).
It changes the staged view, it adds no copy to `paired`, and its staged fire keeps `recorded = none`.
The commit reads this field right after the COMM call of the fire ([S5.2](#s52-commit)).
A call that the observer rejects fails the operation with the error of play, for example `OutOfPhlogistons`, and otherwise the commit fails with `ReplayDivergence`.
The fire then logs no event, removes no entry, keeps its bindings, and rolls its counter step back.

For an honest log, replay therefore ends as play ends.
Play logs the events of an operation only after the observer accepts its calls.
So a deploy that runs out of phlo at operation $`k`$ leaves no COMM of operation $`k`$ in the log.
When operation $`k`$ is a fire, replay stages it as an unpaired fire.
Replay makes the calls of play, and the same call rejects with the cost of play (headline H23, invariant T6).

For a log that omits a COMM, the fire fails closed after the charges of its introduction and its COMM.
These charges never exceed the charges of an honest run.
An agent cannot continue a transaction after an unpaired fire, because the commit always fails at it.
The two lookups below let an agent follow the log before it stages, and no P4 agent calls them.

Two lookups help an agent to choose the same outcome as play:

- `recorded_comms(source)` returns the free occurrences of the COMM events under the key of `source`, one entry for each occurrence, sorted by bincode bytes. It returns `None` in play.
- `admits(comm, datum, incoming)` tells whether replay accepts `datum` for `comm`. It is the host rule `matches` and `was_repeated_enough_times` (`replay_rspace.rs:1346-1361`), evaluated over the staged counters. It returns `true` in play.

The admission rule follows, where the flag $`\mathit{inc}`$ holds for the incoming datum of the produce that the agent matches:

```math
\mathrm{admits}(\mathit{comm}, d, \mathit{inc}) \iff d.\mathit{source} \in \mathit{comm}.\mathit{produces} \;\wedge\; \bigl( d.\mathit{persist} \;\vee\; \mathit{times}(d.\mathit{source}) = \kappa_v(d.\mathit{source}) + [\mathit{inc}] \bigr)
```

#### S5.4. Sweep

The sweep removes the [volatile channels](glossary.md#volatile-channel) of a deploy at its end ([decision I-4](decision-register.md#i-4)).
It removes their data, their continuations, and their join entries.
The host offers no such operation.
`ISpace::remove_all_continuations` removes continuations only (`rspace/ispace_impl.rs:260-268`), and `HotStore::remove_continuation` leaves the join entries (`hot_store.rs:818-861`).
A volatile join entry would then reach the history trie.

```text
SWEEP-VOLATILE(channels):                    ▷ SpaceStoreHandle::sweep_volatile
    register the current task                                    ▷ NestedTransaction
    S ← the distinct channels, keyed by store identity, the first value of a key kept
    acquire the stripes of S in phase A, and then in phase B, in ascending order
    T ← the history reader at the current root
    ⟨Validate the sweep⟩
    ⟨Remove the volatile state⟩
    ⟨Check for residue⟩
    return the report
```

⟨Validate the sweep⟩.
Validation is read-only, so a refused sweep has no effect.
The sweep groups are the single-channel keys of the swept channels, and the stored join groups of each swept channel.
Each sweep group must be canonical, and it must not name a channel that the sweep keeps.
No swept key may have a leaf at the history root.

```text
    require no key of S is installed                             ▷ InstalledChannel
    G ← {[c] : c ∈ S} ∪ {g : g ∈ joins(c), c ∈ S}
    require each g ∈ G is canonical                              ▷ NonCanonicalChannelOrder
    require each channel of each g ∈ G is in S                   ▷ SweepCrossesDurable
    require T has no data leaf and no join leaf for any c ∈ S    ▷ SweepTouchesPersisted
    require T has no continuation leaf for any g ∈ G             ▷ SweepTouchesPersisted
```

⟨Remove the volatile state⟩.
The sweep removes the continuations first, so that every join group is then empty and each join removal succeeds.
It removes each list from the last index down to the first.
It emits no event and makes no observer call.

```text
    for each g ∈ G: for i from |conts(g)| − 1 down to 0:
        remove_continuation(g, i)
        report.continuations ← report.continuations + 1
        in replay: lower the estimate of waiting continuations by one while it is positive
    for each c ∈ S:
        remove each group of joins(c)
        report.joins ← report.joins + the number of groups that it removed
    for each c ∈ S: for i from |data(c)| − 1 down to 0:
        remove_datum(c, i)
        report.data ← report.data + 1
    report.channels ← |S|
```

⟨Check for residue⟩.
After the removals, every swept list must be empty.

```text
    require data(c) = [] and joins(c) = [] for each c ∈ S, and conts(g) = [] for each g ∈ G
                                                                 ▷ Inconsistent
```

**Why the root does not change.** The swept keys end as empty lists in the hot store.
At the next checkpoint, `changes` emits a delete action for each of them (`hot_store.rs:1115-1170`).
None of these keys has a leaf in the trie, because validation refused any key with a leaf.
A radix delete of an absent key returns "not found" (`history/radix_tree.rs:1584-1608`), and `clearing_delete_actions` drops such actions (`history/radix_tree.rs:1646-1663`).
So the new root is the root of the durable changes alone (headline H15).

The replay sweep lowers the estimate of waiting continuations once for each continuation that it removes, as host continuation removal does (FR-8).
A sweep matches nothing, so it counts no match.

#### S5.5. Canonical order (FR-1')

The hot store keys a continuation list by its channel list in the order of the caller (`hot_store.rs:671-718`, `720-783`).
The history trie sorts the channel hashes instead (`hashing/stable_hash_provider.rs:24-37`).
So two orders of one channel set give two hot-store keys for one history key.
The checkpoint then refuses two actions on one key with "Cannot process duplicate actions on one key" (`history/instances/radix_history.rs:100-105`).
The substrate therefore admits exactly one order of each channel multiset: the order that sorts the store identities ([decision 28](decision-register.md#decision-28)).

```text
GROUP-KEY(group):
    require group ≠ []                                           ▷ InvalidOperation
    keys ← [STORE-IDENTITY(c) for each c in group]
    for each adjacent pair (a, b) in keys:
        require a ≤ b, compared as byte strings                  ▷ NonCanonicalChannelOrder
    return keys

ADMIT-GROUP(channels, patterns, use):
    require channels ≠ [] and |channels| = patterns              ▷ InvalidOperation
    keys ← GROUP-KEY(channels)                                   ▷ NonCanonicalChannelOrder
    for each channel c in channels: REQUIRE-COVERAGE(c, use)     ▷ UnlockedChannel
    for each key k in keys: require k ∉ installed                ▷ InstalledChannel
    return keys
```

The Rust check is one pass over adjacent pairs:

```rust
/// A group is canonical when its keys are sorted (non-decreasing), so a group
/// may repeat a channel. The sorted list is the only canonical member of the
/// permutations of a key multiset. Any other order fails with
/// `NonCanonicalChannelOrder`. One channel is always canonical.
pub(crate) fn require_canonical(keys: &[Blake2b256Hash]) -> Result<(), SpaceTxnError> {
    match keys.windows(2).all(|pair| pair[0] <= pair[1]) {
        true => Ok(()),
        false => Err(SpaceTxnError::NonCanonicalChannelOrder),
    }
}
```

The comparison of byte strings is a total order, and it is antisymmetric.
So each multiset of keys has exactly one non-decreasing arrangement: its sorted list.
The Rocq lemma `sorted_unique_canonical` (`EventCorrelation.v:7073`) proves this, also when the list repeats a key.
Headline H22 uses it to show that canonical cache keys have distinct history keys.

The check runs before the hot-store access of its group.
It covers the continuations read, each stored join group that a read or a staging loads, each staged group, and each sweep group.
So the hot store never caches an entry under a non-canonical key, and an aborted transaction stays inert (headlines H9 and H18).

A group of one channel is always canonical.
A sorted group can repeat a channel, and then $`k`$ occurrences of one channel bind $`k`$ distinct records.
In a produce-side fire, the incoming datum binds exactly once.

#### S5.6. Lock coverage (FR-4)

A space transaction must not interleave with a host operation on a channel that it touches.
Otherwise the commit would apply its effects to a state that differs from the staged state.
The host keeps operations apart with [lock stripes](glossary.md#lock-stripe).
So the substrate requires that a transaction holds every stripe that the equivalent host operation would take, from `begin` to `commit`.

```text
BEGIN(lock_keys):                            ▷ SpaceTxnHost::begin, after the registration
    require lock_keys ≠ []                                       ▷ InvalidOperation
    H ← [channel_hash(k) for each k in lock_keys]
    held_A ← {h mod |stripes_A| : h ∈ H}
    held_B ← {h mod |stripes_B| : h ∈ H}
    acquire the stripes of H in phase A, ascending, and then in phase B, ascending
    capture the hot store and the set of installed channels

REQUIRE-COVERAGE(c, use):
    h ← channel_hash(c)
    if use = Produced: require (h mod |stripes_A|) ∈ held_A     ▷ UnlockedChannel
    if use = Joined:   require (h mod |stripes_B|) ∈ held_B     ▷ UnlockedChannel
    if use = Consumed: require both                             ▷ UnlockedChannel
```

Table S5.6 gives the use of each touched channel and the host lock that it mirrors.

| Touched channel | Use | Host lock at HEAD |
|---|---|---|
| The channel of a produce | `Produced`: phase A | `produce_lock`, phase A of the channel (`rspace/locks.rs:33-38`) |
| Each channel of each join group of a produce channel, and each channel of a fired group | `Joined`: phase B | `produce_lock`, phase B of the joined channels (`rspace/locks.rs:40-54`) |
| Each channel of a consume | `Consumed`: both phases | `consume_lock` (`rspace/ispace_impl.rs:339-343`, `striped_locks.rs:74-82`) |

The Rust check maps each use to the phases that it needs:

```rust
    fn require<C: Hash>(&self, channel: &C, lock_use: LockUse) -> Result<(), SpaceTxnError> {
        let hash = striped_locks::channel_hash(channel);
        let held = match lock_use {
            LockUse::Produced => self.phase_a.holds(hash),
            LockUse::Joined => self.phase_b.holds(hash),
            LockUse::Consumed => self.phase_a.holds(hash) && self.phase_b.holds(hash),
        };
        match held {
            true => Ok(()),
            false => Err(SpaceTxnError::UnlockedChannel),
        }
    }
```

Coverage depends on the stripes, not on the lock keys themselves.
A channel is covered when a lock key shares its stripe, as the unit test `space_store_handle_lock_coverage_matches_acquired_stripes` shows.
The sweep locks its own channels in both phases, so it needs no coverage check.

#### S5.7. Parent tracking (FR-5')

Native phlo recording accepts one RSpace operation for each operation order (`rholang/src/rust/interpreter/accounting/native_runtime/operations.rs:264-265`).
So the commit runs operation $`i`$ under `sub_order(parent, i)`, which appends the segment $`(i, 2)`$ to the path of the parent:

```rust
/// Path tag of a sub-order. `ReductionContext` uses 0 (operation) and 1
/// (split).
pub const SUB_OPERATION_TAG: u64 = 2;

/// Order of operation `index` of a transaction committed under `parent`:
/// `parent.path ++ [(index, SUB_OPERATION_TAG)]`.
pub fn sub_order(parent: &OperationOrder, index: usize) -> OperationOrder {
    let mut path = parent.path.clone();
    path.push_back((index as u64, SUB_OPERATION_TAG));
    OperationOrder {
        session: parent.session,
        path,
    }
}
```

Two transactions that commit under one parent would emit the same sub-orders.
So the handle admits at most one commit for each parent order in one [evaluation scope](glossary.md#evaluation-scope).

```text
state of a handle: tracker = (session, store, paths)

BEGIN-EVALUATION-SCOPE():
    tracker ← (none, none, ∅)

ADMIT-PARENT(parent, store):
    if tracker.store is not this store, or tracker.session ≠ parent.session:
        tracker ← (parent.session, store, ∅)
    require parent.path ∉ tracker.paths                          ▷ ParentOrderReused
    tracker.paths ← tracker.paths ∪ {parent.path}
```

The Rust method implements this rule:

```rust
    fn admit(
        &mut self,
        parent: &OperationOrder,
        store: StoreIdentity,
    ) -> Result<(), SpaceTxnError> {
        let same_store = self
            .store
            .as_ref()
            .is_some_and(|recorded| Weak::ptr_eq(recorded, &store));
        if !same_store || self.session != Some(parent.session) {
            self.session = Some(parent.session);
            self.store = Some(store);
            self.paths.clear();
        }
        match self.paths.insert(parent.path.clone()) {
            true => Ok(()),
            false => Err(SpaceTxnError::ParentOrderReused),
        }
    }
```

The tracker resets in three cases, and each one has a reason:

- At the start of each root evaluation, through `SpaceStoreHandle::begin_evaluation_scope`. The session id is the deploy id, so a deploy that runs again would otherwise fail with a false `ParentOrderReused`. A deploy runs again after a soft-checkpoint revert, or when a later block proposes it again.
- When the commit runs against another hot store than the recorded one. A reset or a revert replaces the hot store. The tracker records the store by a weak reference, which keeps the allocation, so a new store cannot reuse the address of a recorded one.
- When the session changes. This reset is defense in depth.

The tracker lives in the handle, which its clones share, so no host struct changes.
A commit without an operation context is not tracked, because it emits no sub-orders.

#### S5.8. Peek re-offer (FR-10)

A peek reads a value and leaves it available to other receives.
The host gets this effect in two steps: RSpace removes the peeked data, and the reducer produces each non-persistent result again (`produce_peeks`, `rholang/src/rust/interpreter/reduce.rs:986-1021`).
[Decision 30](decision-register.md#decision-30) gives spaces the same behavior for the incoming datum.
[Decision 45](decision-register.md#decision-45) (Rule E) gives them the same behavior for the stored records that a peek binds.
It replaces the [retention](glossary.md#retention) of [decision 19](decision-register.md#decision-19), under which the substrate kept these records in place.

The substrate keeps four rules:

1. A produce-side fire never stores its incoming datum, in the staged view and at commit, in every mode.
2. Every fire removes the stored non-persistent records that it binds, as the host does. This holds for a peek fire too, on the consume side, and on the produce side for joins and repeated-channel groups.
3. The receipt keeps the persist flag of the host's `wrap_result` for every result. So the non-persistent results of a peek fire are the records that it removed and the incoming datum of a non-persistent send.
4. Staging accepts a second produce with the same identity in one transaction, and each non-persistent produce steps the counter by one.

The receipt rule of item 3 checks only that the results line up with the candidates:

```rust
/// Receipt of a fire from the host's `wrap_result`, whose result `i` belongs
/// to candidate `i` of the `candidates` candidates of the fire. Each result
/// keeps the flag of the host, so a non-persistent result tells the agent that
/// the fire removed or spent its datum (decision 45, Rule E).
pub(crate) fn fired_receipt<C, P, A, K>(
    comm: COMM,
    wrapped: MaybeConsumeResult<C, P, A, K>,
    candidates: usize,
    produce: Option<Produce>,
) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
```

**The re-offer.** When the send and the fired waiter are both non-persistent, the agent re-offers the datum as the next operation of the same transaction.
The re-offer is a `SpaceProduce` with the identity of `Produce::create`, the same observer sequence, a counter step of one, and the same authority.
These are the properties of the re-produce of the host reducer.
The P4 plan of a head-only queue does this in its produce loop:

```text
OFFER(d):                                    ▷ the produce plan of a head-only queue, P4
    if the queue holds a record: stage Store(d), and stop
    loop:
        W ← the head waiter, or stage Store(d) and stop
        if W does not match d: stage Store(d), and stop
        stage Fire(d, W)
        if d is not persistent and W does not peek: stop        ▷ the fire consumed d
        if W is persistent: offer d in a new transaction, and stop     ▷ decision 34, A10
        ▷ W left and d survives: offer d to the next waiter (the re-offer)
```

**The queue scenario.** A peek waiter $`W_1`$ parks, then a consuming waiter $`W_2`$ parks, and then a program sends $`d`$ once.
Table S5.8 shows the transaction of the send.
The hot store lists $`W_2`$ before $`W_1`$, because `put_continuation` inserts at the head, but the queue takes its head by `ord`.

| Step | Staged operation | COMM | $`\mathit{times}(d)`$ | Waiters after | Records after |
|---|---|---|---|---|---|
| Before | none | none | none | $`W_1`$, $`W_2`$ | none |
| 0 | `produce(d)`, Fire with $`W_1`$ | the consume of $`W_1`$, peeks $`\{0\}`$ | 1 | $`W_2`$ | none |
| 1 | `produce(d)` again, Fire with $`W_2`$: the re-offer | the consume of $`W_2`$, no peeks | 2 | none | none |

The commit runs operation 0 under `sub_order(parent, 0)` and operation 1 under `sub_order(parent, 1)`.
It logs the events Produce $`d`$, COMM of $`W_1`$, Produce $`d`$, and COMM of $`W_2`$.
These are the events of the host produce and of its re-produce, and the test `space_store_handle_peek_reoffer_equals_head_reproduce` compares them with HEAD.
Without $`W_2`$, operation 1 has the outcome Store, and the space holds one copy of $`d`$, as the host does after its re-produce.

Under the P1 rule before decision 30, operation 0 stored $`d`$, and $`W_2`$ then matched the stored $`d`$.
That pending match breaks the no-pending-match invariant of the queue.
The lemma `offer_p1_breaks_no_pending_match` (`EventCorrelation.v:4043`) shows the break.
The lemma `offer_keeps_no_pending_match` (`EventCorrelation.v:4005`) shows that the plan with the re-offer keeps the invariant.

The re-offer registers its introduction authority under the identity of the peek fire.
The registry accepts an equal authority for an identity that it holds (`rholang/src/rust/interpreter/accounting/mod.rs:1041-1046`).
It refuses another authority with an identity conflict, and the commit then fails at the re-offer, with the peek fire applied.

**The re-store.** A peek fire removes each stored non-persistent record that it binds, and the agent stores each one again in the same transaction ([re-store](glossary.md#re-store)).
The re-store is a `SpaceProduce` of the same record bytes, with the outcome Store and `persist = false`.
So it has the produce identity of the first send, and the record keeps its envelope and its order.
Its `before_observe` callback registers the cost authority of the record, because `produce_peeks` sends each result with its own authority.
The re-store steps the produce counter by one, as the second send of the host does, and it bumps no collection counter.

The P4 plan of a head-only queue stages the re-store right after the fire of a peek receive (`Cx::restore`, `restore_peeked`):

```text
RECEIVE-ROUND(op, h):                          ▷ one round of the consume plan of a queue, P4
    stage Fire(op, h)                          ▷ the COMM removes h unless h is persistent
    if op peeks and h is not persistent:
        stage Store(h), with the authority of h        ▷ the re-store (decision 45)
```

The removal and the re-store must land together.
The hot store does not deduplicate data, because `put_datum` inserts every datum.
So a re-store over a substrate that kept the record would hold two copies, and a removal without a re-store would lose the record.

The stores of the host and of the substrate are then equal after each peek.
The host removes a peeked record and its reducer sends it again, and the substrate removes it and the agent stores it again.
Both second sends have the same identity, the same counter step, and the same authority (headline H11).

### S6. Errors

Every failure of the substrate is a typed `SpaceTxnError`, and the substrate does not panic on any input.
Its only `expect` calls take mutexes, which fail only when another thread panicked while it held one.
`From<SpaceTxnError>` for `RSpaceError` maps `Host(e)` back to `e`, and every other variant to `InterpreterError` with its display text.
So the reducer keeps the classification of `OutOfPhlogistons` and `HostWorkRejected`.

The column "Effect" of Table S6 uses three values.
"None" means no store change, no event, no observer call, and no binding change.
"Prefix" means that the operations $`0`$ to $`i - 1`$ of the commit stay applied, and operation $`i`$ fails.
"Prefix with $`i`$" means that operation $`i`$ also stays applied.

| Variant | Raised in | Trigger | Effect |
|---|---|---|---|
| `Host(e)` | commit | The `before_observe` callback or an observer call rejects operation $`i`$, for example with `OutOfPhlogistons`, `HostWorkRejected`, or an identity conflict of the introduction authority. In replay, this also holds for an unpaired fire, which then fails with the error of play ([decision 36](decision-register.md#decision-36)). | Prefix. Operation $`i`$ makes its observer calls up to the rejection, but it logs no event and changes no store entry. A failed replay fire rolls its counter back. |
| `Host(e)` | count, sweep validation | The history reader fails. | None. |
| `InvalidOperation` | begin, stage | The lock keys are empty. A group is empty, or its length differs from the patterns. The candidates do not line up with the channels. The produce channel is not in the fired group. A produce fire has not exactly one incoming candidate, or it does not carry the produced record. A consume fire has an incoming candidate. | None. |
| `InvalidOperation` | any step | A channel or a record cannot be serialized with bincode. | None, or prefix when a commit check of operation $`i`$ raises it. |
| `MixedArrowJoin` | stage | The peeks are neither empty nor all binds $`\{0, \dots, n - 1\}`$. | None. |
| `NotInView` | stage | The waiting continuation of a produce fire is not in the view. A stored candidate does not name its record byte for byte. A fire binds a non-persistent record twice. | None. |
| `InstalledChannel` | read, stage, sweep | A touched channel belongs to an installed system process. | None. |
| `StoreSwapped` | commit | A reset or a revert replaced the hot store between `begin` and `commit`. | None. |
| `ReplayDivergence` | commit | In replay or reporting, operation $`i`$ is an [unpaired fire](glossary.md#unpaired-fire): at staging, its COMM had no free recorded occurrence under its trigger key. The observer accepted every call of the fire, up to and including its COMM call ([decision 36](decision-register.md#decision-36)). | Prefix. Operation $`i`$ makes the observer calls of play, but it logs no event and changes no store entry or binding. A produce rolls its counter step back. |
| `NativeReplayDirectiveUnsupported` | commit | In replay or reporting, the observer returns a directive for operation $`i`$. | Prefix. Operation $`i`$ makes only its start and finish calls. |
| `Inconsistent` | stage | A view change fails after validation. This is a defect, and the transaction closes. | None. |
| `Inconsistent` | commit | A read-only check of operation $`i`$ fails: the joins changed, the COMM differs, or a removed entry is not at its staged position. | Prefix. |
| `Inconsistent` | sweep | A removal fails, or the sweep leaves residue. This is a defect. | The removals before the failure stay. |
| `EventCoverage` | commit | A staged effect of operation $`i`$ has no event, or a COMM comes before its trigger event. This is a defect. | Prefix with $`i`$. |
| `SweepCrossesDurable` | sweep | A join of a swept channel also names a channel that the sweep keeps. | None. |
| `SweepTouchesPersisted` | sweep | A swept channel or group has a leaf at the history root. | None. |
| `Finished` | read, stage, commit | The transaction committed, or it closed after a failed view change. | None. |
| `NonCanonicalChannelOrder` | read, stage, sweep | A group, given or stored, is not non-decreasing by store identity. | None, and no cache entry under a non-canonical key. |
| `UnlockedChannel` | stage | The transaction does not hold the stripe of a touched channel in the phase that the host uses. | None. |
| `ParentOrderReused` | commit | A transaction already committed under the current parent order in this evaluation scope. | None. |
| `CounterOverflow` | stage | A counter would pass $`2^{31} - 1`$. | None. |
| `CounterOverflow` | commit | The host counter of a non-persistent produce has no room, or a live COMM counter overflows. | Prefix. |
| `NestedTransaction` | begin, sweep | The current task already holds an open transaction or a running sweep of the handle. | None, and no lock. |

A staging error other than a failed view change leaves the transaction open, so the agent can continue, commit, or drop it.
No staging error comes from pairing: an unpaired fire stages with no error, and its commit fails at it ([S5.3](#s53-verify-fr-3)).

### S7. Locking and ordering

A transaction takes its stripes in `begin`, through the host `consume_lock` (`rspace/locks.rs:24-29`, `replay_rspace.rs:114-116`).
It holds them until it commits, closes after a failed view change, or is dropped.
An agent must keep three rules while a transaction is open:

- Call no host `ISpace` operation. A tokio mutex is not re-entrant, so the call would wait for a stripe that the transaction holds.
- Open no second transaction of the same handle in the same task. The handle refuses it with `NestedTransaction` (FR-9), and it refuses a sweep in the same way (FR-9').
- Await no other work between `begin` and `commit` ([decision 34](decision-register.md#decision-34)).

The handle identifies a task by its tokio task id, and code without a task by its thread id.

**Proposition 1 (no deadlock).**
Let each holder of stripes take them as `acquire_locks` does, and let each task keep the three rules above.
Then no set of stripe holders can deadlock.

*Proof sketch.*
Order the stripes totally: every phase-A stripe comes before every phase-B stripe, and the stripes of one phase follow their index.
Write $`s \prec s'`$ when the stripe $`s`$ comes before the stripe $`s'`$ in this order.
Each holder takes its stripes in strictly ascending order of $`\prec`$.

`acquire_locks` sorts and deduplicates the indices of one phase (`striped_locks.rs:41-72`).
`consume_lock` takes phase A before phase B (`striped_locks.rs:74-82`), and `produce_lock` takes one phase-A stripe before its sorted phase-B stripes (`rspace/locks.rs:33-57`).
A transaction, a sweep, and a host operation each take their stripes this way.
The three rules forbid any other acquisition of a stripe while a transaction is open.

Assume a cycle of holders $`T_1, \dots, T_k`$, where $`T_j`$ waits for the stripe $`s_j`$ that $`T_{j+1}`$ holds, with $`T_{k+1} = T_1`$.
The holder $`T_{j+1}`$ holds $`s_j`$ while it waits for $`s_{j+1}`$, and it takes stripes in ascending order, so $`s_j \prec s_{j+1}`$.
Around the cycle, this gives $`s_1 \prec s_1`$, which contradicts the strict order.
So no circular wait exists, and without a circular wait no deadlock exists [[CES71](#ref-ces71)].

**Proposition 2 (sub-order contiguity).**
Let a transaction with $`n \geq 1`$ operations commit under the parent order $`P`$ with the path $`p`$, and let $`Q_i`$ be `sub_order(P, i)`.
Then $`P < Q_0 < Q_1 < \dots < Q_{n-1}`$.
No other order that the node emits in the same session lies between $`P`$ and $`Q_{n-1}`$, or equals some $`Q_i`$.

*Proof sketch.*
A causal path compares as its list of segments in lexicographic order, and an operation order compares its session first (`operation_context.rs:177-204`).
The path $`p`$ is a proper prefix of each $`Q_i`$, so $`P < Q_i`$, and $`Q_i < Q_j`$ exactly when $`(i, 2) < (j, 2)`$, which is when $`i < j`$.

Now let $`R`$ be another emitted order of the session.
If $`R`$ lies strictly between $`P`$ and $`Q_{n-1}`$, then $`R`$ must extend $`p`$, because $`P`$ and $`Q_{n-1}`$ agree on every segment of $`p`$.

The scheduler emits only paths of the form $`u \mathbin{+\!\!+} [(k, 0)]`$ (`rholang/src/rust/interpreter/deterministic_reduction.rs:84-135`).
Each segment of the participant path $`u`$ is a split segment $`(k', c + 1)`$ for the child $`c`$ of a split at the step $`k'`$.
This is the compacted split DR-84 of `feature/cost-accounted-rho`, which the campaign merged at `4d67b5ccc`.
No segment of $`u`$ is an operation segment, so a scheduler path never continues after its final operation segment, and no scheduler path extends $`p`$.
A scheduler path ends with an operation segment $`(k, 0)`$, and a sub-order ends with $`(i, 2)`$, so no scheduler path is a sub-order.
A split segment can have the second component 2, so this last argument uses the final segment, not the presence of the tag.

The parent of another transaction is a scheduler order, because a commit starts no transaction inside a sub-order scope.
So its sub-orders extend its own parent path, which is not $`p`$ within one evaluation scope (FR-5'), and they cannot extend $`p`$ either.

The ordered event log groups events by operation order (`rspace/trace_log.rs:22-47`).
So the events of the transaction form one block, after the events of $`P`$ and before the events of the next step of the participant.
Headline H8 (`sub_order_between`) states this result in Rocq.

**Proposition 3 (serializability).**
With lock coverage, every interleaving of space transactions and host operations has the effect of a serial order of the committed units.

*Proof sketch.*
A transaction is a two-phase-locking unit [[EGLT76](#ref-eglt76)].
It takes all its stripes in `begin`, before its first read, and it releases them only when it ends.
Lock coverage gives each staged operation every stripe that the equivalent host operation takes, in the same phase.
So a host operation that conflicts on a touched channel runs either before `begin` or after the commit.

The read-only checks at commit confirm that each changed entry still has its staged value.
The TLA+ model `TxnLockCoverage.tla` checks this result as the invariant T5 over every interleaving of its bounded configurations [[Lam94](#ref-lam94)].

Figure S7 shows a full cycle of play, checkpoint, replay, and sweep.

![Checkpoint, replay and sweep as a sequence. Play on the proposer: the reducer starts the evaluation scope, the space agent opens a transaction through the handle, reads the staged view, stages operations and commits; the commit makes the observer calls and logs the events under sub_order(parent, i) and writes the hot store. At the end of the deploy the reducer sweeps the volatile channels, which removes their data, continuations and joins with no event. The checkpoint turns the hot store changes into history actions; the deletes of the swept keys are absent from the trie and change nothing, so the root covers the durable state only. Replay on the validator: rig binds each recorded COMM under its logged IO events, the scope starts, the agent stages the same operations as play, and verify pairs each fire under its trigger key with the binding multiplicity. A fire with no free recorded occurrence stages unpaired, with no error (decision 36). The commit then makes the observer calls of play in the same order under sub_order(parent, i). An operation that stores, parks, or fires paired writes the hot store, and a paired fire removes its bindings. At an unpaired fire the commit fails after the COMM call with ReplayDivergence, or with the error of a rejected call as in play, and the operations before it stay applied. The sweep runs at the same point as in play, check_replay_data finds no unused binding, and the checkpoint gives the same root.](diagrams/07-checkpoint-replay-sweep.svg)

*Figure S7. Source: [diagrams/07-checkpoint-replay-sweep.puml](diagrams/07-checkpoint-replay-sweep.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

Figure S7a shows a restart in three processes, as the suite `reified_restart_rehydrate.rs` runs it.
The state of a space crosses a restart only through the history trie, because no Reified state outlives an evaluation.
So the first operation after a restart reads the config and the records through the metered view.
It pays what the operation pays on a warm runtime (design v3 RH-1 to RH-4, rule NE-2).

![Restart and lazy attach as a sequence over three processes, an LMDB store, the space agent and the metered view. Process 1 plays block 1 natively: it creates the spaces, stores records and waiters, certifies each deploy by native replay, and checkpoints the config, the counters, the records, the markers, the waiters and the handle into the store at root R1. Process 2 opens the store at R1 with an empty hot store. The first operation on a space reads the config through the metered view, which loads it from the history and reserves the work of the load; the charge is the same on a hit and a miss and depends only on the key and the value. The agent reads the records and the waiters of the operation, each reserved first, stages and commits with the observer calls of play, and process 2 checkpoints root R2 and writes the roots, the cost vector, the evidence bytes and the event log of each deploy. Process 3 opens the store at the pre-state root of each deploy, decodes the evidence, checks the journal, binds the trace to the event log, replays natively, and reaches the post-state root and the phlo usage of play. The parent plays both blocks warm in one process over an in-memory store and finds equal roots, cost vectors, evidence bytes and event logs.](diagrams/07a-restart-lazy-attach.svg)

*Figure S7a. Source: [diagrams/07a-restart-lazy-attach.puml](diagrams/07a-restart-lazy-attach.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

### S8. What is unchanged

P1 keeps every item of the consensus-preservation checklist of the plan that concerns ordinary execution.
No production path calls the substrate in P1, and the footprint policy `p1` pins every other source file at the baseline.
Table S8 maps the checklist items to the reason and to the gate that shows it.

| Item | Checklist item | Why P1 keeps it | Gate |
|---|---|---|---|
| 1 | The COMM produce sort is unchanged. | The substrate builds every COMM with `COMM::new`, and `trace/` is frozen. | R1, footprint `p1`, legacy golden corpus |
| 2 | The host never retains peeked or matched-persistent data. | The host code is unchanged. Since [decision 45](decision-register.md#decision-45), spaces do not retain peeked records either. | R2, the six HEAD peek tests |
| 3 | Joins stay greedy, and `Match` is unchanged. | The matcher modules are frozen. | R3, footprint `p1` |
| 4 | No production EventCoverage exists. | `casper/src` is frozen. The substrate correlates events only inside its own commit. | Footprint `p1` |
| 5 | Leaf, serializer, and event-struct formats are unchanged, and host structs too. | `internal.rs`, `serializers/`, and `trace/` are frozen. | R4, R5, footprint `p1` |
| 6 | The derived bincode of `Datum`, `WaitingContinuation`, and `Produce` is unchanged. | `internal.rs` and `trace/` are frozen. | Legacy golden corpus |
| 7 | Produce and consume identities are unchanged. | The substrate uses only `Produce::create` and `Consume::create`. | Legacy golden corpus |
| 8 | Event proto bytes are unchanged. | The protobuf files are frozen. | R5 |
| 9 | The merger is unchanged. | `merger/` and `casper/src` are frozen. | Footprint `p1` |
| 10 | Host replay acceptance is unchanged. | `replay_rspace.rs` gains one `mod` line only. | R9, `space_store_handle_host_replay_accepts_txn_play_log` |
| 11 | Cost and host-work accounting are unchanged in P1. | The accounting module is frozen, and the substrate drives the existing observer. | R6 |
| 12 | The dedup identity of the hot store is unchanged. | `hot_store.rs` is frozen, and the view copies the identity word for word. | `view_put_continuation_deduplicates_by_debug_identity`, footprint `p1` |
| 13 | The checkpoint root is unchanged in P1. | No ordinary path reaches the substrate. | `runtime_spec`, R8, legacy golden corpus |
| 14 | Scheduler footprints of host intents are unchanged. | `rholang/src` is unchanged in P1. | Footprint `p1` |
| 15 | System byte names are unchanged. | `rholang/src` is unchanged in P1. | Footprint `p1` |
| 16 | No schema shift happens. | The protobuf files are frozen, and ordinary terms keep their bytes. | R7, `runtime_spec` |
| 17 | `ISpace` implementors compile unchanged. | `rspace_interface.rs` and `ispace_impl.rs` are frozen. | Workspace build, footprint `p1` |
| 21 | `contract_call.rs`, `dispatch.rs`, `ispace_impl.rs`, and the cost-accounting module are byte-identical, with two approved exceptions. Decision 47 approves the accessor `space_store` of the native replay session. Decision 78 approves the v6 host-work limits of one native replay test. | The policy `campaign`, which `p1` extends, freezes them. From decision 47, the policy `d47` approves the accessor as the only production edit of the accounting module. From decision 78, the policy `p8` also approves one test edit. | Footprints `p1`, `d47` and `p8` |
| 22 | The parser is a conservative extension of HEAD parsing. | Every repository source normalizes to the bytes of HEAD. | `ordinary_source_normalization_unchanged` |

Items 18 to 20 concern the space machinery of P4 to P7, which P1 does not add.
The sweep already supports item 20.
It refuses keys with a leaf in the history trie, and joins that also name a channel that it keeps.

The gates are these tests and checks:

- **R1 to R9.** `rholang/tests/reified_ordinary_regression.rs` compares nine aspects of ordinary execution with a corpus captured at HEAD (`test-vectors/ordinary-execution-v1.json`). The aspects are the COMM sort, peeks, greedy joins, leaf bytes, event bytes, cost vectors, term bytes, a state dump, and play-replay equality.
- **Legacy golden corpus.** `rspace++/tests/legacy_golden_corpus.rs` checks channel hashes, event identities, the empty radix root, and a non-empty root with its event against `test-vectors/legacy-golden-corpus-v1.json`.
- **Source normalization.** The test `ordinary_source_normalization_unchanged` in `rholang/tests/reified_source_normalization.rs` parses and normalizes every repository source, and it compares the bytes with the capture of HEAD (`test-vectors/ordinary-source-normalization-v1.json`).
- **Footprint policy `p1`.** The script `scripts/verify-reified/check-footprint.py` compares the pinned files with the SHA-256 digests of the base revision. It allows only the edits of Table S1.1. At the P1 milestone the base was `6f7a80da3` (`test-vectors/head-file-digests-6f7a80da3.json`). Since the merge of `feature/cost-accounted-rho` it is `4d67b5ccc` (`test-vectors/head-file-digests-4d67b5ccc.json`, [verification 8.8](verification.md#88-the-merge-of-featurecost-accounted-rho)).
- **`runtime_spec`.** The two casper tests `empty_state_hash_should_be_the_same_as_hard_coded_cached_value` and `state_hash_after_fixed_rholang_term_execution_should_be_hash_fixed_without_hard_fork` keep their constants.

To run the gates, do these steps from the repository root:

1. Run `python3 scripts/verify-reified/check-footprint.py --policy p1`.
2. Run `cargo test -p rholang --release --test reified_ordinary_regression --test reified_source_normalization`.
3. Run `cargo test -p rspace_plus_plus --release --test legacy_golden_corpus`.
4. Run `cargo test -p casper --release runtime_spec`.

### S9. Verification

The substrate has formal models in Rocq and TLA+, property-based tests for each headline and invariant, deterministic suites, and negative controls.
The [model map](../../../formal/reified-rspaces-model-map.json) links each Rust item to the models.
The [invariant map](../../../formal/reified-rspaces-invariant-map.json) links each headline and invariant to its property tests.

**Rocq headlines.** Table S9.1 lists the 23 headlines of P1.
Each one closes under the global context, with no axiom, no parameter, and no admitted proof.

| Id | Headline | Citation | Statement | Property tests |
|---|---|---|---|---|
| H1 | `commit_trace_eq_host` | `EventCorrelation.v:5501` | A commit makes the observer calls, events, and status of the host run of the same operations. | `prop_space_txn_play_trace_equals_host`, `prop_space_txn_repeated_channel_groups_equal_host` |
| H2 | `commit_state_eq_host` | `EventCorrelation.v:5559` | A commit leaves the store of the host run, with data lists equal up to permutation. | `prop_space_txn_play_root_equals_host`, `prop_space_txn_repeated_channel_groups_equal_host` |
| H3 | `counter_timing_play_replay` | `EventCorrelation.v:6200` | The play closure and the replay rule give the same COMM counters. | `prop_counter_timing_play_equals_replay` |
| H4 | `verify_sound_complete` | `EventCorrelation.v:6290` | Verify pairs a COMM exactly when the log holds the trigger event and a free occurrence remains. `recorded_comms` reports the same copies. A replay fire without its trigger event stages unpaired, and an unpaired fire never commits: it emits no event or effect, and it keeps the bindings and the store (decision 36). | `prop_space_txn_replay_divergence_typed_at_commit`, `prop_space_txn_replay_requires_trigger_event` |
| H5 | `replay_pairs_each_comm_once` | `EventCorrelation.v:6494` | Replay pairs each recorded COMM occurrence at most once. | `prop_space_txn_replay_pairs_play_log`, `prop_space_txn_repeated_channel_groups_equal_host`, `prop_space_txn_peek_restore` |
| H6 | `view_commit_coherent` | `EventCorrelation.v:5586` | The staged view equals the committed store. | `prop_space_txn_view_equals_committed_store` |
| H7 | `joins_lockstep_preserved` | `EventCorrelation.v:5609` | Join entries stay in lockstep with the waiting continuations. | `prop_space_txn_joins_lockstep` |
| H8 | `sub_order_between` | `EventCorrelation.v:8058` | Sub-orders follow the parent order, precede its successors, and are contiguous and unique. | `prop_sub_orders_contiguous_and_disjoint` |
| H9 | `abort_inert` | `EventCorrelation.v:7159` | A drop, an access error, and a commit after a store swap change no store, binding, observer call, or event. | `prop_space_txn_abort_inert`, `prop_space_txn_non_canonical_inputs_fail_closed` |
| H10 | `coverage_total` | `EventCorrelation.v:6082` | Every store change of a commit has its event. | `prop_event_coverage_holds_for_generated_commits` |
| H11 | `peek_reproduces_trace_eq_host` | `EventCorrelation.v:5836` | With the re-offer and the re-stores as the next operations, a commit makes the calls, events, status, and bindings of the host run, whose reducer sends the peeked data again. The stores are equal ([decision 45](decision-register.md#decision-45)). | `prop_space_txn_peek_restore` |
| H12 | `reporting_trace_is_replay_trace` | `EventCorrelation.v:7244` | Reporting stages and commits as replay does. | `prop_space_txn_reporting_equals_replay`, `prop_space_txn_peek_restore` |
| H13 | `sweep_exact` | `VolatileSweep.v:894` | The sweep removes exactly the volatile data, continuations, and joins. Through the handle, a sweep in a task with an open transaction fails closed. | `prop_sweep_volatile_exact_and_root_neutral` |
| H14 | `delete_absent_identity` | `VolatileSweep.v:971` | A history delete of an absent key changes nothing. | `prop_sweep_volatile_exact_and_root_neutral` |
| H15 | `sweep_root_neutral` | `VolatileSweep.v:1266` | The checkpoint after a sweep gives the trie of the durable changes alone. | `prop_sweep_volatile_exact_and_root_neutral` |
| H16 | `leaf_count_is_length` | `VolatileSweep.v:1329` | The root leaf record count is the length of the data list. | `prop_root_leaf_record_count_matches_history` |
| H17 | `non_canonical_fails_closed` | `EventCorrelation.v:7101` | A group that is not non-decreasing fails before any store or cache effect. The sorted list is the only canonical member of its permutation class. | `prop_space_txn_non_canonical_inputs_fail_closed`, `prop_space_txn_repeated_channel_groups_equal_host` |
| H18 | `canonical_reads_root_neutral` | `VolatileSweep.v:1444` | Reads of canonical keys leave the checkpoint root unchanged. | `prop_space_txn_reads_root_neutral` |
| H19 | `directive_fails_closed` | `EventCorrelation.v:7284` | A replay directive rejects the operation with no effect. | `prop_space_txn_replay_directive_fails_closed` |
| H20 | `parent_order_single_txn` | `EventCorrelation.v:8331` | Sub-orders are unique in an evaluation scope, and each reset admits the same parent again. | `prop_space_txn_parent_order_single_commit` |
| H21 | `view_identity_is_store_identity` | `EventCorrelation.v:8452` | View lookups agree with store lookups by store identity. | `prop_space_txn_store_identity_ignores_ord_only_fields`, `prop_space_txn_par_identity_ignores_locally_free` |
| H22 | `checkpoint_rejects_duplicate_key_actions` | `VolatileSweep.v:1628` | The checkpoint refuses two actions on one history key, and canonical cache keys have distinct history keys. | `prop_space_txn_non_canonical_inputs_fail_closed` |
| H23 | `replay_reproduces_play_run` | `EventCorrelation.v:7834` | With the commit log of play, replay stages the operations of play, and its commit makes the observer calls, events, status, and store of play. So an operation that play rejects fails replay at the same call, after the same prefix (decision 36). | `prop_space_txn_replay_reproduces_play_rejection` |

The models are [EventCorrelation.v](../../../formal/rocq/reified_rspaces/theories/EventCorrelation.v), with H1 to H12, H17, H19 to H21, and H23, and [VolatileSweep.v](../../../formal/rocq/reified_rspaces/theories/VolatileSweep.v), with H13 to H16, H18, and H22.
Since [decision 36](decision-register.md#decision-36), H1, H2, and H11 assume in replay that every staged fire paired, which a successful staging implied before.
H4 and H23 cover the unpaired fire.
H23 assumes that the observer ignores the stateless directive queries and gives no directive.
The lemma `replay_premises_satisfiable` (`EventCorrelation.v:8836`) shows that the observer of a phlo limit meets both premises.

**TLA+ invariants.** Table S9.2 lists the invariants of the two TLA+ modules [[Lam94](#ref-lam94)].
TLC explores the complete state space of each safe configuration.
Apalache checks the inductive forms `IndInv` and `LockIndInv` with a base step and an induction step.

| Id | Invariant | Module | Safe configurations | Negative controls |
|---|---|---|---|---|
| T1 | `ReplayPrefixEqual` | `CheckpointReplay.tla` | `MC_CheckpointReplay.cfg`, `MC_CheckpointReplay_TwoBlock.cfg`, and the three configurations of decision 45: `MC_CheckpointReplay_Peek.cfg`, `MC_CheckpointReplay_Legacy.cfg`, `MC_CheckpointReplay_Legacy_TwoBlock.cfg` | `UuidUnsafe.cfg`, `AcceptUnpairedUnsafe.cfg` |
| T2 | `SameRootIffSameState` | `CheckpointReplay.tla` | The same | `RootIgnoresVolatileUnsafe.cfg` |
| T3 | `PairedOnce` | `CheckpointReplay.tla` | The same, and Apalache through `IndInv` | `DoublePairUnsafe.cfg`, `DoublePairUnsafe_IndStep_Apalache.cfg` |
| T4 | `NoVolatileResidue` | `CheckpointReplay.tla` | The same, and Apalache through `IndInv` | `JoinResidueUnsafe.cfg`, `JoinResidueUnsafe_IndStep_Apalache.cfg` |
| T5 | `Serializable` | `TxnLockCoverage.tla` | `MC_TxnLockCoverage.cfg`, and Apalache through `LockIndInv` | `UncoveredChannelUnsafe.cfg`, `UnjoinedChannelUnsafe.cfg`, and their Apalache step forms |
| T6 | `ReplayOutcomeEqual` | `CheckpointReplay.tla` | The same as T1 | `StagingDivergenceUnsafe.cfg`, `LegacyPreAfterObserveUnsafe.cfg` |

The modules are [CheckpointReplay.tla](../../../formal/tlaplus/reified_rspaces/CheckpointReplay.tla) and [TxnLockCoverage.tla](../../../formal/tlaplus/reified_rspaces/TxnLockCoverage.tla).
Each configuration file starts with a line that states its bounds.
For example, `MC_TxnLockCoverage.cfg` covers two channels on two stripes, the groups that repeat a channel, two transactions, and two host operations.
Since decision 36, the deploy of each block in `CheckpointReplay.tla` has a phlo limit, and the proposer may omit the COMM of its last fire.
T6 states that an honest block is never rejected, and that its replayed deploy ends with the failure and the spending of play.

Since [decision 45](decision-register.md#decision-45), the module also models peeks under Rule E and the legacy charges of Option P.
Its invariant U13 `LegacyChargesFollowHost` is an id of the P4 map, and section [N10](#n10-legacy-charges) describes it.

**Property tests.** The invariant map names 25 properties.
Twenty-four are in [reified_space_store_handle_props.rs](../../../rspace++/tests/reified_space_store_handle_props.rs), and `prop_space_txn_par_identity_ignores_locally_free` is in [reified_space_store_handle_par.rs](../../../rholang/tests/reified_space_store_handle_par.rs), over real `Par` channels.
Each property runs 64 cases with a fixed seed and without failure persistence, so a run is repeatable and writes nothing to the source tree.
Where a property allows it, the generators also make groups that are not canonical, which the substrate must reject with no effect.
They also make sorted groups that repeat a channel, which the substrate must accept.

**Deterministic suites.** Table S9.3 lists the other suites of the substrate.

| Suite | Tests | Subject |
|---|---|---|
| [reified_space_store_handle.rs](../../../rspace++/tests/reified_space_store_handle.rs), group A | 22 | Host parity of every operation shape, checked against a twin host. |
| Group B | 5 | Checkpoints, history read-through, and a round trip through an LMDB directory that is closed and opened again. |
| Group C | 11 | Replay lookup parity with host replay, the typed divergence at commit, and the replay of a rejection of play (decision 36). |
| Group D | 7 | The volatile sweep. |
| Group E | 3 | Reporting delegation. |
| Group F | 15 | The review regressions: canonical order, store identity, trigger-key pairing, lock coverage, one commit per parent order, nested begin, sweep bookkeeping, and peeks in replay. |
| Group G | 10 | The round-2 rules: repeated channels, parent tracking per scope, the cost of the commit checks, and the nested guard of the sweep. |
| Group H | 9 | The round-3 rule and Rule E of [decision 45](decision-register.md#decision-45): the peek re-offer, the re-store, the queue scenario, host parity, replay, reporting, the receipt flags, and the control of a re-store without the removal. |
| Unit tests in `space_store_handle.rs`, `space_play.rs`, and `space_replay.rs` | 18 | The order of sub-orders, the list rules of the view, the event correlation, the peeks rule, the store identity, canonical groups, lock stripes, the parent tracker, the registration, claimed positions, receipt flags, counter overflow, and the replay estimate of the sweep. |
| `rholang/tests/reified_space_txn_host_bounds.rs` | 1 | The `Par` instance of `SpaceTxnHost` for the three hosts. |
| `rholang/tests/reified_space_store_handle_par.rs` | 3 | The dedup of `TaggedContinuation`, the store identity of `Par` channels that differ only in `locally_free`, and the property `prop_space_txn_par_identity_ignores_locally_free`. |

**Negative controls.** Each control shows that a check or a proof depends on the rule that it covers:

- The script [rocq-negative-controls.py](../../../scripts/verify-reified/rocq-negative-controls.py) runs 29 mutants of the P1 models. Eleven cover FR-1 and FR-1', six cover FR-5', and one covers FR-9'. Four cover FR-10, three cover decision 36, one covers decision 44, and three cover Rule E of [decision 45](decision-register.md#decision-45). Each mutant must fail to compile inside its expected lemma.
- Eight unsafe TLC configurations each refute exactly their invariant, and four Apalache step controls each break their inductive invariant.
- The P4 test `space_queue_out_of_phlo_sweep_replays_equal` is the control of decision 36. On the code before the fix, 143 of its 232 deploys replayed to another outcome than play.
- The unit tests `correlation_rejects_datum_without_produce_event` and `correlation_rejects_comm_before_trigger` show that the event correlation rejects a defect.
- Three control tests pin HEAD behavior that the substrate avoids: `space_store_handle_remove_all_continuations_keeps_joins_control`, `space_store_handle_host_permuted_keys_break_checkpoint_control`, and `space_store_handle_host_replay_repeated_channel_control`.

To run the verification, do these steps from the repository root:

1. Run `scripts/check-reified-rspaces-proofs.sh` to build the Rocq models and run their negative controls.
2. Run `scripts/check-reified-rspaces-tla-invariants.sh` for the TLC, Apalache, and unsafe-configuration legs.
3. Run `python3 scripts/verify-reified/check-model-map.py` and `python3 scripts/verify-reified/check-invariant-map.py`.
4. Run `cargo test -p rspace_plus_plus --release` for the substrate suites and properties.
5. Run `cargo test -p rholang --release --test reified_space_store_handle_par --test reified_space_txn_host_bounds`.

The scripts write their logs under `target/verification/reified-rspaces/` and never under `/tmp`.

### S10. Limits

**The always-on checks cover changed entries only.** The commit checks of FR-7' run in every build profile, so debug and release builds commit identical results.
Each check reads only the entries that its operation changes.
Before the effect, it checks the removed records, the position of a removed continuation, and the join groups of the produce channel.
After the effect, it correlates the staged effects with the events of the operation.

The checks do not read inserted entries back.
The hot store returns whole lists, so a read-back would copy the whole list.
The footprint freezes `hot_store.rs`, so the substrate cannot add a cheaper accessor.
Headline H6 (`view_commit_coherent`) and the property `prop_space_txn_view_equals_committed_store` cover the inserted entries instead.

To apply a fire, the commit still reads the data list of each bound channel, because a host removal takes a list index (`committed_candidates`).
This read belongs to the operation, not to the checks, and the host operation reads the same lists for its own match.

The test `space_store_handle_commit_checks_cost_changed_entries` shows that the record copies of a staging and of a commit do not grow from 1 to 32 stored records.
The measured cost of the checks is +0.8% to +3.0% of the commit latency at 64 records.
The measurement used a release build on one pinned core, and the medians of three alternating rounds, for a one-channel shape and a distinct-channels shape.

The test `space_store_handle_fire_checks_cost_changed_entries` checks the same bound for a consume fire.
Its staging copies and its commit serializations do not grow from 1 to 32 stored records.
Its commit clones grow by exactly one read of the bound list.
A mutant check that serializes the whole list fails this test.

**Consume-side pairing is stricter than host replay.** Host replay finds the COMM of a consume also through `consume_replay_index` (`replay_rspace.rs:946-983`).
`rig` fills that index from every COMM of the log, also from a COMM whose Consume event the log lacks (`replay_rspace.rs:500-510`).

The substrate pairs a consume-side fire only under `Consume(src)`.
So for a log without the Consume event, host replay pairs the fire, but the substrate stages it as an [unpaired fire](glossary.md#unpaired-fire).
The commit then fails at the fire with `ReplayDivergence`, after the observer calls of play, and the operations before it stay applied.
An honest log always holds this event, because a consume-side fire logs its consume in the same operation (Table S3.1, step 6).
The test `space_store_handle_replay_requires_consume_trigger_event` pins both behaviors.

**A log that omits a COMM.** Casper compares only the failed flag and the cost of a replayed deploy (`casper/src/rust/rholang/replay_runtime.rs:619-632`).
A proposer can omit the COMM of a space fire from its log, and replay then fails the deploy with `ReplayDivergence`.
Validators accept such a block when the failed flag and the cost match, both before and after [decision 36](decision-register.md#decision-36).
Host replay gives a proposer the same room for a host COMM, because it stores the datum or parks the continuation instead (`replay_rspace.rs:1214-1219`).

This observation is risk R1 of decision 36, and the campaign does not pursue it.
Section 7.2 of the [finding](findings/space-replay-out-of-phlo.md#72-a-log-that-omits-a-comm) records it, with the related defect of host replay for a rejected COMM charge (pgmcp finding 9921).

**HEAD findings.** The reviews of P1 found three defects of the baseline host.
They are recorded in the pgmcp tracker for the RSpace and Casper owners, and the campaign does not fix them ([decision 12](decision-register.md#decision-12)).
Table S10 lists them with their pgmcp ids.

| pgmcp id | Finding | Severity | Relation to the substrate |
|---|---|---|---|
| 9823, `rrs-finding-head-join-order-checkpoint-panic` | A join whose channels runtime aliasing names in two orders stores one channel set under two hot-store keys. The checkpoint then panics, and the history mutex stays poisoned. | critical | The substrate admits only canonical groups ([S5.5](#s55-canonical-order-fr-1)), so a space key never takes two orders. |
| 9824, `rrs-finding-head-guarded-join-replay-divergence` | A join with a `where` guard can fire from stored data alone after a produce. Play then drops the produced datum, but replay stores it, so the roots differ. | critical | The substrate requires exactly one incoming candidate in a produce-side fire, so it refuses this fire shape. P4 agents that call `check_commit` must keep this rule. |
| 9828, `rrs-finding-head-repeated-channel-replay-panic` | For a join that names one channel twice, host replay can compute another COMM than play, and its membership assertion panics. | high | The substrate pairs by identity under the trigger key and keeps the canonical repeated order, so it replays such logs. |

**Other limits.**

- One handle for each host. The nested guard and the parent tracker live in the handle. Two separate handles over one host would not share them, so the runtime must keep one handle for each host (P4).
- Path depth. Each sub-order adds one path segment, so a transaction at the native limit of path segments fails native recording (`rholang/src/rust/interpreter/accounting/native_runtime/operations.rs:234`).
- Record count. `root_leaf_record_count` counts the records in the outer frame of the leaf, through `get_data_proj_binary`, with no change of the history modules (decision 19, D5).
- Late divergence. A park where the log recorded a fire surfaces only at `check_replay_data`, as in host replay ([S5.3](#s53-verify-fr-3)). A fire where the log holds no free COMM surfaces at commit, after the observer calls of play.

## Native spaces

Phase P4 builds the first working Reified RSpace: the [space family](glossary.md#space-family) `hashmap:queue`.
A Rholang program creates such a space with its [factory](glossary.md#factory), and it then sends and receives on the [collections](glossary.md#collection) of the space.
Each space operation runs as one [space transaction](glossary.md#space-transaction) of the [substrate](#substrate).
So the events, the charges, the replay, and the merge of a space operation are those of host operations ([decision 19](decision-register.md#decision-19)).

The [specification](specification.md) states the required behavior of the queue, of the HashMap outer store, and of the default qualifier.
The [agent API](agent-api.md) states the calls, the replies, and the errors that a program sees.
This section explains how the code meets them, in fourteen parts that follow the life of one space operation:

1. [N1](#n1-module-map-and-hook-lines): the new modules and the hook lines in the three hot files of the reducer.
2. [N2](#n2-namespace-and-records): the ids under the namespace tag, and the envelopes of the records ([decision M-2](decision-register.md#m-2)).
3. [N3](#n3-counters): the counters, which make cross-branch races visible to the merger ([decision M-1](decision-register.md#m-1), [decision 31](decision-register.md#decision-31)).
4. [N4](#n4-genericspace-and-the-queue-discipline): the generic space and its head-only queue ([decision 11](decision-register.md#decision-11)).
5. [N5](#n5-routing-and-scope): the routing of each channel, and the use context of the reducer.
6. [N6](#n6-scheduling): the space intents of the deterministic scheduler.
7. [N7](#n7-factories-and-handles): the factory, the handle, `HANDLE_OP`, `getSpaceAgent`, and `assertEqual`.
8. [N8](#n8-errors): the errors and their codes.
9. [N9](#n9-limits-of-p4): the limits of this phase.
10. [N10](#n10-legacy-charges): the legacy charges of each staged operation ([decision 45](decision-register.md#decision-45)).
11. [N11](#n11-wave-w2-of-p5-the-drivers-the-stores-and-the-hooks): the drivers, the stores and the hooks of P5 wave W2.
12. [N12](#n12-wave-w3-of-p5-joins-of-one-space): the join engine of P5 wave W3 ([decision 29](decision-register.md#decision-29)).
13. [N13](#n13-wave-w3-of-p5-the-pathmap-engine): the PathMap engine of P5 wave W3 ([decision 51](decision-register.md#decision-51)).
14. [N14](#n14-decision-47-spaces-in-native-execution): spaces in native play and native replay ([decision 47](decision-register.md#decision-47)).

The pseudocode of this section uses the notation of [Table S5.0](#s5-algorithms), in the literate form of Knuth [[Knu84](#ref-knu84)].

### N1. Module map and hook lines

P4 adds new files only, plus a few hook lines in the three hot files of the interpreter.
Table N1.1 lists the new modules.
The paths of the rows are relative to `rholang/src/rust/interpreter/`, except the last row.

*Table N1.1. The new modules of P4.*

| Module | Files | Role |
|---|---|---|
| `spaces` | `namespace.rs` | The ids under the tag `0xE0`, their decoder, the collection key, and the random purposes. |
| `spaces` | `record.rs` | The record envelopes, their strict decoders, and the authorities of the staged operations. |
| `spaces` | `counter.rs` | The bump of a collection counter, and the opening of a collection on the space counter. |
| `spaces` | `matching.rs` | The metered calls of the injected matcher. |
| `spaces` | `generic.rs` | `GenericSpace<S, D>`, the traits `OuterStore` and `Discipline`, and the plans of a produce and of a consume. |
| `spaces` | `outer/hashmap.rs`, `disciplines/queue.rs` | The HashMap outer store and the queue discipline. |
| `spaces` | `agent.rs`, `errors.rs` | The interface between the agents and their callers, and the space errors with their codes. |
| `spaces` | `types/`, `factory/` | The config map, the space id, the qualifier, the constructor, the schemas, and the registered families. |
| `reified` | `reducer_hooks.rs` | The hooks of the reducer: USE, NEWSPACE, sends, receives, `new`, and the dispatch of fires. |
| `reified` | `route.rs`, `scope.rs` | The routing classes and rules, the space references, and the use context. |
| `reified` | `schedule.rs`, `runtime.rs`, `costs.rs` | `Intent::Space`, the runtime of a reducer, the host-work meter, and the fire-round cap. |
| `reified` | `execution.rs` | The R7 backend of the system processes. |
| `reified` | `factory.rs`, `handle.rs`, `system_processes.rs`, `system_channels.rs` | The factory, `HANDLE_OP`, `getSpaceAgent`, `assertEqual`, and their op channels. |
| `reified` | `errors.rs` | The codes of the errors of the reducer hooks. |
| `models` | `models/src/rust/space_factory_urn.rs` | The 32 families, their URNs and aliases, and the parser of a constructor. |

*Table N1.2. The P4 hook lines in the hot files.*

| Hook | Lines | Effect |
|---|---|---|
| Fields of `ReducerCore` | `reduce.rs:187-188` | The use context `space_ctx` and the runtime `reified`. |
| Evaluation scope | `reduce.rs:460`, `475`, `507` | `begin_root_evaluation` at the entry of `inj`, `inj_with_observation`, and `inj_with_host_work`: a root evaluation calls `begin_evaluation_scope`, and it sweeps the volatile channels at its end (P7, VR-3 and VR-4). |
| H2 | `reduce.rs:1241` | A marker tuple in process position goes to `eval_marker`. |
| H3 | `reduce.rs:1309` | `eval_send` calls `reified_produce`. |
| Receive | `reduce.rs:1426` | `eval_receive` calls `reified_consume`. |
| `new` | `reduce.rs:1650` | `eval_new` calls `reified_allocate`. |
| H4 | `reduce.rs:1667` | `unbundle_receive` calls `reject_query_source` before its bundle check. |
| Detached runtime | `reduce.rs:7891-7892` | `ReducerCore::new` starts without a use context and with a detached runtime. |
| Hook module | `reduce.rs:8209-8210` | `reified/reducer_hooks.rs` is the child module `reified_hooks`. |
| Scheduler module | `deterministic_reduction.rs:41-42` | `reified/schedule.rs` is the child module `reified_schedule`. |
| Space intents | `deterministic_reduction.rs:556`, `586`, `602`, `611`, `627` | `Intent::Space`, `Completion::Space`, and the rejection, the order, and the delivery of a space completion. |
| Footprint and execution | `deterministic_reduction.rs:935`, `984` | The footprint and the execution of a space intent. |
| Imports | `rho_runtime.rs:30`, `52` | `SpaceTxnHost` and the module `reified`. |
| Reducer setup | `rho_runtime.rs:1494`, `1510`, `1543` | The runtime parameter, the R7 wrap of the dispatch space, and the runtime field. |
| Native replay | `rho_runtime.rs:1918-1919`, `1950` | A runtime that fails closed, with the Reified definitions. |
| Host runtime | `rho_runtime.rs:1991-1992`, `2035` | The runtime of the host space, captured before the scheduler wrap, with the Reified definitions. |
| Host bounds | `rho_runtime.rs:1985`, `2072`, `2142`, `2185`, `2211`, `2217` | The bound `SpaceTxnHost` on the six runtime constructors. |

Every hook returns a future that adds little depth to the evaluation future of the reducer.
`reified_produce` and `reified_consume` return boxed futures.
`eval_marker` returns a boxed future, and `reified_allocate` returns a future that is ready at once outside a use-context and boxed inside one.
The evaluation future of the reducer is part of every runtime future that casper awaits.
When it is deeper, the layout of `CasperLaunchImpl::launch` exceeds the query depth limit of rustc, and the release build of `node` fails.
`cargo check` computes no such layout, so only a build finds this error.

Each hook calls code of the new modules with the arguments of the baseline call.
For a channel without Reified meaning, each hook makes the baseline call with identical arguments.
So ordinary execution does not change, and the footprint policy `p4` pins every other line of the hot files ([verification](verification.md#11-phase-p4-the-hashmapqueue-slice)).

One space operation takes this path:

1. The reducer evaluates a send or a receive and removes the bundle of its channel.
2. A hook routes the channel ([N5](#n5-routing-and-scope)). A default route makes the baseline call.
3. A space route builds a `SpaceOp` and submits it as a [space intent](glossary.md#space-intent) ([N6](#n6-scheduling)).
4. The job of the intent opens one transaction, bumps the counter, plans head-only, and commits once ([N3](#n3-counters), [N4](#n4-genericspace-and-the-queue-discipline)).
5. The reducer dispatches the fired continuations, and it runs a [re-issue](glossary.md#re-issue) as a new space intent.

### N2. Namespace and records

Every Reified name is a `GPrivate` whose id starts with the [namespace tag](glossary.md#namespace-tag) `0xE0` ([decision 7](decision-register.md#decision-7)).
A program cannot write a `GPrivate` id, so only the Reified machinery makes these names.
Table N2.1 lists the ids that P4 constructs in `spaces/namespace.rs:229-284`.
Here ‖ is byte concatenation, `sid` is the 32-byte [space id](glossary.md#space-id), and `k` is the [collection key](glossary.md#collection-key).
[Figure 02](diagrams/02-namespace-bytes.svg) shows every id layout of the namespace.

*Table N2.1. The ids that the P4 code constructs.*

| Id | Bytes | Constructor | Meaning |
|---|---|---|---|
| `E0‖op` | 2 | `op_channel` | an [op channel](glossary.md#op-channel) |
| `E0‖C‖sid` | 34 | `config_channel` | the config channel $`C(\mathit{sid})`$ |
| `E0‖S‖sid` | 34 | `space_counter` | the [space counter](glossary.md#space-counter) $`S(\mathit{sid})`$ |
| `E0‖S‖sid‖k` | more than 34 | `collection_counter` | the [collection counter](glossary.md#collection-counter) $`S(\mathit{sid}, k)`$ |
| `E0‖D‖sid‖k` | more than 34 | `data_channel` | the data channel $`D(\mathit{sid}, k)`$, with the records and the waiters of the collection |
| `E0‖H‖sid` | 34 | `handle_channel` | the channel `this`, $`H(\mathit{sid})`$ |
| `E0‖N‖sid‖tail` | 66 or 42 | `name_id` | a [space name](glossary.md#space-name) |

The decoder `decode_reified_id` (`spaces/namespace.rs:340-370`) is total, and it accepts only the image of a constructor:

- An id without the tag, or with fewer than 2 bytes, is not a Reified id.
- An id of exactly 2 bytes is an op.
- In a longer id, byte 1 must be a KIND letter or its lowercase form, and 32 bytes of space id must follow. The KINDs M and T have no lowercase form.
- The length must fit the KIND. It is 34 bytes for C, H, A, and B, and 66 or 42 bytes for N. It is more than 34 bytes for D, and at least 34 bytes for S, I, X, M, and T.

The Rocq headline `decode_encode_reified_id` (`Namespace.v:638`) proves that the decoder inverts each constructor and accepts only their images.

**Collection keys.** A HashMap space keeps one collection for each collection key.
The key is the bincode of the sorted channel, which is the preimage of the store identity ([decision 32](decision-register.md#decision-32), item A5):

```rust
/// `bincode(ParSortMatcher::sort_match(channel).term)`, the preimage of the store identity of
/// the channel (A5). Serde writes `locally_free` as empty bytes, so channels that are equal as
/// `Par` values have one key.
pub fn canonical_inner_channel_bytes(channel: &Par) -> Result<Vec<u8>, SpaceError> {
    bincode::serialize(&ParSortMatcher::sort_match(channel).term).map_err(|_| {
        SpaceError::Substrate(SpaceTxnError::InvalidOperation(
            "a channel is not serializable",
        ))
    })
}
```

The Protocol Buffers bytes of a `Par` hold its field `locally_free`, so two channels that are equal as `Par` values could have two prost keys.
The bincode bytes have one key for both, and the headline `canon_respects_par_eq` (`Namespace.v:1043`) states this property.

**Random purposes.** The machinery derives each random state that it needs from the random state $`r`$ of a call or of an operation, by two splits:

```math
r_{\mathit{purpose}} = \mathrm{split}_{\mathit{purpose}}\bigl(\mathrm{split}_{\mathtt{0xE0}}(r)\bigr)
```

*Table N2.2. The random purposes of `RandomPurpose` (`spaces/namespace.rs:425-450`).*

| Purpose | Byte | Use |
|---|---|---|
| `SpaceId` | `0x01` | the space id of a factory call ([decision I-19](decision-register.md#i-19)) |
| `Gensym` | `0x02` | the tail of a name that `gensym` makes |
| `Xlate` | `0x03` | the translation of a user agent-backed space (P8) |
| `PathMarker` | `0x04` | a path marker (P5) |
| `Config` | `0x05` | the config datum |
| `Handle` | `0x06` | the handle continuation |
| `SpaceCounter` | `0x07` | the space counter: split 0 for its continuation, split 1 for its datum |
| `Counter` | `0x08` | a collection counter: split 0 for its continuation, split 1 for its datum |

The machinery never splits the random state of a user datum for its record. So a record keeps the random state that the program gave it.

**Record envelopes.** A native space stores ordinary host records on its storage channels ([decision M-2](decision-register.md#m-2)).
Each record carries a [record envelope](glossary.md#record-envelope) inside its payload, so the host structs, the serializers, and the identities do not change.
Table N2.3 gives the four kinds of record of `spaces/record.rs`.
Each envelope starts with a tag `GByteArray([0xE0, 0x52, kind, 1])`, version 1 of the envelope of its kind (item A20).

*Table N2.3. The records of a native space.*

| Record | Content | Encoder | Decoder |
|---|---|---|---|
| datum | `pars = [EList[tag_D, GInt(ord), Nil, Nil], payload…]` | `wrap_datum` | `unwrap_datum` |
| parked continuation | `body = Par{exprs: [EList[tag_K, GInt(ord), Nil, body]]}` | `wrap_continuation` | `unwrap_continuation` |
| counter | `pars = [EList[tag_S], GInt(n)]` | `counter_record` | `counter_value` |
| config | `pars = [EList[tag_C], config map]` | `config_record` | `config_map` |

The `Nil` slots are the slots of later phases: the priority and the embedding of a datum, and the query policy of a continuation.
The first par of every record is a list, so the merger never reads a Reified datum as a number cell (item A11).
A datum record keeps the random state, the cost authority, and the cost stack of the user datum.
A parked continuation keeps its random state, its guard, and its cost authority outside the envelope.

Every decoder is strict: it accepts exactly the image of its encoder, with the version 1 and a non-negative order.
Any other record is a `MalformedRecord`, and the operation fails with `Inconsistent`.
The headlines of `RecordEnvelope.v` prove the round trips, the injectivity, the soundness of the decoders, and the disjointness of the kinds.

**The config record.** The [config record](glossary.md#config-record) holds the canonical map of [decision I-17](decision-register.md#i-17), which `SpaceConfig::to_par` builds (`spaces/types/config.rs:93-127`).
For a queue space with the default qualifier, the map is `{"spaceId": GByteArray(sid), "outer": "hashmap", "inner": "queue", "qualifier": "default", "params": {}, "theory": Nil}`.
The decoder `SpaceConfig::from_par` accepts only the exact image of `to_par`, and in P4 the theory must be `Nil`.
*Since P7 step 4, the theory slot holds `Nil` or one `GByteArray` with the canonical bytes of the theory identity ([decision 74](decision-register.md#decision-74), [specification 5](specification.md#5-theories)).*
Each read of a config record also requires that its `spaceId` is the space id of the space, otherwise the operation fails with `Inconsistent`.

**Authorities.** Each operation builds one `Introduction`, which holds its introduction authority and the phlo budget of the deploy (`spaces/record.rs:2118-2159`).
Each staged operation registers that authority before the observer sees it, as the reducer does for a host operation (`spaces/record.rs:2167-2189`).
Table N2.4 gives the authorities of each record (P4 design B.7).

*Table N2.4. The authorities of the records.*

| Record | Cost authority of the record | Registered introduction authority |
|---|---|---|
| user datum (a send) | the cost authority of the datum | the introduction authority of the send |
| parked continuation (a receive) | the receive authority, when it has a region | the introduction authority of the receive |
| counter continuation `K(v)` | the cost authority of the datum or of the continuation of the operation | the introduction authority of the operation |
| new counter datum | the introduction authority of the operation, when it has a region | the introduction authority of the operation |
| config datum | none | the cost authority of the factory call |
| space counter `S(sid) = 0` | the cost authority of the factory call, when it has a region | the cost authority of the factory call |
| handle continuation | none | the cost authority of the factory call |

So each call is paid by its caller, and never by the creator of the space.
A re-offer and a re-fire repeat an identity with its first authority, which the registry accepts (readiness review S6).
Another authority for a repeated identity fails the commit at that operation, and the operations before it stay applied ([prefix atomicity](glossary.md#prefix-atomicity)).

### N3. Counters

A merge must reject two branches that use one ordered collection, because their merged state could have an order that no serial run gives.
P4 uses the existing merge check #1 for this purpose and changes no merge code ([decision M-1](decision-register.md#m-1)).
Each operation on a queue consumes the counter record of its collection and produces it again with the next value, in its own transaction.
So two branches that use one collection consume one counter produce, and check #1 sees a race.

**The bump.** `bump` (`spaces/counter.rs:165-245`) stages the counter steps before the plan of the operation.
It returns the order of the operation, which is the value of the collection counter before the bump.

```text
BUMP(txn, S_space, S_coll, ctx):               ▷ spaces/counter.rs, bump
    records ← txn.data(S_coll)
    if records = []:
        ⟨Open the collection⟩
        return (ord = 0, staged = [Take, Put, Put])
    if records = [c]:
        ⟨Step the collection counter⟩
        return (ord = v, staged = [Take, Put])
    fail Inconsistent                              ▷ two counter records
```

⟨Open the collection⟩.
The collection has no counter yet, so this operation is its first.
The operation consumes the space counter and produces it again, and it opens the collection counter with the value 1.

```text
        require txn.data(S_space) = [s]                        ▷ Inconsistent
        m ← COUNTER-VALUE(s)                                   ▷ Inconsistent
        require m < i64::MAX                                   ▷ CounterOverflow
        TAKE(S_space, s, m, SpaceCounter)                      ▷ events Consume, COMM
        PUT(S_space, m + 1, SpaceCounter)                      ▷ event Produce
        PUT(S_coll, 1, Counter)                                ▷ event Produce
```

⟨Step the collection counter⟩.
The operation consumes the counter record with the value $`v`$ and produces the value $`v + 1`$.

```text
        v ← COUNTER-VALUE(c)                                   ▷ Inconsistent
        require v < i64::MAX                                   ▷ CounterOverflow
        TAKE(S_coll, c, v, Counter)                            ▷ events Consume, COMM
        PUT(S_coll, v + 1, Counter)                            ▷ event Produce
```

The two steps stage ordinary operations of the substrate.
`TAKE` fires a single-channel consume against the stored counter record, and `PUT` stores the next counter record.

```text
TAKE(S, record, v, purpose):                   ▷ spaces/counter.rs, take
    require record is not persistent                           ▷ Inconsistent
    m ← BIND(COUNTER-PATTERN, record)                          ▷ Inconsistent when no match
    K ← ParBody(GInt(v)) with the random state split_0(r_purpose)
    stage consume([S], [COUNTER-PATTERN], K, persist = false, peeks = ∅, Fire([record bound at index 0]))

PUT(S, v, purpose):                            ▷ spaces/counter.rs, put
    stage produce(S, counter_record(v) with the random state split_1(r_purpose), persist = false, Store)
```

The body `GInt(v)` makes the consume identity new for each value, also when a re-issued operation has the same random state.
A persistent counter record would make check #1 ignore the race, so the bump refuses it.
The pattern `COUNTER-PATTERN` has two free variables, one for each par of a counter record.
A counter at `i64::MAX` fails with `CounterOverflow`, so every order is a natural number.

**Why check #1 sees the race.** A COMM without peeks files its produces as consumed, and a COMM with peeks files them as peeked (`rspace++/src/rspace/merger/event_log_index.rs:261-265`).
The counter COMM of a bump has no peeks, also when the operation is a peek.
Check #1 (`rspace++/src/rspace/merger/merging_logic.rs:279-386`) reports a race between two branches $`A`$ and $`B`$ under this condition:

```math
\mathrm{race}(A, B) \iff \exists\, p \in \mathit{consumed}(A) \cap \mathit{consumed}(B) : \neg\, \mathit{persist}(p) \,\land\, \neg\bigl(\mathit{mergeable}_A(p) \land \mathit{mergeable}_B(p)\bigr)
```

Here $`\mathit{consumed}(L)`$ is the set of produces that the COMMs without peeks of the log $`L`$ consume.
A Reified channel is never mergeable, because the detection of mergeable channels matches only tuples with a registered tag (consensus checklist item 19).
Let two branches start from a pre-state in which the collection counter holds the produce $`p_v`$.
The first operation on the collection in each branch consumes $`p_v`$, so $`p_v`$ is in both consumed sets, and check #1 reports a race.
Table N3.1 lists the cases.

*Table N3.1. The shared consumed produce of two branches.*

| Branch A | Branch B | Shared consumed produce |
|---|---|---|
| any operation on queue `q` | any operation on queue `q` | the counter produce of `q` in the pre-state |
| the first operation on queue `q1` | the first operation on queue `q2` of the same space | the space counter produce in the pre-state (item A25) |
| operations on `q1` and on `q2` | operations on `q2` and on `q1` | both counter produces |
| operations on `q1` only | operations on `q2` only, both queues open | none: the branches merge |

**Two deploys of one block form one chain.** The second deploy consumes the counter produce that the first deploy created.
So `depends` (`rspace++/src/rspace/merger/merging_logic.rs:67-138`) relates them, and the block index puts them in one chain.
A rejection then removes both deploys together, so no deploy keeps a record that refers to an order of the other one.

**Why reads also bump** ([decision 31](decision-register.md#decision-31)).
Without a bump on a peek, two branches can make a [write skew](glossary.md#write-skew) [[BBG95](#ref-bbg95)].
Let the head records be `h1` on queue `q1` and `h2` on queue `q2`.
Branch A peeks `q1` and then dequeues `q2`. Branch B peeks `q2` and then dequeues `q1`.
The two consumed sets do not overlap, but no serial order of the branches gives both peeks.

A second example uses persistent receives: each branch fires the persistent head waiter of one queue and stores a datum on the other queue.
With a bump on every operation, both examples race on the counters.

The headline `merge_without_race_is_serializable` (`CounterConflict.v:1489`) proves that a merge without a race gives the state of some serial run of whole deploys [[Pap79](#ref-pap79)].
The headline `counter_free_peek_admits_write_skew` (`CounterConflict.v:2485`) computes the write skew of the counter-free peek.

**Cost.** Each operation on a queue pays three extra events: the Consume and the COMM of the take, and the Produce of the put.
The first operation on a collection pays four extra events, because it also produces the space counter.

Figure N3 shows the merge flow of the counter conflict.

![Merge flow of the counter conflict, as an activity diagram. The pre-state holds the counter record S(sid,k) = v of a queue. In branch A a deploy enqueues: its bump consumes the counter produce p_v in a COMM without peeks and produces p_{v+1}. In branch B a deploy dequeues or peeks: its bump also consumes p_v. The merger builds an event log index for each branch, in which a COMM without peeks files its produces under produces_consumed. Check #1 intersects the consumed produces of both branches, finds p_v, which is not persistent and not mergeable, and reports a race, so the merge rejects one branch. A second lane shows two deploys d1 and d2 of one block: d2 consumes the counter produce p_{v+1} that d1 created, so depends(d2, d1) holds and the block index puts both deploys into one chain, which is one unit of rejection. A third lane shows branches that use two different queues of one opened space: they consume different counter produces, check #1 finds nothing, and the merge applies both changes, which equals a serial run.](diagrams/08-counter-conflict-merge.svg)

*Figure N3. Source: [diagrams/08-counter-conflict-merge.puml](diagrams/08-counter-conflict-merge.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

A removal must bump the counter too.
The M-1 arbitration found this with two [crossed stacks](glossary.md#crossed-stacks), and Figure N3b shows the case.
`CounterConflict.tla` runs the same merge for stacks, and its Unsafe control `RemovalCounterFreeUnsafe.cfg` drops the bump of a pop.
The headline `crossed_stacks_without_removal_counters_fabricate` (`CounterConflict.v:2578`) computes the counterexample.

![The crossed stacks, as an activity diagram. The pre-state holds the stack a with the record 10 and the stack b with the record 20, each with its counter record. Branch A pushes 1 onto a and pops b, and branch B pushes 2 onto b and pops a. In lane 1, the code, every operation bumps its counter, so both branches consume the pre-state counter produces of a and b, check 1 finds the shared produces, and the merge rejects one branch. In lane 2 a pop does not bump the counter: the branches consume different produces, checks 1, 2 and 3 find nothing, and the merge keeps both pops, A popped 20 and B popped 10. But the serial order A then B makes B pop 1, and the order B then A makes A pop 2, so no serial order gives the merged outcome.](diagrams/23-crossed-stacks-merge.svg)

*Figure N3b. Source: [diagrams/23-crossed-stacks-merge.puml](diagrams/23-crossed-stacks-merge.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

### N4. GenericSpace and the queue discipline

`GenericSpace<S, D>` (`spaces/generic.rs:557-560`) is the native space agent of the family whose outer store is `S` and whose inner collection is `D`.
The runtime installs the family agent `FamilyAgent`, and `native_agent` returns it (`reified/system_processes.rs:87`).
The family agent reads the config of a space once and runs the `GenericSpace` of the family that the config names (`spaces/families.rs:344-354`, `535-626`).
Two traits separate the parts of a family:

- `OuterStore` (`spaces/generic.rs:139-250`) maps a channel to its collection key, its data channel, and its counter channel. It also mints a space name for `gensym`. Its P5 hooks check a routed key, stage the records of a creation, and allocate index names. The defaults of the hooks do no work.
- `Discipline` (`spaces/generic.rs:325-502`) chooses the head of each side of a collection. It also states whether the discipline uses a counter, which send and query params it takes, and how many entries a side can hold. It holds the plans of the operations, and the default plans are the head driver of this section.

`HashMapOuter` keys a collection by the collection key, and it mints a space name with a 32-byte tail from the purpose `Gensym` (`spaces/outer/hashmap.rs:26-56`).
It keeps the default hooks.
`QueueDiscipline` uses a counter, takes no send or query params, and keeps every default plan (`spaces/disciplines/queue.rs:32-75`).
The family agent runs only the wired families, and a config that names another family fails with `FamilyUnavailable` (`spaces/generic.rs:2321-2332`).
[Decision 29](decision-register.md#decision-29) removes the P4 gate that ran only counter disciplines without joins, because joins into ordered collections arrive in P5c.
Until then, a collection that is part of a join fails with `Inconsistent`.

**A collection operation.** `begin_collection` (`spaces/generic.rs:2103-2317`) opens the transaction and reads the config once.
`run_with` (`spaces/generic.rs:2544-2714`) then stages the operation in that transaction.
The job commits the transaction exactly once (`spaces/agent.rs:509-521`).

```text
COLLECTION-OP(op):                             ▷ FamilyAgent, begin_collection, run_with, run_op
    ch ← CollectionChannels(op.space, op.key)          ▷ C(sid), S(sid), S(sid, k), D(sid, k)
    txn ← begin([C(sid), S(sid), S(sid, k), D(sid, k)])  ▷ every channel that the operation can touch
    ⟨Check the collection⟩
    ⟨Check the key, the params and the query⟩          ▷ before the bump: a failure leaves the counter
    bump ← BUMP(txn, S(sid), S(sid, k), ctx)            ▷ N3, only when D uses a counter
    ord ← OPERATION-ORD(op.reissued_ord, bump.ord)      ▷ decision 43, and 0 without a counter
    reissue ← D.offer(op, ord) for a produce, D.receive(op, ord) for a consume   ▷ defaults: OFFER, RECEIVE
    receipt ← txn.commit()                              ▷ exactly once, prefix-atomic
    return OUTCOME(receipt, reissue)
```

⟨Check the collection⟩.
The operation needs the config record of its space, and the collection must not be part of a join.
A missing config record fails with `DanglingName` when the channel is a space name, and with `DanglingHandle` otherwise.

```text
    config ← the only config record on C(sid)          ▷ DanglingName or DanglingHandle, Inconsistent
    require config names this space                    ▷ Inconsistent
    require config names the family S:D                 ▷ FamilyUnavailable
    require txn.joins(D(sid, k)) ∈ {[], [[D(sid, k)]]}  ▷ Inconsistent
```

⟨Check the key, the params and the query⟩.
The checks stage nothing, so a failed operation commits nothing and leaves the counter unchanged.
An operation without params and without a query does no work here.

```text
    S.check_key(config, op.channel, op.key)              ▷ NotAnIndexName for Array and Vector (P5d)
    require each send param key ∈ D.SEND_PARAMS          ▷ UnknownParam, with the closest key
    require no query, or D.QUERY_PARAMS ≠ []              ▷ QuerySpecUnsupported
    require each query param key ∈ D.QUERY_PARAMS        ▷ UnknownParam, with the closest key
    D.check_values(config, params, query)                ▷ InvalidParam
```

The lock keys hold the space counter too, because the first operation on a collection consumes it. The agent knows that only after a read (readiness review S2).
A produce and a consume stage at least a store, a park or a fire, so no commit is empty.

**The head of a side.** The head of a side of a queue is the entry with the least order. The bincode bytes decide between two entries with one order:

```math
\mathrm{head}(E) = \mathop{\mathrm{arg\,min}}_{e \in E} \bigl(\mathit{ord}(e),\ \mathrm{bincode}(e)\bigr)
```

The minimum is lexicographic, and it depends only on the set $`E`$, not on the list order of the hot store.
The hot store inserts at the front, and a restart can load a list in another order, so the list order cannot decide.
`QueueDiscipline` implements the rule (`spaces/disciplines/queue.rs:41-74`):

```rust
    /// The entry with the least `(ord, bincode bytes)`, on both sides.
    fn head_metered<T: Serialize>(
        &self,
        _side: Side,
        entries: &[Ordered<'_, T>],
        meter: &ScanMeter,
    ) -> Result<Option<usize>, SpaceError> {
        let mut head: Option<Best<'_, '_, T>> = None;
        for (index, entry) in entries.iter().enumerate() {
            head = Some(match head {
                None => Best {
                    index,
                    entry,
                    bytes: None,
                },
                Some(best) => best.challenge(index, entry, meter)?,
            });
        }
        Ok(head.map(|best| best.index))
    }

    /// An incoming element joins the tail, so it is the head only of an empty side.
    fn reaches_head(&self, _side: Side, queued: usize) -> bool { queued == 0 }

    /// The least order interacts first, on both sides.
    fn precedes(
        &self,
        _side: Side,
        ord: i64,
        head: i64,
        meter: &ScanMeter,
    ) -> Result<bool, SpaceError> {
        meter.compare(size_of::<i64>(), size_of::<i64>())?;
        Ok(ord < head)
    }
```

**The order of an operation.** A fresh operation takes the value of its bump as its order.
A re-issued persistent operation keeps the order of its first round ([decision 43](decision-register.md#decision-43)).
`operation_ord` checks that a kept order lies below the bump value, because a bump value is never taken twice.
`at_head` then decides whether the element of the operation reaches the head of its side.

```text
OPERATION-ORD(kept, bumped):                   ▷ operation_ord
    if kept = none: return bumped
    require 0 ≤ kept < bumped                           ▷ Inconsistent
    return kept

AT-HEAD(side, kept, ord):                      ▷ Cx::at_head
    if kept = none: return the side holds no entry       ▷ a fresh element joins the tail
    h ← the order of the head of the side, or return true   ▷ head_ord: decodes, no copy
    require h ≠ ord                                     ▷ an element in flight is stored nowhere
    return PRECEDES(side, ord, h)                       ▷ a queue: ord < h
```

A fresh element joins the tail, so it reaches the head only of an empty side.
A re-issued persistent element re-enters at its place.
It meets the other side only when it precedes the head of its own side.
Otherwise it is stored or parked behind the head, and it fires nothing.

**The produce plan.** `offer_head` (`spaces/generic.rs:1723-1791`) is the plan of a send, as the default of `Discipline::offer`.
It is the OFFER of the substrate design ([S5.8](#s58-peek-re-offer-fr-10)), with the fire-round rule of A10 and of [I-21](decision-register.md#i-21).

```text
OFFER(op, ord):                                ▷ offer_head
    rec ← WRAP-DATUM(ord, op.data)
    if not AT-HEAD(Data, op.reissued_ord, ord): STORE(rec) and return no re-issue   ▷ its place
    src ← Produce::create(D, rec, op.persistent)
    rounds ← 0
    loop:
        W ← the head waiter, or STORE(rec) and return no re-issue
        require W has exactly one bind                          ▷ Inconsistent: a join group runs on the join engine (N12)
        m ← MATCH(W.pattern, op.data, W.continuation), or STORE(rec) and return no re-issue
        stage produce(D, rec, op.persistent, Fire([D], W, [INCOMING(D, src, rec, op.persistent, m)]))
        spent ← ¬op.persistent ∧ W.peeks = ∅
        stays ← W.persist
        case AFTER-FIRE(rounds, spent, stays) of
            Done:            return no re-issue
            Continue:        ▷ W left and the datum survives: the next round re-offers it
            Reissue(cause):  return Reissue(op with reissued_ord ← (ord if op.persistent), cause)
```

`MATCH` makes the metered spatial match of the pattern against the payload without its envelope, and then it checks the guard of the waiter (`spaces/matching.rs:107-121`).
A head waiter that does not match blocks every waiter behind it, and the datum is stored.
A peek waiter leaves the datum alive, so the next round re-offers the datum with the same produce identity ([decision 30](decision-register.md#decision-30)).
A persistent send `!!` fires the waiters one after another and stores the datum when no waiter matches.

**The consume plan.** `receive_head` (`spaces/generic.rs:1800-1884`) is the plan of a receive, as the default of `Discipline::receive`, and it mirrors OFFER.

```text
RECEIVE(op, ord):                              ▷ receive_head
    K ← WRAP-CONTINUATION(ord, op.body, op.guard, the cost authority of op)
    peeks ← {0} for a peek, ∅ otherwise
    if not AT-HEAD(Waiters, op.reissued_ord, ord): PARK(K, peeks) and return no re-issue
    rounds ← 0
    loop:
        (i, h) ← the head record and its view index, or PARK(K, peeks) and return no re-issue
        m ← MATCH(op.pattern, UNWRAP-DATUM(h).payload, K), or PARK(K, peeks) and return no re-issue
        stage consume([D], [op.pattern], K, op.persistent, peeks, Fire([BIND(D, h, m, i)]))
        if op.peek ∧ ¬h.persist: RE-STORE(h)      ▷ the COMM removed h (decision 45)
        spent ← ¬op.persistent
        stays ← h.persist ∨ op.peek
        case AFTER-FIRE(rounds, spent, stays) of
            Done:            return no re-issue
            Continue:        ▷ the head record left: a persistent receive meets the next head
            Reissue(cause):  return Reissue(op with reissued_ord ← (ord if op.persistent), cause)
```

A peek `<<-` keeps the head record by a [re-store](glossary.md#re-store) ([decision 45](decision-register.md#decision-45), Rule E).
Its COMM removes a non-persistent head record, as the host COMM does.
`RE-STORE` then stages a produce of the same record bytes with the outcome Store, right after the fire (`Cx::restore`).
So the record keeps its identity, its envelope and its order, and it stays the head.
The re-store registers the cost authority of the record (`Introduction::for_record`), and it bumps no counter.
A peek of a persistent record removes nothing, so it stages no re-store.

A persistent receive `<=` drains the head records one after another and parks when no head record matches.
This drain is the persistent drain of the queue.

**The fire rounds.** `FireRounds::after_fire` (`reified/costs.rs:322-333`) decides the next step after each fire:

```rust
    /// Records one fire and returns the next step (A10, readiness review S5).
    ///
    /// * `incoming_spent`: the fire used up the datum or the continuation of the operation. For
    ///   a produce, the datum is not persistent and the fired waiter does not peek. For a
    ///   consume, the continuation is not persistent.
    /// * `partner_stays`: the fired partner stays in the store. For a produce, the fired waiter
    ///   is persistent. For a consume, the fired record is persistent or the consume peeks.
    pub fn after_fire(&mut self, incoming_spent: bool, partner_stays: bool) -> FireStep {
        let Some(fired) = self.fired.checked_add(1) else {
            return FireStep::Reissue(ReissueCause::RoundCap);
        };
        self.fired = fired;
        match (incoming_spent, partner_stays) {
            (true, _) => FireStep::Done,
            (false, true) => FireStep::Reissue(ReissueCause::DivergentPair),
            (false, false) if fired >= FIRE_ROUND_CAP => FireStep::Reissue(ReissueCause::RoundCap),
            (false, false) => FireStep::Continue,
        }
    }
```

*Table N4.1. The step after a fire.*

| Incoming spent | Partner stays | Step | Example of a produce | Example of a consume |
|---|---|---|---|---|
| yes | any | `Done` | a send `!` fires a receive `<-` or `<=` | a receive `<-` or `<<-` fires with a record |
| no | yes | `Reissue(DivergentPair)` | a send `!!` fires a receive `<=` | a receive `<=` fires with a record of `!!` |
| no | no, fewer than 64 fires | `Continue` | a send `!` fires a peek `<<-`, or a send `!!` fires a receive `<-` | a receive `<=` fires with a record of `!` |
| no | no, 64 fires | `Reissue(RoundCap)` | the 64th round of a chain | the 64th round of a chain |

A divergent pair is a persistent send and a persistent receive that meet: neither side ever leaves.
The host reducer repeats such a pair forever with its re-produce. So the agent re-issues after one round, and each round is a new space intent (item A10).
The pair then stops only when the phlo of the deploy runs out, as on the baseline host.

A re-issued persistent element keeps its record, so every round repeats its identity ([decision 43](decision-register.md#decision-43)).
Its introduction is charged once in each deploy, as the host charges a persistent produce that it produces again.
A divergent pair repeats one COMM identity too, so each later round pays only its counter bump.

A chain without a divergent pair makes at most one more fire than the collection holds entries. The cap of [I-21](decision-register.md#i-21) also bounds one transaction at 64 fires.
So one operation makes at most $`2(\mathtt{FIRE\_ROUND\_CAP} + 1) = 130`$ metered matcher calls: two in each round, and one for the counter.

**The invariant.** Let $`R`$ be the records and $`W`$ the waiters of a collection at the end of a completed deploy.

```math
\mathrm{INV}(R, W) \iff R = \emptyset \;\lor\; W = \emptyset \;\lor\; \neg\, \mathrm{match}\bigl(\mathrm{head}(W),\ \mathrm{head}(R)\bigr)
```

A fresh OFFER meets a waiter only when $`R = \emptyset`$, and it stores the datum only when no head waiter matches.
A fresh RECEIVE meets a record only when $`W = \emptyset`$, and it parks only when no head record matches.
A re-issue stages neither the datum nor the continuation of the operation, so the element is in flight until it re-enters.
So a fresh operation keeps INV, which the headlines `queue_produce_preserves_inv` (`HeadOnlyDisciplines.v:928`) and `queue_consume_preserves_inv` (`HeadOnlyDisciplines.v:948`) prove.

Between two rounds, other operations can store younger entries on the side of an element in flight.
Then the two heads can match while the element is in flight.
Let $`F`$ be the persistent elements in flight, each with its kept order and its side $`X_f`$.
Every committed transaction keeps the shadow form of INV:

```math
\mathrm{SHADOW}(R, W, F) \iff \mathrm{INV}(R, W) \;\lor\; \exists f \in F.\ \mathit{ord}(f) < \mathit{ord}\bigl(\mathrm{head}(X_f)\bigr)
```

When $`f`$ re-enters, the space stores or parks it at the head of its side without a match.
Otherwise $`f`$ is in flight again, and it still precedes that head.
A completed deploy has no element in flight, so INV holds at the end of each deploy.

A datum that is not persistent keeps no order.
With a kept order, it could re-enter ahead of two matching heads, fire an older partner, and be spent.
Then the matching heads would stay with nothing in flight.

The headlines `queue_fifo` (`HeadOnlyDisciplines.v:1135`) and `queue_head_blocks` (`HeadOnlyDisciplines.v:1218`) prove the order of delivery and the blocking of a head.

**Outcome and dispatch.** `outcome` (`spaces/generic.rs:1991-2080`) turns the receipts into a `SpaceOutcome`.
It checks that each receipt has the kind of its staged operation, and it drops the counter fires.
It also drops a retirement and a lift, which P5 stages and never dispatches.
Each other fire gives its continuation without the envelope and the results of the substrate, in staging order.
A fire over `[D; n]` that binds one result keeps only the first result of its receipt (P5a, F3).

A replay of the same operation reads the same staged view, so it stages the same operations.
There, `verify` pairs each fire that play committed ([S5.3](#s53-verify-fr-3)).
A fire that play did not commit stays unpaired, and the replay commit stops at it with the error of play ([decision 36](decision-register.md#decision-36)).

*Table N4.2. The staged operations and their events. The bump is the operations 0 and 1.*

| Case | Staged operations after the bump | Events after the bump |
|---|---|---|
| a send stores | 2: Store the record | Produce |
| a send fires the head waiter | 2: Fire | Produce, COMM |
| a send fires a peek waiter, and the datum survives | 2: Fire, 3: Fire or Store (the re-offer) | Produce, COMM with peeks, then Produce with a COMM or alone |
| a send `!!` fires two waiters `<-` | 2: Fire, 3: Fire, 4: Store | Produce, COMM, Produce, COMM, Produce |
| a send `!!` fires a waiter `<=` | 2: Fire, then a re-issue | Produce, COMM, then a new space intent |
| a receive fires with the head record | 2: Fire | Consume, COMM |
| a peek fires with the head record | 2: Fire, 3: Store (the re-store) | Consume, COMM with peeks, Produce |
| a peek fires with a persistent head record | 2: Fire, the record stays | Consume, COMM with peeks |
| a receive parks | 2: Park | Consume |
| a receive `<=` drains two records | 2: Fire, 3: Fire, 4: Park | Consume, COMM, Consume, COMM, Consume |

The first operation on a collection stages three counter operations instead of two. They are the take and the put of the space counter, and the put of the collection counter.
Figure N4 shows the states of one queue collection.

![State machine of one queue collection. The collection starts unopened; its first operation consumes the space counter S(sid), produces it again, opens S(sid,k) = 1 and takes the order 0. Every later operation bumps S(sid,k) from v to v + 1 and takes the order v, except that a re-issued persistent operation keeps the order of its first round and re-enters at its place. The open collection has four states: Empty, Records queued, Waiters queued, and Blocked, in which both sides hold entries but the head waiter does not match the head record. A send in a state with records joins the tail. A send without records meets the head waiter: on no match it is stored; on a match it fires; a peek waiter or a persistent send leaves the datum alive and the next round re-offers it to the next head waiter; a persistent send that fires a persistent receive re-issues as a new space intent. A receive in a state with waiters joins the tail. A receive without waiters meets the head record: on no match it parks; on a match it fires; a peek removes a non-persistent head record in its COMM and stores the same record again in the same transaction (decision 45); a persistent receive drains the next head; a persistent receive that meets a persistent record re-issues. After 64 fires in one transaction the rest re-issues. A re-entry ahead of a head can fire in the Blocked state. The invariant INV holds at the end of every deploy: no pending match between the two heads. A second note gives the joins of P5 wave W3: a collection that a join waiter holds runs on the join engine, the join waiter fires only as the head waiter of each of its queue collections, m binds on one collection see its first m records, and the cascade fires a matching pair of heads that a fire exposes, in the same transaction.](diagrams/05-head-only-queue.svg)

*Figure N4. Source: [diagrams/05-head-only-queue.puml](diagrams/05-head-only-queue.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

### N5. Routing and scope

The reducer evaluates a channel, removes its bundle, and then routes it.
`classify_channel` (`reified/route.rs:126-160`) gives the channel one [routing class](glossary.md#routing-class), and `route` maps the class to a space:

```rust
/// Routes a channel (P4 design C.1).
///
/// | Rule | Class                                   | Route                                  |
/// |------|-----------------------------------------|----------------------------------------|
/// | R1   | `System`, `DefaultObject`, `Unforgeable` | `Default`                             |
/// | R2   | `Storage`                               | `Default`                              |
/// | R3   | `SpaceName`                             | its space and `key(c)`, for every context |
/// | R4   | `AgentTagged`                           | `Agent`                                |
/// | R5   | `Forged`                                | the context space and `key(c)`, or `Default` without a context |
///
/// The key is computed only for a space route, so a default route allocates nothing.
pub fn route(ctx: &SpaceCtx, channel: &Par) -> Result<Route, SpaceError> {
    match (classify_channel(channel), ctx) {
        (
            ChannelClass::System
            | ChannelClass::DefaultObject
            | ChannelClass::Unforgeable
            | ChannelClass::Storage,
            _,
        ) => Ok(Route::Default),
        (ChannelClass::SpaceName(space), _) => Ok(Route::Space {
            space,
            key: CollectionKey::of_channel(channel)?,
        }),
        (ChannelClass::AgentTagged, _) => Ok(Route::Agent),
        (ChannelClass::Forged, Some(target)) => Ok(Route::Space {
            space: target.native,
            key: CollectionKey::of_channel(channel)?,
        }),
        (ChannelClass::Forged, None) => Ok(Route::Default),
    }
}
```

Two more rules complete the routing:

- **R6.** A space operation builds its produce and consume identities on the data channel $`D(\mathit{sid}, k)`$. So the identities equal what reaches storage.
- **R7.** `RoutingExecution` (`reified/execution.rs:113-131`) routes the replies of the system processes, which bypass the reducer. It applies R1 to R4 with no use context, because a system process has no lexical context. A reply to a space name runs on its space, and its fires dispatch through the reducer.

A channel of an ordinary program routes to the default space, and the hooks then make the baseline call with identical arguments.
The headline `ordinary_channels_route_default` (`SpaceRefs.v:842`) and the regression suites of the [verification dossier](verification.md#8-regression-checks-for-ordinary-execution) check this property.
Mergeable-channel detection runs on the final routed channel, and it never classifies a Reified channel as mergeable (consensus checklist item 19).

Figure N5 shows the decision.

![Routing decision as an activity. The reducer removes the bundle of the channel and classifies it. A sole GPrivate with an id of 1 or 2 bytes is System. A decoded id of KIND C, H, X or B is DefaultObject. A decoded id of KIND D, I, A or S is Storage. A decoded id of KIND N is SpaceName with its native space. Any other sole unforgeable is Unforgeable. A tuple whose head is AGENT_TAG is AgentTagged. Every other channel is Forged. Rule R1 sends System, DefaultObject and Unforgeable to the default space, R2 sends Storage to the default space, R3 sends a space name to the collection key(c) of its own space in every context, R4 sends an agent-tagged name to the user agent, which fails closed with AgentSpacesUnavailable until P8, and R5 sends a forged channel to the collection key(c) of the use context, or to the default space without one. The default route makes the baseline call with identical arguments, and a space route builds the identity on D(sid, k) (R6) and runs one space intent. The R7 backend routes the replies of system processes with the same rules and no use context.](diagrams/12-routing-decision.svg)

*Figure N5. Source: [diagrams/12-routing-decision.puml](diagrams/12-routing-decision.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

**Space references.** `classify_space_ref` (`reified/scope.rs:141-180`) gives the value of `use s`, of `new x : s`, and of `getSpaceAgent!?(s)` one meaning, by the five rules of the [space reference](glossary.md#space-reference).
A native space needs a bare handle id or a writable bundle of one ([decision 34](decision-register.md#decision-34), item A22).
A user agent fails closed with `AgentSpacesUnavailable` until P8.

**The use context.** `use s { P }` evaluates `P` with the [use context](glossary.md#use-context) of the native space `s`.

```text
EVAL-USE(space, body, env, rand):              ▷ reducer_hooks.rs, eval_use and native_space_ref
    s ← SUBSTITUTE(EVAL(space, env))
    case CLASSIFY-SPACE-REF(s) of
        Native(n): child ← a copy of the reducer with space_ctx = (s, n)
                   child evaluates body with env and rand     ▷ rand is not split
        Agent(_):  fail AgentSpacesUnavailable                  ▷ P8
        error e:   fail e                                       ▷ EmptySpaceRef, HandleNotWritable, NotASpaceRef
```

The field `space_ctx` is immutable, and `with_metering_child` copies it into parallel branches.
A body that the reducer stores for later evaluation under a use context gets the USE tuple of the context around it (`reified/scope.rs:185-193`).
So a parked receive body runs in its context when it fires, as a lexical closure does.

The USE tuple is one term, so the reducer evaluates it with the random state of the body. So the wrapper changes no random state.
A data value that a program sends inside a `use` block gets no wrapper (item A24). A forged reply channel of a system process loses its use context (R7).

**NEWSPACE and new names in a context.** `new x : s in { P }` puts the name `x` into the space `s`.

```text
EVAL-NEWSPACE(annotations, new, env, rand):    ▷ reducer_hooks.rs, eval_new_space
    charge new_bindings_cost(new.bind_count)
    for each plain binder i:
        space_i ← NATIVE-SPACE-REF(annotation_i, env) when annotation_i ≠ [], else the use context or none
    env′ ← allocate_new_bindings(new, env, rand)              ▷ the random draws of the baseline
    for each plain binder i with space_i ≠ none:
        the name GPrivate(r_i) at level env.level + i becomes GPrivate(0xE0 ‖ N ‖ sid_i ‖ r_i)
    evaluate new.p with env′ and the rest of rand
```

The annotations evaluate in the environment outside the `new`, as the [syntax contract](syntax-contract.md#scopes-and-binding-order) states.
Inside a use context, a plain `new` without annotations also stamps every plain binder with the space of the context (`reified/reducer_hooks.rs:663-711`).
The stamp keeps the 32 random bytes of the baseline name as the tail, so the random draws do not change ([decision I-3](decision-register.md#i-3)).

**The send hook.** `reified_produce` (`reified/reducer_hooks.rs:429-447`) is hook H3, with the same signature as the baseline `produce`.

```text
REIFIED-PRODUCE(chan, data, persistent, α):    ▷ reducer_hooks.rs, reified_produce and route_send
    params ← TRAILING-PARAMS(data)                 ▷ the refresh of I-20, then the strict decoder: Malformed
    case ROUTE(space_ctx, chan) of
        Default:
            CHECK-PERSISTENT-SEND(chan, persistent)  ▷ item A21
            if chan is a factory op:   reject positional params, data ← PACK(data)
            else if chan is a handle:  reject positional params
            else if params ≠ none:     fail ParamsUnsupported
            produce(chan, data, persistent, α)         ▷ the baseline call
        Space(n, k):
            require params = none                      ▷ PositionalParamsUnsupported, UnknownParam
            SPACE-OP(Produce(n, k, chan, data, persistent, α))
        Agent:
            fail AgentSpacesUnavailable
```

`PACK` turns the arguments of a factory call into `[ret, [ctor…]]`, so every factory has the fixed arity 2 ([decision I-18](decision-register.md#i-18)).
A handle call keeps its trailing PARAMS tuple, which `HANDLE_OP` reads.
The queue discipline takes no send params, so a named param on a queue fails with `UnknownParam` and no suggestion.

**The receive hook.** `reified_consume` (`reified/reducer_hooks.rs:549-626`) takes the parameters of the baseline `consume`.

```text
REIFIED-CONSUME(binds, body, persistent, peek, guard, α, α_intro):
    if no use context and no bind channel is a space name or agent-tagged:
        consume(binds, body, …)                        ▷ the fast path: the baseline call
    body ← WRAP-BODY(space_ctx, body)
    routes ← [ROUTE(space_ctx, c) for each bind (pattern, c)]
    if every route is Default: consume(binds, body, …)  ▷ the identity holds the wrapped body
    if there is exactly one bind:
        Space(n, k) → SPACE-OP(Consume(n, k, c, pattern, body, persistent, peek, guard, α, α_intro))
        Agent      → fail AgentSpacesUnavailable
    if every route is Space(n, ·) of one space n:
        SPACE-OP(Join(n, the binds in bind order with their queries, body, persistent, peek, guard, α, α_intro))
    fail JOIN-ERROR(routes)    ▷ AgentSpacesUnavailable or CrossSpaceJoin
```

A join whose binds all route to one native space runs on the join engine since P5 wave W3 ([decision 29](decision-register.md#decision-29), [N12](#n12-wave-w3-of-p5-joins-of-one-space)).
Until then it failed with `OrderedJoinUnsupported`.
A join with a user agent fails with `AgentSpacesUnavailable`, and every other mixed join fails with `CrossSpaceJoin`.
The fast path classifies without an allocation, so an ordinary receive costs no extra work.

**The refresh of derived flags.** The evaluation and the substitution of the baseline keep a stale `connective_used` flag on a tuple, a list, or a map ([decision I-20](decision-register.md#i-20)).
So before each strict decode, the hooks compute again the flags that the encoder derives from the children, and they change no value (`reified/reducer_hooks.rs:1565-1585`).
Hook H4 refreshes a QUERY tuple the same way before it fails closed with `QuerySpecUnsupported`.

**Examples.** The first program puts a forged channel into a queue space with `use`.
The send and the receive on `@"jobs"` meet in the collection with the key of `@"jobs"`, by rule R5.

```rholang
new hmq(`rho:space:hashmap:queue`) in { for (space <- hmq!?("default")) { use space { @"jobs"!(1) | for (@x <- @"jobs") { Nil } } } }
```

The second program puts a new name into the space with an annotation.
The name `box` is a space name, so the send and the receive on `box` route to the space by rule R3, in every context.

```rholang
new hmq(`rho:space:hashmap:queue`) in { for (space <- hmq!?("default")) { new box : space in { box!(1) | for (@x <- box) { Nil } } } }
```

The test `space_queue_source_program` runs a longer program of this kind end to end ([source](../../../rholang/tests/reified_queue_source.rs#L35-L58)). That program has a handle call, a `use` block, and two receives in arrival order.

### N6. Scheduling

Every space operation runs as a [space intent](glossary.md#space-intent) of the deterministic scheduler, inside `operation_context::scope` of its order ([decision 34](decision-register.md#decision-34)).
Otherwise a proposer and a validator could order two operations on one space in two ways, and the replay of a head-only plan would diverge.
`submit_space` (`reified/schedule.rs:192-226`) submits one job:

```text
SUBMIT-SPACE(sid, α, channels, job):           ▷ reified/schedule.rs, submit_space
    ctx ← the reduction context of the task, or none
    hw ← the host-work budget of ctx, or none            ▷ captured before the job (readiness review S5)
    if ctx = none: return job(hw)                         ▷ outside a reduction: no order, as a host operation
    order ← ctx.next_operation()
    if the participant can claim direct execution:
        return job(hw) inside internal_scope and operation_context::scope(order)
    submit Intent::Space(sid, α, channels, job) at order
    poll the driver once, and await the reply          ▷ a cancelled reply: BugFoundError
```

The footprint of a space intent decides which intents the scheduler orders against it (`reified/schedule.rs:84-97`):

```math
F = \bigl\{[2] \mathbin{\Vert} \mathit{sid}\bigr\} \cup \bigl\{[0] \mathbin{\Vert} \mathrm{prost}(c) : c \in \mathit{channels}\bigr\} \cup \mathit{AuthorityKeys}(\alpha)
```

Host intents use the tag 0 for their channels and the tag 1 for their authority keys. So the tag 2 of a space key never equals a host key ([decision I-11](decision-register.md#i-11)).
The scheduler therefore orders the intents of one space, and it runs the intents of other spaces in parallel.
A creation adds the channel `this` of the new space, because a host send on the handle touches that channel.

**One task for each job.** `SpaceIntent::execute` (`reified/schedule.rs:110-144`) runs the job in its own task (readiness review S9).
The driver polls all its conflict components as futures of one task, and the substrate admits one open transaction for each task (FR-9).

Assume that two jobs share one task, and that one job waits for a stripe. Then the other job can fail with `NestedTransaction`, and that result depends on timing.
The unit test `space_intents_for_two_sids_in_one_frontier_run_in_separate_tasks` checks the rule.
A join error of the task is a `BugFoundError`.
The session registers the task in its registry of detached tasks while the task runs, as it registers a detached reduction (`deterministic_reduction.rs`, `spawn_detached_in_context`).
So when the root evaluation is cancelled, the session aborts the task with its detached reductions.

**One commit for each job.** A job opens at most one transaction and commits it exactly once.
A read, such as the config read of `getSpaceAgent`, stages nothing and drops its transaction without a commit (readiness review S3).
An empty commit would claim the parent order, and a later commit under the same order would fail with `ParentOrderReused`.
The job never awaits between `begin` and `commit`, except inside `begin`, and matching is synchronous.

**The evaluation scope.** `inj`, `inj_with_observation`, and `inj_with_host_work` call `begin_evaluation_scope` at their entry when no reduction context runs (`reified/runtime.rs:102-118`).
The three interpreter entries `inj_attempt`, `inj_attempt_with_native_phlo`, and `inj_attempt_with_host_work` reach the reducer through `inj_with_observation`.
The test `reducer_space_routing_inj_with_host_work_begins_an_evaluation_scope` runs one term twice through `inj_with_host_work` under the same operation orders.
In the host mode, the call resets the tracker of committed parent orders, so a deploy that runs again commits under the same orders (FR-5').
It makes no event, no charge, and no random draw.

**The sweep of a root evaluation (P7).** At the end of a root evaluation, the three entries sweep the volatile channels of its book under the boundary guard.
The rules are QF-8 and QF-9 of the [specification](specification.md#42-temp).

**Host work.** The job gets the host-work budget that `submit_space` captured, because `reserve_host_work` charges nothing inside the task of a job.
`ScanMeter` (`reified/costs.rs:48-262`) applies the rule of DR-88 of `feature/cost-accounted-rho` ([decision 42](decision-register.md#decision-42)).
A reservation has three amounts, and each amount charges one host-work dimension:

- The operations charge VerificationOperations.
- The scanned bytes charge VerificationBytes.
- The allocated bytes charge SearchStateBytes.

The metered matcher reserves its own reads on the meter, and the agent adds no charge for a matcher call.
The agent reserves each of its own reads and copies before it makes it:

- the ids of the channels and the decode of the config
- each envelope decode and each comparison of a head scan, and the copy of the head
- the copies of each staged operation
The agent prepays at staging the work of the outcome after the commit.
So a host-work rejection always happens before the commit, and the operation has no effect (`AgentReadCoverage.v`, `cut_rejects_before_commit`).
Without a budget, the meter accepts every charge and skips every walk, as `reserve_host_work` does ([decision I-12](decision-register.md#i-12)).

**The runtime of a reducer.** `ReifiedRuntime` (`reified/runtime.rs:78-84`) has three modes. The host mode and a bound native replay mode run space jobs.

*Table N6.1. The modes of the runtime.*

| Mode | Made by | A space operation |
|---|---|---|
| `Host` | `for_host`, at the start of `create_rho_env` | runs one space intent over the handle of the host space, in legacy execution and in native play ([decision 47](decision-register.md#decision-47)) |
| `Detached` | `unavailable`, in `ReducerCore::new` and `DebruijnInterpreter::new` | fails with `RuntimeUnavailable` |
| `NativeReplay` | `native_replay`, in the native replay environment, which binds it to the store of its native replay session with `bind_native_replay` | runs one space intent over the native backend of the session once bound, and fails with `RuntimeUnavailable` before ([decision 47](decision-register.md#decision-47)) |

Every space entry point checks the runtime first (`ReifiedRuntime::host` and `ensure_available`, `reified/runtime.rs:194-211, 215`).
`run_op`, `create_space`, and `read_config` check it before `submit_space`, so a failing check takes no operation order and makes no observer call.
Native play and a bound native replay runtime run the same space entry, so native replay reproduces play (`SpaceRefs.v`, `native_execution_runs_spaces`).
[Decision 46](decision-register.md#decision-46) made both modes reject a space deploy before any space work, and decision 47 replaces that rule.
The runtime holds no cache, so nothing in it survives a reset.
The host runtime holds one `SpaceStoreHandle` for the host, which the substrate requires ([S10](#s10-limits)).

**The dispatch of fires.** `dispatch_space_outcome` (`reified/reducer_hooks.rs:856-912`) runs the continuations that a space operation fired.

```text
DISPATCH-SPACE-OUTCOME(fires, reissue, then):  ▷ reducer_hooks.rs, dispatch_space_outcome
    if fires = [] and reissue = none:
        if then = none: return Skip
        return then()                             ▷ the work of the caller runs directly
    replay ← the host space is a replay space
    if fires = [f] and reissue = none and then = none:
        return DISPATCH(f.continuation, f.data, replay)
    branches ← [a metering child of index i dispatches fire i, for each fire i]
    if reissue = r: branches ← branches ++ [a metering child runs SPACE-OP(r.op, none)]
    if then ≠ none: branches ← branches ++ [then]  ▷ the last branch
    run the branches in parallel inside a reduction, and in index order outside one
```

The data of a fire is the channel, the matched datum without its envelope, the removed record, and the persist flag of each result.
The substrate keeps a peeked record in place, so no dispatch produces a datum again.

**A dispatch ends its participant.** The reducer of `feature/cost-accounted-rho` evaluates a fired body in a detached task (`reduce.rs`, `eval_continuation`, from commit `c3aacd649`).
In a reduction, `eval_continuation` splits one child from the participant of the dispatch, so the session removes that participant.
That participant then cannot make another operation.

A parallel branch (`run_parallel_dispatches`) also detaches, and its errors reach the root of the evaluation in the order of the branches.
For this reason, `then` is the work that the caller makes after the operation, for example the reply of a handle method.
`then` is a branch of the outcome, so it runs in a participant of its own.
The test `reducer_space_routing_then_runs_after_the_fires_in_its_own_branch` makes an operation in `then` after one fire and after two fires.
Without the branch, that operation fails with the panic `reduction participant must be registered`.

### N7. Factories and handles

**The factory call.** A call `F!(ret, ctor)` on the factory of a family creates a native space (`reified/factory.rs:75-121`).
The send hook packs the arguments of the program after `ret` into the constructor list `ctor`.

```text
CREATE-SPACE(call, ret, ctor):                 ▷ reified/factory.rs, FactoryProcess::call
    r ← the random state of the call datum, α ← its cost authority
    c ← VALIDATE(family, ctor)                            ▷ the gates of Table N7.1
    sid ← next(split_SpaceId(split_0xE0(r)))              ▷ I-19
    run one space intent with the footprint [2] ‖ sid and H(sid):
        txn ← begin([C(sid), S(sid), H(sid)])
        require txn.data(C(sid)) = []                     ▷ SpaceAlreadyExists
        require the config of c names sid and the family  ▷ Inconsistent, FamilyUnavailable
        stage produce(C(sid), config record, persist = true, Store)
        stage produce(S(sid), counter record 0, persist = false, Store)
        stage consume([H(sid)], [[...@args]], the handle continuation, persist = true, Park)
        txn.commit()
    produce bundle+{H(sid)} on ret
```

The space id is a 32-byte draw of the random state of the call ([decision I-19](decision-register.md#i-19)):

```math
\mathit{sid} = \mathrm{next}\left(\mathrm{split}_{\mathtt{SPACE\_ID}}\left(\mathrm{split}_{\mathtt{0xE0}}\left(r_{\mathit{call}}\right)\right)\right)
```

The random state of a call message is unique, so the id is unique to the call, and play and replay derive the same id.
A second creation of one id fails with `SpaceAlreadyExists` in play and in replay alike.
`FactoryConstructor::validate` (`spaces/factory/constructor.rs:87-178`) applies the gates of Table N7.1 in order, so the first failing gate gives the code.

*Table N7.1. The gates of a factory constructor in P4.*

| Gate | Condition | Code |
|---|---|---|
| 1 | The family has a schema in this phase. | `FamilyUnavailable` |
| 2 | The constructor is the canonical sequence `[qualifier?][FREE?][PARAMS?]`. | `BadConstructorArgument` |
| 3 | The family takes the qualifier: HashSet takes only `seq`. | `BadConstructorArgument` |
| 4 | The qualifier is `default`. `temp` and `seq` wait for P7. | `QualifierUnavailable` |
| 5 | The constructor names no theory. `free Th()` waits for P7. | `TheoryUnavailable` |
| 6 | The params are named. | `PositionalParamsUnsupported` |
| 7 | Each named param is in the schema of the family, in bytewise order of the keys. The schema of `hashmap:queue` is empty. | `UnknownParam`, with a suggestion |

*Since P7, gate 4 accepts `temp` and `seq` in native execution, and gate 5 accepts a builtin theory without arguments ([specification 5](specification.md#5-theories), rule TH-4).
Gate 5 then fails with `UnknownTheory` or `BadConstructorArgument`.*

The Levenshtein distance of two words is the least number of insertions, deletions, and substitutions of one character that change one word into the other. A suggestion is the candidate at the least distance, when that distance is at most 2 (`spaces/factory/defaults.rs:268-277`). A tie goes to the bytewise least candidate.

**The handle.** The handle continuation is a persistent single-channel receive on `this` $`= H(\mathit{sid})`$ (`reified/factory.rs:126-160`).
Its pattern `[...@args]` binds the whole call as one list, and its body `[0xE0, 0x30]!(this, args)` forwards the call to `HANDLE_OP`.
It has no guard and no cost authority, so each call is paid by its caller.
It is a single-channel receive, so it adds no pre-state join, and handle calls merge like contract calls (consensus checklist item 18, [decision I-16](decision-register.md#i-16)).

**`HANDLE_OP`.** The internal operation `HANDLE_OP` on the op channel $`[\mathtt{0xE0}, \mathtt{0x30}]`$ runs each call (`reified/handle.rs:221-371`).

```text
HANDLE-OP(this, message):                      ▷ reified/handle.rs, HandleProcess::call and run
    if this is not a bare handle id of 34 bytes: drop the call   ▷ item A6, no reply, no further event
    config ← READ-CONFIG(sid) in a read-only space intent        ▷ DanglingHandle aborts
    case PARSE(message) of
        Dropped:             drop the call                        ▷ no reply name or no method name
        Malformed(ret, m):   reply MalformedMessage on ret
        Message(ret, m, args, params):
            require params are named                             ▷ PositionalParamsUnsupported aborts
            require m names a method, else reply UnknownMethod with a suggestion
            require |args| = arity(m), else reply BadArgument("arguments")
            require each named param is in the schema of m, else reply UnknownParam
            if m = produce and its arguments pass:
                SPACE-OP(op, then = reply Nil on ret)               ▷ the reply is the last branch
            else: reply on ret with the result of the method m
```

A program holds only the write bundle of the handle, which it cannot open. So only the forwarding receive of the space passes the bare id (item A6).
The [agent API](agent-api.md#5-handle-methods) gives the methods, their replies, and every typed reply.

The method `produce` runs one collection operation with the random state and the cost authority of the call (item A16).
Then it dispatches the fires of the operation, and it replies `Nil` on `ret` as the last branch of the outcome.
The reply is a branch because a dispatch ends the participant that makes it (see N6, the dispatch of fires).
The reply and the fired bodies are parallel processes, as in the equivalent Rholang program `ch!(msg) | ret!(Nil)` after the commit.

**Persistent sends.** The reducer never cancels the re-produce of a persistent send, so a call with `!!` could repeat forever.
So a persistent send to an op channel that takes calls, or to a handle, aborts the deploy before the call ([decision 32](decision-register.md#decision-32), item A21).
P8 adds the matcher and body capabilities of user agents to the rule (P8 design v2, item A13):

```rust
/// A persistent send (`!!`) aborts on every op that takes calls and on every handle (A21),
/// and on every matcher and body capability of a translated receive (P8, design v2 A13). The
/// runtime never cancels the re-produce of a persistent send, so a call could repeat forever.
/// `channel` is the channel after the unbundling of `eval_send`.
pub fn check_persistent_send(channel: &Par, persistent: bool) -> Result<(), ReifiedReduceError> {
    if !persistent {
        return Ok(());
    }
    match namespace::reified_id_of(channel) {
        Some(ReifiedId::Op(op)) if is_factory_op(op) => {
            Err(ReifiedReduceError::PersistentFactoryCall)
        }
        Some(ReifiedId::Op(op)) if is_call_op(op) => Err(ReifiedReduceError::PersistentOpCall),
        Some(ReifiedId::Handle(_)) => Err(ReifiedReduceError::PersistentHandleCall),
        Some(ReifiedId::Matcher(..) | ReifiedId::Body(..)) => {
            Err(ReifiedReduceError::PersistentCapabilityCall)
        }
        _ => Ok(()),
    }
}
```

The factory ops are `0x00` to `0x20`, and the other ops that take calls are `0x21` to `0x25` and `0x30` to `0x32` (`reified/system_channels.rs:104`, `108`).

**`getSpaceAgent` and `assertEqual`.** `getSpaceAgent!(ret, s)` classifies `s` as a space reference (`reified/system_processes.rs:259-297`).
For a native space, it reads the config record in a read-only space intent. It then replies with the capability of the factory of the family ([decision 9.4](decision-register.md#decision-9-4)).
That capability equals the value that the URN of the factory binds, so the FIPS check `getSpaceAgent(space_1) == HMB` holds.
`assertEqual!(ret, a, b)` replies `true` when the sorted values of `a` and `b` are equal, and `false` otherwise ([decision 33](decision-register.md#decision-33)).

Figure N7 shows the lifecycle of a native handle, from the factory call to a reply.

![Lifecycle of a native handle as a sequence. A deploy sends the factory call hmq!(*ret) with its constructor. The reducer routes the op channel [0xE0, 0x01] to the default space, checks the persistent flag, packs the constructor and produces on the op channel. The COMM with the installed factory runs FactoryProcess::call: it validates the constructor and derives sid from the random state of the call. A space intent with the footprint [2] ‖ sid and H(sid) runs one creation transaction: begin on C(sid), S(sid) and H(sid), a check for SpaceAlreadyExists, the config record stored persistently on C(sid), the space counter S(sid) = 0, and the handle continuation parked persistently on H(sid); the commit logs the events. The factory replies bundle+{H(sid)} on ret. Later the deploy calls h!(*ack, "produce", "jobs", [1]): the reducer removes the write bundle, routes H(sid) to the default space and produces on it; the COMM with the persistent handle continuation runs its body, which produces (this, args) on HANDLE_OP. HANDLE_OP checks the bare handle id, reads C(sid) in a read-only space intent that commits nothing, parses the message, and runs the method produce as a collection operation in its own space intent: the counter bump and the head-only offer. It dispatches the fires and replies Nil on ack. A protocol error replies a typed error ("rho:space:error", code, detail) instead, and a missing config record aborts with DanglingHandle.](diagrams/03-native-handle-lifecycle.svg)

*Figure N7. Source: [diagrams/03-native-handle-lifecycle.puml](diagrams/03-native-handle-lifecycle.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

### N8. Errors

A Reified failure is either an abort of the deploy or a [typed reply](glossary.md#typed-reply).
An abort is the error `InterpreterError::ReduceError` with the text `reified:<Code>: <message>`, which the soft checkpoint of the deploy then reverts.
A host error keeps its own class: `SpaceError::Host` converts with `InterpreterError::from`, so `OutOfPhlogistonsError` and `HostWorkRejected` stay what they are (`spaces/errors.rs:294-301`).
The message text is not consensus state, but the code names the failure for tests and for users.

The substrate errors map explicitly, never through the text of `RSpaceError` (readiness review S8, `spaces/errors.rs:280-288`):

- `SpaceTxnError::Host` becomes `SpaceError::Host`, which keeps the class of the host error.
- `SpaceTxnError::ReplayDivergence` becomes `SpaceError::ReplayDivergence`, which keeps its own code.
- Every other variant becomes `SpaceError::Substrate`, for example `UnlockedChannel`, `NestedTransaction`, `ParentOrderReused`, and the 32-bit `CounterOverflow` of the produce counter.

*Table N8.1. The abort codes of the reducer hooks (`reified/errors.rs:15-62`).*

| Code | Trigger |
|---|---|
| `Malformed` | A marker tuple is not the exact canonical image of its encoder, after the refresh of [I-20](decision-register.md#i-20). |
| `RuntimeUnavailable` | The P2 form of the code, for a USE or NEWSPACE tuple without the runtime. P4 disables the variant: the code keeps it as a comment with its reason (`reified/errors.rs:18-20`), because no code builds it since item A19. A space operation in a detached runtime raises the space code `RuntimeUnavailable` of Table N8.2 instead. |
| `ParamsUnsupported` | A send with a trailing PARAMS tuple to a channel that takes no params. |
| `QuerySpecUnsupported` | A receive with a QUERY source, until P5. |
| `PersistentFactoryCall` | A persistent send to a factory op, `0x00` to `0x20`. |
| `PersistentHandleCall` | A persistent send to a handle. |
| `PersistentOpCall` | A persistent send to another op that takes calls. |
| `PersistentCapabilityCall` | A persistent send to a matcher capability `X` or a body capability `B` of a user agent (P8, item A13). |
| `AgentSpacesUnavailable` | A space reference that names a user agent, a send or receive on an agent-tagged name, or a `consume` call with user-agent matchers, until P8. |
| `CrossSpaceJoin` | A join whose binds route to more than one space, or to a space and the default space. |
| `OrderedJoinUnsupported` | Retired in P5 wave W3 ([decision 29](decision-register.md#decision-29)). A join whose binds route to one native space runs on the join engine (N12), and the code stays commented out in `reified/errors.rs`. |
| `EmptySpaceRef`, `HandleNotWritable`, `NotASpaceRef` | A space reference of `use` or of `new x : s` that names no space. Since P8, `HandleNotWritable` also covers the reference of a user agent under a bundle without the write flag, which a call cannot reach (item AG-4). |

*Table N8.2. The abort codes of the space operations (`spaces/errors.rs:20-193`).*

| Code | Trigger |
|---|---|
| `SpaceAlreadyExists` | The creation finds a config record on $`C(\mathit{sid})`$. |
| `BadConstructorArgument` | The constructor is not canonical, or the family does not take the qualifier. |
| `UnknownParam` | A named param outside the schema: on a factory, on a send, or in the query of a receive. |
| `InvalidParam`, `MissingParam` | A param value that the schema does not accept, or a required param that is missing (from P5). |
| `QuerySpecUnsupported` | A receive with a query on a discipline that takes no query params. |
| `CellOccupied` | A store or a park on a side that already holds the capacity of its discipline (F15, from P5). |
| `OutOfNames`, `NotAnIndexName` | An allocation past the size of a fixed Array space, or a channel of an Array or Vector space that is not its own index name (P5d). |
| `UnsupportedPathElement`, `PathTooDeep` | A path element that holds a PathMap or a zipper, or a path of more than 64 elements (P5e). |
| `PositionalParamsUnsupported` | Positional params on a factory, on a handle call, or on a space channel. |
| `QualifierUnavailable` | The `seq` qualifier in legacy execution, because its rules need the job order of native execution (P7, design v3 QT-5). Before P7, also the `temp` and `seq` qualifiers. |
| `TheoryUnavailable` | A config whose theory identity names no checker of this build (P7, [specification 5](specification.md#5-theories), rule TH-3). Before P7, every theory. |
| `UnknownTheory` | A factory call whose theory name is not a builtin, with the nearest builtin within two edits (P7, rule TH-4). |
| `TheoryRefuted`, `TheoryUndetermined` | A value of a send that the theory of the space refutes or cannot decide, before any staging (P7, rule TH-5). |
| `NestedCommitGuard` | A transaction with two or more theory leases, before its commit (P7, rule TH-9). |
| `FamilyUnavailable` | A family that this phase does not register or run. |
| `RuntimeUnavailable` | A space operation in a detached runtime, or in a native replay runtime that its environment has not bound yet. Decision 47 retires `SpacesUnsupportedInNativeExecution`, the code of [decision 46](decision-register.md#decision-46) for native play and native replay. |
| `DanglingHandle`, `DanglingName` | A space without its config record, reached through a handle or through a space name. From P7, also an id of a temp or seq space of an ended deploy. The message names the space id. |
| `SeqSent` | A send whose data holds a name of a seq space of the evaluation (P7, [specification 4.3](specification.md#43-seq), rule QF-14). |
| `SeqConcurrent` | A use of a channel of a seq space by an operation that does not descend from its last use (P7, rule QF-15). |
| `PayloadNotEmpty` | A send with a value to a collection of a HashSet space, before any staging (P7, [specification 2.7](specification.md#27-hashset), rule HS-2). |
| `PatternNotEmpty` | A bind with a pattern, a binder or a remainder on a collection of a HashSet space, before any staging (P7, rule HS-3). |
| `CounterOverflow` | An `ord` counter at `i64::MAX`. |
| `Inconsistent` | A malformed record, a second counter or config record, a persistent counter record, a collection in a join, a fire group that does not line up, or a receipt that differs from its staged operation. |
| `ReplayDivergence` | A fire without a free recorded occurrence of its COMM in replay. |
| `Substrate` | Every other failure of the substrate. |

The typed replies are not aborts, and the [agent API](agent-api.md#7-typed-replies) lists their codes and their detail maps.

### N9. Limits of P4

- **Fail-closed constructs.** P4 rejects the constructs that later phases add, with a deterministic code ([decision 34](decision-register.md#decision-34), item A9). The [syntax contract](syntax-contract.md#meaning-at-run-time) lists each construct with its code and its phase.
- **One family.** Only `hashmap:queue` has a factory. A config record of another family fails with `FamilyUnavailable`, and P5 and P6 add the other families.
- **Conservative merges.** Two branches that only peek one queue conflict (item A2). Two branches that open collections of one space conflict on the space counter (item A25).
- **Divergent pairs.** A persistent send and a persistent receive on one queue fire again in each new space intent. They stop only when the phlo of the deploy runs out, as on the baseline host.
- **Host work.** The meter charges the matcher work and the own reads and copies of each operation ([decision 42](decision-register.md#decision-42)). The production evaluation passes no host-work budget yet. Each charge then takes effect when cost accounting enables the budget.
- **Matching under the stripes.** The agent matches while its transaction holds its lock stripes, as the host does under its own locks.
- **Closures.** A data value that a program sends inside a `use` block gets no wrapper (item A24). A forged reply channel of a system process loses its use context, so a reply channel should be a `new` name (R7).
- **Key size.** A data channel holds the collection key, so the id of a data channel grows with the size of the channel.

### N10. Legacy charges

A legacy deploy charges phlo through `ChargingRSpace` (`rholang/src/rust/interpreter/storage/charging_rspace.rs:79-151`).
Before each host call it charges the storage of the record or the continuation.
After the call, `handle_result` refunds the storage that a fire removes and charges the event fees (`rholang/src/rust/interpreter/storage/charging_rspace.rs:298-381`).
A space transaction reaches the host store through the substrate, not through `ChargingRSpace`.
So the agent makes these [legacy charges](glossary.md#legacy-charge) itself ([decision 45](decision-register.md#decision-45), Option P).
Each staged operation pays what `ChargingRSpace` charges for its equivalent host call.

**The cost functions.** The charges use four functions of `accounting/costs.rs`.
Let $`|x|`$ be the protobuf encoded length of $`x`$, and let $`n`$ be a number of channels.
$`\mathrm{SP}`$ is the storage of a record $`d`$ on the channel $`c`$.
$`\mathrm{SC}`$ is the storage of a continuation $`k`$ on the channels $`cs`$ with the patterns $`ps`$.
$`E`$ and $`\mathit{CE}`$ are the event fees:

```math
\begin{aligned}
\mathrm{SP}(c, d) &= |c| + \sum_{p \in \mathrm{pars}(d)} |p| \\
\mathrm{SC}(cs, ps, k) &= \sum_{c \in cs} |c| + \sum_{p \in ps} |p| + |\mathrm{body}(k)| \\
E(n) &= 32 + 32\,n \\
\mathit{CE}(n) &= E(n) + n \cdot E(1) = 32 + 96\,n
\end{aligned}
```

Let $`\mathrm{rs}(x)`$ be the random state of a record $`x`$, and $`\mathrm{id}(k)`$ the id bytes of a continuation: the random state of its body.
Table N10.1 gives the charges of each staged shape, as `produce_charges` (`rholang/src/rust/interpreter/reified/legacy_charges.rs:59`) and `consume_charges` (`rholang/src/rust/interpreter/reified/legacy_charges.rs:88`) compute them.
The post-charges keep the order of `handle_result`, and each refund is one entry, also when it is zero.

*Table N10.1. The legacy charges of each staged shape.*

| Staged shape | Pre-charge | Post-charges, in order |
|---|---|---|
| Store of the record $`d`$ on $`c`$ | $`\mathrm{SP}(c, d)`$ | $`E(1)`$ |
| Park of $`k`$ on $`cs`$ | $`\mathrm{SC}(cs, ps, k)`$ | $`E(\lvert cs \rvert)`$ |
| Produce of $`d`$ on $`c`$ that fires the waiter $`W`$ | $`\mathrm{SP}(c, d)`$ | The refund of $`\mathrm{SC}(W)`$ when $`W`$ is not persistent or $`\mathrm{id}(W) = \mathrm{rs}(d)`$, else 0. The refund of the sum of $`\mathrm{SP}(c_i, r_i)`$ over the bound records $`r_i`$ that are not persistent or that have $`\mathrm{rs}(r_i) = \mathrm{rs}(d)`$. Then $`E(1)`$ when $`d`$ is not persistent, and $`\mathit{CE}(\lvert W.cs \rvert)`$. |
| Consume of $`k`$ on $`cs`$ that fires | $`\mathrm{SC}(cs, ps, k)`$ | The refund of $`\mathrm{SC}(cs, ps, k)`$. The refund of the sum of $`\mathrm{SP}(c_i, r_i)`$ over the bound records that are not persistent or that have $`\mathrm{rs}(r_i) = \mathrm{id}(k)`$. Then $`E(\lvert cs \rvert)`$ when $`k`$ is not persistent, and $`\mathit{CE}(\lvert cs \rvert)`$. |

The incoming datum of a produce fire is always a refunded record, because its random state is $`\mathrm{rs}(d)`$.
The persist flag of a bound record is the flag of the host's `wrap_result`, which the receipt keeps since Rule E.
So the charges that the agent computes when it stages an operation equal the charges of `ChargingRSpace` on the result of the host call.

**The chain.** The commit runs the callback `before_observe` of each staged operation before the observer calls of that operation.
`LegacyChain` (`rholang/src/rust/interpreter/reified/legacy_charges.rs:211`) puts the charges of each operation into these callbacks:

```text
CHAIN-NEW(budget):                              ▷ LegacyChain::new
    active ← the budget is legacy and metered
    carry ← []

CHAIN-STAGE(op):                                ▷ LegacyChain::produce, LegacyChain::consume
    if active:
        (pre, post) ← CHARGES(op)               ▷ Table N10.1
        carried ← carry
        carry ← post
        callback(op) ← [charge each c of carried; the registration of op; charge pre]
    stage op in the transaction

CHAIN-FINISH():                                 ▷ LegacyChain::finish, after the commit
    if active: charge each c of carry
```

The host program charges, for each operation $`i`$, the registration, then $`\mathrm{pre}_i`$, then the operation, then $`\mathrm{post}_i`$.
The chain charges, in the callback of operation $`i`$, first $`\mathrm{post}_{i-1}`$, then the registration, then $`\mathrm{pre}_i`$, and the tail charges $`\mathrm{post}_{n-1}`$.
So both runs make the same charges in the same order.
The legacy log of a space job is the log of its host program.
The callback of operation $`i`$ wraps the registration of the introduction (`rholang/src/rust/interpreter/reified/legacy_charges.rs:262-279`):

```rust
    fn attach(
        &mut self,
        budget: RuntimeBudget,
        charges: LegacyCharges,
        registration: Option<BeforeObserve>,
    ) -> BeforeObserve {
        let LegacyCharges { pre, post } = charges;
        let carried = std::mem::replace(&mut self.carry, post);
        Box::new(move |source: RSpaceOperationSource<'_>| {
            for charge in carried {
                budget.charge_legacy(charge).map_err(RSpaceError::from)?;
            }
            if let Some(registration) = registration {
                registration(source)?;
            }
            budget.charge_legacy(pre).map_err(RSpaceError::from)
        })
    }
```

Every staged operation of a space job goes through the chain.
This covers the counter bump (`counter::bump`), the plans of the queue, the [re-store](glossary.md#re-store), and the operations that create a space.
The finish step of each prepared job calls `LegacyChain::finish` after the commit and before the outcome (`rholang/src/rust/interpreter/spaces/generic.rs:2695`, `2756`, `3005`).

**Out of phlo.** A charge that runs out of phlo stops the run where the host program stops.
A carried post-charge or a pre-charge fails inside the callback of its operation.
That operation then makes no observer call, and the operations before it stay applied (D6).
The host program fails at the same charge: after the host call of the operation before, or before the host call of the operation.

A tail that runs out of phlo fails after the full commit, as the post-charges of the last host call fail after that call.
Replay stages the same operations and runs the same callbacks, so it makes the same charges and stops at the same point ([decision 36](decision-register.md#decision-36)).
Figure N10 shows the two runs and a stop at a pre-charge.

![Legacy charges of a space transaction as a sequence diagram with three parts. In the host program, the reducer registers the authority of operation 0, ChargingRSpace charges pre 0, runs the RSpace operation, and charges post 0 with the refunds and the event fees, and then the same for operation 1. In the space transaction, the agent stages operation 0 through LegacyChain, which makes callback 0 of the registration and pre 0 and carries post 0, and stages operation 1, whose callback charges post 0, the registration and pre 1 and carries post 1. The commit runs callback 0, operation 0, callback 1 and operation 1, and finish charges the tail post 1 after the commit, so both runs make one log: pre 0, post 0, pre 1, post 1. In the last part, callback 1 runs out of phlo at pre 1: the commit fails with OutOfPhlogistonsError, operation 0 stays applied and operation 1 makes no observer call, which is where the host program stops.](diagrams/17-legacy-charge-chain.svg)

*Figure N10. Source: [diagrams/17-legacy-charge-chain.puml](diagrams/17-legacy-charge-chain.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

**Verification.** The Rocq module `LegacyCharges.v` proves that the space run makes the legacy log, the stop point, and the applied operations of the host program (Q79).
It also proves that replay stops where play stops (Q80).
The invariant U13 `LegacyChargesFollowHost` of `CheckpointReplay.tla` checks the same claim in a model of play and replay with peeks.
Section [11.14](verification.md#1114-legacy-charges-of-decision-45) of the verification dossier lists these results, the tests, and the negative controls.

**Scope and limits.**

- The chain is active only in a legacy deploy with a metered budget (`LegacyChain::new`). In every other deploy the accounting observer charges the space operations ([M-3](decision-register.md#m-3)).
- A legacy deploy has no host-work budget, so the encoded-length walks of the charges reserve no host work, as the walks of `ChargingRSpace` reserve none.
- In a legacy deploy, the reducer runs host operations outside the deterministic scheduler. So the charges of parallel branches follow the schedule (design report of decision 45, D.4). A space job is sequential inside, so its charges keep their order.
- If an operation fails a read-only check of the commit, the post-charges of the operation before it are not made. Only a defect or a produce counter at `i32::MAX` fails such a check.
- The refund sum uses a checked addition, where `ChargingRSpace` adds without a check.
- A continuation without a body fails closed with `Inconsistent`, where `ChargingRSpace` panics.

### N11. Wave W2 of P5: the drivers, the stores and the hooks

Wave W2 adds the bag, the cell and the stack, and the Array and Vector stores with their allocator.
It also adds the pure PathMap layer, the P5 factory rows, and the hooks of receive queries and allocation jobs.
Every new family runs on the engine of S1. A discipline and an outer store plug into `GenericSpace`, and they stage only through `Cx`.

#### N11.1 Family dispatch and registration

`Family` lists the wired families, and `FamilyAgent` runs the agent of the family that the config of the space names (`spaces/families.rs:186-251`, `spaces/families.rs:540-549`).
The dispatch tries the arms in the order of `Family::WIRED`: `hashmap:queue` first, then ascending family index, then the PathMap families of P5e.
The VectorDB families of P6 and the HashSet families of P7 follow them.
Each arm that it tries reserves one comparison of the names of the config and of the arm (`spaces/families.rs:344-354`).
So the arm at the position $`i`$ reserves $`i + 1`$ comparisons, and a queue operation keeps the host work of P4.

At the end of P5, twenty-four families are wired.
They are the bag, queue, stack, set, cell and priority queue families of four stores: `hashmap`, `pathmap`, `array` and `vector`.
The PathMap arms come last, so the dispatch of each family that was wired before them reserves the comparisons that it reserved before.

The registration reads the family dispatch (`spaces/factory/urn.rs:203`).
A factory row is registered exactly when its family has an arm.
So no manual step can register a family without an agent or forget a wired family.

`definitions` installs the four P4 processes, then the factory of each registered P5 family in family-index order, then the factory of each registered alias (`reified/system_processes.rs:101-172`).
The VectorDB factories and the processes of P6 come next, and the HashSet factories of P7 come last.
An alias has the channel, the arity, the body reference and the handler of its family. So the second install of a shared channel changes nothing.

```text
VALIDATE(family, ctor):                          ▷ factory/constructor.rs, FactoryConstructor::validate
  schema, required ← SCHEMA(family)              ▷ none: FamilyUnavailable
  q, theory, params ← PARSE(ctor)                ▷ BadConstructorArgument
  check the qualifier                            ▷ the gates 3 and 4
  identity ← THEORY_GATE(theory)                 ▷ gate 5 (P7): UnknownTheory, BadConstructorArgument
  if params.positional ≠ []: PositionalParamsUnsupported
  for k in keys(params.named), bytewise: if k ∉ schema: UnknownParam(k, SUGGEST(k, schema))
  for k in required: if k ∉ keys(params.named): MissingParam(k)
  for (k, v) in params.named, bytewise: v′ ← RULE(k).canonical(v), else InvalidParam(k)
  return (family, q, identity, {k ↦ v′})
SUGGEST(k, schema):                              ▷ factory/defaults.rs, suggestion
  the least (distance(k, c), c) over c ∈ schema with distance ≤ 2, else none
```

#### N11.2 The scan driver

The scan driver is the one implementation of the plans of the unordered disciplines (`spaces/disciplines/scan.rs:179-218`, `spaces/disciplines/scan.rs:229-321`).
The bag uses it now, and the set of wave W3 reuses all of it through `ScanDiscipline::class_key`.
Every plan stages only through `Cx`, so every staged operation goes through the legacy chain of Option P.
Every fire counts on the fire count of its transaction (section N11.5).

```text
OFFER-SCAN(x)                                   ▷ disciplines/scan.rs, offer_scan
  tested ← 0
  loop
    order ← canonical_waiter_order(D)           ▷ the order of DR-75, reserved by the substrate
    W, m ← first waiter of order[tested..] whose pattern binds x and whose guard admits m
    if no W: store x with its introduction, return Settled
    step ← cx.rounds.after_fire(spent = ¬x.persist ∧ W does not peek, stays = W.persist)
    stage Fire(x, W)
    if W does not peek and x has the class κ: RETIRE-CLASS(κ)            ▷ the set only
    Done: return Settled    Reissue(cause): return Reissue(cause)
    Continue: tested ← position of W            ▷ the order of the other waiters is stable

RECEIVE-SCAN(K)                                 ▷ disciplines/scan.rs, receive
  loop
    r, m ← first record of canonical_data_order(D)[tested..] whose payload the pattern binds
    if no r, or the guard of K refuses m: park K, return            ▷ the guard is checked once
    C ← class_of(r)                             ▷ [r] for a bag or a peek
    step ← cx.rounds.after_fire(spent = ¬K.persist, stays = K.peek ∨ ∃c ∈ C: c.persist)
    stage Fire(K, C) over [D; |C|], dispatch the result of r only
    if K peeks and r is not persistent: RESTORE-SCAN(r)             ▷ the fire counts first (I-21)
    Done: return    Reissue(cause): re-issue K with no order    Continue: tested ← position of r

RESTORE-SCAN(r)                                 ▷ disciplines/scan.rs, restore_scan
  if cx.rounds.fired ≥ FIRE_ROUND_CAP: re-issue the produce of the payload of r
  else OFFER-SCAN(record bytes of r, identity Produce(D, r, false), introduction for_record(r))
```

A single pass over the tested prefix equals the host, which scans every waiter on each call (`offer_pass_is_host_send`).
A waiter that failed the datum fails it again, and the removal of a fired waiter keeps the order of the others.
The receive counts its fire before its restore. So one transaction never exceeds `FIRE_ROUND_CAP` fires, and a second send that meets a full transaction re-issues before it fires.

![Activity diagram of RECEIVE-SCAN with RESTORE-SCAN. The plan opens with the settle before it, and a transaction that made FIRE_ROUND_CAP fires re-issues the receive and stages nothing. The driver visits the records after the tested prefix in the order of DR-75, with one reserved decode for each record, and stops at the first record whose payload the pattern binds. When no record binds, or the guard refuses the record, the receive parks, so the guard is checked once. Otherwise the fire counts on the transaction and the plan stages Fire(K, [r]). When K peeks and r is not persistent, a full transaction re-issues the second send of r as a produce of its payload, and otherwise RESTORE-SCAN offers r with its own identity and authority.](diagrams/18-scan-driver.svg)

*Figure N11.2. Source: [diagrams/18-scan-driver.puml](diagrams/18-scan-driver.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

#### N11.3 The cell

`CellDiscipline` keeps every default plan: the head driver and the plain restore (`spaces/disciplines/cell.rs:53-88`).
It sets four items:

- `CAPACITY = Some(1)` (F15).
- `head_metered` calls the head rule of the queue, so one implementation of the least (ord, bincode bytes) rule serves both disciplines.
- `reaches_head = true`: the cell has no FIFO place to keep. An element that the other side takes needs no slot, and an element that must wait needs the free slot of its side.
- `precedes = true`: a re-entered element must behave as a fresh element, so an older entry on its side does not hold it back.

The restore stays plain. The peek fire removes the record, so the record slot is empty when the restore runs.
`cell_restore_never_occupied` proves that a capacity check at the restore would never fail.

#### N11.4 The settle loop and the stack

`StackDiscipline` is head-only on both sides with the greatest (ord, bincode bytes) as the head (`spaces/disciplines/stack.rs:69-139`).
A fresh element always reaches the top, and a re-issued persistent element precedes the top only when its first-round order is greater.
So a re-issued element re-enters at its LIFO place ([decision 43](decision-register.md#decision-43)).

A fire of a fresh element can expose an older pair below the tops.
So the stack settles before and after each plan, and in a settle job.
The settle loop fires the top pair that `StackView::next_fire` plans (`spaces/settle.rs:404-498`, `spaces/disciplines/stack.rs:152-219`).
It lifts a stored non-persistent record with a consume of `K_lift`, and it offers the record again with its own identity.
A record that a peek waiter did not spend stays in flight, and it lands at its order when the settle ends (Rule E).

The body of `K_lift` holds the counter value of the round and the step of the lift. The step is the fire count of the transaction at the lift.
Each lift comes right before its own fire, so no two lifts of a round share a step.
Each round bumps the counter, so no two lifts of a deploy share an identity.
At `FIRE_ROUND_CAP` or at a divergent pair, the settle before the plan re-issues the operation (`spaces/settle.rs:569-582`).
The settle after the plan and a settle job re-issue a settle job (`spaces/settle.rs:588-601`, `spaces/settle.rs:606-616`).

#### N11.5 One fire count for each transaction (I-21)

One space transaction makes at most `FIRE_ROUND_CAP` fires, its settles included ([decision I-21](decision-register.md#i-21), [decision 49](decision-register.md#decision-49)).
`Plan` holds one `FireRounds` for each transaction.
The settle before the plan, the plan and the settle after it count on it through `Cx::rounds` (`spaces/generic.rs:801`).
The settles of a stack once started counts of their own, so one transaction could make $`3 \times \mathrm{FIRE\_ROUND\_CAP}`$ fires.

```text
OPEN-PLAN(D, op):                               ▷ generic.rs, open_plan
  if D.settle_before(cx, op) re-issues: return that re-issue
  if cx.rounds.fired ≥ FIRE_ROUND_CAP:          ▷ the settle filled the transaction
    return Reissue(reissued_operation(op), RoundCap)   ▷ decision 43 keeps the order
  return none                                   ▷ the plan runs below the cap
```

Every plan calls `open_plan` before it stages (`spaces/generic.rs:1702-1715`).
A plan then fires only below the cap, because `after_fire` continues an element only while the count stays below the cap.
The settle loop checks the cap before each fire, and RESTORE-SCAN checks it before the second send (section N11.2).
A queue, a bag and a cell have no settle, so their plans start at the count 0.
Their behavior and the queue parity golden do not change.

![Activity diagram of one space transaction. run_with bumps the counter, and the count of the transaction starts at 0. The settle before the plan adds 1 for each fire. When it reaches the cap with a fire left, the transaction commits its fires and re-issues the operation, which keeps its order by decision 43. When the count is FIRE_ROUND_CAP after the settle, open_plan commits the fires of the settle and re-issues the operation, and its plan stages nothing. Otherwise the plan fires while the count stays below the cap, and a plan that re-issues commits its staged fires. The settle after the plan continues the count, and at the cap the transaction commits and re-issues a settle job, a transaction of its own that starts from the count 0. A transaction commits at most 64 fires.](diagrams/21-transaction-fire-count.svg)

*Figure N11.5. Source: [diagrams/21-transaction-fire-count.puml](diagrams/21-transaction-fire-count.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

#### N11.6 The allocator and the Array and Vector stores

An Array or a Vector space names its collections by index names, which only its allocator makes (P5d).
`ArrayOuter` and `VectorOuter` key and name their collections as `HashMapOuter` does (`spaces/outer/array.rs:130-197`, `spaces/outer/vector.rs:34-98`).
Their `check_key` accepts only the index names of the space, so a forged channel fails with `NotAnIndexName` (F8).

An allocation is one job over $`[C(\mathit{sid}), A(\mathit{sid})]`$: the family agent reads the config once, and the agent of the family runs `run_allocate`.
`AllocationRule::plan` gives the indices and the next value of the fixed Array, the cyclic Array and the Vector (`spaces/allocator.rs:109-162`).
`AllocCx` then takes $`A(\mathit{sid}) = n`$ and puts the next value (`spaces/generic.rs:2778-2826`).
An allocation that fails stages nothing, and every read and copy is reserved before it is made.

The reducer runs one allocation job for each distinct space of the plain binders of a `new` inside `use s` (F11, `reified/reducer_hooks.rs:732-777`).
Each job takes the cost authority of the process that evaluates the `new`, as its call `s!gensym()` would (`reified/reducer_hooks.rs:783-810`).
So each allocation is paid by its caller (P4 design B.7), and the handle method `gensym` runs a job with the authority of its call.
A store without an allocator answers `Baseline`, and the binders keep the names that HEAD draws.

![Sequence diagram of the allocation job of a new inside use s. The reducer calls reified_allocate with the new, the environment, the random state and the cost authority of the evaluating process. The hooks draw the names as HEAD does. For each distinct space of the plain binders, the hooks call allocate with an AllocOp of the space, the count, the random state and the authority. The family agent reads the config once and finds the family, and the allocator plans the indices. OutOfNames aborts with nothing staged. Otherwise the allocator takes A(sid) = n and puts the next value through the SpaceTxn commit, and the indices return to the hooks. Each binder gets the index name N(sid, u64_be(i)).](diagrams/22-allocation-job.svg)

*Figure N11.6. Source: [diagrams/22-allocation-job.puml](diagrams/22-allocation-job.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

#### N11.7 The pure PathMap layer

The PathMap layer of P5e encodes paths and re-roots payloads. It has no state, and the PathMap store of wave W3 uses it.
`channel_path` gives the path of a channel, and `encode_path` writes the path key and the end of each prefix key in one buffer (`spaces/pathmap/path_codec.rs:281-286`, `spaces/pathmap/path_codec.rs:343-347`).
The segment codec is byte-identical to EPM1 for every element that both codecs accept.
So the swap to the EPM1 adapter at P12 changes no key (`spaces/pathmap/path_codec.rs:369-371`).

The structural arms cover the ground scalars, the ground lists and tuples, and `GPrivate`.
A list or a tuple takes its arm only when every element does.
Every other element takes the escape arm at the top level, as in EPM1.
The escape payload is the Protocol Buffers encoding of the sorted `Par`, and an element that holds an `EPathMap` or an `EZipper` fails with `UnsupportedPathElement`.

`reroot` delivers one argument under a suffix, and `HeadEPathMapAdapter` decodes and encodes the `EPathMap` of HEAD (`spaces/pathmap/reroot.rs:51-75`, `spaces/pathmap/adapter.rs:293-337`).
The EPM1 adapter replaces it after the merge of P12, and the swap touches only `adapter.rs`.
Every read, copy, comparison and allocation of the layer is reserved before it is made ([decision 42](decision-register.md#decision-42)).

![Object diagram of the keys of the path [0, 1, 2]. The channel @[0, 1, 2] gives the path p = [0, 1, 2] by channel_path. encode_path gives path_key(p), the segments 03 00, 03 02 and 03 04 followed by the terminator 00. EncodedPath gives the prefix keys of p: the empty key of the prefix epsilon, 03 00, 03 00 03 02, and 03 00 03 02 03 04. The path key names the data channel D(sid, path_key(p)) of the datum, and each prefix key names the index channel I(sid, prefix_key(q)) of the markers and the waiters of wave W3.](diagrams/19-pathmap-keys.svg)

*Figure N11.7a. Source: [diagrams/19-pathmap-keys.puml](diagrams/19-pathmap-keys.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

![Activity diagram of R(s, v), the delivery of one argument v of a datum on the path p to a receive on the prefix q. The suffix s is p without q. When s is empty, v is delivered as it is. When v is one EPathMap, the adapter decodes it with its paths in key order and one path for each key. An empty map delivers the unit {{| s |}}, and every other map delivers {{| s ++ elems(e) |}} for each entry e, and the adapter encodes the map in the canonical entry form. Every other value is delivered as the list [s_1, ..., s_k, v].](diagrams/20-pathmap-reroot.svg)

*Figure N11.7b. Source: [diagrams/20-pathmap-reroot.puml](diagrams/20-pathmap-reroot.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

#### N11.8 Receive queries and allocation jobs in the hooks

Hook H4 fails only on a malformed QUERY tuple, and a query routes the inner channel of its tuple (`reified/reducer_hooks.rs:934-945`, `reified/reducer_hooks.rs:974-988`).
The fast path of a receive admits no marker tuple, so a QUERY tuple never becomes a channel of the host (`reified/reducer_hooks.rs:961-963`).

```text
REIFIED-PRODUCE: Space(n,k) → remove the PARAMS tuple from data; named ← SPACE-PARAMS(params)   ▷ positional: PositionalParamsUnsupported
                 SPACE-OP(Produce(n, k, chan, data, persistent, named, α))
REIFIED-CONSUME: fast path only when no use context and every channel routes to the default space and is no marker tuple
                 each bind: (c, q) ← QUERY-BIND(source)   ▷ a QUERY tuple gives its inner channel under the read rule, and its query
                            r ← ROUTE(space_ctx, c)
                 one bind: Default with q → QuerySpecUnsupported, Space → Consume with q, Agent → AgentSpacesUnavailable
                 a join: all Default → consume, or QuerySpecUnsupported when a bind has a query, else JOIN-ERROR
ALLOCATE-IN-SPACES(agent, new, env, rand, spaces, α):    ▷ reducer_hooks.rs, allocate_in_spaces (F11)
    env′ ← allocate_new_bindings(new, env, rand)          ▷ HEAD's draws
    one group per distinct space of the plain binders, in the order of the first binder
    each group: ReifiedRuntime::allocate(AllocOp(s, |group|, rand.to_bytes(), α))
        ▷ DanglingHandle, and RuntimeUnavailable in a detached or unbound runtime (N14)
    Baseline → GPrivate(0xE0‖N‖sid‖r_i)    Indices(v) → GPrivate(0xE0‖N‖sid‖u64_be(v_j))
```

`eval_new` in a use block and NEWSPACE both call `allocate_in_spaces` with the cost authority $`\alpha`$ of the evaluating process.
A store without an allocator answers `Baseline`. Its job reads the config, makes no event and charges no phlo, and a `new` without plain binders in a space runs no job.

#### N11.9 The set

The set adds only its class key to the scan driver (`spaces/disciplines/set.rs:160-165`).
The class fire, RETIRE-CLASS and the membership test are the one implementation in `scan.rs` (rule R-2 of the P5 design).

```text
SET-KEY(p_1 … p_n):                          ▷ set.rs:176-227; every buffer and encode reserved (decision 42)
  E ← a reserved buffer of n encodings
  for each p_i:
    b_i ← bincode(sort(p_i))                  ▷ the metered sorter in a metered job, else the host sorter
    L ← L + 8 + |b_i|                         ▷ checked: an overflow gives HostWorkRejected
  K ← a reserved buffer of exactly L bytes
  return u64_be(n) ‖ u64_be(|b_1|) ‖ b_1 ‖ … ‖ u64_be(|b_n|) ‖ b_n
CLASS-KEY(payload) = Some(SET-KEY(payload))   ▷ a bag gives None and does no work
```

The driver computes class keys in three places (`spaces/disciplines/scan.rs:597-653`, `505-527`, `794-879`).
`class_of` keys the members after the found record, `retiring_of` keys the element that fires a waiter, and `retire_class` keys each stored member.
It compares two keys through `same_class`: their metered hashes first, and the full keys only when the hashes are equal (`spaces/disciplines/scan.rs:884-899`).
The membership test reads each candidate once, so the driver keeps no ordered map of classes, and it reserves only the work that it does.

A metered job stores only a record whose key exists under its metering (`spaces/disciplines/scan.rs:419-429`).
The metered sorter has a depth limit, so a record without a key could otherwise enter a set.
Every later class operation that reaches such a record would fail.
Without a budget the host sorter keys every payload, so the check runs only in a metered job.
The check does no work for a bag.

![The set, as an activity diagram. A receive takes the first record in the order of DR-75 whose payload its pattern binds, and it checks its guard once on that record; a refusal parks the receive. A receive that peeks fires on the record alone, and RESTORE-SCAN then sends the record again. A receive that does not peek collects the record and every stored member with an equal key, fires one consume over the data channel for the whole class, dispatches only the result of the found record, and removes the members that are not persistent. A send fires the first waiter in the order of DR-75 that binds and admits its datum, and after a fire of a waiter that does not peek, RETIRE-CLASS stages one single-channel K_retire COMM for each stored non-persistent member of the class of the datum. A send without a waiter stores its datum, but in a metered job whose key computation fails it stops with HostWorkRejected before any staging.](diagrams/24-set-class-fire.svg)

*Figure N11.9. Source: [diagrams/24-set-class-fire.puml](diagrams/24-set-class-fire.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

#### N11.10 The priority queue

`PriorityQueueDiscipline` keeps its records in levels and its waiters in lanes ([decision 38](decision-register.md#decision-38), `spaces/disciplines/priority_queue.rs:232-346`).
A record holds its level in the priority slot of its envelope, and a continuation holds its lane in its policy slot (`spaces/record.rs:867-1233`).
So the queue keeps no storage for each level (I-10). `lane_heads` builds the map of the heads of the levels or of the lanes in one pass over a side (`spaces/disciplines/priority_queue.rs:355-397`).

```text
PQ-OFFER(op, ord):                                     ▷ priority_queue.rs:795-854
  if open_plan(op) re-issues: return it                ▷ settle before the plan, one fire count (decision 49)
  ℓ ← the level of op.params.priority, or the unprioritized level
  if op is not at the FIFO place of the head of level ℓ: STORE; settle after; return   ▷ decision 43
  loop
    W ← the older of the heads of the lanes Level(ℓ) and Any that is not refused,
        whose pattern binds the datum and whose guard admits it    ▷ a refusal moves on (decision 38)
    if W = none: STORE; settle after; return
    fire W; Continue → loop; Done → settle after; Reissue → re-issue op with its order
PQ-RECEIVE(op, ord):                                   ▷ priority_queue.rs:860-965
  if open_plan(op) re-issues: return it
  L ← Level(p) for priority = p, else Any
  if op is not at the head of its lane L: PARK; settle after; return
  loop
    h ← the first level head that L sees and whose payload the pattern binds
    if h = none, or the guard refuses h: PARK; settle after; return   ▷ the guard is checked once (decision 38)
    fire on h; a peek of a non-persistent h stores h again at its order (Rule E)
    Continue → loop; Done → settle after; Reissue → re-issue op with its order
```

A fire can expose a pair, so the queue settles before and after each plan and in a settle job (`spaces/disciplines/priority_queue.rs:319-345`).
The settle runs on the settle loop of the stack.
Strict confluence fails for a priority queue (readiness review S4).
So `PriorityQueueView::next_fire` plans the fireable lane head with the least order (`spaces/disciplines/priority_queue.rs:1010-1105`).
When that fire binds a stored record while a record is in flight, the settle lands the record in flight first (`spaces/settle.rs:425-430`).

The fire outcome decodes a fired continuation with `into_continuation`, which accepts the PQ policy slot and is strict on `Nil` for every other discipline (`spaces/record.rs:773-814`).
The send param and the query param `priority` take a `GInt` of at least 0, below the bound `priorities` when the space sets it.
A positional query part fails with `PositionalParamsUnsupported`.

![The priority queue, as an activity diagram. Every plan opens with open_plan, which runs the settle before the plan on the one fire count of the transaction. A send takes the level of its priority, or the unprioritized level, which comes after every priority level. A send that is not at the FIFO place of the head of its level stores its record at that place. Otherwise it offers the record to the heads of the lane of its level and of the unrestricted lane, the older head first, fires the first head that binds and admits the datum, and stores the record when each head refuses. A receive takes the lane of its priority, or the unrestricted lane, and parks at its place when it is not the head of its lane. A head waiter binds the first level head that its lane sees and whose payload its pattern binds, and a missing head or a guard refusal parks it. A fire of a peek stores a non-persistent record again at its order. After the plan the settle fires the oldest fireable lane head first.](diagrams/25-priority-queue-lanes.svg)

*Figure N11.10. Source: [diagrams/25-priority-queue-lanes.puml](diagrams/25-priority-queue-lanes.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

### N12. Wave W3 of P5: joins of one space

Wave W3 lets a receive join the collections of one native space ([decision 29](decision-register.md#decision-29)).
The [specification](specification.md#38-joins) states the rules JN-1 to JN-15, and this section explains how the code meets them.
The new module `spaces/join.rs` holds the [join engine](glossary.md#join-engine).
The drivers of N4 and N11 stay byte-identical for every collection without a join group.
The join engine runs every operation that a join can reach.

#### N12.1 The agent interface

Table N12.1 lists the new items of the agent interface.
A program never issues a settle job, and the reducer issues a join only through the receive hook.

*Table N12.1. The agent interface of the joins.*

| Item | Content |
|---|---|
| `JoinBind` (`spaces/agent.rs:201-210`) | The collection key, the channel as the program names it, the pattern, and the query of one bind. |
| `JoinOp` (`spaces/agent.rs:215-238`) | The space, the binds in bind order, the body, the arrow flags, the guard, the two authorities, and the carried orders of a re-issued persistent join. |
| `SpaceOp::Join` (`spaces/agent.rs:319-324`) | The fourth operation of a space job. |
| `SettleOp::cascade` (`spaces/agent.rs:276-292`) | The rest of a cascade, or `None` for the settle of one stack or priority-queue collection. |
| `CascadeState` (`spaces/agent.rs:260-269`) | The collections that can still owe a fire, the owed second sends in run order, and the operation to resume. |
| `PendingProduce` (`spaces/agent.rs:244-256`) | One owed second send: its collection, its record with the envelope, and its persistence. |
| `SpaceAgent::prepare_join` (`spaces/agent.rs:480-484`) | Stages a join: the classification, the scope, and the engine. |

The receive hook routes a receive with two or more binds on one native space to `SpaceOp::Join` (`reified/reducer_hooks.rs:1042-1070`).
Any other mix of routes fails with `AgentSpacesUnavailable` or `CrossSpaceJoin` (`reified/reducer_hooks.rs:1100-1127`).

#### N12.2 Classification and the scope

A collection operation opens one transaction over its own collections and reads the config once.
A join opens it with `begin_join`, over the distinct collections of its binds (`spaces/generic.rs:2280-2317`).
`prepare_collection_op` then lets `classify` decide the route (`spaces/generic.rs:2135-2168`, `spaces/join.rs:371-404`).
An operation that no join reaches keeps the driver of its collection.
The classification reserves exactly the reads of the join check of P4.
So such an operation keeps its host work, and the queue parity golden does not change.

```text
CLASSIFY(op):                                    ▷ join.rs, classify
  if op is a JoinOp or a settle job with a cascade: return JOIN-SCOPE(op)
  match the join groups of D(op.key):
    []                         → Single                               ▷ no reservation
    [g]                        → reserve inspect(g), inspect(D), one comparison
                                 Single when g = [D(op.key)], else JOIN-SCOPE(op)
    two or more groups         → reserve inspect(D), one comparison; JOIN-SCOPE(op)
JOIN-SCOPE(op):                                  ▷ join.rs, join_scope
  VALIDATE(op)                                   ▷ the key and the query of each bind, before any staging
  own ← the distinct collections of op, in canonical group order
  todo ← own, with the seeds and the pending keys of a settle job
  while todo ≠ ∅:                                ▷ each read, copy, lookup and growth is reserved first
    c ← the least key of todo; remove c from todo
    for each join group g of D(c), for each channel ch of g:
      require ch = D(sid, k) for a key k of this space, else Inconsistent
      add k to todo unless the closure holds it
    add c to the closure
  keys ← [C(sid), S(sid)] ++ [S(sid, k), D(sid, k) for each k of the closure, ascending]
  return the scope: the closure, own, the group order, and keys
```

`VALIDATE` checks the key of each bind with the outer store and the query of each bind with the discipline (`spaces/join.rs:412-471`).
A join with fewer than two binds, or a join that both persists and peeks, is `Inconsistent`, because the reducer never builds one.
When the open transaction does not cover the scope, `prepare_collection_op` drops it and begins one transaction over the lock keys of the scope.
Nothing is staged before the drop, so the drop applies nothing.

Four facts make this "plan, then lock" step sound:

1. **Coverage.** Every channel that the transaction stages on is a lock key. A fire, a lift, a retire and a second send act on collections of the closure, and each touch bumps a counter of the closure.
2. **Stability.** No transaction of the space commits between the two transactions, because the scheduler footprint $`[2] \mathbin{\Vert} \mathit{sid}`$ serializes the jobs of a space (N6).
3. **Safety.** If the premise of fact 2 failed, a staging outside the lock keys would fail with `UnlockedChannel`, which is deterministic.
4. **One escalation.** The closure is complete, so the second transaction never needs a third.

#### N12.3 Windows and searches

Each discipline declares the shape of its windows with `Discipline::JOIN_WINDOW` (`spaces/generic.rs:273-289`).
Table N12.3 gives the window of the $`m`$ positions of a waiter on one collection.

*Table N12.3. The windows of the disciplines.*

| Shape | Disciplines | Window of $`m`$ positions on a collection |
|---|---|---|
| `Head` | queue, stack, cell | The first $`m`$ entries of the head order of the records ([decision 11](decision-register.md#decision-11)). |
| `Lanes` | priority queue | For each level, the first $`m`$ records of the level that the lane of the position sees ([decision 38](decision-register.md#decision-38)). |
| `Scan` | bag | Every record, in the canonical order of DR-75 ([decision 41](decision-register.md#decision-41)). |
| `Classes` | set | One representative for each class, at the place of its first member. A position claims the whole class. |

An element in flight takes its place in the window of its collection.
A fresh element takes the place of a new entry, and a re-issued element takes the place of its carried order ([decision 43](decision-register.md#decision-43)).
A claim keeps the positions of one fire apart: a record claim names a record, and a class claim names a set class ([decision 28](decision-register.md#decision-28)).

```text
GREEDY(W):                                       ▷ join.rs, search (decision 37: no backtracking)
  σ ← []
  for each position i of W, in group order:
    σ[i] ← the first unclaimed entry of the window of coll(i) that the lane of i sees
           and that the pattern of i binds; none → return none
    claim σ[i]
  return σ if the guard of W admits the values of σ in bind order, else none
SEARCH-PRODUCE(W, x):                            ▷ join.rs, search_produce (decision 28)
  require W is the head waiter of each head-only collection of its group
  for each position p on the collection of x, in group order, where x enters the window:
    if the pattern of p binds x:
      σ ← GREEDY(W) with σ[p] fixed to x and claimed
      if σ ≠ none: return (p, σ)
  return none
```

`admits_all` checks the guard once, over the bound values in bind order (`spaces/matching.rs:88-103`).
The produce-side search tries every position of the datum, so a send fires whenever greedy on the state with the datum stored would fire (`join_produce_search_agrees_with_head`).
Both searches reserve each envelope decode, each payload decode, each match and each claim lookup before they make it.

#### N12.4 The transaction

`run_join` runs the engine in one plan of the transaction (`spaces/join.rs:669-760`).
`Engine::run` follows RUN-JOIN (`spaces/join.rs:2685-2772`).
Figure N12.4 shows one transaction, from the classification to the commit.

```text
RUN-JOIN(op, scope):                             ▷ join.rs, Engine::run
  for each own collection c, in group order: TOUCH(c)        ▷ one bump each (decision 31)
  ords[c] ← the bump of c, or the carried order of c          ▷ decision 43; 0 for a bag or a set
  if op is a settle job:                         ▷ it has no plan: its drain is its settle before the plan
    work ← its pending second sends, the first one on top     ▷ before any fire, so a stop carries them again
    mark every collection of the scope dirty
    DRAIN-AND-CASCADE(); return COMMIT()
  if CASCADES: mark every collection dirty; DRAIN-AND-CASCADE()   ▷ the settle before the plan
  if stopped: return COMMIT()                    ▷ the operation re-issues as the program issued it
  mark every collection clean
  if fired ≥ FIRE_ROUND_CAP: stop; return COMMIT()   ▷ the plan opens below the cap (decision 49)
  match op:
    Produce        → OFFER(its datum)            ▷ join.rs, offer
    Consume, Join  → RECEIVE(its waiter)         ▷ join.rs, receive
  DRAIN-AND-CASCADE()
  return COMMIT()
CASCADES ≡ JOIN_WINDOW ∈ {Head, Lanes, Classes}   ▷ decision 50 adds the set
TOUCH(c):                                        ▷ join.rs, Engine::touch
  if the discipline has a counter and c is not bumped yet: bump S(sid, c) once, before its first staging
```

OFFER offers the element in flight to the waiters of its collection, in the order of the source (`spaces/join.rs:3717-3875`).
A head that refuses stores the element, and on the other shapes a refusal moves on to the next waiter.
RECEIVE parks the waiter at its places, unless it is the head of each head-only collection and its search and guard pass (`spaces/join.rs:4017-4210`).
A receive with one bind on a set keeps its class fire over $`[D; n]`$.
A join with two or more binds fires one consume over its group.
PARK checks the capacity of each cell collection before it stages anything (`spaces/join.rs:4352-4382`).

![Activity diagram of one transaction of the join engine. PREPARE begins over the own collections of the operation and reads the config once. CLASSIFY sends an operation that no join reaches to the driver of its collection, whose reservations equal the P4 join check. Otherwise JOIN-SCOPE checks each bind and closes the collections under their join groups, and a transaction that does not cover the scope is dropped before any staging and begun once again over the lock keys of the scope. RUN-JOIN bumps each own collection once and takes the orders, or the carried orders of decision 43. A settle job puts its pending second sends on the work stack and marks every collection dirty. Another operation runs the settle before the plan when its discipline cascades, which is the queue, the stack, the cell, the priority queue and the set, and not the bag. A stopped transaction or a full fire count commits and re-issues the operation. Otherwise the plan offers the datum or receives the waiter, with the windows, the greedy search and one guard check, and a peek fire pushes its second sends. DRAIN-AND-CASCADE then loops while a second send is owed or a waiter of a dirty collection can fire: a full count stops it and carries the rest, a second send is popped and offered in receipt order with its own identity, and otherwise NEXT-FIRE picks the head, the oldest lane head or the first set waiter of the least dirty collection, and LIFT-AND-REOFFER lifts the first non-persistent bound record with K_lift, keyed by the counter and the step or by the source hash on a set, offers it to the waiter, and retires the bound set classes. COMMIT re-issues one settle job with the pending sends, the dirty seeds and the resume when the transaction stopped with owed work, and otherwise re-issues the operation only when its element is still in flight.](diagrams/26-join-engine.svg)

*Figure N12.4. Source: [diagrams/26-join-engine.puml](diagrams/26-join-engine.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

#### N12.5 Rule E in joins: the second sends

A peek fire removes each non-persistent stored record that it binds, as a host COMM does ([decision 45](decision-register.md#decision-45)).
The same transaction sends each such record again through OFFER, with its identity and its own authority (`spaces/join.rs:3598-3639`).

```text
PUSH-SECOND-SENDS(σ, x at p?):                   ▷ join.rs, second_sends; the results in receipt order
  for i from n − 1 down to 0:
    if i = p:  if x is in flight: push (coll(i), x.record, x.persist, own = x.own)   ▷ decision 30
    else if σ[i] is not persistent: push (coll(i), the record of σ[i], false, own = false)
  ▷ the stack is LIFO, so the pops run in receipt order, the order of the host's produce_peeks
```

The element in flight is offered again also when it is persistent, as `offer_head` does ([decision 30](decision-register.md#decision-30)).
A persistent stored record that the cascade offers again without a lift is not in flight, so it stays stored and is not pushed.
A second send registers the authority of its record (`Introduction::for_record`), and it bumps no counter again.

#### N12.6 The cascade

A fire can make another waiter of the scope fireable ([decision 50](decision-register.md#decision-50)).
A head-only collection can get a new head, or a set class can move to a new place.
DRAIN-AND-CASCADE fires each such waiter in the same transaction (`spaces/join.rs:4471-4503`).

```text
DRAIN-AND-CASCADE():                             ▷ join.rs, drain_and_cascade
  loop
    if stopped: return
    if work ≠ []:
      if fired ≥ FIRE_ROUND_CAP: stop; return    ▷ carry the work, as RESTORE-SCAN does
      OFFER(pop(work)); continue
    if not CASCADES: return                      ▷ the bag: a removal exposes nothing
    F ← NEXT-FIRE(); if F = none: return
    if fired ≥ FIRE_ROUND_CAP: stop; return
    LIFT-AND-REOFFER(F)
NEXT-FIRE():                                     ▷ join.rs, next_fire
  for each dirty collection c, in ascending key order:
    for each waiter W in the cascade order of c:   ▷ the head, the lane heads oldest first, or the
                                                   ▷ DR-75 order of the union of the groups of c
      if W is at the head of each head-only collection of its group and GREEDY(W) = σ ≠ none:
        return (W, σ)
  return none
LIFT-AND-REOFFER(W, σ):                          ▷ join.rs, lift_and_reoffer
  v ← the least position with a non-persistent record in σ, else position 0
  TOUCH each collection of the group of W
  if σ[v] is not persistent:
    K ← lift_continuation(the bump of coll(v), fired)      when the discipline has a counter
        retire_continuation(the source hash of σ[v])       on a set (decision 50)
    stage the K_lift COMM that removes σ[v]       ▷ never dispatched; later indices of coll(v) shift down by one
  stage the produce-side fire of W with σ[v] in flight, with its identity and its own authority
  if W peeks: PUSH-SECOND-SENDS(σ, σ[v] at v)     ▷ σ[v] is in flight only when it was lifted
  else: RETIRE-SET-BINDS(σ, σ[v])                 ▷ the re-offered record is the element (decision 50)
  if the fire count says Reissue(cause): stop with cause   ▷ a stored divergent pair (A10)
```

A collection with a counter keys `K_lift` by the counter value of the transaction and the step, as the settle of a stack does (`spaces/join.rs:4716-4768`).
A set has no counter, so its `K_lift` is keyed by the source hash of the lifted record, as `K_retire` is.
So every lift of a set has an identity of its own, also when a re-issue repeats the random state and the step.

The cascade ends, because every step lowers the measure

```math
\mu = \left(\, w_{np},\ r_{np} + q,\ q \,\right)
```

in lexicographic order, or stops the transaction.
Here $`w_{np}`$ counts the non-persistent waiters, $`r_{np}`$ counts the non-persistent records, and $`q`$ counts the owed second sends, the elements in flight included.

- A peek fire spends its waiter, because no receive both peeks and persists.
- A fire of a non-persistent waiter removes the waiter.
- A fire of a persistent waiter spends its non-persistent element. With a persistent element, the pair is divergent, and the transaction stops.
- A second send that is stored lowers $`q`$, and a `K_retire` lowers $`r_{np}`$.

For a queue, a stack, or a cell, two distinct fireable waiters have disjoint collection sets, because each is the head of all its collections.
So their fires commute (`cascade_order_independent`).
Strict confluence fails for a priority queue, which has two lanes for each collection, and for a set, which has many waiters for each collection.
So NEXT-FIRE fixes an order: the oldest lane head of a priority queue, and the DR-75 order of the union for a set.

RETIRE-SET-BINDS runs after every fire of a waiter that does not peek, on the consume side, on the produce side, and in the cascade (`spaces/join.rs:2258-2292`).
It retires the other non-persistent stored members of each bound set class with single-channel `K_retire` COMMs, in canonical order.
A peek retires nothing, and a bag has no classes, so the step does nothing there.

#### N12.7 Stop, re-issue and the settle job

`commit` decides the re-issue of the transaction (`spaces/join.rs:4773-4835`).
A divergent own element does not stop the transaction: the element stays in flight, and the drain fires the other owed pairs.
Only the fire cap and a stored divergent pair of the cascade stop a transaction.
A stored divergent pair then fires once in each settle job (A10).
The host also fires a persistent send and a persistent receive until the phlo runs out.

```text
COMMIT():                                        ▷ join.rs, commit
  for each work item r, in pop order:
    if r is the own element: the operation is in flight again
    else: pending ← pending ++ [r]
  owed ← pending ≠ [] or (CASCADES and some collection is dirty)
  resume ← the re-issue of op when its element is not settled, else none   ▷ decision 43 orders
  if not stopped, or not owed: re-issue resume                           ▷ none re-issues nothing
  else: re-issue Settle(key, channel, random state and authorities of op,
                        cascade: {seeds: the dirty collections without key, pending, resume})
```

The settle job is an operation of its own, so its count starts at 0 ([decision 49](decision-register.md#decision-49)).
It loads its pending second sends before any fire, cascades over its scope, and re-issues `resume` when it completes.
A new stop in the job re-issues the next settle job, which carries the same `resume`.
One outcome holds one re-issue, so `resume` keeps the order: first the owed second sends, then the operation, which settles before its plan.

*Table N12.7. Decision 43 for the joins.*

| Element | Its order | Re-issue |
|---|---|---|
| A join waiter | One order for each distinct collection, from the bump of each collection in the first round, in the join slots | At the cap or at a divergent pair, `reissued_ords` gives one order for each bind, equal for repeated binds. Each round bumps every own counter again. |
| A produce or a consume on a joined collection | As in P4: `reissued_ord` | `reissued_operation` |
| An owed second send | The record keeps its envelope, so its order and its identity stay | Carried in `CascadeState::pending`, and it re-enters at its place |
| A bag or a set join | Every order is 0 | A persistent join carries the order 0 for each bind in `reissued_ords`, and the identity is stable |

#### N12.8 The record of a join waiter

A join waiter parks with version 2 of the continuation envelope, with one [join slot](glossary.md#join-slot) for each position of its group (`spaces/record.rs:1797-1802`).
A slot holds the order of the waiter in the collection of its position and the index of its bind.
It also holds the lane policy of a priority-queue bind.
The bind indices are a permutation of the binds.
So `restore_bind_order` puts the results of a fire, which follow the group order, back into the bind order of the receive (`spaces/record.rs:1936-1962`).

A waiter with one bind keeps the version 1 record, and the decoders of each version reject the other version.
The engine prepays the unwrap of a fired join record at staging, so the outcome after the commit only moves values (`spaces/record.rs:2097-2113`).

![The space records as a component diagram, from left to right. The record tag is a GByteArray of four bytes: 0xE0, 0x52, the kind and the version, with the kinds D for a datum, K for a continuation, S for a counter and C for the config, version 1 for every record and version 2 for a join waiter. A datum record of version 1 on D(sid, k) holds the list of its tag, its order, its priority and its embedding as its first par, and then the payload of the send, while the random state, the cost authority and the cost stack stay outside the envelope, and a priority queue puts the level of the record in its priority slot. A counter on S(sid) or S(sid, k) holds the list of its tag and then the counter value, and the config on C(sid) holds the list of its tag and then the config map, so no space datum is a single number. A continuation of one bind of version 1 on [D(sid, k)] holds the list of its tag, its order, its policy and the body, with the policy Nil or the lane of a priority queue. A continuation of a join of version 2 on its canonical group holds the list of its tag, the list of its slots and the body, where the slot of the position i holds the order of the waiter in the collection of the position, the bind of the position in the receive, and the policy of the position, and the bind indices are a permutation. A fire of a join waiter gives its results in group order, and restore_bind_order puts the result of the position i at its bind, so the body binds its values in bind order. A note states that the decoders of each version compare the whole tag and are strict.](diagrams/11-record-envelope.svg)

*Figure N12.8. Source: [diagrams/11-record-envelope.puml](diagrams/11-record-envelope.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

#### N12.9 Charges and reservations

Each staged operation of the engine pays the legacy charges of its host equivalent ([decision 45](decision-register.md#decision-45), Option P, N10).
Table N12.9 lists the new staged shapes, and `legacy_charges.rs` computes their rows unchanged.
Every read and copy of the engine is reserved on the host-work meter of the job before it is made ([decision 42](decision-register.md#decision-42)).

*Table N12.9. The staged shapes of the join engine.*

| Staged operation | Option P row | Registered authority |
|---|---|---|
| The park of a join waiter | Park | The introduction of the operation |
| A consume-side join fire | Consume fire | The introduction of the operation |
| A produce-side group fire | Produce fire | The datum of the operation: its introduction. A second send or a lifted record: the authority of the record. |
| A `K_lift` | Consume fire, not persistent | The introduction of the operation |
| A second send that stores | Store | The authority of the record |
| A `K_retire` | Consume fire | The introduction of the operation |
| A counter bump of a touch | Consume fire and store (`counter.rs`) | The introduction of the operation |

There is no structural cap ([decision 40](decision-register.md#decision-40)), and `FIRE_ROUND_CAP` bounds each transaction (I-21).

#### N12.10 Merges

Every join transaction bumps the counter of each own collection and of each collection that it stages on.
So two branches that change one ordered collection race on its counter under check #1, also when a cascade reaches the collection through a join.

A collection that a transaction only reads is read through a join entry. If that entry is in the pre-state, check #3 flags every produce on its channels.
A persisted join waiter makes every later produce on its channels conflict with every other branch under check #3, as a host join does.
Counter channels have no joins, so their produces are not flagged.
Bag and set joins have no counters, so check #2 and check #3 order them, exactly as host joins.

Only the join of a program parks a waiter on two or more channels.
`K_lift`, `K_retire` and the counter takes are single-channel consumes that always fire, and second sends, re-offers and counter puts are produces (I-16).
A fire removes the join entries of its group when no waiter of the group is left.

#### N12.11 The seam of a store

The engine reaches the discipline, the windows and the records of a store through the trait `JoinSource` (`spaces/join.rs:889-1205`).
`KeyedSource` implements it for the keyed stores HashMap, Array and Vector (`spaces/join.rs:1540-2251`).
Its offer order and its cascade order for a set merge the canonical orders of the groups of a collection by source hash (`spaces/join.rs:2316-2395`).

The keyed source holds the records and the waiters of a collection on one data channel.
The PathMap store keeps its waiters and markers on index channels and its records on data channels.
So the seam also covers each place where the engine touches the records of a store (joins plan section 9).
Table N12.11 lists these methods.
`KeyedSource` implements each method with the code that the engine ran at that place before the seam existed.

*Table N12.11. The record methods of `JoinSource`.*

| Method | What the engine asks | Keyed source |
|---|---|---|
| `stored_entry` | the payload, the persistence and the claim of a stored entry of a window | the record of the data channel |
| `members` | the stored entries that a single-bind class fire visits, in canonical order | the canonical order of DR-75 |
| `home` | the channel of the produce identity of an element | the data channel |
| `stored_element` | the element that the cascade lifts or offers again, and the record that the lift removes | the stored record |
| `second_send` | the second send of Rule E for a stored entry that a peek fire binds | the removed record, offered on its collection |
| `incoming_source`, `incoming_record` | the identity and the data of the incoming side of a fire | the element |
| `enters`, `incoming_payload` | the windows that an element enters, and its payload there | its own collection and payload |
| `held`, `land` | the count of the capacity check, and the store of an element | the records of the data channel, and one produce |
| `before_fire`, `retire_spent` | what a fire spends, and the retirement after a fire that does not peek | the class claims, and RETIRE-SET-BINDS |
| `refusal_blocks` | whether a refusal by a waiter of an offer stores the element | true for a head-only collection |
| `shares_counter`, `finish` | one counter for all collections, and the last staging before the commit | false, and nothing |

Work items, elements in flight and pending second sends carry `restore`, the depth of the PathMap marker that a peek COMM removed.
A keyed source always carries `None`.
Each window also receives the pending second sends, and a keyed source ignores them.
The outer store chooses the source: `OuterStore::classify` and `OuterStore::run_join` use the keyed functions unless a store overrides them (`spaces/generic.rs:147-172`).
A store with `ALWAYS_ESCALATES` runs every join in a new transaction over the lock keys of its scope.
The PathMap source of Phase 2 implements the same methods with the plans of N13 ([decision 51](decision-register.md#decision-51)).

The seam leaves the keyed engine unchanged.
A unit test pins the sizes of the types that the engine reserves by size, because a larger type reserves more host work.
A differential test runs 80 programs and compares their staged operations, events, charges, roots and host-work usage with a golden ([dossier 12.15](verification.md#1215-the-joins-of-one-space)).

### N13. Wave W3 of P5: the PathMap engine

The [PathMap delivery](pathmap-prefix-delivery.md) explains the PathMap store in full, and the [specification](specification.md#26-the-pathmap-store) states its rules PS-1 to PS-10.
Phase 1 of wave W3 adds three modules. Phase 2 runs their plans on the join engine of N12 and registers the PathMap families ([decision 51](decision-register.md#decision-51)).
Phase 2 adds the fourth module of Table N13, the [prefix source](glossary.md#prefix-source), and the joins over prefixes of wave W4.

*Table N13. The modules of the PathMap engine.*

| Module | Content |
|---|---|
| `spaces/outer/pathmap.rs` | `PathMapOuter`: the key check with the path rules, the data channel of a path, the index channel of a prefix, and the token. |
| `spaces/pathmap/markers.rs` | The marker record, its random state, the vehicle, and their strict and metered decoders. |
| `spaces/pathmap/delivery.rs` | The pure plans over a staged view: CONSUME-CANDIDATES, PRODUCE-WAITERS, REROOT, STORE-MARKERS, RETIRE-PLAN, and the head and scan orders on prefixes. |
| `spaces/pathmap/join.rs` | `PrefixSource`, the store seam `JoinSource` of a PathMap space (Phase 2). PREFIX-SCOPE gives the own prefixes of an operation, the closure of their join groups and of the stored datums, and the lock keys. The windows, the vehicles, the second sends, RETIRE-PLAN at each fire site and FINISH run there. |

The PATHMAP-ENGINE batch took five design choices within the recorded decisions:

1. **D-1: the peek of Rule E.** A peek COMM removes only the marker of its prefix. The second send offers the datum most specific first, and it spends the datum or restores the marker.
2. **D-2: the order.** An ordered discipline takes the value of the token as its order. A bag or a set takes the order 0, as on a HashMap store.
3. **D-3: the marker record.** The marker holds the tag of the kind I and the source hash of its datum. It also holds the suffix of the path under its prefix (rule PS-3).
4. **D-4: the vehicle.** The vehicle is the marker followed by the payload, and no store keeps it (rule PS-4).
5. **D-5: the keys.** `PathMapOuter::key` gives the path key, and a new space stages no record of the store, because the generic creation stages the token.

The choice D-1 keeps the second send of a peek exactly as the host's second send ([decision 45](decision-register.md#decision-45)).
The other reading, a peek that retires the record and the markers itself, would mark records of the pre-state as consumed.
It would also change shared index channels without a bump of the token.

Phase 2 connects the plans to the join engine through the record seams of N12.11.
`PathMapOuter` sets `ALWAYS_ESCALATES`, and its hooks `classify` and `run_join` send every PathMap operation to the engine, also a single-bind one.
The scope key of a prefix $`q`$ is `[0x49] ‖ prefix_key(q)`, so no scope key of a PathMap space is equal to a keyed collection key.
A receive with two or more binds on prefixes of one space is a join over prefixes (wave W4).
Its group order is ascending by the hash of each index channel $`I(q_i)`$, then by bind index.
The [PathMap delivery](pathmap-prefix-delivery.md#510-the-prefix-source-on-the-join-engine) gives each method of the source with its pseudocode.

Phase 2 took three more design choices within the recorded decisions:

6. **D-6: the marker authority.** Every marker of a datum $`d`$ carries the cost authority of the record of $`d`$ on $`D(p)`$ (rule PS-3).
   One function, `marker_authority`, gives it to STORE-MARKERS and to the restore of a marker.
   So a restored marker has the bytes and the produce identity of the removed marker.
   This holds also when the record carries no authority or an empty one.
7. **D-7: a fire cap that leaves a marker owed** (open item O-2 of the seam arbitration).
   FINISH stores again each marker that the capped transaction still owes, before its commit.
   The settle job carries only the offer of the datum, with its depth in `restore` (rule PS-8).
   When another job of the deploy spends the datum first, the offer meets no waiter and stores nothing.
   So no job sees a stored datum without one of its markers.
8. **D-8: the full closure.** When a prefix of the scope holds a marker, PREFIX-SCOPE reads the root $`I(\varepsilon)`$ and adds each prefix of each stored datum.
   So the work of an operation grows with the number of datums of the space, and the meter reserves each read first.
   The scheduler already runs the operations of one space one at a time ([I-11](decision-register.md#i-11)), so the wider scope removes no parallel work.

The choice D-7 keeps the marker invariant at every commit.
The other reading of O-2, a settle job that restores the marker, would leave the invariant broken between two commits.
Each other job of the deploy could then read a datum without one of its markers.

### N14. Decision 47: spaces in native execution

Protocol version 6 of `feature/cost-accounted-rho` runs user deploys in native execution ([specification, section 8](specification.md#8-native-execution)).
[Native play](glossary.md#native-play) records a [journal row](glossary.md#journal-row) for each RSpace operation, and [native replay](glossary.md#native-replay) checks each operation against its row.
[Decision 46](decision-register.md#decision-46) made a space deploy fail in both modes, and [decision 47](decision-register.md#decision-47) runs spaces in both modes.
This section describes the metered view, the [native backend](glossary.md#native-backend), the wiring, and the argument that native replay always ends.
The [dossier](verification.md#13-decision-47-spaces-in-native-execution) gives the proofs, the models, the tests and the measurements.

The change edits one file of the accounting module: the accessor `space_store`, which decision 47 approves as the only exception to [M-3](decision-register.md#m-3).
Everything else is in the campaign files, the one-line `mod space;` of `native_session.rs`, and the bind call of `rho_runtime.rs`.

#### N14.1 The metered view

Native play and native replay must charge the same host work for each space operation.
A legacy transaction loads its [staged view](glossary.md#staged-view) through the plain reads of the hot store, which charge nothing.
The physical read of a key does different work on a cache hit and on a cache miss.
So a charge for the physical read would depend on the cache, and the host-work rule forbids that ([I-12](decision-register.md#i-12)).

A native transaction therefore reads through [metered view reads](glossary.md#metered-view-read).
Each first load of a key into the view makes three steps:

1. Reserve the key: inspect the channel, and then hash its bincode bytes with the metered writer. A group of channels charges the hash of its sorted channel keys, as the native replay of a host consume does.
2. Read the value through the native history reader, with a meter that accepts every reservation.
3. Reserve the copy of the value, and then copy the value into the view.

The charge of a load is then a function of the channel and of the loaded value only:

```math
\mathrm{charge}(c, v) = \mathrm{inspect}(c) + \mathrm{hash}(c) + \mathrm{copy}(v)
```

Here $`c`$ is the channel or the channel group of the load.
The value $`v`$ is the list of records, continuations or join groups that the load copies.
The commit checks of [S5.2](#s52-commit) read through the same three steps.
The Rocq theorem `view_charge_cache_independent` (`EventCorrelation.v:8773`) proves that a coherent cache does not change the charge of a load.

No deploy pays for the physical read of step 2. Three bounds limit that read instead:

- The native reader has structural limits on depth, allocation and overflow, and these limits act in both modes.
- The first read of a key leaves the key in the cache.
- Step 3 charges every byte that enters the view.

*Table N14.1. The metered view in the substrate.*

| Part | Code | Rule |
|---|---|---|
| The mode `NativeReplay` | `SpaceMode`, `space_store_handle.rs:86` | Staging treats the mode as play: it pairs no fire and admits every candidate. |
| `begin_metered` | `space_store_handle.rs:823-835` | The registration of the transaction carries the metered reads. `begin` and `SpaceTxnHost::begin` keep their signatures. |
| `NativeViewReads` | `space_store_handle.rs:996-1080` | The three steps of a load, for data, for continuations and for joins. |
| `history_records` | `space_store_handle.rs:1117-1147` | A cold load decodes its leaf with the native history reader in every mode. So the decode limit of depth 128 acts alike in play and in replay. |
| The effect helpers | `space_store_handle.rs:3248-3355` | `store_data`, `remove_matched_datum_and_join`, `store_waiting_continuation` and `store_persistent_data` moved here from `space_play.rs`. Play and the native backend call the same functions. |
| `SpaceEnv::begin` | `spaces/agent.rs:112-121` | A space job opens a metered transaction when its budget is native, and a plain transaction otherwise. |

Legacy execution does not change.
A transaction of `begin` carries no metered reads, so each of its loads reads as before.
The legacy goldens and the suites of P4 and P5 pass unchanged.

#### N14.2 The native backend

`NativeSpaceHost` (`replay_rspace/native_session/space.rs:76-176`) implements `SpaceTxnHost` over a native replay session.
Its module is a child of `native_session.rs`, so it sees the private items of the session, as `space_replay.rs` sees those of `ReplayRSpace` ([M-4](decision-register.md#m-4)).
The host holds four parts: the owner of the session, a projection to the session, the authority resolver of the accessor, and the installed channels.
It reads the installed channels once, at creation, so the guard of installed channels holds in native replay as in play.

`begin` (`native_session/space.rs:234-274`) requires metered reads, and it takes the stripes with no charge, as play takes them.
It then opens a `HostTxn` in the mode `NativeReplay`, so staging is the staging of play.
The difference is at commit: each staged operation meets its journal row through the [ticket](glossary.md#ticket) protocol of the session (`apply_op`, `native_session/space.rs:398-432`).

```text
NATIVE-APPLY-OP(store, op, observe, reads, events):  ▷ native_session/space.rs, apply_op
    require the session is open, and take its gate for reading   ▷ at a boundary: mismatch
    result ← NATIVE-OPERATION(op)
    if result is an error and the operation published no denial:
        close the epoch                      ▷ every waiting operation fails with Closed
    return result

NATIVE-OPERATION(op):                        ▷ apply_produce and apply_consume
    op ← the commit checks of play on op, with the metered reads   ▷ S5.2
    observe.before_observe(source)           ▷ registers the authority of the operation
    α ← the authority of source, from the resolver of the accessor
    ticket ← epoch.begin_operation(source)   ▷ a row that is not ready: mismatch
    ticket.authenticate_footprint(channels, joins)   ▷ a consume passes no joins
    o ← ticket.outcome()
    require o fits the staged kind           ▷ store or park: Stored or DeniedIntroduction
                                             ▷ fire: Matched, DeniedIntroduction or DeniedComm
    d ← the observe_produce or observe_consume call of play, with α
    if o = DeniedIntroduction: require d = Denied, and return PUBLISH-DENIAL(ticket, o)
    require d = Granted
    if op fires:
        require the staged COMM is the recorded candidate     ▷ metered_matches_comm
        d ← ticket.observe_comm(comm, continuation, candidates)
        if o = DeniedComm: require d = Denied, and return PUBLISH-DENIAL(ticket, o)
        require d = Granted
    if op is a produce: prepare its produce counter
    completion ← ticket.prepare(Stored or Matched)
    under one publication guard:
        apply the effects of play            ▷ the helpers of Table N14.1
        publish the counter of a produce, then publish the completion
    append the IO event and, for a fire, the COMM to events, in the order of play
    return the receipt of play

PUBLISH-DENIAL(ticket, o):                   ▷ publish_denial
    publish ticket.prepare(o) under a publication guard
    denied ← true                            ▷ NATIVE-APPLY-OP keeps the epoch open
    fail with OutOfPhlogistons               ▷ operations 0 to i - 1 stay applied (decision 19, D6)
```

Each `require` fails with a mismatch in the class of the mismatch of the session: `SpaceTxnError::Host` with an `InterpreterError`.
So a failed space operation fails the certification as a failed host operation does, and decision 47 adds no error variant.
The session logs no event, as for host operations, and the backend gives the events of play only to the event correlation of the commit.
Native play charges no host work for the source, the locks and the produce counter of a space operation, so the backend charges none either.
The ticket protocol charges its own verification.

`sweep_volatile` (`native_session/space.rs:286-318`) and `root_leaf_record_count` (`native_session/space.rs:277-283`) make the steps of the replay backend over the session store and the session history.
Phase P7 enables the temp and seq qualifiers, so P7 runs the sweep of native replay for the first time.

#### N14.3 The wiring

*Table N14.2. The wiring of decision 47.*

| Step | Code | Rule |
|---|---|---|
| W1, the accessor | `accounting/native_runtime/checked_operations/trace/replay/session/execution.rs:39-66` | `Session::space_store` returns a `ParSpaceStore` over `NativeSpaceHost`. Its resolver asks the budget for the introduction authority, as `NativeExecution` does for a host operation. |
| W2, the bound state | `reified/runtime.rs:68-73`, `reified/runtime.rs:241-258` | The mode `NativeReplay` holds a slot that the environment binds once, with the store and the budget. An unbound slot fails with `RuntimeUnavailable`. |
| W3, the bind call | `rho_runtime.rs:1918-1934` | `create_native_replay_env` binds the runtime right after it builds the session. |
| N1, native play | `reified/runtime.rs:194-211` | `ReifiedRuntime::host` no longer fails for a native budget. The guard of decision 46 stays in a comment that gives the reason. |

[Table N6.1](#n6-scheduling) gives the three modes of the runtime.
`NativeReplayEnvironment::restore` keeps the same store across evaluations, so `begin_evaluation_scope` forwards to the bound store (FR-5').
The test `space_native_scope_reset_after_restore` checks the reset.
The observer path of a replay directive does not change ([decision 19](decision-register.md#decision-19), D4).
No production observer overrides `replay_operation_directive` (`rspace_interface.rs:74`), so legacy replay keeps the fail-closed rule of [S3](#s3-host-equivalence).

#### N14.4 Why native replay ends

Two facts make every native replay of a space deploy end: the order of the space transactions, and the [epoch closure](glossary.md#epoch-closure).

**An honest journal.** Assume that the journal is the journal of native play.

1. The per-space key $`[2] \mathbin{\Vert} \mathit{sid}`$ of [I-11](decision-register.md#i-11) puts every transaction of one space into one conflict group ([N6](#n6-scheduling)). The group runs its transactions one after another, in operation order, in play and in replay.
2. Each predecessor of a space row is one of three rows:
   - an earlier operation of the same transaction, which `apply_op` published before the next operation began
   - a row of an earlier transaction of the same space, which ran earlier in the same group
   - the first-round row of a retry link ([decision 43](decision-register.md#decision-43)), which also belongs to an earlier transaction of that space
3. So every predecessor is complete when a space row begins, and `begin_operation` returns a ticket.
4. A host operation waits for the readiness of its row before it takes stripes. A space transaction never waits, because a row that is not ready fails closed.

No operation therefore holds a stripe while it waits, so no cycle of waits can form [[CES71](#ref-ces71)].
The native key $`[2, 0]`$ of native execution (DR-78, DR-79) puts each frontier into one conflict group.
It is a second guarantee, and the argument does not use it.

**A forged journal.** A forged journal can add a channel to the footprint of a space row.
The journal check derives the predecessors of each row from the footprints, so it accepts the forgery when the forger derives the predecessors again.
A later host row that touches the added channel then waits for the space row.
The space row fails closed at its footprint check (rule [NE-4](specification.md#8-native-execution)).
A failed operation that only drops its ticket returns the row to the available state, and the host row then waits forever.

The epoch closure removes this case.
When a space operation fails, `apply_op` closes the epoch of the session.
Every waiting operation then wakes and fails with `Closed`, and the certification fails closed.
A recorded denial is the one exception: it publishes its row, and play continues after it, so the epoch stays open.
Before the closure, the forged-journal probe found 64 hangs among 109 forgeries of a space row ([dossier 13.7](verification.md#137-the-forged-journal-probe-and-the-epoch-closure)).
The host path of native replay has the same lost wake-up, and [finding F-47-4](findings/native-execution-cost-accounting.md#6-f-47-4-a-failed-host-operation-leaves-its-waiters-waiting) records it for its owner.

`NativeReplay.tla` checks the invariants `NoBlockedReadiness` and `NoDeadlock` over every interleaving of the groups.
It checks them with the honest journal and with every footprint-consistent forgery of a space row.
The controls `NoSpaceKeyUnsafe`, `WaitUnderLocksUnsafe` and `NoInvalidateUnsafe` each refute exactly one of the two invariants ([dossier 13.3](verification.md#133-tla-model-nativereplaytla)).

#### N14.5 Shapes

Every shape of a space operation is a sequence of staged produces and consumes, so each staged operation has one ticket.
Table N14.3 gives the tickets of each shape.

*Table N14.3. The tickets of each shape.*

| Shape | Staged operations | Tickets |
|---|---|---|
| Store (bag, set, cell, queue, stack, PQ, Array, Vector) | a produce on D | one `Stored` |
| Park | a consume on the group | one `Stored` |
| Fire, produce side | a produce with a fire | one `Matched` |
| Fire, consume side (one bind or a join) | a consume with a fire | one `Matched` |
| Counter bump ([M-1](decision-register.md#m-1), decisions 31 and 49) | the consume of the counter, the produce of the new value | `Matched`, `Stored` |
| Rule E store again ([decision 45](decision-register.md#decision-45)) | a produce with the identity of the record | one `Stored`, or `Matched` if it fires |
| Set class retirement, `K_retire`, `K_lift` | a consume with a fire | one `Matched` |
| `K_retire` over `[D; n]` ([decision 28](decision-register.md#decision-28)) | one consume over a repeated group | one `Matched`, with a deduplicated footprint |
| PathMap delivery | a produce of the vehicle on I(q) with a fire | one `Matched` |
| PathMap store | a produce on D(p), then one produce for each marker | one `Stored` each |
| PathMap token bump ([decision 51](decision-register.md#decision-51)) | a counter bump on S(sid) | `Matched`, `Stored` |
| Factory | the config on C(sid) (persistent), S(sid) = 0, the handle park on H(sid) (persistent) | three `Stored` |
| `read_config` | none: the job drops its transaction | none |
| Array or Vector allocation | the take of A(sid) as a fire, the put of A(sid) as a store | `Matched`, `Stored` |
| First operation on a collection | the take and put of S(sid), then the put of S(sid, k) | `Matched`, `Stored`, `Stored` |
| Persistent element offered again in flight ([decision 30](decision-register.md#decision-30), DV-3) | a produce with the same identity | one ticket with a retry link |
| Handle call | a host produce on H(sid) | a host row whose predecessor is the park of the factory |
| Settle job, or a re-issue at the cap (decisions 43 and 49) | a new intent with its own order | its own tickets |
| PathMap escalation | the first transaction stages nothing | none |

The suite `rholang/tests/reified_native_spaces.rs` runs these shapes through the full certification pipeline at the v6 limits ([dossier 13.6](verification.md#136-the-native-certification-suite)).
Each budget attempt of native replay reserves about 1.2 MB of `VerificationBytes`, so the v6 limit bounds a certified deploy near 190 attempts.
The [measurement M-47-1](findings/native-execution-cost-accounting.md#7-m-47-1-verification-bytes-of-native-replay) records this bound for the owner of cost accounting.

### N15. Phase P6: the VectorDB family

Phase P6 adds the families `hashmap:vectordb` and `vector:vectordb`.
A record of such a space can carry a vector, and a receive selects records by a similarity query ([specification, section 3.9](specification.md#39-vectordb)).
This section describes the exact numeric core, the records and the selection, the driver, the join engine, the factory, `rho:lang:vector` and native execution.
Section [N16](#n16-phase-p6-the-chromadb-search-backend) describes the ChromaDB search backend.
The [dossier](verification.md#14-phase-p6-step-2-the-exact-numeric-core) gives the proofs, the models, the tests and the measurements, from section 14 on.

The design record is design v3 of the P6 refresh, which [decision 66](decision-register.md#decision-66) approves.
Every consensus decision of the family compares exact numbers ([decision 54](decision-register.md#decision-54)).
A float enters with its exact value, and a delivered value is one rounding of an exact value.

#### N15.1 The number model

The module `rholang/src/rust/interpreter/tensor/` holds the exact numeric core.
It has no dependency on a linear-algebra crate, and it computes with big integers and reduced rationals.
Figure N15.1 shows the path of a number from the input to a delivered value.

*Table N15.1. The exact numeric core.*

| Part | Code | Rule |
|---|---|---|
| Ingress | `number_of`, `tensor/num.rs:243-292` | `GInt`, `GBigInt` and `GBool` are dyadic numbers $`m \cdot 2^{0}`$. A `GDouble` or a `GFloat32` enters with the exact value of the float, also a dyadic number. A `GBigRat` or a `GFixedPoint` is a reduced rational, and it is dyadic when its denominator is a power of two. |
| Vectors | `vector_of`, `tensor/num.rs:325-360` | A vector is dyadic when every element is dyadic, and rational otherwise. Its length must be the dimension of the space. |
| Kernels | `dot`, `distance2` and `manhattan`, `tensor/exact.rs:265-348` | A dyadic kernel sums integers at one common exponent. A rational kernel sums reduced rationals. No partial sum is rounded. |
| Scores | `score`, `tensor/exact.rs:429-462` | The score of a record keeps exact values: the dot product and the squared norm for `cosine`, the exact distance for `euclidean` (squared) and `manhattan`, and counts for `hamming` and `jaccard`. |
| Order | `better`, `tensor/exact.rs:478-521` | Two scores compare exactly. `cosine` compares squared forms after the signs. |
| Bounds | `within`, `tensor/exact.rs:525-574` | `threshold` and `radius` compare exactly with the score. |
| Delivery | `delivered`, `tensor/exact.rs:578-599` | A delivered value is $`\mathrm{RN}_{53}`$ of an exact value: the `f64` nearest to it, with ties to even. |
| Roots | `sqrt_to_f64`, `tensor/num.rs:484-511` | $`\mathrm{RN}_{53}(\sqrt{x})`$ from an exact integer square root of a scaled numerator, with no loop. |

The cosine order needs no square root.
For a query $`q`$ and two records $`e`$ and $`e'`$, let $`u = q \cdot e`$ and $`v = q \cdot e'`$.
When $`u`$ and $`v`$ have one sign, the order of the cosines is the order of these exact products, reversed for negative signs:

```math
\frac{u}{\lVert q \rVert \, \lVert e \rVert} \;\le\; \frac{v}{\lVert q \rVert \, \lVert e' \rVert}
\quad\Longleftrightarrow\quad
u^{2} \, \lVert e' \rVert^{2} \;\le\; v^{2} \, \lVert e \rVert^{2} \qquad (u, v \ge 0)
```

The Rocq theorem `cosine_order_squared_form` of `Similarity.v` proves this form.
A delivered cosine is the one rounding of the exact root, with the sign of $`u`$:

```math
\cos(q, e) \;=\; \operatorname{sign}(u) \, \sqrt{\frac{u^{2}}{\lVert q \rVert^{2} \, \lVert e \rVert^{2}}}
```

Every decode, ingress, kernel step, comparison, sort step, list construction and copy reserves its work on the space meter before it runs ([decision 42](decision-register.md#decision-42)).
The reservations live in `tensor/costs.rs`, so a host-work rejection is a deterministic outcome in play and in replay.

![Activity diagram of the number model. INGRESS turns a numeric Par with one expression and no connective into its exact value: GInt, GBigInt and GBool are dyadic numbers m times 2 to the 0; GDouble and GFloat32 enter with the exact value of the float as a dyadic number m times 2 to the e, NaN and the infinities fail with NonFiniteValue, and the zero -0 enters as 0; GBigRat and GFixedPoint are reduced rationals n over d, dyadic when d is a power of two, and a fixed-point scale above 4,096 fails with ValueOutOfRange. A vector is dyadic when every element is dyadic, else rational, and a wrong length fails with DimensionMismatch. KERNELS compute dot, the squared euclidean distance and manhattan exactly: dyadic vectors sum integers at one common exponent, rational vectors sum reduced rationals, and hamming and jaccard apply to binary vectors only. DECISIONS compare exact values: cosine by squared forms with the signs first, u squared times the squared norm of e prime against v squared times the squared norm of e; a lower distance is better; threshold and radius are exact bounds; equal scores keep the lower canonical rank. DELIVERY rounds once: RN 53 of x, the nearest f64 with ties to even, for dot, manhattan, hamming and jaccard; RN 53 of the square root of x for cosine and euclidean, from an exact integer square root of a scaled numerator, with the mantissa rounded by the square of the odd midpoint; a value beyond the finite range of f64 fails with ValueOutOfRange. rho:lang:vector replies add, scale and dot in the widest kind of the inputs: GInt or GBigInt for integers, GBigRat for a rational or a fixed point, GDouble with one rounding for a float; norm and distance reply a GDouble with the correctly rounded root.](diagrams/33-number-model.svg)

*Figure N15.1. Source: [diagrams/33-number-model.puml](diagrams/33-number-model.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

#### N15.2 Records, queries and the selection

A datum of a VectorDB space keeps envelope version 1 and fills its embedding slot (`rholang/src/rust/interpreter/spaces/record.rs:1548-1555`).
The record is `[tag_D, 0, Nil, embedding]` followed by the payload, where the embedding is the vector of the record or `Nil`.
The order slot holds 0, because the family has no counter, and the priority slot holds `Nil`.

A parked waiter keeps the canonical map of its query in its policy slot.
The map holds the query vector under `q` and one entry for each named param.
For a VectorDB waiter, the decoder of the policy accepts only `Nil` or a non-empty map of the keys of `VDB_QUERY_KEYS`.
The map must equal its own re-encoding.

`vdb_record` builds the record of a send (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:635-657`).
The vector is the param `embedding`, or the payload when the payload is one non-empty list of numbers, or none.
`parse_query` checks a query in a fixed order, so a query with several faults reports one code (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:404-511`).

The selection is a function of the records, the query and the canonical order.
Figure N15.2 shows it, and the reference algorithm follows it.

```text
SELECT(Q, records, flight):                       ▷ vectordb.rs, eligible and ranked
    order ← CANONICAL(records + flight)           ▷ DR-75: source hash, then digest
    E ← []
    for each record r in order:
        if Q.ids ≠ ⊥ and id(r) ∉ Q.ids: continue  ▷ id(r) = the hash of the source produce
        if Q.q ≠ ⊥:
            s ← SCORE(Q, vector(r))               ▷ ⊥: no vector, zero cosine norm, not binary
            if s = ⊥ or not WITHIN(s, Q.bound): continue
        append (r, s) to E
    if Q.q ≠ ⊥: STABLE-SORT E by BETTER on s      ▷ equal scores keep the canonical order
    return E

RECEIVE-SELECTION(W, E):                          ▷ receive_selection and list_selection
    if W is in list mode:
        L ← the first min(k, |E|) records of E, or all of E for results = "all"
        if L ≠ [] and W binds VALUES(L) and the guard of W admits: fire over [D; |L|]
        else: park W with its query               ▷ LM-b
    else:
        r ← the first record of E whose payload W binds
        if r ≠ ⊥ and the guard admits r: fire over [D]   ▷ the guard is checked once
        else: park W with its query
```

![Activity diagram of the selection of a VectorDB query. ORDER: the records of the channel and the element in flight of an offer are sorted by the hash of the source produce, then by the full digest, the order of DR-75 of decision 41. FILTER: for each record in that order, a record whose id is not among the ids of the query is skipped; with a query vector, a record whose score is undefined is skipped, because it has no embedding, a zero norm for cosine, or a record that is not binary for hamming and jaccard; a record whose score fails the threshold or the radius is skipped; every other record is kept in E with its exact score. RANK: a query with a vector stably sorts E by the better exact score, with cosine by squared forms and distances ascending, and equal scores keep the canonical order; without a vector, E keeps the canonical order and the receive is a list read of the records. SELECT: in list mode with k or results all, L is the first min of k and the size of E records of E, and the one bind binds the list of their values, a pair of score and value for each one with scores true; when L is not empty, the pattern binds L and the guard admits, which is checked once by decision 37, the receive fires in one COMM over D repeated m times, and otherwise it parks with the query as the policy under LM-b. In single mode, the first record of E whose payload the pattern binds fires in one COMM over D when the guard admits it, checked once, and otherwise the receive parks with the query as the policy.](diagrams/28-vectordb-selection.svg)

*Figure N15.2. Source: [diagrams/28-vectordb-selection.puml](diagrams/28-vectordb-selection.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

#### N15.3 The driver

`VectorDbDiscipline` runs its own driver (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:156-276`).
The family has no head, so the head methods of the trait `Discipline` keep the queue answers, and only the head driver would ask them.
A receive runs RECEIVE-VDB, a send runs OFFER-VDB, and both end with the tail of the transaction.
One fire count runs through the transaction, the tail included ([decision 49](decision-register.md#decision-49)).

```text
RECEIVE-VDB(W):                                   ▷ vectordb.rs, receive_plan
    E ← SELECT(W.query, records, ⊥)
    fire or park W by RECEIVE-SELECTION(W, E)
    if the fire peeks: owe each removed non-persistent record   ▷ Rule E, the last owe first
    FINISH-TAIL

OFFER-VDB(x):                                     ▷ vectordb.rs, offer_vdb
    for each parked waiter W in canonical order, from the first untested one:
        if W takes x (single: eligible and binds; list: x ∈ L(W, R + x) and binds; plain: binds)
           and the guard of W admits:
            fire W over [D] with x
            if W does not peek: K_retire each other non-persistent member of its list
            step ← AFTER-FIRE(spent: x not persistent and W does not peek, stays: W persistent)
            Done → x settles; DivergentPair or RoundCap → x stops; Continue → next waiter
    no waiter took x: CHECK-CAPACITY; store x with its vector

TAIL:                                             ▷ vectordb.rs, run_tail
    loop:
        if a record is owed:
            if fired = FIRE_ROUND_CAP: owe it again; stop with a settle job
            OFFER-VDB(the last owed record)       ▷ a stop owes it again and stops
        else if no record left the collection: return
        else:
            W ← the first parked list waiter whose L(W, R) is not empty, binds, and passes
            if W = ⊥: return
            if fired = FIRE_ROUND_CAP: stop with a settle job
            LIFT-AND-REOFFER(W)                   ▷ K_lift the first member v, fire W with v
```

FINISH-TAIL runs the tail after a fire that settled the element and after a divergent pair (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:1772-1786`).
After a stop at the round cap, it stops at once only when a record is owed or a record left the collection.
`conclude` then re-issues one settle job with the owed records, or the operation when its element is still in flight ([decision 43](decision-register.md#decision-43)).
A settle job of a VectorDB collection runs in the join engine, which offers the owed records first and then always cascades.

LIST-CASCADE closes the gap of the counterexamples CE-1 and CE-2 of the arbitration.
In CE-1, a list waiter parks on an empty collection, and a later send makes its list complete.
In CE-2, a removal moves a parked list waiter onto a list that its pattern binds.
Each removal therefore makes the tail look for a parked list waiter that can fire.
The fire lifts one member of the list and offers it to the waiter.
The theorem `vdb_no_pending_match` (S22) proves that no parked waiter without a guard can fire when a transaction ends.

![Activity diagram of list fires and LIST-CASCADE of a VectorDB collection. OFFER-VDB of the element x of a send checks the parked waiters in canonical order: a single-mode waiter W takes x when x is eligible for its query, its pattern binds x and its guard admits; a list waiter takes x when x is in the list L of W over R plus x, its pattern binds that list and its guard admits. When no waiter takes x, CHECK-CAPACITY fails with CapacityExceeded on a full collection, and otherwise x is stored with its embedding and settles. A fire binds x over D; a waiter that does not peek retires each other non-persistent member of its list with one K_retire COMM each, and a peek takes no datum. Then x settles when it is spent, that is not persistent with a waiter that does not peek; x stops when W is persistent, a divergent pair, and the operation re-issues at the end by decision 43; x stops at the round cap; otherwise x is offered to the next waiter. FINISH-TAIL: when x stopped at the round cap, the transaction stops with one settle job that carries the owed second sends and the resume of x when a second send is owed or a record left, and otherwise the transaction ends and x re-issues. TAIL, the drain and cascade: an owed second send is owed again with a stop at the round cap, and otherwise the last owe is popped and offered by OFFER-VDB with its own identity under Rule E, where a stop owes it again; when no record left the collection, the tail ends; otherwise FIRST-CASCADE-TARGET finds the first parked list waiter in canonical order whose list over R is not empty, binds and passes the guard; with no such waiter the tail ends; at the round cap a settle job carries the cascade; otherwise LIFT-AND-REOFFER lifts the first member v with K_lift when v is not persistent and fires W with v, and a peek waiter owes v as a second send when it was lifted by decision 30, while a waiter that does not peek retires the other non-persistent members of the list.](diagrams/29-vectordb-list-cascade.svg)

*Figure N15.3. Source: [diagrams/29-vectordb-list-cascade.puml](diagrams/29-vectordb-list-cascade.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

#### N15.4 The join engine

A VectorDB collection that a join reaches runs on the join engine ([N12](#n12-wave-w3-of-p5-joins-of-one-space)).
The discipline names the window shape `JoinWindow::Similarity`, so each bind of a join sees its own eligible records in its own order.
`Engine::similarity_bind` searches them for a single-mode bind, and a bind without a query reads the canonical order (`rholang/src/rust/interpreter/spaces/join.rs:3403-3471`).
A receive with one bind on a joined collection takes the list paths of the engine in list mode.

A multi-bind join with a bind in list mode fails with `ListModeInJoin`, because a list is one value of one bind (`rholang/src/rust/interpreter/spaces/join.rs:412-471`).
A removal never makes a single-mode join waiter without a guard fireable, because eligibility is a property of each record.
So single-mode joins need no cascade, and the theorem `vdb_join_removal_exposes_nothing` (S21) proves it.
The engine runs LIST-CASCADE for its list waiters, and a settle job of a VectorDB collection always cascades.

#### N15.5 The factory, the config and merges

The factory rows of both families take `capacity`, `dimensions`, `metric` and `mirror` (`rholang/src/rust/interpreter/spaces/factory/defaults.rs:63-102`).
Gate 9 of `FactoryConstructor::validate` refuses `mirror` unless the qualifier is the default one and the metric is `cosine`, `dot` or `euclidean` (`rholang/src/rust/interpreter/spaces/factory/constructor.rs:229-260`).
`vdb_config` reads the params of each operation from the config record, with each read reserved first (`rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:293-307`).

The family has no counter (`USES_COUNTER = false`), so two branches that write one collection merge with no conflict ([M-1](decision-register.md#m-1)).
`check_capacity` compares the records of the view of the transaction with `capacity` before each store (`rholang/src/rust/interpreter/spaces/generic.rs:1118-1135`).
A merge can therefore exceed `capacity`, and every store then fails until removals bring the count below it ([decision 71](decision-register.md#decision-71)).
The test `space_collection_conflicts_vectordb_capacity_soft_bound` pins the soft bound, and `capped_writes_soft_bound` of `CounterConflict.v` proves it.

#### N15.6 `rho:lang:vector`

`rho:lang:vector` computes `add`, `scale`, `dot`, `norm` and `distance` with the same core (`rholang/src/rust/interpreter/tensor/ops.rs:234-272`).
`vector_operand` reads the widest kind of the elements of each vector, and `number_par` encodes an exact value in a kind (`rholang/src/rust/interpreter/tensor/ops.rs:401-428`).
The kind of a reply is the widest kind of the inputs: an integer, a rational, or a float ([decision 66](decision-register.md#decision-66)).
For the float kind, the reply is one rounding of the exact value, so `dot` rounds its exact sum once.
`VectorOps.v` proves this rule (`vector_replies_exact`, S27) and the midpoint test of the roots of `norm` and `distance` (`root_midpoint_nearest`, S28).

#### N15.7 Native execution

Every VectorDB plan stages only through the transaction context `Cx`, and it reserves each step on the meter of the context.
So native play and native replay reserve the same amounts and replay the same journal rows ([decision 47](decision-register.md#decision-47)).
The certification suite plays and replays every shape of the family at the v6 limits ([dossier, section 16](verification.md#16-phase-p6-step-5-native-certification-of-the-vectordb-shapes)).
At these limits a natively certified deploy holds only a small collection.
In play, 8 records and 8 list waiters use 0.54 of the limit of `VerificationBytes`.
The growth is quadratic in the size of the collection, because each operation loads all records or all waiters of its collection.

### N16. Phase P6: the ChromaDB search backend

ChromaDB is the search backend of a VectorDB space, and never a space of its own ([decision 52](decision-register.md#decision-52)).
A VectorDB space has three layers.
The frontend is the factory, the sends with `embedding=` and the receives with queries.
RSpace is the store of the consensus records, with the state root, replay, merges and LFS.
ChromaDB holds an optional mirror of the records of each node, and it answers advisory queries.

An answer of ChromaDB reaches consensus only as a recorded output of `ann`, and the exact engine checks it through the query param `ids=`.
So consensus never reads the mirror, and a stale or missing mirror can only give weaker advice ([decision 58](decision-register.md#decision-58)).
This section describes the services of a node, the mirror path, the recorded processes and the node setup.
The [specification](specification.md#10-the-chromadb-mirror) states the rules, and [`docs/rholang/chromadb.md`](../../rholang/chromadb.md) explains how to run a node with a mirror.

#### N16.1 The services of a node

`VectorDbServices` holds the embedder, the mirror backend and the writer of a node (`rholang/src/rust/interpreter/reified/vectordb_services.rs:547-553`).
The node installs its services once, and every process and every job reads them through `current`.
Without an install, `current` gives the unavailable services: an embedder and a mirror that refuse every call, and no writer.

*Table N16.1. The services and their backends.*

| Part | Code | Rule |
|---|---|---|
| `Embedder` | `vectordb_services.rs:190-196` | One finite vector for each text, or a failure. |
| `MirrorBackend` | `vectordb_services.rs:209-267` | The calls of the mirror: query, watermark, collections, registration, upsert, delete, ids and metadata. |
| `node_services` | `vectordb_services.rs:698-708` | With the `chromadb` feature: the SBERT embedder, and the ChromaDB mirror of the environment of the node with a writer. Without the feature: the unavailable services. |
| `SbertEmbedder`, `ChromaMirror` | `vectordb_services.rs:721-1182` | The model `all-MiniLM-L6-v2` with 384 dimensions, and the REST client of the pinned server. |
| `MirrorWriter` | `vectordb_services.rs:455-499` | One task, a bounded queue of 1,024 batches, 3 attempts, and dirty marks. |

The processes and the writer never see a backend type, only the two traits.
So the tests run fake backends in the sandbox, and the live leg runs the real backends against a disposable server (`scripts/verify-reified/chroma-live.sh`).

#### N16.2 The mirror path

The mirror path has five steps, and Figure N16.2 shows them.
Each step runs outside the consensus transition: no step charges phlo, logs an event or fails a deploy.

```text
CAPTURE(receipt):                                 ▷ spaces/mirror.rs, MirrorCapture::writes
    for each record id in the receipt, by its first and its last event:
        (store, store)   → upsert, when the record has a vector finite as f32
        (remove, remove) → delete
        (store, remove), (remove, store) → nothing   ▷ a record that comes and goes, and Rule E
    the journal of the evaluation keeps the last write of each id

SETTLE(result):                                   ▷ reified/runtime.rs, settle_mirror_journal
    batches ← take the journal: one batch for each collection
    if result is Ok and has no errors: PUBLISH(batches)     ▷ M1
    the start of the next root evaluation clears a journal that no result settled

PUBLISH(batches):                                 ▷ vectordb_services.rs, MirrorWriter::publish
    for each batch: try_send to the queue; a full queue marks the collection dirty

APPLY(batch):                                     ▷ the task of the writer, in queue order
    up to 3 attempts: make the collection, register its channels, upsert, delete
    a batch that still fails marks the collection dirty

PASS(block L, root R):                            ▷ reified/mirror_reconciler.rs, reconcile
    take the dirty marks; make each dirty collection and register its channels
    for each collection c: if watermark(c) < L or c is dirty:
        for each registered channel: delete the orphans, add the missing records of R
        on success: watermark(c) ← max(watermark(c), L); on failure: keep it, mark c again
```

A deploy can make several transactions on one collection.
The journal keeps the last write of each record over all of them.
So a record that one transaction stores and a later one removes leaves a delete.
That delete names an id that the mirror never received, and ChromaDB accepts it with no effect.

![Activity diagram of the ChromaDB mirror of a VectorDB space. CAPTURE runs after each commit of a VectorDB transaction, only for a space with mirror chroma: the net writes take the first and the last event of each record id, so a record that is stored and stays is an upsert when it has an embedding, a non-persistent removed record is a delete, and a record that is stored and removed, or removed and stored again by Rule E, writes nothing; the journal of the evaluation keeps the last write of each id, with no meter, no error path and no await. SETTLE, M1: when the final result has no errors, at the settle points of interpreter.rs and of the native path of rho_runtime.rs, the journal becomes one batch for each collection, and the bounded queue of 1,024 batches takes it with try_send, so the deploy goes on at once, or the collection is marked dirty and the deploy never waits; any other result drops the journal, and the start of each root evaluation also drops a journal that no result settled, so an exploratory evaluation never publishes. WRITER TASK: one batch at a time in publication order, APPLY makes the collection with the distance of the metric, registers its channels, upserts and deletes; when an attempt of three succeeds, with pauses of 100 ms and 200 ms, the mirror holds the writes, and otherwise the collection is marked dirty with its channels and a part of the batch can stay applied. RECONCILER: BlockFinalised gives the post-state root of the block, and a newer block replaces one that no pass took yet; the pass takes the dirty marks and prepares each one by making the collection and registering its channels; when the mirror does not list its collections, the dirty collections are marked dirty again and the pass ends; for each collection, a failed PREPARE fails the pass for it and it stays dirty, a collection at the watermark of the block that is not dirty is skipped as current, and any other collection is repaired: each registered channel is set to the finalized state, orphans deleted and missing records added; when every call of the repair succeeds, the watermark becomes the larger of the block and the old watermark, and otherwise the watermark is kept and a dirty collection is marked dirty again.](diagrams/30-mirror-journal-reconciler.svg)

*Figure N16.2. Source: [diagrams/30-mirror-journal-reconciler.puml](diagrams/30-mirror-journal-reconciler.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

#### N16.3 Recorded embeddings

`rho:vectordb:embed` turns texts into vectors with the embedder of the node (`rholang/src/rust/interpreter/reified/system_processes.rs:395-411`).
The model can differ between nodes, so the vector of a text is never a function of consensus state.
The dispatcher therefore records the whole reply of each call on the produce event of the reply, a failure reply included.
Replay produces the recorded reply and asks no embedder, and Figure N16.3 shows both runs.
A program then sends the vector with `embedding=`, and from then on the vector is consensus state.

![Sequence diagram of rho:vectordb:embed in play and in replay. In play on the proposer, the program calls embed with the reply channel and a text or a list of texts; the process checks the shape, one text or a list of texts, else the reply BadArgument, and the quota of at most 64 texts and 64 KiB, else the reply EmbeddingRateLimited; it asks the embedder of the node, all-MiniLM-L6-v2 with 384 dimensions, for the texts, and the embedder returns one vector of f32 values for each text; the process produces the reply true, vector, model or true, list of vectors, model on the reply channel, and the dispatcher records the whole reply, a failure reply included, because the process is a recorded process of non_deterministic_ops. The program receives the reply and sends docs with the text and embedding equal to the vector, and OFFER-VDB offers the record with its embedding, so it fires a waiter or the space stores it, consensus state from now on. In replay on another node, the same call produces the recorded reply on the reply channel; replay asks no embedder, so two nodes with other models compute the same state; the program receives the recorded reply and makes the same send with the same embedding.](diagrams/31-recorded-embeddings.svg)

*Figure N16.3. Source: [diagrams/31-recorded-embeddings.puml](diagrams/31-recorded-embeddings.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

#### N16.4 The advisory query

`rho:vectordb:ann` asks the mirror of the node for the records of a channel that are nearest to a query (`rholang/src/rust/interpreter/reified/system_processes.rs:485-511`).
Its checks read consensus state, so play and replay both make them ([decision 67](decision-register.md#decision-67)).
In native execution each check is a journaled operation, and a replay that skipped one would fail its certification with an incomplete journal.
Only the answers of the services are recorded: the embedding of a text query, the neighbours and the watermark.

The ids of the reply are hints.
A program passes them to a receive with `ids=`, and the exact engine keeps only the stored records whose source hash is among them.
A stale id names no record, so it selects nothing, and the exact order decides among the other records.
Figure N16.4 shows a call in play and in replay.

![Sequence diagram of rho:vectordb:ann in play and in replay. In play on the proposer, the program calls ann with the reply channel, the space, the channel, the query and the options map with k equal to 5. The checks of decision 67 run in play and in replay: the process reads the config record of the space, reserved on the budget of the deploy, and gets the family, the params, the dimensions and the capacity; it checks a native VectorDB space, else NotAVectorDbSpace, with a mirror, else MirrorDisabled; a text or a numeric vector of the dimensions with finite f32 values, else DimensionMismatch or BadArgument; and a map of the one key k, an Int from 1 to the capacity, else InvalidOption. When the query is a text, the embedder and mirror of the node embed the text into one vector of the dimensions. The process asks the mirror for the collection, the channel digest, the vector and k, and gets the ids and distances of at most k neighbours; it asks the watermark of the collection and gets the finalized block of the last repair that finished. It produces the recorded reply true, ids, distances, watermark, or false MirrorUnavailable, on the reply channel. The program receives the reply and runs a receive on docs with the query vector, ids equal to the ids and k equal to 1; the exact engine of the VectorDB space keeps only the stored records whose id is in ids, a stale id names no record and selects nothing, and the exact order decides among the others. The ids are hints: only the exact query param ids turns them into a decision, so a stale mirror gives weaker advice and never changes consensus state. In replay on another node, the same call makes the same checks with the same host work and produces the recorded reply; replay asks no embedder and no mirror, and the program makes the same exact query with the same decision.](diagrams/32-advisory-query.svg)

*Figure N16.4. Source: [diagrams/32-advisory-query.puml](diagrams/32-advisory-query.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

#### N16.5 The node and the docker environment

The node starts the reconciler with one spawn call in `setup.rs` ([decision 61](decision-register.md#decision-61), `node/src/rust/runtime/setup.rs:685-723`).
The call passes two read-only closures.
One maps a block hash to the post-state root of the block, and one gives a runtime at a root.
The reconciler starts only when the installed services have a writer, so a node without the `chromadb` feature starts no task.

`docker/shard.yml` pins the server image by its digest, under the Compose profile `chromadb`.
Each node gets the endpoint `CHROMA_ENDPOINT` and a database of its own, `CHROMA_DATABASE`, so no node writes the mirror of another node.
A node built without the `chromadb` feature ignores these entries.

## References

<a id="ref-bbg95"></a>[BBG95] H. Berenson, P. Bernstein, J. Gray, J. Melton, E. O'Neil, and P. O'Neil, "A critique of ANSI SQL isolation levels," in *Proceedings of the 1995 ACM SIGMOD International Conference on Management of Data*, ACM, 1995, pp. 1–10. [doi:10.1145/223784.223785](https://doi.org/10.1145/223784.223785)

<a id="ref-ces71"></a>[CES71] E. G. Coffman, M. Elphick, and A. Shoshani, "System deadlocks," *ACM Computing Surveys*, vol. 3, no. 2, pp. 67–78, 1971. [doi:10.1145/356586.356588](https://doi.org/10.1145/356586.356588)

<a id="ref-eglt76"></a>[EGLT76] K. P. Eswaran, J. N. Gray, R. A. Lorie, and I. L. Traiger, "The notions of consistency and predicate locks in a database system," *Communications of the ACM*, vol. 19, no. 11, pp. 624–633, 1976. [doi:10.1145/360363.360369](https://doi.org/10.1145/360363.360369)

<a id="ref-gel85"></a>[Gel85] D. Gelernter, "Generative communication in Linda," *ACM Transactions on Programming Languages and Systems*, vol. 7, no. 1, pp. 80–112, 1985. [doi:10.1145/2363.2433](https://doi.org/10.1145/2363.2433)

<a id="ref-knu84"></a>[Knu84] D. E. Knuth, "Literate programming," *The Computer Journal*, vol. 27, no. 2, pp. 97–111, 1984. [doi:10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97)

<a id="ref-lam94"></a>[Lam94] L. Lamport, "The temporal logic of actions," *ACM Transactions on Programming Languages and Systems*, vol. 16, no. 3, pp. 872–923, 1994. [doi:10.1145/177492.177726](https://doi.org/10.1145/177492.177726)

<a id="ref-mr05"></a>[MR05] L. G. Meredith and M. Radestock, "A reflective higher-order calculus," *Electronic Notes in Theoretical Computer Science*, vol. 141, no. 5, pp. 49–67, 2005. [doi:10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016)

<a id="ref-pap79"></a>[Pap79] C. H. Papadimitriou, "The serializability of concurrent database updates," *Journal of the ACM*, vol. 26, no. 4, pp. 631–653, 1979. [doi:10.1145/322154.322158](https://doi.org/10.1145/322154.322158)
