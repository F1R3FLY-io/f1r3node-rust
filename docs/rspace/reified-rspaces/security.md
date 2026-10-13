# Reified RSpaces security

This document states the threat model of Reified RSpaces and the controls of the node against each threat.
It names the assets, the adversaries and the trust boundaries.
It gives each control with the input that it rejects and the evidence that checks it.
The [specification](specification.md) states the rules that the controls enforce, and the [agent API](agent-api.md) states the codes of the aborts.
The [decision register](decision-register.md) is normative. Where this document and the register differ, the register applies.

## Contents

- [Terms](#terms)
- [1. Assets](#1-assets)
- [2. Adversaries](#2-adversaries)
- [3. Trust boundaries](#3-trust-boundaries)
- [4. Controls](#4-controls)
- [5. Failure behavior](#5-failure-behavior)
- [6. Residual risks](#6-residual-risks)
- [References](#references)

## Terms

The [glossary](glossary.md) defines each linked term.
Table 1 defines the other terms of this document.

*Table 1. The terms of this document.*

| Term | Definition |
|---|---|
| Asset | A part of the state or of the authority of the node that an adversary wants to read, change or spend. |
| Adversary | A party that writes an input of the node with an intent that the rules do not allow. |
| Trust boundary | A place where an input of an adversary enters a part of the node that must check it. |
| Control | A check of the node at a trust boundary, with the input that it rejects and the code of the rejection. |
| Capability | A name whose possession gives an authority, for example a [handle capability](glossary.md#handle-capability) [[DV66](#ref-dv66)]. |
| Fail closed | A check fails closed when an input that it cannot decide is rejected, never admitted [[SS75](#ref-ss75)]. |

## 1. Assets

*Table 2. The assets of Reified RSpaces.*

| Asset | What an adversary wants | Where the state lives |
|---|---|---|
| Space state | To read or change the records, the waiters, the counters or the config of a space that it does not hold | The [storage channels](glossary.md#storage-channel) of the space, in the RSpace of the node |
| Handles and names | To call a space, or to send and receive on its channels, without the capability of the space | The [handle capability](glossary.md#handle-capability) and the [space names](glossary.md#space-name) |
| Matchers and bodies | To fire a receive with values that its pattern does not bind, or to fire it twice | The [matcher capabilities](glossary.md#matcher-capability) and the [body capabilities](glossary.md#body-capability) of a translation |
| The deploy budget | To make the node do work that the deploy does not pay for | The phlo and the [host work](glossary.md#host-work) of each deploy |
| Consensus | To make two nodes compute two results for one block, or to make a merge accept a state that no serial order gives | The post-state roots, the event logs and the merge of the node |

## 2. Adversaries

*Table 3. The adversaries.*

| Adversary | Abilities | Goals |
|---|---|---|
| A deploy author | Writes arbitrary Rholang, quotes its own code, and receives the replies of the system processes | To reach the state of a space that it does not hold, to forge a capability, or to spend work that it does not pay for |
| A malicious agent author | Writes the code of a user agent that other programs use as a space | To give a receive values that its matchers did not give, or to stall the programs that use the space |
| A malicious validator | Proposes blocks, journals and LFS pages, and sends them to its peers | To make a peer accept a forged result, hang in replay, or import a forged state |
| A faulty or hostile service | Answers the advisory queries and the embedding calls of the node | To change a consensus result through its answers |

## 3. Trust boundaries

Four boundaries carry the inputs of the adversaries into the node:

1. **The program boundary.** The program of a deploy enters through the normalizer and the reducer hooks. It reaches the system processes and the space agents only through calls and routed operations.
2. **The agent boundary.** The code of a user agent runs as an ordinary program. It reaches a space only through the capabilities that the node built for a translation ([decision 77](decision-register.md#decision-77)).
3. **The consensus boundary.** The blocks of a peer enter native replay and the merger, and the LFS pages of a peer enter the importer.
4. **The service boundary.** The answers of ChromaDB and of the embedder enter as advisory ids or as recorded outputs ([decision 58](decision-register.md#decision-58)).

Figure 1 shows the boundaries and the control at each of them.

![Trust boundaries of Reified RSpaces as a component diagram, from the left to the right. A package of untrusted inputs holds the program of a deploy (arbitrary Rholang of a deploy author), the code of a user agent (the Rholang of an agent author), a peer node (blocks, journals, LFS pages), and an external service (ChromaDB answers, embeddings). The program boundary of the node holds the normalizer and decoders (marker tuples: exactly the canonical image, SC-1), the reducer hooks (routing and space references SC-2 and SC-3, persistent calls and params SC-5 and SC-6, seq rules, dangling ids and joins SC-10 to SC-12), the system processes (factory gates SC-7, bare handle SC-4, reply names SC-8, internal ops SC-15 and SC-16), the space agents (theories fail closed SC-9, bounds and metered reads SC-13, deterministic choices SC-18), and the substrate (prefix-atomic commit and deploy revert SC-14). The program flows as source text into the normalizer, as normalized Par into the hooks, as calls on op channels and handles into the system processes, as space intents and routed operations into the space agents, and as staged operations with one commit into the substrate. The agent boundary holds the capabilities of a translation (matchers, bodies and the sink, built by the node, decoded strictly SC-15, no persistent send SC-16) and the obligations ASO-1 to ASO-3 (not checked by the node, checked by the conformance kit SC-17); the code of a user agent calls the capabilities, which reach the system processes through the matcher op, the sink and the handle consume. The consensus boundary holds native replay (fails closed on a forged row SC-19), the merger (counter races and event coverage SC-20), and the LFS importer (validates each page SC-21); the peer sends blocks and journals to replay, sibling blocks to the merger, and LFS pages to the importer, and replay and the importer reach the substrate. The service boundary (advisory ids and recorded outputs, the exact engine decides SC-22) receives the answers of the service and passes ids to the space agents as hints only. A legend gives the colors of untrusted input, the node, the syntax layer, the system processes, the agents, the capabilities, the substrate and consensus parts, and the services, and the arrows of inputs or calls, effects on the stored state, and checks or rules.](diagrams/55-security-trust-boundaries.svg)

*Figure 1. Source: [diagrams/55-security-trust-boundaries.puml](diagrams/55-security-trust-boundaries.puml), rendered by [diagrams/render.sh](diagrams/render.sh).*

## 4. Controls

Each control below names the input that it rejects, the code of the rejection, and its evidence.
An example of `examples/reified_rspaces/` shows the control where a source program can reach it, and [chapter 21](../../rholang/21-reified-spaces.md#all-examples) lists the examples.

### 4.1 The program boundary

*Table 4. The controls of the program boundary.*

| Id | Control | Rejected input | Evidence |
|---|---|---|---|
| SC-1 | A decoder accepts exactly the canonical image of its encoder, so a marker gives no capability beyond the syntax ([decision 18](decision-register.md#decision-18)) | A marker tuple that a program builds in a form that the normalizer never writes: `Malformed` | `marker_forging_grants_no_capability`, the example `factory-rejects-a-forged-params-tuple` |
| SC-2 | A program cannot build a `GPrivate` with chosen bytes, so it holds a Reified id only from a reply. A reference that is not a handle names no native space. | A Reified id that is not a handle: `NotASpaceRef`. The value `Nil`: `EmptySpaceRef` | `prop_reified_non_handle_not_a_space_ref`, `reducer_handle_unwrap_not_a_space_ref`, `reducer_handle_unwrap_empty_space_ref` |
| SC-3 | A handle capability is a bundle with the write flag only, so a program can call the space but cannot receive on the handle | A reference under a bundle without the write flag: `HandleNotWritable` | `prop_handle_requires_write_flag`, `reducer_handle_unwrap_write_flag`, `prop_agent_ref_requires_write_flag` |
| SC-4 | `HANDLE_OP` accepts only the bare handle id, which only the forwarder of the space passes ([decision 32](decision-register.md#decision-32), item A6) | A call of the URN `rho:reified:internal:handleOp` with any other value: dropped with no effect | `reified_internal_op_requires_handle_capability` |
| SC-5 | A persistent send to a factory, a system process or a handle aborts, so no call repeats forever (item A21) | `!!` on such a channel: `PersistentFactoryCall`, `PersistentOpCall`, `PersistentHandleCall` | `space_system_ops_persistent_send_aborts`, `reified_factory_persistent_send_aborts_once`, three examples |
| SC-6 | Only a factory and a handle take params, and only named params of their schema | Params on another system channel: `ParamsUnsupported`. A positional param: `PositionalParamsUnsupported`. An unknown key: `UnknownParam` | `reified_hook_errors_are_typed`, `space_queue_send_params_rejected`, four examples |
| SC-7 | A factory runs nine gates before the space exists ([specification FA-6](specification.md#15-families)) | A bad constructor, qualifier, theory, param key or param value: the code of its gate | `reified_factory_rejects_bad_constructor`, `reified_factory_p4_gates_fail_closed`, six examples |
| SC-8 | A process sends a typed reply only to a reply name, and it drops a malformed message | A forged reply channel, or a message that is not a call: no reply | `space_handle_methods_reply_names_and_typed_errors`, `reified_malformed_message_dropped` |
| SC-9 | A theory admits each value of a send before any staging, and an undecided value fails closed ([specification 5](specification.md#5-theories)) | A refuted value: `TheoryRefuted`. An undecided value: `TheoryUndetermined`. An identity without a checker: `TheoryUnavailable` | `theory_refuted_value_aborts_before_staging`, `theory_undetermined_budget0_aborts`, `theory_unavailable_fails_each_config_read`, `prop_theory_fail_closed` |
| SC-10 | A name of a seq space never travels as data, and each channel of a seq space has one chain of uses ([specification 4.3](specification.md#43-seq)) | A send with a seq name: `SeqSent`. Two parallel uses of one channel: `SeqConcurrent` | `reducer_seq_payload`, `prop_seq_scan_finds_every_seq_name`, `reducer_seq_atomic_guard`, `prop_seq_claim_chain` |
| SC-11 | An id of a temp or seq space of an ended deploy fails before any job, at a fixed charge | A send, a receive or a call on an ended space: `DanglingName`, `DanglingHandle` | `reified_ephemeral_dangling_next_deploy`, `reducer_dangling_handle` |
| SC-12 | A join takes its channels from one target | A join across spaces, or across a space and the default space: `CrossSpaceJoin` | `space_joins_cross_space_join_aborts`, `space_joins_default_and_space_join_aborts` |
| SC-13 | The work of each operation has a bound, and every read and copy is reserved on the deploy budget before it happens ([decision 40](decision-register.md#decision-40)) | A full VectorDB collection: `CapacityExceeded`. A wrong vector length: `DimensionMismatch`. A path of more than 64 elements: `PathTooDeep`. A forged index name: `NotAnIndexName`. Work over the budget: `HostWorkRejected` | `space_vectordb_capacity_bounds_records_not_waiters`, `pathmap_path_too_deep_aborts`, `space_array_forged_channel_rejected`, `prop_engine_primitives_reserve_before_they_read`, `space_disciplines_bag_reservation_cut_rejects_without_effect` |
| SC-14 | The commit of a transaction is prefix-atomic, and the revert of a failed deploy restores its pre-state (decision 19, item D6) | A deploy that fails after a partial transaction: no effect remains | `soft_revert_after_prefix_failure` (`Checkpoint.v`), `prop_soft_revert_restores_the_parent`, `space_temp_failed_deploy_reverts` |

A transaction also has at most 64 fire rounds ([decision I-21](decision-register.md#i-21)), so one operation never makes an unbounded chain of fires.
The tests `space_disciplines_bag_round_cap_and_divergent_pair_reissue` and `space_joins_cascade_round_cap_reissues_settle` check the round cap.

### 4.2 The agent boundary

*Table 5. The controls of the agent boundary.*

| Id | Control | Rejected input | Evidence |
|---|---|---|---|
| SC-15 | The node builds the matchers and the body of a translation, and each one decodes strictly. A native handle accepts only such matchers. An agent-minted name is a bundle of a tuple with the unforgeable agent tag, so a pattern cannot open it. | A matcher that no translation built: the typed reply `ForeignMatcher`. A call of the matcher process without a state: dropped | `space_handle_methods_consume_rejects_foreign_matcher`, `matcher_op_drops_calls_without_a_state_or_a_reply_name` |
| SC-16 | A capability of a translation takes no persistent send. The body of a linear receive runs at most once, and the sink absorbs every reply. | `!!` on a matcher or a body: `PersistentCapabilityCall`. A second send to a linear body: it parks | The example `agent-capability-rejects-a-persistent-call`, `sink_drops_every_message` |
| SC-17 | The node does not check the obligations ASO-1 to ASO-3 of an agent. A faulty agent breaks the semantics of its space, but never consensus, and the conformance kit checks an agent. | Not applicable: the control is the replay of every call as an ordinary event | `conformance_rejects_an_agent_that_leaves_a_match_pending`, `agent_never_replies_pending_replay_equal` |

### 4.3 The consensus boundary

*Table 6. The controls of the consensus boundary.*

| Id | Control | Rejected input | Evidence |
|---|---|---|---|
| SC-18 | Every choice of a space depends only on its entries and on the deploy, never on timing ([specification 11](specification.md#11-determinism)) | Not applicable: no input can change a choice | `determinism_replay_trace_root_is_pinned`, `n9_run_twice_assert_canonical_equal` |
| SC-19 | Native replay fails closed and never waits for a row ([specification NE-4](specification.md#8-native-execution)) | A forged journal row, footprint or outcome: the operation fails, and the epoch closes | `native_replay_fails_closed` (`EventCorrelation.v`), `space_native_forged_predecessor_fails_closed`, `space_native_forged_footprints_never_hang` |
| SC-20 | Two branches that use one ordered collection race on its counter, and every space write is in the event log of its deploy | A merge of two orders that no serial run gives: a conflict | `merge_without_race_is_serializable` (`CounterConflict.v`), `space_queue_counter_race_conflict_enq_deq`, `space_event_coverage_debug_assert` |
| SC-21 | The importer validates each LFS page before it imports the page | A page with one changed byte: the validation fails | `space_lfs_export_import` |

### 4.4 The service boundary

*Table 7. The controls of the service boundary.*

| Id | Control | Rejected input | Evidence |
|---|---|---|---|
| SC-22 | The answers of the mirror are hints: the exact engine selects every record. The node records each output of `embed` and `ann`, and replay asks no service. A failed deploy publishes nothing to the mirror. | A stale or forged id: it selects no record. A changed answer in replay: never asked | `vectordb_ann_replay_equal`, `vectordb_embed_replay_equal`, `mirror_journal_drops_failed_deploy` |

## 5. Failure behavior

Every abort of a space operation is a deterministic `ReduceError`, so play and replay fail with the same code.
A space transaction commits as a prefix: when operation $`i`$ fails, the operations before it stay applied ([decision 19](decision-register.md#decision-19), item D6).
The deploy then fails, and the revert of its soft checkpoint removes every effect of the deploy, the applied prefix included.
So no effect of a failed deploy reaches a checkpoint, and the deploy keeps the charges of the work that it did.
This rule replaces the item "failure atomicity" of the first outline of this document.
A failed transaction can leave its applied prefix until the deploy revert removes it.

A protocol error of a call is not a failure of the deploy.
The call gets a typed reply, and the space does not change ([agent API, section 7](agent-api.md#7-typed-replies)).

## 6. Residual risks

These risks remain, and the documents state them:

- **Agent trust.** A program trusts an agent to send the values that the matchers gave. The reference agent waits for each matcher while it holds its state, so a matcher that never answers stops the instance. Give the reference of an agent space only to programs that you trust.
- **Divergent pairs.** A persistent send and a persistent receive on one queue fire again in each new space intent. They stop only when the phlo of the deploy runs out, as on the baseline host.
- **Soft capacity.** The `capacity` of a VectorDB collection bounds the view of each transaction. A merge of concurrent branches can exceed it by the stores of the branches ([decision 71](decision-register.md#decision-71)).
- **Legacy evaluation.** Legacy evaluation passes no host-work budget, so the reservations of SC-13 bind only in native execution ([decision 69](decision-register.md#decision-69)).
- **Forged reply names.** A reply of a system process on a forged channel routes to the default space (rule R7), so a reply name should come from `new`.
- **The cost of agent spaces.** Each matcher call of an agent space costs three COMMs, so an agent space costs more than a native space.

## References

<a id="ref-dv66"></a>[DV66] J. B. Dennis and E. C. Van Horn, "Programming semantics for multiprogrammed computations," *Communications of the ACM*, vol. 9, no. 3, pp. 143–155, 1966. [doi:10.1145/365230.365252](https://doi.org/10.1145/365230.365252)

<a id="ref-ss75"></a>[SS75] J. H. Saltzer and M. D. Schroeder, "The protection of information in computer systems," *Proceedings of the IEEE*, vol. 63, no. 9, pp. 1278–1308, 1975. [doi:10.1109/PROC.1975.9939](https://doi.org/10.1109/PROC.1975.9939)
