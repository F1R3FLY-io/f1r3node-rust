# Reified RSpaces FIPS conformance matrix

This document is generated. Do not edit it.
`fips-conformance-matrix.json` is the source, and `scripts/verify-reified/check-fips-matrix.py --write` renders it.
The checker also proves that every normative line of each FIPS has a row and that every cell resolves.

Each row binds one clause of a FIPS to its evidence, or to the decision that records a deviation.
The [specification](specification.md) states the rules that the rows cite, and the [decision register](decision-register.md) holds the decisions.

## Contents

- [The FIPS documents](#the-fips-documents)
- [Status values](#status-values)
- [Summary](#summary)
- [The rows of Reifying RSpaces](#the-rows-of-reifying-rspaces)
- [The rows of Agents](#the-rows-of-agents)
- [Spec holes](#spec-holes)

## The FIPS documents

*Table 1. The FIPS documents and their normative lines.*

| Key | FIPS | Date | Path under the FIPS repository | SHA-256 | Normative lines | Outside the normative lines |
|---|---|---|---|---|---|---|
| `RR` | Reifying RSpaces | 2025-09-26 | `approved/2025-09-26-Reifying-RSpaces/Reifying RSpaces.md` | `ecf1ebfab339b2257ea62ff00ffccdb973f1fcb7f9ce6357dce1d7f08aba02e3` | 1–825 | None |
| `AG` | Agents | 2025-08-20 | `approved/2025-08-20-Agents/Agents.md` | `fa75ed55be09d68df6aca3e2f43de76706bd8f72eda11463a6ee65a2089e3491` | 8–57 | Lines 1 to 7 (the title), 59 to 131 (the example) and 133 to 138 (contracts as agents) |

The normative lines are the non-blank lines of the ranges.
A row may cover more lines than the clause needs, and two rows may share a line.

## Status values

*Table 2. The status values of a row.*

| Status | Meaning |
|---|---|
| `implemented` | The code, its tests, its documents, and a formal model or a stated reason for none give the clause. |
| `deviation` | The campaign differs from the clause, and a recorded decision states how and why. |
| `future` | A later phase gives the clause, and a recorded decision names the phase. |
| `out-of-scope` | Another owner gives the clause, and a recorded decision states the owner. |
| `informative` | The text states background, motivation or an open question, and requires no behavior. |

## Summary

*Table 3. The number of rows of each status.*

| FIPS | `implemented` | `deviation` | `future` | `out-of-scope` | `informative` | Rows |
|---|---|---|---|---|---|---|
| Reifying RSpaces | 34 | 4 | 4 | 0 | 8 | 50 |
| Agents | 6 | 0 | 0 | 1 | 0 | 7 |

## The rows of Reifying RSpaces

*Table 4. The clauses of Reifying RSpaces and their status.*

| Row | Lines | Clause | Status |
|---|---|---|---|
| [RR-01](#rr-01) | 1–11 | The title, the authors, the date and the abstract of the FIPS. | `informative` |
| [RR-02](#rr-02) | 13–65 | Background: the four groups of operations of the ISpace trait of the baseline (checkpoints, replay, produce and consume). | `informative` |
| [RR-03](#rr-03) | 67–95 | An RSpace is a key-value store: a channel or a group of channels is a key, a data bag and a continuation bag are the value, and a channel exists on its first use. | `implemented` |
| [RR-04](#rr-04) | 97–107 | The invariant: after each produce or consume, no data and no continuation match, and every generalization keeps the invariant. | `implemented` |
| [RR-05](#rr-05) | 109–111 | Every agent with the behavioral type of a space can be an RSpace, and every RSpace can be reified into an agent. | `implemented` |
| [RR-06](#rr-06) | 113–121 | The constructor takes a qualifier (default, temp or seq), a theory and more arguments, and it may abort when it does not support the qualifier. | `implemented` |
| [RR-07](#rr-07) | 117–118 | The theory of a space is a MeTTaIL theory that describes the data of its channels. | `future` |
| [RR-08](#rr-08) | 123–125 | The method gensym returns a name, and it may abort, for example when it runs out of names. | `implemented` |
| [RR-09](#rr-09) | 127–130 | The method produce sends a process on a channel, and it may abort, for example when the process holds a Seq name. | `implemented` |
| [RR-10](#rr-10) | 132–145 | The method consume receives on a join: pairs of a matcher and a channel, the arrow <-, <= or <<-, and a body that takes a map of the bindings. | `implemented` |
| [RR-11](#rr-11) | 147 | Patterns will grow into spatial-behavioral types in datalog form, and matchers will then tailor patterns to the inner structure and the theory. | `informative` |
| [RR-12](#rr-12) | 149–151 | The namespace rho:space holds combinations of stores and collections, and developers can write their own spaces. | `implemented` |
| [RR-13](#rr-13) | 153–157 | The outer store HashMap, the store of the current implementation. | `implemented` |
| [RR-14](#rr-14) | 159–186 | The outer store PathMap: channels are paths, a send also sends on every prefix, and a receive on a prefix binds the re-rooted data and leaves the rest. | `implemented` |
| [RR-15](#rr-15) | 187–193 | The quantale semantics of sequential processes on sets of paths. | `informative` |
| [RR-16](#rr-16) | 195–201 | The outer store Array of fixed length: new gives the next index up to the maximum, then aborts with an out-of-names error, and indices can be unforgeable. | `implemented` |
| [RR-17](#rr-17) | 203–205 | The outer store Array with reuse: new gives the next index modulo the size. | `implemented` |
| [RR-18](#rr-18) | 207–208, 211 | The outer store Vector of unbounded length: new grows the vector and gives the index of its last element, and indices can be unforgeable. | `implemented` |
| [RR-19](#rr-19) | 209 | When the server runs out of memory, the deploy fails with an out-of-memory error. | `deviation` |
| [RR-20](#rr-20) | 213–215 | The outer store HashSet of channels: a send adds the channel to the set, a receive uses only the empty pattern, and joins are allowed. | `implemented` |
| [RR-21](#rr-21) | 217–227 | Inner collections: a pair of collection types replaces the two bags, the collections are unindexed, patterns depend on the collection type, and produces need not commute. | `implemented` |
| [RR-22](#rr-22) | 229–231 | Bags: the multiset semantics of the default space. | `implemented` |
| [RR-23](#rr-23) | 233–235 | Sets: each datum or continuation appears at most once, and a second equal send is idempotent. | `implemented` |
| [RR-24](#rr-24) | 237–241 | Cells: at most one datum or continuation on a channel, and a second send is an error. | `implemented` |
| [RR-25](#rr-25) | 243–245 | Queues: FIFO semantics, only the heads interact, one queue is empty or the heads do not match, and nodes agree on the arrival order. | `implemented` |
| [RR-26](#rr-26) | 247–249 | Priority queues: a queue for each priority, and patterns can name a priority. | `implemented` |
| [RR-27](#rr-27) | 251–253 | Stacks: FILO semantics, and only the tops interact. | `implemented` |
| [RR-28](#rr-28) | 255–257 | A bag and a vector DB: receives wait in a bag, data goes into a vector DB, a pattern gives a vector and a similarity bound, and a receive takes one vector within the bound. | `implemented` |
| [RR-29](#rr-29) | 259 | Look into tensor logic for a vector DB. | `future` |
| [RR-30](#rr-30) | 261 | Look into nonlinear operations on vectors, such as majority for binary vectors and an elementwise sigmoid. | `future` |
| [RR-31](#rr-31) | 263–265 | Other collections: fixed-size versions of the collection types, and collections that mix data types. | `informative` |
| [RR-32](#rr-32) | 267–269 | Qualifiers change the behavior and the persistence of the channels of a space, and some outer structures, such as sets, require the sequential qualifier. | `implemented` |
| [RR-33](#rr-33) | 269 | Pathmaps with quantale semantics require the sequential qualifier. | `informative` |
| [RR-34](#rr-34) | 271–273 | The default qualifier: a channel persists, and it allows concurrent access. | `implemented` |
| [RR-35](#rr-35) | 275–277 | The temp qualifier: concurrent access, and the data and continuations of the channel are never written to disk. | `implemented` |
| [RR-36](#rr-36) | 279–285 | The seq qualifier: temp, and a seq channel is never sent, is not used in a concurrent process, and is used in at most one sequential process. | `implemented` |
| [RR-37](#rr-37) | 287–289 | The use block sets the default space of untagged channels in new blocks and of forged names. | `implemented` |
| [RR-38](#rr-38) | 291–293 | The introduction of the example: one space on each of n channels, qualifiers that set persistence and sequentiality, and theories that a later design document specifies. | `informative` |
| [RR-39](#rr-39) | 295–302 | The factories by URN, the FIPS names rho:space:HashMapBagSpace and rho:space:PathMapSpace included, and their calls with a qualifier and a theory. | `implemented` |
| [RR-40](#rr-40) | 303–308 | Inside a use block, new x : s creates a name in the space s, and a forged name is created in the space of the use block. | `implemented` |
| [RR-41](#rr-41) | 309–317 | Joins are permitted in one space and prohibited across spaces. | `implemented` |
| [RR-42](#rr-42) | 318–320, 323–330 | Inside a new block that creates names in the spaces, the names of the spaces refer to the modified spaces, and outside it they refer to the unmodified spaces. | `implemented` |
| [RR-43](#rr-43) | 321–322 | getSpaceAgent returns the agent of a space, and assertEqual compares it with the factory. | `implemented` |
| [RR-44](#rr-44) | 332–339 | The issues of the current architecture: ISpace couples the outer structure, the inner structure and the operations. | `informative` |
| [RR-45](#rr-45) | 341–442 | Layers 1 and 2 of the proposal: the traits DataCollection, ContinuationCollection and ChannelStore. | `deviation` |
| [RR-46](#rr-46) | 444–561 | Layers 3 and 4 of the proposal: the qualifier enum, the trait SpaceAgent, and the extension traits for checkpoints and replay. | `deviation` |
| [RR-47](#rr-47) | 563–716 | Layer 5 of the proposal: the bag, queue, set and cell collections, the HashMap and PathMap stores, and a generic RSpace over a store and a matcher. | `implemented` |
| [RR-48](#rr-48) | 718–770 | Layer 6 of the proposal: a factory for each kind of space that creates a space from a qualifier, a theory and more arguments. | `implemented` |
| [RR-49](#rr-49) | 771–815 | Layer 6 of the proposal: a registry that registers space factories at run time and creates spaces by name. | `future` |
| [RR-50](#rr-50) | 817–824 | The migration path of the proposal in six phases. | `deviation` |

### RR-01

| Field | Value |
|---|---|
| Lines | 1–11 |
| Status | `informative` |
| Clause | The title, the authors, the date and the abstract of the FIPS. |
| Reason | The abstract summarizes the clauses of the rows RR-04 (the invariant), RR-05 (the agent) and RR-13 to RR-28 (the stores and the collections). |

### RR-02

| Field | Value |
|---|---|
| Lines | 13–65 |
| Status | `informative` |
| Clause | Background: the four groups of operations of the ISpace trait of the baseline (checkpoints, replay, produce and consume). |
| Documents | [design.md#s1-purpose-and-the-rule-of-no-new-ispace-surface](design.md#s1-purpose-and-the-rule-of-no-new-ispace-surface) |
| Decisions | [I-1](decision-register.md#i-1) |
| Reason | The section describes the baseline. The campaign does not change ISpace, and the substrate reaches the same operations through SpaceTxnHost. |

### RR-03

| Field | Value |
|---|---|
| Lines | 67–95 |
| Status | `implemented` |
| Clause | An RSpace is a key-value store: a channel or a group of channels is a key, a data bag and a continuation bag are the value, and a channel exists on its first use. |
| Code | `rholang/src/rust/interpreter/spaces/generic.rs:721-864`, `rholang/src/rust/interpreter/spaces/generic.rs:965-1286` |
| Formal | `storage_id_injective` (`Namespace.v:732`), `space_and_collection_counters_distinct` (`Namespace.v:789`), `first_ops_race_on_space_counter` (`CounterConflict.v:939`) |
| Tests | `space_meta_channels_counters` in `rholang/tests/reified_queue.rs`, `space_queue_events_namespaced` in `rholang/tests/reified_queue.rs`, `prop_first_ops_race_on_space_counter` in `rholang/tests/reified_p4_props.rs`, `space_counter_first_operation_bumps_the_space_counter` in `rholang/src/rust/interpreter/spaces/counter.rs`, `example_queue_fifo` in `rholang/tests/reified_examples_execute.rs`, `example_bag_basics` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#1-space-model](specification.md#1-space-model), [specification.md#12-collections](specification.md#12-collections), [specification.md#13-entries-and-operations](specification.md#13-entries-and-operations), [21-reified-spaces.md#terms](../../rholang/21-reified-spaces.md#terms) |
| Note | Each collection of a space holds its records and its waiters, and its first operation opens it (rule OC-2). |

### RR-04

| Field | Value |
|---|---|
| Lines | 97–107 |
| Status | `implemented` |
| Clause | The invariant: after each produce or consume, no data and no continuation match, and every generalization keeps the invariant. |
| Code | `rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:335-361`, `rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:1169-1280`, `rholang/src/rust/interpreter/spaces/settle.rs:473-478`, `rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:3753-3930`, `rholang/src/rust/interpreter/spaces/disciplines/scan.rs:1411-1507`, `rholang/src/rust/interpreter/spaces/settle.rs:453-577` |
| Formal | `queue_produce_preserves_inv` (`HeadOnlyDisciplines.v:932`), `queue_consume_preserves_inv` (`HeadOnlyDisciplines.v:952`), `queue_reentry_preserves_shadow_inv` (`HeadOnlyDisciplines.v:2716`), `queue_quiescent_inv` (`HeadOnlyDisciplines.v:2785`), the TLA+ invariant `QuiescentNoPendingMatch` of `HeadOnlyDiscipline.tla`, `stack_lifo` (`StackDiscipline.v:1263`), the TLA+ invariant `LifoOrder` of `StackDiscipline.tla`, `cascade_restores_inv` (`JoinCascade.v:7424`), the TLA+ invariant `QuiescentNoPendingMatch` of `JoinCascade.tla`, `pq_settle_oldest_first` (`PriorityQueue.v:1904`), `pq_settle_order_matters` (`PriorityQueue.v:4086`), `pq_produce_preserves_inv` (`PriorityQueue.v:1395`), `pq_without_settle_breaks_inv` (`PriorityQueue.v:4007`), the TLA+ invariant `PqOldestFirst` of `PriorityQueue.tla`, `vdb_no_pending_match` (`VectorDbDiscipline.v:662`), `vdb_list_needs_cascade` (`VectorDbDiscipline.v:1311`), `set_produce_fire_retires_stored_class` (`SetDiscipline.v:1157`), the TLA+ invariant `SetClassRetired` of `GenericRSpace.tla`, `cell_capacity_one` (`CellDiscipline.v:672`), the TLA+ invariant `NoPendingMatch` of `GenericRSpace.tla`, the TLA+ invariant `StackNoPendingMatch` of `StackDiscipline.tla`, the TLA+ invariant `PqNoPendingMatch` of `PriorityQueue.tla`, the TLA+ invariant `NoMatchRemains` of `PathMapPrefix.tla`, the TLA+ invariant `QuiescentNoPendingMatch` of `VectorDb.tla` |
| Tests | `prop_queue_produce_preserves_invariant` in `rholang/tests/reified_p4_props.rs`, `prop_queue_consume_preserves_invariant` in `rholang/tests/reified_p4_props.rs`, `prop_queue_shadow_invariant_with_inflight` in `rholang/tests/reified_p4_props.rs`, `prop_stack_lifo_order_any_schedule` in `rholang/tests/reified_stack.rs`, `prop_cascade_restores_inv_any_schedule` in `rholang/tests/reified_join_props.rs`, `prop_quiescent_no_pending_match_any_schedule` in `rholang/tests/reified_join_props.rs`, `prop_join_engine_agrees_with_drivers_without_joins` in `rholang/tests/reified_join_props.rs`, `prop_pq_oldest_first_any_schedule` in `rholang/tests/reified_priority_queue.rs`, `prop_pq_no_pending_match_any_schedule` in `rholang/tests/reified_priority_queue.rs`, `space_disciplines_pq_older_waiter_wins` in `rholang/tests/reified_priority_queue.rs`, `prop_vectordb_no_pending_match` in `rholang/tests/reified_vectordb.rs`, `space_vectordb_ce2_removal_cascade_fires_the_list_waiter` in `rholang/tests/reified_vectordb.rs`, `prop_set_produce_fire_retires_stored_class` in `rholang/tests/reified_set.rs`, `prop_set_class_retired_any_schedule` in `rholang/tests/reified_set.rs`, `space_set_discipline_guarded_waiter_then_equal_send_retires_class` in `rholang/tests/reified_set.rs`, `space_disciplines_cell_every_operation_bumps_counter` in `rholang/tests/reified_cell.rs`, `space_disciplines_cell_branch_writes_race_on_the_counter` in `rholang/tests/reified_cell.rs`, `prop_engine_no_guard_free_pending_match` in `rholang/tests/reified_engine_props.rs`, `prop_stack_no_pending_match_any_schedule` in `rholang/tests/reified_stack.rs`, `prop_candidates_cover_the_prefix_in_key_order` in `rholang/src/rust/interpreter/spaces/pathmap/delivery.rs`, `prop_waiter_prefixes_are_most_specific_first` in `rholang/src/rust/interpreter/spaces/pathmap/delivery.rs`, `prop_pathmap_no_match_remains_any_schedule` in `rholang/tests/reified_pathmap.rs`, `example_queue_peeks_and_persistence` in `rholang/tests/reified_examples_execute.rs`, `example_stack_peeks_and_persistence` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#30-ordered-collections](specification.md#30-ordered-collections), [specification.md#38-joins](specification.md#38-joins), [21-reified-spaces.md#inner-collections](../../rholang/21-reified-spaces.md#inner-collections) |
| Decisions | [decision 11](decision-register.md#decision-11), [decision 29](decision-register.md#decision-29), [decision 50](decision-register.md#decision-50) |
| Note | On a head-only collection the invariant concerns the heads: a head that does not match blocks the entries behind it (decision 11). |

### RR-05

| Field | Value |
|---|---|
| Lines | 109–111 |
| Status | `implemented` |
| Clause | Every agent with the behavioral type of a space can be an RSpace, and every RSpace can be reified into an agent. |
| Code | `rholang/src/rust/interpreter/reified/handle.rs:232-411`, `rholang/src/rust/interpreter/reified/user_agent.rs:498-521` |
| Formal | `agent_ref_requires_write_flag` (`SpaceRefs.v:1286`), the TLA+ invariant `RoutingIsolation` of `AgentSpaces.tla`, `r7_agent_reply_bundle_exact` (`SpaceRefs.v:999`), `offsets_partition` (`AgentTranslation.v:226`), `env_keys_disjoint_union` (`AgentTranslation.v:324`), `body_equivalence` (`AgentTranslation.v:401`), `guard_fold_sound` (`AgentTranslation.v:596`), `matcher_sound_complete` (`AgentTranslation.v:650`), `matcher_state_roundtrip` (`AgentTranslation.v:783`), the TLA+ invariant `JoinSameSpace` of `AgentSpaces.tla`, `tag_untag_id` (`SpaceRefs.v:1055`), `resume_binds_innermost` (`AgentTranslation.v:914`) |
| Tests | `prop_agent_ref_requires_write_flag` in `rholang/tests/reified_p8_props.rs`, `prop_routes_follow_the_owner_of_the_channel` in `rholang/tests/reified_p8_props.rs`, `prop_space_ref_unwraps_one_bundle` in `rholang/tests/reified_p8_props.rs`, `agent_reference_not_writable_rejected` in `rholang/tests/reified_user_agents.rs`, `agent_use_and_new_follow_the_write_rule` in `rholang/tests/reified_user_agents.rs`, `prop_r7_agent_reply_bundle_exact` in `rholang/tests/reified_p8_props.rs`, `user_agent_send_calls_the_agent` in `rholang/tests/reified_user_agents.rs`, `user_agent_reply_reaches_the_agent` in `rholang/tests/reified_user_agents.rs`, `agent_xlate_bang_bang_rejected` in `rholang/tests/reified_user_agents.rs`, `user_agent_errors_params` in `rholang/tests/reified_user_agents.rs`, `prop_body_equivalence` in `rholang/tests/reified_user_agents.rs`, `prop_matcher_sound_complete` in `rholang/tests/reified_user_agents.rs`, `prop_guard_fold_sound` in `rholang/tests/reified_user_agents.rs`, `prop_receive_joins_one_space` in `rholang/tests/reified_user_agents.rs`, `prop_linear_body_runs_at_most_once` in `rholang/tests/reified_user_agents.rs`, `agent_xlate_join_two_matchers` in `rholang/tests/reified_user_agents.rs`, `agent_xlate_guard_span_rejected` in `rholang/tests/reified_user_agents.rs`, `agent_xlate_cross_space_join_rejected` in `rholang/tests/reified_user_agents.rs`, `user_agent_errors_query` in `rholang/tests/reified_user_agents.rs`, `user_agent_errors_pattern_too_deep` in `rholang/tests/reified_user_agents.rs`, `prop_tag_untag_id` in `rholang/tests/reified_p8_props.rs`, `prop_resume_binds_innermost` in `rholang/tests/reified_p8_props.rs`, `user_agent_new_binding_order_matches_native` in `rholang/tests/reified_user_agents.rs`, `agent_gensym_calls_have_distinct_states` in `rholang/tests/reified_user_agents.rs`, `agent_never_replies_pending_replay_equal` in `rholang/tests/reified_user_agents.rs`, `agent_wrapper_forwards_to_native` in `rholang/tests/reified_user_agents.rs`, `space_handle_methods_consume_rejects_foreign_matcher` in `rholang/tests/reified_handle.rs`, `example_handle_methods` in `rholang/tests/reified_examples_execute.rs`, `example_agent_hashmap_bag` in `rholang/tests/reified_examples_execute.rs`, `example_agent_wrapper_forwards_to_native` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [agent-api.md#1-overview](agent-api.md#1-overview), [agent-api.md#12-user-agent-backed-spaces](agent-api.md#12-user-agent-backed-spaces), [21-reified-spaces.md#writing-your-own-space](../../rholang/21-reified-spaces.md#writing-your-own-space), [design.md#n18-phase-p8-user-agent-backed-spaces](design.md#n18-phase-p8-user-agent-backed-spaces) |
| Decisions | [decision 8](decision-register.md#decision-8), [decision 77](decision-register.md#decision-77) |

### RR-06

| Field | Value |
|---|---|
| Lines | 113–121 |
| Status | `implemented` |
| Clause | The constructor takes a qualifier (default, temp or seq), a theory and more arguments, and it may abort when it does not support the qualifier. |
| Code | `rholang/src/rust/interpreter/spaces/factory/urn.rs:207`, `rholang/src/rust/interpreter/spaces/factory/urn.rs:246-253`, `rholang/src/rust/interpreter/spaces/families.rs:998-1007`, `rholang/src/rust/interpreter/spaces/families.rs:486-519`, `rholang/src/rust/interpreter/spaces/factory/defaults.rs:133-189`, `rholang/src/rust/interpreter/spaces/factory/defaults.rs:206-221`, `rholang/src/rust/interpreter/spaces/factory/constructor.rs:115-213`, `rholang/src/rust/interpreter/spaces/factory/constructor.rs:305-335`, `rholang/src/rust/interpreter/reified/factory.rs:58-130` |
| Formal | `registered_families_complete` (`SpaceFactory.v:1571`), `space_match_is_injected_match` (`Match.v:644`), `constructor_params_validated` (`SpaceFactory.v:2065`), `priorities_validated` (`SpaceFactory.v:2202`), `array_size_validated` (`SpaceFactory.v:2315`), `qualifier_matrix` (`SpaceFactory.v:438`), `theory_gate_validated` (`SpaceFactory.v:2727`) |
| Tests | `prop_registered_families_complete` in `rholang/tests/reified_families.rs`, `space_factory_families_registered` in `rholang/tests/reified_families.rs`, `space_families_dispatch_by_config` in `rholang/src/rust/interpreter/spaces/families.rs`, `space_family_dispatch_by_config` in `rholang/tests/reified_families.rs`, `prop_constructor_params_validated` in `rholang/tests/reified_families.rs`, `space_factory_kwargs_unknown_param_with_suggestion` in `rholang/tests/reified_families.rs`, `prop_priorities_validated` in `rholang/tests/reified_families.rs`, `prop_array_size_validated` in `rholang/tests/reified_families.rs`, `space_factory_kwargs_are_stored_canonical` in `rholang/tests/reified_families.rs`, `prop_qualifier_matrix` in `rholang/tests/reified_p4_props.rs`, `space_urn_factory_qualifier_support_matrix` in `models/tests/space_factory_urn.rs`, `prop_theory_gate` in `rholang/tests/reified_theory.rs`, `theory_unknown_name_suggestion` in `rholang/tests/reified_theory.rs`, `theory_builtin_args_rejected` in `rholang/tests/reified_theory.rs`, `reified_factory_theory_gate` in `rholang/src/rust/interpreter/spaces/factory/constructor.rs`, `example_factory_rejects_a_bad_argument` in `rholang/tests/reified_examples_execute.rs`, `example_factory_rejects_an_unknown_param` in `rholang/tests/reified_examples_execute.rs`, `example_factory_rejects_a_missing_param` in `rholang/tests/reified_examples_execute.rs`, `example_factory_rejects_an_invalid_param` in `rholang/tests/reified_examples_execute.rs`, `example_space_qualifier_temp` in `rholang/tests/reified_examples_execute.rs`, `example_space_qualifier_seq` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [agent-api.md#3-factories](agent-api.md#3-factories), [agent-api.md#32-the-gates](agent-api.md#32-the-gates), [21-reified-spaces.md#creating-a-space](../../rholang/21-reified-spaces.md#creating-a-space) |
| Decisions | [decision 8](decision-register.md#decision-8), [decision 73](decision-register.md#decision-73) |

### RR-07

| Field | Value |
|---|---|
| Lines | 117–118 |
| Status | `future` |
| Clause | The theory of a space is a MeTTaIL theory that describes the data of its channels. |
| Documents | [specification.md#5-theories](specification.md#5-theories), [theory-seam.md#4-the-builtin-theories](theory-seam.md#4-the-builtin-theories), [f1r3lang-integration.md#4-what-changes-at-the-merge](f1r3lang-integration.md#4-what-changes-at-the-merge) |
| Decisions | [decision 9](decision-register.md#decision-9), [decision 3](decision-register.md#decision-3) |
| Note | The builtin structural theories Nat, Int, String, Bool and Any work now (row RR-06). The MeTTaIL theories come through the theory seam in phase P12, after the readiness signal of F1r3lang. |

### RR-08

| Field | Value |
|---|---|
| Lines | 123–125 |
| Status | `implemented` |
| Clause | The method gensym returns a name, and it may abort, for example when it runs out of names. |
| Code | `rholang/src/rust/interpreter/spaces/allocator.rs:240-253`, `rholang/src/rust/interpreter/spaces/allocator.rs:87-131`, `rholang/src/rust/interpreter/reified/handle.rs:383-410` |
| Formal | `name_id_routes_to_sid` (`Namespace.v:1003`), `fixed_array_allocates_at_most_size` (`OuterStorage.v:318`), `vector_index_is_counter` (`VectorStore.v:184`), `fixed_array_indices_sequential` (`OuterStorage.v:350`), `cyclic_array_index_mod_size` (`OuterStorage.v:381`), `vector_gensym_monotonic` (`VectorStore.v:219`), `allocation_batch_atomic` (`OuterStorage.v:407`), `fixed_array_out_of_names_exact` (`OuterStorage.v:335`) |
| Tests | `prop_name_id_routes_to_sid` in `rholang/tests/reified_p4_props.rs`, `space_handle_methods_gensym` in `rholang/tests/reified_handle.rs`, `prop_fixed_array_allocates_at_most_size` in `rholang/tests/reified_array_vector.rs`, `prop_vector_index_is_counter` in `rholang/tests/reified_array_vector.rs`, `prop_fixed_array_indices_sequential` in `rholang/tests/reified_array_vector.rs`, `prop_cyclic_array_index_mod_size` in `rholang/tests/reified_array_vector.rs`, `prop_vector_gensym_monotonic` in `rholang/tests/reified_array_vector.rs`, `prop_allocation_batch_atomic` in `rholang/tests/reified_array_vector.rs`, `prop_fixed_array_out_of_names_exact` in `rholang/tests/reified_array_vector.rs`, `example_handle_methods` in `rholang/tests/reified_examples_execute.rs`, `example_array_out_of_names` in `rholang/tests/reified_examples_execute.rs`, `example_space_annotations` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [agent-api.md#5-handle-methods](agent-api.md#5-handle-methods), [specification.md#24-the-allocator-of-array-and-vector-spaces](specification.md#24-the-allocator-of-array-and-vector-spaces) |

### RR-09

| Field | Value |
|---|---|
| Lines | 127–130 |
| Status | `implemented` |
| Clause | The method produce sends a process on a channel, and it may abort, for example when the process holds a Seq name. |
| Code | `rholang/src/rust/interpreter/reified/handle.rs:441-483`, `rholang/src/rust/interpreter/reified/seq.rs:107-126` |
| Formal | `seq_name_iff` (`Namespace.v:1150`), the TLA+ invariant `SeqNotSent` of `SpaceCoordination.tla`, `fail_closed` (`TheoryAdmission.v:398`), `replay_preserves_theory_admission` (`TheoryAdmission.v:851`) |
| Tests | `prop_seq_scan_finds_every_seq_name` in `rholang/tests/reified_seq.rs`, `reducer_seq_payload` in `rholang/tests/reified_seq.rs`, `prop_theory_fail_closed` in `rholang/tests/reified_theory.rs`, `prop_replay_preserves_theory_admission` in `rholang/tests/reified_theory.rs`, `theory_refuted_value_aborts_before_staging` in `rholang/src/rust/interpreter/spaces/theory/tests.rs`, `theory_undetermined_budget0_aborts` in `rholang/src/rust/interpreter/spaces/theory/tests.rs`, `theory_admission_precedes_any_staging` in `rholang/src/rust/interpreter/spaces/generic.rs`, `example_handle_methods` in `rholang/tests/reified_examples_execute.rs`, `example_space_qualifiers_seq_sent` in `rholang/tests/reified_examples_execute.rs`, `example_theory_refuses_a_value` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [agent-api.md#5-handle-methods](agent-api.md#5-handle-methods), [specification.md#43-seq](specification.md#43-seq) |

### RR-10

| Field | Value |
|---|---|
| Lines | 132–145 |
| Status | `implemented` |
| Clause | The method consume receives on a join: pairs of a matcher and a channel, the arrow <-, <= or <<-, and a body that takes a map of the bindings. |
| Code | `rholang/src/rust/interpreter/reified/handle.rs:547-662`, `rholang/src/rust/interpreter/reified/user_agent.rs:329-399` |
| Formal | `matcher_state_roundtrip` (`AgentTranslation.v:783`), `offsets_partition` (`AgentTranslation.v:226`), `env_keys_disjoint_union` (`AgentTranslation.v:324`), `body_equivalence` (`AgentTranslation.v:401`), `guard_fold_sound` (`AgentTranslation.v:596`), `matcher_sound_complete` (`AgentTranslation.v:650`), the TLA+ invariant `JoinSameSpace` of `AgentSpaces.tla` |
| Tests | `agent_wrapper_forwards_to_native` in `rholang/tests/reified_user_agents.rs`, `space_handle_methods_consume_rejects_foreign_matcher` in `rholang/tests/reified_handle.rs`, `prop_body_equivalence` in `rholang/tests/reified_user_agents.rs`, `prop_matcher_sound_complete` in `rholang/tests/reified_user_agents.rs`, `prop_guard_fold_sound` in `rholang/tests/reified_user_agents.rs`, `prop_receive_joins_one_space` in `rholang/tests/reified_user_agents.rs`, `prop_linear_body_runs_at_most_once` in `rholang/tests/reified_user_agents.rs`, `agent_xlate_join_two_matchers` in `rholang/tests/reified_user_agents.rs`, `agent_xlate_guard_span_rejected` in `rholang/tests/reified_user_agents.rs`, `agent_xlate_cross_space_join_rejected` in `rholang/tests/reified_user_agents.rs`, `user_agent_errors_query` in `rholang/tests/reified_user_agents.rs`, `user_agent_errors_pattern_too_deep` in `rholang/tests/reified_user_agents.rs`, `example_agent_wrapper_forwards_to_native` in `rholang/tests/reified_examples_execute.rs`, `example_agent_hashmap_bag` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [agent-api.md#5-handle-methods](agent-api.md#5-handle-methods), [agent-api.md#122-the-protocol](agent-api.md#122-the-protocol), [21-reified-spaces.md#the-methods-of-a-space-agent](../../rholang/21-reified-spaces.md#the-methods-of-a-space-agent) |
| Decisions | [decision 8](decision-register.md#decision-8) |

### RR-11

| Field | Value |
|---|---|
| Lines | 147 |
| Status | `informative` |
| Clause | Patterns will grow into spatial-behavioral types in datalog form, and matchers will then tailor patterns to the inner structure and the theory. |
| Documents | [f1r3lang-integration.md#6-open-items-for-phase-p12](f1r3lang-integration.md#6-open-items-for-phase-p12) |
| Reason | The sentence states a direction of future pattern work and requires no behavior of this version. |

### RR-12

| Field | Value |
|---|---|
| Lines | 149–151 |
| Status | `implemented` |
| Clause | The namespace rho:space holds combinations of stores and collections, and developers can write their own spaces. |
| Code | `rholang/src/rust/interpreter/spaces/factory/urn.rs:207`, `rholang/src/rust/interpreter/spaces/factory/urn.rs:246-253`, `rholang/src/rust/interpreter/reified/system_channels.rs:351-355`, `rholang/src/rust/interpreter/reified/system_channels.rs:359-363`, `models/src/rust/space_factory_urn.rs:311-433` |
| Formal | `registered_families_complete` (`SpaceFactory.v:1571`), `aliases_bind_family_capability` (`SpaceFactory.v:1660`) |
| Tests | `prop_registered_families_complete` in `rholang/tests/reified_families.rs`, `space_factory_families_registered` in `rholang/tests/reified_families.rs`, `prop_aliases_bind_family_capability` in `rholang/tests/reified_families.rs`, `reified_getspaceagent_alias_equal` in `rholang/tests/reified_families.rs`, `example_agent_hashmap_bag` in `rholang/tests/reified_examples_execute.rs`, `example_fips_example` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#15-families](specification.md#15-families), [21-reified-spaces.md#space-families](../../rholang/21-reified-spaces.md#space-families), [21-reified-spaces.md#writing-your-own-space](../../rholang/21-reified-spaces.md#writing-your-own-space) |
| Decisions | [decision 8](decision-register.md#decision-8) |

### RR-13

| Field | Value |
|---|---|
| Lines | 153–157 |
| Status | `implemented` |
| Clause | The outer store HashMap, the store of the current implementation. |
| Code | `rholang/src/rust/interpreter/spaces/outer/hashmap.rs:32-67` |
| Formal | `canon_respects_par_eq` (`Namespace.v:1043`), `name_id_routes_to_sid` (`Namespace.v:1003`) |
| Tests | `prop_canonical_inner_bytes_respects_par_eq` in `rholang/tests/reified_p4_props.rs`, `prop_name_id_routes_to_sid` in `rholang/tests/reified_p4_props.rs`, `space_handle_methods_gensym` in `rholang/tests/reified_handle.rs`, `example_queue_fifo` in `rholang/tests/reified_examples_execute.rs`, `example_bag_basics` in `rholang/tests/reified_examples_execute.rs`, `example_multi_space_isolation` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#21-hashmap](specification.md#21-hashmap), [21-reified-spaces.md#hashmap](../../rholang/21-reified-spaces.md#hashmap) |

### RR-14

| Field | Value |
|---|---|
| Lines | 159–186 |
| Status | `implemented` |
| Clause | The outer store PathMap: channels are paths, a send also sends on every prefix, and a receive on a prefix binds the re-rooted data and leaves the rest. |
| Code | `rholang/src/rust/interpreter/spaces/pathmap/path_codec.rs:283-288`, `rholang/src/rust/interpreter/spaces/pathmap/path_codec.rs:345-349`, `rholang/src/rust/interpreter/spaces/pathmap/path_codec.rs:371-373`, `rholang/src/rust/interpreter/spaces/pathmap/reroot.rs:48-71`, `rholang/src/rust/interpreter/spaces/pathmap/adapter.rs:257-301`, `rholang/src/rust/interpreter/spaces/outer/pathmap.rs:80-91`, `rholang/src/rust/interpreter/spaces/outer/pathmap.rs:153-161`, `rholang/src/rust/interpreter/spaces/pathmap/markers.rs:137-165`, `rholang/src/rust/interpreter/spaces/pathmap/markers.rs:116-118`, `rholang/src/rust/interpreter/spaces/pathmap/markers.rs:152-170`, `rholang/src/rust/interpreter/spaces/pathmap/delivery.rs:169-240`, `rholang/src/rust/interpreter/spaces/pathmap/delivery.rs:350-397`, `rholang/src/rust/interpreter/spaces/pathmap/delivery.rs:521-530` |
| Formal | `prefix_key_iff_prefix` (`PathMapPrefix.v:586`), `candidate_order_total` (`PathMapPrefix.v:1016`), `reroot_ident` (`PathMapPrefix.v:845`), `reroot_compose` (`PathMapPrefix.v:852`), `fips_example` (`PathMapPrefix.v:961`), `reroot_injective` (`PathMapPrefix.v:924`), `prefix_contact_anchored` (`PathMapPrefix.v:1954`), `path_element_rules` (`PathMapPrefix.v:1461`), `marker_record_roundtrip` (`PathMapPrefix.v:1356`), `marker_invariant_preserved` (`PathMapPrefix.v:2566`), the TLA+ invariant `MarkerCount` of `PathMapPrefix.tla`, `vehicle_carries_payload` (`PathMapPrefix.v:1388`), `delivery_vehicle_single_channel` (`PathMapPrefix.v:1917`), `no_match_remains` (`PathMapPrefix.v:2386`), the TLA+ invariant `NoMatchRemains` of `PathMapPrefix.tla`, `peek_restores_delivered_marker` (`PathMapPrefix.v:2016`), `token_ord_reentry` (`PathMapPrefix.v:3132`), the TLA+ invariant `MergeDetectsPrefixContact` of `PathMapPrefix.tla` |
| Tests | `prop_prefix_key_iff_prefix` in `rholang/src/rust/interpreter/spaces/pathmap/path_codec.rs`, `prop_candidate_order_total` in `rholang/src/rust/interpreter/spaces/pathmap/path_codec.rs`, `the_depth_cap_bounds_channel_paths_and_prefixes` in `rholang/src/rust/interpreter/spaces/pathmap/path_codec.rs`, `pathmaps_and_zippers_are_unsupported_at_any_depth` in `rholang/src/rust/interpreter/spaces/pathmap/path_codec.rs`, `prop_reroot_ident` in `rholang/src/rust/interpreter/spaces/pathmap/reroot.rs`, `prop_reroot_compose` in `rholang/src/rust/interpreter/spaces/pathmap/reroot.rs`, `prop_unit_shift` in `rholang/src/rust/interpreter/spaces/pathmap/reroot.rs`, `fips_example` in `rholang/src/rust/interpreter/spaces/pathmap/reroot.rs`, `prop_head_adapter_roundtrip` in `rholang/src/rust/interpreter/spaces/pathmap/adapter.rs`, `prop_reroot_injective` in `rholang/src/rust/interpreter/spaces/pathmap/reroot.rs`, `the_path_channels_are_storage_channels_of_the_space` in `rholang/src/rust/interpreter/spaces/outer/pathmap.rs`, `the_token_is_the_space_counter_and_the_counter_of_every_path` in `rholang/src/rust/interpreter/spaces/outer/pathmap.rs`, `prop_path_element_rules` in `rholang/src/rust/interpreter/spaces/pathmap/delivery.rs`, `check_key_follows_the_path_rules` in `rholang/src/rust/interpreter/spaces/outer/pathmap.rs`, `gensym_names_root_a_private_subtree` in `rholang/src/rust/interpreter/spaces/outer/pathmap.rs`, `prop_marker_record_roundtrip` in `rholang/src/rust/interpreter/spaces/pathmap/markers.rs`, `prop_marker_invariant_preserved` in `rholang/src/rust/interpreter/spaces/pathmap/delivery.rs`, `marker_random_states_differ_by_depth_and_follow_the_path_marker_purpose` in `rholang/src/rust/interpreter/spaces/pathmap/markers.rs`, `prop_vehicle_carries_payload` in `rholang/src/rust/interpreter/spaces/pathmap/markers.rs`, `a_vehicle_is_the_marker_followed_by_the_payload` in `rholang/src/rust/interpreter/spaces/pathmap/markers.rs`, `prop_candidates_cover_the_prefix_in_key_order` in `rholang/src/rust/interpreter/spaces/pathmap/delivery.rs`, `the_fips_paths_deliver_the_exact_path_first_and_rerooted` in `rholang/src/rust/interpreter/spaces/pathmap/delivery.rs`, `prop_waiter_prefixes_are_most_specific_first` in `rholang/src/rust/interpreter/spaces/pathmap/delivery.rs`, `the_waiters_of_a_send_are_the_most_specific_first` in `rholang/src/rust/interpreter/spaces/pathmap/delivery.rs`, `a_removal_retires_the_record_and_every_other_marker` in `rholang/src/rust/interpreter/spaces/pathmap/delivery.rs`, `example_pathmap_prefix_aggregation` in `rholang/tests/reified_examples_execute.rs`, `example_pathmap_hierarchical_spaces` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#25-pathmap-paths-and-re-rooting](specification.md#25-pathmap-paths-and-re-rooting), [specification.md#26-the-pathmap-store](specification.md#26-the-pathmap-store), [pathmap-prefix-delivery.md#2-the-model](pathmap-prefix-delivery.md#2-the-model), [21-reified-spaces.md#pathmap](../../rholang/21-reified-spaces.md#pathmap) |
| Decisions | [decision 9](decision-register.md#decision-9), [decision 51](decision-register.md#decision-51) |
| Note | Lines 181 to 183 of the FIPS write the pathmaps without their opening bar, for example {[2, "hello"], [3, "there"]\|}. The rows read them as pathmaps. |

### RR-15

| Field | Value |
|---|---|
| Lines | 187–193 |
| Status | `informative` |
| Clause | The quantale semantics of sequential processes on sets of paths. |
| Decisions | [decision 87](decision-register.md#decision-87) |
| Reason | The FIPS states the semantics with an open question on line 193. Decision 87 records it as informative, and follow-up rrs-fu-quantale-unification routes it to the FIPS authors. |

### RR-16

| Field | Value |
|---|---|
| Lines | 195–201 |
| Status | `implemented` |
| Clause | The outer store Array of fixed length: new gives the next index up to the maximum, then aborts with an out-of-names error, and indices can be unforgeable. |
| Code | `rholang/src/rust/interpreter/spaces/outer/array.rs:116-180`, `rholang/src/rust/interpreter/spaces/outer/array.rs:127-136`, `rholang/src/rust/interpreter/spaces/allocator.rs:220-234`, `rholang/src/rust/interpreter/spaces/allocator.rs:240-253`, `rholang/src/rust/interpreter/spaces/allocator.rs:87-131`, `rholang/tests/reified_merge_p5.rs:916-977` |
| Formal | `fixed_array_allocates_at_most_size` (`OuterStorage.v:318`), `vector_index_is_counter` (`VectorStore.v:184`), `hashmap_get_put_other` (`HashMapStore.v:256`), `hashmap_iteration_permutation_invariant` (`HashMapStore.v:211`), `fixed_array_indices_sequential` (`OuterStorage.v:350`), `cyclic_array_index_mod_size` (`OuterStorage.v:381`), `vector_gensym_monotonic` (`VectorStore.v:219`), `allocation_batch_atomic` (`OuterStorage.v:407`), `fixed_array_out_of_names_exact` (`OuterStorage.v:335`), `allocator_identities_unique` (`OuterStorage.v:560`), the TLA+ invariant `NoDuplicateIndex` of `CounterConflict.tla` |
| Tests | `prop_fixed_array_allocates_at_most_size` in `rholang/tests/reified_array_vector.rs`, `prop_vector_index_is_counter` in `rholang/tests/reified_array_vector.rs`, `prop_hashmap_get_put_other` in `rholang/tests/reified_array_vector.rs`, `prop_hashmap_iteration_permutation_invariant` in `rholang/tests/reified_array_vector.rs`, `prop_fixed_array_indices_sequential` in `rholang/tests/reified_array_vector.rs`, `prop_cyclic_array_index_mod_size` in `rholang/tests/reified_array_vector.rs`, `prop_vector_gensym_monotonic` in `rholang/tests/reified_array_vector.rs`, `prop_allocation_batch_atomic` in `rholang/tests/reified_array_vector.rs`, `prop_fixed_array_out_of_names_exact` in `rholang/tests/reified_array_vector.rs`, `prop_allocator_identities_unique` in `rholang/tests/reified_array_vector.rs`, `prop_no_duplicate_index` in `rholang/tests/reified_array_vector.rs`, `prop_allocations_race` in `rholang/tests/reified_array_vector.rs`, `space_allocator_race_conflict` in `rholang/tests/reified_array_vector.rs`, `example_array_indexed_space` in `rholang/tests/reified_examples_execute.rs`, `example_array_out_of_names` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#22-array](specification.md#22-array), [specification.md#24-the-allocator-of-array-and-vector-spaces](specification.md#24-the-allocator-of-array-and-vector-spaces), [21-reified-spaces.md#array-and-vector](../../rholang/21-reified-spaces.md#array-and-vector) |
| Decisions | [decision 39](decision-register.md#decision-39) |
| Note | Each index name is an unforgeable id of its space, so a client cannot reach an index ambiently (rule AR-1). |

### RR-17

| Field | Value |
|---|---|
| Lines | 203–205 |
| Status | `implemented` |
| Clause | The outer store Array with reuse: new gives the next index modulo the size. |
| Code | `rholang/src/rust/interpreter/spaces/outer/array.rs:63-91` |
| Formal | `fixed_array_indices_sequential` (`OuterStorage.v:350`), `cyclic_array_index_mod_size` (`OuterStorage.v:381`), `vector_gensym_monotonic` (`VectorStore.v:219`) |
| Tests | `prop_fixed_array_indices_sequential` in `rholang/tests/reified_array_vector.rs`, `prop_cyclic_array_index_mod_size` in `rholang/tests/reified_array_vector.rs`, `prop_vector_gensym_monotonic` in `rholang/tests/reified_array_vector.rs` |
| Documents | [specification.md#22-array](specification.md#22-array), [21-reified-spaces.md#array-and-vector](../../rholang/21-reified-spaces.md#array-and-vector) |
| Decisions | [decision 39](decision-register.md#decision-39) |

### RR-18

| Field | Value |
|---|---|
| Lines | 207–208, 211 |
| Status | `implemented` |
| Clause | The outer store Vector of unbounded length: new grows the vector and gives the index of its last element, and indices can be unforgeable. |
| Code | `rholang/src/rust/interpreter/spaces/outer/vector.rs:33-94`, `rholang/src/rust/interpreter/spaces/outer/vector.rs:44-51`, `rholang/src/rust/interpreter/reified/reducer_hooks.rs:1417-1464` |
| Formal | `fixed_array_allocates_at_most_size` (`OuterStorage.v:318`), `vector_index_is_counter` (`VectorStore.v:184`), `hashmap_get_put_other` (`HashMapStore.v:256`), `hashmap_iteration_permutation_invariant` (`HashMapStore.v:211`), `fixed_array_indices_sequential` (`OuterStorage.v:350`), `cyclic_array_index_mod_size` (`OuterStorage.v:381`), `vector_gensym_monotonic` (`VectorStore.v:219`) |
| Tests | `prop_fixed_array_allocates_at_most_size` in `rholang/tests/reified_array_vector.rs`, `prop_vector_index_is_counter` in `rholang/tests/reified_array_vector.rs`, `prop_hashmap_get_put_other` in `rholang/tests/reified_array_vector.rs`, `prop_hashmap_iteration_permutation_invariant` in `rholang/tests/reified_array_vector.rs`, `prop_fixed_array_indices_sequential` in `rholang/tests/reified_array_vector.rs`, `prop_cyclic_array_index_mod_size` in `rholang/tests/reified_array_vector.rs`, `prop_vector_gensym_monotonic` in `rholang/tests/reified_array_vector.rs`, `reducer_space_routing_allocation_jobs_give_index_names` in `rholang/src/rust/interpreter/reified/reducer_hooks.rs`, `example_vector_indexed_space` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#23-vector](specification.md#23-vector), [21-reified-spaces.md#array-and-vector](../../rholang/21-reified-spaces.md#array-and-vector) |

### RR-19

| Field | Value |
|---|---|
| Lines | 209 |
| Status | `deviation` |
| Clause | When the server runs out of memory, the deploy fails with an out-of-memory error. |
| Decisions | [decision 87](decision-register.md#decision-87) |
| Note | A deploy fails deterministically when it exceeds its phlo or its host-work budget. Memory exhaustion cannot be a consensus outcome, because it depends on the node. |

### RR-20

| Field | Value |
|---|---|
| Lines | 213–215 |
| Status | `implemented` |
| Clause | The outer store HashSet of channels: a send adds the channel to the set, a receive uses only the empty pattern, and joins are allowed. |
| Code | `rholang/src/rust/interpreter/spaces/generic.rs:5519-5524`, `rholang/src/rust/interpreter/spaces/join.rs:401-454` |
| Formal | `hashmap_get_put_other` (`HashMapStore.v:256`), `hashset_unit_rules` (`HashMapStore.v:378`), `qualifier_matrix` (`SpaceFactory.v:438`), the TLA+ invariant `QualifierSupport` of `SpaceCoordination.tla`, `registered_families_complete` (`SpaceFactory.v:1571`) |
| Tests | `space_hashset_keys_channels_and_names_are_those_of_the_hashmap_store` in `rholang/src/rust/interpreter/spaces/outer/hashset.rs`, `prop_hashset_unit_rules` in `rholang/tests/reified_hashset.rs`, `reified_hashset_payload_not_empty` in `rholang/tests/reified_hashset.rs`, `reified_hashset_pattern_not_empty` in `rholang/tests/reified_hashset.rs`, `reified_hashset_guards` in `rholang/tests/reified_hashset.rs`, `space_engine_hashset_families_take_units_only` in `rholang/src/rust/interpreter/spaces/generic.rs`, `space_hashset_accepts_exactly_the_empty_payload` in `rholang/src/rust/interpreter/spaces/outer/hashset.rs`, `space_hashset_accepts_exactly_the_empty_pattern_list` in `rholang/src/rust/interpreter/spaces/outer/hashset.rs`, `pattern_enforcement_hashset_rows` in `rholang/tests/reified_pattern_enforcement.rs`, `reified_hashset_joins` in `rholang/tests/reified_hashset.rs`, `reified_hashset_unit_sends` in `rholang/tests/reified_hashset.rs`, `reified_hashset_disciplines` in `rholang/tests/reified_hashset.rs`, `reified_hashset_head_order` in `rholang/tests/reified_hashset.rs`, `reified_hashset_seq_only` in `rholang/tests/reified_hashset.rs`, `reified_hashset_absent_unit_waits` in `rholang/tests/reified_hashset.rs`, `reducer_qualifier_matrix` in `rholang/tests/reified_temp_seq.rs`, `reified_factory_hashset_takes_only_seq_in_native_execution` in `rholang/src/rust/interpreter/spaces/factory/constructor.rs`, `pattern_enforcement_native_unsupported_pairs_reject` in `rholang/tests/reified_pattern_enforcement.rs`, `reified_hashset_factory_params` in `rholang/tests/reified_hashset.rs`, `space_factory_families_registered` in `rholang/tests/reified_families.rs`, `p7_system_channels_have_the_table_values` in `rholang/src/rust/interpreter/reified/system_channels.rs`, `example_hashset_dedup_spaces` in `rholang/tests/reified_examples_execute.rs`, `example_hashset_rejects_a_payload` in `rholang/tests/reified_examples_execute.rs`, `example_hashset_rejects_a_pattern` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#27-hashset](specification.md#27-hashset), [21-reified-spaces.md#hashset](../../rholang/21-reified-spaces.md#hashset) |
| Decisions | [decision 73](decision-register.md#decision-73) |
| Note | A receive fires with the unit, which is Some(()), or it waits. So a process never observes an absent unit (rule HS-6, decision 73). |

### RR-21

| Field | Value |
|---|---|
| Lines | 217–227 |
| Status | `implemented` |
| Clause | Inner collections: a pair of collection types replaces the two bags, the collections are unindexed, patterns depend on the collection type, and produces need not commute. |
| Code | `rholang/src/rust/interpreter/reified/reducer_hooks.rs:599-615`, `rholang/src/rust/interpreter/reified/reducer_hooks.rs:1721-1766`, `rholang/src/rust/interpreter/reified/reducer_hooks.rs:960-965` |
| Formal | `queue_ord_fresh` (`HeadOnlyDisciplines.v:1050`), `ordered_writes_race_on_counter` (`CounterConflict.v:910`), `ordered_reads_race_on_counter` (`CounterConflict.v:924`), `same_block_ops_chain` (`CounterConflict.v:1113`), `shared_collection_race` (`CounterConflict.v:889`), `merge_without_race_is_serializable` (`CounterConflict.v:1489`), `crossed_stacks_without_removal_counters_fabricate` (`CounterConflict.v:2578`), the TLA+ invariant `NoMergedOrderFabrication` of `CounterConflict.tla`, `query_bind_routes_its_channel` (`SpaceRefs.v:1946`) |
| Tests | `prop_queue_ord_fresh` in `rholang/tests/reified_p4_props.rs`, `space_queue_counter_bump_shape` in `rholang/tests/reified_queue.rs`, `space_queue_counter_race_conflict_enq_deq` in `rholang/tests/reified_merge.rs`, `space_queue_counter_race_conflict_same_block_one_chain` in `rholang/tests/reified_merge.rs`, `space_collection_conflicts_cell_writes_race` in `rholang/tests/reified_merge_p5.rs`, `space_collection_conflicts_stack_push_pop_race` in `rholang/tests/reified_merge_p5.rs`, `prop_cell_ops_race_on_counter` in `rholang/tests/reified_merge_p5.rs`, `prop_nonracing_queue_branches_serializable` in `rholang/tests/reified_p4_props.rs`, `space_queue_counter_race_conflict_unrelated_queues_merge` in `rholang/tests/reified_merge.rs`, `prop_merged_stack_state_is_serial` in `rholang/tests/reified_merge_p5.rs`, `space_collection_conflicts_crossed_stacks` in `rholang/tests/reified_merge_p5.rs`, `prop_query_bind_routes_inner_channel` in `rholang/src/rust/interpreter/reified/reducer_hooks.rs`, `prop_query_source_routes_inner_channel` in `rholang/tests/reified_hooks_p5.rs`, `example_determinism_demo` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#3-inner-collections](specification.md#3-inner-collections), [21-reified-spaces.md#inner-collections](../../rholang/21-reified-spaces.md#inner-collections) |
| Decisions | [M-1](decision-register.md#m-1), [decision 31](decision-register.md#decision-31) |

### RR-22

| Field | Value |
|---|---|
| Lines | 229–231 |
| Status | `implemented` |
| Clause | Bags: the multiset semantics of the default space. |
| Code | `rholang/src/rust/interpreter/spaces/disciplines/scan.rs:1340-1381`, `rholang/tests/reified_merge_p5.rs:686-741` |
| Formal | `bag_visit_order_canonical` (`DataCollection.v:251`), `waiter_visit_order_canonical` (`ContinuationCollection.v:338`), `bag_consume_first_spatial_match` (`DataCollection.v:453`), `guard_once_greedy_like_head` (`Match.v:687`), `bag_multiplicity_preserved` (`DataCollection.v:567`), `bag_removal_keeps_equal_duplicate` (`DataCollection.v:586`), `bag_persistent_match_not_removed` (`DataCollection.v:544`), `bag_peek_restore_is_host_second_send` (`DataCollection.v:733`), `peek_restore_equals_host_reproduce` (`GenericRSpace.v:1167`), the TLA+ invariant `PeekRestoresRecord` of `GenericRSpace.tla`, `scan_reissue_identity_stable` (`GenericRSpace.v:1311`) |
| Tests | `prop_bag_visit_order_canonical` in `rholang/tests/reified_bag.rs`, `prop_bag_waiter_visit_order_canonical` in `rholang/tests/reified_bag.rs`, `prop_bag_consume_first_spatial_match` in `rholang/tests/reified_bag.rs`, `prop_engine_guard_checked_once_as_the_host_checks_it` in `rholang/tests/reified_engine_props.rs`, `prop_bag_multiplicity_preserved` in `rholang/tests/reified_bag.rs`, `prop_bag_removal_keeps_equal_duplicate` in `rholang/tests/reified_bag.rs`, `prop_bag_observer_calls_equal_host` in `rholang/tests/reified_bag.rs`, `prop_bag_restore_registers_the_record_authority` in `rholang/tests/reified_bag.rs`, `prop_engine_scan_reissue_keeps_identity` in `rholang/tests/reified_engine_props.rs`, `example_bag_basics` in `rholang/tests/reified_examples_execute.rs`, `example_bag_peeks_and_persistence` in `rholang/tests/reified_examples_execute.rs`, `example_bag_rejects_a_query` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#32-bag](specification.md#32-bag), [21-reified-spaces.md#bag](../../rholang/21-reified-spaces.md#bag) |
| Decisions | [decision 41](decision-register.md#decision-41), [decision 45](decision-register.md#decision-45) |

### RR-23

| Field | Value |
|---|---|
| Lines | 233–235 |
| Status | `implemented` |
| Clause | Sets: each datum or continuation appears at most once, and a second equal send is idempotent. |
| Code | `rholang/src/rust/interpreter/spaces/disciplines/set.rs:204-257`, `rholang/src/rust/interpreter/spaces/disciplines/scan.rs:947-1265`, `rholang/src/rust/interpreter/spaces/disciplines/scan.rs:1411-1507`, `rholang/src/rust/interpreter/spaces/disciplines/scan.rs:648-715`, `rholang/src/rust/interpreter/spaces/disciplines/scan.rs:1511-1521`, `rholang/src/rust/interpreter/spaces/disciplines/scan.rs:633-637` |
| Formal | `set_key_injective` (`SetDiscipline.v:303`), `set_view_dedups_equal_pars` (`SetDiscipline.v:710`), `set_consume_retires_class` (`SetDiscipline.v:798`), `set_retire_comm_lines_up` (`SetDiscipline.v:878`), `set_produce_fire_retires_stored_class` (`SetDiscipline.v:1157`), the TLA+ invariant `SetClassRetired` of `GenericRSpace.tla`, `set_peek_restores_representative_only` (`SetDiscipline.v:1211`), `set_membership_full_compare_on_equal_hash` (`SetDiscipline.v:1280`), `scan_reissue_identity_stable` (`GenericRSpace.v:1311`), `set_retirements_race` (`CounterConflict.v:1992`), `unordered_set_writes_commute` (`CounterConflict.v:2024`) |
| Tests | `prop_set_key_injective` in `rholang/tests/reified_set.rs`, `space_set_discipline_key_length_prefix_no_collision` in `rholang/tests/reified_set.rs`, `prop_set_view_dedups_equal_pars` in `rholang/tests/reified_set.rs`, `space_set_discipline_duplicates_stay_distinct_records` in `rholang/tests/reified_set.rs`, `prop_set_consume_retires_class` in `rholang/tests/reified_set.rs`, `prop_set_retire_comm_lines_up` in `rholang/tests/reified_set.rs`, `prop_set_produce_fire_retires_stored_class` in `rholang/tests/reified_set.rs`, `prop_set_class_retired_any_schedule` in `rholang/tests/reified_set.rs`, `space_set_discipline_guarded_waiter_then_equal_send_retires_class` in `rholang/tests/reified_set.rs`, `prop_set_peek_restores_representative_only` in `rholang/tests/reified_set.rs`, `space_set_discipline_peek_restore_reoffer_retires_class` in `rholang/tests/reified_set.rs`, `prop_set_membership_full_compare_on_equal_hash` in `rholang/tests/reified_set.rs`, `space_set_discipline_persistent_reissue_keeps_identity` in `rholang/tests/reified_set.rs`, `space_set_discipline_equal_receives_stay_two_waiters` in `rholang/tests/reified_set.rs`, `prop_set_retirements_race` in `rholang/tests/reified_merge_p5.rs`, `prop_unordered_set_writes_commute` in `rholang/tests/reified_merge_p5.rs`, `space_collection_conflicts_set_retirements_race` in `rholang/tests/reified_merge_p5.rs`, `space_collection_conflicts_set_writes_merge` in `rholang/tests/reified_merge_p5.rs`, `example_set_idempotent` in `rholang/tests/reified_examples_execute.rs`, `example_set_peeks_and_persistence` in `rholang/tests/reified_examples_execute.rs`, `example_set_rejects_a_send_param` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#35-set](specification.md#35-set), [21-reified-spaces.md#set](../../rholang/21-reified-spaces.md#set) |
| Decisions | [decision 39](decision-register.md#decision-39), [decision 41](decision-register.md#decision-41) |
| Note | Equal data form one class (rule ST-2). Two waiters are equal only when their bytes are equal (rule ST-9, item F14 of decision 39). |

### RR-24

| Field | Value |
|---|---|
| Lines | 237–241 |
| Status | `implemented` |
| Clause | Cells: at most one datum or continuation on a channel, and a second send is an error. |
| Code | `rholang/src/rust/interpreter/spaces/generic.rs:2424-2439`, `rholang/tests/reified_merge_p5.rs:745-793` |
| Formal | `cell_capacity_one` (`CellDiscipline.v:672`), the TLA+ invariant `CellCapacity` of `GenericRSpace.tla`, `cell_restore_never_occupied` (`CellDiscipline.v:858`), `cell_reentry_as_fresh` (`CellDiscipline.v:889`) |
| Tests | `prop_cell_capacity_one` in `rholang/tests/reified_cell.rs`, `space_disciplines_cell_second_produce_occupied` in `rholang/tests/reified_cell.rs`, `space_disciplines_cell_every_operation_bumps_counter` in `rholang/tests/reified_cell.rs`, `space_disciplines_cell_branch_writes_race_on_the_counter` in `rholang/tests/reified_cell.rs`, `prop_cell_restore_never_occupied` in `rholang/tests/reified_cell.rs`, `prop_cell_reentry_as_fresh` in `rholang/tests/reified_cell.rs`, `example_cell_exactly_once` in `rholang/tests/reified_examples_execute.rs`, `example_cell_occupied` in `rholang/tests/reified_examples_execute.rs`, `example_cell_peeks_and_persistence` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#33-cell](specification.md#33-cell), [21-reified-spaces.md#cell](../../rholang/21-reified-spaces.md#cell) |
| Decisions | [decision 39](decision-register.md#decision-39) |
| Note | Line 239 notes that exactly one datum would need more machinery. That sentence requires no behavior. |

### RR-25

| Field | Value |
|---|---|
| Lines | 243–245 |
| Status | `implemented` |
| Clause | Queues: FIFO semantics, only the heads interact, one queue is empty or the heads do not match, and nodes agree on the arrival order. |
| Code | `rholang/src/rust/interpreter/spaces/disciplines/queue.rs:28-63` |
| Formal | `queue_head_is_min_ord` (`HeadOnlyDisciplines.v:530`), `queue_head_blocks` (`HeadOnlyDisciplines.v:1222`), the TLA+ invariant `HeadBlocks` of `HeadOnlyDiscipline.tla`, `queue_loop_bounded_or_reissues` (`HeadOnlyDisciplines.v:1384`), `queue_fifo` (`HeadOnlyDisciplines.v:1139`), `queue_fifo_with_reentries` (`HeadOnlyDisciplines.v:2950`), the TLA+ invariant `FifoOrder` of `HeadOnlyDiscipline.tla`, `queue_reissue_keeps_ord` (`HeadOnlyDisciplines.v:2377`), `queue_head_blocks_reentry` (`HeadOnlyDisciplines.v:3025`), `persistent_reentry_never_spent` (`HeadOnlyDisciplines.v:2346`), the TLA+ invariant `ReissueKeepsOrd` of `HeadOnlyDiscipline.tla`, `queue_ord_fresh` (`HeadOnlyDisciplines.v:1050`), `first_ops_race_on_space_counter` (`CounterConflict.v:939`) |
| Tests | `prop_queue_candidates_head_only` in `rholang/tests/reified_p4_props.rs`, `space_queue_head_only_data_head_blocks` in `rholang/tests/reified_queue.rs`, `space_queue_head_only_waiter_head_blocks` in `rholang/tests/reified_queue.rs`, `space_queue_reissue_persistent_send` in `rholang/tests/reified_queue.rs`, `space_queue_reissue_peek_produce` in `rholang/tests/reified_queue.rs`, `space_queue_divergent_pair_exhausts_phlo` in `rholang/tests/reified_queue.rs`, `space_queue_head_only_fifo_delivery` in `rholang/tests/reified_queue.rs`, `prop_queue_fifo_with_reentries` in `rholang/tests/reified_p4_props.rs`, `space_joins_queue_programs_run_as_joins` in `rholang/tests/reified_joins.rs`, `space_queue_send_params_rejected` in `rholang/tests/reified_queue.rs`, `space_queue_query_rejected` in `rholang/tests/reified_queue.rs`, `space_queue_reissue_reenters_ahead_of_younger_records` in `rholang/tests/reified_queue.rs`, `prop_queue_reissue_keeps_first_round_ord` in `rholang/tests/reified_p4_props.rs`, `prop_queue_reentry_head_blocks` in `rholang/tests/reified_p4_props.rs`, `prop_queue_ord_fresh` in `rholang/tests/reified_p4_props.rs`, `space_queue_counter_bump_shape` in `rholang/tests/reified_queue.rs`, `prop_first_ops_race_on_space_counter` in `rholang/tests/reified_p4_props.rs`, `space_counter_first_operation_bumps_the_space_counter` in `rholang/src/rust/interpreter/spaces/counter.rs`, `space_counter_rejects_corrupt_counters` in `rholang/src/rust/interpreter/spaces/counter.rs`, `example_queue_fifo` in `rholang/tests/reified_examples_execute.rs`, `example_queue_peeks_and_persistence` in `rholang/tests/reified_examples_execute.rs`, `example_queue_rejects_a_send_param` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#31-queue](specification.md#31-queue), [specification.md#30-ordered-collections](specification.md#30-ordered-collections), [21-reified-spaces.md#queue](../../rholang/21-reified-spaces.md#queue) |
| Decisions | [decision 11](decision-register.md#decision-11), [decision 31](decision-register.md#decision-31), [M-1](decision-register.md#m-1) |

### RR-26

| Field | Value |
|---|---|
| Lines | 247–249 |
| Status | `implemented` |
| Clause | Priority queues: a queue for each priority, and patterns can name a priority. |
| Code | `rholang/src/rust/interpreter/spaces/disciplines/lanes.rs:7-44`, `rholang/src/rust/interpreter/spaces/disciplines/lanes.rs:46-82`, `rholang/src/rust/interpreter/spaces/record.rs:879-1260`, `rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:393-431`, `rholang/src/rust/interpreter/spaces/disciplines/lanes.rs:75-81`, `rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:584-612`, `rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:620-683`, `rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:981-1118`, `rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:1123-1132`, `rholang/src/rust/interpreter/spaces/disciplines/priority_queue.rs:830-837` |
| Formal | `pq_level_order` (`PriorityQueue.v:868`), `pq_unprioritized_lowest` (`PriorityQueue.v:892`), the TLA+ invariant `PqLevelOrder` of `PriorityQueue.tla`, `pq_restricted_waiter_in_level_queue` (`PriorityQueue.v:1926`), `pq_unrestricted_first_matching_level` (`PriorityQueue.v:1948`), `pq_fifo_within_level` (`PriorityQueue.v:2156`), `pq_head_blocks_within_level` (`PriorityQueue.v:2329`), `pq_no_level_storage` (`PriorityQueue.v:3908`), `datum_priority_slot_roundtrip` (`RecordEnvelope.v:1506`), `continuation_policy_slot_roundtrip` (`RecordEnvelope.v:1542`), `pq_settle_oldest_first` (`PriorityQueue.v:1904`), `pq_settle_order_matters` (`PriorityQueue.v:4086`), `pq_produce_preserves_inv` (`PriorityQueue.v:1395`), `pq_without_settle_breaks_inv` (`PriorityQueue.v:4007`), the TLA+ invariant `PqOldestFirst` of `PriorityQueue.tla`, `pq_consume_preserves_inv` (`PriorityQueue.v:1409`), `pq_peek_restores_head` (`PriorityQueue.v:2491`), `pq_settle_keeps_unspent_records` (`PriorityQueue.v:2869`), the TLA+ invariant `NoRecordLost` of `PriorityQueue.tla`, `pq_priority_domain` (`PriorityQueue.v:3735`), `pq_settle_terminates` (`PriorityQueue.v:3186`), `pq_reissue_takes_level_fifo_place` (`PriorityQueue.v:2678`), `pq_reentered_waiter_keeps_seniority` (`PriorityQueue.v:2721`), the TLA+ invariant `TxnFiresBounded` of `PriorityQueue.tla` |
| Tests | `prop_pq_level_order` in `rholang/tests/reified_priority_queue.rs`, `space_disciplines_pq_ascending_levels` in `rholang/tests/reified_priority_queue.rs`, `space_disciplines_pq_unprioritized_lowest` in `rholang/tests/reified_priority_queue.rs`, `prop_pq_fifo_order_any_schedule` in `rholang/tests/reified_priority_queue.rs`, `space_disciplines_pq_restricted_waiter_waits_in_level_queue` in `rholang/tests/reified_priority_queue.rs`, `space_disciplines_pq_head_blocks_within_level` in `rholang/tests/reified_priority_queue.rs`, `prop_pq_no_level_storage` in `rholang/tests/reified_priority_queue.rs`, `prop_pq_record_slots_roundtrip` in `rholang/tests/reified_priority_queue.rs`, `prop_pq_oldest_first_any_schedule` in `rholang/tests/reified_priority_queue.rs`, `prop_pq_no_pending_match_any_schedule` in `rholang/tests/reified_priority_queue.rs`, `space_disciplines_pq_older_waiter_wins` in `rholang/tests/reified_priority_queue.rs`, `space_disciplines_pq_guarded_unrestricted_receive_parks` in `rholang/tests/reified_priority_queue.rs`, `space_disciplines_pq_peek_restores_head` in `rholang/tests/reified_priority_queue.rs`, `space_disciplines_pq_produce_peek_waiter_reoffers` in `rholang/tests/reified_priority_queue.rs`, `prop_pq_priority_domain` in `rholang/tests/reified_priority_queue.rs`, `space_disciplines_pq_priorities_bound_rejects` in `rholang/tests/reified_priority_queue.rs`, `space_disciplines_pq_priority_param_type_checked` in `rholang/tests/reified_priority_queue.rs`, `prop_pq_txn_fires_bounded_at_the_cap` in `rholang/tests/reified_priority_queue.rs`, `prop_pq_reissue_keeps_ord_any_schedule` in `rholang/tests/reified_priority_queue.rs`, `example_priority_queue` in `rholang/tests/reified_examples_execute.rs`, `example_priority_queue_peeks_and_persistence` in `rholang/tests/reified_examples_execute.rs`, `example_priority_queue_rejects_a_level` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#36-priority-queue](specification.md#36-priority-queue), [21-reified-spaces.md#priority-queue](../../rholang/21-reified-spaces.md#priority-queue) |
| Decisions | [decision 11](decision-register.md#decision-11), [decision 38](decision-register.md#decision-38) |

### RR-27

| Field | Value |
|---|---|
| Lines | 251–253 |
| Status | `implemented` |
| Clause | Stacks: FILO semantics, and only the tops interact. |
| Code | `rholang/src/rust/interpreter/spaces/settle.rs:453-577`, `rholang/src/rust/interpreter/spaces/generic.rs:3791-3804` |
| Formal | `stack_head_is_max_ord` (`StackDiscipline.v:564`), `stack_head_blocks` (`StackDiscipline.v:1356`), `stack_reentry_at_lifo_place` (`StackDiscipline.v:1452`), `stack_reissue_keeps_ord` (`StackDiscipline.v:1584`), the TLA+ invariant `ReissueKeepsOrd` of `StackDiscipline.tla`, `stack_produce_preserves_inv` (`StackDiscipline.v:1051`), `stack_settle_keeps_unspent_records` (`StackDiscipline.v:2159`), `stack_peek_restore_needs_no_settle` (`StackDiscipline.v:1413`), the TLA+ invariant `StackNoPendingMatch` of `StackDiscipline.tla`, `stack_settle_terminates` (`StackDiscipline.v:1967`), the TLA+ invariant `TxnFiresBounded` of `StackDiscipline.tla` |
| Tests | `prop_stack_head_is_max_ord` in `rholang/tests/reified_stack.rs`, `prop_stack_reissue_keeps_ord_any_schedule` in `rholang/tests/reified_stack.rs`, `prop_stack_no_pending_match_any_schedule` in `rholang/tests/reified_stack.rs`, `prop_stack_txn_fires_bounded_at_the_cap` in `rholang/tests/reified_stack.rs`, `prop_stack_settle_bounded_any_schedule` in `rholang/tests/reified_stack.rs`, `space_stack_plan_after_a_full_settle_reissues_the_operation` in `rholang/tests/reified_stack.rs`, `example_stack_lifo` in `rholang/tests/reified_examples_execute.rs`, `example_stack_peeks_and_persistence` in `rholang/tests/reified_examples_execute.rs`, `example_stack_rejects_a_query` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#34-stack](specification.md#34-stack), [21-reified-spaces.md#stack](../../rholang/21-reified-spaces.md#stack) |
| Decisions | [decision 11](decision-register.md#decision-11), [decision 49](decision-register.md#decision-49) |

### RR-28

| Field | Value |
|---|---|
| Lines | 255–257 |
| Status | `implemented` |
| Clause | A bag and a vector DB: receives wait in a bag, data goes into a vector DB, a pattern gives a vector and a similarity bound, and a receive takes one vector within the bound. |
| Code | `rholang/src/rust/interpreter/spaces/factory/defaults.rs:68-122`, `rholang/src/rust/interpreter/spaces/factory/constructor.rs:264-295`, `rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:796-817`, `rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:590-690`, `rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:1028-1129`, `rholang/src/rust/interpreter/tensor/exact.rs:478-521`, `rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:2235-2326`, `rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:1369-1429`, `rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:949-984`, `rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:1817-1905`, `rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:1477-1525`, `rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs:3478-3526`, `rholang/src/rust/interpreter/spaces/join.rs:401-454`, `rholang/src/rust/interpreter/spaces/generic.rs:2424-2439`, `rholang/src/rust/interpreter/tensor/exact.rs:578-599` |
| Formal | `vectordb_params_validated` (`SpaceFactory.v:2514`), `cosine_squared_form_iff` (`Similarity.v:412`), `cosine_order_squared_form` (`Similarity.v:471`), `euclidean_radius_squared_form` (`Similarity.v:545`), `binary_laws` (`Similarity.v:1042`), `selection_total_order` (`Similarity.v:1169`), `topk_deterministic` (`Similarity.v:1199`), `list_selection_deterministic` (`Similarity.v:1278`), the TLA+ invariant `ListAtomic` of `VectorDb.tla`, `vdb_txn_bounded` (`VectorDbDiscipline.v:924`), the TLA+ invariant `CascadeTerminates` of `VectorDb.tla`, `vdb_no_record_lost` (`VectorDbDiscipline.v:1082`), the TLA+ invariant `PeekConservation` of `VectorDb.tla`, `vdb_join_removal_exposes_nothing` (`VectorDbDiscipline.v:781`), `capped_writes_soft_bound` (`CounterConflict.v:2383`), `squared_forms_embed` (`Similarity.v:1000`), `root_midpoint_nearest` (`VectorOps.v:367`) |
| Tests | `prop_vectordb_params_validated` in `rholang/tests/reified_families.rs`, `reified_factory_gate_9_needs_the_default_qualifier_and_an_indexed_metric` in `rholang/src/rust/interpreter/spaces/factory/constructor.rs`, `space_vectordb_payload_is_the_vector_by_default` in `rholang/tests/reified_vectordb.rs`, `space_vectordb_peek_restores_records_with_their_embedding` in `rholang/tests/reified_vectordb.rs`, `space_native_vectors_every_kind_enters_exactly` in `rholang/tests/reified_vectordb.rs`, `space_vectordb_errors_have_their_codes` in `rholang/tests/reified_vectordb.rs`, `space_vectordb_source_errors_have_their_codes` in `rholang/tests/reified_vectordb.rs`, `space_vectordb_fixed_point_and_rational_thresholds` in `rholang/tests/reified_vectordb.rs`, `prop_selection_order_is_a_strict_total_order` in `rholang/src/rust/interpreter/tensor/exact.rs`, `space_vectordb_bounds_select_the_eligible_records` in `rholang/tests/reified_vectordb.rs`, `space_vectordb_ids_keep_only_the_listed_records` in `rholang/tests/reified_vectordb.rs`, `space_vectordb_binary_metrics_read_binary_vectors_only` in `rholang/tests/reified_vectordb.rs`, `space_vectordb_lists_without_a_vector_keep_the_canonical_order` in `rholang/tests/reified_vectordb.rs`, `space_similarity_topk_reference_cases_through_the_space` in `rholang/tests/reified_vectordb.rs`, `space_hash_collision_tie_duplicates_are_interchangeable` in `rholang/tests/reified_vectordb.rs`, `space_hash_collision_tie_forced_equal_source_hashes_select_one_value` in `rholang/tests/reified_vectordb.rs`, `prop_topk_is_deterministic` in `rholang/src/rust/interpreter/tensor/exact.rs`, `space_vectordb_single_mode_takes_the_best_record` in `rholang/tests/reified_vectordb.rs`, `space_vectordb_guard_refusal_parks_until_a_worse_record_passes` in `rholang/tests/reified_vectordb.rs`, `space_vector_discipline_restart_keeps_values_ties_and_order` in `rholang/tests/reified_vectordb.rs`, `prop_vectordb_list_is_ordered_and_complete` in `rholang/tests/reified_vectordb.rs`, `space_vectordb_list_mode_delivers_one_list_in_selection_order` in `rholang/tests/reified_vectordb.rs`, `rspace_list_random_state_zero_one_and_two_members` in `rholang/tests/reified_vectordb.rs`, `prop_vectordb_list_fires_are_atomic` in `rholang/tests/reified_vectordb.rs`, `space_vectordb_ce1_list_waiter_fires_with_the_incoming_datum` in `rholang/tests/reified_vectordb.rs`, `prop_vectordb_transactions_are_bounded` in `rholang/tests/reified_vectordb.rs`, `space_vectordb_list_cascade_carries_at_the_cap` in `rholang/tests/reified_vectordb.rs`, `prop_vectordb_records_are_conserved` in `rholang/tests/reified_vectordb.rs`, `space_vectordb_cascade_peek_restores_the_lifted_member` in `rholang/tests/reified_vectordb.rs`, `space_vectordb_join_binds_each_position_by_its_query` in `rholang/tests/reified_vectordb.rs`, `space_vectordb_list_mode_in_a_join_fails` in `rholang/tests/reified_vectordb.rs`, `space_vectordb_join_waiter_fires_on_an_eligible_send` in `rholang/tests/reified_vectordb.rs`, `prop_vectordb_capacity_soft_bound` in `rholang/tests/reified_merge_p5.rs`, `space_collection_conflicts_vectordb_capacity_soft_bound` in `rholang/tests/reified_merge_p5.rs`, `prop_vectordb_writes_commute` in `rholang/tests/reified_merge_p5.rs`, `space_vectordb_capacity_bounds_records_not_waiters` in `rholang/tests/reified_vectordb.rs`, `prop_dyadic_and_rational_kernels_agree` in `rholang/src/rust/interpreter/tensor/exact.rs`, `space_vectordb_scores_are_the_correctly_rounded_exact_scores` in `rholang/tests/reified_vectordb.rs`, `tensor_square_roots_are_correctly_rounded` in `rholang/src/rust/interpreter/tensor/num.rs`, `space_native_vectordb_errors_reproduce` in `rholang/tests/reified_native_spaces.rs`, `space_native_vectordb_cascade_carries_at_the_cap` in `rholang/tests/reified_native_spaces.rs`, `space_native_vectordb_denial_replay` in `rholang/tests/reified_native_spaces.rs`, `example_vectordb_queries` in `rholang/tests/reified_examples_execute.rs`, `example_vectordb_list_scores` in `rholang/tests/reified_examples_execute.rs`, `example_vectordb_multi_topic` in `rholang/tests/reified_examples_execute.rs`, `example_vectordb_peeks_and_persistence` in `rholang/tests/reified_examples_execute.rs`, `example_vectordb_dimension_mismatch` in `rholang/tests/reified_examples_execute.rs`, `example_vectordb_capacity_exceeded` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#39-vectordb](specification.md#39-vectordb), [21-reified-spaces.md#vectordb-spaces](../../rholang/21-reified-spaces.md#vectordb-spaces) |
| Decisions | [decision 52](decision-register.md#decision-52), [decision 66](decision-register.md#decision-66), [decision 71](decision-register.md#decision-71) |

### RR-29

| Field | Value |
|---|---|
| Lines | 259 |
| Status | `future` |
| Clause | Look into tensor logic for a vector DB. |
| Documents | [21-reified-spaces.md#vector-arithmetic](../../rholang/21-reified-spaces.md#vector-arithmetic) |
| Decisions | [decision 62](decision-register.md#decision-62), [decision 65](decision-register.md#decision-65) |
| Note | The design study of decision 52 answered the question. The TensorLogic GSLT and its FLT surface come in phase P12, after the readiness signal of F1r3lang. |

### RR-30

| Field | Value |
|---|---|
| Lines | 261 |
| Status | `future` |
| Clause | Look into nonlinear operations on vectors, such as majority for binary vectors and an elementwise sigmoid. |
| Decisions | [decision 87](decision-register.md#decision-87), [decision 88](decision-register.md#decision-88) |
| Note | These operations come with tensor logic in phase P12. The five exact operations of rho:lang:vector exist now (example vector-ops). |

### RR-31

| Field | Value |
|---|---|
| Lines | 263–265 |
| Status | `informative` |
| Clause | Other collections: fixed-size versions of the collection types, and collections that mix data types. |
| Decisions | [decision 87](decision-register.md#decision-87) |
| Reason | Decision 87 records the passage as informative. Its fixed-size versions exist where the legacy branch had them: the size of an Array and the capacity of a VectorDB collection. |

### RR-32

| Field | Value |
|---|---|
| Lines | 267–269 |
| Status | `implemented` |
| Clause | Qualifiers change the behavior and the persistence of the channels of a space, and some outer structures, such as sets, require the sequential qualifier. |
| Code | `rholang/src/rust/interpreter/spaces/types/qualifier.rs:23-31` |
| Formal | `qualifier_matrix` (`SpaceFactory.v:438`), the TLA+ invariant `QualifierSupport` of `SpaceCoordination.tla` |
| Tests | `prop_qualifier_matrix` in `rholang/tests/reified_p4_props.rs`, `space_urn_factory_qualifier_support_matrix` in `models/tests/space_factory_urn.rs`, `reified_hashset_seq_only` in `rholang/tests/reified_hashset.rs`, `reified_hashset_absent_unit_waits` in `rholang/tests/reified_hashset.rs`, `reducer_qualifier_matrix` in `rholang/tests/reified_temp_seq.rs`, `reified_factory_hashset_takes_only_seq_in_native_execution` in `rholang/src/rust/interpreter/spaces/factory/constructor.rs`, `pattern_enforcement_native_unsupported_pairs_reject` in `rholang/tests/reified_pattern_enforcement.rs`, `example_hashset_dedup_spaces` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#4-qualifiers](specification.md#4-qualifiers), [21-reified-spaces.md#qualifiers](../../rholang/21-reified-spaces.md#qualifiers) |
| Decisions | [decision 73](decision-register.md#decision-73) |

### RR-33

| Field | Value |
|---|---|
| Lines | 269 |
| Status | `informative` |
| Clause | Pathmaps with quantale semantics require the sequential qualifier. |
| Decisions | [decision 87](decision-register.md#decision-87) |
| Reason | The clause depends on the quantale semantics of row RR-15, which decision 87 records as informative. A PathMap space takes every qualifier. |

### RR-34

| Field | Value |
|---|---|
| Lines | 271–273 |
| Status | `implemented` |
| Clause | The default qualifier: a channel persists, and it allows concurrent access. |
| Code | `rholang/src/rust/interpreter/spaces/types/qualifier.rs:33-61` |
| Formal | `qualifier_matrix` (`SpaceFactory.v:438`), the TLA+ invariant `DurableSurvivesRestart` of `NativeHandles.tla`, the TLA+ invariant `HandleCallsMergeLikeContracts` of `NativeHandles.tla` |
| Tests | `prop_qualifier_matrix` in `rholang/tests/reified_p4_props.rs`, `space_urn_factory_qualifier_support_matrix` in `models/tests/space_factory_urn.rs`, `prop_handle_survives_checkpoint_reset` in `rholang/tests/reified_p4_props.rs`, `reified_handle_persisted_restart_replay` in `rholang/tests/reified_queue_restart.rs`, `prop_attach_warm_cold_cost_equal` in `rholang/tests/reified_rehydrate.rs`, `space_lazy_attach` in `rholang/tests/reified_rehydrate.rs`, `space_checkpoint_reset` in `rholang/tests/reified_rehydrate.rs`, `space_attach_warm_cold_cost_equal` in `rholang/tests/reified_rehydrate.rs`, `space_restart_rehydrate_three_processes` in `rholang/tests/reified_restart_rehydrate.rs`, `reified_handle_cross_block_merge_no_conflict` in `rholang/tests/reified_merge.rs`, `example_space_qualifier_default` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#41-default](specification.md#41-default), [21-reified-spaces.md#default](../../rholang/21-reified-spaces.md#default) |

### RR-35

| Field | Value |
|---|---|
| Lines | 275–277 |
| Status | `implemented` |
| Clause | The temp qualifier: concurrent access, and the data and continuations of the channel are never written to disk. |
| Code | `rholang/src/rust/interpreter/spaces/ephemeral.rs:121-291` |
| Formal | `qualifier_matrix` (`SpaceFactory.v:438`), `ephemeral_durable_disjoint` (`Namespace.v:867`), `sweep_complete` (`VolatileSweep.v:1714`), `sweep_exact` (`VolatileSweep.v:894`), `sweep_root_neutral` (`VolatileSweep.v:1266`), `dead_ephemeral_dangling` (`VolatileSweep.v:1759`), the TLA+ invariant `QualifierSupport` of `SpaceCoordination.tla` |
| Tests | `reducer_qualifier_matrix` in `rholang/tests/reified_temp_seq.rs`, `space_temp_replay_equal_hashmap` in `rholang/tests/reified_temp_seq.rs`, `space_temp_replay_equal_vector` in `rholang/tests/reified_temp_seq.rs`, `reified_factory_ephemeral_staging` in `rholang/tests/reified_rehydrate.rs`, `pattern_enforcement_temp_rows` in `rholang/tests/reified_pattern_enforcement.rs`, `reified_ephemeral_cost_charged` in `rholang/tests/reified_temp_seq.rs`, `prop_sweep_complete` in `rholang/tests/reified_temp_seq.rs`, `space_temp_swept_at_deploy_end` in `rholang/tests/reified_temp_seq.rs`, `space_temp_join_swept` in `rholang/tests/reified_temp_seq.rs`, `space_temp_failed_deploy_reverts` in `rholang/tests/reified_temp_seq.rs`, `space_temp_many_volatile_keys_export` in `rholang/tests/reified_temp_seq.rs`, `reified_sweep_runs_at_each_entry_point_of_the_reducer` in `rholang/src/rust/interpreter/reified/reducer_hooks.rs`, `reified_sweep_runs_at_each_entry_point_native` in `rholang/tests/reified_temp_seq.rs`, `prop_dead_ephemeral_dangling` in `rholang/src/rust/interpreter/reified/runtime.rs`, `reified_ephemeral_dangling_next_deploy` in `rholang/tests/reified_temp_seq.rs`, `reified_ephemeral_dangling_charge_is_fixed` in `rholang/tests/reified_temp_seq.rs`, `reducer_dangling_handle` in `rholang/tests/reified_rehydrate.rs`, `mirror_temp_never_mirrored` in `rholang/tests/reified_vectordb_chroma.rs`, `pattern_enforcement_native_unsupported_pairs_reject` in `rholang/tests/reified_pattern_enforcement.rs`, `prop_seq_qualifier_support` in `rholang/tests/reified_seq.rs`, `pattern_enforcement_seq_rows` in `rholang/tests/reified_pattern_enforcement.rs`, `example_space_qualifier_temp` in `rholang/tests/reified_examples_execute.rs`, `example_temp_space_ends_with_its_deploy` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#42-temp](specification.md#42-temp), [21-reified-spaces.md#temp](../../rholang/21-reified-spaces.md#temp) |
| Decisions | [I-4](decision-register.md#i-4), [decision 72](decision-register.md#decision-72) |
| Note | A temp space lives for one deploy, and the sweep at the end of the deploy removes its state (plan revision I-4). |

### RR-36

| Field | Value |
|---|---|
| Lines | 279–285 |
| Status | `implemented` |
| Clause | The seq qualifier: temp, and a seq channel is never sent, is not used in a concurrent process, and is used in at most one sequential process. |
| Code | `rholang/src/rust/interpreter/reified/seq.rs:107-126`, `rholang/src/rust/interpreter/spaces/ephemeral.rs:392-566` |
| Formal | `seq_name_iff` (`Namespace.v:1150`), the TLA+ invariant `SeqNotSent` of `SpaceCoordination.tla`, the TLA+ invariant `SeqCausalChain` of `SpaceCoordination.tla` |
| Tests | `prop_seq_scan_finds_every_seq_name` in `rholang/tests/reified_seq.rs`, `reducer_seq_payload` in `rholang/tests/reified_seq.rs`, `prop_seq_claim_causal_chain` in `rholang/tests/reified_seq.rs`, `reducer_seq_atomic_guard` in `rholang/tests/reified_seq.rs`, `pattern_enforcement_seq_rows` in `rholang/tests/reified_pattern_enforcement.rs`, `example_space_qualifier_seq` in `rholang/tests/reified_examples_execute.rs`, `example_space_qualifiers_seq_sent` in `rholang/tests/reified_examples_execute.rs`, `example_seq_rejects_concurrent_uses` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#43-seq](specification.md#43-seq), [21-reified-spaces.md#seq](../../rholang/21-reified-spaces.md#seq) |
| Decisions | [decision 73](decision-register.md#decision-73), [decision 83](decision-register.md#decision-83) |

### RR-37

| Field | Value |
|---|---|
| Lines | 287–289 |
| Status | `implemented` |
| Clause | The use block sets the default space of untagged channels in new blocks and of forged names. |
| Code | `rholang/src/rust/interpreter/reified/route.rs:175-227`, `rholang/src/rust/interpreter/reified/scope.rs:198-214` |
| Formal | `route_rules` (`SpaceRefs.v:864`), `space_name_route_ignores_ctx` (`SpaceRefs.v:892`), `ordinary_channels_route_default` (`SpaceRefs.v:1085`) |
| Tests | `prop_route_rules` in `rholang/tests/reified_p4_props.rs`, `reducer_space_routing_ordinary_unchanged` in `rholang/tests/reified_routing.rs`, `example_multi_space_isolation` in `rholang/tests/reified_examples_execute.rs`, `example_fips_example` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#14-routing](specification.md#14-routing), [21-reified-spaces.md#use-blocks](../../rholang/21-reified-spaces.md#use-blocks), [syntax-contract.md#use-blocks](syntax-contract.md#use-blocks) |
| Decisions | [decision 18](decision-register.md#decision-18), [decision 112](decision-register.md#decision-112) |

### RR-38

| Field | Value |
|---|---|
| Lines | 291–293 |
| Status | `informative` |
| Clause | The introduction of the example: one space on each of n channels, qualifiers that set persistence and sequentiality, and theories that a later design document specifies. |
| Reason | The paragraph introduces the example of rows RR-39 to RR-43. Its sentence on theories is row RR-07. |

### RR-39

| Field | Value |
|---|---|
| Lines | 295–302 |
| Status | `implemented` |
| Clause | The factories by URN, the FIPS names rho:space:HashMapBagSpace and rho:space:PathMapSpace included, and their calls with a qualifier and a theory. |
| Code | `rholang/src/rust/interpreter/reified/system_channels.rs:351-355`, `rholang/src/rust/interpreter/reified/system_channels.rs:359-363`, `models/src/rust/space_factory_urn.rs:311-433` |
| Formal | `aliases_bind_family_capability` (`SpaceFactory.v:1660`), `builtin_decidable_structural` (`TheoryAdmission.v:254`), `resolve_identity_deterministic` (`TheoryAdmission.v:587`), `replay_preserves_theory_admission` (`TheoryAdmission.v:851`), `theory_gate_validated` (`SpaceFactory.v:2727`) |
| Tests | `prop_aliases_bind_family_capability` in `rholang/tests/reified_families.rs`, `reified_getspaceagent_alias_equal` in `rholang/tests/reified_families.rs`, `prop_theory_builtin_decidable` in `rholang/tests/reified_theory.rs`, `theory_nat_accepts_bigint_nonneg` in `rholang/tests/reified_theory.rs`, `theory_nat_rejects_minus1_string_par` in `rholang/tests/reified_theory.rs`, `prop_resolve_identity_deterministic` in `rholang/tests/reified_theory.rs`, `theory_unavailable_fails_each_config_read` in `rholang/src/rust/interpreter/spaces/theory/tests.rs`, `reified_factory_theory_argument` in `rholang/tests/reified_rehydrate.rs`, `prop_theory_gate` in `rholang/tests/reified_theory.rs`, `theory_unknown_name_suggestion` in `rholang/tests/reified_theory.rs`, `theory_builtin_args_rejected` in `rholang/tests/reified_theory.rs`, `reified_factory_theory_gate` in `rholang/src/rust/interpreter/spaces/factory/constructor.rs`, `example_fips_example` in `rholang/tests/reified_examples_execute.rs`, `example_theory_validated_space` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#15-families](specification.md#15-families), [21-reified-spaces.md#the-fips-example](../../rholang/21-reified-spaces.md#the-fips-example) |
| Decisions | [decision 9](decision-register.md#decision-9) |

### RR-40

| Field | Value |
|---|---|
| Lines | 303–308 |
| Status | `implemented` |
| Clause | Inside a use block, new x : s creates a name in the space s, and a forged name is created in the space of the use block. |
| Code | `rholang/src/rust/interpreter/reified/route.rs:175-227` |
| Formal | `name_id_routes_to_sid` (`Namespace.v:1003`), `route_rules` (`SpaceRefs.v:864`) |
| Tests | `prop_name_id_routes_to_sid` in `rholang/tests/reified_p4_props.rs`, `space_handle_methods_gensym` in `rholang/tests/reified_handle.rs`, `prop_route_rules` in `rholang/tests/reified_p4_props.rs`, `example_space_annotations` in `rholang/tests/reified_examples_execute.rs`, `example_multi_space_isolation` in `rholang/tests/reified_examples_execute.rs`, `example_fips_example` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [21-reified-spaces.md#space-annotations](../../rholang/21-reified-spaces.md#space-annotations), [21-reified-spaces.md#use-blocks](../../rholang/21-reified-spaces.md#use-blocks), [syntax-contract.md#space-annotations-on-new](syntax-contract.md#space-annotations-on-new) |
| Decisions | [decision 18](decision-register.md#decision-18), [decision 21](decision-register.md#decision-21), [decision 112](decision-register.md#decision-112) |

### RR-41

| Field | Value |
|---|---|
| Lines | 309–317 |
| Status | `implemented` |
| Clause | Joins are permitted in one space and prohibited across spaces. |
| Code | `rholang/src/rust/interpreter/spaces/join.rs:371-393`, `rholang/src/rust/interpreter/spaces/join.rs:597-609` |
| Formal | `plan_then_lock_stable` (`JoinCascade.v:5917`), `join_repeated_channel_distinct_records` (`JoinCascade.v:1438`), `bind_permutation_restores_order` (`JoinCascade.v:2342`), `continuation_v2_roundtrip` (`RecordEnvelope.v:1734`), the TLA+ invariant `NonPersistentBoundOnce` of `JoinCascade.tla`, `cascade_bumps_touched_counters` (`JoinCascade.v:4250`), `join_reissue_keeps_bind_ords` (`JoinCascade.v:4324`), the TLA+ invariant `TouchedCountersBumped` of `JoinCascade.tla`, `join_fires_heads_only` (`JoinCascade.v:3404`), the TLA+ invariant `JoinFiresHeadsOnly` of `JoinCascade.tla`, `join_search_is_head_greedy` (`JoinCascade.v:1662`), `greedy_join_can_miss_assignment` (`JoinCascade.v:1539`), `join_produce_search_agrees_with_head` (`JoinCascade.v:2065`), `join_incoming_binds_once` (`JoinCascade.v:1340`), the TLA+ invariant `IncomingHeld` of `JoinCascade.tla`, `cascade_terminates` (`JoinCascade.v:4691`), `cascade_order_independent` (`JoinCascade.v:3338`), `lift_reoffer_equals_direct_comm` (`JoinCascade.v:3770`), the TLA+ invariant `CascadeMeasureDecreases` of `JoinCascade.tla`, `join_set_bind_retires_class` (`JoinCascade.v:3167`), `peek_join_restores_stored_candidates` (`JoinCascade.v:5350`), the TLA+ invariant `NoRecordLost` of `JoinCascade.tla`, `join_txn_fires_bounded` (`JoinCascade.v:4836`), the TLA+ invariant `TxnFiresBounded` of `JoinCascade.tla`, `join_ops_race_on_every_counter` (`CounterConflict.v:1947`) |
| Tests | `space_joins_cross_space_join_aborts` in `rholang/tests/reified_joins.rs`, `space_joins_default_and_space_join_aborts` in `rholang/tests/reified_joins.rs`, `reducer_space_routing_agent_gate` in `rholang/tests/reified_routing.rs`, `prop_plan_then_lock_covers_every_staged_channel` in `rholang/tests/reified_join_props.rs`, `space_joins_phase_b_locks_the_closure_once` in `rholang/tests/reified_joins.rs`, `space_joins_classification_fires_as_check_joins` in `rholang/tests/reified_joins.rs`, `prop_join_nonpersistent_record_binds_once` in `rholang/tests/reified_join_props.rs`, `prop_bind_permutation_restores_order` in `rholang/tests/reified_join_props.rs`, `prop_continuation_v2_roundtrip` in `rholang/tests/reified_join_props.rs`, `space_joins_persistent_record_binds_two_positions` in `rholang/tests/reified_joins.rs`, `space_joins_bind_order_restored_for_body` in `rholang/tests/reified_joins.rs`, `space_joins_multi_ord_envelope_roundtrip` in `rholang/tests/reified_joins.rs`, `space_joins_peek_join_restores_stored_candidates` in `rholang/tests/reified_joins.rs`, `space_joins_reissued_join_keeps_bind_ords` in `rholang/tests/reified_joins.rs`, `prop_cascade_bumps_touched_counters` in `rholang/tests/reified_join_props.rs`, `prop_join_reissue_keeps_bind_ords` in `rholang/tests/reified_join_props.rs`, `prop_join_fires_heads_only` in `rholang/tests/reified_join_props.rs`, `space_joins_queue_heads_only` in `rholang/tests/reified_joins.rs`, `space_joins_stack_heads_only` in `rholang/tests/reified_joins.rs`, `space_joins_pq_heads_only` in `rholang/tests/reified_joins.rs`, `space_joins_cell_join` in `rholang/tests/reified_joins.rs`, `prop_join_search_is_head_greedy` in `rholang/tests/reified_join_props.rs`, `prop_greedy_join_limit` in `rholang/tests/reified_join_props.rs`, `space_joins_bag_greedy` in `rholang/tests/reified_joins.rs`, `space_joins_set_classes` in `rholang/tests/reified_joins.rs`, `space_joins_guard_checked_once_after_all_binds` in `rholang/tests/reified_joins.rs`, `prop_join_produce_search_agrees_with_head` in `rholang/tests/reified_join_props.rs`, `prop_join_incoming_held` in `rholang/tests/reified_join_props.rs`, `space_joins_incoming_binds_once` in `rholang/tests/reified_joins.rs`, `prop_cascade_terminates_any_schedule` in `rholang/tests/reified_join_props.rs`, `prop_cascade_order_independent` in `rholang/tests/reified_join_props.rs`, `prop_lift_reoffer_equals_direct_comm` in `rholang/tests/reified_join_props.rs`, `prop_cascade_measure_decreases` in `rholang/tests/reified_join_props.rs`, `space_joins_cascade_after_produce` in `rholang/tests/reified_joins.rs`, `space_joins_cascade_after_consume` in `rholang/tests/reified_joins.rs`, `space_joins_cascade_order_independent` in `rholang/tests/reified_joins.rs`, `space_joins_set_join_cascade_fires_exposed_waiter` in `rholang/tests/reified_joins.rs`, `prop_join_set_bind_retires_class` in `rholang/tests/reified_join_props.rs`, `space_joins_set_bind_retires_class` in `rholang/tests/reified_joins.rs`, `prop_peek_join_restores_stored_candidates` in `rholang/tests/reified_join_props.rs`, `prop_no_record_lost_any_schedule` in `rholang/tests/reified_join_props.rs`, `space_joins_produce_side_peek_reproduces_in_receipt_order` in `rholang/tests/reified_joins.rs`, `prop_join_txn_fires_bounded_at_the_cap` in `rholang/tests/reified_join_props.rs`, `space_joins_txn_fires_bounded_at_the_cap` in `rholang/tests/reified_joins.rs`, `space_joins_cascade_round_cap_reissues_settle` in `rholang/tests/reified_joins.rs`, `space_joins_cap_carries_pending_second_sends` in `rholang/tests/reified_joins.rs`, `space_joins_settle_before_cap_resumes_the_operation` in `rholang/tests/reified_joins.rs`, `prop_join_ops_race_on_every_counter` in `rholang/tests/reified_join_props.rs`, `space_collection_conflicts_join_ops_race_all_counters` in `rholang/tests/reified_merge_p5.rs`, `space_collection_conflicts_unrelated_collections_merge` in `rholang/tests/reified_merge_p5.rs`, `space_collection_conflicts_bag_join_potential_comm` in `rholang/tests/reified_merge_p5.rs`, `space_joins_persisted_waiter_check3_conflict` in `rholang/tests/reified_joins.rs`, `reified_p5c_only_user_joins_persist` in `rholang/tests/reified_merge_p5.rs`, `example_cross_space_join` in `rholang/tests/reified_examples_execute.rs`, `example_fips_example` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [specification.md#38-joins](specification.md#38-joins), [21-reified-spaces.md#joins](../../rholang/21-reified-spaces.md#joins) |
| Decisions | [decision 29](decision-register.md#decision-29), [decision 50](decision-register.md#decision-50) |
| Note | `join_produce_search_agrees_with_head` is a theorem of the search before decision 118. The GREEDY part of `prop_join_produce_search_agrees_with_head` runs for a non-persistent x only, and the allowed set checks a persistent x. |

### RR-42

| Field | Value |
|---|---|
| Lines | 318–320, 323–330 |
| Status | `implemented` |
| Clause | Inside a new block that creates names in the spaces, the names of the spaces refer to the modified spaces, and outside it they refer to the unmodified spaces. |
| Code | `rholang/src/rust/interpreter/reified/scope.rs:198-214`, `rholang/src/rust/interpreter/reified/scope.rs:148-171` |
| No model | The clause is the scoping of a typing context. A name from new is unforgeable and lexically scoped in the baseline, so no new model is needed (decision 77). ([reason](decision-register.md#decision-77)) |
| Tests | `prop_space_ref_unwraps_one_bundle` in `rholang/tests/reified_p8_props.rs`, `example_fips_example` in `rholang/tests/reified_examples_execute.rs`, `example_space_annotations` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [21-reified-spaces.md#the-fips-example](../../rholang/21-reified-spaces.md#the-fips-example), [agent-api.md#41-the-handle-capability](agent-api.md#41-the-handle-capability) |
| Decisions | [decision 77](decision-register.md#decision-77) |
| Note | A space keeps its theory. The new block adds its names to the space, and the names reach only the scope of the block. |

### RR-43

| Field | Value |
|---|---|
| Lines | 321–322 |
| Status | `implemented` |
| Clause | getSpaceAgent returns the agent of a space, and assertEqual compares it with the factory. |
| Code | `rholang/src/rust/interpreter/reified/system_processes.rs:330-373`, `rholang/src/rust/interpreter/reified/system_processes.rs:431-445` |
| Formal | `aliases_map_to_family` (`SpaceFactory.v:347`) |
| Tests | `space_system_ops_get_space_agent_capability` in `rholang/tests/reified_factory.rs`, `space_system_ops_assert_equal` in `rholang/tests/reified_factory.rs`, `example_get_space_agent` in `rholang/tests/reified_examples_execute.rs`, `example_fips_example` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [agent-api.md#8-getspaceagent](agent-api.md#8-getspaceagent), [agent-api.md#9-assertequal](agent-api.md#9-assertequal), [21-reified-spaces.md#getspaceagent-and-assertequal](../../rholang/21-reified-spaces.md#getspaceagent-and-assertequal) |
| Decisions | [decision 9](decision-register.md#decision-9), [decision 33](decision-register.md#decision-33) |
| Note | getSpaceAgent replies with the factory capability of the family of the space (decision 9). assertEqual takes a reply name, so its arity is 3 (decision 33). |

### RR-44

| Field | Value |
|---|---|
| Lines | 332–339 |
| Status | `informative` |
| Clause | The issues of the current architecture: ISpace couples the outer structure, the inner structure and the operations. |
| Reason | The section motivates the proposal of the rows RR-45 to RR-50. |

### RR-45

| Field | Value |
|---|---|
| Lines | 341–442 |
| Status | `deviation` |
| Clause | Layers 1 and 2 of the proposal: the traits DataCollection, ContinuationCollection and ChannelStore. |
| Code | `rholang/src/rust/interpreter/spaces/generic.rs:721-864`, `rholang/src/rust/interpreter/spaces/generic.rs:965-1286` |
| Documents | [design.md#n4-genericspace-and-the-queue-discipline](design.md#n4-genericspace-and-the-queue-discipline), [design.md#n1211-the-seam-of-a-store](design.md#n1211-the-seam-of-a-store) |
| Decisions | [decision 19](decision-register.md#decision-19), [I-1](decision-register.md#i-1) |
| Note | The traits Discipline and OuterStore separate the inner collection from the outer store. They plan whole operations on the staged view of one space transaction, not on mutable collection objects. |

### RR-46

| Field | Value |
|---|---|
| Lines | 444–561 |
| Status | `deviation` |
| Clause | Layers 3 and 4 of the proposal: the qualifier enum, the trait SpaceAgent, and the extension traits for checkpoints and replay. |
| Code | `rholang/src/rust/interpreter/spaces/agent.rs:547-600`, `rholang/src/rust/interpreter/spaces/types/qualifier.rs:23-31` |
| Documents | [design.md#substrate](design.md#substrate), [design.md#s1-purpose-and-the-rule-of-no-new-ispace-surface](design.md#s1-purpose-and-the-rule-of-no-new-ispace-surface) |
| Decisions | [I-1](decision-register.md#i-1), [decision 19](decision-register.md#decision-19) |
| Note | Spaces are not ISpace implementors. Checkpoints, replay and reset stay with the one RSpace of the node, which holds the state of every space. |

### RR-47

| Field | Value |
|---|---|
| Lines | 563–716 |
| Status | `implemented` |
| Clause | Layer 5 of the proposal: the bag, queue, set and cell collections, the HashMap and PathMap stores, and a generic RSpace over a store and a matcher. |
| Code | `rholang/src/rust/interpreter/spaces/disciplines/set.rs:204-257`, `rholang/src/rust/interpreter/spaces/generic.rs:2424-2439`, `rholang/src/rust/interpreter/spaces/pathmap/path_codec.rs:283-288`, `rholang/src/rust/interpreter/spaces/generic.rs:1301-1342` |
| Formal | `bag_visit_order_canonical` (`DataCollection.v:251`), `waiter_visit_order_canonical` (`ContinuationCollection.v:338`), `queue_head_is_min_ord` (`HeadOnlyDisciplines.v:530`), `set_key_injective` (`SetDiscipline.v:303`), `cell_capacity_one` (`CellDiscipline.v:672`), the TLA+ invariant `CellCapacity` of `GenericRSpace.tla`, `canon_respects_par_eq` (`Namespace.v:1043`), `prefix_key_iff_prefix` (`PathMapPrefix.v:586`), `candidate_order_total` (`PathMapPrefix.v:1016`) |
| Tests | `prop_bag_visit_order_canonical` in `rholang/tests/reified_bag.rs`, `prop_bag_waiter_visit_order_canonical` in `rholang/tests/reified_bag.rs`, `prop_queue_candidates_head_only` in `rholang/tests/reified_p4_props.rs`, `prop_set_key_injective` in `rholang/tests/reified_set.rs`, `space_set_discipline_key_length_prefix_no_collision` in `rholang/tests/reified_set.rs`, `prop_cell_capacity_one` in `rholang/tests/reified_cell.rs`, `space_disciplines_cell_second_produce_occupied` in `rholang/tests/reified_cell.rs`, `prop_canonical_inner_bytes_respects_par_eq` in `rholang/tests/reified_p4_props.rs`, `prop_prefix_key_iff_prefix` in `rholang/src/rust/interpreter/spaces/pathmap/path_codec.rs`, `prop_candidate_order_total` in `rholang/src/rust/interpreter/spaces/pathmap/path_codec.rs`, `example_bag_basics` in `rholang/tests/reified_examples_execute.rs`, `example_queue_fifo` in `rholang/tests/reified_examples_execute.rs`, `example_set_idempotent` in `rholang/tests/reified_examples_execute.rs`, `example_cell_exactly_once` in `rholang/tests/reified_examples_execute.rs`, `example_pathmap_prefix_aggregation` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [design.md#n4-genericspace-and-the-queue-discipline](design.md#n4-genericspace-and-the-queue-discipline), [design.md#n111-family-dispatch-and-registration](design.md#n111-family-dispatch-and-registration) |
| Decisions | [decision 19](decision-register.md#decision-19), [M-1](decision-register.md#m-1) |
| Note | GenericSpace takes the injected matcher of the runtime. A space keeps its counters in records and its state in the one history of the node, not in a history repository of its own. |

### RR-48

| Field | Value |
|---|---|
| Lines | 718–770 |
| Status | `implemented` |
| Clause | Layer 6 of the proposal: a factory for each kind of space that creates a space from a qualifier, a theory and more arguments. |
| Code | `rholang/src/rust/interpreter/spaces/factory/urn.rs:207`, `rholang/src/rust/interpreter/spaces/factory/urn.rs:246-253`, `rholang/src/rust/interpreter/spaces/families.rs:998-1007`, `rholang/src/rust/interpreter/spaces/families.rs:486-519`, `rholang/src/rust/interpreter/spaces/factory/defaults.rs:133-189`, `rholang/src/rust/interpreter/spaces/factory/constructor.rs:115-213`, `rholang/src/rust/interpreter/spaces/factory/constructor.rs:305-335`, `rholang/src/rust/interpreter/reified/factory.rs:58-130` |
| Formal | `registered_families_complete` (`SpaceFactory.v:1571`), `space_match_is_injected_match` (`Match.v:644`), `constructor_params_validated` (`SpaceFactory.v:2065`), `array_size_validated` (`SpaceFactory.v:2315`) |
| Tests | `prop_registered_families_complete` in `rholang/tests/reified_families.rs`, `space_factory_families_registered` in `rholang/tests/reified_families.rs`, `space_families_dispatch_by_config` in `rholang/src/rust/interpreter/spaces/families.rs`, `space_family_dispatch_by_config` in `rholang/tests/reified_families.rs`, `prop_constructor_params_validated` in `rholang/tests/reified_families.rs`, `space_factory_kwargs_unknown_param_with_suggestion` in `rholang/tests/reified_families.rs`, `space_factory_kwargs_are_stored_canonical` in `rholang/tests/reified_families.rs`, `example_queue_fifo` in `rholang/tests/reified_examples_execute.rs`, `example_factory_rejects_an_unknown_param` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [agent-api.md#3-factories](agent-api.md#3-factories), [design.md#n7-factories-and-handles](design.md#n7-factories-and-handles) |

### RR-49

| Field | Value |
|---|---|
| Lines | 771–815 |
| Status | `future` |
| Clause | Layer 6 of the proposal: a registry that registers space factories at run time and creates spaces by name. |
| Decisions | [decision 17](decision-register.md#decision-17) |
| Note | The fixed URN table registers the builtin families (rule FA-1). Decision 17 postpones the registration of custom spaces at run time to the follow-up rrs-fu-runtime-space-registration. |

### RR-50

| Field | Value |
|---|---|
| Lines | 817–824 |
| Status | `deviation` |
| Clause | The migration path of the proposal in six phases. |
| Documents | [README.md#campaign-phases](README.md#campaign-phases) |
| Decisions | [M-2](decision-register.md#m-2), [I-1](decision-register.md#i-1) |
| Note | The campaign runs its phases P0 to P12. It keeps HotStore, the host structs and ISpace unchanged, so the phases 1 to 3 of the proposal do not apply. |

## The rows of Agents

*Table 5. The clauses of Agents and their status.*

| Row | Lines | Clause | Status |
|---|---|---|---|
| [AG-01](#ag-01) | 8 | Agents emulate objects: a constructor sets up the state of a new instance and returns a reference to it. | `implemented` |
| [AG-02](#ag-02) | 10–35 | The agent block: a replicated receive on the constructor creates this and private, dispatches each message [return, method, args] to its method or to default, and replies bundle+{*this}. | `implemented` |
| [AG-03](#ag-03) | 37–40 | The method-call sugar: x!y(args) means x!?("y", args) as a receive source, before a continuation and as a final send. | `implemented` |
| [AG-04](#ag-04) | 42 | The constructor channel r is fresh, this and return are bound in the method bodies, default sees only this, and a return channel is the first element of args. | `implemented` |
| [AG-05](#ag-05) | 44 | Every agent has a default clause, because messages of an unexpected shape can arrive on this. | `implemented` |
| [AG-06](#ag-06) | 46 | The desugaring binds a private channel for the state of an instance, and the Private Methods FIP builds on it. | `out-of-scope` |
| [AG-07](#ag-07) | 48–57 | The constructor composes with the send-return syntax: for (b <- ctor!?(args)) calls methods of b. | `implemented` |

### AG-01

| Field | Value |
|---|---|
| Lines | 8 |
| Status | `implemented` |
| Clause | Agents emulate objects: a constructor sets up the state of a new instance and returns a reference to it. |
| Code | `rholang/src/rust/interpreter/spaces/families.rs:998-1007`, `rholang/src/rust/interpreter/spaces/families.rs:486-519`, `rholang/src/rust/interpreter/reified/factory.rs:172-178` |
| Formal | `sid_derivation_deterministic` (`SpaceFactory.v:716`), `space_already_exists_is_deterministic_abort` (`SpaceFactory.v:938`), `counter_and_config_roundtrip` (`RecordEnvelope.v:1075`), the TLA+ invariant `HandleUnique` of `NativeHandles.tla`, the TLA+ invariant `DanglingIsDeterministicError` of `NativeHandles.tla`, `space_match_is_injected_match` (`Match.v:644`) |
| Tests | `prop_sid_derivation_deterministic` in `rholang/tests/reified_p4_props.rs`, `space_already_exists_aborts` in `rholang/src/rust/interpreter/reified/factory.rs`, `prop_counter_and_config_records_roundtrip` in `rholang/tests/reified_p4_props.rs`, `space_meta_channels_config_record` in `rholang/tests/reified_queue.rs`, `reified_handle_dangling_aborts` in `rholang/tests/reified_handle.rs`, `space_queue_dangling_name_aborts` in `rholang/tests/reified_queue.rs`, `reducer_dangling_handle` in `rholang/tests/reified_rehydrate.rs`, `reducer_attach_costs` in `rholang/tests/reified_rehydrate.rs`, `space_families_dispatch_by_config` in `rholang/src/rust/interpreter/spaces/families.rs`, `space_family_dispatch_by_config` in `rholang/tests/reified_families.rs`, `example_handle_methods` in `rholang/tests/reified_examples_execute.rs`, `example_get_space_agent` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [agent-api.md#33-the-creation-and-the-reply](agent-api.md#33-the-creation-and-the-reply), [agent-api.md#41-the-handle-capability](agent-api.md#41-the-handle-capability) |
| Decisions | [decision 8](decision-register.md#decision-8) |

### AG-02

| Field | Value |
|---|---|
| Lines | 10–35 |
| Status | `implemented` |
| Clause | The agent block: a replicated receive on the constructor creates this and private, dispatches each message [return, method, args] to its method or to default, and replies bundle+{*this}. |
| Code | `rholang/src/rust/interpreter/reified/handle.rs:232-411`, `rholang/src/rust/interpreter/reified/handle.rs:156-188`, `rholang/src/rust/interpreter/reified/factory.rs:144-169` |
| Formal | the TLA+ invariant `HandleUnique` of `NativeHandles.tla`, the TLA+ invariant `HandleCallsMergeLikeContracts` of `NativeHandles.tla` |
| Tests | `reified_factory_handle_continuation_forwards_to_handle_op` in `rholang/src/rust/interpreter/reified/factory.rs`, `reified_unknown_method_typed_reply` in `rholang/tests/reified_handle.rs`, `example_handle_methods` in `rholang/tests/reified_examples_execute.rs`, `example_agent_hashmap_bag` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [agent-api.md#4-handles](agent-api.md#4-handles), [agent-api.md#43-the-steps-of-a-call](agent-api.md#43-the-steps-of-a-call), [design.md#n7-factories-and-handles](design.md#n7-factories-and-handles) |
| Decisions | [decision 8](decision-register.md#decision-8) |
| Note | The parser desugars the agent block (decision 8), and the reference agent uses it. A native space follows the same protocol: its factory replies bundle+{*this}, and HANDLE_OP dispatches each message. |

### AG-03

| Field | Value |
|---|---|
| Lines | 37–40 |
| Status | `implemented` |
| Clause | The method-call sugar: x!y(args) means x!?("y", args) as a receive source, before a continuation and as a final send. |
| Code | `rholang/src/rust/interpreter/reified/handle.rs:103-124` |
| No model | The parser desugars the sugar (decision 8), and the handle receives the desugared call, so the handle protocol models of row AG-02 apply. ([reason](agent-api.md#5-handle-methods)) |
| Tests | `space_handle_methods_gensym` in `rholang/tests/reified_handle.rs`, `example_handle_methods` in `rholang/tests/reified_examples_execute.rs`, `example_agent_wrapper_forwards_to_native` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [agent-api.md#5-handle-methods](agent-api.md#5-handle-methods), [21-reified-spaces.md#the-handle-and-its-methods](../../rholang/21-reified-spaces.md#the-handle-and-its-methods) |
| Decisions | [decision 8](decision-register.md#decision-8) |

### AG-04

| Field | Value |
|---|---|
| Lines | 42 |
| Status | `implemented` |
| Clause | The constructor channel r is fresh, this and return are bound in the method bodies, default sees only this, and a return channel is the first element of args. |
| Code | `rholang/src/rust/interpreter/reified/handle.rs:201-210` |
| No model | The name rule is a check of the first element of a message, which the tests of the drop rule cover. ([reason](agent-api.md#44-the-drop-rule-and-the-name-rule)) |
| Tests | `reified_malformed_message_dropped` in `rholang/tests/reified_handle.rs`, `example_handle_methods` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [agent-api.md#44-the-drop-rule-and-the-name-rule](agent-api.md#44-the-drop-rule-and-the-name-rule), [agent-api.md#6-replies](agent-api.md#6-replies) |
| Decisions | [decision 8](decision-register.md#decision-8) |

### AG-05

| Field | Value |
|---|---|
| Lines | 44 |
| Status | `implemented` |
| Clause | Every agent has a default clause, because messages of an unexpected shape can arrive on this. |
| Code | `rholang/src/rust/interpreter/reified/handle.rs:213-223`, `rholang/src/rust/interpreter/reified/handle.rs:804-809` |
| No model | The default behavior is one typed reply or one drop for each message, which the tests check. ([reason](agent-api.md#7-typed-replies)) |
| Tests | `reified_unknown_method_typed_reply` in `rholang/tests/reified_handle.rs`, `reified_malformed_message_dropped` in `rholang/tests/reified_handle.rs`, `example_agent_hashmap_bag` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [agent-api.md#7-typed-replies](agent-api.md#7-typed-replies), [agent-api.md#44-the-drop-rule-and-the-name-rule](agent-api.md#44-the-drop-rule-and-the-name-rule) |
| Decisions | [decision 8](decision-register.md#decision-8) |
| Note | A native handle answers an unknown method with a typed reply and drops a malformed message, as a default clause does. The reference agent has a default clause. |

### AG-06

| Field | Value |
|---|---|
| Lines | 46 |
| Status | `out-of-scope` |
| Clause | The desugaring binds a private channel for the state of an instance, and the Private Methods FIP builds on it. |
| Decisions | [decision 8](decision-register.md#decision-8) |
| Note | The private channel belongs to the agent sugar of the parser. A native handle keeps its state in the records of its space, and a user agent may use private. |

### AG-07

| Field | Value |
|---|---|
| Lines | 48–57 |
| Status | `implemented` |
| Clause | The constructor composes with the send-return syntax: for (b <- ctor!?(args)) calls methods of b. |
| Code | `rholang/src/rust/interpreter/reified/factory.rs:58-130` |
| Formal | the TLA+ invariant `HandleUnique` of `NativeHandles.tla` |
| Tests | `reified_factory_creates_config_counter_handle` in `rholang/tests/reified_factory.rs`, `example_handle_methods` in `rholang/tests/reified_examples_execute.rs`, `example_queue_fifo` in `rholang/tests/reified_examples_execute.rs`, `example_fips_example` in `rholang/tests/reified_examples_execute.rs` |
| Documents | [agent-api.md#31-the-call](agent-api.md#31-the-call), [21-reified-spaces.md#a-first-space](../../rholang/21-reified-spaces.md#a-first-space) |
| Decisions | [decision 8](decision-register.md#decision-8) |

## Spec holes

The design ledger names four holes of the FIPS that the legacy branches filled ad hoc.
Table 6 gives the resolution of each hole and the rows that carry it.

*Table 6. The spec holes and their resolutions.*

| Hole | FIPS lines | Resolution | Rows | Decisions |
|---|---|---|---|---|
| SH-1: The surface syntax of similarity queries and of params | 249, 257 | The syntax contract gives send params and receive queries as the protobuf fields `Send.params` and `ReceiveBind.query_spec`. The VectorDB family takes the query keys of rule VD-3. | [RR-26](#rr-26), [RR-28](#rr-28) | [decision 112](decision-register.md#decision-112), [decision 21](decision-register.md#decision-21), [decision 56](decision-register.md#decision-56) |
| SH-2: The theory documents, which a later design document was to specify | 117–118, 293 | The theory seam admits the builtin structural theories now. The MeTTaIL theories come through the seam in phase P12. | [RR-06](#rr-06), [RR-07](#rr-07) | [decision 9](decision-register.md#decision-9), [decision 74](decision-register.md#decision-74) |
| SH-3: The unification question of the quantale section | 187–193 | The passage is informative, and follow-up rrs-fu-quantale-unification routes the question to the FIPS authors. | [RR-15](#rr-15), [RR-33](#rr-33) | [decision 87](decision-register.md#decision-87) |
| SH-4: Tensor logic and nonlinear vector operations, which the FIPS marks as topics to look into | 259–261 | The design study of decision 52 answered the question. Tensor logic and the nonlinear operations come in phase P12. | [RR-29](#rr-29), [RR-30](#rr-30) | [decision 52](decision-register.md#decision-52), [decision 62](decision-register.md#decision-62), [decision 65](decision-register.md#decision-65), [decision 88](decision-register.md#decision-88) |
