# Reified RSpaces design

This document describes how the campaign builds Reified RSpaces in the node.
Each phase of the campaign adds a section, and the [README](README.md#campaign-phases) lists the phases.
Phase P1 writes the [substrate](#substrate) section, which describes the storage layer of spaces in `rspace++`.
Later phases add the space agents, the routing, the theory seam, and the other parts.

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
| FR-10 | A peek fire does not store the sent value, and the agent re-offers it ([decision 30](decision-register.md#decision-30)). | [S5.8](#s58-peek-re-offer-fr-10) |

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
| [Retention](glossary.md#retention) | A peek fire keeps the stored records that it binds in place. |
| [Re-offer](glossary.md#re-offer) | The second produce of a datum that a peek fire did not store. |
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
| 1 | Run the callback `before_observe` of the agent with the produce identity. It takes the place of `register_introduction_authority` in the reducer. | `rholang/src/rust/interpreter/reduce.rs:535-539` | The same, with the consume identity. | `rholang/src/rust/interpreter/reduce.rs:647-651` |
| 2 | `start_accounting_operation` with the channel and its join groups. | `rspace/ops_produce.rs:43-52` | `start_accounting_operation` with the channels and no joins. | `rspace/ops_consume.rs:44-45` |
| 3 | `observe_produce`. | `rspace/ops_produce.rs:77` | `observe_consume`. | `rspace/ops_consume.rs:73-81` |
| 4, Store or Park | `log_produce`, which logs the event and steps the counter of a non-persistent produce. Then `store_data`, which calls `put_datum`. | `rspace/ops_produce.rs:105-106`, `212-226`, `rspace/trace_log.rs:158-172` | `log_consume`, then `store_waiting_continuation`: `put_continuation`, and then `put_join` for each channel. | `rspace/ops_consume.rs:146-147`, `179-190` |
| 5, Fire | `observe_comm` with the COMM whose counter closure counts the produce itself. | `rspace/ops_produce.rs:185-198` | `observe_comm` with the COMM of the plain counters. | `rspace/ops_consume.rs:126-135` |
| 6, Fire | `log_produce`, then `log_comm` with `PRODUCE_COMM_LABEL`. | `rspace/ops_produce.rs:199-200` | `log_consume`, then `log_comm` with `CONSUME_COMM_LABEL`. | `rspace/ops_consume.rs:136-137` |
| 7, Fire | Remove a non-persistent continuation at its live index. | `rspace/ops_produce.rs:202-205` | None. | |
| 8, Fire | `remove_matched_datum_and_join`: in descending index order, remove each stored non-persistent record, and then the fired join of its channel. A retaining fire removes only the joins. | `rspace/ops_produce.rs:207`, `228-248` | `store_persistent_data`: in descending index order, remove each stored non-persistent record. A retaining fire removes nothing. | `rspace/ops_consume.rs:138`, `192-202` |
| 9, Fire | The receipt from `wrap_result`. A retained stored result is reported persistent ([S5.8](#s58-peek-re-offer-fr-10)). | `rspace/ops_consume.rs:204-229` | The same. | `rspace/ops_consume.rs:204-229` |
| 10 | `observe_operation_finish` with `Stored`, `Matched`, or `Rejected`. | `rspace/ops_produce.rs:61-64` | The same. | `rspace/ops_consume.rs:55-58` |

The play store steps `store_data`, `remove_matched_datum_and_join`, `store_waiting_continuation`, and `store_persistent_data` are private to their sibling modules.
So `space_play.rs` restates them statement for statement over the public `HotStore` API (decision 19, D7).
The host-parity tests compare the results with the host.

Table S3.2 gives the replay steps of `ReplayBackend` in `replay_rspace/space_replay.rs`, over the host `ReplayRSpace`.
Its HEAD columns cite `replay_rspace.rs`.

| Step | Produce | HEAD | Consume | HEAD |
|---|---|---|---|---|
| 1 | Run `before_observe`, as in Table S3.1. | `rholang/src/rust/interpreter/reduce.rs:535-539` | The same, with the consume identity. | `rholang/src/rust/interpreter/reduce.rs:647-651` |
| 2 | `start_accounting_operation` with the channel and its join groups. | `1053-1059` | `start_accounting_operation` with the channels and no joins. | `760-761` |
| 3 | A replay directive of the observer fails the operation with `NativeReplayDirectiveUnsupported`. The host would run the operation natively instead. | `1084-1100` | The same. | `788-805` |
| 4 | `observe_produce`. | `1101` | `observe_consume`. | `806-814` |
| 5 | `increment_produce_counter`, before any match. | `1103`, `1488-1494` | None. | |
| 6, Store or Park | `log_produce` to the logger, then `store_data`. | `1126-1127`, `1170-1171`, `1579-1593` | `log_consume` to the logger, then `store_waiting_continuation`, which also raises the estimate of waiting continuations. | `852-853`, `896-904`, `914-922`, `1561-1577` |
| 7, Fire | `observe_comm`. | `1312` | `observe_comm`. | `944-950` |
| 8, Fire | `log_produce`, then `log_comm` with the label `comm.produce`. | `1314-1321` | `log_consume`, then `log_comm` with the label `comm.consume`. | `952-966` |
| 9, Fire | Remove a non-persistent continuation. In both cases, call `mark_replay_waiting_continuation_match`. | `1323-1329` | None. | |
| 10, Fire | `remove_matched_datum_and_join`, or only the fired joins when the fire retains. | `1331`, `1707-1727` | `store_persistent_data`, unless the fire retains. | `968`, `1595-1605` |
| 11, Fire | `remove_bindings_for` the COMM. | `1332`, `1407-1435` | The same. | `969` |
| 12, Fire | The receipt from `wrap_result`. Its produce is the copy that the paired COMM recorded. | `1150-1161`, `1679-1705` | The receipt from `wrap_result`. | `971` |
| 13 | After a failed fire, `rollback_produce_counter`. | `1163-1165`, `1496-1509` | None. | |
| 14 | `observe_operation_finish`. | `1068-1071` | The same. | `771-774` |

Host replay asserts that the COMM it computes is in the recorded list, and it panics otherwise (`replay_rspace.rs:936-943`, `1306-1311`).
The substrate checks the COMM at staging instead, and a missing COMM fails with the typed error `ReplayDivergence` ([S5.3](#s53-verify-fr-3)).

Table S3.3 gives the reporting steps.

| Step | Reporting | HEAD |
|---|---|---|
| Host | `ReportingRspace` implements `SpaceTxnHost` with the mode `Reporting`. It delegates `begin`, `root_leaf_record_count`, and `sweep_volatile` to its inner replay space, the field `replay_rspace`. | `reporting_rspace.rs:118` |
| Calls | The steps of Table S3.2, through `ReplayBackend`. | `replay_rspace.rs`, as Table S3.2 cites it |
| Report | The inner replay space carries the `ReportingLogger`. So the calls `log_produce`, `log_consume`, and `log_comm` of steps 6 and 8 add the events `ReportingProduce`, `ReportingConsume`, and `ReportingComm` to the report, in commit order. | `reporting_rspace.rs:159-182`, `466-554` |

**Differences by decision.** The substrate differs from the host in five places, and a decision covers each one.

1. Retention. A peek fire keeps its stored records in place, and the host removes them and the reducer produces them again. The stores then differ only by these kept records (decision 19, headline H11). Ordinary channels never meet this rule, because only agents reach storage channels.
2. Directives. A native replay directive fails the operation closed ([I-13](decision-register.md#i-13), decision 19 D4).
3. Divergence. A missing COMM is a typed error at staging, not a panic at commit.
4. Candidate choice. The agent chooses the continuation and the candidates, and the substrate validates any choice. The host takes the first match of its deterministic shuffle.
5. Consume-side pairing. A consume-side fire pairs only under its trigger key, which is stricter than host replay ([S10](#s10-limits)).

The incoming datum of a produce-side peek fire is not a difference.
Neither the host nor the substrate stores it: the host reducer produces it again, and the agent re-offers it ([S5.8](#s58-peek-re-offer-fr-10)).

### S4. Counter timing

Replay must reproduce the times-repeated map of each COMM, so play and replay must count a fired produce at the same point.
The host does this with two different rules.
Play computes a produce-side COMM before `log_produce` steps the counter, so its closure adds one for the produce itself (`rspace/ops_produce.rs:185-191`).
Replay steps the counter before it matches (`replay_rspace.rs:1104`), so its plain counters already count the produce.
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
The peeks of the waiting continuation must be empty, or they must cover every bind, which means retention.
Candidate $`j`$ must bind channel $`j`$ of the group.
Exactly one candidate must have the index $`-1`$ and carry the produced record: its channel, its source, its data, and its persist flag.

```text
        group ← ADMIT-GROUP(channels, |wk.patterns|, Joined)     ▷ S5.5
        require key ∈ group                                      ▷ InvalidOperation
        retain ← PEEK-RETENTION(|channels|, wk.peeks)            ▷ MixedArrowJoin
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
Last, it applies the effects to the view in the host order.
The fire never stores the incoming datum ([S5.8](#s58-peek-re-offer-fr-10)).

```text
        LOAD-GROUP(group)                                        ▷ NonCanonicalChannelOrder
        require a continuation of view.conts[group] is byte-equal to wk   ▷ NotInView
        CHECK-STORED-CANDIDATES(group, candidates)               ▷ NotInView
        positions ← CLAIM-POSITIONS(group, candidates)
        comm ← COMM::new(candidates, wk.source, wk.peeks, t)
            with t(p) = κ_v(p) + [¬persist ∧ p = src]            ▷ CounterOverflow
        recorded ← VERIFY(comm, Produce(src))                    ▷ ReplayDivergence
        δ′ ← NEXT-DELTA(src, persist)                            ▷ CounterOverflow
        removed ← REMOVED-RECORDS(candidates, positions, retain)
        effects ← APPLY-FIRE-TO-VIEW(group, wk, candidates, removed)
        if δ′ ≠ none: δ(src) ← δ′
        if recorded ≠ none: paired ← paired ++ [comm]
        append (the operation with its fire, before_observe, effects)
        return (index, Produce(src), comm)
```

The outline of a consume has two named steps.

```text
STAGE-CONSUME(channels, patterns, k, persist, peeks, outcome, before_observe):
    require the transaction is open                              ▷ Finished
    group ← ADMIT-GROUP(channels, |patterns|, Consumed)          ▷ S5.5
    retain ← PEEK-RETENTION(|channels|, peeks)                   ▷ MixedArrowJoin
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
These are the list rules of `put_continuation` and `put_join` (`hot_store.rs:660-723`, `957-980`).

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
A retaining fire removes nothing.

```text
        require |candidates| = |group|, and candidate j binds group[j]    ▷ InvalidOperation
        require every candidate has an index of 0 or more         ▷ InvalidOperation
        for each key ck of group: LOAD-DATA(ck)
        CHECK-STORED-CANDIDATES(group, candidates)               ▷ NotInView
        positions ← CLAIM-POSITIONS(group, candidates)
        comm ← COMM::new(candidates, src, peeks, κ_v)            ▷ CounterOverflow
        recorded ← VERIFY(comm, Consume(src))                    ▷ ReplayDivergence
        removed ← REMOVED-RECORDS(candidates, positions, retain)
        effects ← REMOVE-RECORDS(group, candidates, removed)
        if recorded ≠ none: paired ← paired ++ [comm]
        append (the operation with its fire, before_observe, effects)
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

REMOVED-RECORDS(candidates, positions, retain):
    if retain: return []
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

The peeks rule decides retention:

```rust
/// Peeks rule of an operation with `binds` channels: `Ok(false)` for no
/// peeks, `Ok(true)` for `{0..binds-1}` (retention), `MixedArrowJoin`
/// otherwise.
pub(crate) fn peek_retention(binds: usize, peeks: &BTreeSet<i32>) -> Result<bool, SpaceTxnError> {
    let all_binds = || i32::try_from(binds).is_ok_and(|binds| peeks.iter().copied().eq(0..binds));
    match peeks.is_empty() {
        true => Ok(false),
        false if all_binds() => Ok(true),
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
    run the observed body of the host operation, and record its events
    finish the accounting operation with Stored, Matched, or Rejected
```

A commit without an operation context is not tracked, and it emits no sub-orders.
A failure of operation $`i`$ leaves the operations $`0`$ to $`i - 1`$ applied, as the equivalent host program would ([prefix atomicity](glossary.md#prefix-atomicity)).
The deploy then fails, and the soft checkpoint of the deploy removes the applied operations.
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

![Commit of a space transaction as an activity. The commit refuses a finished transaction with Finished, takes the locks and the registration, and clears the paired COMM events and the counter delta. It refuses a swapped hot store with StoreSwapped and a reused parent order with ParentOrderReused, both with no effect. Then a loop runs each staged operation i in staging order under sub_order(parent, i): the read-only checks (joins unchanged, counter room, live COMM equal to the staged COMM, removed entries at their staged positions), the before_observe callback, the observer start, in replay the directive check, the observe call, in replay the counter step, and then the outcome: Store, Park, or Fire with observe_comm, the produce or consume event and the COMM event, the continuation removal, the record removal unless the fire retains, and in replay the removal of the bindings. The observer finish follows, and then the event correlation. An error in operation i leaves the operations before it applied. At the end the commit releases the locks and then the registration and returns the receipts.](diagrams/06-space-txn-commit.svg)

*Figure S5.2. Source: [diagrams/06-space-txn-commit.puml](diagrams/06-space-txn-commit.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

Figure S5.2a shows the lifecycle of a transaction as a state diagram.
It is the figure 06a of the diagram plan.

![Lifecycle of a space transaction as a state machine. A call of SpaceStoreHandle::begin first registers the current task, and a second open transaction or sweep of the same task fails with NestedTransaction and takes no lock. Empty lock keys fail with InvalidOperation. The transaction then acquires the phase-A stripes and then the phase-B stripes, captures the hot store and the installed set, and becomes Open. In the Open state the agent reads the staged view and stages operations. A rejected staging leaves the transaction open with no effect. A failed view change after validation closes it with Inconsistent and releases its locks. A drop releases the locks and the registration with no effect. A commit moves to Committing, which ends in Committed with the receipts, or in Failed with no effect for StoreSwapped and ParentOrderReused, or with the operations before the failing one applied. Every later call on a closed transaction fails with Finished.](diagrams/06a-space-txn-lifecycle.svg)

*Figure S5.2a. Source: [diagrams/06a-space-txn-lifecycle.puml](diagrams/06a-space-txn-lifecycle.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

#### S5.3. Verify (FR-3)

In replay and in reporting, staging verifies each fire against the recorded bindings before it applies the fire to the view.
Play has no recorded log, so it skips this step.
The lookup uses only the trigger key of the fire.
The trigger key is `Produce(src)` for a produce-side fire and `Consume(src)` for a consume-side fire.
Host replay reads the same key for a produce (`replay_rspace.rs:1112-1123`), and for a consume it also reads an index of all COMM events ([S10](#s10-limits)).

Before replay, `rig` binds one recorded occurrence of a COMM under each IO event of the COMM that the log holds (`replay_rspace.rs:390-450`).
A produce that occurs twice in the produce list of the COMM gives two bindings under its key.
So a key holds $`\lfloor b / m \rfloor`$ recorded occurrences of the COMM, where $`m`$ is the [binding multiplicity](glossary.md#binding) of the key.
An occurrence is free while the transaction has not paired it:

```math
\mathit{free}(\mathit{comm}, \mathit{key}) \;=\; \Bigl\lfloor \frac{b}{m} \Bigr\rfloor - \bigl\lvert \{\, c \in \mathit{paired} : c = \mathit{comm} \,\} \bigr\rvert
```

```text
VERIFY(comm, trigger):
    if mode = Play: return none
    (recorded, b) ← the binding of comm under trigger in the replay table   ▷ ReplayDivergence when absent
    m ← BINDING-MULTIPLICITY(trigger, comm)
    available ← b div m, or 0 when m = 0
    if the copies of comm in paired < available: return recorded
    fail ReplayDivergence(comm)
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
    fn verify<B: TxnBackend<C, P, A, K>>(
        &self,
        backend: &B,
        comm: &COMM,
        trigger: &IOEvent,
    ) -> Result<Option<COMM>, SpaceTxnError> {
        match self.mode {
            SpaceMode::Play => Ok(None),
            SpaceMode::Replay | SpaceMode::Reporting => {
                let divergence = || SpaceTxnError::ReplayDivergence(Box::new(comm.clone()));
                let (recorded, count) = backend
                    .recorded_binding(trigger, comm)
                    .ok_or_else(divergence)?;
                let available = match binding_multiplicity(trigger, comm) {
                    0 => 0,
                    multiplicity => count / multiplicity,
                };
                let paired = self.paired.iter().filter(|paired| *paired == comm).count();
                match paired < available {
                    true => Ok(Some(recorded)),
                    false => Err(divergence()),
                }
            }
        }
    }
```

The literal rule of the design pass compared $`b`$ with the paired copies, without the division.
For a COMM that lists one produce twice, that rule pairs one recorded occurrence two times.
The Rocq lemma `design_verify_overpairs` (`EventCorrelation.v:4287`) proves this over-pairing, and the test `space_store_handle_replay_cross_channel_comm_pairs_once` pins the corrected rule.

The commit removes all bindings of a paired COMM with `remove_bindings_for` (Table S3.2, step 11).
A binding that no operation pairs fails the deploy at `check_replay_data` with `UnusedCommEvent`, as in host replay (`replay_rspace.rs:452-468`).
So a park where the log recorded a fire is not an error at once, because the binding may belong to a later produce.

Two lookups help an agent to choose the same outcome as play:

- `recorded_comms(source)` returns the free occurrences of the COMM events under the key of `source`, one entry for each occurrence, sorted by bincode bytes. It returns `None` in play.
- `admits(comm, datum, incoming)` tells whether replay accepts `datum` for `comm`. It is the host rule `matches` and `was_repeated_enough_times` (`replay_rspace.rs:1256-1271`), evaluated over the staged counters. It returns `true` in play.

The admission rule follows, where the flag $`\mathit{inc}`$ holds for the incoming datum of the produce that the agent matches:

```math
\mathrm{admits}(\mathit{comm}, d, \mathit{inc}) \iff d.\mathit{source} \in \mathit{comm}.\mathit{produces} \;\wedge\; \bigl( d.\mathit{persist} \;\vee\; \mathit{times}(d.\mathit{source}) = \kappa_v(d.\mathit{source}) + [\mathit{inc}] \bigr)
```

#### S5.4. Sweep

The sweep removes the [volatile channels](glossary.md#volatile-channel) of a deploy at its end ([decision I-4](decision-register.md#i-4)).
It removes their data, their continuations, and their join entries.
The host offers no such operation.
`ISpace::remove_all_continuations` removes continuations only (`rspace/ispace_impl.rs:170-178`), and `HotStore::remove_continuation` leaves the join entries (`hot_store.rs:758-801`).
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
At the next checkpoint, `changes` emits a delete action for each of them (`hot_store.rs:1055-1110`).
None of these keys has a leaf in the trie, because validation refused any key with a leaf.
A radix delete of an absent key returns "not found" (`history/radix_tree.rs:1584-1608`), and `clearing_delete_actions` drops such actions (`history/radix_tree.rs:1646-1663`).
So the new root is the root of the durable changes alone (headline H15).

The replay sweep lowers the estimate of waiting continuations once for each continuation that it removes, as host continuation removal does (FR-8).
A sweep matches nothing, so it counts no match.

#### S5.5. Canonical order (FR-1')

The hot store keys a continuation list by its channel list in the order of the caller (`hot_store.rs:611-658`, `660-723`).
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
The Rocq lemma `sorted_unique_canonical` (`EventCorrelation.v:5937`) proves this, also when the list repeats a key.
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
| Each channel of a consume | `Consumed`: both phases | `consume_lock` (`rspace/ispace_impl.rs:249-253`, `striped_locks.rs:74-82`) |

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

Native phlo recording accepts one RSpace operation for each operation order (`rholang/src/rust/interpreter/accounting/native_runtime/operations.rs:211-212`).
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
The host gets this effect in two steps: RSpace removes the peeked data, and the reducer produces each non-persistent result again (`produce_peeks`, `rholang/src/rust/interpreter/reduce.rs:973-1008`).
[Decision 30](decision-register.md#decision-30) gives spaces the same behavior for the incoming datum.
It keeps [retention](glossary.md#retention) for the stored records that a peek binds ([decision 19](decision-register.md#decision-19)).

The substrate keeps four rules:

1. A produce-side fire never stores its incoming datum, in the staged view and at commit, in every mode.
2. A peek fire keeps the stored records that it binds in place. This holds on the consume side, and on the produce side for joins and repeated-channel groups.
3. The receipt reports a retained stored result as persistent, and the incoming result keeps its own persist flag. So the non-persistent results of a peek fire are exactly the incoming datum of a non-persistent send.
4. Staging accepts a second produce with the same identity in one transaction, and each produce steps the counter by one.

The receipt rule of item 3:

```rust
/// The results of a fire that its receipt reports as retained: when the fire
/// retains (peeks), the results of its stored candidates. The incoming
/// candidate of a produce fire is never retained, because the fire does not
/// store it (FR-10).
pub(crate) fn retained_results<C, A: Clone>(
    candidates: &[ConsumeCandidate<C, A>],
    retain: bool,
) -> Vec<bool> {
    candidates
        .iter()
        .map(|candidate| retain && candidate.datum_index >= 0)
        .collect()
}
```

When the send and the fired waiter are both non-persistent, the agent re-offers the datum as the next operation of the same transaction.
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
The lemma `offer_p1_breaks_no_pending_match` (`EventCorrelation.v:3202`) shows the break.
The lemma `offer_keeps_no_pending_match` (`EventCorrelation.v:3164`) shows that the plan with the re-offer keeps the invariant.

The re-offer registers its introduction authority under the identity of the peek fire.
The registry accepts an equal authority for an identity that it holds (`rholang/src/rust/interpreter/accounting/mod.rs:959-964`).
It refuses another authority with an identity conflict, and the commit then fails at the re-offer, with the peek fire applied.

The stores of the host and of the substrate differ only by the stored records that a peek keeps in place.
The host reducer also produces these records again, and the substrate keeps them instead.
On each channel, the re-produces that the substrate replaces send exactly the values of the kept records (headline H11, part 3).

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
| `Host(e)` | commit | The `before_observe` callback or an observer call rejects operation $`i`$, for example with `OutOfPhlogistons`, `HostWorkRejected`, or an identity conflict of the introduction authority. | Prefix. Operation $`i`$ makes its observer calls up to the rejection, but it logs no event and changes no store entry. A failed replay fire rolls its counter back. |
| `Host(e)` | count, sweep validation | The history reader fails. | None. |
| `InvalidOperation` | begin, stage | The lock keys are empty. A group is empty, or its length differs from the patterns. The candidates do not line up with the channels. The produce channel is not in the fired group. A produce fire has not exactly one incoming candidate, or it does not carry the produced record. A consume fire has an incoming candidate. | None. |
| `InvalidOperation` | any step | A channel or a record cannot be serialized with bincode. | None, or prefix when a commit check of operation $`i`$ raises it. |
| `MixedArrowJoin` | stage | The peeks are neither empty nor all binds $`\{0, \dots, n - 1\}`$. | None. |
| `NotInView` | stage | The waiting continuation of a produce fire is not in the view. A stored candidate does not name its record byte for byte. A fire binds a non-persistent record twice. | None. |
| `InstalledChannel` | read, stage, sweep | A touched channel belongs to an installed system process. | None. |
| `StoreSwapped` | commit | A reset or a revert replaced the hot store between `begin` and `commit`. | None. |
| `ReplayDivergence` | stage | In replay or reporting, the COMM has no free recorded occurrence under its trigger key. | None. |
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

### S7. Locking and ordering

A transaction takes its stripes in `begin`, through the host `consume_lock` (`rspace/locks.rs:24-29`, `replay_rspace.rs:107-109`).
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
A causal path compares as its list of segments in lexicographic order, and an operation order compares its session first (`operation_context.rs:138-165`).
The path $`p`$ is a proper prefix of each $`Q_i`$, so $`P < Q_i`$, and $`Q_i < Q_j`$ exactly when $`(i, 2) < (j, 2)`$, which is when $`i < j`$.

Now let $`R`$ be another emitted order of the session.
If $`R`$ lies strictly between $`P`$ and $`Q_{n-1}`$, then $`R`$ must extend $`p`$, because $`P`$ and $`Q_{n-1}`$ agree on every segment of $`p`$.
The scheduler emits only paths of the form $`u \mathbin{+\!\!+} [(k, 0)]`$, where the participant path $`u`$ alternates split segments $`(k', 1)`$ and branch segments $`(b, 0)`$ (`rholang/src/rust/interpreter/deterministic_reduction.rs:79-116`).
Such a path never continues after its final operation segment, so no scheduler path extends $`p`$, and none has the tag 2.
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

![Checkpoint, replay and sweep as a sequence. Play on the proposer: the reducer starts the evaluation scope, the space agent opens a transaction through the handle, reads the staged view, stages operations and commits; the commit makes the observer calls and logs the events under sub_order(parent, i) and writes the hot store. At the end of the deploy the reducer sweeps the volatile channels, which removes their data, continuations and joins with no event. The checkpoint turns the hot store changes into history actions; the deletes of the swept keys are absent from the trie and change nothing, so the root covers the durable state only. Replay on the validator: rig binds each recorded COMM under its logged IO events, the scope starts, the agent stages the same operations, verify pairs each fire under its trigger key with the binding multiplicity, the commit removes the bindings, the sweep runs at the same point, check_replay_data finds no unused binding, and the checkpoint gives the same root. A divergent fire fails at staging with ReplayDivergence and no effect.](diagrams/07-checkpoint-replay-sweep.svg)

*Figure S7. Source: [diagrams/07-checkpoint-replay-sweep.puml](diagrams/07-checkpoint-replay-sweep.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

### S8. What is unchanged

P1 keeps every item of the consensus-preservation checklist of the plan that concerns ordinary execution.
No production path calls the substrate in P1, and the footprint policy `p1` pins every other source file at the baseline.
Table S8 maps the checklist items to the reason and to the gate that shows it.

| Item | Checklist item | Why P1 keeps it | Gate |
|---|---|---|---|
| 1 | The COMM produce sort is unchanged. | The substrate builds every COMM with `COMM::new`, and `trace/` is frozen. | R1, footprint `p1`, legacy golden corpus |
| 2 | The host never retains peeked or matched-persistent data. | The host code is unchanged, and retention is a rule of spaces only. | R2, the six HEAD peek tests |
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
| 21 | `contract_call.rs`, `dispatch.rs`, `ispace_impl.rs`, and the cost-accounting module are byte-identical. | The policy `campaign`, which `p1` extends, freezes them. | Footprint `p1` |
| 22 | The parser is a conservative extension of HEAD parsing. | Every repository source normalizes to the bytes of HEAD. | `ordinary_source_normalization_unchanged` |

Items 18 to 20 concern the space machinery of P4 to P7, which P1 does not add.
The sweep already supports item 20.
It refuses keys with a leaf in the history trie, and joins that also name a channel that it keeps.

The gates are these tests and checks:

- **R1 to R9.** `rholang/tests/reified_ordinary_regression.rs` compares nine aspects of ordinary execution with a corpus captured at HEAD (`test-vectors/ordinary-execution-v1.json`). The aspects are the COMM sort, peeks, greedy joins, leaf bytes, event bytes, cost vectors, term bytes, a state dump, and play-replay equality.
- **Legacy golden corpus.** `rspace++/tests/legacy_golden_corpus.rs` checks channel hashes, event identities, the empty radix root, and a non-empty root with its event against `test-vectors/legacy-golden-corpus-v1.json`.
- **Source normalization.** The test `ordinary_source_normalization_unchanged` in `rholang/tests/reified_source_normalization.rs` parses and normalizes every repository source, and it compares the bytes with the capture of HEAD (`test-vectors/ordinary-source-normalization-v1.json`).
- **Footprint policy `p1`.** The script `scripts/verify-reified/check-footprint.py` compares the pinned files with the SHA-256 digests of the base revision (`test-vectors/head-file-digests-6f7a80da3.json`). It allows only the edits of Table S1.1.
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

**Rocq headlines.** Table S9.1 lists the 22 headlines of P1.
Each one closes under the global context, with no axiom, no parameter, and no admitted proof.

| Id | Headline | Citation | Statement | Property tests |
|---|---|---|---|---|
| H1 | `commit_trace_eq_host` | `EventCorrelation.v:4612` | A commit makes the observer calls, events, and status of the host run of the same operations. | `prop_space_txn_play_trace_equals_host`, `prop_space_txn_repeated_channel_groups_equal_host` |
| H2 | `commit_state_eq_host` | `EventCorrelation.v:4633` | A commit leaves the store of the host run, with data lists equal up to permutation. | `prop_space_txn_play_root_equals_host`, `prop_space_txn_repeated_channel_groups_equal_host` |
| H3 | `counter_timing_play_replay` | `EventCorrelation.v:5091` | The play closure and the replay rule give the same COMM counters. | `prop_counter_timing_play_equals_replay` |
| H4 | `verify_sound_complete` | `EventCorrelation.v:5154` | Verify pairs a COMM exactly when the log holds the trigger event and a free occurrence remains. `recorded_comms` reports the same copies. | `prop_space_txn_replay_divergence_typed_and_inert`, `prop_space_txn_replay_requires_trigger_event` |
| H5 | `replay_pairs_each_comm_once` | `EventCorrelation.v:5358` | Replay pairs each recorded COMM occurrence at most once. | `prop_space_txn_replay_pairs_play_log`, `prop_space_txn_repeated_channel_groups_equal_host`, `prop_space_txn_peek_retention` |
| H6 | `view_commit_coherent` | `EventCorrelation.v:4664` | The staged view equals the committed store. | `prop_space_txn_view_equals_committed_store` |
| H7 | `joins_lockstep_preserved` | `EventCorrelation.v:4687` | Join entries stay in lockstep with the waiting continuations. | `prop_space_txn_joins_lockstep` |
| H8 | `sub_order_between` | `EventCorrelation.v:6301` | Sub-orders follow the parent order, precede its successors, and are contiguous and unique. | `prop_sub_orders_contiguous_and_disjoint` |
| H9 | `abort_inert` | `EventCorrelation.v:6023` | A drop, an access error, and a commit after a store swap change no store, binding, observer call, or event. | `prop_space_txn_abort_inert`, `prop_space_txn_non_canonical_inputs_fail_closed` |
| H10 | `coverage_total` | `EventCorrelation.v:4974` | Every store change of a commit has its event. | `prop_event_coverage_holds_for_generated_commits` |
| H11 | `peek_retention_trace_eq_host` | `EventCorrelation.v:4732` | With the re-offer as the next operation, a commit makes the calls, events, status, and bindings of the host run. The stores differ only by the kept stored records. | `prop_space_txn_peek_retention` |
| H12 | `reporting_trace_is_replay_trace` | `EventCorrelation.v:6097` | Reporting stages and commits as replay does. | `prop_space_txn_reporting_equals_replay`, `prop_space_txn_peek_retention` |
| H13 | `sweep_exact` | `VolatileSweep.v:876` | The sweep removes exactly the volatile data, continuations, and joins. Through the handle, a sweep in a task with an open transaction fails closed. | `prop_sweep_volatile_exact_and_root_neutral` |
| H14 | `delete_absent_identity` | `VolatileSweep.v:953` | A history delete of an absent key changes nothing. | `prop_sweep_volatile_exact_and_root_neutral` |
| H15 | `sweep_root_neutral` | `VolatileSweep.v:1248` | The checkpoint after a sweep gives the trie of the durable changes alone. | `prop_sweep_volatile_exact_and_root_neutral` |
| H16 | `leaf_count_is_length` | `VolatileSweep.v:1311` | The root leaf record count is the length of the data list. | `prop_root_leaf_record_count_matches_history` |
| H17 | `non_canonical_fails_closed` | `EventCorrelation.v:5965` | A group that is not non-decreasing fails before any store or cache effect. The sorted list is the only canonical member of its permutation class. | `prop_space_txn_non_canonical_inputs_fail_closed`, `prop_space_txn_repeated_channel_groups_equal_host` |
| H18 | `canonical_reads_root_neutral` | `VolatileSweep.v:1426` | Reads of canonical keys leave the checkpoint root unchanged. | `prop_space_txn_reads_root_neutral` |
| H19 | `directive_fails_closed` | `EventCorrelation.v:6137` | A replay directive rejects the operation with no effect. | `prop_space_txn_replay_directive_fails_closed` |
| H20 | `parent_order_single_txn` | `EventCorrelation.v:6574` | Sub-orders are unique in an evaluation scope, and each reset admits the same parent again. | `prop_space_txn_parent_order_single_commit` |
| H21 | `view_identity_is_store_identity` | `EventCorrelation.v:6695` | View lookups agree with store lookups by store identity. | `prop_space_txn_store_identity_ignores_ord_only_fields`, `prop_space_txn_par_identity_ignores_locally_free` |
| H22 | `checkpoint_rejects_duplicate_key_actions` | `VolatileSweep.v:1610` | The checkpoint refuses two actions on one history key, and canonical cache keys have distinct history keys. | `prop_space_txn_non_canonical_inputs_fail_closed` |

The models are [EventCorrelation.v](../../../formal/rocq/reified_rspaces/theories/EventCorrelation.v), with H1 to H12, H17, and H19 to H21, and [VolatileSweep.v](../../../formal/rocq/reified_rspaces/theories/VolatileSweep.v), with H13 to H16, H18, and H22.

**TLA+ invariants.** Table S9.2 lists the invariants of the two TLA+ modules [[Lam94](#ref-lam94)].
TLC explores the complete state space of each safe configuration.
Apalache checks the inductive forms `IndInv` and `LockIndInv` with a base step and an induction step.

| Id | Invariant | Module | Safe configurations | Negative controls |
|---|---|---|---|---|
| T1 | `ReplayPrefixEqual` | `CheckpointReplay.tla` | `MC_CheckpointReplay.cfg`, `MC_CheckpointReplay_TwoBlock.cfg` | `UuidUnsafe.cfg` |
| T2 | `SameRootIffSameState` | `CheckpointReplay.tla` | The same | `RootIgnoresVolatileUnsafe.cfg` |
| T3 | `PairedOnce` | `CheckpointReplay.tla` | The same, and Apalache through `IndInv` | `DoublePairUnsafe.cfg`, `DoublePairUnsafe_IndStep_Apalache.cfg` |
| T4 | `NoVolatileResidue` | `CheckpointReplay.tla` | The same, and Apalache through `IndInv` | `JoinResidueUnsafe.cfg`, `JoinResidueUnsafe_IndStep_Apalache.cfg` |
| T5 | `Serializable` | `TxnLockCoverage.tla` | `MC_TxnLockCoverage.cfg`, and Apalache through `LockIndInv` | `UncoveredChannelUnsafe.cfg`, `UnjoinedChannelUnsafe.cfg`, and their Apalache step forms |

The modules are [CheckpointReplay.tla](../../../formal/tlaplus/reified_rspaces/CheckpointReplay.tla) and [TxnLockCoverage.tla](../../../formal/tlaplus/reified_rspaces/TxnLockCoverage.tla).
Each configuration file starts with a line that states its bounds.
For example, `MC_TxnLockCoverage.cfg` covers two channels on two stripes, the groups that repeat a channel, two transactions, and two host operations.

**Property tests.** The invariant map names 24 properties.
Twenty-three are in [reified_space_store_handle_props.rs](../../../rspace++/tests/reified_space_store_handle_props.rs), and `prop_space_txn_par_identity_ignores_locally_free` is in [reified_space_store_handle_par.rs](../../../rholang/tests/reified_space_store_handle_par.rs), over real `Par` channels.
Each property runs 64 cases with a fixed seed and without failure persistence, so a run is repeatable and writes nothing to the source tree.
Where a property allows it, the generators also make groups that are not canonical, which the substrate must reject with no effect.
They also make sorted groups that repeat a channel, which the substrate must accept.

**Deterministic suites.** Table S9.3 lists the other suites of the substrate.

| Suite | Tests | Subject |
|---|---|---|
| [reified_space_store_handle.rs](../../../rspace++/tests/reified_space_store_handle.rs), group A | 22 | Host parity of every operation shape, checked against a twin host. |
| Group B | 5 | Checkpoints, history read-through, and a round trip through an LMDB directory that is closed and opened again. |
| Group C | 10 | Replay lookup parity with host replay. |
| Group D | 7 | The volatile sweep. |
| Group E | 3 | Reporting delegation. |
| Group F | 15 | The review regressions: canonical order, store identity, trigger-key pairing, lock coverage, one commit per parent order, nested begin, sweep bookkeeping, and peeks in replay. |
| Group G | 10 | The round-2 rules: repeated channels, parent tracking per scope, the cost of the commit checks, and the nested guard of the sweep. |
| Group H | 7 | The round-3 rule: the peek re-offer, the queue scenario, host parity of the re-offer, replay, reporting, and the receipt flags. |
| Unit tests in `space_store_handle.rs`, `space_play.rs`, and `space_replay.rs` | 18 | The order of sub-orders, the list rules of the view, the event correlation, the peeks rule, the store identity, canonical groups, lock stripes, the parent tracker, the registration, claimed positions, receipt flags, counter overflow, and the replay estimate of the sweep. |
| `rholang/tests/reified_space_txn_host_bounds.rs` | 1 | The `Par` instance of `SpaceTxnHost` for the three hosts. |
| `rholang/tests/reified_space_store_handle_par.rs` | 3 | The dedup of `TaggedContinuation`, the store identity of `Par` channels that differ only in `locally_free`, and the property `prop_space_txn_par_identity_ignores_locally_free`. |

**Negative controls.** Each control shows that a check or a proof depends on the rule that it covers:

- The script [rocq-negative-controls.py](../../../scripts/verify-reified/rocq-negative-controls.py) runs 24 mutants of the Rocq models. Eleven cover FR-1 and FR-1', six cover FR-5', one covers FR-9', and six cover FR-10. Each mutant must fail to compile inside its expected lemma.
- Six unsafe TLC configurations each refute exactly their invariant, and four Apalache step controls each break their inductive invariant.
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

**Consume-side pairing is stricter than host replay.** Host replay finds the COMM of a consume also through `consume_replay_index` (`replay_rspace.rs:856-893`).
`rig` fills that index from every COMM of the log, also from a COMM whose Consume event the log lacks (`replay_rspace.rs:410-420`).

The substrate pairs a consume-side fire only under `Consume(src)`.
So for a log without the Consume event, host replay pairs the fire, and the substrate fails it with `ReplayDivergence` and no effect.
An honest log always holds this event, because a consume-side fire logs its consume in the same operation (Table S3.1, step 6).
The test `space_store_handle_replay_requires_consume_trigger_event` pins both behaviors.

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
- Path depth. Each sub-order adds one path segment, so a transaction at the native limit of path segments fails native recording (`rholang/src/rust/interpreter/accounting/native_runtime/operations.rs:181`).
- Record count. `root_leaf_record_count` counts the records in the outer frame of the leaf, through `get_data_proj_binary`, with no change of the history modules (decision 19, D5).
- Late divergence. A park where the log recorded a fire surfaces only at `check_replay_data`, as in host replay ([S5.3](#s53-verify-fr-3)).

## References

<a id="ref-ces71"></a>[CES71] E. G. Coffman, M. Elphick, and A. Shoshani, "System deadlocks," *ACM Computing Surveys*, vol. 3, no. 2, pp. 67–78, 1971. [doi:10.1145/356586.356588](https://doi.org/10.1145/356586.356588)

<a id="ref-eglt76"></a>[EGLT76] K. P. Eswaran, J. N. Gray, R. A. Lorie, and I. L. Traiger, "The notions of consistency and predicate locks in a database system," *Communications of the ACM*, vol. 19, no. 11, pp. 624–633, 1976. [doi:10.1145/360363.360369](https://doi.org/10.1145/360363.360369)

<a id="ref-gel85"></a>[Gel85] D. Gelernter, "Generative communication in Linda," *ACM Transactions on Programming Languages and Systems*, vol. 7, no. 1, pp. 80–112, 1985. [doi:10.1145/2363.2433](https://doi.org/10.1145/2363.2433)

<a id="ref-knu84"></a>[Knu84] D. E. Knuth, "Literate programming," *The Computer Journal*, vol. 27, no. 2, pp. 97–111, 1984. [doi:10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97)

<a id="ref-lam94"></a>[Lam94] L. Lamport, "The temporal logic of actions," *ACM Transactions on Programming Languages and Systems*, vol. 16, no. 3, pp. 872–923, 1994. [doi:10.1145/177492.177726](https://doi.org/10.1145/177492.177726)

<a id="ref-mr05"></a>[MR05] L. G. Meredith and M. Radestock, "A reflective higher-order calculus," *Electronic Notes in Theoretical Computer Science*, vol. 141, no. 5, pp. 49–67, 2005. [doi:10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016)
