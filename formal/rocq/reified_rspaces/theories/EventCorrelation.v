(* ═══════════════════════════════════════════════════════════════════════════
   EventCorrelation.v — the SpaceTxn substrate against the host RSpace

   This module models P1 of the Reified RSpaces campaign: the storage
   substrate SpaceTxnHost, SpaceStoreHandle and SpaceTxn in rspace++. A space
   agent stages whole operations with an explicit outcome. The commit runs
   the host call sequence of each operation. The module proves the
   headlines H1-H12 of the P1 design report (§5(a)) and the headlines H17,
   H19, H20 and H21 of the P1 fix rules. H4 and H9 have the statements of
   the fix rules (FR-3 and FR-1). The round-2 rules change two things here.
   FR-1' makes a group canonical when it is non-decreasing, so a group can
   repeat a channel (H17). FR-5' resets the parent tracker at each root
   evaluation scope, on a store swap and on a session change (H20). The
   round-3 rule FR-10 changes peek fires. A produce-side fire never stores
   its incoming datum. The agent re-offers that datum as the next operation
   of the transaction, with the same produce identity (H11).

   Decision 45 (Rule E, with the scope "Apply everywhere") removes the
   retention of FR-10. The substrate keeps no record for any peek. Each fire
   removes every stored non-persistent candidate, as the host does, also
   when its peeks cover every bind. The receipt keeps the persist flags
   that the host gives the results. The agent then stores each record that
   a consume-side peek removed again, in the same transaction, with the
   same bytes and so the same produce identity. This re-store is the
   produce that the host reducer makes in produce_peeks. So H2 holds with
   no premise about peeks, and H11 states that a peek fire followed by its
   re-stores makes the trace, the observer calls and the store of the host
   peek COMM followed by produce_peeks.

   Decision 36 (it amends decision 19) changes replay pairing. A replay fire
   with no free recorded occurrence stages unpaired: its view effects are
   the effects of play, and it adds no pairing. Its commit makes the calls
   of play. When a call rejects, the commit returns that rejection, which
   is the outcome of play. When every call accepts, the commit fails with
   ReplayDivergence (StDiverged) and keeps the operations before it (D6).
   So replay reproduces the run of play when the log is the commit log of
   play (H23), also for a deploy that runs out of phlo inside a transaction
   (docs/rspace/reified-rspaces/findings/space-replay-out-of-phlo.md).

   Decision 47 runs spaces in native execution. Native play records one
   journal row for each staged operation: its locked footprint and its
   outcome. The native replay backend of spaces (NativeBackend::apply_op in
   replay_rspace/native_session/space.rs) replays each staged operation
   against its row. It begins the row, authenticates the footprint, checks
   that the outcome fits the staged kind, and observes the introduction. A
   fire then checks its candidate and observes its COMM. Last, the backend
   applies the effects of play. H24 to H26 model this protocol over
   abstract operations, and H27 models the charge of a view load (P5 of
   design v2).

   Code base: the campaign tree after the merge of feature/cost-accounted-rho
   at 4d67b5ccc. Every host file that this header cites with a line range
   equals that file at 4d67b5ccc, except the Reified hooks that footprint
   policy p4 approves. Before the merge, the code base was HEAD 6f7a80da3
   (e748426 for the cited host files).
   The P1 files space_store_handle.rs, rspace/space_play.rs and
   replay_rspace/space_replay.rs are new. This header cites their items by
   name, because their line numbers change often. Decision 36 changes
   TxnCore::verify, StagedConsumeFire and the replay commit of a fire
   (ReplayBackend::produce_fire and consume_observed). The rows of decision
   45 also cite lines. These lines are those of the tree of decision 45:
   HEAD bc7783417 with the Rule E change of the substrate and the agent.
   Design: p1-design-report §1.3-§1.4 (staging rules), §2.1-§2.4 (host call
   order and replay pairing), §5(a). Plan decision 19 (D1-D7). Fix rules
   FR-1 to FR-9 (prompts/p1-fix-rules.md), which close the defects D1-D8 of
   the independent P1 review, the round-2 rules FR-1', FR-5', FR-7' and
   FR-9', and the round-3 rule FR-10 of the same file (plan decision 30).
   FR-10 supersedes the storing of the incoming datum of P1 §6 (decision
   19). The head-only plan of a queue is p4-design-report B.4 (decision 11).
   Decision 45 (Rule E) supersedes the retention part of FR-10 and the
   deviation of decision 19. Its design report gives the retention sites
   (§B), the identity of a re-store (§C.3), play and replay (§C.5), FR-10
   after Rule E (§C.10) and this formal plan (§E.5).

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   (rspace++/src/rspace/ is abbreviated as rs/, and
   rs/space_store_handle.rs as ssh)
   ─────────────────────────────────────────────────────────────────────────
   produce_id, consume_id    rs/trace/event.rs:238-249, 322-337 (create)
   pid_leb, comm_sort        rs/trace/event.rs:57-65 (COMM produce sort)
   mk_comm                   rs/trace/event.rs:37-78 (COMM::new)
   counters_of               rs/rspace/trace_log.rs:69-86 (produce_counters),
                             rs/replay_rspace.rs:808-824
   play_counters_closure     rs/rspace/ops_produce.rs:185-191
   incr, bump                rs/rspace/trace_log.rs:158-172 (log_produce
                             counter), rs/replay_rspace.rs:1579-1585
   rollback, unbump          rs/replay_rspace.rs:1587-1600, 1254-1256
   Store                     rs/hot_store.rs:202-349 (HotStore view with
                             history read-through)
   put_datum                 rs/hot_store.rs:879-897
   host_remove_datum         rs/hot_store.rs:912-945
   put_continuation,         rs/hot_store.rs:720-783, 1315-1317
     wk_ident
   host_remove_continuation  rs/hot_store.rs:818-861
   put_join, remove_join     rs/hot_store.rs:1017-1040, 1069-1113
   store_waiting_continuation rs/rspace/ops_consume.rs:179-190,
                             rs/replay_rspace.rs:1652-1668
   host_rm_loop              rs/rspace/ops_produce.rs:228-248,
                             rs/replay_rspace.rs:1798-1818
   host_spd_loop             rs/rspace/ops_consume.rs:192-202,
                             rs/replay_rspace.rs:1686-1696
   hkey, sort_desc           rs/rspace/ops_produce.rs:233-234,
                             rs/rspace/ops_consume.rs:193-194,
                             rs/replay_rspace.rs:1687-1688, 1803-1804
                             (stable sort_by_key(Reverse(datum_index)))
   host_prelude              rs/rspace/accounting.rs:20-87,
                             rs/rspace/ops_produce.rs:32-78,
                             rs/rspace/ops_consume.rs:31-81,
                             rs/replay_rspace.rs:836-905, 1133-1192,
                             rholang/src/rust/interpreter/reduce.rs:539-543,
                             651-655 (register_introduction_authority)
   host_play_produce         rs/rspace/ops_produce.rs:32-113, 163-210
   host_play_consume         rs/rspace/ops_consume.rs:31-154
   host_replay_produce       rs/replay_rspace.rs:1133-1267, 1363-1425
   host_replay_consume       rs/replay_rspace.rs:836-1067
   Ev, Obs                   rs/trace/event.rs:16-26 (Event, IOEvent),
                             rs/rspace_interface.rs:42-122 (observer)
   IOKey, Bindings, unbind   rs/replay_rspace.rs:71, 1498-1526
                             (remove_bindings_for), rs/internal.rs:116-164
   bound                     rs/replay_rspace.rs:1026-1033, 1396-1401 (assert)
   rig                       rs/replay_rspace.rs:480-540 (the binding loop
                             at 398-446)
   View, vdata .. vcount     design §1.3 (TxnView overlay, κ_v = κ_host + δ),
                             ssh TxnView
   stage, pfire_ok, ...      design §1.4 rules 1-4, ssh TxnCore::stage_produce
                             and TxnCore::stage_consume
   verify, trigger_key       ssh TxnCore::verify and binding_multiplicity
                             (FR-3): only the trigger key, as host replay
                             looks it up (rs/replay_rspace.rs:949-957 for a
                             consume, 1112-1120 for a produce)
   available                 ssh SpaceTxn::recorded_comms (FR-3: the trigger
                             key of the queried source)
   stage_without_trigger_    ssh TxnCore::verify (decision 36: an unpaired
     unpaired                fire stages, verify gives None)
   StDiverged                ssh SpaceTxnError::ReplayDivergence, raised at
                             commit (decision 36)
   sg_paired, pair_step,     ssh TxnCore::verify (the recorded COMM or None,
     stage_fire              never an error), StagedFire.recorded and
                             StagedConsumeFire.recorded. stage_fire_some is
                             the only fact of stage_fire that the proofs of
                             H1-H12 use
   commit_replay_produce,    rs/replay_rspace/space_replay.rs ReplayBackend::
     commit_replay_consume   produce_fire and consume_observed: an unpaired
     (paired = false)        fire fails with ReplayDivergence after
                             observe_comm accepts. produce_observed rolls the
                             counter step back
   unpaired_fire_inert       an unpaired fire logs, removes and unbinds
                             nothing (decision 36)
   fire_paired               the replay premise of H1, H2 and H11
   erase_dir,                replay_operation_directive: a stateless query
     ob_ignores_directive,   of replay that play does not make. The
     dir_always_none         production RhoCommObserver gives no directive
                             for a space operation (H19 covers one)
   replay_reproduces_play_   play keeps the log of a failed deploy and
     run, replay_sim_gen     reverts it (casper/src/rust/rholang/runtime.rs:934-958).
                             Replay compares the status, then the cost
                             (casper/src/rust/rholang/replay_runtime.rs:619-632)
   space_prelude, commit_*   design §2.1 (play) and §2.2 (replay) tables,
                             ssh commit_operation and commit_staged
   space_pf_loop,            design §2.1 row 6 (record removal by equality).
     space_cf_loop,          Decision 45 (Rule E): no loop has a retain flag,
     vpf_loop, vcf_loop      so a peek fire removes its stored
                             non-persistent records. Each commit of a fire
                             calls remove_matched_datum_and_join
                             (rs/rspace/space_play.rs:208,
                             rs/replay_rspace/space_replay.rs:300) or
                             store_persistent_data
                             (rs/rspace/space_play.rs:307,
                             rs/replay_rspace/space_replay.rs:442). ssh
                             removed_records (rs/space_store_handle.rs:1595)
                             stages the same records
   backend                   design §2.3 (ReportingRspace delegates)
   sub_order, sched_part     rholang/src/rust/interpreter/
                             deterministic_reduction.rs:84-135,
                             rs/operation_context.rs:177-214, ssh sub_order
   covered                   ssh check_event_correlation (FR-7: it runs in
                             every build profile. FR-7': it reads the staged
                             effects of one operation)
   host_rm_valid, hpos       ssh check_removed_records, check_staged_position
                             (FR-7': each removed record sits at its staged
                             position)
   group_check, canonical,   ssh require_canonical, canonical_group_key,
     nondecreasing,          group_identity (FR-1'): the keys must be
     chan_leb                non-decreasing, else SpaceTxnError::
                             NonCanonicalChannelOrder (FR-1' removes
                             DuplicateChannelInGroup)
   isort, sorted_unique_     the canonical form of a group: its channels
     canonical               sorted by hash, as hash_vec sorts them
   op_groups, access,        ssh TxnCore::admit_group, read_continuations,
     session                 read_joins (TxnView::load_joins checks each
                             returned group), stage_produce, stage_consume
   CKey, touch, stage_keys   rs/hot_store.rs:671-718 (get_continuations
                             caches the history value), 805-817
                             (get_data), 927-955 (get_joins), ssh
                             TxnView::load_data, load_continuations,
                             load_joins
   leaf_at, ck_actions       rs/hot_store.rs:1115-1170 (changes)
   hkey                      rs/history/history_repository_impl.rs:306-348
                             (transform: hash, hash_from_vec), rs/hashing/
                             stable_hash_provider.rs (hash_from_hashes sorts
                             the channel hashes)
   checkpoint, dup_free      rs/history/history_repository_impl.rs:467-490
                             (checkpoint), rs/history/instances/
                             radix_history.rs:100-105 (process rejects two
                             actions on one key)
   txn_lifecycle, aborts     ssh SpaceStoreHandle::begin, HostTxn::commit,
                             TxnRegistration (drop), commit_staged
                             (SpaceTxnError::StoreSwapped)
   sg_src, sg_start          rs/replay_rspace/space_replay.rs
                             reject_directive, produce_observed,
                             consume_observed (SpaceTxnError::
                             NativeReplayDirectiveUnsupported)
   Tracker, commit_gate,     ssh CommittedParents::admit, TxnRegistration::
     run_calls               admit_parent, HandleState (FR-5, SpaceTxnError::
                             ParentOrderReused), CommittedParents::reset
   CallScope, handle_step,   ssh SpaceStoreHandle::begin_evaluation_scope
     tracker_reset           (FR-5': P4 calls it at the entry of each root
                             evaluation, in play and in replay)
   tr_store, same_scope      ssh CommittedParents: the store Arc that the
                             tracker records (FR-5': a reset or a revert
                             replaces it)
   sid, view_get, overlay,   ssh channel_key, group_identity, the TxnView
     installed_by_sid        maps keyed by Blake2b256Hash, and the installed
                             set of TxnCore (FR-2)
   peeks_ok                  ssh check_peeks (rs/space_store_handle.rs:1524):
                             no peeks, or every bind peeks, else
                             MixedArrowJoin. Decision 45: it only checks the
                             peek set (peek_retention is disabled)
   vpf_effects, vcf_effects, FR-10 and decision 45: ssh TxnCore::stage_produce
     commit_play_produce,    (rs/space_store_handle.rs:2405),
     commit_replay_produce,  apply_fire_to_view (rs/space_store_handle.rs:2559)
     commit_play_consume,    and TxnCore::stage_consume
     commit_replay_consume   (rs/space_store_handle.rs:2620), PlayBackend::
                             produce_fire and consume_observed
                             (rs/rspace/space_play.rs:161 and 243), and
                             ReplayBackend::produce_fire and consume_observed
                             (rs/replay_rspace/space_replay.rs:226 and 347).
                             A fire never stores its incoming datum, and a
                             peek fire removes the records of any fire
   fire_stores_no_datum,     FR-10 after decision 45 (design report §C.10):
     fire_view_removes       a fire removes exactly what the host removes,
                             and it never stores a datum
   wrap_flags,               ssh fired_receipt (rs/space_store_handle.rs:3206)
     fired_receipt           keeps the flags of wrap_result
                             (rs/rspace/ops_consume.rs:204-229). It fails
                             with Inconsistent when the results do not line
                             up with the candidates. wrap_result gives one
                             result per candidate, and the model maps each
                             candidate, so that check never fails here
   reoffer_due, reoffer_of,  FR-10: the agent re-offers the datum of a peek
     reoffers                fire as the next operation of the transaction,
                             with the same Produce::create identity
                             (decision 30)
   restore_of,               decision 45: receive_head
     restores_after          (rholang/src/rust/interpreter/spaces/generic.rs:1591)
                             copies the head record when a peek fires on a
                             non-persistent record
                             (rholang/src/rust/interpreter/spaces/generic.rs:1635).
                             restore_peeked and Cx::restore
                             (rholang/src/rust/interpreter/spaces/generic.rs:1678
                             and 1057) stage a Store of that record with
                             persist = false right after the fire. The model
                             does not include the record authority that the
                             re-store registers (Introduction::for_record,
                             rholang/src/rust/interpreter/spaces/record.rs:2149)
   host_produce_peeks,       produce_peeks
     head_reproduces,        (rholang/src/rust/interpreter/reduce.rs:981-1016)
     restore_reproduces      produces the removed value of each result whose
                             persist flag is false, not persistent. The
                             reducer calls it after a consume with a peek
                             fires (rholang/src/rust/interpreter/reduce.rs:909-936),
                             after a non-persistent produce fires a peek
                             waiter (rholang/src/rust/interpreter/reduce.rs:784-811)
                             and after a persistent produce
                             (rholang/src/rust/interpreter/reduce.rs:776-781).
                             rs/util.rs:57-70 and rs/rspace/ops_consume.rs:204-229
                             give the data_list tuples, and
                             rs/space_matcher.rs:72-90 gives removed_datum
   stored_sources,           Produce::create (rs/trace/event.rs:238-249)
     restore_same_identity   hashes the channel, the data and the persist
                             flag. ssh stage_produce gives a staged produce
                             that identity (rs/space_store_handle.rs:2422). So
                             a re-store of the bytes of a record has the
                             identity of the source produce of the record
   restore_store_staged,     a re-store is a staged Store. put_datum inserts
     restores_stage,         at index 0 (rs/hot_store.rs:879-897), so the
     peek_restore_keeps_data record moves to the front, and each data list
                             keeps its multiset (design report §C.7)
   peek_restores_sim         the transaction of a peek with its re-stores
                             against the host program of the peek COMM and
                             produce_peeks (design report §C.5)
   no_pending_match, offer,  the produce plan of a head-only queue with the
     offer_ops, offer_p1     re-offer (p4-design-report B.4, invariant INV).
                             offer_p1 is the same plan under the P1 rule
                             before FR-10. P4 has no code for it yet
   (disabled)                decision 45 disables retained, retaining,
                             retained_nonretaining, retained_stored,
                             space_pf_loop_retain_data, space_cf_loop_retain,
                             peek_fire_keeps_data, retained_result,
                             receipt_persistent, receipt_reports_reoffer,
                             kept_reproduces, head_reproduces_consume,
                             values_on, kept_reproduces_values,
                             kept_reproduces_retained, kept_after,
                             store_rel_kept_perm, the store_rel and the
                             rel_after_loops with kept records, and the
                             H11 peek_retention_trace_eq_host. They modeled
                             ssh peek_retention and retained_results, which
                             the code disables too

   ─────────────────────────────────────────────────────────────────────────
   Modeling notes
   ─────────────────────────────────────────────────────────────────────────
   - The host types are Section variables with decidable equality. Record
     equality stands for equality of bincode bytes, because bincode is
     injective on these types.
   - A channel of the model is a store identity (FR-2): every substrate map
     and set uses stable_hash_provider::hash(channel). H21 shows that view
     lookups by store identity agree with store lookups, and that the Ord
     keys before FR-2 do not.
   - The continuation dedup identity (a Debug string) is the tuple of the
     patterns, the continuation, the persist flag and the peeks.
   - The observer is deterministic. Its decision is a function of the full
     history of its calls. before_observe is a fallible call at the position
     of register_introduction_authority.
   - A Produce identity stands for the Produce hash. Produce metadata
     (is_deterministic, output_value, failed) is not modeled.
   - The store is a set of total functions: the hot store view with history
     read-through. The cache list of a World holds the cached keys. A
     first read of a key caches it, also in a transaction that aborts.
   - The history is a map from history keys to leaves. A Merkle radix tree
     has one root for each map, so a checkpoint with the same map has the
     same root.
   - The locks of a transaction cover its channels from begin to commit.
     FR-4 enforces this in the code (LockCoverage::require,
     SpaceTxnError::UnlockedChannel), and TxnLockCoverage.tla (T5) checks
     it. So the model runs staging and commit with no interleaving.
   - FR-1: the model caches every stage key of a stage that passes the
     FR-1 check, also when staging fails later. The code can fail before it
     loads every key. H9 covers each such case, because it holds for every
     extension of the cache by fresh canonical keys. A joins read loads its
     own key, which is canonical, before it checks the returned groups.
   - An access error is a validation error. The code returns it before any
     view mutation and keeps the transaction open. TxnCore::poison closes
     only a transaction whose view mutation failed after validation. The
     view operations of the model are total, so that case does not arise in
     the model. The agent sees the error and continues, commits or drops.
   - Counters are natural numbers. FR-6 makes every i32 counter
     computation of the code checked: an overflow fails with
     SpaceTxnError::CounterOverflow before any effect. The theorems hold
     for every execution that does not overflow.
   - FR-7: the commit-time state check and check_event_correlation run in
     every build profile. H6 and H10 show that neither check fails for a
     staged transaction.
   - FR-9 (SpaceTxnError::NestedTransaction) is a guard at begin with no
     store effect. The model does not include it. VolatileSweep.v models
     the FR-9' guard of the sweep (H13).
   - FR-1' (round 2): a group is canonical when it is non-decreasing by
     hash. Staging and commit already handle repeated channels as the host
     does: the candidates align with the channels position by position
     (pfire_ok, cfire_ok), stored_ok binds a non-persistent record at most
     once per COMM, and a produce-side fire binds the incoming datum
     exactly once. So H1-H12 hold for repeated channels with no change.
   - FR-5' (round 2): the store identity of the tracker stands for the
     store Arc. CommittedParents records a Weak reference to the hot store
     and compares it with Weak::ptr_eq. A Weak reference keeps the
     allocation, so a store that a reset or a revert installs cannot reuse
     the address of the recorded store. The model gives such a store a new
     identity.
   - FR-7' (round 2) changes how the always-on checks of FR-7 compute their
     result: only the changed entries. check_removed_records checks that
     each removed record sits at its staged position (host_rm_valid), and
     check_event_correlation reads the staged effects of the operation. The
     failure semantics stay (Inconsistent, EventCoverage). H6 shows that
     the staged view is the committed store, so the staged effects are the
     effects of the commit, and H10 shows that the correlation never fails.
     So H6 and H10 are unchanged.
   - FR-10 (round 3): a produce-side fire never stores its incoming datum
     (vpf_effects, commit_play_produce, commit_replay_produce). A peek fire
     leaves the datum to the agent. The agent re-offers it as the next
     operation of the transaction (reoffers): a produce with the same
     channel, value and persist flag. So the re-offer has the produce
     identity, the observer calls and the counter step of HEAD's re-produce
     (reduce.rs:981-1016). The re-offer fires the next waiter or stores the
     datum. When the fired waiter is persistent, it stays at the head, and
     the P4 plan re-issues the datum in a new transaction (p4-design-report
     A10, a divergent pair). The model does not include the authority value
     that before_observe registers.
   - Decision 45 (Rule E, "Apply everywhere"): HEAD's reducer re-produces
     each non-persistent result of a peek fire (produce_peeks). The
     substrate keeps no record of a peek fire. A fire removes each stored
     non-persistent candidate, on the consume side and on the produce side
     (joins and repeated-channel groups), as the host does. So the stores
     of the substrate and the host are equal up to the order of each data
     list (store_rel, H2), and the receipt keeps the host flags (wrap_flags,
     fired_receipt). HEAD's re-produces are the re-offer of the incoming
     datum (decision 30) and the re-stores of the removed stored records
     (head_reproduces_produce, restores_are_produce_peeks). The agent
     stages the re-stores of a consume-side peek fire right after the fire
     (restores_after). A P4 produce-side fire removes no stored record,
     because its one-channel group binds only the incoming datum.
   - A re-store sends the bytes of the record, so its identity is
     Produce::create of the channel, the value and persist = false. This is
     the source of the record when each stored source is the
     Produce::create identity of its record (stored_sources). Staging keeps
     that property (stage_keeps_stored_sources), and every staged Store and
     every host produce stores such a source. The model gives the re-store
     the outcome Store, as in P4: the queue fires a receive only at its
     head when no waiter is parked, so the re-produce meets no waiter. In
     replay, a re-store is a staged Store, so replay stages it as play does
     (H23), and the log holds its Produce event.
   - The deviation of decision 19 (the kept stored records) is gone. The
     disabled definitions and lemmas stay in the module as comments.
   - In replay, a native directive sends the host to replay_native_produce or
     replay_native_consume. The model stops the host there (StNative). The
     substrate fails closed (decision D4, H19). H1 assumes that the observer
     gives no native directive, as the production RhoCommObserver does.
   - consume_replay_index is an index over replay_data. It does not change
     which COMMs are bound, so the model does not include it.
   - Decision 36: staging pairs a replay fire when verify accepts it, and
     stages it unpaired otherwise (sg_paired = false). Staging no longer
     depends on pairing, so replay stages exactly the operations of play
     (replay_stage_sim). The commit of an unpaired fire makes the calls of
     play and fails with StDiverged after the observer accepts its COMM
     (unpaired_fire_inert). The model does not count phlo. An out-of-phlo
     charge is an observer rejection, because ob_accept is a function of
     the call history and the accounting layer charges in the observer
     calls. So H23 covers it: replay rejects the same call after the same
     prefix, and the attempts are the same, so the cost is the same. H23
     assumes that ob_accept ignores the directive queries of the history
     (ob_ignores_directive) and that the observer gives no directive
     (dir_always_none).
   - Design deviation: the literal verify rule of design §2.4 compares a raw
     binding count with the number of staged pairings. A COMM whose produce
     list repeats a produce carries two bindings per occurrence under that
     key, so the literal rule accepts a second pairing of a COMM that the log
     records once (lemma design_verify_overpairs). The model normalizes by
     the key multiplicity. FR-3 then restricts the rule to the trigger key.
     The rule before FR-3 accepted any key of the COMM (lemma
     any_key_pairs_without_trigger, review D3).

   ─────────────────────────────────────────────────────────────────────────
   Headlines (each is closed under the global context)
   ─────────────────────────────────────────────────────────────────────────
   H1  commit_trace_eq_host            same observer calls, events, status
   H2  commit_state_eq_host            same store (data up to permutation).
                                       Decision 45: no premise about peeks
   H3  counter_timing_play_replay      play closure = replay increment rule
   H4  verify_sound_complete           trigger-key pairing (FR-3) is sound
                                       and complete. Decision 36: a fire
                                       without its trigger event stages
                                       unpaired, and an unpaired fire never
                                       commits
   H5  replay_pairs_each_comm_once     every recorded COMM pairs once at most
   H6  view_commit_coherent            staged view = committed store
   H7  joins_lockstep_preserved        joins stay in lockstep
   H8  sub_order_between               sub-orders are contiguous and unique
   H9  abort_inert                     an abort after any reads and stages
                                       leaves the checkpointed store
                                       unchanged (FR-1)
   H10 coverage_total                  every change has its event
   H11 peek_reproduces_trace_eq_host   peeks (FR-10, decision 45 Rule E): a
                                       consume-side peek fire followed by
                                       the re-stores of the agent makes the
                                       trace, the observer calls and the
                                       store of the host peek COMM followed
                                       by produce_peeks. A fire stores no
                                       datum, its view loses exactly its
                                       non-persistent records, and its
                                       receipt keeps the host flags. A
                                       re-store is a staged Store with the
                                       identity of its record, and the peek
                                       with its re-stores keeps each data
                                       list up to permutation. The re-offer
                                       (decision 30) keeps the head-only
                                       no-pending-match invariant
   H12 reporting_trace_is_replay_trace reporting delegates to replay
   H17 non_canonical_fails_closed      FR-1' rejects a group that is not
                                       non-decreasing before any store or
                                       cache effect. A sorted group with a
                                       repeated channel passes. Each
                                       permutation class has exactly one
                                       member that passes
   H19 directive_fails_closed          a replay directive rejects the
                                       operation with no effects
   H20 parent_order_single_txn         FR-5 and FR-5' make sub-orders
                                       unique in one evaluation scope. A
                                       scope reset, a store swap and a
                                       session change each reset the
                                       tracker
   H21 view_identity_is_store_identity FR-2: view lookups agree with store
                                       lookups
   H23 replay_reproduces_play_run      decision 36: with the commit log of
                                       play, replay stages the operations of
                                       play and its commit makes the calls
                                       (directive queries erased), events,
                                       status and store of play
   H24 native_replay_reproduces_play   decision 47: over the rows of native
                                       play, native replay makes the
                                       observer calls, the effects and the
                                       status of play, with one row for
                                       each operation
   H25 native_replay_denial_keeps_prefix
                                       a recorded denial at operation k
                                       gives the failure of play, with the
                                       operations before k applied
   H26 native_replay_fails_closed      no ready row, a forged footprint, an
                                       outcome that does not fit and another
                                       candidate fail before any effect
   H27 view_charge_cache_independent   P5: a view load charges its key and
                                       its value only, so a hit and a miss
                                       charge the same

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables become universally quantified parameters of the closed
   theorems. The file imports only the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - A fixed sequence of destructs (call prefix, COMM check, observer
     decision) in the commit lemmas breaks when a control moves the
     pairing check before observe_comm. commit_split takes the decisions
     apart in any order.
   - `simpl in Hst` unfolds stage_fire. A proof that inverts a staged fire
     then depends on the body of stage_fire, and the negative control of a
     staging failure breaks it before H23. Use `cbn [negb] in Hst` and
     stage_fire_some.
   - `first [unfold commit_play_produce | unfold commit_replay_produce | ..]`
     takes the first branch also when that constant does not occur, so the
     commit of the other mode stays folded. Unfold the four commits in one
     `unfold`.
   - After `destruct (backend (t_mode T)) eqn:Hb`, Rocq 9.1.1 also rewrites
     the hypotheses, so a side goal is `Replay <> Play`, and
     `rewrite Hb; discriminate` fails. Use
     `first [discriminate | rewrite Hb; discriminate]`.
   - `fold src in *` fails ("Cannot create self-referring hypothesis"): it
     also folds the body of the local definition src.
   - `f_equal` closes `erase_dir [o0; a; od] = erase_dir [o0; a]` by
     conversion, so a later tactic finds no goal.
   - Decision 45: a control that makes only the two consume commits retain
     the records of a peek fire cannot fail first in op_sim or H2. It fails
     first in coh_consume_fire, which restates the commit expression of a
     consume fire (tested). The control ec-peek-retains also makes the view
     retain, and it fails in the H11 lemma fire_view_removes, which comes
     before the commit lemmas.
   - Decision 47: in H24, `rewrite Sc` replaces every occurrence of
     `staged_comm o` at once, so a second `rewrite Sc` for the second match
     fails ("Found no subterm matching"). Rewrite once, then `simpl`.
   - Decision 45: a leaf tactic of fire_stores_no_datum that names the loop
     of each commit fails under the control ec-peek-retains, although the
     statement stays true there (tested). The same tactic proves the
     pristine lemma. Split each unknown boolean test of the leaf first
     (destruct_ifs), then close each case.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List PeanoNat Bool Lia Permutation Sorting.Sorted Arith.
Import ListNotations.

(* ───────────────────────── Generic list toolkit ───────────────────────── *)

Fixpoint remove_first {A : Type} (eqd : forall x y : A, {x = y} + {x <> y})
    (x : A) (l : list A) : list A :=
  match l with
  | [] => []
  | y :: t => if eqd x y then t else y :: remove_first eqd x t
  end.

Fixpoint remove_nth {A : Type} (i : nat) (l : list A) : list A :=
  match l, i with
  | [], _ => []
  | _ :: t, 0 => t
  | y :: t, S j => y :: remove_nth j t
  end.

Lemma count_occ_remove_first :
  forall (A : Type) (eqd : forall x y : A, {x = y} + {x <> y}) x l y,
    count_occ eqd (remove_first eqd x l) y =
    count_occ eqd l y - (if eqd x y then 1 else 0).
Proof.
  intros A eqd x l y. induction l as [| h t IH]; simpl.
  - destruct (eqd x y); reflexivity.
  - destruct (eqd x h) as [Exh | Nxh].
    + subst h. destruct (eqd x y) as [Exy | Nxy].
      * subst y. destruct (eqd x x) as [_ | C]; [lia | contradiction].
      * destruct (eqd x y) as [C | _]; [contradiction | lia].
    + simpl. rewrite IH. destruct (eqd h y) as [Ehy | Nhy].
      * subst y. destruct (eqd x h) as [C | _]; [contradiction | lia].
      * reflexivity.
Qed.

Lemma remove_first_perm :
  forall (A : Type) (eqd : forall x y : A, {x = y} + {x <> y}) x l,
    In x l -> Permutation l (x :: remove_first eqd x l).
Proof.
  intros A eqd x l. induction l as [| h t IH]; simpl; intros Hin.
  - contradiction.
  - destruct (eqd x h) as [E | N].
    + subst h. reflexivity.
    + destruct Hin as [E | Hin]; [congruence |].
      eapply perm_trans; [apply perm_skip, IH, Hin | apply perm_swap].
Qed.

Lemma remove_first_not_in :
  forall (A : Type) (eqd : forall x y : A, {x = y} + {x <> y}) x l,
    ~ In x l -> remove_first eqd x l = l.
Proof.
  intros A eqd x l. induction l as [| h t IH]; simpl; intros Hn; auto.
  destruct (eqd x h) as [E | N].
  - exfalso. apply Hn. left. symmetry. exact E.
  - f_equal. apply IH. intro H. apply Hn. right. exact H.
Qed.

Lemma remove_first_incl :
  forall (A : Type) (eqd : forall x y : A, {x = y} + {x <> y}) x l z,
    In z (remove_first eqd x l) -> In z l.
Proof.
  intros A eqd x l z. induction l as [| h t IH]; simpl; auto.
  destruct (eqd x h); simpl; intuition.
Qed.

Lemma remove_first_nodup :
  forall (A : Type) (eqd : forall x y : A, {x = y} + {x <> y}) x l,
    NoDup l -> NoDup (remove_first eqd x l).
Proof.
  intros A eqd x l. induction l as [| h t IH]; simpl; intros Hnd; auto.
  inversion Hnd as [| h' t' Hni Hndt]; subst.
  destruct (eqd x h).
  - exact Hndt.
  - constructor.
    + intro Hin. apply Hni. eapply remove_first_incl. exact Hin.
    + apply IH. exact Hndt.
Qed.

Lemma remove_first_nodup_not_in :
  forall (A : Type) (eqd : forall x y : A, {x = y} + {x <> y}) x l,
    NoDup l -> ~ In x (remove_first eqd x l).
Proof.
  intros A eqd x l. induction l as [| h t IH]; simpl; intros Hnd; auto.
  inversion Hnd as [| h' t' Hni Hndt]; subst.
  destruct (eqd x h) as [E | N].
  - subst h. exact Hni.
  - simpl. intros [E | Hin]; [congruence | exact (IH Hndt Hin)].
Qed.

Lemma remove_first_other_in :
  forall (A : Type) (eqd : forall x y : A, {x = y} + {x <> y}) x l z,
    z <> x -> In z l -> In z (remove_first eqd x l).
Proof.
  intros A eqd x l z Hzx. induction l as [| h t IH]; simpl; auto.
  intros [E | Hin].
  - subst h. destruct (eqd x z) as [E | _]; [congruence | left; reflexivity].
  - destruct (eqd x h); [exact Hin | right; apply IH, Hin].
Qed.

Lemma nth_error_remove_nth_lt :
  forall (A : Type) (l : list A) i j,
    j < i -> nth_error (remove_nth i l) j = nth_error l j.
Proof.
  intros A l. induction l as [| h t IH]; intros i j Hji; simpl.
  - destruct i; reflexivity.
  - destruct i as [| i']; [lia |].
    destruct j as [| j']; simpl; [reflexivity |].
    apply IH. lia.
Qed.

Lemma remove_nth_perm :
  forall (A : Type) (l : list A) i x,
    nth_error l i = Some x -> Permutation l (x :: remove_nth i l).
Proof.
  intros A l. induction l as [| h t IH]; intros i x Hn.
  - destruct i; discriminate.
  - destruct i as [| i']; simpl in *.
    + inversion Hn; subst. reflexivity.
    + eapply perm_trans; [apply perm_skip, IH, Hn | apply perm_swap].
Qed.

Lemma remove_nth_eq_remove_first :
  forall (A : Type) (eqd : forall x y : A, {x = y} + {x <> y}) l i x,
    NoDup l -> nth_error l i = Some x -> remove_nth i l = remove_first eqd x l.
Proof.
  intros A eqd l. induction l as [| h t IH]; intros i x Hnd Hn.
  - destruct i; discriminate.
  - inversion Hnd as [| h' t' Hni Hndt]; subst.
    destruct i as [| i']; simpl in *.
    + inversion Hn; subst. destruct (eqd x x) as [_ | C]; [reflexivity | contradiction].
    + destruct (eqd x h) as [E | N].
      * subst h. exfalso. apply Hni. eapply nth_error_In. exact Hn.
      * f_equal. apply IH; assumption.
Qed.

Lemma count_occ_remove_nth :
  forall (A : Type) (eqd : forall x y : A, {x = y} + {x <> y}) l i x y,
    nth_error l i = Some x ->
    count_occ eqd (remove_nth i l) y + (if eqd x y then 1 else 0) =
    count_occ eqd l y.
Proof.
  intros A eqd l i x y Hn.
  pose proof (remove_nth_perm A l i x Hn) as P.
  apply (Permutation_count_occ eqd) with (x := y) in P.
  rewrite P. simpl. destruct (eqd x y); lia.
Qed.

Lemma remove_nth_incl :
  forall (A : Type) (l : list A) i z, In z (remove_nth i l) -> In z l.
Proof.
  intros A l. induction l as [| h t IH]; intros i z Hin; simpl in *.
  - destruct i; contradiction.
  - destruct i as [| i']; [right; exact Hin |].
    destruct Hin as [E | Hin]; [left; exact E | right; eapply IH; exact Hin].
Qed.

Lemma remove_nth_nodup :
  forall (A : Type) (l : list A) i, NoDup l -> NoDup (remove_nth i l).
Proof.
  intros A l. induction l as [| h t IH]; intros i Hnd; simpl.
  - destruct i; constructor.
  - inversion Hnd as [| h' t' Hni Hndt]; subst.
    destruct i as [| i']; [exact Hndt |].
    constructor.
    + intro Hin. apply Hni. eapply remove_nth_incl. exact Hin.
    + apply IH. exact Hndt.
Qed.

(* A filter splits by a second predicate, up to order. *)
Lemma filter_split :
  forall (A : Type) (g f : A -> bool) (l : list A),
    Permutation (filter f l) (filter (fun x => g x && f x) l ++ filter (fun x => negb (g x) && f x) l).
Proof.
  intros A g f l. induction l as [| x t IH]; simpl; [reflexivity |].
  destruct (f x), (g x); simpl.
  - apply perm_skip, IH.
  - eapply perm_trans; [apply perm_skip, IH | apply Permutation_middle].
  - exact IH.
  - exact IH.
Qed.

Lemma filter_and :
  forall (A : Type) (g f : A -> bool) (l : list A),
    filter (fun x => g x && f x) l = filter f (filter g l).
Proof.
  intros A g f l. induction l as [| x t IH]; simpl; [reflexivity |].
  destruct (g x); simpl; [destruct (f x); [f_equal |]; exact IH | exact IH].
Qed.

(* Stable descending insertion sort keyed by a natural number. It models the
   stable Rust call [sort_by_key(|c| std::cmp::Reverse(c.datum_index))]. *)
Fixpoint ins_desc {A : Type} (key : A -> nat) (x : A) (l : list A) : list A :=
  match l with
  | [] => [x]
  | y :: t => if key y <? key x then x :: y :: t else y :: ins_desc key x t
  end.

Definition sort_desc {A : Type} (key : A -> nat) (l : list A) : list A :=
  fold_left (fun acc x => ins_desc key x acc) l [].

Lemma ins_desc_perm :
  forall (A : Type) (key : A -> nat) x l, Permutation (ins_desc key x l) (x :: l).
Proof.
  intros A key x l. induction l as [| y t IH]; simpl; [reflexivity |].
  destruct (key y <? key x); [reflexivity |].
  eapply perm_trans; [apply perm_skip, IH | apply perm_swap].
Qed.

Lemma sort_desc_acc_perm :
  forall (A : Type) (key : A -> nat) l acc,
    Permutation (fold_left (fun acc x => ins_desc key x acc) l acc) (l ++ acc).
Proof.
  intros A key l. induction l as [| x t IH]; intros acc; simpl; [reflexivity |].
  eapply perm_trans; [apply IH |].
  eapply perm_trans; [apply Permutation_app_head, ins_desc_perm |].
  apply Permutation_sym, Permutation_middle.
Qed.

Lemma sort_desc_perm :
  forall (A : Type) (key : A -> nat) l, Permutation (sort_desc key l) l.
Proof.
  intros A key l. unfold sort_desc.
  eapply perm_trans; [apply sort_desc_acc_perm | rewrite app_nil_r; reflexivity].
Qed.

Definition desc_rel {A : Type} (key : A -> nat) (a b : A) : Prop := key b <= key a.

Lemma ins_desc_sorted :
  forall (A : Type) (key : A -> nat) x l,
    StronglySorted (desc_rel key) l -> StronglySorted (desc_rel key) (ins_desc key x l).
Proof.
  intros A key x l. induction l as [| y t IH]; simpl; intros Hs.
  - repeat constructor.
  - apply StronglySorted_inv in Hs as [Hst Hfy].
    destruct (key y <? key x) eqn:Ek.
    + apply Nat.ltb_lt in Ek.
      constructor.
      * constructor; assumption.
      * constructor.
        -- unfold desc_rel. lia.
        -- eapply Forall_impl; [| exact Hfy].
           intros z Hz. unfold desc_rel in *. lia.
    + apply Nat.ltb_ge in Ek.
      constructor.
      * apply IH. exact Hst.
      * eapply Permutation_Forall; [apply Permutation_sym, ins_desc_perm |].
        constructor; [unfold desc_rel; lia | exact Hfy].
Qed.

Lemma sort_desc_sorted :
  forall (A : Type) (key : A -> nat) l, StronglySorted (desc_rel key) (sort_desc key l).
Proof.
  intros A key l. unfold sort_desc.
  assert (G : forall acc, StronglySorted (desc_rel key) acc ->
            StronglySorted (desc_rel key) (fold_left (fun acc x => ins_desc key x acc) l acc)).
  { induction l as [| x t IH]; intros acc Hacc; simpl; [exact Hacc |].
    apply IH. apply ins_desc_sorted. exact Hacc. }
  apply G. constructor.
Qed.

(* Iterated removal of a fixed element. The closed form of the remove_join
   loops uses it. *)
Definition rm_iter {A : Type} (eqd : forall x y : A, {x = y} + {x <> y})
    (n : nat) (g : A) (l : list A) : list A :=
  Nat.iter n (remove_first eqd g) l.

Lemma rm_iter_S :
  forall (A : Type) (eqd : forall x y : A, {x = y} + {x <> y}) n g l,
    rm_iter eqd (S n) g l = rm_iter eqd n g (remove_first eqd g l).
Proof.
  intros A eqd n g l. unfold rm_iter. revert l.
  induction n as [| n IH]; intros l; simpl; [reflexivity |].
  f_equal. rewrite <- IH. reflexivity.
Qed.

(* Insertion sort under a comparison [leb]: the canonical form of a channel
   group (FR-1'). hash_vec sorts the channel hashes the same way. *)
Fixpoint insb {A : Type} (leb : A -> A -> bool) (x : A) (l : list A) : list A :=
  match l with
  | [] => [x]
  | y :: t => if leb x y then x :: y :: t else y :: insb leb x t
  end.

Fixpoint isort {A : Type} (leb : A -> A -> bool) (l : list A) : list A :=
  match l with [] => [] | x :: t => insb leb x (isort leb t) end.

Lemma insb_perm : forall (A : Type) leb (x : A) l, Permutation (insb leb x l) (x :: l).
Proof.
  intros A leb x l. induction l as [| y t IH]; simpl; [reflexivity |].
  destruct (leb x y); [reflexivity |].
  eapply perm_trans; [apply perm_skip, IH | apply perm_swap].
Qed.

Lemma isort_perm : forall (A : Type) leb (l : list A), Permutation (isort leb l) l.
Proof.
  intros A leb l. induction l as [| x t IH]; simpl; [reflexivity |].
  eapply perm_trans; [apply insb_perm | apply perm_skip, IH].
Qed.

(* Under a total comparison, insertion keeps a list sorted, so the
   insertion sort is sorted. *)
Lemma insb_sorted :
  forall (A : Type) (leb : A -> A -> bool), (forall x y, leb x y = true \/ leb y x = true) ->
    forall x l, Sorted (fun a b => leb a b = true) l -> Sorted (fun a b => leb a b = true) (insb leb x l).
Proof.
  intros A leb Htot x l Hs. induction Hs as [| y t Hs IH Hhd]; simpl; [repeat constructor |].
  destruct (leb x y) eqn:Exy.
  - constructor; [constructor; [exact Hs | exact Hhd] | constructor; exact Exy].
  - assert (Hyx : leb y x = true) by (destruct (Htot x y) as [C | C]; [congruence | exact C]).
    constructor; [exact IH |].
    destruct t as [| z t']; simpl; [constructor; exact Hyx |].
    destruct (leb x z); constructor; [exact Hyx | inversion Hhd; assumption].
Qed.

Lemma isort_sorted :
  forall (A : Type) (leb : A -> A -> bool), (forall x y, leb x y = true \/ leb y x = true) ->
    forall l, Sorted (fun a b => leb a b = true) (isort leb l).
Proof.
  intros A leb Htot l. induction l as [| x t IH]; simpl; [constructor |].
  apply insb_sorted; assumption.
Qed.

(* ═════════════════════════════ The model ═════════════════════════════════ *)

Section Substrate.

(* Abstract host types C (channels), A (data), P (patterns), K (continuations)
   and the identity types of Produce and Consume events. *)
Variables Chan Val Pat Kont PId CId : Type.
Variable chan_eq_dec : forall x y : Chan, {x = y} + {x <> y}.
Variable val_eq_dec : forall x y : Val, {x = y} + {x <> y}.
Variable pat_eq_dec : forall x y : Pat, {x = y} + {x <> y}.
Variable kont_eq_dec : forall x y : Kont, {x = y} + {x <> y}.
Variable pid_eq_dec : forall x y : PId, {x = y} + {x <> y}.
Variable cid_eq_dec : forall x y : CId, {x = y} + {x <> y}.

(* Produce::create and Consume::create (event.rs:238-249, 322-337). *)
Variable produce_id : Chan -> Val -> bool -> PId.
Variable consume_id : list Chan -> list Pat -> Kont -> bool -> CId.

(* The comparison that COMM::new uses to sort the produce identities
   (event.rs:60-65).  Any comparison function is allowed. *)
Variable pid_leb : PId -> PId -> bool.

(* ── Records ─────────────────────────────────────────────────────────── *)

Record Datum := mkDatum { d_val : Val; d_persist : bool; d_src : PId }.

Record WK := mkWK {
  w_pats : list Pat; w_kont : Kont; w_persist : bool;
  w_peeks : list nat; w_src : CId }.

(* A candidate. [cd_vidx] is the view index of the substrate
   (bind_candidate). [cd_hidx] is the datum_index of the host matcher. The
   substrate never reads [cd_hidx]. The host never reads [cd_vidx]. *)
Record Cand := mkCand {
  cd_chan : Chan; cd_matched : Val; cd_rec : Datum;
  cd_incoming : bool; cd_vidx : nat; cd_hidx : nat }.

Record Comm := mkComm {
  c_consume : CId; c_produces : list PId; c_peeks : list nat;
  c_times : list (PId * nat) }.

Definition lc_eq_dec : forall x y : list Chan, {x = y} + {x <> y} :=
  list_eq_dec chan_eq_dec.

Definition datum_eq_dec : forall x y : Datum, {x = y} + {x <> y}.
Proof.
  decide equality; first [apply pid_eq_dec | apply bool_dec | apply val_eq_dec].
Defined.

Definition wk_eq_dec : forall x y : WK, {x = y} + {x <> y}.
Proof.
  decide equality;
    first [ apply cid_eq_dec | apply (list_eq_dec Nat.eq_dec) | apply bool_dec
          | apply kont_eq_dec | apply (list_eq_dec pat_eq_dec) ].
Defined.

Definition times_eq_dec : forall x y : list (PId * nat), {x = y} + {x <> y}.
Proof.
  apply list_eq_dec. intros [a n] [b m].
  destruct (pid_eq_dec a b) as [E | N]; [| right; congruence].
  destruct (Nat.eq_dec n m) as [E2 | N2]; [left; congruence | right; congruence].
Defined.

Definition comm_eq_dec : forall x y : Comm, {x = y} + {x <> y}.
Proof.
  decide equality;
    first [ apply times_eq_dec | apply (list_eq_dec Nat.eq_dec)
          | apply (list_eq_dec pid_eq_dec) | apply cid_eq_dec ].
Defined.

Definition comm_eqb (x y : Comm) : bool := if comm_eq_dec x y then true else false.

(* ── Events, observer calls and statuses ──────────────────────────────── *)

Inductive Src := SrcP (p : PId) | SrcC (c : CId).
Inductive Completion := Stored | Matched | Rejected.
Inductive Side := ProduceSide | ConsumeSide.

Inductive Ev :=
| EvProduce (p : PId)
| EvConsume (c : CId)
| EvComm (s : Side) (cm : Comm).

Inductive Obs :=
| ObBefore (s : Src)
| ObStart (s : Src) (chs : list Chan) (joins : list (list Chan))
| ObDirective (s : Src)
| ObProduce (p : PId) (ch : Chan) (v : Val) (persist : bool)
| ObConsume (c : CId) (chs : list Chan) (pats : list Pat) (k : Kont)
    (persist : bool) (peeks : list nat)
| ObComm (cm : Comm) (k : Kont) (persist : bool) (data : list (Val * bool))
| ObFinish (s : Src) (r : Completion).

Inductive Dir := DirNone | DirSome | DirErr.

(* A deterministic observer: its decision is a function of the full history
   of calls it has received.  [ob_accept] covers the fallible calls
   (before_observe, observe_operation_start, observe_produce,
   observe_consume and observe_comm).  [ob_directive] is
   replay_operation_directive.  observe_operation_finish cannot fail. *)
Variable ob_accept : list Obs -> Obs -> bool.
Variable ob_directive : list Obs -> Src -> Dir.

(* StDiverged is SpaceTxnError::ReplayDivergence at commit (decision 36): a
   replay fire that staged unpaired, after the observer accepted every call
   of the operation. *)
Inductive Status := StOk | StRejected | StPanic | StNative | StDiverged.

(* Effects that one operation applies to the store, for the event-coverage
   check. *)
Inductive Fx :=
| FxPutDatum (c : Chan) (d : Datum)
| FxRemoveDatum (c : Chan) (d : Datum)
| FxPutCont (g : list Chan) (w : WK)
| FxRemoveCont (g : list Chan) (w : WK)
| FxPutJoin (c : Chan) (g : list Chan)
| FxRemoveJoin (c : Chan) (g : list Chan).

(* ── Replay bindings (replay_rspace.rs:71, 480-540, 1498-1526) ─────────── *)

Inductive IOKey := KConsume (c : CId) | KProduce (p : PId).

Definition iokey_eq_dec : forall x y : IOKey, {x = y} + {x <> y}.
Proof. decide equality; first [apply cid_eq_dec | apply pid_eq_dec]. Defined.

Definition Bindings := IOKey -> Comm -> nat.

(* Consume first, then the produces in COMM order (design §2.4). *)
Definition keys (cm : Comm) : list IOKey :=
  KConsume (c_consume cm) :: map KProduce (c_produces cm).

Definition mult (k : IOKey) (cm : Comm) : nat := count_occ iokey_eq_dec (keys cm) k.

(* remove_bindings_for: one remove_binding_in_place per key occurrence,
   saturating at zero (internal.rs MultisetMultiMap). *)
Definition unbind (rd : Bindings) (cm : Comm) : Bindings :=
  fun k c => if comm_eq_dec c cm then rd k c - mult k cm else rd k c.

Definition bound (rd : Bindings) (k : IOKey) (cm : Comm) : bool := 0 <? rd k cm.

(* ── Modes ─────────────────────────────────────────────────────────────── *)

Inductive Mode := Play | Replay | Reporting.

(* ReportingRspace delegates every SpaceTxnHost call to its inner replay
   space (design §2.3). *)
Definition backend (m : Mode) : Mode :=
  match m with Reporting => Replay | m' => m' end.

(* ── Store (hot store with history read-through) ──────────────────────── *)

Record Store := mkStore {
  s_data : Chan -> list Datum;
  s_conts : list Chan -> list WK;
  s_joins : Chan -> list (list Chan);
  s_count : PId -> nat }.

Definition store_eq (S1 S2 : Store) : Prop :=
  (forall c, s_data S1 c = s_data S2 c) /\
  (forall g, s_conts S1 g = s_conts S2 g) /\
  (forall c, s_joins S1 c = s_joins S2 c) /\
  (forall p, s_count S1 p = s_count S2 p).

(* Disabled by decision 45 (Rule E): no fire keeps a record, so no kept records separate the two stores.
(* The store of the substrate and the store of the host agree, except for
   the [kept] records. The substrate holds these records and the host does
   not. *)
Definition store_rel (Sc Sh : Store) (kept : Chan -> list Datum) : Prop :=
  (forall c, Permutation (s_data Sc c) (s_data Sh c ++ kept c)) /\
  (forall g, s_conts Sc g = s_conts Sh g) /\
  (forall c, s_joins Sc c = s_joins Sh c) /\
  (forall p, s_count Sc p = s_count Sh p).
*)

(* The store of the substrate and the store of the host agree. Each data
   list of the substrate is a permutation of the data list of the host, and
   the continuations, the joins and the counters are equal. Decision 45
   (Rule E): a fire removes the records that the host removes, so no kept
   record separates the two stores. *)
Definition store_rel (Sc Sh : Store) : Prop :=
  (forall c, Permutation (s_data Sc c) (s_data Sh c)) /\
  (forall g, s_conts Sc g = s_conts Sh g) /\
  (forall c, s_joins Sc c = s_joins Sh c) /\
  (forall p, s_count Sc p = s_count Sh p).

Definition upd_data (S : Store) (c : Chan) (l : list Datum) : Store :=
  mkStore (fun c' => if chan_eq_dec c' c then l else s_data S c')
          (s_conts S) (s_joins S) (s_count S).
Definition upd_conts (S : Store) (g : list Chan) (l : list WK) : Store :=
  mkStore (s_data S) (fun g' => if lc_eq_dec g' g then l else s_conts S g')
          (s_joins S) (s_count S).
Definition upd_joins (S : Store) (c : Chan) (l : list (list Chan)) : Store :=
  mkStore (s_data S) (s_conts S)
          (fun c' => if chan_eq_dec c' c then l else s_joins S c') (s_count S).
Definition upd_count (S : Store) (k : PId -> nat) : Store :=
  mkStore (s_data S) (s_conts S) (s_joins S) k.

(* ── Counters (trace_log.rs:69-86, 158-172 and replay_rspace.rs:808-834,
      1579-1600) ────────────────────────────────────────────────────────── *)

Definition pid_eqb (a b : PId) : bool := if pid_eq_dec a b then true else false.

Definition incr (k : PId -> nat) (src : PId) (persist : bool) : PId -> nat :=
  fun p => if negb persist && pid_eqb p src then S (k p) else k p.

Definition rollback (k : PId -> nat) (src : PId) (persist : bool) : PId -> nat :=
  fun p => if negb persist && pid_eqb p src then pred (k p) else k p.

Definition bump (S : Store) (src : PId) (persist : bool) : Store :=
  upd_count S (incr (s_count S) src persist).

Definition unbump (S : Store) (src : PId) (persist : bool) : Store :=
  upd_count S (rollback (s_count S) src persist).

(* produce_counters (trace_log.rs:69-86 and replay_rspace.rs:808-824). *)
Definition counters_of (k : PId -> nat) (ps : list PId) : list (PId * nat) :=
  map (fun p => (p, k p)) ps.

(* The play produce closure (ops_produce.rs:185-191). *)
Definition play_counters_closure (k : PId -> nat) (src : PId) (persist : bool)
    (ps : list PId) : list (PId * nat) :=
  map (fun p => (p, k p + (if negb persist && existsb (pid_eqb src) ps && pid_eqb p src
                           then 1 else 0))) ps.

(* ── COMM::new (event.rs:37-78) ───────────────────────────────────────── *)

Fixpoint pins (x : PId) (l : list PId) : list PId :=
  match l with
  | [] => [x]
  | y :: t => if pid_leb x y then x :: y :: t else y :: pins x t
  end.

Fixpoint comm_sort (l : list PId) : list PId :=
  match l with [] => [] | x :: t => pins x (comm_sort t) end.

Definition cand_srcs (cands : list Cand) : list PId :=
  map (fun cd => d_src (cd_rec cd)) cands.

Definition mk_comm (cands : list Cand) (consume : CId) (peeks : list nat)
    (closure : list PId -> list (PId * nat)) : Comm :=
  let ps := comm_sort (cand_srcs cands) in mkComm consume ps peeks (closure ps).

Definition comm_data (cands : list Cand) : list (Val * bool) :=
  map (fun cd => (cd_matched cd, d_persist (cd_rec cd))) cands.

(* ── Hot-store primitives (hot_store.rs) ──────────────────────────────── *)

(* The continuation dedup identity (hot_store.rs:1315-1317). *)
Definition wk_ident (w : WK) : list Pat * Kont * bool * list nat :=
  (w_pats w, w_kont w, w_persist w, w_peeks w).

Definition ident_eq_dec :
  forall x y : list Pat * Kont * bool * list nat, {x = y} + {x <> y}.
Proof.
  repeat decide equality;
    first [apply pat_eq_dec | apply kont_eq_dec | apply bool_dec | apply Nat.eq_dec].
Defined.

Definition ident_eqb (w1 w2 : WK) : bool :=
  if ident_eq_dec (wk_ident w1) (wk_ident w2) then true else false.

(* put_datum inserts at index 0 (hot_store.rs:879-897). *)
Definition put_datum (S : Store) (c : Chan) (d : Datum) : Store * list Fx :=
  (upd_data S c (d :: s_data S c), [FxPutDatum c d]).

(* Host remove_datum by index (hot_store.rs:912-945). None is the
   out-of-bounds error. *)
Definition host_remove_datum (S : Store) (c : Chan) (i : nat) : option (Store * list Fx) :=
  match nth_error (s_data S c) i with
  | Some d => Some (upd_data S c (remove_nth i (s_data S c)), [FxRemoveDatum c d])
  | None => None
  end.

(* Substrate record removal: the first record that is equal (design §1.3). *)
Definition space_remove_datum (S : Store) (c : Chan) (d : Datum) : Store * list Fx :=
  if in_dec datum_eq_dec d (s_data S c)
  then (upd_data S c (remove_first datum_eq_dec d (s_data S c)), [FxRemoveDatum c d])
  else (S, []).

(* put_continuation: insert at index 0 unless the identity is present
   (hot_store.rs:720-783). *)
Definition put_continuation (S : Store) (g : list Chan) (w : WK) : Store * list Fx :=
  if existsb (ident_eqb w) (s_conts S g)
  then (S, [])
  else (upd_conts S g (w :: s_conts S g), [FxPutCont g w]).

(* Host remove_continuation by index (hot_store.rs:818-861), for channels
   without an installed continuation. *)
Definition host_remove_continuation (S : Store) (g : list Chan) (i : nat) : Store * list Fx :=
  match nth_error (s_conts S g) i with
  | Some w => (upd_conts S g (remove_nth i (s_conts S g)), [FxRemoveCont g w])
  | None => (S, [])
  end.

Definition space_remove_continuation (S : Store) (g : list Chan) (w : WK) : Store * list Fx :=
  if in_dec wk_eq_dec w (s_conts S g)
  then (upd_conts S g (remove_first wk_eq_dec w (s_conts S g)), [FxRemoveCont g w])
  else (S, []).

(* put_join inserts at index 0 only if absent (hot_store.rs:1017-1040). *)
Definition put_join (S : Store) (c : Chan) (g : list Chan) : Store * list Fx :=
  if in_dec lc_eq_dec g (s_joins S c)
  then (S, [])
  else (upd_joins S c (g :: s_joins S c), [FxPutJoin c g]).

(* remove_join removes only if the continuation list of the join is empty
   (hot_store.rs:1069-1113). *)
Definition remove_join (S : Store) (c : Chan) (g : list Chan) : Store * list Fx :=
  match s_conts S g with
  | [] =>
      if in_dec lc_eq_dec g (s_joins S c)
      then (upd_joins S c (remove_first lc_eq_dec g (s_joins S c)), [FxRemoveJoin c g])
      else (S, [])
  | _ :: _ => (S, [])
  end.

Fixpoint put_joins (S : Store) (cs : list Chan) (g : list Chan) : Store * list Fx :=
  match cs with
  | [] => (S, [])
  | c :: rest =>
      let p1 := put_join S c g in
      let p2 := put_joins (fst p1) rest g in (fst p2, snd p1 ++ snd p2)
  end.

(* store_waiting_continuation (ops_consume.rs:179-190 and
   replay_rspace.rs:1652-1668). *)
Definition store_waiting_continuation (S : Store) (chs : list Chan) (w : WK) : Store * list Fx :=
  let p1 := put_continuation S chs w in
  let p2 := put_joins (fst p1) chs chs in (fst p2, snd p1 ++ snd p2).

(* ── Operations ───────────────────────────────────────────────────────── *)

Inductive POut :=
| PStore
| PFire (chs : list Chan) (wk : WK) (kidx : nat) (cands : list Cand).

Inductive COut :=
| CPark
| CFire (cands : list Cand).

Inductive Op :=
| OProduce (ch : Chan) (v : Val) (persist : bool) (out : POut)
| OConsume (chs : list Chan) (pats : list Pat) (k : Kont) (persist : bool)
    (peeks : list nat) (out : COut).

Definition removable (cd : Cand) : bool :=
  negb (cd_incoming cd) && negb (d_persist (cd_rec cd)).

(* The sort keys: Reverse(datum_index) with the incoming datum (-1) last. *)
Definition hkey (cd : Cand) : nat := if cd_incoming cd then 0 else S (cd_hidx cd).
Definition vkey (cd : Cand) : nat := if cd_incoming cd then 0 else S (cd_vidx cd).

(* Host remove_matched_datum_and_join (ops_produce.rs:228-248 and
   replay_rspace.rs:1798-1818). On a remove_datum error, the loop continues
   and skips remove_join. *)
Fixpoint host_rm_loop (S : Store) (chs : list Chan) (cs : list Cand) : Store * list Fx :=
  match cs with
  | [] => (S, [])
  | cd :: rest =>
      let p1 :=
        if removable cd
        then match host_remove_datum S (cd_chan cd) (cd_hidx cd) with
             | None => (S, [])
             | Some q => let q' := remove_join (fst q) (cd_chan cd) chs in (fst q', snd q ++ snd q')
             end
        else remove_join S (cd_chan cd) chs in
      let p2 := host_rm_loop (fst p1) chs rest in (fst p2, snd p1 ++ snd p2)
  end.

(* Host store_persistent_data (ops_consume.rs:192-202 and
   replay_rspace.rs:1686-1696). The loop ignores errors. *)
Fixpoint host_spd_loop (S : Store) (cs : list Cand) : Store * list Fx :=
  match cs with
  | [] => (S, [])
  | cd :: rest =>
      let p1 :=
        if negb (d_persist (cd_rec cd))
        then match host_remove_datum S (cd_chan cd) (cd_hidx cd) with
             | None => (S, [])
             | Some r => r
             end
        else (S, []) in
      let p2 := host_spd_loop (fst p1) rest in (fst p2, snd p1 ++ snd p2)
  end.

(* Substrate produce-side loop (design §2.1 row 6, §2.2). Decision 45 (Rule
   E): the loop has no retain flag. A peek fire removes each stored
   non-persistent candidate, as the host loop host_rm_loop does. *)
Fixpoint space_pf_loop (S : Store) (chs : list Chan) (cs : list Cand)
    : Store * list Fx :=
  match cs with
  | [] => (S, [])
  | cd :: rest =>
      let p1 :=
        (* Disabled by decision 45 (Rule E): a retaining (peek) fire removed no record.
        if negb retain && removable cd then space_remove_datum S (cd_chan cd) (cd_rec cd) *)
        if removable cd then space_remove_datum S (cd_chan cd) (cd_rec cd)
        else (S, []) in
      let p2 := remove_join (fst p1) (cd_chan cd) chs in
      let p3 := space_pf_loop (fst p2) chs rest in (fst p3, snd p1 ++ snd p2 ++ snd p3)
  end.

(* Substrate consume-side loop (design §2.1 row 6, §2.2). Decision 45 (Rule
   E): the loop has no retain flag. A peek fire removes each stored
   non-persistent candidate, as the host loop host_spd_loop does. *)
Fixpoint space_cf_loop (S : Store) (cs : list Cand) : Store * list Fx :=
  match cs with
  | [] => (S, [])
  | cd :: rest =>
      let p1 :=
        (* Disabled by decision 45 (Rule E): a retaining (peek) fire removed no record.
        if negb retain && negb (d_persist (cd_rec cd)) *)
        if negb (d_persist (cd_rec cd))
        then space_remove_datum S (cd_chan cd) (cd_rec cd) else (S, []) in
      let p2 := space_cf_loop (fst p1) rest in (fst p2, snd p1 ++ snd p2)
  end.

Definition nullb {A : Type} (l : list A) : bool :=
  match l with [] => true | _ => false end.

(* ── Results ──────────────────────────────────────────────────────────── *)

Record Res := mkRes {
  r_st : Store; r_rd : Bindings; r_obs : list Obs; r_ev : list Ev;
  r_fx : list Fx; r_status : Status }.

Inductive Pre := PreStop (obs : list Obs) (st : Status) | PreGo (obs : list Obs).

(* Host call prefix (accounting.rs:20-31, ops_produce.rs:44-78,
   ops_consume.rs:45-81 and replay_rspace.rs:851-905, 1144-1192). The first
   call is the register_introduction_authority call of the reducer
   (reduce.rs:539-543, 651-655). The substrate calls before_observe at the
   same position. *)
Definition host_prelude (m : Mode) (H : list Obs) (s : Src) (start observe : Obs) : Pre :=
  let o0 := ObBefore s in
  if negb (ob_accept H o0) then PreStop [o0] StRejected else
  if negb (ob_accept (H ++ [o0]) start) then PreStop [o0; start] StRejected else
  match m with
  | Play =>
      if negb (ob_accept (H ++ [o0; start]) observe)
      then PreStop [o0; start; observe; ObFinish s Rejected] StRejected
      else PreGo [o0; start; observe]
  | _ =>
      let od := ObDirective s in
      match ob_directive (H ++ [o0; start]) s with
      | DirNone =>
          if negb (ob_accept (H ++ [o0; start; od]) observe)
          then PreStop [o0; start; od; observe; ObFinish s Rejected] StRejected
          else PreGo [o0; start; od; observe]
      | DirErr => PreStop [o0; start; od; ObFinish s Rejected] StRejected
      | DirSome => PreStop [o0; start; od] StNative
      end
  end.

(* Substrate call prefix (design §2.1 rows 0-2 and §2.2 steps 0-3). A replay
   directive fails the operation closed (decision D4). *)
Definition space_prelude (m : Mode) (H : list Obs) (s : Src) (start observe : Obs) : Pre :=
  let o0 := ObBefore s in
  if negb (ob_accept H o0) then PreStop [o0] StRejected else
  if negb (ob_accept (H ++ [o0]) start) then PreStop [o0; start] StRejected else
  match m with
  | Play =>
      if negb (ob_accept (H ++ [o0; start]) observe)
      then PreStop [o0; start; observe; ObFinish s Rejected] StRejected
      else PreGo [o0; start; observe]
  | _ =>
      let od := ObDirective s in
      match ob_directive (H ++ [o0; start]) s with
      | DirNone =>
          if negb (ob_accept (H ++ [o0; start; od]) observe)
          then PreStop [o0; start; od; observe; ObFinish s Rejected] StRejected
          else PreGo [o0; start; od; observe]
      | _ => PreStop [o0; start; od; ObFinish s Rejected] StRejected
      end
  end.

(* ── Host operations, in HEAD code order ──────────────────────────────── *)

(* Play produce: ops_produce.rs:32-113 and 163-210. *)
Definition host_play_produce (H : list Obs) (S : Store) (rd : Bindings)
    (ch : Chan) (v : Val) (persist : bool) (out : POut) : Res :=
  let src := produce_id ch v persist in
  let s := SrcP src in
  match host_prelude Play H s (ObStart s [ch] (s_joins S ch)) (ObProduce src ch v persist) with
  | PreStop obs st => mkRes S rd obs [] [] st
  | PreGo obs =>
      match out with
      | PStore =>
          let p := put_datum (bump S src persist) ch (mkDatum v persist src) in
          mkRes (fst p) rd (obs ++ [ObFinish s Stored]) [EvProduce src] (snd p) StOk
      | PFire chs wk kidx cands =>
          let cm := mk_comm cands (w_src wk) (w_peeks wk)
                      (play_counters_closure (s_count S) src persist) in
          let oc := ObComm cm (w_kont wk) (w_persist wk) (comm_data cands) in
          if negb (ob_accept (H ++ obs) oc)
          then mkRes S rd (obs ++ [oc; ObFinish s Rejected]) [] [] StRejected
          else
            let S1 := bump S src persist in
            let p1 := if w_persist wk then (S1, []) else host_remove_continuation S1 chs kidx in
            let p2 := host_rm_loop (fst p1) chs (sort_desc hkey cands) in
            mkRes (fst p2) rd (obs ++ [oc; ObFinish s Matched])
                  [EvProduce src; EvComm ProduceSide cm] (snd p1 ++ snd p2) StOk
      end
  end.

(* Play consume: ops_consume.rs:31-154. *)
Definition host_play_consume (H : list Obs) (S : Store) (rd : Bindings)
    (chs : list Chan) (pats : list Pat) (k : Kont) (persist : bool) (peeks : list nat)
    (out : COut) : Res :=
  let src := consume_id chs pats k persist in
  let s := SrcC src in
  match host_prelude Play H s (ObStart s chs []) (ObConsume src chs pats k persist peeks) with
  | PreStop obs st => mkRes S rd obs [] [] st
  | PreGo obs =>
      match out with
      | CPark =>
          let p := store_waiting_continuation S chs (mkWK pats k persist peeks src) in
          mkRes (fst p) rd (obs ++ [ObFinish s Stored]) [EvConsume src] (snd p) StOk
      | CFire cands =>
          let cm := mk_comm cands src peeks (counters_of (s_count S)) in
          let oc := ObComm cm k persist (comm_data cands) in
          if negb (ob_accept (H ++ obs) oc)
          then mkRes S rd (obs ++ [oc; ObFinish s Rejected]) [] [] StRejected
          else
            let p := host_spd_loop S (sort_desc hkey cands) in
            mkRes (fst p) rd (obs ++ [oc; ObFinish s Matched])
                  [EvConsume src; EvComm ConsumeSide cm] (snd p) StOk
      end
  end.

(* Replay produce: replay_rspace.rs:1133-1267 and 1363-1425.  The assert!
   at 1305 is StPanic. *)
Definition host_replay_produce (H : list Obs) (S : Store) (rd : Bindings)
    (ch : Chan) (v : Val) (persist : bool) (out : POut) : Res :=
  let src := produce_id ch v persist in
  let s := SrcP src in
  match host_prelude Replay H s (ObStart s [ch] (s_joins S ch)) (ObProduce src ch v persist) with
  | PreStop obs st => mkRes S rd obs [] [] st
  | PreGo obs =>
      let S1 := bump S src persist in
      match out with
      | PStore =>
          let p := put_datum S1 ch (mkDatum v persist src) in
          mkRes (fst p) rd (obs ++ [ObFinish s Stored]) [EvProduce src] (snd p) StOk
      | PFire chs wk kidx cands =>
          let cm := mk_comm cands (w_src wk) (w_peeks wk) (counters_of (s_count S1)) in
          if negb (bound rd (KProduce src) cm) then mkRes S1 rd obs [] [] StPanic else
          let oc := ObComm cm (w_kont wk) (w_persist wk) (comm_data cands) in
          if negb (ob_accept (H ++ obs) oc)
          then mkRes (unbump S1 src persist) rd (obs ++ [oc; ObFinish s Rejected]) [] [] StRejected
          else
            let p1 := if w_persist wk then (S1, []) else host_remove_continuation S1 chs kidx in
            let p2 := host_rm_loop (fst p1) chs (sort_desc hkey cands) in
            mkRes (fst p2) (unbind rd cm) (obs ++ [oc; ObFinish s Matched])
                  [EvProduce src; EvComm ProduceSide cm] (snd p1 ++ snd p2) StOk
      end
  end.

(* Replay consume: replay_rspace.rs:836-1067. *)
Definition host_replay_consume (H : list Obs) (S : Store) (rd : Bindings)
    (chs : list Chan) (pats : list Pat) (k : Kont) (persist : bool) (peeks : list nat)
    (out : COut) : Res :=
  let src := consume_id chs pats k persist in
  let s := SrcC src in
  match host_prelude Replay H s (ObStart s chs []) (ObConsume src chs pats k persist peeks) with
  | PreStop obs st => mkRes S rd obs [] [] st
  | PreGo obs =>
      match out with
      | CPark =>
          let p := store_waiting_continuation S chs (mkWK pats k persist peeks src) in
          mkRes (fst p) rd (obs ++ [ObFinish s Stored]) [EvConsume src] (snd p) StOk
      | CFire cands =>
          let cm := mk_comm cands src peeks (counters_of (s_count S)) in
          if negb (bound rd (KConsume src) cm) then mkRes S rd obs [] [] StPanic else
          let oc := ObComm cm k persist (comm_data cands) in
          if negb (ob_accept (H ++ obs) oc)
          then mkRes S rd (obs ++ [oc; ObFinish s Rejected]) [] [] StRejected
          else
            let p := host_spd_loop S (sort_desc hkey cands) in
            mkRes (fst p) (unbind rd cm) (obs ++ [oc; ObFinish s Matched])
                  [EvConsume src; EvComm ConsumeSide cm] (snd p) StOk
      end
  end.

Definition host_op (m : Mode) (H : list Obs) (S : Store) (rd : Bindings) (op : Op) : Res :=
  match op with
  | OProduce ch v persist out =>
      match backend m with
      | Play => host_play_produce H S rd ch v persist out
      | _ => host_replay_produce H S rd ch v persist out
      end
  | OConsume chs pats k persist peeks out =>
      match backend m with
      | Play => host_play_consume H S rd chs pats k persist peeks out
      | _ => host_replay_consume H S rd chs pats k persist peeks out
      end
  end.

Fixpoint host_run (m : Mode) (H : list Obs) (S : Store) (rd : Bindings) (ops : list Op) : Res :=
  match ops with
  | [] => mkRes S rd [] [] [] StOk
  | op :: rest =>
      let r := host_op m H S rd op in
      match r_status r with
      | StOk =>
          let r' := host_run m (H ++ r_obs r) (r_st r) (r_rd r) rest in
          mkRes (r_st r') (r_rd r') (r_obs r ++ r_obs r') (r_ev r ++ r_ev r')
                (r_fx r ++ r_fx r') (r_status r')
      | _ => r
      end
  end.

(* ── Staged view (design §1.3: TxnView overlay over the captured store) ── *)

Fixpoint lookup {K B : Type} (eqd : forall x y : K, {x = y} + {x <> y})
    (k : K) (m : list (K * B)) : option B :=
  match m with
  | [] => None
  | (k', b) :: t => if eqd k k' then Some b else lookup eqd k t
  end.

Record View := mkView {
  v_base : Store;
  v_data : list (Chan * list Datum);
  v_conts : list (list Chan * list WK);
  v_joins : list (Chan * list (list Chan));
  v_delta : list (PId * nat) }.

Definition vdata (V : View) (c : Chan) : list Datum :=
  match lookup chan_eq_dec c (v_data V) with Some l => l | None => s_data (v_base V) c end.
Definition vconts (V : View) (g : list Chan) : list WK :=
  match lookup lc_eq_dec g (v_conts V) with Some l => l | None => s_conts (v_base V) g end.
Definition vjoins (V : View) (c : Chan) : list (list Chan) :=
  match lookup chan_eq_dec c (v_joins V) with Some l => l | None => s_joins (v_base V) c end.
Definition vdelta (V : View) (p : PId) : nat :=
  match lookup pid_eq_dec p (v_delta V) with Some n => n | None => 0 end.
(* κ_v(p) = κ_host(p) + δ(p) (design §1.3). *)
Definition vcount (V : View) (p : PId) : nat := s_count (v_base V) p + vdelta V p.

Definition view_store (V : View) : Store :=
  mkStore (vdata V) (vconts V) (vjoins V) (vcount V).

Definition view_init (S : Store) : View := mkView S [] [] [] [].

Definition vset_data (V : View) (c : Chan) (l : list Datum) : View :=
  mkView (v_base V) ((c, l) :: v_data V) (v_conts V) (v_joins V) (v_delta V).
Definition vset_conts (V : View) (g : list Chan) (l : list WK) : View :=
  mkView (v_base V) (v_data V) ((g, l) :: v_conts V) (v_joins V) (v_delta V).
Definition vset_joins (V : View) (c : Chan) (l : list (list Chan)) : View :=
  mkView (v_base V) (v_data V) (v_conts V) ((c, l) :: v_joins V) (v_delta V).
Definition vset_delta (V : View) (p : PId) (n : nat) : View :=
  mkView (v_base V) (v_data V) (v_conts V) (v_joins V) ((p, n) :: v_delta V).

Definition vbump (V : View) (src : PId) (persist : bool) : View :=
  if persist then V else vset_delta V src (S (vdelta V src)).

Definition vput_datum (V : View) (c : Chan) (d : Datum) : View :=
  vset_data V c (d :: vdata V c).
Definition vremove_datum (V : View) (c : Chan) (d : Datum) : View :=
  vset_data V c (remove_first datum_eq_dec d (vdata V c)).
Definition vput_continuation (V : View) (g : list Chan) (w : WK) : View :=
  if existsb (ident_eqb w) (vconts V g) then V else vset_conts V g (w :: vconts V g).
Definition vremove_continuation (V : View) (g : list Chan) (w : WK) : View :=
  vset_conts V g (remove_first wk_eq_dec w (vconts V g)).
Definition vput_join (V : View) (c : Chan) (g : list Chan) : View :=
  if in_dec lc_eq_dec g (vjoins V c) then V else vset_joins V c (g :: vjoins V c).
Definition vremove_join (V : View) (c : Chan) (g : list Chan) : View :=
  match vconts V g with
  | [] => vset_joins V c (remove_first lc_eq_dec g (vjoins V c))
  | _ :: _ => V
  end.

Fixpoint vput_joins (V : View) (cs : list Chan) (g : list Chan) : View :=
  match cs with [] => V | c :: rest => vput_joins (vput_join V c g) rest g end.

Definition vstore_wk (V : View) (chs : list Chan) (w : WK) : View :=
  vput_joins (vput_continuation V chs w) chs chs.

(* Decision 45 (Rule E): the view loops have no retain flag. *)
Fixpoint vpf_loop (V : View) (chs : list Chan) (cs : list Cand) : View :=
  match cs with
  | [] => V
  | cd :: rest =>
      (* Disabled by decision 45 (Rule E): a retaining (peek) fire removed no record.
      let V1 := if negb retain && removable cd then vremove_datum V (cd_chan cd) (cd_rec cd) else V in *)
      let V1 := if removable cd then vremove_datum V (cd_chan cd) (cd_rec cd) else V in
      vpf_loop (vremove_join V1 (cd_chan cd) chs) chs rest
  end.

Fixpoint vcf_loop (V : View) (cs : list Cand) : View :=
  match cs with
  | [] => V
  | cd :: rest =>
      (* Disabled by decision 45 (Rule E): a retaining (peek) fire removed no record.
      let V1 := if negb retain && negb (d_persist (cd_rec cd)) *)
      let V1 := if negb (d_persist (cd_rec cd))
                then vremove_datum V (cd_chan cd) (cd_rec cd) else V in
      vcf_loop V1 rest
  end.

(* vpf_effects gives the view effects of a produce-side fire (design §2.1
   row 6). The fire removes a non-persistent continuation. Then it removes
   the records that it consumes and the fired join of each candidate
   channel. Decision 45 (Rule E): a peek fire removes the same records as
   any other fire, as the host does. FR-10: the fire never stores its
   incoming datum, so the channel, value and persist flag of the produce do
   not change the view. A peek fire leaves the datum to the agent, which
   re-offers it as the next operation (reoffers). *)
Definition vpf_effects (V : View) (ch : Chan) (v : Val) (persist : bool)
    (chs : list Chan) (wk : WK) (cands : list Cand) : View :=
  let V1 := if w_persist wk then V else vremove_continuation V chs wk in
  (* Disabled by decision 45 (Rule E): a peek fire (retain) removed no stored record.
  let retain := negb (nullb (w_peeks wk)) in
  vpf_loop V1 chs retain (sort_desc vkey cands). *)
  vpf_loop V1 chs (sort_desc vkey cands).

(* vcf_effects gives the view effects of a consume-side fire: it removes the
   stored non-persistent candidates. Decision 45 (Rule E): the effects do not
   depend on the peeks, so a peek fire removes the same records as any other
   fire. The agent then re-stores each removed record (restores_after). *)
(* Disabled by decision 45 (Rule E): the peeks of a fire no longer select a retaining loop.
Definition vcf_effects (V : View) (peeks : list nat) (cands : list Cand) : View :=
  vcf_loop V (negb (nullb peeks)) (sort_desc vkey cands).
*)
Definition vcf_effects (V : View) (cands : list Cand) : View :=
  vcf_loop V (sort_desc vkey cands).

(* ── Staging (design §1.4) ────────────────────────────────────────────── *)

(* sg_paired holds when staging paired the fire with a recorded COMM: the
   field recorded of StagedFire and of StagedConsumeFire is Some. It is
   false in play, for a store or a park, and for a replay fire that staged
   unpaired (decision 36). *)
Record Staged := mkStaged {
  sg_op : Op; sg_joins0 : list (list Chan); sg_comm : option Comm; sg_paired : bool }.

Record Txn := mkTxn {
  t_mode : Mode;
  t_view : View;
  t_installed : Chan -> bool;
  t_rd : Bindings;
  t_paired : list Comm;
  t_staged : list Staged }.

Definition txn_push (T : Txn) (V : View) (paired : list Comm) (sg : Staged) : Txn :=
  mkTxn (t_mode T) V (t_installed T) (t_rd T) paired (t_staged T ++ [sg]).

(* The number of recorded copies of [cm] under [k] that this transaction
   has not paired yet: the binding count divided by the multiplicity of
   the key in the COMM (binding_multiplicity), minus the paired copies.
   SpaceTxn::recorded_comms reports these copies for the trigger key of the
   queried source (FR-3). *)
Definition available (rd : Bindings) (paired : list Comm) (k : IOKey) (cm : Comm) : nat :=
  match mult k cm with
  | 0 => 0
  | n => rd k cm / n - count_occ comm_eq_dec paired cm
  end.

(* Verify (design §2.4, FR-3): TxnCore::verify looks up only the trigger
   key [k] of the fire. [k] is KProduce source for a produce-side fire and
   KConsume source for a consume-side fire, exactly as host replay does
   (replay_rspace.rs:949-957, 1203-1211). The COMM pairs while an unpaired
   recorded copy remains under [k]: paired < count / binding_multiplicity.
   The normalization by the multiplicity makes the rule sound: a key that
   occurs [mult k cm] times in the COMM carries [mult k cm] bindings per
   recorded occurrence. *)
Definition verify (rd : Bindings) (paired : list Comm) (k : IOKey) (cm : Comm) : bool :=
  0 <? available rd paired k cm.

(* The rule before FR-3 (review D3): the COMM pairs under any of its keys.
   A consume binding then pairs a produce-side fire whose trigger Produce
   event the log lacks. The lemma any_key_pairs_without_trigger shows this. *)
Definition verify_any_key (rd : Bindings) (paired : list Comm) (cm : Comm) : bool :=
  existsb (fun k => verify rd paired k cm) (keys cm).

(* The verify rule exactly as the design report words it, with no
   normalization. It is unsound. The lemma design_verify_overpairs shows
   this. *)
Definition verify_literal (rd : Bindings) (paired : list Comm) (cm : Comm) : bool :=
  existsb (fun k => count_occ comm_eq_dec paired cm <? rd k cm) (keys cm).

(* Decision 36: pairing is total. TxnCore::verify returns the recorded COMM
   when the fire pairs and None otherwise. It no longer fails, so staging
   never fails because of pairing. pair_step gives the paired list after the
   fire and the flag sg_paired of the staged fire. *)
Definition pair_step (T : Txn) (k : IOKey) (cm : Comm) : list Comm * bool :=
  match backend (t_mode T) with
  | Play => (t_paired T, false)
  | _ => if verify (t_rd T) (t_paired T) k cm then (t_paired T ++ [cm], true) else (t_paired T, false)
  end.

(* stage_fire pushes a staged fire with the result of pair_step. A replay
   fire with no free recorded occurrence stages unpaired (decision 36): its
   view effects are the effects of play, and it adds no pairing. The rule
   of decision 19 failed here with ReplayDivergence. *)
Definition stage_fire (T : Txn) (V : View) (k : IOKey) (cm : Comm) (mk : bool -> Staged) : option Txn :=
  let ps := pair_step T k cm in
  Some (txn_push T V (fst ps) (mk (snd ps))).

Definition chan_eqb (a b : Chan) : bool := if chan_eq_dec a b then true else false.

Definition peeks_ok (peeks : list nat) (n : nat) : bool :=
  nullb peeks || (if list_eq_dec Nat.eq_dec peeks (seq 0 n) then true else false).

Definition np_recs (c : Chan) (cands : list Cand) : list Datum :=
  map cd_rec (filter (fun cd => removable cd && chan_eqb (cd_chan cd) c) cands).

(* Each stored candidate exists in the view, with multiplicity for
   non-persistent records (design §1.4 rule 1). *)
Definition stored_ok (V : View) (cands : list Cand) : bool :=
  forallb (fun cd =>
    if cd_incoming cd then true
    else if d_persist (cd_rec cd)
         then (if in_dec datum_eq_dec (cd_rec cd) (vdata V (cd_chan cd)) then true else false)
         else count_occ datum_eq_dec (np_recs (cd_chan cd) cands) (cd_rec cd)
              <=? count_occ datum_eq_dec (vdata V (cd_chan cd)) (cd_rec cd)) cands.

Definition not_installed (T : Txn) (chs : list Chan) : bool :=
  forallb (fun c => negb (t_installed T c)) chs.

Definition pfire_ok (T : Txn) (ch : Chan) (v : Val) (persist : bool)
    (chs : list Chan) (wk : WK) (cands : list Cand) : bool :=
  let V := t_view T in
  let src := produce_id ch v persist in
  negb (nullb chs) && not_installed T (ch :: chs)
  && (if lc_eq_dec (map cd_chan cands) chs then true else false)
  && (length (filter cd_incoming cands) =? 1)
  && forallb (fun cd => negb (cd_incoming cd)
                        || (chan_eqb (cd_chan cd) ch
                            && (if datum_eq_dec (cd_rec cd) (mkDatum v persist src)
                                then true else false))) cands
  && (if in_dec wk_eq_dec wk (vconts V chs) then true else false)
  && peeks_ok (w_peeks wk) (length chs)
  && stored_ok V cands.

Definition consume_args_ok (T : Txn) (chs : list Chan) (pats : list Pat) (peeks : list nat) : bool :=
  negb (nullb chs) && (length chs =? length pats) && not_installed T chs
  && peeks_ok peeks (length chs).

Definition cfire_ok (T : Txn) (chs : list Chan) (cands : list Cand) : bool :=
  (if lc_eq_dec (map cd_chan cands) chs then true else false)
  && forallb (fun cd => negb (cd_incoming cd)) cands
  && stored_ok (t_view T) cands.

Definition stage (T : Txn) (op : Op) : option Txn :=
  let V := t_view T in
  match op with
  | OProduce ch v persist out =>
      let src := produce_id ch v persist in
      if t_installed T ch then None else
      match out with
      | PStore =>
          Some (txn_push T (vput_datum (vbump V src persist) ch (mkDatum v persist src))
                         (t_paired T) (mkStaged op (vjoins V ch) None false))
      | PFire chs wk kidx cands =>
          if negb (pfire_ok T ch v persist chs wk cands) then None else
          let cm := mk_comm cands (w_src wk) (w_peeks wk)
                      (play_counters_closure (vcount V) src persist) in
          stage_fire T (vpf_effects (vbump V src persist) ch v persist chs wk cands)
                     (KProduce src) cm (mkStaged op (vjoins V ch) (Some cm))
      end
  | OConsume chs pats k persist peeks out =>
      let src := consume_id chs pats k persist in
      if negb (consume_args_ok T chs pats peeks) then None else
      let wk := mkWK pats k persist peeks src in
      match out with
      | CPark => Some (txn_push T (vstore_wk V chs wk) (t_paired T) (mkStaged op [] None false))
      | CFire cands =>
          if negb (cfire_ok T chs cands) then None else
          let cm := mk_comm cands src peeks (counters_of (vcount V)) in
          (* Disabled by decision 45 (Rule E): the peeks no longer select the view effect.
          stage_fire T (vcf_effects V peeks cands) (KConsume src) cm (mkStaged op [] (Some cm)) *)
          stage_fire T (vcf_effects V cands) (KConsume src) cm (mkStaged op [] (Some cm))
      end
  end.

Fixpoint stage_all (T : Txn) (ops : list Op) : option Txn :=
  match ops with
  | [] => Some T
  | op :: rest => match stage T op with None => None | Some T' => stage_all T' rest end
  end.

(* ── Commit (design §2.1, §2.2) ───────────────────────────────────────── *)

(* FR-10: the commit of a produce-side fire never stores the incoming datum,
   in play (space_play.rs) and in replay (space_replay.rs). The effects of a
   fire are the continuation removal and the loop of row 6. *)
Definition commit_play_produce (H : list Obs) (S : Store) (rd : Bindings)
    (ch : Chan) (v : Val) (persist : bool) (out : POut)
    (joins0 : list (list Chan)) (scomm : option Comm) : Res :=
  let src := produce_id ch v persist in
  let s := SrcP src in
  match space_prelude Play H s (ObStart s [ch] joins0) (ObProduce src ch v persist) with
  | PreStop obs st => mkRes S rd obs [] [] st
  | PreGo obs =>
      match out with
      | PStore =>
          let p := put_datum (bump S src persist) ch (mkDatum v persist src) in
          mkRes (fst p) rd (obs ++ [ObFinish s Stored]) [EvProduce src] (snd p) StOk
      | PFire chs wk kidx cands =>
          let live := mk_comm cands (w_src wk) (w_peeks wk)
                        (play_counters_closure (s_count S) src persist) in
          match scomm with
          | None => mkRes S rd (obs ++ [ObFinish s Rejected]) [] [] StRejected
          | Some cm =>
              if negb (comm_eqb live cm)
              then mkRes S rd (obs ++ [ObFinish s Rejected]) [] [] StRejected
              else
                let oc := ObComm cm (w_kont wk) (w_persist wk) (comm_data cands) in
                if negb (ob_accept (H ++ obs) oc)
                then mkRes S rd (obs ++ [oc; ObFinish s Rejected]) [] [] StRejected
                else
                  let S1 := bump S src persist in
                  let p1 := if w_persist wk then (S1, []) else space_remove_continuation S1 chs wk in
                  (* Disabled by decision 45 (Rule E): a peek fire removed only the fired joins.
                  let retain := negb (nullb (w_peeks wk)) in
                  let p2 := space_pf_loop (fst p1) chs retain (sort_desc vkey cands) in *)
                  let p2 := space_pf_loop (fst p1) chs (sort_desc vkey cands) in
                  mkRes (fst p2) rd (obs ++ [oc; ObFinish s Matched])
                        [EvProduce src; EvComm ProduceSide cm] (snd p1 ++ snd p2) StOk
          end
      end
  end.

Definition commit_play_consume (H : list Obs) (S : Store) (rd : Bindings)
    (chs : list Chan) (pats : list Pat) (k : Kont) (persist : bool) (peeks : list nat)
    (out : COut) (scomm : option Comm) : Res :=
  let src := consume_id chs pats k persist in
  let s := SrcC src in
  match space_prelude Play H s (ObStart s chs []) (ObConsume src chs pats k persist peeks) with
  | PreStop obs st => mkRes S rd obs [] [] st
  | PreGo obs =>
      match out with
      | CPark =>
          let p := store_waiting_continuation S chs (mkWK pats k persist peeks src) in
          mkRes (fst p) rd (obs ++ [ObFinish s Stored]) [EvConsume src] (snd p) StOk
      | CFire cands =>
          let live := mk_comm cands src peeks (counters_of (s_count S)) in
          match scomm with
          | None => mkRes S rd (obs ++ [ObFinish s Rejected]) [] [] StRejected
          | Some cm =>
              if negb (comm_eqb live cm)
              then mkRes S rd (obs ++ [ObFinish s Rejected]) [] [] StRejected
              else
                let oc := ObComm cm k persist (comm_data cands) in
                if negb (ob_accept (H ++ obs) oc)
                then mkRes S rd (obs ++ [oc; ObFinish s Rejected]) [] [] StRejected
                else
                  (* Disabled by decision 45 (Rule E): a peek fire kept its stored records.
                  let p := space_cf_loop S (negb (nullb peeks)) (sort_desc vkey cands) in *)
                  let p := space_cf_loop S (sort_desc vkey cands) in
                  mkRes (fst p) rd (obs ++ [oc; ObFinish s Matched])
                        [EvConsume src; EvComm ConsumeSide cm] (snd p) StOk
          end
      end
  end.

(* Decision 36: after the observer accepts the COMM, an unpaired fire
   (paired = false) fails with ReplayDivergence (StDiverged). The counter
   step rolls back, as for a rejection (produce_observed), and the fire
   logs, removes and unbinds nothing (space_replay.rs ReplayBackend::
   produce_fire and consume_observed). *)
Definition commit_replay_produce (H : list Obs) (S : Store) (rd : Bindings)
    (ch : Chan) (v : Val) (persist : bool) (out : POut)
    (joins0 : list (list Chan)) (scomm : option Comm) (paired : bool) : Res :=
  let src := produce_id ch v persist in
  let s := SrcP src in
  match space_prelude Replay H s (ObStart s [ch] joins0) (ObProduce src ch v persist) with
  | PreStop obs st => mkRes S rd obs [] [] st
  | PreGo obs =>
      let S1 := bump S src persist in
      match out with
      | PStore =>
          let p := put_datum S1 ch (mkDatum v persist src) in
          mkRes (fst p) rd (obs ++ [ObFinish s Stored]) [EvProduce src] (snd p) StOk
      | PFire chs wk kidx cands =>
          let live := mk_comm cands (w_src wk) (w_peeks wk) (counters_of (s_count S1)) in
          match scomm with
          | None => mkRes (unbump S1 src persist) rd (obs ++ [ObFinish s Rejected]) [] [] StRejected
          | Some cm =>
              if negb (comm_eqb live cm)
              then mkRes (unbump S1 src persist) rd (obs ++ [ObFinish s Rejected]) [] [] StRejected
              else
                let oc := ObComm cm (w_kont wk) (w_persist wk) (comm_data cands) in
                if negb (ob_accept (H ++ obs) oc)
                then mkRes (unbump S1 src persist) rd (obs ++ [oc; ObFinish s Rejected]) [] [] StRejected
                else if negb paired
                then mkRes (unbump S1 src persist) rd (obs ++ [oc; ObFinish s Rejected]) [] [] StDiverged
                else
                  let p1 := if w_persist wk then (S1, []) else space_remove_continuation S1 chs wk in
                  (* Disabled by decision 45 (Rule E): a peek fire removed only the fired joins.
                  let retain := negb (nullb (w_peeks wk)) in
                  let p2 := space_pf_loop (fst p1) chs retain (sort_desc vkey cands) in *)
                  let p2 := space_pf_loop (fst p1) chs (sort_desc vkey cands) in
                  mkRes (fst p2) (unbind rd cm) (obs ++ [oc; ObFinish s Matched])
                        [EvProduce src; EvComm ProduceSide cm] (snd p1 ++ snd p2) StOk
          end
      end
  end.

Definition commit_replay_consume (H : list Obs) (S : Store) (rd : Bindings)
    (chs : list Chan) (pats : list Pat) (k : Kont) (persist : bool) (peeks : list nat)
    (out : COut) (scomm : option Comm) (paired : bool) : Res :=
  let src := consume_id chs pats k persist in
  let s := SrcC src in
  match space_prelude Replay H s (ObStart s chs []) (ObConsume src chs pats k persist peeks) with
  | PreStop obs st => mkRes S rd obs [] [] st
  | PreGo obs =>
      match out with
      | CPark =>
          let p := store_waiting_continuation S chs (mkWK pats k persist peeks src) in
          mkRes (fst p) rd (obs ++ [ObFinish s Stored]) [EvConsume src] (snd p) StOk
      | CFire cands =>
          let live := mk_comm cands src peeks (counters_of (s_count S)) in
          match scomm with
          | None => mkRes S rd (obs ++ [ObFinish s Rejected]) [] [] StRejected
          | Some cm =>
              if negb (comm_eqb live cm)
              then mkRes S rd (obs ++ [ObFinish s Rejected]) [] [] StRejected
              else
                let oc := ObComm cm k persist (comm_data cands) in
                if negb (ob_accept (H ++ obs) oc)
                then mkRes S rd (obs ++ [oc; ObFinish s Rejected]) [] [] StRejected
                else if negb paired
                then mkRes S rd (obs ++ [oc; ObFinish s Rejected]) [] [] StDiverged
                else
                  (* Disabled by decision 45 (Rule E): a peek fire kept its stored records.
                  let p := space_cf_loop S (negb (nullb peeks)) (sort_desc vkey cands) in *)
                  let p := space_cf_loop S (sort_desc vkey cands) in
                  mkRes (fst p) (unbind rd cm) (obs ++ [oc; ObFinish s Matched])
                        [EvConsume src; EvComm ConsumeSide cm] (snd p) StOk
          end
      end
  end.

Definition commit_op (m : Mode) (H : list Obs) (S : Store) (rd : Bindings) (sg : Staged) : Res :=
  match sg_op sg with
  | OProduce ch v persist out =>
      match backend m with
      | Play => commit_play_produce H S rd ch v persist out (sg_joins0 sg) (sg_comm sg)
      | _ => commit_replay_produce H S rd ch v persist out (sg_joins0 sg) (sg_comm sg) (sg_paired sg)
      end
  | OConsume chs pats k persist peeks out =>
      match backend m with
      | Play => commit_play_consume H S rd chs pats k persist peeks out (sg_comm sg)
      | _ => commit_replay_consume H S rd chs pats k persist peeks out (sg_comm sg) (sg_paired sg)
      end
  end.

Fixpoint commit_run (m : Mode) (H : list Obs) (S : Store) (rd : Bindings)
    (sgs : list Staged) : Res :=
  match sgs with
  | [] => mkRes S rd [] [] [] StOk
  | sg :: rest =>
      let r := commit_op m H S rd sg in
      match r_status r with
      | StOk =>
          let r' := commit_run m (H ++ r_obs r) (r_st r) (r_rd r) rest in
          mkRes (r_st r') (r_rd r') (r_obs r ++ r_obs r') (r_ev r ++ r_ev r')
                (r_fx r ++ r_fx r') (r_status r')
      | _ => r
      end
  end.

(* ═══════════════════════════ Basic store facts ═══════════════════════════ *)

Lemma store_eq_refl : forall S, store_eq S S.
Proof. intros S. repeat split; reflexivity. Qed.

Lemma store_eq_sym : forall S1 S2, store_eq S1 S2 -> store_eq S2 S1.
Proof.
  intros S1 S2 (Hd & Hc & Hj & Hn). repeat split; intros; symmetry; auto.
Qed.

Lemma store_eq_trans : forall S1 S2 S3, store_eq S1 S2 -> store_eq S2 S3 -> store_eq S1 S3.
Proof.
  intros S1 S2 S3 (Hd & Hc & Hj & Hn) (Hd' & Hc' & Hj' & Hn').
  repeat split; intros; [rewrite Hd | rewrite Hc | rewrite Hj | rewrite Hn]; auto.
Qed.

(* Decision 45 (Rule E): store_rel has no kept records, so these lemmas
   have no kept parameter. *)
Lemma store_rel_eq_l :
  forall S1 S2 Sh, store_eq S1 S2 -> store_rel S2 Sh -> store_rel S1 Sh.
Proof.
  intros S1 S2 Sh (Hd & Hc & Hj & Hn) (Rd & Rc & Rj & Rn).
  repeat split; intros; [rewrite Hd; apply Rd | rewrite Hc; apply Rc
                        | rewrite Hj; apply Rj | rewrite Hn; apply Rn].
Qed.

Lemma store_rel_eq_r :
  forall Sc S1 S2, store_eq S1 S2 -> store_rel Sc S2 -> store_rel Sc S1.
Proof.
  intros Sc S1 S2 (Hd & Hc & Hj & Hn) (Rd & Rc & Rj & Rn).
  repeat split; intros; [rewrite Hd; apply Rd | rewrite Hc; apply Rc
                        | rewrite Hj; apply Rj | rewrite Hn; apply Rn].
Qed.

Lemma store_rel_self : forall S, store_rel S S.
Proof. intros S. repeat split; intros; reflexivity. Qed.

Lemma bump_unbump_eq : forall S src persist, store_eq (unbump (bump S src persist) src persist) S.
Proof.
  intros S src persist. unfold unbump, bump, upd_count, rollback, incr.
  repeat split; intros; simpl; try reflexivity.
  destruct (negb persist && pid_eqb p src); reflexivity.
Qed.

(* Congruence of every primitive under store_eq. *)

Ltac solve_upd_cong :=
  match goal with
  | E : store_eq ?S1 ?S2 |- _ =>
      let Hd := fresh "Hd" in let Hc := fresh "Hc" in
      let Hj := fresh "Hj" in let Hn := fresh "Hn" in
      destruct E as (Hd & Hc & Hj & Hn);
      unfold upd_data, upd_conts, upd_joins, upd_count in *; simpl;
      repeat split; intros; simpl;
      repeat match goal with |- context [if ?x then _ else _] => destruct x end;
      repeat (first [rewrite Hd | rewrite Hc | rewrite Hj | rewrite Hn]); reflexivity
  end.

Lemma put_datum_cong :
  forall S1 S2 c d, store_eq S1 S2 ->
    store_eq (fst (put_datum S1 c d)) (fst (put_datum S2 c d)) /\
    snd (put_datum S1 c d) = snd (put_datum S2 c d).
Proof.
  intros S1 S2 c d E.
  unfold put_datum. simpl. split; [solve_upd_cong | reflexivity].
Qed.

Lemma bump_cong :
  forall S1 S2 src p, store_eq S1 S2 -> store_eq (bump S1 src p) (bump S2 src p).
Proof.
  intros S1 S2 src p E.
  unfold bump, incr. solve_upd_cong.
Qed.

Lemma unbump_cong :
  forall S1 S2 src p, store_eq S1 S2 -> store_eq (unbump S1 src p) (unbump S2 src p).
Proof.
  intros S1 S2 src p E.
  unfold unbump, rollback. solve_upd_cong.
Qed.

Lemma space_remove_datum_cong :
  forall S1 S2 c d, store_eq S1 S2 ->
    store_eq (fst (space_remove_datum S1 c d)) (fst (space_remove_datum S2 c d)) /\
    snd (space_remove_datum S1 c d) = snd (space_remove_datum S2 c d).
Proof.
  intros S1 S2 c d E. pose proof E as (Hd & Hc & Hj & Hn).
  unfold space_remove_datum. rewrite Hd.
  destruct (in_dec datum_eq_dec d (s_data S2 c)); simpl; split; try reflexivity;
    [solve_upd_cong | exact E].
Qed.

Lemma put_continuation_cong :
  forall S1 S2 g w, store_eq S1 S2 ->
    store_eq (fst (put_continuation S1 g w)) (fst (put_continuation S2 g w)) /\
    snd (put_continuation S1 g w) = snd (put_continuation S2 g w).
Proof.
  intros S1 S2 g w E. pose proof E as (Hd & Hc & Hj & Hn).
  unfold put_continuation. rewrite Hc.
  destruct (existsb (ident_eqb w) (s_conts S2 g)); simpl; split; try reflexivity;
    [exact E | solve_upd_cong].
Qed.

Lemma space_remove_continuation_cong :
  forall S1 S2 g w, store_eq S1 S2 ->
    store_eq (fst (space_remove_continuation S1 g w)) (fst (space_remove_continuation S2 g w)) /\
    snd (space_remove_continuation S1 g w) = snd (space_remove_continuation S2 g w).
Proof.
  intros S1 S2 g w E. pose proof E as (Hd & Hc & Hj & Hn).
  unfold space_remove_continuation. rewrite Hc.
  destruct (in_dec wk_eq_dec w (s_conts S2 g)); simpl; split; try reflexivity;
    [solve_upd_cong | exact E].
Qed.

Lemma put_join_cong :
  forall S1 S2 c g, store_eq S1 S2 ->
    store_eq (fst (put_join S1 c g)) (fst (put_join S2 c g)) /\
    snd (put_join S1 c g) = snd (put_join S2 c g).
Proof.
  intros S1 S2 c g E. pose proof E as (Hd & Hc & Hj & Hn).
  unfold put_join. rewrite Hj.
  destruct (in_dec lc_eq_dec g (s_joins S2 c)); simpl; split; try reflexivity;
    [exact E | solve_upd_cong].
Qed.

Lemma remove_join_cong :
  forall S1 S2 c g, store_eq S1 S2 ->
    store_eq (fst (remove_join S1 c g)) (fst (remove_join S2 c g)) /\
    snd (remove_join S1 c g) = snd (remove_join S2 c g).
Proof.
  intros S1 S2 c g E. pose proof E as (Hd & Hc & Hj & Hn).
  unfold remove_join. rewrite Hc, Hj.
  destruct (s_conts S2 g); [| simpl; split; [exact E | reflexivity]].
  destruct (in_dec lc_eq_dec g (s_joins S2 c)); simpl; split; try reflexivity;
    [solve_upd_cong | exact E].
Qed.

Lemma put_joins_cong :
  forall cs S1 S2 g, store_eq S1 S2 ->
    store_eq (fst (put_joins S1 cs g)) (fst (put_joins S2 cs g)) /\
    snd (put_joins S1 cs g) = snd (put_joins S2 cs g).
Proof.
  induction cs as [| c rest IH]; intros S1 S2 g E; simpl; [split; [exact E | reflexivity] |].
  destruct (put_join_cong S1 S2 c g E) as [E1 F1].
  destruct (IH _ _ g E1) as [E2 F2].
  split; [exact E2 | rewrite F1, F2; reflexivity].
Qed.

Lemma store_waiting_continuation_cong :
  forall S1 S2 chs w, store_eq S1 S2 ->
    store_eq (fst (store_waiting_continuation S1 chs w)) (fst (store_waiting_continuation S2 chs w)) /\
    snd (store_waiting_continuation S1 chs w) = snd (store_waiting_continuation S2 chs w).
Proof.
  intros S1 S2 chs w E. unfold store_waiting_continuation. simpl.
  destruct (put_continuation_cong S1 S2 chs w E) as [E1 F1].
  destruct (put_joins_cong chs _ _ chs E1) as [E2 F2].
  split; [exact E2 | rewrite F1, F2; reflexivity].
Qed.

Lemma space_pf_loop_cong :
  forall cs S1 S2 chs, store_eq S1 S2 ->
    store_eq (fst (space_pf_loop S1 chs cs)) (fst (space_pf_loop S2 chs cs)) /\
    snd (space_pf_loop S1 chs cs) = snd (space_pf_loop S2 chs cs).
Proof.
  induction cs as [| cd rest IH]; intros S1 S2 chs E; simpl; [split; [exact E | reflexivity] |].
  assert (A : store_eq (fst (if removable cd then space_remove_datum S1 (cd_chan cd) (cd_rec cd) else (S1, [])))
                       (fst (if removable cd then space_remove_datum S2 (cd_chan cd) (cd_rec cd) else (S2, []))) /\
              snd (if removable cd then space_remove_datum S1 (cd_chan cd) (cd_rec cd) else (S1, [])) =
              snd (if removable cd then space_remove_datum S2 (cd_chan cd) (cd_rec cd) else (S2, []))).
  { destruct (removable cd); [apply space_remove_datum_cong, E | split; [exact E | reflexivity]]. }
  destruct A as [E1 F1].
  destruct (remove_join_cong _ _ (cd_chan cd) chs E1) as [E2 F2].
  destruct (IH _ _ chs E2) as [E3 F3].
  split; [exact E3 | rewrite F1, F2, F3; reflexivity].
Qed.

Lemma space_cf_loop_cong :
  forall cs S1 S2, store_eq S1 S2 ->
    store_eq (fst (space_cf_loop S1 cs)) (fst (space_cf_loop S2 cs)) /\
    snd (space_cf_loop S1 cs) = snd (space_cf_loop S2 cs).
Proof.
  induction cs as [| cd rest IH]; intros S1 S2 E; simpl; [split; [exact E | reflexivity] |].
  assert (A : store_eq (fst (if negb (d_persist (cd_rec cd)) then space_remove_datum S1 (cd_chan cd) (cd_rec cd) else (S1, [])))
                       (fst (if negb (d_persist (cd_rec cd)) then space_remove_datum S2 (cd_chan cd) (cd_rec cd) else (S2, []))) /\
              snd (if negb (d_persist (cd_rec cd)) then space_remove_datum S1 (cd_chan cd) (cd_rec cd) else (S1, [])) =
              snd (if negb (d_persist (cd_rec cd)) then space_remove_datum S2 (cd_chan cd) (cd_rec cd) else (S2, []))).
  { destruct (negb (d_persist (cd_rec cd))); [apply space_remove_datum_cong, E | split; [exact E | reflexivity]]. }
  destruct A as [E1 F1].
  destruct (IH _ _ E1) as [E2 F2].
  split; [exact E2 | rewrite F1, F2; reflexivity].
Qed.

(* ════════════════ Overlay refinement (the staged view, design §1.3) ═════════ *)

Lemma vdata_set : forall V c l c', vdata (vset_data V c l) c' = if chan_eq_dec c' c then l else vdata V c'.
Proof. intros. unfold vdata, vset_data. simpl. destruct (chan_eq_dec c' c); reflexivity. Qed.
Lemma vconts_set : forall V g l g', vconts (vset_conts V g l) g' = if lc_eq_dec g' g then l else vconts V g'.
Proof. intros. unfold vconts, vset_conts. simpl. destruct (lc_eq_dec g' g); reflexivity. Qed.
Lemma vjoins_set : forall V c l c', vjoins (vset_joins V c l) c' = if chan_eq_dec c' c then l else vjoins V c'.
Proof. intros. unfold vjoins, vset_joins. simpl. destruct (chan_eq_dec c' c); reflexivity. Qed.
Lemma vdelta_set : forall V p n p', vdelta (vset_delta V p n) p' = if pid_eq_dec p' p then n else vdelta V p'.
Proof. intros. unfold vdelta, vset_delta. simpl. destruct (pid_eq_dec p' p); reflexivity. Qed.

Lemma vdata_other : forall V g l c, vdata (vset_conts V g l) c = vdata V c.
Proof. reflexivity. Qed.
Lemma vdata_other2 : forall V c' l c, vdata (vset_joins V c' l) c = vdata V c.
Proof. reflexivity. Qed.
Lemma vdata_other3 : forall V p n c, vdata (vset_delta V p n) c = vdata V c.
Proof. reflexivity. Qed.

Lemma view_set_data :
  forall V c l, store_eq (view_store (vset_data V c l)) (upd_data (view_store V) c l).
Proof.
  intros V c l. unfold view_store, upd_data. repeat split; intros; simpl; try reflexivity.
  apply vdata_set.
Qed.
Lemma view_set_conts :
  forall V g l, store_eq (view_store (vset_conts V g l)) (upd_conts (view_store V) g l).
Proof.
  intros V g l. unfold view_store, upd_conts. repeat split; intros; simpl; try reflexivity.
  apply vconts_set.
Qed.
Lemma view_set_joins :
  forall V c l, store_eq (view_store (vset_joins V c l)) (upd_joins (view_store V) c l).
Proof.
  intros V c l. unfold view_store, upd_joins. repeat split; intros; simpl; try reflexivity.
  apply vjoins_set.
Qed.

Lemma view_bump : forall V src p, store_eq (view_store (vbump V src p)) (bump (view_store V) src p).
Proof.
  intros V src p. unfold vbump, bump, upd_count, incr.
  destruct p.
  - repeat split; intros; reflexivity.
  - repeat split; intros; try reflexivity. cbn.
    unfold vcount, vdelta, vset_delta. cbn. unfold pid_eqb.
    destruct (pid_eq_dec p src) as [E | N]; cbn; [subst; lia | reflexivity].
Qed.

Lemma view_put_datum :
  forall V c d, store_eq (view_store (vput_datum V c d)) (fst (put_datum (view_store V) c d)).
Proof. intros V c d. unfold vput_datum, put_datum. simpl. apply view_set_data. Qed.

Lemma view_remove_datum :
  forall V c d, store_eq (view_store (vremove_datum V c d)) (fst (space_remove_datum (view_store V) c d)).
Proof.
  intros V c d. unfold vremove_datum, space_remove_datum. simpl.
  destruct (in_dec datum_eq_dec d (vdata V c)) as [Hin | Hn]; simpl.
  - apply view_set_data.
  - eapply store_eq_trans; [apply view_set_data |].
    rewrite remove_first_not_in by exact Hn.
    unfold upd_data, view_store. repeat split; intros; simpl; try reflexivity.
    destruct (chan_eq_dec c0 c); subst; reflexivity.
Qed.

Lemma view_put_continuation :
  forall V g w, store_eq (view_store (vput_continuation V g w)) (fst (put_continuation (view_store V) g w)).
Proof.
  intros V g w. unfold vput_continuation, put_continuation. simpl.
  destruct (existsb (ident_eqb w) (vconts V g)); simpl; [apply store_eq_refl | apply view_set_conts].
Qed.

Lemma view_remove_continuation :
  forall V g w, store_eq (view_store (vremove_continuation V g w)) (fst (space_remove_continuation (view_store V) g w)).
Proof.
  intros V g w. unfold vremove_continuation, space_remove_continuation. simpl.
  destruct (in_dec wk_eq_dec w (vconts V g)) as [Hin | Hn]; simpl.
  - apply view_set_conts.
  - eapply store_eq_trans; [apply view_set_conts |].
    rewrite remove_first_not_in by exact Hn.
    unfold upd_conts, view_store. repeat split; intros; simpl; try reflexivity.
    destruct (lc_eq_dec g0 g); subst; reflexivity.
Qed.

Lemma view_put_join :
  forall V c g, store_eq (view_store (vput_join V c g)) (fst (put_join (view_store V) c g)).
Proof.
  intros V c g. unfold vput_join, put_join. simpl.
  destruct (in_dec lc_eq_dec g (vjoins V c)); simpl; [apply store_eq_refl | apply view_set_joins].
Qed.

Lemma view_remove_join :
  forall V c g, store_eq (view_store (vremove_join V c g)) (fst (remove_join (view_store V) c g)).
Proof.
  intros V c g. unfold vremove_join, remove_join. simpl.
  destruct (vconts V g) eqn:Ec; simpl; [| apply store_eq_refl].
  destruct (in_dec lc_eq_dec g (vjoins V c)) as [Hin | Hn]; simpl.
  - apply view_set_joins.
  - eapply store_eq_trans; [apply view_set_joins |].
    rewrite remove_first_not_in by exact Hn.
    unfold upd_joins, view_store. repeat split; intros; simpl; try reflexivity.
    destruct (chan_eq_dec c0 c); subst; reflexivity.
Qed.

(* A view operation on a view that is store_eq to a store. *)
Lemma view_store_cong_data : forall V S, store_eq (view_store V) S -> forall c, vdata V c = s_data S c.
Proof. intros V S (Hd & _) c. exact (Hd c). Qed.

Lemma view_put_joins :
  forall cs V S g, store_eq (view_store V) S ->
    store_eq (view_store (vput_joins V cs g)) (fst (put_joins S cs g)).
Proof.
  induction cs as [| c rest IH]; intros V S g E; simpl; [exact E |].
  apply IH.
  eapply store_eq_trans; [apply view_put_join |].
  apply put_join_cong. exact E.
Qed.

Lemma view_store_wk :
  forall V S chs w, store_eq (view_store V) S ->
    store_eq (view_store (vstore_wk V chs w)) (fst (store_waiting_continuation S chs w)).
Proof.
  intros V S chs w E. unfold vstore_wk, store_waiting_continuation. simpl.
  apply view_put_joins.
  eapply store_eq_trans; [apply view_put_continuation |].
  apply put_continuation_cong. exact E.
Qed.

Lemma view_pf_loop :
  forall cs V S chs, store_eq (view_store V) S ->
    store_eq (view_store (vpf_loop V chs cs)) (fst (space_pf_loop S chs cs)).
Proof.
  induction cs as [| cd rest IH]; intros V S chs E; simpl; [exact E |].
  apply IH.
  eapply store_eq_trans; [apply view_remove_join |].
  apply remove_join_cong.
  destruct (removable cd); simpl; [| exact E].
  eapply store_eq_trans; [apply view_remove_datum |].
  apply space_remove_datum_cong. exact E.
Qed.

Lemma view_cf_loop :
  forall cs V S, store_eq (view_store V) S ->
    store_eq (view_store (vcf_loop V cs)) (fst (space_cf_loop S cs)).
Proof.
  induction cs as [| cd rest IH]; intros V S E; simpl; [exact E |].
  apply IH.
  destruct (negb (d_persist (cd_rec cd))); simpl; [| exact E].
  eapply store_eq_trans; [apply view_remove_datum |].
  apply space_remove_datum_cong. exact E.
Qed.

(* ═══════════════════ Closed forms of the removal loops ════════════════════ *)

Lemma perm_filter :
  forall (A : Type) (f : A -> bool) (l l' : list A),
    Permutation l l' -> Permutation (filter f l) (filter f l').
Proof.
  intros A f l l' P. induction P; simpl.
  - constructor.
  - destruct (f x); [constructor |]; exact IHP.
  - destruct (f x), (f y); try constructor; try reflexivity.
  - eapply perm_trans; eassumption.
Qed.

Lemma np_recs_perm :
  forall c cs cs' x, Permutation cs cs' ->
    count_occ datum_eq_dec (np_recs c cs) x = count_occ datum_eq_dec (np_recs c cs') x.
Proof.
  intros c cs cs' x P. apply Permutation_count_occ.
  unfold np_recs. apply Permutation_map, perm_filter, P.
Qed.

Lemma chan_count_perm :
  forall c cs cs', Permutation cs cs' ->
    count_occ chan_eq_dec (map cd_chan cs) c = count_occ chan_eq_dec (map cd_chan cs') c.
Proof.
  intros c cs cs' P. apply Permutation_count_occ. apply Permutation_map, P.
Qed.

Lemma np_recs_cons :
  forall c cd rest x,
    count_occ datum_eq_dec (np_recs c (cd :: rest)) x =
    (if removable cd && chan_eqb (cd_chan cd) c
     then (if datum_eq_dec (cd_rec cd) x then 1 else 0) else 0)
    + count_occ datum_eq_dec (np_recs c rest) x.
Proof.
  intros c cd rest x. unfold np_recs. simpl.
  destruct (removable cd && chan_eqb (cd_chan cd) c); simpl; [| reflexivity].
  destruct (datum_eq_dec (cd_rec cd) x); reflexivity.
Qed.

Definition rj_closed (S : Store) (chs : list Chan) (cs : list Cand) (c : Chan)
    : list (list Chan) :=
  match s_conts S chs with
  | [] => rm_iter lc_eq_dec (count_occ chan_eq_dec (map cd_chan cs) c) chs (s_joins S c)
  | _ :: _ => s_joins S c
  end.

Lemma rj_closed_perm :
  forall S chs cs cs' c, Permutation cs cs' -> rj_closed S chs cs c = rj_closed S chs cs' c.
Proof.
  intros S chs cs cs' c P. unfold rj_closed.
  rewrite (chan_count_perm c cs cs' P). reflexivity.
Qed.

(* Field behaviour of the primitives. *)

Lemma remove_join_fields :
  forall S c g,
    (forall c', s_data (fst (remove_join S c g)) c' = s_data S c') /\
    (forall g', s_conts (fst (remove_join S c g)) g' = s_conts S g') /\
    (forall c', s_joins (fst (remove_join S c g)) c' =
       if chan_eq_dec c' c
       then match s_conts S g with
            | [] => remove_first lc_eq_dec g (s_joins S c)
            | _ :: _ => s_joins S c
            end
       else s_joins S c') /\
    (forall p, s_count (fst (remove_join S c g)) p = s_count S p).
Proof.
  intros S c g. unfold remove_join.
  destruct (s_conts S g) as [| w ws] eqn:Ec.
  - destruct (in_dec lc_eq_dec g (s_joins S c)) as [Hin | Hn]; simpl;
      repeat split; intros; try reflexivity.
    destruct (chan_eq_dec c' c) as [E | N]; [| reflexivity].
    subst c'. rewrite remove_first_not_in by exact Hn. reflexivity.
  - simpl. repeat split; intros; try reflexivity.
    destruct (chan_eq_dec c' c); [subst |]; reflexivity.
Qed.

Lemma space_remove_datum_fields :
  forall S c d,
    (forall c' x, count_occ datum_eq_dec (s_data (fst (space_remove_datum S c d)) c') x =
       count_occ datum_eq_dec (s_data S c') x
       - (if chan_eq_dec c' c then (if datum_eq_dec d x then 1 else 0) else 0)) /\
    (forall g', s_conts (fst (space_remove_datum S c d)) g' = s_conts S g') /\
    (forall c', s_joins (fst (space_remove_datum S c d)) c' = s_joins S c') /\
    (forall p, s_count (fst (space_remove_datum S c d)) p = s_count S p).
Proof.
  intros S c d. unfold space_remove_datum.
  destruct (in_dec datum_eq_dec d (s_data S c)) as [Hin | Hn]; simpl;
    repeat split; intros; try reflexivity.
  - destruct (chan_eq_dec c' c) as [E | N]; [subst | lia].
    apply count_occ_remove_first.
  - destruct (chan_eq_dec c' c) as [E | N]; [subst | lia].
    destruct (datum_eq_dec d x) as [Edx | Ndx]; [subst | lia].
    apply (count_occ_not_In datum_eq_dec) in Hn. rewrite Hn. reflexivity.
Qed.

(* Decision 45 (Rule E): the loops have no retain flag, so the closed forms
   always subtract the removed records. *)
Lemma space_pf_loop_spec :
  forall cs S chs,
    let S' := fst (space_pf_loop S chs cs) in
    (forall c x, count_occ datum_eq_dec (s_data S' c) x =
       count_occ datum_eq_dec (s_data S c) x - count_occ datum_eq_dec (np_recs c cs) x) /\
    (forall g, s_conts S' g = s_conts S g) /\
    (forall c, s_joins S' c = rj_closed S chs cs c) /\
    (forall p, s_count S' p = s_count S p).
Proof.
  induction cs as [| cd rest IH]; intros S chs; simpl.
  - repeat split; intros; try reflexivity.
    + simpl. lia.
    + unfold rj_closed. destruct (s_conts S chs); reflexivity.
  - set (p1 := if removable cd then space_remove_datum S (cd_chan cd) (cd_rec cd) else (S, [])).
    set (S1 := fst p1).
    set (S2 := fst (remove_join S1 (cd_chan cd) chs)).
    destruct (IH S2 chs) as (Id & Ic & Ij & In_).
    assert (F1 : (forall c x, count_occ datum_eq_dec (s_data S1 c) x =
                   count_occ datum_eq_dec (s_data S c) x
                   - (if removable cd
                      then (if chan_eq_dec c (cd_chan cd)
                            then (if datum_eq_dec (cd_rec cd) x then 1 else 0) else 0) else 0)) /\
                 (forall g, s_conts S1 g = s_conts S g) /\
                 (forall c, s_joins S1 c = s_joins S c) /\
                 (forall p, s_count S1 p = s_count S p)).
    { subst S1 p1. destruct (removable cd).
      - apply space_remove_datum_fields.
      - simpl. repeat split; intros; lia || reflexivity. }
    destruct F1 as (F1d & F1c & F1j & F1n).
    destruct (remove_join_fields S1 (cd_chan cd) chs) as (F2d & F2c & F2j & F2n).
    fold S2 in F2d, F2c, F2j, F2n.
    repeat split; intros.
    + rewrite Id, F2d, F1d, np_recs_cons.
      destruct (removable cd) eqn:Er; simpl; [| lia].
      unfold chan_eqb. destruct (chan_eq_dec c (cd_chan cd)) as [E | N];
        destruct (chan_eq_dec (cd_chan cd) c) as [E' | N']; try congruence; lia.
    + rewrite Ic, F2c, F1c. reflexivity.
    + rewrite Ij. unfold rj_closed. rewrite F2c, F1c.
      destruct (s_conts S chs) as [| w ws] eqn:Ec.
      * rewrite F2j, F1c, Ec, !F1j. simpl.
        destruct (chan_eq_dec c (cd_chan cd)) as [E | N].
        -- subst c. destruct (chan_eq_dec (cd_chan cd) (cd_chan cd)) as [_ | C]; [| contradiction].
           symmetry. apply rm_iter_S.
        -- destruct (chan_eq_dec (cd_chan cd) c) as [E' | _]; [congruence |].
           reflexivity.
      * rewrite F2j, F1c, Ec, !F1j. destruct (chan_eq_dec c (cd_chan cd)); [subst |]; reflexivity.
    + rewrite In_, F2n, F1n. reflexivity.
Qed.

Lemma space_cf_loop_spec :
  forall cs S,
    (forall cd, In cd cs -> cd_incoming cd = false) ->
    let S' := fst (space_cf_loop S cs) in
    (forall c x, count_occ datum_eq_dec (s_data S' c) x =
       count_occ datum_eq_dec (s_data S c) x - count_occ datum_eq_dec (np_recs c cs) x) /\
    (forall g, s_conts S' g = s_conts S g) /\
    (forall c, s_joins S' c = s_joins S c) /\
    (forall p, s_count S' p = s_count S p).
Proof.
  induction cs as [| cd rest IH]; intros S Hinc; simpl.
  - repeat split; intros; try reflexivity. simpl. lia.
  - assert (Hcd : cd_incoming cd = false) by (apply Hinc; left; reflexivity).
    assert (Hr : forall cd', In cd' rest -> cd_incoming cd' = false) by (intros; apply Hinc; right; assumption).
    set (p1 := if negb (d_persist (cd_rec cd)) then space_remove_datum S (cd_chan cd) (cd_rec cd) else (S, [])).
    destruct (IH (fst p1) Hr) as (Id & Ic & Ij & In_).
    assert (F1 : (forall c x, count_occ datum_eq_dec (s_data (fst p1) c) x =
                   count_occ datum_eq_dec (s_data S c) x
                   - (if negb (d_persist (cd_rec cd))
                      then (if chan_eq_dec c (cd_chan cd)
                            then (if datum_eq_dec (cd_rec cd) x then 1 else 0) else 0) else 0)) /\
                 (forall g, s_conts (fst p1) g = s_conts S g) /\
                 (forall c, s_joins (fst p1) c = s_joins S c) /\
                 (forall p, s_count (fst p1) p = s_count S p)).
    { subst p1. destruct (negb (d_persist (cd_rec cd))).
      - apply space_remove_datum_fields.
      - simpl. repeat split; intros; lia || reflexivity. }
    destruct F1 as (F1d & F1c & F1j & F1n).
    repeat split; intros.
    + rewrite Id, F1d, np_recs_cons.
      unfold removable. rewrite Hcd. simpl.
      destruct (d_persist (cd_rec cd)); simpl; [lia |].
      unfold chan_eqb. destruct (chan_eq_dec c (cd_chan cd)) as [E | N];
        destruct (chan_eq_dec (cd_chan cd) c) as [E' | N']; try congruence; lia.
    + rewrite Ic, F1c. reflexivity.
    + rewrite Ij, F1j. reflexivity.
    + rewrite In_, F1n. reflexivity.
Qed.

Definition hpos (cd : Cand) : Chan * nat := (cd_chan cd, cd_hidx cd).

(* The indices of the host matcher are valid. Each removable candidate
   points at its record, and no two removable candidates share a
   position. *)
Definition host_rm_valid (S : Store) (cs : list Cand) : Prop :=
  (forall cd, In cd cs -> removable cd = true ->
     nth_error (s_data S (cd_chan cd)) (cd_hidx cd) = Some (cd_rec cd)) /\
  NoDup (map hpos (filter removable cs)).

Lemma host_rm_valid_perm :
  forall S cs cs', Permutation cs cs' -> host_rm_valid S cs -> host_rm_valid S cs'.
Proof.
  intros S cs cs' P [Hv Hnd]. split.
  - intros cd Hin Hr. apply Hv; [| exact Hr]. eapply Permutation_in; [apply Permutation_sym, P | exact Hin].
  - eapply Permutation_NoDup; [| exact Hnd]. apply Permutation_map, perm_filter, P.
Qed.

Lemma host_remove_datum_some :
  forall S c i d, nth_error (s_data S c) i = Some d ->
    host_remove_datum S c i = Some (upd_data S c (remove_nth i (s_data S c)), [FxRemoveDatum c d]).
Proof. intros S c i d H. unfold host_remove_datum. rewrite H. reflexivity. Qed.

Lemma host_rm_loop_spec :
  forall cs S chs,
    StronglySorted (desc_rel hkey) cs ->
    host_rm_valid S cs ->
    let S' := fst (host_rm_loop S chs cs) in
    (forall c x, count_occ datum_eq_dec (s_data S' c) x
                 + count_occ datum_eq_dec (np_recs c cs) x
                 = count_occ datum_eq_dec (s_data S c) x) /\
    (forall g, s_conts S' g = s_conts S g) /\
    (forall c, s_joins S' c = rj_closed S chs cs c) /\
    (forall p, s_count S' p = s_count S p).
Proof.
  induction cs as [| cd rest IH]; intros S chs Hs [Hv Hnd]; simpl.
  - repeat split; intros; try reflexivity.
    + simpl. lia.
    + unfold rj_closed. destruct (s_conts S chs); reflexivity.
  - apply StronglySorted_inv in Hs as [Hsr Hfa].
    rewrite Forall_forall in Hfa.
    destruct (removable cd) eqn:Er.
    + (* removable: the index is valid *)
      assert (Hn0 : nth_error (s_data S (cd_chan cd)) (cd_hidx cd) = Some (cd_rec cd))
        by (apply Hv; [left; reflexivity | exact Er]).
      rewrite (host_remove_datum_some S (cd_chan cd) (cd_hidx cd) (cd_rec cd) Hn0). simpl.
      set (S0 := upd_data S (cd_chan cd) (remove_nth (cd_hidx cd) (s_data S (cd_chan cd)))).
      set (S1 := fst (remove_join S0 (cd_chan cd) chs)).
      simpl in Hnd. rewrite Er in Hnd. simpl in Hnd.
      apply NoDup_cons_iff in Hnd as [Hni Hndr].
      assert (Hvr : host_rm_valid S1 rest).
      { split; [| exact Hndr].
        intros cd' Hin' Hr'.
        destruct (remove_join_fields S0 (cd_chan cd) chs) as (Rd & _).
        fold S1 in Rd. rewrite Rd. subst S0. unfold upd_data. simpl.
        destruct (chan_eq_dec (cd_chan cd') (cd_chan cd)) as [Ech | Nch].
        - rewrite nth_error_remove_nth_lt.
          + rewrite <- Ech. apply Hv; [right; exact Hin' | exact Hr'].
          + assert (Hle := Hfa cd' Hin'). unfold desc_rel, hkey in Hle.
            assert (Hneq : cd_hidx cd' <> cd_hidx cd).
            { intro Eq. apply Hni. apply in_map_iff. exists cd'.
              split; [unfold hpos; rewrite Ech, Eq; reflexivity |].
              apply filter_In. split; [exact Hin' | exact Hr']. }
            unfold removable in Er, Hr'.
            destruct (cd_incoming cd); [discriminate |].
            destruct (cd_incoming cd'); [discriminate |].
            lia.
        - apply Hv; [right; exact Hin' | exact Hr']. }
      destruct (IH S1 chs Hsr Hvr) as (Id & Ic & Ij & In_).
      destruct (remove_join_fields S0 (cd_chan cd) chs) as (Rd & Rc & Rj & Rn).
      fold S1 in Rd, Rc, Rj, Rn.
      repeat split; intros.
      * rewrite np_recs_cons, Er. simpl.
        specialize (Id c x). rewrite Rd in Id. subst S0. unfold upd_data in Id. simpl in Id.
        unfold chan_eqb.
        destruct (chan_eq_dec (cd_chan cd) c) as [E | N].
        -- subst c. destruct (chan_eq_dec (cd_chan cd) (cd_chan cd)) as [_ | C]; [| contradiction].
           pose proof (count_occ_remove_nth Datum datum_eq_dec (s_data S (cd_chan cd)) (cd_hidx cd) (cd_rec cd) x Hn0) as Hc.
           lia.
        -- destruct (chan_eq_dec c (cd_chan cd)) as [E' | _]; [congruence |]. lia.
      * rewrite Ic, Rc. reflexivity.
      * rewrite Ij. unfold rj_closed. rewrite Rc. subst S0. unfold upd_data. simpl.
        destruct (s_conts S chs) as [| w ws] eqn:Ec.
        -- rewrite Rj. simpl. rewrite Ec.
           destruct (chan_eq_dec c (cd_chan cd)) as [E | N].
           ++ subst c. destruct (chan_eq_dec (cd_chan cd) (cd_chan cd)) as [_ | C]; [| contradiction].
              symmetry. apply rm_iter_S.
           ++ destruct (chan_eq_dec (cd_chan cd) c) as [E' | _]; [congruence |]. reflexivity.
        -- rewrite Rj. simpl. rewrite Ec.
           destruct (chan_eq_dec c (cd_chan cd)); [subst |]; reflexivity.
      * rewrite In_, Rn. reflexivity.
    + (* not removable: only remove_join *)
      set (S1 := fst (remove_join S (cd_chan cd) chs)).
      assert (Hvr : host_rm_valid S1 rest).
      { split.
        - intros cd' Hin' Hr'.
          destruct (remove_join_fields S (cd_chan cd) chs) as (Rd & _).
          fold S1 in Rd. rewrite Rd. apply Hv; [right; exact Hin' | exact Hr'].
        - simpl in Hnd. rewrite Er in Hnd. exact Hnd. }
      destruct (IH S1 chs Hsr Hvr) as (Id & Ic & Ij & In_).
      destruct (remove_join_fields S (cd_chan cd) chs) as (Rd & Rc & Rj & Rn).
      fold S1 in Rd, Rc, Rj, Rn.
      repeat split; intros.
      * rewrite np_recs_cons, Er. simpl. rewrite <- Rd. exact (Id c x).
      * rewrite Ic, Rc. reflexivity.
      * rewrite Ij. unfold rj_closed. rewrite Rc.
        destruct (s_conts S chs) as [| w ws] eqn:Ec.
        -- rewrite Rj; try rewrite Ec.
           destruct (chan_eq_dec c (cd_chan cd)) as [E | N].
           ++ subst c. simpl. destruct (chan_eq_dec (cd_chan cd) (cd_chan cd)) as [_ | C]; [| contradiction].
              symmetry. apply rm_iter_S.
           ++ simpl. destruct (chan_eq_dec (cd_chan cd) c) as [E' | _]; [congruence |]. reflexivity.
        -- rewrite Rj; try rewrite Ec. destruct (chan_eq_dec c (cd_chan cd)); [subst |]; reflexivity.
      * rewrite In_, Rn. reflexivity.
Qed.

Lemma host_spd_loop_spec :
  forall cs S,
    StronglySorted (desc_rel hkey) cs ->
    (forall cd, In cd cs -> cd_incoming cd = false) ->
    host_rm_valid S cs ->
    let S' := fst (host_spd_loop S cs) in
    (forall c x, count_occ datum_eq_dec (s_data S' c) x
                 + count_occ datum_eq_dec (np_recs c cs) x
                 = count_occ datum_eq_dec (s_data S c) x) /\
    (forall g, s_conts S' g = s_conts S g) /\
    (forall c, s_joins S' c = s_joins S c) /\
    (forall p, s_count S' p = s_count S p).
Proof.
  induction cs as [| cd rest IH]; intros S Hs Hinc [Hv Hnd]; simpl.
  - repeat split; intros; try reflexivity. simpl. lia.
  - apply StronglySorted_inv in Hs as [Hsr Hfa].
    rewrite Forall_forall in Hfa.
    assert (Hcd : cd_incoming cd = false) by (apply Hinc; left; reflexivity).
    assert (Hr : forall cd', In cd' rest -> cd_incoming cd' = false) by (intros; apply Hinc; right; assumption).
    destruct (d_persist (cd_rec cd)) eqn:Ep; simpl.
    + assert (Er : removable cd = false) by (unfold removable; rewrite Ep; apply andb_false_r).
      assert (Hvr : host_rm_valid S rest).
      { split.
        - intros cd' Hin' Hr'. apply Hv; [right; exact Hin' | exact Hr'].
        - simpl in Hnd. rewrite Er in Hnd. exact Hnd. }
      destruct (IH S Hsr Hr Hvr) as (Id & Ic & Ij & In_).
      repeat split; intros; auto.
      rewrite np_recs_cons, Er. simpl. exact (Id c x).
    + assert (Er : removable cd = true) by (unfold removable; rewrite Ep, Hcd; reflexivity).
      assert (Hn0 : nth_error (s_data S (cd_chan cd)) (cd_hidx cd) = Some (cd_rec cd))
        by (apply Hv; [left; reflexivity | exact Er]).
      rewrite (host_remove_datum_some S (cd_chan cd) (cd_hidx cd) (cd_rec cd) Hn0). simpl.
      set (S0 := upd_data S (cd_chan cd) (remove_nth (cd_hidx cd) (s_data S (cd_chan cd)))).
      simpl in Hnd. rewrite Er in Hnd. simpl in Hnd.
      apply NoDup_cons_iff in Hnd as [Hni Hndr].
      assert (Hvr : host_rm_valid S0 rest).
      { split; [| exact Hndr].
        intros cd' Hin' Hr'. subst S0. unfold upd_data. simpl.
        destruct (chan_eq_dec (cd_chan cd') (cd_chan cd)) as [Ech | Nch].
        - rewrite nth_error_remove_nth_lt.
          + rewrite <- Ech. apply Hv; [right; exact Hin' | exact Hr'].
          + assert (Hle := Hfa cd' Hin'). unfold desc_rel, hkey in Hle.
            rewrite Hcd, (Hr cd' Hin') in Hle.
            assert (Hneq : cd_hidx cd' <> cd_hidx cd).
            { intro Eq. apply Hni. apply in_map_iff. exists cd'.
              split; [unfold hpos; rewrite Ech, Eq; reflexivity |].
              apply filter_In. split; [exact Hin' | exact Hr']. }
            lia.
        - apply Hv; [right; exact Hin' | exact Hr']. }
      destruct (IH S0 Hsr Hr Hvr) as (Id & Ic & Ij & In_).
      repeat split; intros.
      * rewrite np_recs_cons, Er. simpl.
        specialize (Id c x).
        assert (HS0 : s_data S0 c = if chan_eq_dec c (cd_chan cd)
                                    then remove_nth (cd_hidx cd) (s_data S (cd_chan cd))
                                    else s_data S c) by reflexivity.
        rewrite HS0 in Id.
        unfold chan_eqb.
        destruct (chan_eq_dec (cd_chan cd) c) as [E | N].
        -- subst c. destruct (chan_eq_dec (cd_chan cd) (cd_chan cd)) as [_ | C]; [| contradiction].
           pose proof (count_occ_remove_nth Datum datum_eq_dec (s_data S (cd_chan cd)) (cd_hidx cd) (cd_rec cd) x Hn0) as Hc.
           lia.
        -- destruct (chan_eq_dec c (cd_chan cd)) as [E' | _]; [congruence |]. lia.
      * rewrite Ic. reflexivity.
      * rewrite Ij. reflexivity.
      * rewrite In_. reflexivity.
Qed.

(* ═══════════════ Well-formedness: joins lockstep (H7) ══════════════════════ *)

(* A join group is registered at a channel exactly when the group contains
   the channel and has at least one waiting continuation. *)
Definition lockstep (S : Store) : Prop :=
  forall c g, In g (s_joins S c) <-> (In c g /\ s_conts S g <> []).

Definition joins_nodup (S : Store) : Prop := forall c, NoDup (s_joins S c).

Definition conts_nodup (S : Store) : Prop := forall g, NoDup (map wk_ident (s_conts S g)).

Definition WF (S : Store) : Prop := lockstep S /\ joins_nodup S /\ conts_nodup S.

Lemma WF_store_eq : forall S1 S2, store_eq S1 S2 -> WF S1 -> WF S2.
Proof.
  intros S1 S2 (Hd & Hc & Hj & Hn) (L & J & C). unfold lockstep, joins_nodup, conts_nodup in *. split; [| split].
  - intros c g. rewrite <- Hj, <- Hc. apply L.
  - intros c. rewrite <- Hj. apply J.
  - intros g. rewrite <- Hc. apply C.
Qed.

Lemma WF_conts_nodup : forall S g, WF S -> NoDup (s_conts S g).
Proof. intros S g (_ & _ & C). eapply NoDup_map_inv. apply C. Qed.

Lemma nodup_map_remove_first :
  forall (A B : Type) (eqd : forall x y : A, {x = y} + {x <> y}) (f : A -> B) x l,
    NoDup (map f l) -> NoDup (map f (remove_first eqd x l)).
Proof.
  intros A B eqd f x l. induction l as [| h t IH]; simpl; intros Hnd; auto.
  inversion Hnd as [| h' t' Hni Hndt]; subst.
  destruct (eqd x h); [exact Hndt |].
  simpl. constructor.
  - intro Hin. apply Hni. apply in_map_iff in Hin as (y & Ey & Hy).
    apply in_map_iff. exists y. split; [exact Ey | eapply remove_first_incl; exact Hy].
  - apply IH. exact Hndt.
Qed.

Lemma rm_iter_nodup :
  forall (A : Type) (eqd : forall x y : A, {x = y} + {x <> y}) n g l,
    NoDup l -> NoDup (rm_iter eqd n g l).
Proof.
  intros A eqd n. induction n as [| n IH]; intros g l Hnd; [exact Hnd |].
  rewrite rm_iter_S. apply IH. apply remove_first_nodup. exact Hnd.
Qed.

Lemma rm_iter_other :
  forall (A : Type) (eqd : forall x y : A, {x = y} + {x <> y}) n g l z,
    z <> g -> (In z (rm_iter eqd n g l) <-> In z l).
Proof.
  intros A eqd n. induction n as [| n IH]; intros g l z Hzg; [reflexivity |].
  rewrite rm_iter_S, IH by exact Hzg. split.
  - apply remove_first_incl.
  - apply remove_first_other_in. exact Hzg.
Qed.

Lemma rm_iter_gone :
  forall (A : Type) (eqd : forall x y : A, {x = y} + {x <> y}) n g l,
    NoDup l -> 0 < n -> ~ In g (rm_iter eqd n g l).
Proof.
  intros A eqd n g l Hnd Hn. destruct n as [| n]; [lia |].
  rewrite rm_iter_S.
  assert (Hnd' := remove_first_nodup A eqd g l Hnd).
  assert (Hg := remove_first_nodup_not_in A eqd g l Hnd).
  clear Hn. revert Hnd' Hg. generalize (remove_first eqd g l) as l'.
  induction n as [| n IH]; intros l' Hnd' Hg; [exact Hg |].
  rewrite rm_iter_S. apply IH.
  - apply remove_first_nodup. exact Hnd'.
  - intro Hin. apply Hg. eapply remove_first_incl. exact Hin.
Qed.

Lemma count_occ_map_chan_in :
  forall c cs, 0 < count_occ chan_eq_dec (map cd_chan cs) c <-> In c (map cd_chan cs).
Proof.
  intros c cs. rewrite (count_occ_In chan_eq_dec). lia.
Qed.

(* After the continuation removal, the closed form of the remove_join loop
   keeps the store well formed. The candidate channels must be exactly the
   channels of the group. *)
Lemma rj_closed_WF :
  forall S S' chs cs,
    WF S ->
    s_conts S chs <> [] ->
    (forall g, g <> chs -> s_conts S' g = s_conts S g) ->
    (forall c, s_joins S' c = rj_closed (mkStore (s_data S') (s_conts S') (s_joins S) (s_count S')) chs cs c) ->
    (forall g, NoDup (map wk_ident (s_conts S' g))) ->
    (forall c, In c chs <-> In c (map cd_chan cs)) ->
    WF S'.
Proof.
  intros S S' chs cs (L & J & C) Hne Hother Hjoins Hcn Hchan.
  unfold lockstep, joins_nodup, conts_nodup in *.
  unfold rj_closed in Hjoins. simpl in Hjoins.
  split; [| split].
  - intros c g.
    rewrite Hjoins.
    destruct (s_conts S' chs) as [| w ws] eqn:Ec.
    + (* the group became empty: its joins were removed *)
      destruct (lc_eq_dec g chs) as [Eg | Ng].
      * subst g. rewrite Ec. split.
        -- intros Hin. exfalso.
           destruct (in_dec chan_eq_dec c (map cd_chan cs)) as [Hc | Hc].
           ++ apply (rm_iter_gone _ lc_eq_dec (count_occ chan_eq_dec (map cd_chan cs) c) chs (s_joins S c));
                [apply J | apply count_occ_map_chan_in; exact Hc | exact Hin].
           ++ apply (count_occ_not_In chan_eq_dec) in Hc. rewrite Hc in Hin. simpl in Hin.
              apply L in Hin as [Hcin _]. apply Hchan in Hcin. apply (count_occ_not_In chan_eq_dec) in Hcin; [| exact Hc].
              contradiction.
        -- intros [_ C']. exfalso. apply C'. reflexivity.
      * rewrite (rm_iter_other _ lc_eq_dec _ chs _ g Ng). rewrite L. rewrite Hother by exact Ng. reflexivity.
    + (* the group is still non-empty: nothing changed *)
      destruct (lc_eq_dec g chs) as [Eg | Ng].
      * subst g. rewrite L, Ec. split; intros [Hc _]; split; try exact Hc; congruence.
      * rewrite L, Hother by exact Ng. reflexivity.
  - intros c. rewrite Hjoins. destruct (s_conts S' chs); [apply rm_iter_nodup, J | apply J].
  - exact Hcn.
Qed.

(* put_joins closed form. *)
Lemma put_joins_fields :
  forall cs S g,
    (forall c, s_data (fst (put_joins S cs g)) c = s_data S c) /\
    (forall g', s_conts (fst (put_joins S cs g)) g' = s_conts S g') /\
    (forall c, s_joins (fst (put_joins S cs g)) c =
       if in_dec chan_eq_dec c cs
       then (if in_dec lc_eq_dec g (s_joins S c) then s_joins S c else g :: s_joins S c)
       else s_joins S c) /\
    (forall p, s_count (fst (put_joins S cs g)) p = s_count S p).
Proof.
  induction cs as [| c0 rest IH]; intros S g; cbn [put_joins fst snd].
  - repeat split; intros; try reflexivity; destruct (in_dec chan_eq_dec c []) as [[] | _]; reflexivity.
  - set (S1 := fst (put_join S c0 g)).
    assert (F1 : (forall c, s_data S1 c = s_data S c) /\ (forall g', s_conts S1 g' = s_conts S g') /\
                 (forall c, s_joins S1 c = if chan_eq_dec c c0
                                          then (if in_dec lc_eq_dec g (s_joins S c0) then s_joins S c0 else g :: s_joins S c0)
                                          else s_joins S c) /\
                 (forall p, s_count S1 p = s_count S p)).
    { subst S1. unfold put_join. destruct (in_dec lc_eq_dec g (s_joins S c0)) as [Hin | Hn]; simpl;
        repeat split; intros; try reflexivity; destruct (chan_eq_dec c c0); subst; reflexivity. }
    destruct F1 as (F1d & F1c & F1j & F1n).
    destruct (IH S1 g) as (Id & Ic & Ij & In_).
    repeat split; intros.
    + rewrite Id. apply F1d.
    + rewrite Ic. apply F1c.
    + rewrite Ij, !F1j.
      destruct (chan_eq_dec c c0) as [E | N].
      * subst c0.
        destruct (in_dec chan_eq_dec c (c :: rest)) as [_ | Cn]; [| exfalso; apply Cn; left; reflexivity].
        destruct (in_dec lc_eq_dec g (s_joins S c)) as [Hin | Hn].
        -- destruct (in_dec chan_eq_dec c rest); [| reflexivity].
           destruct (in_dec lc_eq_dec g (s_joins S c)) as [_ | Cg]; [reflexivity | contradiction].
        -- destruct (in_dec chan_eq_dec c rest); [| reflexivity].
           destruct (in_dec lc_eq_dec g (g :: s_joins S c)) as [_ | Cg];
             [reflexivity | exfalso; apply Cg; left; reflexivity].
      * destruct (in_dec chan_eq_dec c rest) as [Hr | Hr];
          destruct (in_dec chan_eq_dec c (c0 :: rest)) as [Hr' | Hr'].
        -- reflexivity.
        -- exfalso. apply Hr'. right. exact Hr.
        -- exfalso. destruct Hr' as [E' | E']; [congruence | contradiction].
        -- reflexivity.
    + rewrite In_. apply F1n.
Qed.

Lemma put_continuation_fields :
  forall S g w,
    (forall c, s_data (fst (put_continuation S g w)) c = s_data S c) /\
    (forall g', s_conts (fst (put_continuation S g w)) g' =
       if lc_eq_dec g' g
       then (if existsb (ident_eqb w) (s_conts S g) then s_conts S g else w :: s_conts S g)
       else s_conts S g') /\
    (forall c, s_joins (fst (put_continuation S g w)) c = s_joins S c) /\
    (forall p, s_count (fst (put_continuation S g w)) p = s_count S p).
Proof.
  intros S g w. unfold put_continuation.
  destruct (existsb (ident_eqb w) (s_conts S g)) eqn:Ex; simpl; repeat split; intros; try reflexivity;
    destruct (lc_eq_dec g' g); subst; reflexivity.
Qed.

Lemma existsb_ident_nonempty :
  forall w l, existsb (ident_eqb w) l = true -> l <> [].
Proof. intros w l H E. subst. discriminate. Qed.

Lemma ident_nodup_insert :
  forall w l, existsb (ident_eqb w) l = false -> NoDup (map wk_ident l) ->
    NoDup (map wk_ident (w :: l)).
Proof.
  intros w l Hex Hnd. simpl. constructor; [| exact Hnd].
  intro Hin. apply in_map_iff in Hin as (y & Ey & Hy).
  assert (existsb (ident_eqb w) l = true).
  { apply existsb_exists. exists y. split; [exact Hy |].
    unfold ident_eqb. destruct (ident_eq_dec (wk_ident w) (wk_ident y)) as [_ | N]; [reflexivity |].
    exfalso. apply N. symmetry. exact Ey. }
  congruence.
Qed.

Lemma swc_WF :
  forall S chs w, WF S -> WF (fst (store_waiting_continuation S chs w)).
Proof.
  intros S chs w (L & J & C). unfold lockstep, joins_nodup, conts_nodup in *.
  unfold store_waiting_continuation. simpl.
  set (S1 := fst (put_continuation S chs w)).
  destruct (put_continuation_fields S chs w) as (Pd & Pc & Pj & Pn). fold S1 in Pd, Pc, Pj, Pn.
  destruct (put_joins_fields chs S1 chs) as (Qd & Qc & Qj & Qn).
  assert (Hne : s_conts S1 chs <> []).
  { rewrite Pc. destruct (lc_eq_dec chs chs) as [_ | Cn]; [| contradiction].
    destruct (existsb (ident_eqb w) (s_conts S chs)) eqn:Ex;
      [apply (existsb_ident_nonempty w); exact Ex | discriminate]. }
  split; [| split].
  - intros c g. rewrite Qj, Qc, !Pj.
    destruct (lc_eq_dec g chs) as [Eg | Ng].
    + subst g. destruct (in_dec chan_eq_dec c chs) as [Hc | Hc].
      * split; [intros _; split; [exact Hc | exact Hne] |].
        intros _. destruct (in_dec lc_eq_dec chs (s_joins S c)); [assumption | left; reflexivity].
      * rewrite L. split; intros [Hc' _]; contradiction.
    + assert (Ecg : s_conts S1 g = s_conts S g) by (rewrite Pc; destruct (lc_eq_dec g chs); [contradiction | reflexivity]).
      rewrite Ecg, <- L.
      destruct (in_dec chan_eq_dec c chs); [| reflexivity].
      destruct (in_dec lc_eq_dec chs (s_joins S c)); [reflexivity |].
      simpl. split; [intros [E | H]; [congruence | exact H] | intros H; right; exact H].
  - intros c. rewrite Qj, !Pj.
    destruct (in_dec chan_eq_dec c chs); [| apply J].
    destruct (in_dec lc_eq_dec chs (s_joins S c)) as [Hin | Hn]; [apply J |].
    constructor; [exact Hn | apply J].
  - intros g. rewrite Qc, Pc. destruct (lc_eq_dec g chs) as [Eg | Ng]; [subst | apply C].
    destruct (existsb (ident_eqb w) (s_conts S chs)) eqn:Ex; [apply C |].
    apply ident_nodup_insert; [exact Ex | apply C].
Qed.

(* Data-only and counter-only changes keep well-formedness. *)
Lemma WF_same_cj :
  forall S S', WF S -> (forall g, s_conts S' g = s_conts S g) -> (forall c, s_joins S' c = s_joins S c) -> WF S'.
Proof.
  intros S S' (L & J & C) Hc Hj. unfold lockstep, joins_nodup, conts_nodup in *. split; [| split].
  - intros c g. rewrite Hj, Hc. apply L.
  - intros c. rewrite Hj. apply J.
  - intros g. rewrite Hc. apply C.
Qed.

(* ═══════════════════ COMM construction and counters ══════════════════════ *)

Lemma pins_perm : forall x l, Permutation (pins x l) (x :: l).
Proof.
  intros x l. induction l as [| y t IH]; simpl; [reflexivity |].
  destruct (pid_leb x y); [reflexivity |].
  eapply perm_trans; [apply perm_skip, IH | apply perm_swap].
Qed.

Lemma comm_sort_perm : forall l, Permutation (comm_sort l) l.
Proof.
  induction l as [| x t IH]; simpl; [reflexivity |].
  eapply perm_trans; [apply pins_perm | apply perm_skip, IH].
Qed.

Lemma play_closure_ext :
  forall k1 k2 src persist ps, (forall p, k1 p = k2 p) ->
    play_counters_closure k1 src persist ps = play_counters_closure k2 src persist ps.
Proof.
  intros k1 k2 src persist ps E. unfold play_counters_closure.
  apply map_ext. intros p. rewrite E. reflexivity.
Qed.

Lemma counters_of_ext :
  forall k1 k2 ps, (forall p, k1 p = k2 p) -> counters_of k1 ps = counters_of k2 ps.
Proof.
  intros k1 k2 ps E. unfold counters_of. apply map_ext. intros p. rewrite E. reflexivity.
Qed.

(* The play closure on κ equals the replay closure on κ after the increment
   (design §1.4 rule 2, "counter_timing"). *)
Lemma play_closure_is_replay_closure :
  forall k src persist ps,
    play_counters_closure k src persist ps = counters_of (incr k src persist) ps.
Proof.
  intros k src persist ps. unfold play_counters_closure, counters_of, incr.
  apply map_ext_in. intros p Hp. f_equal.
  destruct persist; simpl; [lia |].
  unfold pid_eqb.
  destruct (pid_eq_dec p src) as [E | N]; simpl.
  - subst p. assert (Hex : existsb (fun q => if pid_eq_dec src q then true else false) ps = true).
    { apply existsb_exists. exists src. split; [exact Hp |]. destruct (pid_eq_dec src src); [reflexivity | contradiction]. }
    unfold pid_eqb in Hex |- *. rewrite Hex. simpl. lia.
  - rewrite andb_false_r. lia.
Qed.

Lemma mk_comm_play_ext :
  forall cands consume peeks k1 k2 src persist, (forall p, k1 p = k2 p) ->
    mk_comm cands consume peeks (play_counters_closure k1 src persist) =
    mk_comm cands consume peeks (play_counters_closure k2 src persist).
Proof.
  intros. unfold mk_comm. f_equal. apply play_closure_ext. assumption.
Qed.

Lemma mk_comm_counters_ext :
  forall cands consume peeks k1 k2, (forall p, k1 p = k2 p) ->
    mk_comm cands consume peeks (counters_of k1) = mk_comm cands consume peeks (counters_of k2).
Proof.
  intros. unfold mk_comm. f_equal. apply counters_of_ext. assumption.
Qed.

Lemma comm_eqb_refl : forall cm, comm_eqb cm cm = true.
Proof. intros cm. unfold comm_eqb. destruct (comm_eq_dec cm cm); [reflexivity | contradiction]. Qed.

(* ══════════════════════ Staging characterization ══════════════════════════ *)

(* The transaction after a staged fire. The proof does not depend on the
   form of stage_fire beyond this equation: it also holds for a rule that
   fails an unpaired replay fire at staging. *)
Lemma stage_fire_some :
  forall T V k cm mk T', stage_fire T V k cm mk = Some T' ->
    T' = txn_push T V (fst (pair_step T k cm)) (mk (snd (pair_step T k cm))).
Proof.
  intros T V k cm mk T' H. unfold stage_fire in H. cbv zeta in H.
  destruct (backend (t_mode T)); destruct (snd (pair_step T k cm)); cbv beta iota in H; congruence.
Qed.

(* commit_split takes apart the decisions of a commit in any order: the
   call prefix, the COMM check, the observer decision on the COMM and the
   pairing flag of the staged fire. *)
Ltac commit_split :=
  repeat match goal with
  | |- context [match space_prelude ?m ?h ?s ?a ?b with _ => _ end] =>
      let E := fresh "Ep" in destruct (space_prelude m h s a b) eqn:E; cbv iota
  | |- context [if negb (comm_eqb ?a ?b) then _ else _] =>
      let E := fresh "Ec" in destruct (negb (comm_eqb a b)) eqn:E; cbv iota
  | |- context [if negb (ob_accept ?h ?o) then _ else _] =>
      let E := fresh "Ea" in destruct (negb (ob_accept h o)) eqn:E; cbv iota
  | |- context [if negb ?p then _ else _] =>
      is_var p; let E := fresh "Eq" in destruct (negb p) eqn:E; cbv iota
  end.

Definition trigger_key (op : Op) : IOKey :=
  match op with
  | OProduce ch v persist _ => KProduce (produce_id ch v persist)
  | OConsume chs pats k persist _ _ => KConsume (consume_id chs pats k persist)
  end.

(* A fire operation: a produce Fire or a consume Fire. *)
Definition is_fire (op : Op) : bool :=
  match op with
  | OProduce _ _ _ (PFire _ _ _ _) | OConsume _ _ _ _ _ (CFire _) => true
  | _ => false
  end.

(* Decision 36: the premise of the host simulation (H1, H2, H11) in replay
   and reporting. Every staged fire paired with a recorded COMM. Under the
   rule of decision 19, a successful staging implied it. *)
Definition fire_paired (m : Mode) (sg : Staged) : Prop :=
  backend m <> Play -> is_fire (sg_op sg) = true -> sg_paired sg = true.

Lemma not_installed_true :
  forall T chs, not_installed T chs = true -> forall c, In c chs -> t_installed T c = false.
Proof.
  intros T chs H c Hin. unfold not_installed in H. rewrite forallb_forall in H.
  apply negb_true_iff. apply H. exact Hin.
Qed.

Lemma pfire_ok_spec :
  forall T ch v persist chs wk cands,
    pfire_ok T ch v persist chs wk cands = true ->
    chs <> [] /\
    (forall c, In c (ch :: chs) -> t_installed T c = false) /\
    map cd_chan cands = chs /\
    (exists cd, In cd cands /\ cd_incoming cd = true) /\
    (forall cd, In cd cands -> cd_incoming cd = true ->
       cd_chan cd = ch /\ cd_rec cd = mkDatum v persist (produce_id ch v persist)) /\
    In wk (vconts (t_view T) chs) /\
    peeks_ok (w_peeks wk) (length chs) = true /\
    stored_ok (t_view T) cands = true.
Proof.
  intros T ch v persist chs wk cands H. unfold pfire_ok in H.
  repeat rewrite andb_true_iff in H.
  destruct H as (((((((Hne & Hni) & Hmap) & Hlen) & Hinc) & Hwk) & Hpk) & Hst).
  split; [destruct chs; [discriminate | discriminate] |].
  split; [apply not_installed_true; exact Hni |].
  split; [destruct (lc_eq_dec (map cd_chan cands) chs); [assumption | discriminate] |].
  split.
  { apply Nat.eqb_eq in Hlen.
    destruct (filter cd_incoming cands) as [| cd t] eqn:Ef; [discriminate |].
    exists cd. assert (Hin : In cd (filter cd_incoming cands)) by (rewrite Ef; left; reflexivity).
    apply filter_In in Hin. exact Hin. }
  split.
  { intros cd Hin Hi. rewrite forallb_forall in Hinc. specialize (Hinc cd Hin).
    rewrite Hi in Hinc. simpl in Hinc. apply andb_true_iff in Hinc as [Hc Hd].
    unfold chan_eqb in Hc. destruct (chan_eq_dec (cd_chan cd) ch); [| discriminate].
    destruct (datum_eq_dec (cd_rec cd) (mkDatum v persist (produce_id ch v persist))); [| discriminate].
    split; assumption. }
  split; [destruct (in_dec wk_eq_dec wk (vconts (t_view T) chs)); [assumption | discriminate] |].
  split; assumption.
Qed.

Lemma cfire_ok_spec :
  forall T chs cands,
    cfire_ok T chs cands = true ->
    map cd_chan cands = chs /\
    (forall cd, In cd cands -> cd_incoming cd = false) /\
    stored_ok (t_view T) cands = true.
Proof.
  intros T chs cands H. unfold cfire_ok in H. repeat rewrite andb_true_iff in H.
  destruct H as ((Hmap & Hinc) & Hst).
  split; [destruct (lc_eq_dec (map cd_chan cands) chs); [assumption | discriminate] |].
  split; [| exact Hst].
  intros cd Hin. rewrite forallb_forall in Hinc. specialize (Hinc cd Hin).
  apply negb_true_iff in Hinc. exact Hinc.
Qed.

Lemma incoming_src_in_produces :
  forall cands consume peeks closure ch v persist,
    (exists cd, In cd cands /\ cd_incoming cd = true) ->
    (forall cd, In cd cands -> cd_incoming cd = true ->
       cd_chan cd = ch /\ cd_rec cd = mkDatum v persist (produce_id ch v persist)) ->
    In (produce_id ch v persist) (c_produces (mk_comm cands consume peeks closure)).
Proof.
  intros cands consume peeks closure ch v persist (cd & Hin & Hi) Hall.
  unfold mk_comm. simpl.
  eapply Permutation_in; [apply Permutation_sym, comm_sort_perm |].
  unfold cand_srcs. apply in_map_iff. exists cd. split; [| exact Hin].
  destruct (Hall cd Hin Hi) as [_ Hr]. rewrite Hr. reflexivity.
Qed.

(* ════ FR-10 and decision 45: peek fires, the re-offer and the re-store (H11) ════ *)

(* fire_cands gives the candidates of a fire. *)
Definition fire_cands (op : Op) : list Cand :=
  match op with
  | OProduce _ _ _ (PFire _ _ _ cands) => cands
  | OConsume _ _ _ _ _ (CFire cands) => cands
  | _ => []
  end.

(* Disabled by decision 45 (Rule E): no fire keeps a record, so no fire has retained records.
(* retained gives the stored records that a retaining (peek) fire keeps in
   place and that the host removes. These are its non-persistent stored
   candidates. HEAD's reducer re-produces them (head_reproduces). FR-10: a
   produce-side fire never keeps its incoming datum, so a kept record is
   always a stored record (retained_stored). *)
Definition retained (op : Op) : Chan -> list Datum :=
  match op with
  | OProduce _ _ _ (PFire _ wk _ cands) =>
      if negb (nullb (w_peeks wk)) then fun c => np_recs c cands else fun _ => []
  | OConsume _ _ _ _ peeks (CFire cands) =>
      if negb (nullb peeks) then fun c => np_recs c cands else fun _ => []
  | _ => fun _ => []
  end.
*)

(* Disabled by decision 45 (Rule E): no fire retains, so the retention test of a fire is gone.
(* retaining holds for a fire whose waiter peeks (retention, ssh
   peek_retention). *)
Definition retaining (op : Op) : bool :=
  match op with
  | OProduce _ _ _ (PFire _ wk _ _) => negb (nullb (w_peeks wk))
  | OConsume _ _ _ _ peeks (CFire _) => negb (nullb peeks)
  | _ => false
  end.
*)

(* Disabled by decision 45 (Rule E): retained and retaining are gone.
Lemma retained_nonretaining : forall op c, retaining op = false -> retained op c = [].
Proof.
  intros op c H. destruct op as [ch v p [| chs wk kidx cands] | chs pats k p peeks [| cands]];
    simpl in *; try reflexivity.
  - rewrite H. reflexivity.
  - rewrite H. reflexivity.
Qed.
*)

(* Disabled by decision 45 (Rule E): no fire keeps a record, so no kept record needs a source.
(* Each kept record is a stored non-persistent candidate of its fire, on
   its channel. It is never the incoming datum (FR-10). *)
Lemma retained_stored :
  forall op c d, In d (retained op c) ->
    exists cd, In cd (fire_cands op) /\ removable cd = true /\ cd_chan cd = c /\ cd_rec cd = d.
Proof.
  intros op c d Hin.
  assert (Hnp : forall cands, In d (np_recs c cands) ->
            exists cd, In cd cands /\ removable cd = true /\ cd_chan cd = c /\ cd_rec cd = d).
  { intros cands H. unfold np_recs in H. apply in_map_iff in H as (cd & Hd & Hf).
    apply filter_In in Hf as [Hcd Hb]. apply andb_true_iff in Hb as [Hr Hc].
    exists cd. split; [exact Hcd |]. split; [exact Hr |]. split; [| exact Hd].
    unfold chan_eqb in Hc. destruct (chan_eq_dec (cd_chan cd) c); [assumption | discriminate]. }
  destruct op as [ch v p [| chs wk kidx cands] | chs pats k p peeks [| cands]];
    simpl in Hin |- *; try contradiction.
  - destruct (negb (nullb (w_peeks wk))); [apply Hnp, Hin | simpl in Hin; contradiction].
  - destruct (negb (nullb peeks)); [apply Hnp, Hin | simpl in Hin; contradiction].
Qed.
*)

Lemma host_remove_continuation_data :
  forall S g i, (forall c, s_data (fst (host_remove_continuation S g i)) c = s_data S c) /\
                (forall c, s_joins (fst (host_remove_continuation S g i)) c = s_joins S c) /\
                (forall p, s_count (fst (host_remove_continuation S g i)) p = s_count S p).
Proof.
  intros S g i. unfold host_remove_continuation.
  destruct (nth_error (s_conts S g) i); simpl; repeat split; reflexivity.
Qed.

Lemma space_remove_continuation_data :
  forall S g w, (forall c, s_data (fst (space_remove_continuation S g w)) c = s_data S c) /\
                (forall c, s_joins (fst (space_remove_continuation S g w)) c = s_joins S c) /\
                (forall p, s_count (fst (space_remove_continuation S g w)) p = s_count S p).
Proof.
  intros S g w. unfold space_remove_continuation.
  destruct (in_dec wk_eq_dec w (s_conts S g)); simpl; repeat split; reflexivity.
Qed.

(* Disabled by decision 45 (Rule E): the loops have no retain flag, so no loop retains the records.
(* A retaining produce-side loop removes no record. It removes only the
   fired join of each candidate channel. *)
Lemma space_pf_loop_retain_data :
  forall cs S chs c, s_data (fst (space_pf_loop S chs true cs)) c = s_data S c.
Proof.
  induction cs as [| cd rest IH]; intros S chs c; simpl; [reflexivity |].
  rewrite IH. destruct (remove_join_fields S (cd_chan cd) chs) as (Rd & _). apply Rd.
Qed.
*)

(* Disabled by decision 45 (Rule E): the loops have no retain flag, so no loop retains the records.
(* A retaining consume-side loop changes nothing. *)
Lemma space_cf_loop_retain : forall cs S, space_cf_loop S true cs = (S, []).
Proof.
  induction cs as [| cd rest IH]; intros S; simpl; [reflexivity |].
  rewrite IH. reflexivity.
Qed.
*)

(* Disabled by decision 45 (Rule E): a peek fire now removes its records, so it changes data lists.
(* FR-10 and retention: a peek fire changes no data list, in play, replay
   and reporting, and also when it is rejected. It removes no stored record,
   and it never stores its incoming datum. The agent re-offers that datum as
   the next operation (reoffers). *)
Lemma peek_fire_keeps_data :
  forall m H S rd sg, retaining (sg_op sg) = true ->
    forall c, s_data (r_st (commit_op m H S rd sg)) c = s_data S c.
Proof.
  intros m H S rd [op j0 scomm sp] Hr c. unfold commit_op.
  cbn [sg_op sg_joins0 sg_comm sg_paired] in *.
  destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]];
    cbn [retaining] in Hr; try discriminate.
  - assert (Hp1 : forall S1, s_data (fst (if w_persist wk then (S1, [])
                                             else space_remove_continuation S1 chs wk)) c
                             = s_data S1 c).
    { intros S1. destruct (w_persist wk); [reflexivity |]. apply space_remove_continuation_data. }
    destruct (backend m);
      [unfold commit_play_produce | unfold commit_replay_produce | unfold commit_replay_produce];
      destruct scomm as [cm |]; commit_split; cbn zeta; cbn [r_st];
      first [reflexivity | rewrite Hr, space_pf_loop_retain_data, Hp1; reflexivity].
  - destruct (backend m);
      [unfold commit_play_consume | unfold commit_replay_consume | unfold commit_replay_consume];
      destruct scomm as [cm |]; commit_split; cbn zeta; cbn [r_st];
      first [reflexivity | rewrite Hr, space_cf_loop_retain; reflexivity].
Qed.
*)

(* ── FR-10 restated: a fire stores no datum (decision 45, C.10) ──────── *)

(* A removal loop never adds a record. *)
Lemma space_pf_loop_data_le :
  forall cs S chs c x,
    count_occ datum_eq_dec (s_data (fst (space_pf_loop S chs cs)) c) x <=
    count_occ datum_eq_dec (s_data S c) x.
Proof.
  intros cs S chs c x. destruct (space_pf_loop_spec cs S chs) as (Ld & _). rewrite Ld. lia.
Qed.

Lemma space_cf_loop_data_le :
  forall cs S c x,
    count_occ datum_eq_dec (s_data (fst (space_cf_loop S cs)) c) x <=
    count_occ datum_eq_dec (s_data S c) x.
Proof.
  induction cs as [| cd rest IH]; intros S c x; simpl; [lia |].
  eapply Nat.le_trans; [apply IH |].
  destruct (negb (d_persist (cd_rec cd))).
  - destruct (space_remove_datum_fields S (cd_chan cd) (cd_rec cd)) as (Fd & _).
    rewrite Fd. lia.
  - cbn [fst]. lia.
Qed.

(* destruct_ifs splits each boolean test of the goal whose value is not
   known. *)
Ltac destruct_ifs :=
  repeat match goal with
  | |- context [if ?b then _ else _] =>
      lazymatch b with true => fail | false => fail | _ => destruct b end
  end.

(* FR-10 after decision 45 (Rule E): the commit of a fire never stores a
   datum, in play, replay and reporting, and also when it is rejected. It
   only removes records, so no data list gains a record. A produce-side
   fire never stores its incoming datum: the agent re-offers that datum as
   the next operation (reoffers). A consume-side peek fire removes its
   non-persistent records, and the agent stores each of them again as the
   next operations (restores_after). *)
Lemma fire_stores_no_datum :
  forall m H S rd sg, is_fire (sg_op sg) = true ->
    forall c x, count_occ datum_eq_dec (s_data (r_st (commit_op m H S rd sg)) c) x <=
                count_occ datum_eq_dec (s_data S c) x.
Proof.
  intros m H S rd [op j0 scomm sp] Hf c x. unfold commit_op.
  cbn [sg_op sg_joins0 sg_comm sg_paired] in *.
  destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]];
    cbn [is_fire] in Hf; try discriminate;
    destruct (backend m);
    unfold commit_play_produce, commit_replay_produce, commit_play_consume, commit_replay_consume;
    cbn zeta; destruct scomm as [cm |]; commit_split; cbn [r_st];
    destruct_ifs; cbn [fst];
    first
      [ apply Nat.eq_le_incl; reflexivity
      | eapply Nat.le_trans; [apply space_pf_loop_data_le |];
        first [ apply Nat.eq_le_incl; reflexivity
              | rewrite (proj1 (space_remove_continuation_data _ _ _));
                apply Nat.eq_le_incl; reflexivity ]
      | apply space_cf_loop_data_le ].
Qed.

(* ── The records that a fire removes (decision 45, Rule E) ──────────── *)

(* The data of the view after a produce-side fire: each stored
   non-persistent candidate loses one copy of its record. The incoming
   datum is never stored (FR-10). *)
Lemma vpf_effects_data :
  forall V ch v persist chs wk cands c x,
    count_occ datum_eq_dec (vdata (vpf_effects V ch v persist chs wk cands) c) x =
    count_occ datum_eq_dec (vdata V c) x - count_occ datum_eq_dec (np_recs c cands) x.
Proof.
  intros V ch v persist chs wk cands c x. unfold vpf_effects. cbv zeta.
  set (V1 := if w_persist wk then V else vremove_continuation V chs wk).
  assert (E1 : vdata V1 c = vdata V c) by (subst V1; destruct (w_persist wk); reflexivity).
  destruct (view_pf_loop (sort_desc vkey cands) V1 (view_store V1) chs (store_eq_refl _)) as (Ed & _).
  destruct (space_pf_loop_spec (sort_desc vkey cands) (view_store V1) chs) as (Ld & _).
  change (vdata (vpf_loop V1 chs (sort_desc vkey cands)) c)
    with (s_data (view_store (vpf_loop V1 chs (sort_desc vkey cands))) c).
  rewrite Ed, Ld. change (s_data (view_store V1) c) with (vdata V1 c). rewrite E1.
  rewrite (np_recs_perm c (sort_desc vkey cands) cands x (sort_desc_perm _ vkey cands)).
  reflexivity.
Qed.

(* The data of the view after a consume-side fire: each stored
   non-persistent candidate loses one copy of its record, also when the
   fire peeks. *)
Lemma vcf_effects_data :
  forall V cands, (forall cd, In cd cands -> cd_incoming cd = false) ->
    forall c x, count_occ datum_eq_dec (vdata (vcf_effects V cands) c) x =
                count_occ datum_eq_dec (vdata V c) x - count_occ datum_eq_dec (np_recs c cands) x.
Proof.
  intros V cands Hinc c x. unfold vcf_effects.
  assert (Hinc' : forall cd, In cd (sort_desc vkey cands) -> cd_incoming cd = false).
  { intros cd Hin. apply Hinc. eapply Permutation_in; [apply sort_desc_perm | exact Hin]. }
  destruct (view_cf_loop (sort_desc vkey cands) V (view_store V) (store_eq_refl _)) as (Ed & _).
  destruct (space_cf_loop_spec (sort_desc vkey cands) (view_store V) Hinc') as (Ld & _).
  change (vdata (vcf_loop V (sort_desc vkey cands)) c)
    with (s_data (view_store (vcf_loop V (sort_desc vkey cands))) c).
  rewrite Ed, Ld. change (s_data (view_store V) c) with (vdata V c).
  rewrite (np_recs_perm c (sort_desc vkey cands) cands x (sort_desc_perm _ vkey cands)).
  reflexivity.
Qed.

(* stored_ok bounds the records that a fire removes: on each channel, the
   view holds each removed record at least as often as the fire removes
   it. *)
Lemma stored_ok_np_le :
  forall V cands, stored_ok V cands = true ->
    forall c x, count_occ datum_eq_dec (np_recs c cands) x <= count_occ datum_eq_dec (vdata V c) x.
Proof.
  intros V cands Hso c x.
  destruct (count_occ datum_eq_dec (np_recs c cands) x) as [| n] eqn:E0; [lia |].
  assert (Hin : In x (np_recs c cands)) by (apply (count_occ_In datum_eq_dec); lia).
  unfold np_recs in Hin. apply in_map_iff in Hin as (cd & Hx & Hf).
  apply filter_In in Hf as [Hcd Hb]. apply andb_true_iff in Hb as [Hr Hc].
  unfold chan_eqb in Hc. destruct (chan_eq_dec (cd_chan cd) c) as [Ec | _]; [| discriminate].
  unfold stored_ok in Hso. rewrite forallb_forall in Hso. specialize (Hso cd Hcd).
  cbv beta in Hso.
  unfold removable in Hr. apply andb_true_iff in Hr as [Hi Hp].
  apply negb_true_iff in Hi. apply negb_true_iff in Hp.
  rewrite Hi, Hp in Hso. cbv beta iota in Hso. apply Nat.leb_le in Hso.
  rewrite <- E0. subst c x. exact Hso.
Qed.

(* H11 (decision 45, Rule E): the view of a staged fire loses exactly the
   records that the fire removes. These are its stored non-persistent
   candidates (np_recs), on both sides and also for a peek. The host
   removes the same records (host_rm_loop, host_spd_loop). A produce-side
   fire never stores its incoming datum (FR-10). *)
Lemma fire_view_removes :
  forall T op T1, stage T op = Some T1 -> is_fire op = true ->
    forall c x, count_occ datum_eq_dec (vdata (t_view T1) c) x
                + count_occ datum_eq_dec (np_recs c (fire_cands op)) x
                = count_occ datum_eq_dec (vdata (t_view T) c) x.
Proof.
  intros T op T1 Hst Hf c x.
  destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]];
    cbn [is_fire] in Hf; try discriminate; cbn [fire_cands].
  - unfold stage in Hst.
    destruct (t_installed T ch); [discriminate |].
    destruct (pfire_ok T ch v persist chs wk cands) eqn:Hok; cbn [negb] in Hst; [| discriminate].
    apply stage_fire_some in Hst. subst T1. cbn [t_view txn_push].
    destruct (pfire_ok_spec T ch v persist chs wk cands Hok) as (_ & _ & _ & _ & _ & _ & _ & Hso).
    rewrite vpf_effects_data.
    assert (Hb : forall c', vdata (vbump (t_view T) (produce_id ch v persist) persist) c' =
                            vdata (t_view T) c')
      by (intros c'; unfold vbump; destruct persist; reflexivity).
    rewrite Hb. pose proof (stored_ok_np_le (t_view T) cands Hso c x). lia.
  - unfold stage in Hst.
    destruct (consume_args_ok T chs pats peeks); cbn [negb] in Hst; [| discriminate].
    destruct (cfire_ok T chs cands) eqn:Hcf; cbn [negb] in Hst; [| discriminate].
    apply stage_fire_some in Hst. subst T1. cbn [t_view txn_push].
    destruct (cfire_ok_spec T chs cands Hcf) as (_ & Hinc & Hso).
    rewrite (vcf_effects_data (t_view T) cands Hinc).
    pose proof (stored_ok_np_le (t_view T) cands Hso c x). lia.
Qed.

(* ── The re-offer ─────────────────────────────────────────────────────── *)

(* FR-10: reoffer_due holds for a fire whose datum the agent re-offers as
   the next operation of the same transaction. A non-persistent produce
   fires a waiter that peeks and is not persistent. The fire does not store
   the datum, and the waiter leaves. A persistent waiter stays at the head,
   so the P4 plan re-issues the datum in a new transaction instead
   (p4-design-report A10). *)
Definition reoffer_due (op : Op) : bool :=
  match op with
  | OProduce _ _ persist (PFire _ wk _ _) =>
      negb persist && negb (nullb (w_peeks wk)) && negb (w_persist wk)
  | _ => false
  end.

(* [next] re-offers the datum of [op]: a produce with the same channel,
   value and persist flag. So it has the Produce::create identity, the
   trigger key and the counter step of HEAD's re-produce
   (reduce.rs:977-1012). Its outcome is the agent's decision: it fires the
   next waiter or stores the datum. *)
Definition reoffer_of (op next : Op) : Prop :=
  match op, next with
  | OProduce ch v persist _, OProduce ch' v' persist' _ => ch' = ch /\ v' = v /\ persist' = persist
  | _, _ => False
  end.

(* reoffers states the FR-10 discipline of an agent: each due re-offer is
   the next operation of the transaction. *)
Fixpoint reoffers (ops : list Op) : Prop :=
  match ops with
  | [] => True
  | op :: rest =>
      (reoffer_due op = true ->
         match rest with [] => False | next :: _ => reoffer_of op next end)
      /\ reoffers rest
  end.

Lemma reoffer_same_identity :
  forall op next, reoffer_of op next -> trigger_key next = trigger_key op.
Proof.
  intros [ch v persist out | chs pats k persist peeks out]
         [ch' v' persist' out' | chs' pats' k' persist' peeks' out']; simpl; intros H;
    try contradiction.
  destruct H as (-> & -> & ->). reflexivity.
Qed.

Lemma vpf_loop_counts :
  forall cs V chs p, vcount (vpf_loop V chs cs) p = vcount V p.
Proof.
  induction cs as [| cd rest IH]; intros V chs p; simpl; [reflexivity |].
  rewrite IH. destruct (removable cd); unfold vremove_join;
    destruct (vconts _ chs); reflexivity.
Qed.

Lemma vpf_effects_counts :
  forall V ch v persist chs wk cands p,
    vcount (vpf_effects V ch v persist chs wk cands) p = vcount V p.
Proof.
  intros V ch v persist chs wk cands p. unfold vpf_effects. rewrite vpf_loop_counts.
  destruct (w_persist wk); reflexivity.
Qed.

Lemma vbump_count_self : forall V src, vcount (vbump V src false) src = S (vcount V src).
Proof.
  intros V src. unfold vbump, vcount, vset_delta, vdelta. simpl.
  destruct (pid_eq_dec src src) as [_ | C]; [lia | contradiction].
Qed.

(* FR-10: staging accepts the re-offer, a second produce with the same
   identity in the transaction. The accounting layer also accepts a second
   registration of an identity with an equal authority
   (accounting/mod.rs:1041-1046). Each produce steps the counter of the
   identity by one, as log_produce does for HEAD's produce and re-produce.
   With no next waiter, the re-offer stores the datum at the head of the
   list of its channel. *)
Lemma reoffer_store_staged :
  forall T ch v chs wk kidx cands T1,
    stage T (OProduce ch v false (PFire chs wk kidx cands)) = Some T1 ->
    let src := produce_id ch v false in
    exists T2, stage T1 (OProduce ch v false PStore) = Some T2 /\
      vcount (t_view T1) src = S (vcount (t_view T) src) /\
      vcount (t_view T2) src = S (vcount (t_view T1) src) /\
      vdata (t_view T2) ch = mkDatum v false src :: vdata (t_view T1) ch.
Proof.
  intros T ch v chs wk kidx cands T1 Hst src.
  unfold stage in Hst.
  destruct (t_installed T ch) eqn:Hi; [discriminate |].
  destruct (pfire_ok T ch v false chs wk cands); cbn [negb] in Hst; [| discriminate].
  apply stage_fire_some in Hst. subst T1.
  eexists. split; [unfold stage; simpl; rewrite Hi; reflexivity |].
  simpl. split; [| split].
  - rewrite vpf_effects_counts. apply vbump_count_self.
  - exact (vbump_count_self _ _).
  - unfold vput_datum. rewrite vdata_set.
    destruct (chan_eq_dec ch ch) as [_ | C]; [reflexivity | contradiction].
Qed.

(* ── The fire receipt ────────────────────────────────────────────────── *)

(* Disabled by decision 45 (Rule E): no fire retains a result, so the receipt marks no result retained.
(* ssh retained_results: the receipt retains a result when the fire retains
   (peeks) and the result is a stored candidate. FR-10: the incoming datum is
   never retained, because the fire does not store it. *)
Definition retained_result (retain : bool) (cd : Cand) : bool := retain && negb (cd_incoming cd).
*)

(* Disabled by decision 45 (Rule E): the receipt keeps the host flag of each result and rewrites none.
(* ssh fired_receipt: the receipt reports a retained result as persistent,
   because the store keeps its record. Every other result keeps the flag of
   its record, as wrap_result reports it (rs/rspace/ops_consume.rs:204-229). *)
Definition receipt_persistent (retain : bool) (cd : Cand) : bool :=
  retained_result retain cd || d_persist (cd_rec cd).
*)

(* wrap_result (rs/rspace/ops_consume.rs:204-229, rs/util.rs:57-70) gives one
   result for each candidate. The persist flag of a result is the persist
   flag of its record (persistent = datum.persist): the stored record, or
   the incoming datum of a produce-side fire. *)
Definition wrap_flags (cands : list Cand) : list bool :=
  map (fun cd => d_persist (cd_rec cd)) cands.

(* ssh fired_receipt: the receipt of a fire takes the results of
   wrap_result. It fails (None, SpaceTxnError::Inconsistent) when the
   results do not line up with the candidates. wrap_result gives one result
   for each candidate, so that check never fails here. Decision 45 (Rule E):
   the receipt rewrites no flag. *)
Definition fired_receipt (n : nat) (flags : list bool) : option (list bool) :=
  if Nat.eqb (length flags) n then Some flags else None.

(* H11 (decision 45, Rule E): the receipt of a fire reports the persist flag
   of the record of each result, as the host reports it. So a result that
   is not persistent is a record that the fire removed, or the incoming
   datum of a non-persistent produce. These are the data that HEAD's
   produce_peeks sends again (head_reproduces). *)
Lemma receipt_flags_are_persist :
  forall cands,
    fired_receipt (length cands) (wrap_flags cands) = Some (map (fun cd => d_persist (cd_rec cd)) cands).
Proof.
  intros cands. unfold fired_receipt, wrap_flags. rewrite length_map, Nat.eqb_refl. reflexivity.
Qed.

Lemma pfire_ok_one_incoming :
  forall T ch v persist chs wk cands, pfire_ok T ch v persist chs wk cands = true ->
    length (filter cd_incoming cands) = 1.
Proof.
  intros T ch v persist chs wk cands H. unfold pfire_ok in H.
  repeat rewrite andb_true_iff in H.
  destruct H as (((((((_ & _) & _) & Hlen) & _) & _) & _) & _).
  apply Nat.eqb_eq. exact Hlen.
Qed.

(* A staged produce-side fire has exactly one incoming candidate, on the
   produce channel. *)
Lemma pfire_ok_incoming :
  forall T ch v persist chs wk cands, pfire_ok T ch v persist chs wk cands = true ->
    exists cd0, filter cd_incoming cands = [cd0] /\ In cd0 cands /\
      cd_chan cd0 = ch /\ cd_rec cd0 = mkDatum v persist (produce_id ch v persist).
Proof.
  intros T ch v persist chs wk cands Hok.
  destruct (pfire_ok_spec T ch v persist chs wk cands Hok) as (_ & _ & _ & _ & Hinc & _).
  assert (Hlen := pfire_ok_one_incoming T ch v persist chs wk cands Hok).
  destruct (filter cd_incoming cands) as [| cd0 [| cd1 rest]] eqn:Ef; simpl in Hlen; try discriminate.
  assert (Hcd0 : In cd0 (filter cd_incoming cands)) by (rewrite Ef; left; reflexivity).
  apply filter_In in Hcd0 as [Hin0 Hi0].
  destruct (Hinc cd0 Hin0 Hi0) as [Hch0 Hrec0].
  exists cd0. auto.
Qed.

(* Disabled by decision 45 (Rule E): the receipt no longer reports the stored results of a peek persistent.
(* H11 (FR-10): the receipt of a staged produce-side peek fire reports every
   stored result persistent. It reports the incoming result with the persist
   flag of the produce. So the results that are not persistent are the
   incoming datum of a non-persistent produce: the datum that the agent
   re-offers. *)
Lemma receipt_reports_reoffer :
  forall T ch v persist chs wk cands,
    pfire_ok T ch v persist chs wk cands = true -> w_peeks wk <> [] ->
    let retain := negb (nullb (w_peeks wk)) in
    (forall cd, In cd cands -> cd_incoming cd = false -> receipt_persistent retain cd = true) /\
    (forall cd, In cd cands -> cd_incoming cd = true -> receipt_persistent retain cd = persist) /\
    map (fun cd => (cd_chan cd, d_val (cd_rec cd)))
        (filter (fun cd => negb (receipt_persistent retain cd)) cands)
      = (if persist then [] else [(ch, v)]).
Proof.
  intros T ch v persist chs wk cands Hok Hpk retain.
  assert (Hret : retain = true).
  { subst retain. destruct (w_peeks wk) eqn:E; [contradiction | reflexivity]. }
  destruct (pfire_ok_spec T ch v persist chs wk cands Hok) as (_ & _ & _ & _ & Hinc & _).
  destruct (pfire_ok_incoming T ch v persist chs wk cands Hok) as (cd0 & Ef & _ & Hch0 & Hrec0).
  rewrite Hret. unfold receipt_persistent, retained_result.
  split; [intros cd _ Hi; rewrite Hi; reflexivity |].
  split; [intros cd Hin Hi; rewrite Hi; destruct (Hinc cd Hin Hi) as [_ Hrec]; rewrite Hrec; reflexivity |].
  assert (E : filter (fun cd => negb (true && negb (cd_incoming cd) || d_persist (cd_rec cd))) cands =
              filter (fun cd => cd_incoming cd && negb persist) cands).
  { apply filter_ext_in. intros cd Hin. destruct (cd_incoming cd) eqn:Ei; simpl.
    - destruct (Hinc cd Hin Ei) as [_ Hrec]. rewrite Hrec. reflexivity.
    - reflexivity. }
  rewrite E, (filter_and Cand cd_incoming (fun _ => negb persist)), Ef.
  destruct persist; simpl; [reflexivity |].
  rewrite Hch0, Hrec0. reflexivity.
Qed.
*)

(* ── HEAD's re-produces ─────────────────────────────────────────────── *)

(* head_reproduces lists the channel and the value of each produce that HEAD
   re-produces after a fire whose waiter peeks. The function produce_peeks
   (rholang/src/rust/interpreter/reduce.rs:981-1016) takes each result of
   wrap_result whose persist flag is false (rs/util.rs:57-70, wrap_flags).
   It re-produces the removed value of that result (rs/space_matcher.rs:72-90)
   on its channel, as a non-persistent produce. The reducer calls
   produce_peeks after a non-persistent produce fires a peek waiter
   (rholang/src/rust/interpreter/reduce.rs:784-811) and after a consume with
   a peek fires (rholang/src/rust/interpreter/reduce.rs:909-936). After a
   persistent produce, the reducer calls it too
   (rholang/src/rust/interpreter/reduce.rs:776-781). *)
Definition head_reproduces (cands : list Cand) : list (Chan * Val) :=
  map (fun cd => (cd_chan cd, d_val (cd_rec cd)))
      (filter (fun cd => negb (d_persist (cd_rec cd))) cands).

(* Disabled by decision 45 (Rule E): the substrate keeps no record, so no re-produce of HEAD is replaced.
(* kept_reproduces lists the re-produces of HEAD that the substrate
   replaces: the stored records that a peek fire keeps in place. *)
Definition kept_reproduces (cands : list Cand) : list (Chan * Val) :=
  map (fun cd => (cd_chan cd, d_val (cd_rec cd))) (filter removable cands).
*)

(* restore_reproduces lists the channel and the value of each record that a
   fire removes (decision 45, Rule E): its stored non-persistent candidates.
   The agent stores each of these records again (restores_after). *)
Definition restore_reproduces (cands : list Cand) : list (Chan * Val) :=
  map (fun cd => (cd_chan cd, d_val (cd_rec cd))) (filter removable cands).

(* H11 (FR-10 and decision 45): after a produce-side fire, HEAD re-produces
   two kinds of data. The first is the incoming datum of a non-persistent
   produce, which the agent re-offers (reoffers). The second is the stored
   records that the fire removes (restore_reproduces). A P4 produce-side
   fire has none of them, because its one-channel group binds only the
   incoming datum. A join re-stores them, as the consume side does. *)
Lemma head_reproduces_produce :
  forall T ch v persist chs wk cands, pfire_ok T ch v persist chs wk cands = true ->
    Permutation (head_reproduces cands)
                ((if persist then [] else [(ch, v)]) ++ restore_reproduces cands).
Proof.
  intros T ch v persist chs wk cands Hok.
  destruct (pfire_ok_incoming T ch v persist chs wk cands Hok) as (cd0 & Ef & _ & Hch0 & Hrec0).
  unfold head_reproduces, restore_reproduces.
  eapply perm_trans; [apply Permutation_map, (filter_split Cand cd_incoming) |].
  rewrite map_app, (filter_and Cand cd_incoming (fun cd => negb (d_persist (cd_rec cd)))), Ef.
  cbn [filter]. rewrite Hrec0. destruct persist; simpl; [reflexivity |].
  rewrite Hch0, Hrec0. reflexivity.
Qed.

(* Disabled by decision 45 (Rule E): restores_are_produce_peeks states the consume side with the re-stores.
(* After a consume-side fire, HEAD re-produces exactly the stored records
   that the substrate keeps. *)
Lemma head_reproduces_consume :
  forall T chs cands, cfire_ok T chs cands = true -> head_reproduces cands = kept_reproduces cands.
Proof.
  intros T chs cands Hok. destruct (cfire_ok_spec T chs cands Hok) as (_ & Hinc & _).
  unfold head_reproduces, kept_reproduces. f_equal. apply filter_ext_in. intros cd Hin.
  unfold removable. rewrite (Hinc cd Hin). reflexivity.
Qed.
*)

(* Disabled by decision 45 (Rule E): values_on served only the deviation of decision 19, which is gone.
(* values_on gives the values that a list of re-produces sends on the
   channel c, in order. *)
Definition values_on (c : Chan) (rps : list (Chan * Val)) : list Val :=
  map snd (filter (fun p => chan_eqb (fst p) c) rps).
*)

(* Disabled by decision 45 (Rule E): the substrate keeps no record, so the deviation of decision 19 is gone.
(* The deviation of decision 19 is exact. On each channel, the re-produces
   of HEAD that the substrate replaces send the values of the kept stored
   records (np_recs, the records of retained). *)
Lemma kept_reproduces_values :
  forall cands c, values_on c (kept_reproduces cands) = map d_val (np_recs c cands).
Proof.
  intros cands c. unfold values_on, kept_reproduces, np_recs.
  induction cands as [| cd rest IH]; [reflexivity |].
  cbn [filter]. destruct (removable cd); cbn [andb]; [| exact IH].
  cbn [map filter fst]. destruct (chan_eqb (cd_chan cd) c); cbn [map snd]; [f_equal |]; exact IH.
Qed.
*)

(* Disabled by decision 45 (Rule E): no fire retains a record, so no re-produce of HEAD is replaced.
(* For every retaining fire, on each channel, the re-produces of HEAD that
   the substrate replaces send exactly the values of the retained records. *)
Lemma kept_reproduces_retained :
  forall op c, retaining op = true ->
    values_on c (kept_reproduces (fire_cands op)) = map d_val (retained op c).
Proof.
  intros op c Hr.
  destruct op as [ch v p [| chs wk kidx cands] | chs pats k p peeks [| cands]];
    cbn [retaining] in Hr; try discriminate; cbn [retained fire_cands]; rewrite Hr;
    apply kept_reproduces_values.
Qed.
*)

(* ── The re-store (decision 45, Rule E) ─────────────────────────────── *)

(* restore_of gives the re-store of a candidate: a Store of the record that
   the fire removed, on its channel, with the value of the record and
   persist = false (generic.rs Cx::restore). HEAD's produce_peeks makes
   the same produce: the removed value on its channel, not persistent. *)
Definition restore_of (cd : Cand) : Op :=
  OProduce (cd_chan cd) (d_val (cd_rec cd)) false PStore.

(* restores_after gives the operations that the agent stages right after a
   fire. After a consume-side peek fire, it stages the re-store of each
   record that the fire removes (generic.rs receive_head and
   restore_peeked). A fire that does not peek, and a produce-side
   fire, stage no re-store here. The agent re-offers the incoming datum of a
   produce-side peek fire instead (reoffers). *)
Definition restores_after (op : Op) : list Op :=
  match op with
  | OConsume _ _ _ _ peeks (CFire cands) =>
      if nullb peeks then [] else map restore_of (filter removable cands)
  | _ => []
  end.

(* host_produce_peeks gives the produces that HEAD's produce_peeks makes
   after a consume-side fire whose waiter peeks: a non-persistent produce of
   each result whose persist flag is false, on its channel, with its
   removed value. The model gives each produce the outcome Store, as in P4:
   the queue fires a receive only at its head, when no waiter is parked,
   so the re-produce meets no waiter. *)
Definition host_produce_peeks (op : Op) : list Op :=
  match op with
  | OConsume _ _ _ _ peeks (CFire cands) =>
      if nullb peeks then []
      else map (fun p => OProduce (fst p) (snd p) false PStore) (head_reproduces cands)
  | _ => []
  end.

(* stored_sources holds when the source of each record of the view is the
   Produce::create identity of its channel, value and persist flag
   (rs/trace/event.rs:238-249). A staged Store and a host produce store
   exactly that identity, and staging keeps the property
   (stage_keeps_stored_sources). *)
Definition stored_sources (V : View) : Prop :=
  forall c d, In d (vdata V c) -> d_src d = produce_id c (d_val d) (d_persist d).

Lemma chan_eqb_refl : forall c, chan_eqb c c = true.
Proof. intros c. unfold chan_eqb. destruct (chan_eq_dec c c); [reflexivity | contradiction]. Qed.

Lemma chan_eqb_neq : forall a b, a <> b -> chan_eqb a b = false.
Proof. intros a b N. unfold chan_eqb. destruct (chan_eq_dec a b); [contradiction | reflexivity]. Qed.

(* H11 (decision 45, Rule E): a re-store has the identity of the produce
   that stored the record. Its trigger key is the source of the record, and
   the datum that it stores is the record itself. So the re-store and
   HEAD's re-produce have one Produce identity. *)
Lemma restore_same_identity :
  forall V cd, stored_sources V -> In (cd_rec cd) (vdata V (cd_chan cd)) -> removable cd = true ->
    trigger_key (restore_of cd) = KProduce (d_src (cd_rec cd)) /\
    mkDatum (d_val (cd_rec cd)) false (produce_id (cd_chan cd) (d_val (cd_rec cd)) false) = cd_rec cd.
Proof.
  intros V cd Hsrc Hin Hr.
  assert (Hp : d_persist (cd_rec cd) = false).
  { unfold removable in Hr. apply andb_true_iff in Hr as [_ Hp]. apply negb_true_iff, Hp. }
  assert (Hs : d_src (cd_rec cd) = produce_id (cd_chan cd) (d_val (cd_rec cd)) false).
  { rewrite <- Hp. apply Hsrc, Hin. }
  unfold restore_of. cbn [trigger_key]. split; [rewrite Hs; reflexivity |].
  rewrite <- Hs, <- Hp. destruct (cd_rec cd); reflexivity.
Qed.

(* H11 (decision 45, Rule E): the re-stores that the agent stages after a
   consume-side fire are exactly the produces of HEAD's produce_peeks, in
   order: one for each result whose persist flag is false. The fire has no
   incoming candidate, so these results are the records that it removes. *)
Lemma restores_are_produce_peeks :
  forall T chs pats k persist peeks cands T1,
    stage T (OConsume chs pats k persist peeks (CFire cands)) = Some T1 ->
    restores_after (OConsume chs pats k persist peeks (CFire cands)) =
    host_produce_peeks (OConsume chs pats k persist peeks (CFire cands)).
Proof.
  intros T chs pats k persist peeks cands T1 Hst.
  unfold stage in Hst.
  destruct (consume_args_ok T chs pats peeks); cbn [negb] in Hst; [| discriminate].
  destruct (cfire_ok T chs cands) eqn:Hcf; cbn [negb] in Hst; [| discriminate].
  destruct (cfire_ok_spec T chs cands Hcf) as (_ & Hinc & _).
  assert (E : filter removable cands = filter (fun cd => negb (d_persist (cd_rec cd))) cands).
  { apply filter_ext_in. intros cd Hin. unfold removable. rewrite (Hinc cd Hin). reflexivity. }
  unfold restores_after, host_produce_peeks, head_reproduces.
  destruct (nullb peeks); [reflexivity |].
  rewrite E, map_map. reflexivity.
Qed.

(* The facts of a staged consume-side fire about a record that it removes:
   the channel of the record is not installed after the fire, the record is
   in the view before the fire, and (under stored_sources) the re-store
   stores the record itself. *)
Lemma consume_fire_removed :
  forall T chs pats k persist peeks cands T1,
    stage T (OConsume chs pats k persist peeks (CFire cands)) = Some T1 ->
    stored_sources (t_view T) ->
    forall cd, In cd cands -> removable cd = true ->
      t_installed T1 (cd_chan cd) = false /\
      In (cd_rec cd) (vdata (t_view T) (cd_chan cd)) /\
      mkDatum (d_val (cd_rec cd)) false (produce_id (cd_chan cd) (d_val (cd_rec cd)) false) = cd_rec cd.
Proof.
  intros T chs pats k persist peeks cands T1 Hst Hsrc cd Hin Hr.
  unfold stage in Hst.
  destruct (consume_args_ok T chs pats peeks) eqn:Hargs; cbn [negb] in Hst; [| discriminate].
  destruct (cfire_ok T chs cands) eqn:Hcf; cbn [negb] in Hst; [| discriminate].
  apply stage_fire_some in Hst. subst T1.
  destruct (cfire_ok_spec T chs cands Hcf) as (Hmap & _ & Hso).
  assert (Hv : In (cd_rec cd) (vdata (t_view T) (cd_chan cd))).
  { pose proof (stored_ok_np_le (t_view T) cands Hso (cd_chan cd) (cd_rec cd)) as Hle.
    assert (Hnp : In (cd_rec cd) (np_recs (cd_chan cd) cands)).
    { unfold np_recs. apply in_map. apply filter_In. split; [exact Hin |].
      rewrite Hr, chan_eqb_refl. reflexivity. }
    apply (count_occ_In datum_eq_dec) in Hnp. apply (count_occ_In datum_eq_dec). lia. }
  split; [| split; [exact Hv | exact (proj2 (restore_same_identity (t_view T) cd Hsrc Hv Hr))]].
  cbn [t_installed txn_push].
  unfold consume_args_ok in Hargs. repeat rewrite andb_true_iff in Hargs.
  destruct Hargs as (((_ & _) & Hnot) & _).
  apply (not_installed_true T chs Hnot). rewrite <- Hmap. apply in_map, Hin.
Qed.

(* H11 (decision 45, Rule E): after a staged consume-side fire, the re-store
   of each record that the fire removes is a staged Store. Staging accepts
   it, also when the transaction already holds a produce of that identity.
   It stores the record itself at the head of the list of its channel
   (put_datum inserts at index 0, rs/hot_store.rs:879-897). It steps the
   counter of the identity by one, as log_produce does for HEAD's
   re-produce. *)
Lemma restore_store_staged :
  forall T chs pats k persist peeks cands T1 cd,
    stage T (OConsume chs pats k persist peeks (CFire cands)) = Some T1 ->
    In cd cands -> removable cd = true -> stored_sources (t_view T) ->
    exists T2, stage T1 (restore_of cd) = Some T2 /\
      trigger_key (restore_of cd) = KProduce (d_src (cd_rec cd)) /\
      vdata (t_view T2) (cd_chan cd) = cd_rec cd :: vdata (t_view T1) (cd_chan cd) /\
      vcount (t_view T2) (d_src (cd_rec cd)) = S (vcount (t_view T1) (d_src (cd_rec cd))).
Proof.
  intros T chs pats k persist peeks cands T1 cd Hst Hin Hr Hsrc.
  destruct (consume_fire_removed T chs pats k persist peeks cands T1 Hst Hsrc cd Hin Hr)
    as (Hni & Hv & Hrec).
  destruct (restore_same_identity (t_view T) cd Hsrc Hv Hr) as (Hkey & _).
  assert (Hs : d_src (cd_rec cd) = produce_id (cd_chan cd) (d_val (cd_rec cd)) false)
    by (rewrite <- Hrec; reflexivity).
  eexists. split; [unfold stage, restore_of; rewrite Hni; reflexivity |].
  split; [exact Hkey |].
  cbn [t_view txn_push]. split.
  - unfold vput_datum. rewrite vdata_set.
    destruct (chan_eq_dec (cd_chan cd) (cd_chan cd)) as [_ | C]; [| contradiction].
    rewrite Hrec. reflexivity.
  - rewrite Hs. exact (vbump_count_self _ _).
Qed.

(* Staging the re-stores of removed records: each re-store is a staged
   Store, and it puts its record back on its channel. *)
Lemma restores_stage :
  forall F T1,
    (forall cd, In cd F -> t_installed T1 (cd_chan cd) = false /\
       mkDatum (d_val (cd_rec cd)) false (produce_id (cd_chan cd) (d_val (cd_rec cd)) false) = cd_rec cd) ->
    exists T2, stage_all T1 (map restore_of F) = Some T2 /\
      forall c x, count_occ datum_eq_dec (vdata (t_view T2) c) x =
                  count_occ datum_eq_dec (vdata (t_view T1) c) x
                  + count_occ datum_eq_dec (map cd_rec (filter (fun cd => chan_eqb (cd_chan cd) c) F)) x.
Proof.
  induction F as [| cd rest IH]; intros T1 Hall.
  - exists T1. split; [reflexivity |]. intros c x. cbn [filter map count_occ]. lia.
  - destruct (Hall cd (or_introl eq_refl)) as (Hni & Hrec).
    assert (Hst1 : exists T1', stage T1 (restore_of cd) = Some T1' /\
                     t_installed T1' = t_installed T1 /\
                     forall c, vdata (t_view T1') c =
                               if chan_eq_dec c (cd_chan cd) then cd_rec cd :: vdata (t_view T1) c
                               else vdata (t_view T1) c).
    { eexists. split; [unfold stage, restore_of; rewrite Hni; reflexivity |].
      split; [reflexivity |].
      intros c. cbn [t_view txn_push]. unfold vput_datum. rewrite vdata_set, Hrec.
      destruct (chan_eq_dec c (cd_chan cd)) as [E | N]; [subst c |]; reflexivity. }
    destruct Hst1 as (T1' & H1 & Hi1 & Hd1).
    destruct (IH T1') as (T2 & H2 & Hd2).
    { intros cd' Hin'. rewrite Hi1. apply Hall. right. exact Hin'. }
    exists T2. split; [cbn [map stage_all]; rewrite H1; exact H2 |].
    intros c x. rewrite Hd2, Hd1.
    destruct (chan_eq_dec c (cd_chan cd)) as [E | N].
    + subst c. cbn [filter]. rewrite chan_eqb_refl. cbn [map count_occ].
      destruct (datum_eq_dec (cd_rec cd) x); lia.
    + cbn [filter]. rewrite (chan_eqb_neq (cd_chan cd) c) by congruence. reflexivity.
Qed.

(* H11 (decision 45, Rule E): a consume-side peek fire followed by its
   re-stores leaves each data list a permutation of the list before the
   fire. The fire removes each non-persistent record, and the re-store puts
   the same record back, so no record is lost and none is duplicated. The
   host has the same net effect: its COMM removes the records and
   produce_peeks produces them again. With the retention of FR-10, the
   re-store would duplicate each record. So the substrate change and the
   agent change must land together (design report, recommendation 2). *)
Lemma peek_restore_keeps_data :
  forall T chs pats k persist peeks cands T1,
    stage T (OConsume chs pats k persist peeks (CFire cands)) = Some T1 ->
    peeks <> [] -> stored_sources (t_view T) ->
    exists T2,
      stage_all T1 (restores_after (OConsume chs pats k persist peeks (CFire cands))) = Some T2 /\
      forall c, Permutation (vdata (t_view T2) c) (vdata (t_view T) c).
Proof.
  intros T chs pats k persist peeks cands T1 Hst Hpk Hsrc.
  pose proof (fire_view_removes T _ T1 Hst eq_refl) as Hrm. cbn [fire_cands] in Hrm.
  assert (Hall : forall cd, In cd (filter removable cands) ->
            t_installed T1 (cd_chan cd) = false /\
            mkDatum (d_val (cd_rec cd)) false (produce_id (cd_chan cd) (d_val (cd_rec cd)) false) = cd_rec cd).
  { intros cd Hin. apply filter_In in Hin as [Hin Hr].
    destruct (consume_fire_removed T chs pats k persist peeks cands T1 Hst Hsrc cd Hin Hr)
      as (Hni & _ & Hrec).
    split; [exact Hni | exact Hrec]. }
  destruct (restores_stage (filter removable cands) T1 Hall) as (T2 & H2 & Hd2).
  exists T2. split.
  - unfold restores_after. destruct peeks as [| p ps]; [contradiction |]. cbn [nullb]. exact H2.
  - intros c. apply (Permutation_count_occ datum_eq_dec). intros x.
    rewrite Hd2. specialize (Hrm c x).
    rewrite <- (filter_and Cand removable (fun cd => chan_eqb (cd_chan cd) c) cands) in *.
    unfold np_recs in Hrm. lia.
Qed.

(* ── stored_sources is an invariant of staging ───────────────────────── *)

Lemma vremove_datum_incl :
  forall V c0 d0 c d, In d (vdata (vremove_datum V c0 d0) c) -> In d (vdata V c).
Proof.
  intros V c0 d0 c d Hin. unfold vremove_datum in Hin. rewrite vdata_set in Hin.
  destruct (chan_eq_dec c c0) as [E | N]; [subst c; eapply remove_first_incl; exact Hin | exact Hin].
Qed.

Lemma vremove_join_data : forall V c0 g c, vdata (vremove_join V c0 g) c = vdata V c.
Proof. intros V c0 g c. unfold vremove_join. destruct (vconts V g); reflexivity. Qed.

Lemma vpf_loop_incl :
  forall cs V chs c d, In d (vdata (vpf_loop V chs cs) c) -> In d (vdata V c).
Proof.
  induction cs as [| cd rest IH]; intros V chs c d Hin; cbn [vpf_loop] in Hin; [exact Hin |].
  apply IH in Hin. rewrite vremove_join_data in Hin.
  destruct (removable cd); [eapply vremove_datum_incl; exact Hin | exact Hin].
Qed.

Lemma vcf_loop_incl :
  forall cs V c d, In d (vdata (vcf_loop V cs) c) -> In d (vdata V c).
Proof.
  induction cs as [| cd rest IH]; intros V c d Hin; cbn [vcf_loop] in Hin; [exact Hin |].
  apply IH in Hin.
  destruct (negb (d_persist (cd_rec cd))); [eapply vremove_datum_incl; exact Hin | exact Hin].
Qed.

Lemma vput_joins_data : forall cs V g c, vdata (vput_joins V cs g) c = vdata V c.
Proof.
  induction cs as [| c0 rest IH]; intros V g c; cbn [vput_joins]; [reflexivity |].
  rewrite IH. unfold vput_join. destruct (in_dec lc_eq_dec g (vjoins V c0)); reflexivity.
Qed.

Lemma vstore_wk_data : forall V chs w c, vdata (vstore_wk V chs w) c = vdata V c.
Proof.
  intros V chs w c. unfold vstore_wk. rewrite vput_joins_data.
  unfold vput_continuation. destruct (existsb (ident_eqb w) (vconts V chs)); reflexivity.
Qed.

(* H11 (decision 45, Rule E): staging keeps stored_sources. A staged Store
   stores the Produce::create identity of its record, and every other
   effect removes records or leaves the data alone. So the premise of
   restore_same_identity holds for each view that staging reaches from a
   store whose records came from produces. *)
Lemma stage_keeps_stored_sources :
  forall T op T1, stage T op = Some T1 -> stored_sources (t_view T) -> stored_sources (t_view T1).
Proof.
  intros T op T1 Hst Hsrc c d Hin. unfold stored_sources in Hsrc.
  destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]];
    unfold stage in Hst.
  - destruct (t_installed T ch); [discriminate |]. injection Hst as <-.
    cbn [t_view txn_push] in Hin. unfold vput_datum in Hin. rewrite vdata_set in Hin.
    destruct (chan_eq_dec c ch) as [E | N].
    + subst c. destruct Hin as [<- | Hin]; [reflexivity |].
      apply Hsrc. unfold vbump in Hin. destruct persist; exact Hin.
    + apply Hsrc. unfold vbump in Hin. destruct persist; exact Hin.
  - destruct (t_installed T ch); [discriminate |].
    destruct (pfire_ok T ch v persist chs wk cands); cbn [negb] in Hst; [| discriminate].
    apply stage_fire_some in Hst. subst T1. cbn [t_view txn_push] in Hin.
    apply Hsrc. unfold vpf_effects in Hin. cbv zeta in Hin. apply vpf_loop_incl in Hin.
    destruct (w_persist wk); unfold vbump in Hin; destruct persist; exact Hin.
  - destruct (consume_args_ok T chs pats peeks); cbn [negb] in Hst; [| discriminate].
    injection Hst as <-. cbn [t_view txn_push] in Hin.
    apply Hsrc. rewrite vstore_wk_data in Hin. exact Hin.
  - destruct (consume_args_ok T chs pats peeks); cbn [negb] in Hst; [| discriminate].
    destruct (cfire_ok T chs cands); cbn [negb] in Hst; [| discriminate].
    apply stage_fire_some in Hst. subst T1. cbn [t_view txn_push] in Hin.
    apply Hsrc. unfold vcf_effects in Hin. apply vcf_loop_incl in Hin. exact Hin.
Qed.

(* ── FR-10 on a head-only waiter list (p4-design-report B.4) ─────────── *)

(* A queue space matches only its head waiter and its head record (plan
   decision 11). [wmatch w d] stands for the matcher calls get and
   check_commit of the waiter w on the record d. The no-pending-match
   invariant (INV of the P4 design) holds when the head waiter does not
   match the head record. *)
Definition no_pending_match (wmatch : WK -> Datum -> bool) (ws : list WK) (rs : list Datum) : Prop :=
  match ws, rs with
  | w :: _, r :: _ => wmatch w r = false
  | _, _ => True
  end.

Lemma no_pending_match_nil_r : forall wmatch ws, no_pending_match wmatch ws [].
Proof. intros wmatch [| w ws]; exact I. Qed.

Lemma no_pending_match_nil_l : forall wmatch rs, no_pending_match wmatch [] rs.
Proof. intros wmatch [| r rs]; exact I. Qed.

(* offer is the produce plan of a head-only queue for a datum d
   (p4-design-report B.4), with FR-10. A stored record blocks the waiters,
   so the datum goes to the tail. Otherwise the head waiter fires when it
   matches. A head that does not match blocks, and the plan stores the
   datum.

   A fire consumes d when d is not persistent and the waiter does not peek.
   Otherwise d survives. A persistent waiter stays, so the plan re-issues d
   in a new transaction (A10, a divergent pair). A waiter that is not
   persistent leaves, and the plan re-offers d to the next waiter as the
   next operation (FR-10 when the waiter peeks). *)
Fixpoint offer (wmatch : WK -> Datum -> bool) (ws : list WK) (rs : list Datum) (d : Datum)
    {struct ws} : list WK * list Datum :=
  match rs with
  | _ :: _ => (ws, rs ++ [d])
  | [] =>
      match ws with
      | [] => ([], [d])
      | w :: ws' =>
          if wmatch w d then
            if negb (d_persist d) && nullb (w_peeks w) then ((if w_persist w then ws else ws'), [])
            else if w_persist w then (ws, [])
            else offer wmatch ws' [] d
          else (ws, [d])
      end
  end.

(* H11 (FR-10): the plan with the re-offer keeps the no-pending-match
   invariant of a head-only waiter list, for every datum. *)
Lemma offer_keeps_no_pending_match :
  forall wmatch ws rs d, no_pending_match wmatch ws rs ->
    no_pending_match wmatch (fst (offer wmatch ws rs d)) (snd (offer wmatch ws rs d)).
Proof.
  intros wmatch ws. induction ws as [| w ws' IH]; intros rs d Hinv.
  - destruct rs as [| r rs']; exact I.
  - destruct rs as [| r rs']; [| exact Hinv].
    cbn [offer].
    destruct (wmatch w d) eqn:Em; [| exact Em].
    destruct (negb (d_persist d) && nullb (w_peeks w)); [exact (no_pending_match_nil_r wmatch _) |].
    destruct (w_persist w); [exact I |].
    apply IH. exact (no_pending_match_nil_r wmatch _).
Qed.

(* offer_p1 is the same plan under the P1 rule before FR-10. The fire of a
   peek waiter stored a non-persistent incoming datum, so the plan stopped
   there. *)
Fixpoint offer_p1 (wmatch : WK -> Datum -> bool) (ws : list WK) (rs : list Datum) (d : Datum)
    {struct ws} : list WK * list Datum :=
  match rs with
  | _ :: _ => (ws, rs ++ [d])
  | [] =>
      match ws with
      | [] => ([], [d])
      | w :: ws' =>
          if wmatch w d then
            if negb (d_persist d) && nullb (w_peeks w) then ((if w_persist w then ws else ws'), [])
            else if negb (d_persist d) then ((if w_persist w then ws else ws'), [d])
            else if w_persist w then (ws, [])
            else offer_p1 wmatch ws' [] d
          else (ws, [d])
      end
  end.

(* This lemma runs the queue scenario of FR-10 under the P1 rule: a peek
   waiter w1, then a waiter w2 that matches the datum. The fire of w1 stores
   the datum, and w2 then matches it. This is a pending match, the reason
   for P4 arbitration item A1. *)
Lemma offer_p1_breaks_no_pending_match :
  forall wmatch w1 w2 d,
    d_persist d = false -> w_persist w1 = false -> w_peeks w1 <> [] ->
    wmatch w1 d = true -> wmatch w2 d = true ->
    no_pending_match wmatch [w1; w2] [] /\
    offer_p1 wmatch [w1; w2] [] d = ([w2], [d]) /\
    ~ no_pending_match wmatch [w2] [d].
Proof.
  intros wmatch w1 w2 d Hd Hp1 Hk1 Hm1 Hm2.
  split; [exact I |]. split.
  - simpl. rewrite Hm1, Hd, Hp1.
    destruct (w_peeks w1) as [| x xs] eqn:E; [exfalso; apply Hk1; reflexivity | reflexivity].
  - simpl. rewrite Hm2. discriminate.
Qed.

(* offer_ops gives the operations that the plan stages for a send of v on
   the queue channel ch. The plan stages a fire for each waiter that the
   datum reaches. When the datum stays, the plan then stages a store.
   [kidx_of] and [cands_of] give the host index and the candidates of each
   fire. *)
Fixpoint offer_ops (wmatch : WK -> Datum -> bool) (kidx_of : WK -> nat)
    (cands_of : WK -> list Cand) (ch : Chan) (v : Val) (persist : bool)
    (ws : list WK) (rs : list Datum) {struct ws} : list Op :=
  match rs with
  | _ :: _ => [OProduce ch v persist PStore]
  | [] =>
      match ws with
      | [] => [OProduce ch v persist PStore]
      | w :: ws' =>
          if wmatch w (mkDatum v persist (produce_id ch v persist)) then
            let fire := OProduce ch v persist (PFire [ch] w (kidx_of w) (cands_of w)) in
            if negb persist && nullb (w_peeks w) then [fire]
            else if w_persist w then [fire]
            else fire :: offer_ops wmatch kidx_of cands_of ch v persist ws' []
          else [OProduce ch v persist PStore]
      end
  end.

Lemma offer_ops_head :
  forall wmatch kidx_of cands_of ch v persist ws rs,
    exists out rest,
      offer_ops wmatch kidx_of cands_of ch v persist ws rs = OProduce ch v persist out :: rest.
Proof.
  intros wmatch kidx_of cands_of ch v persist ws rs.
  destruct ws as [| w ws']; destruct rs as [| r rs']; try (eexists; eexists; reflexivity).
  simpl. destruct (wmatch w (mkDatum v persist (produce_id ch v persist)));
    [| eexists; eexists; reflexivity].
  destruct (negb persist && nullb (w_peeks w)); [eexists; eexists; reflexivity |].
  destruct (w_persist w); eexists; eexists; reflexivity.
Qed.

(* H11 (FR-10): the plan stages each due re-offer as the next operation. *)
Lemma offer_ops_reoffers :
  forall wmatch kidx_of cands_of ch v persist ws rs,
    reoffers (offer_ops wmatch kidx_of cands_of ch v persist ws rs).
Proof.
  intros wmatch kidx_of cands_of ch v persist ws. induction ws as [| w ws' IH]; intros rs.
  - destruct rs; simpl; (split; [intros Hd; discriminate | exact I]).
  - destruct rs as [| r rs']; [| simpl; split; [intros Hd; discriminate | exact I]].
    simpl. destruct (wmatch w (mkDatum v persist (produce_id ch v persist)));
      [| simpl; split; [intros Hd; discriminate | exact I]].
    destruct (negb persist && nullb (w_peeks w)) eqn:Ec.
    + simpl. split; [| exact I]. intros Hd.
      apply andb_true_iff in Ec as [Ep Ek]. rewrite Ep, Ek in Hd. simpl in Hd. discriminate.
    + destruct (w_persist w) eqn:Ew.
      * simpl. split; [| exact I]. intros Hd. rewrite Ew, andb_false_r in Hd. discriminate.
      * cbn [reoffers]. split; [| apply IH].
        intros _. destruct (offer_ops_head wmatch kidx_of cands_of ch v persist ws' []) as (out & rest & E).
        rewrite E. simpl. auto.
Qed.

(* H11 (FR-10): this lemma runs the queue scenarios of the FR-10 tests. A
   non-persistent send d meets a peek waiter w1 and then a consuming waiter
   w2. The waiter w1 fires, the re-offer fires w2, and the plan stores
   nothing. With no second waiter, the re-offer stores d. *)
Lemma offer_scenarios :
  forall wmatch kidx_of cands_of ch v w1 w2,
    let d := mkDatum v false (produce_id ch v false) in
    let fire1 := OProduce ch v false (PFire [ch] w1 (kidx_of w1) (cands_of w1)) in
    let fire2 := OProduce ch v false (PFire [ch] w2 (kidx_of w2) (cands_of w2)) in
    w_persist w1 = false -> w_peeks w1 <> [] -> w_persist w2 = false -> w_peeks w2 = [] ->
    wmatch w1 d = true -> wmatch w2 d = true ->
    offer wmatch [w1; w2] [] d = ([], []) /\
    offer_ops wmatch kidx_of cands_of ch v false [w1; w2] [] = [fire1; fire2] /\
    offer wmatch [w1] [] d = ([], [d]) /\
    offer_ops wmatch kidx_of cands_of ch v false [w1] [] = [fire1; OProduce ch v false PStore].
Proof.
  intros wmatch kidx_of cands_of ch v w1 w2 d fire1 fire2 Hp1 Hk1 Hp2 Hk2 Hm1 Hm2.
  subst d fire1 fire2.
  assert (Hn1 : nullb (w_peeks w1) = false)
    by (destruct (w_peeks w1); [exfalso; apply Hk1; reflexivity | reflexivity]).
  repeat split; simpl; rewrite ?Hm1, ?Hn1, ?Hp1; simpl; rewrite ?Hm2, ?Hk2, ?Hp2; reflexivity.
Qed.

(* ═══════════ Commit coherence: the committed store is the view (H6) ═══════ *)

Definition rd_after (m : Mode) (st : Status) (sgc : option Comm) (rd : Bindings) : Bindings :=
  match backend m, st, sgc with
  | Play, _, _ => rd
  | _, StOk, Some cm => unbind rd cm
  | _, _, _ => rd
  end.

Lemma space_prelude_status :
  forall m H s a b obs st, space_prelude m H s a b = PreStop obs st -> st = StRejected.
Proof.
  intros m H s a b obs st E. unfold space_prelude in E.
  destruct (ob_accept H (ObBefore s)); simpl in E; [| inversion E; reflexivity].
  destruct (ob_accept (H ++ [ObBefore s]) a); simpl in E; [| inversion E; reflexivity].
  destruct m.
  - destruct (ob_accept (H ++ [ObBefore s; a]) b); simpl in E; inversion E; reflexivity.
  - destruct (ob_directive (H ++ [ObBefore s; a]) s);
      [destruct (ob_accept (H ++ [ObBefore s; a; ObDirective s]) b); simpl in E | |];
      inversion E; reflexivity.
  - destruct (ob_directive (H ++ [ObBefore s; a]) s);
      [destruct (ob_accept (H ++ [ObBefore s; a; ObDirective s]) b); simpl in E | |];
      inversion E; reflexivity.
Qed.


Lemma backend_cases : forall m, backend m = Play \/ backend m = Replay.
Proof. intros []; simpl; auto. Qed.

(* Commit-side characterization of each outcome. *)

(* The leaf tactics of the commit characterizations below: a committed
   leaf, and a leaf that commits nothing (a rejection by the observer, a
   COMM that does not equal the live COMM, or an unpaired replay fire). *)
Ltac close_ok :=
  left; split; [reflexivity | split; [reflexivity | split; [reflexivity | split; reflexivity]]].

Ltac close_inert :=
  right;
  match goal with
  | E : space_prelude _ _ _ _ _ = PreStop _ ?st |- _ => apply space_prelude_status in E; subst st
  | |- _ => idtac
  end;
  split; [discriminate | split; [discriminate | split; [discriminate | split;
    [first [apply store_eq_refl | apply bump_unbump_eq] | split; [reflexivity | split; reflexivity]]]]].

Lemma coh_produce_store :
  forall m H Sc rd ch v persist j0 sp,
    let src := produce_id ch v persist in
    let r := commit_op m H Sc rd (mkStaged (OProduce ch v persist PStore) j0 None sp) in
    (r_status r = StOk /\
     r_st r = fst (put_datum (bump Sc src persist) ch (mkDatum v persist src)) /\
     r_rd r = rd /\ r_ev r = [EvProduce src] /\
     r_fx r = snd (put_datum (bump Sc src persist) ch (mkDatum v persist src)))
    \/ (r_status r = StRejected /\ r_st r = Sc /\ r_rd r = rd /\ r_ev r = [] /\ r_fx r = []).
Proof.
  intros m H Sc rd ch v persist j0 sp src r. subst src r. unfold commit_op. simpl.
  destruct (backend m).
  - unfold commit_play_produce.
    destruct (space_prelude Play _ _ _ _) as [obs st | obs] eqn:Ep.
    + right. apply space_prelude_status in Ep. subst st. simpl. auto.
    + left. simpl. auto.
  - unfold commit_replay_produce.
    destruct (space_prelude Replay _ _ _ _) as [obs st | obs] eqn:Ep.
    + right. apply space_prelude_status in Ep. subst st. simpl. auto.
    + left. simpl. auto.
  - unfold commit_replay_produce.
    destruct (space_prelude Replay _ _ _ _) as [obs st | obs] eqn:Ep.
    + right. apply space_prelude_status in Ep. subst st. simpl. auto.
    + left. simpl. auto.
Qed.

Lemma coh_consume_park :
  forall m H Sc rd chs pats k persist peeks sp,
    let src := consume_id chs pats k persist in
    let r := commit_op m H Sc rd (mkStaged (OConsume chs pats k persist peeks CPark) [] None sp) in
    (r_status r = StOk /\
     r_st r = fst (store_waiting_continuation Sc chs (mkWK pats k persist peeks src)) /\
     r_rd r = rd /\ r_ev r = [EvConsume src] /\
     r_fx r = snd (store_waiting_continuation Sc chs (mkWK pats k persist peeks src)))
    \/ (r_status r = StRejected /\ r_st r = Sc /\ r_rd r = rd /\ r_ev r = [] /\ r_fx r = []).
Proof.
  intros m H Sc rd chs pats k persist peeks sp src r. subst src r. unfold commit_op. simpl.
  destruct (backend m).
  - unfold commit_play_consume.
    destruct (space_prelude Play _ _ _ _) as [obs st | obs] eqn:Ep.
    + right. apply space_prelude_status in Ep. subst st. simpl. auto.
    + left. simpl. auto.
  - unfold commit_replay_consume.
    destruct (space_prelude Replay _ _ _ _) as [obs st | obs] eqn:Ep.
    + right. apply space_prelude_status in Ep. subst st. simpl. auto.
    + left. simpl. auto.
  - unfold commit_replay_consume.
    destruct (space_prelude Replay _ _ _ _) as [obs st | obs] eqn:Ep.
    + right. apply space_prelude_status in Ep. subst st. simpl. auto.
    + left. simpl. auto.
Qed.

(* A staged produce-side fire commits with its view effects, or it commits
   nothing: a rejection, or a replay fire that staged unpaired
   (StDiverged, decision 36). The statement does not depend on the pairing
   flag. *)
Lemma coh_produce_fire :
  forall m H Sc rd ch v persist chs wk kidx cands j0 cm sp,
    let src := produce_id ch v persist in
    cm = mk_comm cands (w_src wk) (w_peeks wk) (play_counters_closure (s_count Sc) src persist) ->
    let r := commit_op m H Sc rd
               (mkStaged (OProduce ch v persist (PFire chs wk kidx cands)) j0 (Some cm) sp) in
    let S1 := bump Sc src persist in
    let p1 := if w_persist wk then (S1, []) else space_remove_continuation S1 chs wk in
    let p2 := space_pf_loop (fst p1) chs (sort_desc vkey cands) in
    (r_status r = StOk /\ r_st r = fst p2 /\ r_rd r = rd_after m StOk (Some cm) rd /\
     r_ev r = [EvProduce src; EvComm ProduceSide cm] /\ r_fx r = snd p1 ++ snd p2)
    \/ (r_status r <> StOk /\ r_status r <> StPanic /\ r_status r <> StNative /\
        store_eq (r_st r) Sc /\ r_rd r = rd /\ r_ev r = [] /\ r_fx r = []).
Proof.
  intros m H Sc rd ch v persist chs wk kidx cands j0 cm sp src Hcm r S1 p1 p2.
  subst r. unfold commit_op. cbn [sg_op sg_joins0 sg_comm sg_paired]. unfold rd_after.
  destruct (backend m) eqn:Hb;
    [unfold commit_play_produce | unfold commit_replay_produce | unfold commit_replay_produce];
    cbn zeta; commit_split; cbn [r_status r_st r_rd r_ev r_fx]; first [close_ok | close_inert].
Qed.

Lemma coh_consume_fire :
  forall m H Sc rd chs pats k persist peeks cands cm sp,
    let src := consume_id chs pats k persist in
    cm = mk_comm cands src peeks (counters_of (s_count Sc)) ->
    let r := commit_op m H Sc rd (mkStaged (OConsume chs pats k persist peeks (CFire cands)) [] (Some cm) sp) in
    let p := space_cf_loop Sc (sort_desc vkey cands) in
    (r_status r = StOk /\ r_st r = fst p /\ r_rd r = rd_after m StOk (Some cm) rd /\
     r_ev r = [EvConsume src; EvComm ConsumeSide cm] /\ r_fx r = snd p)
    \/ (r_status r <> StOk /\ r_status r <> StPanic /\ r_status r <> StNative /\
        store_eq (r_st r) Sc /\ r_rd r = rd /\ r_ev r = [] /\ r_fx r = []).
Proof.
  intros m H Sc rd chs pats k persist peeks cands cm sp src Hcm r p.
  subst r. unfold commit_op. cbn [sg_op sg_joins0 sg_comm sg_paired]. unfold rd_after.
  destruct (backend m) eqn:Hb;
    [unfold commit_play_consume | unfold commit_replay_consume | unfold commit_replay_consume];
    cbn zeta; commit_split; cbn [r_status r_st r_rd r_ev r_fx]; first [close_ok | close_inert].
Qed.

(* rd_after leaves the bindings as they are when the operation does not
   commit. *)
Lemma rd_after_not_ok : forall m st sgc rd, st <> StOk -> rd_after m st sgc rd = rd.
Proof. intros m st sgc rd Hst. unfold rd_after. destruct (backend m), st, sgc; congruence. Qed.

(* The commit of one staged operation. On success, the store equals the
   view after staging. On rejection, and for an unpaired replay fire
   (decision 36), the store does not change (decision D6). Staging pairs a
   replay fire exactly when verify accepts it, and never in play. *)
Lemma commit_op_coherent :
  forall T op T' H Sc rd,
    stage T op = Some T' ->
    store_eq (view_store (t_view T)) Sc ->
    exists sg,
      t_staged T' = t_staged T ++ [sg] /\ sg_op sg = op /\
      t_mode T' = t_mode T /\ t_installed T' = t_installed T /\ t_rd T' = t_rd T /\
      (sg_comm sg = None -> t_paired T' = t_paired T /\ sg_paired sg = false) /\
      (forall cm, sg_comm sg = Some cm ->
         t_paired T' = t_paired T ++ (if sg_paired sg then [cm] else [])
         /\ sg_paired sg = match backend (t_mode T) with
                           | Play => false
                           | _ => verify (t_rd T) (t_paired T) (trigger_key op) cm
                           end
         /\ In (trigger_key op) (keys cm) /\ is_fire op = true) /\
      let rc := commit_op (t_mode T) H Sc rd sg in
      r_status rc <> StPanic /\ r_status rc <> StNative /\
      r_rd rc = rd_after (t_mode T) (r_status rc) (sg_comm sg) rd /\
      (r_status rc = StOk -> store_eq (r_st rc) (view_store (t_view T'))) /\
      (r_status rc <> StOk -> store_eq (r_st rc) Sc /\ r_ev rc = [] /\ r_fx rc = []).
Proof.
  intros T op T' H Sc rd Hst Heq.
  set (V := t_view T) in *.
  pose proof Heq as (Ed & Ec & Ej & En).
  destruct op as [ch v persist out | chs pats k persist peeks out].
  - set (src := produce_id ch v persist).
    destruct out as [| chs wk kidx cands].
    + (* produce, Store *)
      unfold stage in Hst. fold V in Hst. fold src in Hst.
      destruct (t_installed T ch) eqn:Hi; [discriminate |].
      injection Hst as <-.
      exists (mkStaged (OProduce ch v persist PStore) (vjoins V ch) None false).
      unfold txn_push. simpl.
      do 5 (split; [reflexivity |]).
      split; [intros _; split; reflexivity |].
      split; [intros cm Hc; discriminate |].
      destruct (coh_produce_store (t_mode T) H Sc rd ch v persist (vjoins V ch) false)
        as [(Hs & Hst' & Hrd & Hev & Hfx) | (Hs & Hst' & Hrd & Hev & Hfx)];
        fold src in Hs, Hst', Hrd, Hev, Hfx;
        rewrite Hs; (split; [discriminate |]); (split; [discriminate |]).
      * split; [unfold rd_after; destruct (backend (t_mode T)); exact Hrd |].
        split; [| intros C; contradiction].
        intros _. rewrite Hst'.
        apply store_eq_sym.
        eapply store_eq_trans; [apply view_put_datum |].
        apply put_datum_cong.
        eapply store_eq_trans; [apply view_bump |].
        apply bump_cong. exact Heq.
      * split; [unfold rd_after; destruct (backend (t_mode T)); exact Hrd |].
        split; [intros C; discriminate |].
        intros _. rewrite Hst'. split; [apply store_eq_refl | auto].
    + (* produce, Fire *)
      unfold stage in Hst. fold V in Hst. fold src in Hst.
      destruct (t_installed T ch) eqn:Hi; [discriminate |].
      destruct (pfire_ok T ch v persist chs wk cands) eqn:Hok; cbn [negb] in Hst; [| discriminate].
      set (cm := mk_comm cands (w_src wk) (w_peeks wk) (play_counters_closure (vcount V) src persist)) in Hst.
      apply stage_fire_some in Hst. subst T'.
      destruct (pfire_ok_spec T ch v persist chs wk cands Hok)
        as (Hne & Hnoinst & Hmap & Hex & Hinc & Hwk & Hpk & Hso).
      set (sp := snd (pair_step T (KProduce src) cm)).
      exists (mkStaged (OProduce ch v persist (PFire chs wk kidx cands)) (vjoins V ch) (Some cm) sp).
      unfold txn_push. cbn [t_staged t_mode t_installed t_rd t_paired t_view sg_op sg_comm sg_paired].
      do 5 (split; [reflexivity |]).
      split; [intros C; discriminate |].
      split.
      { intros cm' Hcm'. injection Hcm' as <-.
        subst sp. unfold pair_step. cbn [trigger_key]. fold src.
        split; [| split].
        - destruct (backend (t_mode T)); [simpl; rewrite app_nil_r; reflexivity | |];
            destruct (verify (t_rd T) (t_paired T) (KProduce src) cm); simpl;
            first [reflexivity | rewrite app_nil_r; reflexivity].
        - destruct (backend (t_mode T)); [reflexivity | |];
            destruct (verify (t_rd T) (t_paired T) (KProduce src) cm); reflexivity.
        - split; [| reflexivity].
          unfold keys. right. apply in_map. subst cm. apply incoming_src_in_produces; assumption. }
      assert (Hcm : cm = mk_comm cands (w_src wk) (w_peeks wk) (play_counters_closure (s_count Sc) src persist)).
      { subst cm. apply mk_comm_play_ext. intros p. apply En. }
      destruct (coh_produce_fire (t_mode T) H Sc rd ch v persist chs wk kidx cands (vjoins V ch) cm sp Hcm)
        as [(Hs & Hst' & Hrd & Hev & Hfx) | (Hs1 & Hs2 & Hs3 & Hst' & Hrd & Hev & Hfx)].
      * rewrite Hs. split; [discriminate |]. split; [discriminate |].
        split; [exact Hrd |].
        split; [| intros C; contradiction].
        intros _. rewrite Hst'. apply store_eq_sym.
        unfold vpf_effects. fold src.
        set (V0 := vbump V src persist).
        assert (E0 : store_eq (view_store V0) (bump Sc src persist)).
        { eapply store_eq_trans; [apply view_bump | apply bump_cong; exact Heq]. }
        set (V1 := if w_persist wk then V0 else vremove_continuation V0 chs wk).
        assert (E1 : store_eq (view_store V1)
                       (fst (if w_persist wk then (bump Sc src persist, [])
                             else space_remove_continuation (bump Sc src persist) chs wk))).
        { subst V1. destruct (w_persist wk); simpl; [exact E0 |].
          eapply store_eq_trans; [apply view_remove_continuation |].
          apply space_remove_continuation_cong. exact E0. }
        exact (view_pf_loop (sort_desc vkey cands) V1 _ chs E1).
      * split; [exact Hs2 |]. split; [exact Hs3 |].
        split; [rewrite Hrd; symmetry; apply rd_after_not_ok; exact Hs1 |].
        split; [intros C; contradiction |].
        intros _. split; [exact Hst' | auto].
  - set (src := consume_id chs pats k persist).
    destruct out as [| cands].
    + (* consume, Park *)
      unfold stage in Hst. fold V in Hst. fold src in Hst.
      destruct (consume_args_ok T chs pats peeks) eqn:Hok; simpl in Hst; [| discriminate].
      injection Hst as <-.
      exists (mkStaged (OConsume chs pats k persist peeks CPark) [] None false).
      unfold txn_push. simpl.
      do 5 (split; [reflexivity |]).
      split; [intros _; split; reflexivity |].
      split; [intros cm Hc; discriminate |].
      destruct (coh_consume_park (t_mode T) H Sc rd chs pats k persist peeks false)
        as [(Hs & Hst' & Hrd & Hev & Hfx) | (Hs & Hst' & Hrd & Hev & Hfx)];
        fold src in Hs, Hst', Hrd, Hev, Hfx;
        rewrite Hs; (split; [discriminate |]); (split; [discriminate |]).
      * split; [unfold rd_after; destruct (backend (t_mode T)); exact Hrd |].
        split; [| intros C; contradiction].
        intros _. rewrite Hst'. apply store_eq_sym. apply view_store_wk. exact Heq.
      * split; [unfold rd_after; destruct (backend (t_mode T)); exact Hrd |].
        split; [intros C; discriminate |].
        intros _. rewrite Hst'. split; [apply store_eq_refl | auto].
    + (* consume, Fire *)
      unfold stage in Hst. fold V in Hst. fold src in Hst.
      destruct (consume_args_ok T chs pats peeks) eqn:Hok; cbn [negb] in Hst; [| discriminate].
      destruct (cfire_ok T chs cands) eqn:Hcf; cbn [negb] in Hst; [| discriminate].
      set (cm := mk_comm cands src peeks (counters_of (vcount V))) in Hst.
      apply stage_fire_some in Hst. subst T'.
      set (sp := snd (pair_step T (KConsume src) cm)).
      exists (mkStaged (OConsume chs pats k persist peeks (CFire cands)) [] (Some cm) sp).
      unfold txn_push. cbn [t_staged t_mode t_installed t_rd t_paired t_view sg_op sg_comm sg_paired].
      do 5 (split; [reflexivity |]).
      split; [intros C; discriminate |].
      split.
      { intros cm' Hcm'. injection Hcm' as <-.
        subst sp. unfold pair_step. cbn [trigger_key]. fold src.
        split; [| split].
        - destruct (backend (t_mode T)); [simpl; rewrite app_nil_r; reflexivity | |];
            destruct (verify (t_rd T) (t_paired T) (KConsume src) cm); simpl;
            first [reflexivity | rewrite app_nil_r; reflexivity].
        - destruct (backend (t_mode T)); [reflexivity | |];
            destruct (verify (t_rd T) (t_paired T) (KConsume src) cm); reflexivity.
        - split; [| reflexivity].
          unfold keys. left. subst cm. reflexivity. }
      assert (Hcm : cm = mk_comm cands src peeks (counters_of (s_count Sc))).
      { subst cm. apply mk_comm_counters_ext. intros p. apply En. }
      destruct (coh_consume_fire (t_mode T) H Sc rd chs pats k persist peeks cands cm sp Hcm)
        as [(Hs & Hst' & Hrd & Hev & Hfx) | (Hs1 & Hs2 & Hs3 & Hst' & Hrd & Hev & Hfx)].
      * rewrite Hs. split; [discriminate |]. split; [discriminate |].
        split; [exact Hrd |].
        split; [| intros C; contradiction].
        intros _. rewrite Hst'. apply store_eq_sym.
        unfold vcf_effects. apply view_cf_loop. exact Heq.
      * split; [exact Hs2 |]. split; [exact Hs3 |].
        split; [rewrite Hrd; symmetry; apply rd_after_not_ok; exact Hs1 |].
        split; [intros C; contradiction |].
        intros _. split; [exact Hst' | auto].
Qed.

(* ═══════════════ Host simulation: relation-preservation lemmas ═════════════ *)

Definition no_dir_some : Prop := forall h s, ob_directive h s <> DirSome.

Lemma prelude_eq :
  no_dir_some -> forall m H s a b, space_prelude m H s a b = host_prelude m H s a b.
Proof.
  intros Hnd m H s a b. unfold space_prelude, host_prelude.
  destruct (ob_accept H (ObBefore s)); [| reflexivity]. simpl.
  destruct (ob_accept (H ++ [ObBefore s]) a); [| reflexivity]. simpl.
  destruct m; [reflexivity | |];
    destruct (ob_directive (H ++ [ObBefore s; a]) s) eqn:E; try reflexivity;
    exfalso; exact (Hnd _ _ E).
Qed.

(* Disabled by decision 45 (Rule E): store_rel has no kept records to permute.
Lemma store_rel_kept_perm :
  forall Sc Sh k1 k2, store_rel Sc Sh k1 -> (forall c, Permutation (k1 c) (k2 c)) ->
    store_rel Sc Sh k2.
Proof.
  intros Sc Sh k1 k2 (Rd & Rc & Rj & Rn) Hk. repeat split; auto.
  intros c. eapply perm_trans; [apply Rd |]. apply Permutation_app_head, Hk.
Qed.
*)

(* Decision 45 (Rule E): store_rel has no kept records, so the relation
   lemmas below have no kept parameter. *)
Lemma rel_bump :
  forall Sc Sh src p, store_rel Sc Sh -> store_rel (bump Sc src p) (bump Sh src p).
Proof.
  intros Sc Sh src p (Rd & Rc & Rj & Rn). unfold bump, upd_count, incr.
  repeat split; intros; simpl; auto. rewrite Rn. reflexivity.
Qed.

Lemma rel_unbump :
  forall Sc Sh src p, store_rel Sc Sh -> store_rel (unbump Sc src p) (unbump Sh src p).
Proof.
  intros Sc Sh src p (Rd & Rc & Rj & Rn). unfold unbump, upd_count, rollback.
  repeat split; intros; simpl; auto. rewrite Rn. reflexivity.
Qed.

Lemma rel_put_datum :
  forall Sc Sh c d, store_rel Sc Sh ->
    store_rel (fst (put_datum Sc c d)) (fst (put_datum Sh c d)).
Proof.
  intros Sc Sh c d (Rd & Rc & Rj & Rn). unfold put_datum, upd_data. simpl.
  repeat split; intros; simpl; auto.
  destruct (chan_eq_dec c0 c) as [E | N]; [subst | apply Rd].
  simpl. apply perm_skip, Rd.
Qed.

Lemma rel_swc :
  forall Sc Sh chs w, store_rel Sc Sh ->
    store_rel (fst (store_waiting_continuation Sc chs w)) (fst (store_waiting_continuation Sh chs w)).
Proof.
  intros Sc Sh chs w (Rd & Rc & Rj & Rn). unfold store_waiting_continuation. simpl.
  destruct (put_continuation_fields Sc chs w) as (Pd & Pc & Pj & Pn).
  destruct (put_continuation_fields Sh chs w) as (Pd' & Pc' & Pj' & Pn').
  destruct (put_joins_fields chs (fst (put_continuation Sc chs w)) chs) as (Qd & Qc & Qj & Qn).
  destruct (put_joins_fields chs (fst (put_continuation Sh chs w)) chs) as (Qd' & Qc' & Qj' & Qn').
  repeat split; intros.
  - rewrite Qd, Qd', Pd, Pd'. apply Rd.
  - rewrite Qc, Qc', Pc, Pc', !Rc. reflexivity.
  - rewrite Qj, Qj', !Pj, !Pj', !Rj. reflexivity.
  - rewrite Qn, Qn', Pn, Pn'. apply Rn.
Qed.

Lemma rel_remove_continuation :
  forall Sc Sh g w i,
    store_rel Sc Sh -> In w (s_conts Sc g) -> nth_error (s_conts Sh g) i = Some w ->
    NoDup (s_conts Sh g) ->
    store_rel (fst (space_remove_continuation Sc g w)) (fst (host_remove_continuation Sh g i)).
Proof.
  intros Sc Sh g w i (Rd & Rc & Rj & Rn) Hin Hnth Hnd.
  unfold space_remove_continuation, host_remove_continuation.
  destruct (in_dec wk_eq_dec w (s_conts Sc g)) as [_ | Cn]; [| contradiction].
  rewrite Hnth. unfold upd_conts. simpl.
  repeat split; intros; simpl; auto.
  destruct (lc_eq_dec g0 g) as [E | N]; [subst | apply Rc].
  rewrite Rc. symmetry. apply remove_nth_eq_remove_first; assumption.
Qed.

Lemma rel_nonpersist_cont :
  forall Sc Sh (persist : bool) g w i,
    store_rel Sc Sh -> In w (s_conts Sc g) ->
    (persist = false -> nth_error (s_conts Sh g) i = Some w) -> NoDup (s_conts Sh g) ->
    store_rel (fst (if persist then (Sc, []) else space_remove_continuation Sc g w))
              (fst (if persist then (Sh, []) else host_remove_continuation Sh g i)).
Proof.
  intros Sc Sh persist g w i R Hin Hnth Hnd.
  destruct persist; simpl; [exact R |].
  apply rel_remove_continuation; auto.
Qed.

Lemma host_rm_valid_data :
  forall S S' cs, (forall c, s_data S' c = s_data S c) -> host_rm_valid S cs -> host_rm_valid S' cs.
Proof.
  intros S S' cs E [Hv Hnd]. split; [| exact Hnd].
  intros cd Hin Hr. rewrite E. apply Hv; assumption.
Qed.

(* Disabled by decision 45 (Rule E): the substrate loop no longer retains, so no kept records arise.
(* The data arithmetic that relates the two removal loops. The substrate
   loop removes the records R unless it retains them, and the host loop
   always removes them. FR-10: neither loop adds a record, because a fire
   never stores its incoming datum. *)
Lemma rel_after_loops :
  forall Sc1 Sh1 Sc2 Sh2 kept (retain : bool) (R : Chan -> list Datum),
    store_rel Sc1 Sh1 kept ->
    (forall c x, count_occ datum_eq_dec (s_data Sc2 c) x =
                 count_occ datum_eq_dec (s_data Sc1 c) x
                 - (if retain then 0 else count_occ datum_eq_dec (R c) x)) ->
    (forall c x, count_occ datum_eq_dec (s_data Sh2 c) x + count_occ datum_eq_dec (R c) x =
                 count_occ datum_eq_dec (s_data Sh1 c) x) ->
    (forall g, s_conts Sc2 g = s_conts Sh2 g) ->
    (forall c, s_joins Sc2 c = s_joins Sh2 c) ->
    (forall p, s_count Sc2 p = s_count Sh2 p) ->
    store_rel Sc2 Sh2 (fun c => kept c ++ (if retain then R c else [])).
Proof.
  intros Sc1 Sh1 Sc2 Sh2 kept retain R (Rd & Rc & Rj & Rn) Hc Hh Ec Ej En.
  repeat split; auto.
  intros c. apply (Permutation_count_occ datum_eq_dec). intros x.
  specialize (Rd c). apply (Permutation_count_occ datum_eq_dec) with (x := x) in Rd.
  rewrite count_occ_app in Rd.
  rewrite Hc, !count_occ_app, Rd.
  specialize (Hh c x).
  destruct retain; simpl; lia.
Qed.
*)

(* The data arithmetic that relates the two removal loops. Decision 45
   (Rule E): the substrate loop removes the records R, as the host loop
   does, also for a peek. FR-10: neither loop adds a record, because a fire
   never stores its incoming datum. *)
Lemma rel_after_loops :
  forall Sc1 Sh1 Sc2 Sh2 (R : Chan -> list Datum),
    store_rel Sc1 Sh1 ->
    (forall c x, count_occ datum_eq_dec (s_data Sc2 c) x =
                 count_occ datum_eq_dec (s_data Sc1 c) x - count_occ datum_eq_dec (R c) x) ->
    (forall c x, count_occ datum_eq_dec (s_data Sh2 c) x + count_occ datum_eq_dec (R c) x =
                 count_occ datum_eq_dec (s_data Sh1 c) x) ->
    (forall g, s_conts Sc2 g = s_conts Sh2 g) ->
    (forall c, s_joins Sc2 c = s_joins Sh2 c) ->
    (forall p, s_count Sc2 p = s_count Sh2 p) ->
    store_rel Sc2 Sh2.
Proof.
  intros Sc1 Sh1 Sc2 Sh2 R (Rd & Rc & Rj & Rn) Hc Hh Ec Ej En.
  repeat split; auto.
  intros c. apply (Permutation_count_occ datum_eq_dec). intros x.
  specialize (Rd c). apply (Permutation_count_occ datum_eq_dec) with (x := x) in Rd.
  rewrite Hc, Rd. specialize (Hh c x). lia.
Qed.

(* ═════════════════ Host simulation of one operation ═══════════════════════ *)

(* The host program is a genuine host execution. The indices of the
   matcher are valid in the store of the host. *)
Definition host_valid_op (S : Store) (op : Op) : Prop :=
  match op with
  | OProduce _ _ _ (PFire chs wk kidx cands) =>
      (w_persist wk = false -> nth_error (s_conts S chs) kidx = Some wk) /\ host_rm_valid S cands
  | OConsume _ _ _ _ _ (CFire cands) => host_rm_valid S cands
  | _ => True
  end.

(* Disabled by decision 45 (Rule E): no operation keeps a record, so the kept records never grow.
(* kept_after gives the kept records after an operation: the records before
   it and the stored records that it retains (retained, FR-10). *)
Definition kept_after (op : Op) (kept : Chan -> list Datum) : Chan -> list Datum :=
  fun c => kept c ++ retained op c.
*)

(* In replay, the host asserts that the recorded bindings contain the COMM
   (replay_rspace.rs:1026-1033, 1396-1401).  The run-level theorems discharge
   this from the binding invariant. *)
Definition bound_ok (T : Txn) (op : Op) (rd : Bindings) : Prop :=
  backend (t_mode T) <> Play ->
  forall cm, verify (t_rd T) (t_paired T) (trigger_key op) cm = true -> In (trigger_key op) (keys cm) ->
  bound rd (trigger_key op) cm = true.

Lemma count_occ_cons_split :
  forall (A : Type) (eqd : forall x y : A, {x = y} + {x <> y}) a l x,
    count_occ eqd (a :: l) x = (if eqd a x then 1 else 0) + count_occ eqd l x.
Proof. intros. simpl. destruct (eqd a x); reflexivity. Qed.

(* Disabled by decision 45 (Rule E): op_sim has five conjuncts, so split5 replaces this tactic.
Ltac split6 := split; [| split; [| split; [| split; [| split]]]].
*)
Ltac split5 := split; [| split; [| split; [| split]]].

Lemma op_sim :
  forall T op T' H Sc Sh rd,
    no_dir_some ->
    stage T op = Some T' ->
    store_eq (view_store (t_view T)) Sc ->
    store_rel Sc Sh ->
    WF Sc ->
    host_valid_op Sh op ->
    bound_ok T op rd ->
    (forall sg, t_staged T' = t_staged T ++ [sg] -> fire_paired (t_mode T) sg) ->
    exists sg,
      t_staged T' = t_staged T ++ [sg] /\
      let rc := commit_op (t_mode T) H Sc rd sg in
      let rh := host_op (t_mode T) H Sh rd op in
      r_obs rc = r_obs rh /\ r_ev rc = r_ev rh /\ r_status rc = r_status rh /\
      r_rd rc = r_rd rh /\ store_rel (r_st rc) (r_st rh).
Proof.
  intros T op T' H Sc Sh rd Hnd Hst Heq Hrel Hwf Hhv Hbo Hfp.
  pose proof Heq as (Ed & Ec & Ej & En).
  pose proof Hrel as (Rd & Rc & Rj & Rn).
  assert (Hpre : forall m s a b, space_prelude m H s a b = host_prelude m H s a b)
    by (intros; apply prelude_eq; exact Hnd).
  assert (Hstop : forall m s a b obs st, host_prelude m H s a b = PreStop obs st -> st = StRejected).
  { intros m s a b obs st E. rewrite <- Hpre in E. eapply space_prelude_status. exact E. }
  destruct op as [ch v persist out | chs pats k persist peeks out].
  - set (src := produce_id ch v persist).
    assert (Hj0 : vjoins (t_view T) ch = s_joins Sh ch).
    { change (vjoins (t_view T) ch) with (s_joins (view_store (t_view T)) ch).
      rewrite Ej. apply Rj. }
    destruct out as [| chs wk kidx cands].
    + (* produce, Store *)
      unfold stage in Hst. fold src in Hst.
      destruct (t_installed T ch) eqn:Hi; [discriminate |].
      injection Hst as <-.
      eexists. split; [reflexivity |]. cbv zeta.
      unfold commit_op, host_op. simpl. rewrite Hj0.
      assert (Hok : store_rel (fst (put_datum (bump Sc src persist) ch (mkDatum v persist src)))
                              (fst (put_datum (bump Sh src persist) ch (mkDatum v persist src))))
        by (apply rel_put_datum, rel_bump, Hrel).
      destruct (backend (t_mode T)).
      * unfold commit_play_produce, host_play_produce. fold src. rewrite Hpre.
        destruct (host_prelude Play H (SrcP src) _ _) as [obs st | obs] eqn:Ep.
        -- apply Hstop in Ep. subst st. simpl.
           split5; try reflexivity; exact Hrel.
        -- simpl. split5; try reflexivity; exact Hok.
      * unfold commit_replay_produce, host_replay_produce. fold src. rewrite Hpre.
        destruct (host_prelude Replay H (SrcP src) _ _) as [obs st | obs] eqn:Ep.
        -- apply Hstop in Ep. subst st. simpl.
           split5; try reflexivity; exact Hrel.
        -- simpl. split5; try reflexivity; exact Hok.
      * unfold commit_replay_produce, host_replay_produce. fold src. rewrite Hpre.
        destruct (host_prelude Replay H (SrcP src) _ _) as [obs st | obs] eqn:Ep.
        -- apply Hstop in Ep. subst st. simpl.
           split5; try reflexivity; exact Hrel.
        -- simpl. split5; try reflexivity; exact Hok.
    + (* produce, Fire *)
      pose proof (commit_op_coherent T _ T' H Sc rd Hst Heq) as (sg0 & Hsg0 & _ & _ & _ & _ & _ & Hpair0 & _).
      pose proof (Hfp sg0 Hsg0) as Hfp0. unfold fire_paired in Hfp0.
      unfold stage in Hst. fold src in Hst.
      destruct (t_installed T ch) eqn:Hi; [discriminate |].
      destruct (pfire_ok T ch v persist chs wk cands) eqn:Hok; cbn [negb] in Hst; [| discriminate].
      set (cm := mk_comm cands (w_src wk) (w_peeks wk) (play_counters_closure (vcount (t_view T)) src persist)) in Hst.
      apply stage_fire_some in Hst. subst T'.
      cbn [t_staged txn_push] in Hsg0. apply app_inj_tail in Hsg0 as [_ Hsg0]. subst sg0.
      destruct (Hpair0 cm eq_refl) as (_ & Hsp & Htrig & _).
      cbn [sg_paired sg_op is_fire] in Hsp, Hfp0.
      set (sp := snd (pair_step T (KProduce src) cm)) in *. clearbody sp.
      (* In replay the staged fire paired: verify accepted it (decision 36). *)
      assert (Hver : backend (t_mode T) <> Play ->
                     verify (t_rd T) (t_paired T) (trigger_key (OProduce ch v persist (PFire chs wk kidx cands))) cm = true
                     /\ sp = true).
      { intros Hnp. specialize (Hfp0 Hnp eq_refl). split; [| exact Hfp0].
        rewrite Hfp0 in Hsp. destruct (backend (t_mode T)); [contradiction | symmetry; exact Hsp | symmetry; exact Hsp]. }
      destruct (pfire_ok_spec T ch v persist chs wk cands Hok)
        as (Hne & Hnoinst & Hmap & Hex & Hinc & Hwk & Hpk & Hso).
      destruct Hhv as [Hkidx Hhrv].
      eexists. split; [reflexivity |]. cbv zeta.
      assert (Hcm_c : mk_comm cands (w_src wk) (w_peeks wk) (play_counters_closure (s_count Sc) src persist) = cm).
      { subst cm. apply mk_comm_play_ext. intros p. symmetry. apply En. }
      assert (Hcm_h : mk_comm cands (w_src wk) (w_peeks wk) (play_counters_closure (s_count Sh) src persist) = cm).
      { rewrite <- Hcm_c. apply mk_comm_play_ext. intros p. symmetry. apply Rn. }
      assert (Hcm_rc : mk_comm cands (w_src wk) (w_peeks wk) (counters_of (s_count (bump Sc src persist))) = cm).
      { rewrite <- Hcm_c. unfold mk_comm. f_equal. rewrite play_closure_is_replay_closure. reflexivity. }
      assert (Hcm_rh : mk_comm cands (w_src wk) (w_peeks wk) (counters_of (s_count (bump Sh src persist))) = cm).
      { rewrite <- Hcm_h. unfold mk_comm. f_equal. rewrite play_closure_is_replay_closure. reflexivity. }
      (* the relation after the accepted COMM *)
      set (S1c := bump Sc src persist). set (S1h := bump Sh src persist).
      set (p1c := if w_persist wk then (S1c, []) else space_remove_continuation S1c chs wk).
      set (p1h := if w_persist wk then (S1h, []) else host_remove_continuation S1h chs kidx).
      assert (R1 : store_rel (fst p1c) (fst p1h)).
      { subst p1c p1h. apply rel_nonpersist_cont.
        - apply rel_bump, Hrel.
        - subst S1c. simpl. rewrite <- Ec. exact Hwk.
        - intros Hp. subst S1h. simpl. apply Hkidx, Hp.
        - subst S1h. simpl. rewrite <- Rc. apply WF_conts_nodup, Hwf. }
      assert (D1c : forall c, s_data (fst p1c) c = s_data Sc c).
      { intros c. subst p1c S1c. destruct (w_persist wk); [reflexivity |].
        destruct (space_remove_continuation_data (bump Sc src persist) chs wk) as (X & _). apply X. }
      assert (D1h : forall c, s_data (fst p1h) c = s_data Sh c).
      { intros c. subst p1h S1h. destruct (w_persist wk); [reflexivity |].
        destruct (host_remove_continuation_data (bump Sh src persist) chs kidx) as (X & _). apply X. }
      set (p2c := space_pf_loop (fst p1c) chs (sort_desc vkey cands)).
      set (p2h := host_rm_loop (fst p1h) chs (sort_desc hkey cands)).
      (* Decision 45 (Rule E): the substrate store after the fire is the
         host store, also for a peek fire. FR-10: the incoming datum is in
         neither store. *)
      assert (Rfinal : store_rel (fst p2c) (fst p2h)).
      { destruct (space_pf_loop_spec (sort_desc vkey cands) (fst p1c) chs) as (Ld & Lc & Lj & Ln).
        fold p2c in Ld, Lc, Lj, Ln.
        assert (Hvh : host_rm_valid (fst p1h) (sort_desc hkey cands)).
        { apply host_rm_valid_perm with (cs := cands).
          - apply Permutation_sym, sort_desc_perm.
          - apply host_rm_valid_data with (S := Sh); [exact D1h | exact Hhrv]. }
        destruct (host_rm_loop_spec (sort_desc hkey cands) (fst p1h) chs (sort_desc_sorted _ hkey cands) Hvh)
          as (Hd & Hc & Hj & Hn).
        fold p2h in Hd, Hc, Hj, Hn.
        pose proof R1 as (R1d & R1c & R1j & R1n).
        apply rel_after_loops with (Sc1 := fst p1c) (Sh1 := fst p1h) (R := fun c => np_recs c cands).
        - exact R1.
        - intros c x. cbv beta. rewrite (Ld c x).
          rewrite (np_recs_perm c (sort_desc vkey cands) cands x (sort_desc_perm _ vkey cands)).
          reflexivity.
        - intros c x. cbv beta.
          rewrite <- (np_recs_perm c (sort_desc hkey cands) cands x (sort_desc_perm _ hkey cands)).
          apply Hd.
        - intros g. rewrite Lc, Hc. apply R1c.
        - intros c. rewrite Lj, Hj. unfold rj_closed.
          rewrite R1c, (chan_count_perm c (sort_desc vkey cands) cands (sort_desc_perm _ vkey cands)),
                  (chan_count_perm c (sort_desc hkey cands) cands (sort_desc_perm _ hkey cands)), !R1j.
          reflexivity.
        - intros p. rewrite Ln, Hn. apply R1n. }
      unfold commit_op, host_op. simpl. rewrite Hj0.
      destruct (backend (t_mode T)) eqn:Hb.
      * unfold commit_play_produce, host_play_produce. fold src. rewrite Hpre.
        destruct (host_prelude Play H (SrcP src) _ _) as [obs st | obs] eqn:Ep.
        -- apply Hstop in Ep. subst st. simpl.
           split5; try reflexivity; exact Hrel.
        -- rewrite Hcm_c, Hcm_h, comm_eqb_refl. simpl.
           destruct (ob_accept (H ++ obs) (ObComm cm (w_kont wk) (w_persist wk) (comm_data cands))); simpl.
           ++ split5; try reflexivity; exact Rfinal.
           ++ split5; try reflexivity; exact Hrel.
      * destruct (Hver ltac:(first [discriminate | rewrite Hb; discriminate])) as [Hv Hsp1].
        assert (Hb' : bound rd (KProduce src) cm = true).
        { apply (Hbo ltac:(first [discriminate | rewrite Hb; discriminate]) cm Hv Htrig). }
        unfold commit_replay_produce, host_replay_produce. fold src. rewrite Hpre.
        destruct (host_prelude Replay H (SrcP src) _ _) as [obs st | obs] eqn:Ep.
        -- apply Hstop in Ep. subst st. simpl.
           split5; try reflexivity; exact Hrel.
        -- rewrite Hcm_rc, Hcm_rh, comm_eqb_refl, Hb'. rewrite ?Hsp1. simpl.
           destruct (ob_accept (H ++ obs) (ObComm cm (w_kont wk) (w_persist wk) (comm_data cands))); simpl.
           ++ split5; try reflexivity; exact Rfinal.
           ++ split5; try reflexivity.
              apply rel_unbump, rel_bump, Hrel.
      * destruct (Hver ltac:(first [discriminate | rewrite Hb; discriminate])) as [Hv Hsp1].
        assert (Hb' : bound rd (KProduce src) cm = true).
        { apply (Hbo ltac:(first [discriminate | rewrite Hb; discriminate]) cm Hv Htrig). }
        unfold commit_replay_produce, host_replay_produce. fold src. rewrite Hpre.
        destruct (host_prelude Replay H (SrcP src) _ _) as [obs st | obs] eqn:Ep.
        -- apply Hstop in Ep. subst st. simpl.
           split5; try reflexivity; exact Hrel.
        -- rewrite Hcm_rc, Hcm_rh, comm_eqb_refl, Hb'. rewrite ?Hsp1. simpl.
           destruct (ob_accept (H ++ obs) (ObComm cm (w_kont wk) (w_persist wk) (comm_data cands))); simpl.
           ++ split5; try reflexivity; exact Rfinal.
           ++ split5; try reflexivity.
              apply rel_unbump, rel_bump, Hrel.
  - set (src := consume_id chs pats k persist).
    destruct out as [| cands].
    + (* consume, Park *)
      unfold stage in Hst. fold src in Hst.
      destruct (consume_args_ok T chs pats peeks) eqn:Hok; simpl in Hst; [| discriminate].
      injection Hst as <-.
      eexists. split; [reflexivity |]. cbv zeta.
      unfold commit_op, host_op. simpl.
      assert (Hok' : store_rel (fst (store_waiting_continuation Sc chs (mkWK pats k persist peeks src)))
                               (fst (store_waiting_continuation Sh chs (mkWK pats k persist peeks src))))
        by (apply rel_swc, Hrel).
      destruct (backend (t_mode T)).
      * unfold commit_play_consume, host_play_consume. fold src. rewrite Hpre.
        destruct (host_prelude Play H (SrcC src) _ _) as [obs st | obs] eqn:Ep.
        -- apply Hstop in Ep. subst st. simpl.
           split5; try reflexivity; exact Hrel.
        -- simpl. split5; try reflexivity; exact Hok'.
      * unfold commit_replay_consume, host_replay_consume. fold src. rewrite Hpre.
        destruct (host_prelude Replay H (SrcC src) _ _) as [obs st | obs] eqn:Ep.
        -- apply Hstop in Ep. subst st. simpl.
           split5; try reflexivity; exact Hrel.
        -- simpl. split5; try reflexivity; exact Hok'.
      * unfold commit_replay_consume, host_replay_consume. fold src. rewrite Hpre.
        destruct (host_prelude Replay H (SrcC src) _ _) as [obs st | obs] eqn:Ep.
        -- apply Hstop in Ep. subst st. simpl.
           split5; try reflexivity; exact Hrel.
        -- simpl. split5; try reflexivity; exact Hok'.
    + (* consume, Fire *)
      pose proof (commit_op_coherent T _ T' H Sc rd Hst Heq) as (sg0 & Hsg0 & _ & _ & _ & _ & _ & Hpair0 & _).
      pose proof (Hfp sg0 Hsg0) as Hfp0. unfold fire_paired in Hfp0.
      unfold stage in Hst. fold src in Hst.
      destruct (consume_args_ok T chs pats peeks) eqn:Hok; cbn [negb] in Hst; [| discriminate].
      destruct (cfire_ok T chs cands) eqn:Hcf; cbn [negb] in Hst; [| discriminate].
      set (cm := mk_comm cands src peeks (counters_of (vcount (t_view T)))) in Hst.
      apply stage_fire_some in Hst. subst T'.
      cbn [t_staged txn_push] in Hsg0. apply app_inj_tail in Hsg0 as [_ Hsg0]. subst sg0.
      destruct (Hpair0 cm eq_refl) as (_ & Hsp & Htrig & _).
      cbn [sg_paired sg_op is_fire] in Hsp, Hfp0.
      set (sp := snd (pair_step T (KConsume src) cm)) in *. clearbody sp.
      assert (Hver : backend (t_mode T) <> Play ->
                     verify (t_rd T) (t_paired T) (trigger_key (OConsume chs pats k persist peeks (CFire cands))) cm = true
                     /\ sp = true).
      { intros Hnp. specialize (Hfp0 Hnp eq_refl). split; [| exact Hfp0].
        rewrite Hfp0 in Hsp. destruct (backend (t_mode T)); [contradiction | symmetry; exact Hsp | symmetry; exact Hsp]. }
      destruct (cfire_ok_spec T chs cands Hcf) as (Hmap & Hnoinc & Hso).
      eexists. split; [reflexivity |]. cbv zeta.
      assert (Hcm_c : mk_comm cands src peeks (counters_of (s_count Sc)) = cm).
      { subst cm. apply mk_comm_counters_ext. intros p. symmetry. apply En. }
      assert (Hcm_h : mk_comm cands src peeks (counters_of (s_count Sh)) = cm).
      { rewrite <- Hcm_c. apply mk_comm_counters_ext. intros p. symmetry. apply Rn. }
      set (pc := space_cf_loop Sc (sort_desc vkey cands)).
      set (ph := host_spd_loop Sh (sort_desc hkey cands)).
      assert (Rfinal : store_rel (fst pc) (fst ph)).
      { assert (Hnoinc_v : forall cd, In cd (sort_desc vkey cands) -> cd_incoming cd = false).
        { intros cd Hin. apply Hnoinc. eapply Permutation_in; [apply sort_desc_perm | exact Hin]. }
        assert (Hnoinc_h : forall cd, In cd (sort_desc hkey cands) -> cd_incoming cd = false).
        { intros cd Hin. apply Hnoinc. eapply Permutation_in; [apply sort_desc_perm | exact Hin]. }
        destruct (space_cf_loop_spec (sort_desc vkey cands) Sc Hnoinc_v) as (Ld & Lc & Lj & Ln).
        fold pc in Ld, Lc, Lj, Ln.
        assert (Hvh : host_rm_valid Sh (sort_desc hkey cands)).
        { apply host_rm_valid_perm with (cs := cands); [apply Permutation_sym, sort_desc_perm | exact Hhv]. }
        destruct (host_spd_loop_spec (sort_desc hkey cands) Sh (sort_desc_sorted _ hkey cands) Hnoinc_h Hvh)
          as (Hd & Hc & Hj & Hn).
        fold ph in Hd, Hc, Hj, Hn.
        apply rel_after_loops with (Sc1 := Sc) (Sh1 := Sh) (R := fun c => np_recs c cands).
        - exact Hrel.
        - intros c x. cbv beta. rewrite Ld.
          rewrite (np_recs_perm c (sort_desc vkey cands) cands x (sort_desc_perm _ vkey cands)).
          reflexivity.
        - intros c x. cbv beta.
          rewrite <- (np_recs_perm c (sort_desc hkey cands) cands x (sort_desc_perm _ hkey cands)).
          apply Hd.
        - intros g. rewrite Lc, Hc. apply Rc.
        - intros c. rewrite Lj, Hj. apply Rj.
        - intros p. rewrite Ln, Hn. apply Rn. }
      unfold commit_op, host_op. simpl.
      destruct (backend (t_mode T)) eqn:Hb.
      * unfold commit_play_consume, host_play_consume. fold src. rewrite Hpre.
        destruct (host_prelude Play H (SrcC src) _ _) as [obs st | obs] eqn:Ep.
        -- apply Hstop in Ep. subst st. simpl.
           split5; try reflexivity; exact Hrel.
        -- rewrite Hcm_c, Hcm_h, comm_eqb_refl. simpl.
           destruct (ob_accept (H ++ obs) (ObComm cm k persist (comm_data cands))); simpl.
           ++ split5; try reflexivity; exact Rfinal.
           ++ split5; try reflexivity; exact Hrel.
      * destruct (Hver ltac:(first [discriminate | rewrite Hb; discriminate])) as [Hv Hsp1].
        assert (Hb' : bound rd (KConsume src) cm = true).
        { apply (Hbo ltac:(first [discriminate | rewrite Hb; discriminate]) cm Hv Htrig). }
        unfold commit_replay_consume, host_replay_consume. fold src. rewrite Hpre.
        destruct (host_prelude Replay H (SrcC src) _ _) as [obs st | obs] eqn:Ep.
        -- apply Hstop in Ep. subst st. simpl.
           split5; try reflexivity; exact Hrel.
        -- rewrite Hcm_c, Hcm_h, comm_eqb_refl, Hb'. rewrite ?Hsp1. simpl.
           destruct (ob_accept (H ++ obs) (ObComm cm k persist (comm_data cands))); simpl.
           ++ split5; try reflexivity; exact Rfinal.
           ++ split5; try reflexivity; exact Hrel.
      * destruct (Hver ltac:(first [discriminate | rewrite Hb; discriminate])) as [Hv Hsp1].
        assert (Hb' : bound rd (KConsume src) cm = true).
        { apply (Hbo ltac:(first [discriminate | rewrite Hb; discriminate]) cm Hv Htrig). }
        unfold commit_replay_consume, host_replay_consume. fold src. rewrite Hpre.
        destruct (host_prelude Replay H (SrcC src) _ _) as [obs st | obs] eqn:Ep.
        -- apply Hstop in Ep. subst st. simpl.
           split5; try reflexivity; exact Hrel.
        -- rewrite Hcm_c, Hcm_h, comm_eqb_refl, Hb'. rewrite ?Hsp1. simpl.
           destruct (ob_accept (H ++ obs) (ObComm cm k persist (comm_data cands))); simpl.
           ++ split5; try reflexivity; exact Rfinal.
           ++ split5; try reflexivity; exact Hrel.
Qed.

(* ══════════════ The store effect of one staged operation ══════════════════ *)

(* pf_effect gives the store effect of a produce-side fire. FR-10: the
   effect never stores the incoming datum. Decision 45 (Rule E): a peek
   fire removes the same records as any other fire. *)
Definition pf_effect (S : Store) (ch : Chan) (v : Val) (persist : bool)
    (chs : list Chan) (wk : WK) (cands : list Cand) : Store :=
  let src := produce_id ch v persist in
  let S1 := bump S src persist in
  let p1 := if w_persist wk then (S1, []) else space_remove_continuation S1 chs wk in
  (* Disabled by decision 45 (Rule E): a peek fire removed only the fired joins.
  let retain := negb (nullb (w_peeks wk)) in
  fst (space_pf_loop (fst p1) chs retain (sort_desc vkey cands)). *)
  fst (space_pf_loop (fst p1) chs (sort_desc vkey cands)).

Definition effect (S : Store) (op : Op) : Store :=
  match op with
  | OProduce ch v persist PStore =>
      fst (put_datum (bump S (produce_id ch v persist) persist) ch
                     (mkDatum v persist (produce_id ch v persist)))
  | OProduce ch v persist (PFire chs wk kidx cands) => pf_effect S ch v persist chs wk cands
  | OConsume chs pats k persist peeks CPark =>
      fst (store_waiting_continuation S chs (mkWK pats k persist peeks (consume_id chs pats k persist)))
  | OConsume chs pats k persist peeks (CFire cands) =>
      (* Disabled by decision 45 (Rule E): a peek fire kept its stored records.
      fst (space_cf_loop S (negb (nullb peeks)) (sort_desc vkey cands)) *)
      fst (space_cf_loop S (sort_desc vkey cands))
  end.

Lemma effect_cong : forall S1 S2 op, store_eq S1 S2 -> store_eq (effect S1 op) (effect S2 op).
Proof.
  intros S1 S2 op E.
  destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]]; simpl.
  - apply (proj1 (put_datum_cong _ _ _ _ (bump_cong _ _ _ _ E))).
  - unfold pf_effect.
    assert (E1 : store_eq (fst (if w_persist wk then (bump S1 (produce_id ch v persist) persist, [])
                                else space_remove_continuation (bump S1 (produce_id ch v persist) persist) chs wk))
                          (fst (if w_persist wk then (bump S2 (produce_id ch v persist) persist, [])
                                else space_remove_continuation (bump S2 (produce_id ch v persist) persist) chs wk))).
    { destruct (w_persist wk); simpl; [apply bump_cong, E |].
      apply (proj1 (space_remove_continuation_cong _ _ _ _ (bump_cong _ _ _ _ E))). }
    exact (proj1 (space_pf_loop_cong (sort_desc vkey cands) _ _ chs E1)).
  - apply (proj1 (store_waiting_continuation_cong _ _ _ _ E)).
  - apply (proj1 (space_cf_loop_cong _ _ _ E)).
Qed.

Lemma stage_effect :
  forall T op T', stage T op = Some T' ->
    store_eq (view_store (t_view T')) (effect (view_store (t_view T)) op).
Proof.
  intros T op T' Hst. set (V := t_view T).
  destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]].
  - unfold stage in Hst. fold V in Hst.
    destruct (t_installed T ch); [discriminate |]. injection Hst as <-. simpl.
    eapply store_eq_trans; [apply view_put_datum |]. apply (proj1 (put_datum_cong _ _ _ _ (view_bump _ _ _))).
  - unfold stage in Hst. fold V in Hst.
    destruct (t_installed T ch); [discriminate |].
    destruct (pfire_ok T ch v persist chs wk cands); cbn [negb] in Hst; [| discriminate].
    apply stage_fire_some in Hst. subst T'. simpl.
    unfold vpf_effects, pf_effect.
    set (src := produce_id ch v persist).
    assert (E0 : store_eq (view_store (vbump V src persist)) (bump (view_store V) src persist)) by apply view_bump.
    assert (E1 : store_eq (view_store (if w_persist wk then vbump V src persist
                                       else vremove_continuation (vbump V src persist) chs wk))
                          (fst (if w_persist wk then (bump (view_store V) src persist, [])
                                else space_remove_continuation (bump (view_store V) src persist) chs wk))).
    { destruct (w_persist wk); simpl; [exact E0 |].
      eapply store_eq_trans; [apply view_remove_continuation | apply (proj1 (space_remove_continuation_cong _ _ _ _ E0))]. }
    exact (view_pf_loop (sort_desc vkey cands) _ _ chs E1).
  - unfold stage in Hst. fold V in Hst.
    destruct (consume_args_ok T chs pats peeks); simpl in Hst; [| discriminate].
    injection Hst as <-. simpl. apply view_store_wk, store_eq_refl.
  - unfold stage in Hst. fold V in Hst.
    destruct (consume_args_ok T chs pats peeks); cbn [negb] in Hst; [| discriminate].
    destruct (cfire_ok T chs cands); cbn [negb] in Hst; [| discriminate].
    apply stage_fire_some in Hst. subst T'. simpl.
    apply view_cf_loop, store_eq_refl.
Qed.

(* ═══════════════ Well-formedness of every staged effect (H7) ══════════════ *)

Definition effect_pre (S : Store) (op : Op) : Prop :=
  match op with
  | OProduce _ _ _ (PFire chs wk _ cands) => In wk (s_conts S chs) /\ map cd_chan cands = chs
  | _ => True
  end.

Lemma stage_effect_pre :
  forall T op T', stage T op = Some T' -> effect_pre (view_store (t_view T)) op.
Proof.
  intros T op T' Hst.
  destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]]; simpl; auto.
  unfold stage in Hst.
  destruct (t_installed T ch); [discriminate |].
  destruct (pfire_ok T ch v persist chs wk cands) eqn:Hok; simpl in Hst; [| discriminate].
  destruct (pfire_ok_spec T ch v persist chs wk cands Hok) as (_ & _ & Hmap & _ & _ & Hwk & _).
  split; [exact Hwk | exact Hmap].
Qed.

Lemma space_cf_loop_cj :
  forall cs S,
    (forall g, s_conts (fst (space_cf_loop S cs)) g = s_conts S g) /\
    (forall c, s_joins (fst (space_cf_loop S cs)) c = s_joins S c).
Proof.
  induction cs as [| cd rest IH]; intros S; simpl; [split; reflexivity |].
  set (p1 := if negb (d_persist (cd_rec cd)) then space_remove_datum S (cd_chan cd) (cd_rec cd) else (S, [])).
  destruct (IH (fst p1)) as [Ic Ij].
  assert (F : (forall g, s_conts (fst p1) g = s_conts S g) /\ (forall c, s_joins (fst p1) c = s_joins S c)).
  { subst p1. destruct (negb (d_persist (cd_rec cd))); [| split; reflexivity].
    destruct (space_remove_datum_fields S (cd_chan cd) (cd_rec cd)) as (_ & X & Y & _). split; assumption. }
  destruct F as [Fc Fj]. split; intros; [rewrite Ic; apply Fc | rewrite Ij; apply Fj].
Qed.

Lemma effect_WF : forall S op, WF S -> effect_pre S op -> WF (effect S op).
Proof.
  intros S op Hwf Hpre.
  destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]]; simpl in *.
  - apply WF_same_cj with (S := S); [exact Hwf | |]; intros; reflexivity.
  - destruct Hpre as [Hwk Hmap].
    unfold pf_effect.
    set (src := produce_id ch v persist).
    set (S1 := bump S src persist).
    set (p1 := if w_persist wk then (S1, []) else space_remove_continuation S1 chs wk).
    set (p2 := space_pf_loop (fst p1) chs (sort_desc vkey cands)).
    destruct (space_pf_loop_spec (sort_desc vkey cands) (fst p1) chs) as (Ld & Lc & Lj & Ln).
    fold p2 in Ld, Lc, Lj, Ln.
    assert (C1 : forall g, s_conts (fst p1) g =
                   if w_persist wk then s_conts S g
                   else (if lc_eq_dec g chs then remove_first wk_eq_dec wk (s_conts S chs) else s_conts S g)).
    { intros g. subst p1 S1. destruct (w_persist wk); [reflexivity |].
      unfold space_remove_continuation. simpl.
      destruct (in_dec wk_eq_dec wk (s_conts S chs)) as [_ | Cn]; [| contradiction].
      simpl. destruct (lc_eq_dec g chs); reflexivity. }
    assert (J1 : forall c, s_joins (fst p1) c = s_joins S c).
    { intros c. subst p1 S1. destruct (w_persist wk); [reflexivity |].
      destruct (space_remove_continuation_data (bump S src persist) chs wk) as (_ & X & _). apply X. }
    apply rj_closed_WF with (S := S) (chs := chs) (cs := sort_desc vkey cands).
    + exact Hwf.
    + intro E. rewrite E in Hwk. contradiction.
    + intros g Ng. rewrite Lc, C1. destruct (w_persist wk); [reflexivity |].
      destruct (lc_eq_dec g chs); [contradiction | reflexivity].
    + intros c. rewrite Lj. unfold rj_closed. simpl. rewrite Lc, J1. reflexivity.
    + intros g. rewrite Lc, C1. destruct Hwf as (_ & _ & Hcn).
      destruct (w_persist wk); [apply Hcn |].
      destruct (lc_eq_dec g chs); [apply nodup_map_remove_first, Hcn | apply Hcn].
    + intros c. rewrite <- Hmap. split; intros Hin.
      * eapply Permutation_in; [apply Permutation_map, Permutation_sym, sort_desc_perm | exact Hin].
      * eapply Permutation_in; [apply Permutation_map, sort_desc_perm | exact Hin].
  - apply swc_WF, Hwf.
  - destruct (space_cf_loop_cj (sort_desc vkey cands) S) as (Lc & Lj).
    apply WF_same_cj with (S := S); assumption.
Qed.

(* ═══════════════ Replay bindings: rig, invariant, verify (H4) ═════════════ *)

Definition io_of_log (log : list Ev) : list IOKey :=
  flat_map (fun e => match e with
                     | EvProduce p => [KProduce p]
                     | EvConsume c => [KConsume c]
                     | EvComm _ _ => []
                     end) log.

Definition comms_of_log (log : list Ev) : list Comm :=
  flat_map (fun e => match e with EvComm _ cm => [cm] | _ => [] end) log.

Definition add_binding (rd : Bindings) (k : IOKey) (c : Comm) : Bindings :=
  fun k' c' => if iokey_eq_dec k' k then (if comm_eq_dec c' c then S (rd k' c') else rd k' c')
               else rd k' c'.

(* rig (replay_rspace.rs:480-540): every COMM event of the log is bound under
   each of its IO events (consume first, then produces) that the log
   contains. *)
Definition rig_comm (io : list IOKey) (rd : Bindings) (c : Comm) : Bindings :=
  fold_left (fun rd k => if in_dec iokey_eq_dec k io then add_binding rd k c else rd) (keys c) rd.

Definition rig (log : list Ev) : Bindings :=
  fold_left (rig_comm (io_of_log log)) (comms_of_log log) (fun _ _ => 0).

(* The binding invariant: under a key the log contains, a COMM with [rem]
   unpaired occurrences carries [rem * mult] bindings. *)
Definition binding_inv (rd : Bindings) (io : list IOKey) (rem : Comm -> nat) : Prop :=
  forall k c, rd k c = (if in_dec iokey_eq_dec k io then rem c * mult k c else 0).

Lemma rig_keys_value :
  forall ks io rd c k' c',
    fold_left (fun rd k => if in_dec iokey_eq_dec k io then add_binding rd k c else rd) ks rd k' c' =
    rd k' c' + (if comm_eq_dec c' c
                then (if in_dec iokey_eq_dec k' io then count_occ iokey_eq_dec ks k' else 0) else 0).
Proof.
  induction ks as [| k ks IH]; intros io rd c k' c'; simpl.
  - destruct (comm_eq_dec c' c); [destruct (in_dec iokey_eq_dec k' io) |]; lia.
  - rewrite IH. destruct (in_dec iokey_eq_dec k io) as [Hk | Hk].
    + unfold add_binding.
      destruct (iokey_eq_dec k' k) as [E | N]; destruct (comm_eq_dec c' c) as [Ec | Nc];
        destruct (iokey_eq_dec k k') as [E' | N']; try congruence;
        destruct (in_dec iokey_eq_dec k' io); subst; try contradiction; lia.
    + destruct (comm_eq_dec c' c) as [Ec | Nc]; [| lia].
      destruct (in_dec iokey_eq_dec k' io) as [Hk' | Hk']; [| lia].
      destruct (iokey_eq_dec k k') as [E | N]; [subst; contradiction | lia].
Qed.

Lemma rig_comms_value :
  forall cs io rd k c,
    fold_left (rig_comm io) cs rd k c =
    rd k c + (if in_dec iokey_eq_dec k io then count_occ comm_eq_dec cs c * mult k c else 0).
Proof.
  induction cs as [| c0 cs IH]; intros io rd k c; simpl.
  - destruct (in_dec iokey_eq_dec k io); lia.
  - rewrite IH. unfold rig_comm. rewrite rig_keys_value. unfold mult.
    destruct (comm_eq_dec c c0) as [E | N]; destruct (comm_eq_dec c0 c) as [E' | N']; try congruence;
      destruct (in_dec iokey_eq_dec k io); subst; try lia.
Qed.

Lemma rig_binding_inv :
  forall log, binding_inv (rig log) (io_of_log log) (fun c => count_occ comm_eq_dec (comms_of_log log) c).
Proof.
  intros log k c. unfold rig. rewrite rig_comms_value. simpl. reflexivity.
Qed.

Lemma mult_pos : forall k c, In k (keys c) -> 0 < mult k c.
Proof. intros k c Hin. unfold mult. apply (count_occ_In iokey_eq_dec). exact Hin. Qed.

Lemma mult_not_in : forall k c, ~ In k (keys c) -> mult k c = 0.
Proof. intros k c H. unfold mult. apply count_occ_not_In. exact H. Qed.

(* recorded_comms reports [available] copies under the key. It reports a
   copy exactly when verify accepts the COMM under the same key. *)
Lemma available_spec :
  forall rd paired k c, verify rd paired k c = true <-> 0 < available rd paired k c.
Proof. intros rd paired k c. unfold verify. apply Nat.ltb_lt. Qed.

(* The trigger-key rule (FR-3): under the binding invariant, verify accepts
   exactly when the log holds the key, the key is a key of the COMM, and an
   unpaired recorded occurrence of the COMM remains. *)
Lemma verify_spec :
  forall rd io rem paired k c,
    binding_inv rd io rem ->
    (verify rd paired k c = true <->
       In k io /\ In k (keys c) /\ count_occ comm_eq_dec paired c < rem c).
Proof.
  intros rd io rem paired k c Hinv.
  rewrite available_spec. unfold available. rewrite (Hinv k c).
  destruct (in_dec iokey_eq_dec k (keys c)) as [Hk | Hk].
  - assert (Hm := mult_pos k c Hk).
    destruct (mult k c) as [| n] eqn:Em; [lia |]. cbv iota beta.
    destruct (in_dec iokey_eq_dec k io) as [Hio | Hio].
    + rewrite Nat.div_mul by discriminate.
      split; [intros H; split; [exact Hio | split; [exact Hk | lia]] | intros (_ & _ & H); lia].
    + rewrite Nat.Div0.div_0_l.
      split; [intros H; lia | intros (C & _); contradiction].
  - rewrite (mult_not_in k c Hk). split; [intros H; lia | intros (_ & C & _); contradiction].
Qed.

(* The literal design rule over-pairs a COMM whose produce list repeats a
   produce: with one recorded occurrence already paired, it still accepts.
   The trigger-key rule and the any-key rule both refuse. *)
Lemma design_verify_overpairs :
  forall cons p peeks times io rd,
    In (KProduce p) io ->
    let c := mkComm cons [p; p] peeks times in
    binding_inv rd io (fun c' => if comm_eq_dec c' c then 1 else 0) ->
    verify_literal rd [c] c = true /\ verify rd [c] (KProduce p) c = false /\
    verify_any_key rd [c] c = false /\ count_occ comm_eq_dec [c] c = 1.
Proof.
  intros cons p peeks times io rd Hio c Hinv.
  assert (Hcnt : count_occ comm_eq_dec [c] c = 1).
  { cbn [count_occ]. destruct (comm_eq_dec c c); [reflexivity | contradiction]. }
  assert (Hm : mult (KProduce p) c = 2).
  { unfold mult, keys. unfold c. cbn [c_consume c_produces map count_occ].
    destruct (iokey_eq_dec (KConsume cons) (KProduce p)) as [E | _]; [discriminate |].
    destruct (iokey_eq_dec (KProduce p) (KProduce p)) as [_ | C]; [| contradiction]. reflexivity. }
  assert (Hnone : forall k, verify rd [c] k c = false).
  { intros k. apply Bool.not_true_iff_false. intros Hv.
    apply (verify_spec rd io _ [c] k c Hinv) in Hv as (_ & _ & Hlt).
    rewrite Hcnt in Hlt. destruct (comm_eq_dec c c) as [_ | C]; [lia | contradiction]. }
  split; [| split; [apply Hnone | split; [| exact Hcnt]]].
  - unfold verify_literal. apply existsb_exists. exists (KProduce p).
    split; [unfold keys, c; cbn [c_consume c_produces map]; right; left; reflexivity |].
    rewrite Hcnt, Hinv. destruct (in_dec iokey_eq_dec (KProduce p) io) as [_ | Cn]; [| contradiction].
    destruct (comm_eq_dec c c) as [_ | C]; [| contradiction]. rewrite Hm. reflexivity.
  - unfold verify_any_key. apply Bool.not_true_iff_false. intros Hv.
    apply existsb_exists in Hv as (k & _ & Hk). rewrite Hnone in Hk. discriminate.
Qed.

(* Review D3: the rule before FR-3 pairs a produce-side fire through the
   consume binding of its COMM, although the log lacks the trigger Produce
   event. The trigger-key rule refuses. *)
Lemma any_key_pairs_without_trigger :
  forall cons p peeks times rd,
    let c := mkComm cons [p] peeks times in
    binding_inv rd [KConsume cons] (fun c' => if comm_eq_dec c' c then 1 else 0) ->
    verify_any_key rd [] c = true /\ verify rd [] (KProduce p) c = false.
Proof.
  intros cons p peeks times rd c Hinv. split.
  - unfold verify_any_key. apply existsb_exists. exists (KConsume cons).
    split; [unfold keys, c; left; reflexivity |].
    apply (verify_spec rd [KConsume cons] _ [] (KConsume cons) c Hinv).
    split; [left; reflexivity |]. split; [unfold keys, c; left; reflexivity |].
    cbn [count_occ]. destruct (comm_eq_dec c c) as [_ | C]; [lia | contradiction].
  - apply Bool.not_true_iff_false. intros Hv.
    apply (verify_spec rd [KConsume cons] _ [] (KProduce p) c Hinv) in Hv as (Hio & _).
    destruct Hio as [E | []]. discriminate.
Qed.

Lemma fold_unbind_value :
  forall ps rd k c, fold_left unbind ps rd k c = rd k c - count_occ comm_eq_dec ps c * mult k c.
Proof.
  induction ps as [| p ps IH]; intros rd k c; cbn [fold_left count_occ]; [lia |].
  rewrite IH. unfold unbind.
  destruct (comm_eq_dec c p) as [E | N]; destruct (comm_eq_dec p c) as [E' | N']; subst; try congruence; lia.
Qed.

Lemma binding_inv_fold_unbind :
  forall ps rd io rem,
    binding_inv rd io rem ->
    binding_inv (fold_left unbind ps rd) io (fun c => rem c - count_occ comm_eq_dec ps c).
Proof.
  intros ps rd io rem Hinv k c. rewrite fold_unbind_value, Hinv.
  destruct (in_dec iokey_eq_dec k io); [rewrite Nat.mul_sub_distr_r; reflexivity | lia].
Qed.

Lemma bound_ok_of_inv :
  forall T op rd io rem,
    (backend (t_mode T) <> Play ->
       binding_inv (t_rd T) io rem /\ rd = fold_left unbind (t_paired T) (t_rd T) /\
       In (trigger_key op) io) ->
    bound_ok T op rd.
Proof.
  intros T op rd io rem Hpre Hnp cm Hver Htrig.
  destruct (Hpre Hnp) as (Hinv & Hrd & _).
  destruct (proj1 (verify_spec (t_rd T) io rem (t_paired T) (trigger_key op) cm Hinv) Hver)
    as (Hio & _ & Hlt).
  unfold bound. apply Nat.ltb_lt. subst rd.
  rewrite fold_unbind_value, Hinv.
  destruct (in_dec iokey_eq_dec (trigger_key op) io) as [_ | Cn]; [| contradiction].
  assert (Hm := mult_pos (trigger_key op) cm Htrig).
  rewrite <- Nat.mul_sub_distr_r.
  apply Nat.mul_pos_pos; lia.
Qed.

(* ═════════════════════════ Run-level lemmas ═══════════════════════════════ *)

Lemma stage_props :
  forall T op T', stage T op = Some T' ->
    exists sg, t_staged T' = t_staged T ++ [sg] /\ sg_op sg = op /\
               t_mode T' = t_mode T /\ t_installed T' = t_installed T /\ t_rd T' = t_rd T.
Proof.
  intros T op T' Hst.
  destruct (commit_op_coherent T op T' [] (view_store (t_view T)) (fun _ _ => 0) Hst (store_eq_refl _))
    as (sg & A & B & C & D & E & _).
  exists sg. auto.
Qed.

Lemma stage_all_props :
  forall ops T T', stage_all T ops = Some T' ->
    (exists new, t_staged T' = t_staged T ++ new /\ length new = length ops) /\
    t_mode T' = t_mode T /\ t_installed T' = t_installed T /\ t_rd T' = t_rd T.
Proof.
  induction ops as [| op rest IH]; intros T T' Hst; simpl in Hst.
  - injection Hst as <-. split; [exists []; rewrite app_nil_r; auto | auto].
  - destruct (stage T op) as [T1 |] eqn:H1; [| discriminate].
    destruct (stage_props T op T1 H1) as (sg & Hsg & _ & Hm & Hi & Hr).
    destruct (IH T1 T' Hst) as ((new & Hnew & Hlen) & Hm' & Hi' & Hr').
    split; [| split; [congruence | split; congruence]].
    exists (sg :: new). split; [rewrite Hnew, Hsg, <- app_assoc; reflexivity | simpl; lia].
Qed.

(* Staging a concatenation stages the first list, then the second. *)
Lemma stage_all_app :
  forall l1 l2 T, stage_all T (l1 ++ l2) =
    match stage_all T l1 with Some T1 => stage_all T1 l2 | None => None end.
Proof.
  induction l1 as [| op rest IH]; intros l2 T; cbn [app stage_all]; [reflexivity |].
  destruct (stage T op) as [T1 |]; [apply IH | reflexivity].
Qed.

Lemma skipn_len_app : forall (A : Type) (l1 l2 : list A), skipn (length l1) (l1 ++ l2) = l2.
Proof. intros A l1 l2. induction l1 as [| x t IH]; simpl; [reflexivity | exact IH]. Qed.

Lemma skipn_len_self : forall (A : Type) (l : list A), skipn (length l) l = [].
Proof. intros A l. induction l as [| x t IH]; simpl; [reflexivity | exact IH]. Qed.

Lemma skipn_step :
  forall T T1 T' sg, t_staged T1 = t_staged T ++ [sg] ->
    (exists new, t_staged T' = t_staged T1 ++ new) ->
    skipn (length (t_staged T)) (t_staged T') = sg :: skipn (length (t_staged T1)) (t_staged T').
Proof.
  intros T T1 T' sg Hsg (new & Hnew).
  rewrite Hnew, skipn_len_app, Hsg, <- app_assoc, skipn_len_app. reflexivity.
Qed.

Lemma stage_all_WF :
  forall ops T T', WF (view_store (t_view T)) -> stage_all T ops = Some T' ->
    WF (view_store (t_view T')).
Proof.
  induction ops as [| op rest IH]; intros T T' Hwf Hst; simpl in Hst.
  - injection Hst as <-. exact Hwf.
  - destruct (stage T op) as [T1 |] eqn:H1; [| discriminate].
    apply (IH T1 T'); [| exact Hst].
    apply WF_store_eq with (S1 := effect (view_store (t_view T)) op).
    + apply store_eq_sym, stage_effect, H1.
    + apply effect_WF; [exact Hwf | eapply stage_effect_pre; exact H1].
Qed.

Lemma commit_coherent_gen :
  forall ops T T' H S rd,
    stage_all T ops = Some T' ->
    store_eq (view_store (t_view T)) S ->
    let r := commit_run (t_mode T) H S rd (skipn (length (t_staged T)) (t_staged T')) in
    exists n Tn,
      n <= length ops /\ stage_all T (firstn n ops) = Some Tn /\
      store_eq (r_st r) (view_store (t_view Tn)) /\
      (r_status r = StOk -> n = length ops) /\
      r_status r <> StPanic /\ r_status r <> StNative.
Proof.
  induction ops as [| op rest IH]; intros T T' H S rd Hst Heq; simpl in Hst.
  - injection Hst as <-. cbv zeta. rewrite skipn_len_self.
    simpl. exists 0, T.
    split; [lia |]. split; [reflexivity |]. split; [apply store_eq_sym, Heq |].
    split; [intros _; reflexivity |]. split; discriminate.
  - destruct (stage T op) as [T1 |] eqn:H1; [| discriminate].
    destruct (commit_op_coherent T op T1 H S rd H1 Heq)
      as (sg & Hsg & _ & Hmode & _ & _ & _ & _ & Hnp & Hnn & _ & Hok & Hnok).
    destruct (stage_all_props rest T1 T' Hst) as ((new & Hnew & _) & _).
    cbv zeta. rewrite (skipn_step T T1 T' sg Hsg (ex_intro _ new Hnew)). simpl.
    destruct (r_status (commit_op (t_mode T) H S rd sg)) eqn:Hs.
    + specialize (Hok eq_refl).
      destruct (IH T1 T' (H ++ r_obs (commit_op (t_mode T) H S rd sg))
                   (r_st (commit_op (t_mode T) H S rd sg)) (r_rd (commit_op (t_mode T) H S rd sg))
                   Hst (store_eq_sym _ _ Hok))
        as (n & Tn & Hn & Hstn & Heqn & Hokn & Hnp' & Hnn').
      rewrite Hmode in Heqn, Hokn, Hnp', Hnn'.
      exists (Datatypes.S n), Tn. simpl. rewrite H1.
      split; [lia |]. split; [exact Hstn |]. split; [exact Heqn |].
      split; [intros E; rewrite (Hokn E); reflexivity |]. split; assumption.
    + exists 0, T. simpl. destruct (Hnok ltac:(discriminate)) as (E & _).
      split; [lia |]. split; [reflexivity |].
      split; [eapply store_eq_trans; [exact E | apply store_eq_sym, Heq] |].
      split; [intros C; congruence |]. split; intro C; congruence.
    + exfalso. apply Hnp. reflexivity.
    + exfalso. apply Hnn. reflexivity.
    + (* an unpaired replay fire (decision 36): the commit stops with the
         operations before it applied (D6) *)
      exists 0, T. simpl. destruct (Hnok ltac:(discriminate)) as (E & _).
      split; [lia |]. split; [reflexivity |].
      split; [eapply store_eq_trans; [exact E | apply store_eq_sym, Heq] |].
      split; [intros C; congruence |]. split; intro C; congruence.
Qed.

Fixpoint host_valid_run (m : Mode) (H : list Obs) (S : Store) (rd : Bindings) (ops : list Op) : Prop :=
  match ops with
  | [] => True
  | op :: rest =>
      host_valid_op S op /\
      (r_status (host_op m H S rd op) = StOk ->
       host_valid_run m (H ++ r_obs (host_op m H S rd op)) (r_st (host_op m H S rd op))
                      (r_rd (host_op m H S rd op)) rest)
  end.

Definition replay_pre (T : Txn) (rd : Bindings) (io : list IOKey) (rem : Comm -> nat)
    (ops : list Op) : Prop :=
  backend (t_mode T) <> Play ->
  binding_inv (t_rd T) io rem /\ rd = fold_left unbind (t_paired T) (t_rd T) /\
  Forall (fun op => In (trigger_key op) io) ops.

(* The host simulation of a run of staged operations. Decision 45 (Rule
   E): after each operation the two stores are related by store_rel, with
   no kept records. *)
Lemma run_sim_gen :
  forall ops T T' H Sc Sh rd io rem,
    no_dir_some ->
    stage_all T ops = Some T' ->
    store_eq (view_store (t_view T)) Sc ->
    store_rel Sc Sh ->
    WF (view_store (t_view T)) ->
    host_valid_run (t_mode T) H Sh rd ops ->
    replay_pre T rd io rem ops ->
    Forall (fire_paired (t_mode T)) (skipn (length (t_staged T)) (t_staged T')) ->
    let rc := commit_run (t_mode T) H Sc rd (skipn (length (t_staged T)) (t_staged T')) in
    let rh := host_run (t_mode T) H Sh rd ops in
    r_obs rc = r_obs rh /\ r_ev rc = r_ev rh /\ r_status rc = r_status rh /\ r_rd rc = r_rd rh /\
    store_rel (r_st rc) (r_st rh).
Proof.
  induction ops as [| op rest IH]; intros T T' H Sc Sh rd io rem Hnd Hst Heq Hrel Hwf Hhv Hpre Hfps;
    simpl in Hst.
  - injection Hst as <-. cbv zeta. rewrite skipn_len_self.
    simpl. split5; try reflexivity; exact Hrel.
  - destruct (stage T op) as [T1 |] eqn:H1; [| discriminate].
    assert (HwfSc : WF Sc) by (eapply WF_store_eq; [exact Heq | exact Hwf]).
    destruct Hhv as [Hhv0 Hhvr].
    assert (Hbo : bound_ok T op rd).
    { apply (bound_ok_of_inv T op rd io rem). intros Hnp. destruct (Hpre Hnp) as (A & B & C).
      inversion C; subst. auto. }
    destruct (commit_op_coherent T op T1 H Sc rd H1 Heq)
      as (sg' & Hsg' & Hsgop & Hmode & _ & Hrd1 & Hpn & Hps & Hnp & Hnn & Hrda & Hok & Hnok).
    destruct (stage_all_props rest T1 T' Hst) as ((new & Hnew & _) & _).
    rewrite (skipn_step T T1 T' sg' Hsg' (ex_intro _ new Hnew)) in Hfps.
    inversion Hfps as [| sg0 rest0 Hfp0 Hfps1 E0]; subst sg0 rest0.
    assert (Hfp_op : forall sg0, t_staged T1 = t_staged T ++ [sg0] -> fire_paired (t_mode T) sg0).
    { intros sg0 Hsg0. rewrite Hsg' in Hsg0. apply app_inj_tail in Hsg0 as [_ <-]. exact Hfp0. }
    destruct (op_sim T op T1 H Sc Sh rd Hnd H1 Heq Hrel HwfSc Hhv0 Hbo Hfp_op)
      as (sg & Hsg & Hobs & Hev & Hstatus & Hrd & Hrel1).
    rewrite Hsg in Hsg'. apply app_inj_tail in Hsg' as [_ <-].
    cbv zeta. rewrite (skipn_step T T1 T' sg Hsg (ex_intro _ new Hnew)).
    simpl (commit_run _ _ _ _ (sg :: _)). simpl (host_run _ _ _ _ (op :: _)).
    rewrite <- Hstatus.
    destruct (r_status (commit_op (t_mode T) H Sc rd sg)) eqn:Hs.
    + (* the operation committed: continue with the rest *)
      specialize (Hok eq_refl).
      assert (HwfT1 : WF (view_store (t_view T1))).
      { apply WF_store_eq with (S1 := effect (view_store (t_view T)) op).
        - apply store_eq_sym, stage_effect, H1.
        - apply effect_WF; [exact Hwf | eapply stage_effect_pre; exact H1]. }
      assert (Hpre1 : replay_pre T1 (r_rd (commit_op (t_mode T) H Sc rd sg)) io rem rest).
      { intros Hnp1. rewrite Hmode in Hnp1. destruct (Hpre Hnp1) as (Hinv & Hrdeq & Htr).
        destruct (stage_props T op T1 H1) as (_ & _ & _ & _ & _ & Hrdt).
        split; [rewrite Hrdt; exact Hinv |].
        split; [| inversion Htr; assumption].
        rewrite Hrda, Hrdt. unfold rd_after.
        destruct (sg_comm sg) as [cm |] eqn:Hc.
        - destruct (Hps cm eq_refl) as (Hp & _ & _ & Hfire).
          assert (Hsp : sg_paired sg = true).
          { apply Hfp0; [exact Hnp1 | rewrite Hsgop; exact Hfire]. }
          destruct (backend (t_mode T)) eqn:Hb; [contradiction | |];
            rewrite Hp, Hsp, fold_left_app, <- Hrdeq; reflexivity.
        - rewrite (proj1 (Hpn eq_refl)). destruct (backend (t_mode T)); exact Hrdeq. }
      assert (Hhv1 : host_valid_run (t_mode T1) (H ++ r_obs (commit_op (t_mode T) H Sc rd sg))
                       (r_st (host_op (t_mode T) H Sh rd op)) (r_rd (commit_op (t_mode T) H Sc rd sg)) rest).
      { rewrite Hmode, Hobs, Hrd. apply Hhvr. rewrite <- Hstatus. first [reflexivity | exact Hs]. }
      destruct (IH T1 T' (H ++ r_obs (commit_op (t_mode T) H Sc rd sg))
                   (r_st (commit_op (t_mode T) H Sc rd sg)) (r_st (host_op (t_mode T) H Sh rd op))
                   (r_rd (commit_op (t_mode T) H Sc rd sg)) io rem
                   Hnd Hst (store_eq_sym _ _ Hok) Hrel1 HwfT1 Hhv1 Hpre1
                   ltac:(rewrite Hmode; exact Hfps1))
        as (Iobs & Iev & Ist & Ird & Irel).
      rewrite Hmode in Iobs, Iev, Ist, Ird, Irel.
      rewrite <- Hobs, <- Hrd in *.
      simpl. split; [rewrite Iobs; reflexivity |].
      split; [rewrite Hev, Iev; reflexivity |].
      split; [exact Ist |]. split; [exact Ird | exact Irel].
    + (* rejected *)
      simpl. split; [exact Hobs |]. split; [exact Hev |]. split; [congruence |].
      split; [exact Hrd | exact Hrel1].
    + exfalso. apply Hnp. reflexivity.
    + exfalso. apply Hnn. reflexivity.
    + (* diverged: the proof of the rejected case applies. The host never
         diverges, so op_sim excludes this case *)
      simpl. split; [exact Hobs |]. split; [exact Hev |]. split; [congruence |].
      split; [exact Hrd | exact Hrel1].
Qed.

(* ═════════════════════════ Headline theorems ═════════════════════════════ *)

(* Premises shared by H1, H2 and H11: a fresh transaction, staged over a view
   of the store S, against a genuine host execution of the same operations
   and, in replay, against the recorded bindings of the log. *)
Definition replay_ctx (T0 : Txn) (rd : Bindings) (io : list IOKey) (rem : Comm -> nat)
    (ops : list Op) : Prop :=
  backend (t_mode T0) <> Play ->
  rd = t_rd T0 /\ binding_inv rd io rem /\ Forall (fun op => In (trigger_key op) io) ops.

Lemma run_sim_fresh :
  forall ops T0 T H S rd io rem,
    no_dir_some ->
    t_staged T0 = [] -> t_paired T0 = [] ->
    stage_all T0 ops = Some T ->
    store_eq (view_store (t_view T0)) S ->
    WF S ->
    host_valid_run (t_mode T0) H S rd ops ->
    replay_ctx T0 rd io rem ops ->
    Forall (fire_paired (t_mode T0)) (t_staged T) ->
    let rc := commit_run (t_mode T0) H S rd (t_staged T) in
    let rh := host_run (t_mode T0) H S rd ops in
    r_obs rc = r_obs rh /\ r_ev rc = r_ev rh /\ r_status rc = r_status rh /\ r_rd rc = r_rd rh /\
    store_rel (r_st rc) (r_st rh).
Proof.
  intros ops T0 T H S rd io rem Hnd Hs0 Hp0 Hst Heq Hwf Hhv Hctx Hfp.
  assert (Hwfv : WF (view_store (t_view T0))) by (eapply WF_store_eq; [apply store_eq_sym, Heq | exact Hwf]).
  assert (Hpre : replay_pre T0 rd io rem ops).
  { intros Hnp. destruct (Hctx Hnp) as (Hrd & Hinv & Htr).
    split; [rewrite <- Hrd; exact Hinv |]. split; [rewrite Hp0; exact Hrd | exact Htr]. }
  destruct (run_sim_gen ops T0 T H S S rd io rem Hnd Hst Heq (store_rel_self S) Hwfv Hhv Hpre
              ltac:(rewrite Hs0; exact Hfp))
    as (A & B & C & D & Rel).
  rewrite Hs0 in A, B, C, D, Rel. simpl in A, B, C, D, Rel.
  split; [exact A |]. split; [exact B |]. split; [exact C |]. split; [exact D | exact Rel].
Qed.

(* H1: committing the staged operations makes exactly the observer calls,
   the events and the status that the host makes for the same operations.
   Decision 36: in replay, every staged fire paired (fire_paired). Under the
   rule of decision 19 a successful staging implied it. An unpaired fire
   fails at commit (H4), where the host would fail its bound assertion. *)
Theorem commit_trace_eq_host :
  forall ops T0 T H S rd io rem,
    no_dir_some ->
    t_staged T0 = [] -> t_paired T0 = [] ->
    stage_all T0 ops = Some T ->
    store_eq (view_store (t_view T0)) S ->
    WF S ->
    host_valid_run (t_mode T0) H S rd ops ->
    replay_ctx T0 rd io rem ops ->
    Forall (fire_paired (t_mode T0)) (t_staged T) ->
    let rc := commit_run (t_mode T0) H S rd (t_staged T) in
    let rh := host_run (t_mode T0) H S rd ops in
    r_obs rc = r_obs rh /\ r_ev rc = r_ev rh /\ r_status rc = r_status rh /\ r_rd rc = r_rd rh.
Proof.
  intros ops T0 T H S rd io rem Hnd Hs0 Hp0 Hst Heq Hwf Hhv Hctx Hfp.
  destruct (run_sim_fresh ops T0 T H S rd io rem Hnd Hs0 Hp0 Hst Heq Hwf Hhv Hctx Hfp)
    as (A & B & C & D & _). auto.
Qed.

(* Disabled by decision 45 (Rule E): H2 no longer needs the premise that no operation retains.
(* H2: without peek retention, the committed store equals the host store:
   continuations, joins and counters are equal, and every data list is a
   permutation of the host list (leaf encodings sort records). *)
Theorem commit_state_eq_host :
  forall ops T0 T H S rd io rem,
    no_dir_some ->
    t_staged T0 = [] -> t_paired T0 = [] ->
    stage_all T0 ops = Some T ->
    store_eq (view_store (t_view T0)) S ->
    WF S ->
    host_valid_run (t_mode T0) H S rd ops ->
    replay_ctx T0 rd io rem ops ->
    Forall (fire_paired (t_mode T0)) (t_staged T) ->
    (forall op, In op ops -> retaining op = false) ->
    let rc := commit_run (t_mode T0) H S rd (t_staged T) in
    let rh := host_run (t_mode T0) H S rd ops in
    (forall c, Permutation (s_data (r_st rc) c) (s_data (r_st rh) c)) /\
    (forall g, s_conts (r_st rc) g = s_conts (r_st rh) g) /\
    (forall c, s_joins (r_st rc) c = s_joins (r_st rh) c) /\
    (forall p, s_count (r_st rc) p = s_count (r_st rh) p) /\
    r_rd rc = r_rd rh.
Proof.
  intros ops T0 T H S rd io rem Hnd Hs0 Hp0 Hst Heq Hwf Hhv Hctx Hfp Hnr.
  destruct (run_sim_fresh ops T0 T H S rd io rem Hnd Hs0 Hp0 Hst Heq Hwf Hhv Hctx Hfp)
    as (_ & _ & _ & D & kept' & (Rd & Rc & Rj & Rn) & _ & K2).
  assert (Hnk : forall op c, In op ops -> retained op c = []).
  { intros op c Hin. apply retained_nonretaining, Hnr, Hin. }
  split; [| split; [exact Rc | split; [exact Rj | split; [exact Rn | exact D]]]].
  intros c. rewrite <- (app_nil_r (s_data (r_st (host_run (t_mode T0) H S rd ops)) c)).
  rewrite <- (K2 Hnk c). apply Rd.
Qed.
*)

(* H2: the committed store equals the host store: continuations, joins and
   counters are equal, and every data list is a permutation of the host
   list (leaf encodings sort records). Decision 45 (Rule E): H2 has no
   premise about peeks. A peek fire removes the records that the host
   removes, so H2 holds for every staged transaction. *)
Theorem commit_state_eq_host :
  forall ops T0 T H S rd io rem,
    no_dir_some ->
    t_staged T0 = [] -> t_paired T0 = [] ->
    stage_all T0 ops = Some T ->
    store_eq (view_store (t_view T0)) S ->
    WF S ->
    host_valid_run (t_mode T0) H S rd ops ->
    replay_ctx T0 rd io rem ops ->
    Forall (fire_paired (t_mode T0)) (t_staged T) ->
    let rc := commit_run (t_mode T0) H S rd (t_staged T) in
    let rh := host_run (t_mode T0) H S rd ops in
    (forall c, Permutation (s_data (r_st rc) c) (s_data (r_st rh) c)) /\
    (forall g, s_conts (r_st rc) g = s_conts (r_st rh) g) /\
    (forall c, s_joins (r_st rc) c = s_joins (r_st rh) c) /\
    (forall p, s_count (r_st rc) p = s_count (r_st rh) p) /\
    r_rd rc = r_rd rh.
Proof.
  intros ops T0 T H S rd io rem Hnd Hs0 Hp0 Hst Heq Hwf Hhv Hctx Hfp.
  destruct (run_sim_fresh ops T0 T H S rd io rem Hnd Hs0 Hp0 Hst Heq Hwf Hhv Hctx Hfp)
    as (_ & _ & _ & D & (Rd & Rc & Rj & Rn)).
  split; [exact Rd |]. split; [exact Rc |]. split; [exact Rj |]. split; [exact Rn | exact D].
Qed.

(* H6: the staged view is the committed store.  Commit is prefix-atomic
   (decision D6): it stops at the first rejected operation, and the store is
   then the view after staging exactly the operations before it. *)
Theorem view_commit_coherent :
  forall ops T0 T H S rd,
    t_staged T0 = [] ->
    stage_all T0 ops = Some T ->
    store_eq (view_store (t_view T0)) S ->
    let r := commit_run (t_mode T0) H S rd (t_staged T) in
    exists n Tn,
      n <= length ops /\ stage_all T0 (firstn n ops) = Some Tn /\
      store_eq (r_st r) (view_store (t_view Tn)) /\
      (r_status r = StOk -> n = length ops /\ store_eq (r_st r) (view_store (t_view T))) /\
      r_status r <> StPanic /\ r_status r <> StNative.
Proof.
  intros ops T0 T H S rd Hs0 Hst Heq.
  destruct (commit_coherent_gen ops T0 T H S rd Hst Heq) as (n & Tn & Hn & Hstn & Heqn & Hok & Hnp & Hnn).
  rewrite Hs0 in Heqn, Hok, Hnp, Hnn. simpl in Heqn, Hok, Hnp, Hnn.
  exists n, Tn. split; [exact Hn |]. split; [exact Hstn |]. split; [exact Heqn |].
  split; [| split; assumption].
  intros Hs. specialize (Hok Hs). split; [exact Hok |].
  subst n. rewrite firstn_all in Hstn. rewrite Hst in Hstn. injection Hstn as <-. exact Heqn.
Qed.

(* H7: staging keeps joins in lockstep with continuations, and so does the
   commit of the staged operations. *)
Theorem joins_lockstep_preserved :
  forall ops T0 T,
    WF (view_store (t_view T0)) ->
    stage_all T0 ops = Some T ->
    WF (view_store (t_view T)) /\
    (forall H S rd, t_staged T0 = [] -> store_eq (view_store (t_view T0)) S ->
       WF (r_st (commit_run (t_mode T0) H S rd (t_staged T)))).
Proof.
  intros ops T0 T Hwf Hst. split; [eapply stage_all_WF; eassumption |].
  intros H S rd Hs0 Heq.
  destruct (view_commit_coherent ops T0 T H S rd Hs0 Hst Heq) as (n & Tn & _ & Hstn & Heqn & _).
  apply WF_store_eq with (S1 := view_store (t_view Tn)); [apply store_eq_sym, Heqn |].
  eapply stage_all_WF; eassumption.
Qed.

(* Disabled by decision 45 (Rule E): H11 is restated as peek_reproduces_trace_eq_host, with no kept records.
(* H11 (FR-10): peek fires against HEAD.
   1. A committed transaction makes the observer calls, the events, the
      status and the bindings of the host run of the same operations. By
      FR-10, the agent stages the re-offer of the incoming datum as the next
      operation (reoffers). So the trace equality with HEAD covers HEAD's
      re-produce of the incoming datum.
   2. A peek fire keeps its stored records in place (decision 19): on the
      consume side, and on the produce side for joins and repeated-channel
      groups. A produce-side fire never stores its incoming datum. So a peek
      fire changes no data list.
   3. Decision 19 is a deviation from HEAD. HEAD's produce_peeks also
      re-produces the stored records that a peek fire binds, and the
      substrate keeps them in place instead. The two stores differ only by
      these kept records, never by an incoming datum. On each channel, the
      re-produces of HEAD that the substrate replaces send exactly the
      values of the kept records. When no fire keeps a stored record, the
      stores are equal (data lists up to permutation), also after a
      re-offer.
   4. The re-offer has the trigger key of the fire. Staging accepts it as a
      second produce of the identity in the transaction, and each produce
      steps the counter by one. With no next waiter, the re-offer stores the
      datum. The receipt sets the persist flag of every kept stored result,
      and the incoming result keeps its own flag. So the results that are
      not persistent are the datum that the agent re-offers.
   5. On a head-only waiter list, the plan with the re-offer keeps the
      no-pending-match invariant and stages each due re-offer as the next
      operation. In the queue scenario (a peek waiter, then a consuming
      waiter), the plan stores nothing. With no second waiter, the re-offer
      stores the datum. The P1 rule (the fire stores the datum) breaks the
      invariant in the queue scenario. *)
Theorem peek_retention_trace_eq_host :
  (forall ops T0 T H S rd io rem,
     no_dir_some ->
     t_staged T0 = [] -> t_paired T0 = [] ->
     stage_all T0 ops = Some T ->
     store_eq (view_store (t_view T0)) S ->
     WF S ->
     host_valid_run (t_mode T0) H S rd ops ->
     replay_ctx T0 rd io rem ops ->
     Forall (fire_paired (t_mode T0)) (t_staged T) ->
     let rc := commit_run (t_mode T0) H S rd (t_staged T) in
     let rh := host_run (t_mode T0) H S rd ops in
     r_obs rc = r_obs rh /\ r_ev rc = r_ev rh /\ r_status rc = r_status rh /\ r_rd rc = r_rd rh /\
     exists kept,
       (forall c, Permutation (s_data (r_st rc) c) (s_data (r_st rh) c ++ kept c)) /\
       (forall g, s_conts (r_st rc) g = s_conts (r_st rh) g) /\
       (forall c, s_joins (r_st rc) c = s_joins (r_st rh) c) /\
       (forall p, s_count (r_st rc) p = s_count (r_st rh) p) /\
       (forall c d, In d (kept c) ->
          exists op cd, In op ops /\ retaining op = true /\ In cd (fire_cands op) /\
            removable cd = true /\ cd_chan cd = c /\ cd_rec cd = d) /\
       ((forall op c, In op ops -> retained op c = []) -> forall c, kept c = [])) /\
  (forall m H S rd sg, retaining (sg_op sg) = true ->
     forall c, s_data (r_st (commit_op m H S rd sg)) c = s_data S c) /\
  (forall op next, reoffer_of op next -> trigger_key next = trigger_key op) /\
  (forall T ch v chs wk kidx cands T1,
     stage T (OProduce ch v false (PFire chs wk kidx cands)) = Some T1 ->
     let src := produce_id ch v false in
     exists T2, stage T1 (OProduce ch v false PStore) = Some T2 /\
       vcount (t_view T1) src = S (vcount (t_view T) src) /\
       vcount (t_view T2) src = S (vcount (t_view T1) src) /\
       vdata (t_view T2) ch = mkDatum v false src :: vdata (t_view T1) ch) /\
  (forall T ch v persist chs wk kidx cands T1,
     stage T (OProduce ch v persist (PFire chs wk kidx cands)) = Some T1 -> w_peeks wk <> [] ->
     let retain := negb (nullb (w_peeks wk)) in
     (forall cd, In cd cands -> cd_incoming cd = false -> receipt_persistent retain cd = true) /\
     (forall cd, In cd cands -> cd_incoming cd = true -> receipt_persistent retain cd = persist) /\
     map (fun cd => (cd_chan cd, d_val (cd_rec cd)))
         (filter (fun cd => negb (receipt_persistent retain cd)) cands)
       = (if persist then [] else [(ch, v)]) /\
     Permutation (head_reproduces cands) ((if persist then [] else [(ch, v)]) ++ kept_reproduces cands)) /\
  (forall T chs pats k persist peeks cands T1,
     stage T (OConsume chs pats k persist peeks (CFire cands)) = Some T1 ->
     head_reproduces cands = kept_reproduces cands) /\
  (forall op c, retaining op = true ->
     values_on c (kept_reproduces (fire_cands op)) = map d_val (retained op c)) /\
  (forall wmatch ws rs d, no_pending_match wmatch ws rs ->
     no_pending_match wmatch (fst (offer wmatch ws rs d)) (snd (offer wmatch ws rs d))) /\
  (forall wmatch kidx_of cands_of ch v persist ws rs,
     reoffers (offer_ops wmatch kidx_of cands_of ch v persist ws rs)) /\
  (forall wmatch kidx_of cands_of ch v w1 w2,
     let d := mkDatum v false (produce_id ch v false) in
     let fire1 := OProduce ch v false (PFire [ch] w1 (kidx_of w1) (cands_of w1)) in
     let fire2 := OProduce ch v false (PFire [ch] w2 (kidx_of w2) (cands_of w2)) in
     w_persist w1 = false -> w_peeks w1 <> [] -> w_persist w2 = false -> w_peeks w2 = [] ->
     wmatch w1 d = true -> wmatch w2 d = true ->
     offer wmatch [w1; w2] [] d = ([], []) /\
     offer_ops wmatch kidx_of cands_of ch v false [w1; w2] [] = [fire1; fire2] /\
     offer wmatch [w1] [] d = ([], [d]) /\
     offer_ops wmatch kidx_of cands_of ch v false [w1] [] = [fire1; OProduce ch v false PStore]) /\
  (forall wmatch w1 w2 d,
     d_persist d = false -> w_persist w1 = false -> w_peeks w1 <> [] ->
     wmatch w1 d = true -> wmatch w2 d = true ->
     no_pending_match wmatch [w1; w2] [] /\
     offer_p1 wmatch [w1; w2] [] d = ([w2], [d]) /\
     ~ no_pending_match wmatch [w2] [d]).
Proof.
  split.
  { intros ops T0 T H S rd io rem Hnd Hs0 Hp0 Hst Heq Hwf Hhv Hctx Hfp.
    destruct (run_sim_fresh ops T0 T H S rd io rem Hnd Hs0 Hp0 Hst Heq Hwf Hhv Hctx Hfp)
      as (A & B & C & D & kept' & (Rd & Rc & Rj & Rn) & K1 & K2).
    split; [exact A |]. split; [exact B |]. split; [exact C |]. split; [exact D |].
    exists kept'. split; [exact Rd |]. split; [exact Rc |]. split; [exact Rj |]. split; [exact Rn |].
    split; [| exact K2].
    intros c d Hin. destruct (K1 c d Hin) as (op & Hop & Hr & Hd).
    destruct (retained_stored op c d Hd) as (cd & Hcd & Hrm & Hch & Hrec).
    exists op, cd. repeat split; assumption. }
  split; [exact peek_fire_keeps_data |].
  split; [exact reoffer_same_identity |].
  split; [exact reoffer_store_staged |].
  split.
  { intros T ch v persist chs wk kidx cands T1 Hst Hpk retain.
    assert (Hok : pfire_ok T ch v persist chs wk cands = true).
    { unfold stage in Hst. destruct (t_installed T ch); [discriminate |].
      destruct (pfire_ok T ch v persist chs wk cands); simpl in Hst; [reflexivity | discriminate]. }
    destruct (receipt_reports_reoffer T ch v persist chs wk cands Hok Hpk) as (R1 & R2 & R3).
    split; [exact R1 |]. split; [exact R2 |]. split; [exact R3 |].
    exact (head_reproduces_produce T ch v persist chs wk cands Hok). }
  split.
  { intros T chs pats k persist peeks cands T1 Hst.
    unfold stage in Hst. destruct (consume_args_ok T chs pats peeks); simpl in Hst; [| discriminate].
    destruct (cfire_ok T chs cands) eqn:Hcf; simpl in Hst; [| discriminate].
    exact (head_reproduces_consume T chs cands Hcf). }
  split; [exact kept_reproduces_retained |].
  split; [exact offer_keeps_no_pending_match |].
  split; [exact offer_ops_reoffers |].
  split; [exact offer_scenarios |].
  exact offer_p1_breaks_no_pending_match.
Qed.
*)

(* H11 (decision 45, Rule E), the trace part: a transaction in which a
   consume-side peek fire is followed by its re-stores commits as the host
   runs the same program with the produces of HEAD's produce_peeks in place
   of the re-stores. The commit makes the observer calls, the events, the
   status and the bindings of that host run, and the two stores are equal
   (data lists up to permutation). The operations before and after the
   peek are arbitrary. *)
Lemma peek_restores_sim :
  forall pre post chs pats k persist peeks cands T0 T H S rd io rem,
    let fire := OConsume chs pats k persist peeks (CFire cands) in
    let hops := pre ++ fire :: host_produce_peeks fire ++ post in
    no_dir_some ->
    t_staged T0 = [] -> t_paired T0 = [] ->
    stage_all T0 (pre ++ fire :: restores_after fire ++ post) = Some T ->
    store_eq (view_store (t_view T0)) S ->
    WF S ->
    host_valid_run (t_mode T0) H S rd hops ->
    replay_ctx T0 rd io rem hops ->
    Forall (fire_paired (t_mode T0)) (t_staged T) ->
    let rc := commit_run (t_mode T0) H S rd (t_staged T) in
    let rh := host_run (t_mode T0) H S rd hops in
    r_obs rc = r_obs rh /\ r_ev rc = r_ev rh /\ r_status rc = r_status rh /\ r_rd rc = r_rd rh /\
    (forall c, Permutation (s_data (r_st rc) c) (s_data (r_st rh) c)) /\
    (forall g, s_conts (r_st rc) g = s_conts (r_st rh) g) /\
    (forall c, s_joins (r_st rc) c = s_joins (r_st rh) c) /\
    (forall p, s_count (r_st rc) p = s_count (r_st rh) p).
Proof.
  intros pre post chs pats k persist peeks cands T0 T H S rd io rem fire hops
         Hnd Hs0 Hp0 Hst Heq Hwf Hhv Hctx Hfp rc rh.
  subst rc rh hops fire.
  assert (Hops : pre ++ OConsume chs pats k persist peeks (CFire cands)
                   :: restores_after (OConsume chs pats k persist peeks (CFire cands)) ++ post =
                 pre ++ OConsume chs pats k persist peeks (CFire cands)
                   :: host_produce_peeks (OConsume chs pats k persist peeks (CFire cands)) ++ post).
  { pose proof Hst as Hst1. rewrite stage_all_app in Hst1.
    destruct (stage_all T0 pre) as [T1 |]; [| discriminate].
    cbn [stage_all] in Hst1.
    destruct (stage T1 (OConsume chs pats k persist peeks (CFire cands))) as [T2 |] eqn:H2;
      [| discriminate].
    rewrite (restores_are_produce_peeks T1 chs pats k persist peeks cands T2 H2). reflexivity. }
  rewrite Hops in Hst.
  destruct (run_sim_fresh _ T0 T H S rd io rem Hnd Hs0 Hp0 Hst Heq Hwf Hhv Hctx Hfp)
    as (A & B & C & D & (Rd & Rc & Rj & Rn)).
  split; [exact A |]. split; [exact B |]. split; [exact C |]. split; [exact D |].
  split; [exact Rd |]. split; [exact Rc |]. split; [exact Rj | exact Rn].
Qed.

(* H11 (FR-10 and decision 45, Rule E): peek fires against HEAD.
   1. A consume-side peek fire followed by the re-stores of the agent has
      the observer calls, the events, the status, the bindings and the
      store (data lists up to permutation) of HEAD's peek COMM followed by
      produce_peeks, inside any transaction (peek_restores_sim).
   2. FR-10: the commit of a fire never stores a datum, in every mode and
      also when it is rejected. A produce-side fire never stores its
      incoming datum.
   3. The receipt of a fire keeps the persist flag of the record of each
      result, as wrap_result gives it. No flag is rewritten.
   4. The view of a staged fire loses exactly its stored non-persistent
      records, on both sides and also for a peek, as the host does.
   5. The re-stores that the agent stages after a consume-side fire are
      exactly the produces of HEAD's produce_peeks.
   6. A re-store has the identity of the source produce of its record,
      when each stored source is the Produce::create identity of its bytes.
   7. Each re-store is a staged Store of the record itself at the head of
      its channel, and it steps the counter of that identity by one.
   8. A consume-side peek fire followed by its re-stores leaves each data
      list a permutation of the list before the fire: no record is lost
      and none is duplicated.
   9. Staging keeps the premise of 6 (stored_sources).
   10. After a produce-side fire, HEAD's re-produces are the re-offer of
      the incoming datum and the re-stores of the removed stored records.
   11. Decision 30, kept as before: the re-offer has the trigger key of the
      fire. Staging accepts it as a second produce of the identity, and each
      produce steps the counter by one. With no next waiter, the re-offer
      stores the datum. On a head-only waiter list, the plan with the
      re-offer keeps the no-pending-match invariant and stages each due
      re-offer as the next operation. In the queue scenario (a peek waiter,
      then a consuming waiter), the plan stores nothing, and with no second
      waiter the re-offer stores the datum. The P1 rule (the fire stores
      the datum) breaks the invariant in the queue scenario. *)
Theorem peek_reproduces_trace_eq_host :
  (forall pre post chs pats k persist peeks cands T0 T H S rd io rem,
     let fire := OConsume chs pats k persist peeks (CFire cands) in
     let hops := pre ++ fire :: host_produce_peeks fire ++ post in
     no_dir_some ->
     t_staged T0 = [] -> t_paired T0 = [] ->
     stage_all T0 (pre ++ fire :: restores_after fire ++ post) = Some T ->
     store_eq (view_store (t_view T0)) S ->
     WF S ->
     host_valid_run (t_mode T0) H S rd hops ->
     replay_ctx T0 rd io rem hops ->
     Forall (fire_paired (t_mode T0)) (t_staged T) ->
     let rc := commit_run (t_mode T0) H S rd (t_staged T) in
     let rh := host_run (t_mode T0) H S rd hops in
     r_obs rc = r_obs rh /\ r_ev rc = r_ev rh /\ r_status rc = r_status rh /\ r_rd rc = r_rd rh /\
     (forall c, Permutation (s_data (r_st rc) c) (s_data (r_st rh) c)) /\
     (forall g, s_conts (r_st rc) g = s_conts (r_st rh) g) /\
     (forall c, s_joins (r_st rc) c = s_joins (r_st rh) c) /\
     (forall p, s_count (r_st rc) p = s_count (r_st rh) p)) /\
  (forall m H S rd sg, is_fire (sg_op sg) = true ->
     forall c x, count_occ datum_eq_dec (s_data (r_st (commit_op m H S rd sg)) c) x <=
                 count_occ datum_eq_dec (s_data S c) x) /\
  (forall cands,
     fired_receipt (length cands) (wrap_flags cands) = Some (map (fun cd => d_persist (cd_rec cd)) cands)) /\
  (forall T op T1, stage T op = Some T1 -> is_fire op = true ->
     forall c x, count_occ datum_eq_dec (vdata (t_view T1) c) x
                 + count_occ datum_eq_dec (np_recs c (fire_cands op)) x
                 = count_occ datum_eq_dec (vdata (t_view T) c) x) /\
  (forall T chs pats k persist peeks cands T1,
     stage T (OConsume chs pats k persist peeks (CFire cands)) = Some T1 ->
     restores_after (OConsume chs pats k persist peeks (CFire cands)) =
     host_produce_peeks (OConsume chs pats k persist peeks (CFire cands))) /\
  (forall V cd, stored_sources V -> In (cd_rec cd) (vdata V (cd_chan cd)) -> removable cd = true ->
     trigger_key (restore_of cd) = KProduce (d_src (cd_rec cd)) /\
     mkDatum (d_val (cd_rec cd)) false (produce_id (cd_chan cd) (d_val (cd_rec cd)) false) = cd_rec cd) /\
  (forall T chs pats k persist peeks cands T1 cd,
     stage T (OConsume chs pats k persist peeks (CFire cands)) = Some T1 ->
     In cd cands -> removable cd = true -> stored_sources (t_view T) ->
     exists T2, stage T1 (restore_of cd) = Some T2 /\
       trigger_key (restore_of cd) = KProduce (d_src (cd_rec cd)) /\
       vdata (t_view T2) (cd_chan cd) = cd_rec cd :: vdata (t_view T1) (cd_chan cd) /\
       vcount (t_view T2) (d_src (cd_rec cd)) = S (vcount (t_view T1) (d_src (cd_rec cd)))) /\
  (forall T chs pats k persist peeks cands T1,
     stage T (OConsume chs pats k persist peeks (CFire cands)) = Some T1 ->
     peeks <> [] -> stored_sources (t_view T) ->
     exists T2,
       stage_all T1 (restores_after (OConsume chs pats k persist peeks (CFire cands))) = Some T2 /\
       forall c, Permutation (vdata (t_view T2) c) (vdata (t_view T) c)) /\
  (forall T op T1, stage T op = Some T1 -> stored_sources (t_view T) -> stored_sources (t_view T1)) /\
  (forall T ch v persist chs wk kidx cands T1,
     stage T (OProduce ch v persist (PFire chs wk kidx cands)) = Some T1 ->
     Permutation (head_reproduces cands) ((if persist then [] else [(ch, v)]) ++ restore_reproduces cands)) /\
  (forall op next, reoffer_of op next -> trigger_key next = trigger_key op) /\
  (forall T ch v chs wk kidx cands T1,
     stage T (OProduce ch v false (PFire chs wk kidx cands)) = Some T1 ->
     let src := produce_id ch v false in
     exists T2, stage T1 (OProduce ch v false PStore) = Some T2 /\
       vcount (t_view T1) src = S (vcount (t_view T) src) /\
       vcount (t_view T2) src = S (vcount (t_view T1) src) /\
       vdata (t_view T2) ch = mkDatum v false src :: vdata (t_view T1) ch) /\
  (forall wmatch ws rs d, no_pending_match wmatch ws rs ->
     no_pending_match wmatch (fst (offer wmatch ws rs d)) (snd (offer wmatch ws rs d))) /\
  (forall wmatch kidx_of cands_of ch v persist ws rs,
     reoffers (offer_ops wmatch kidx_of cands_of ch v persist ws rs)) /\
  (forall wmatch kidx_of cands_of ch v w1 w2,
     let d := mkDatum v false (produce_id ch v false) in
     let fire1 := OProduce ch v false (PFire [ch] w1 (kidx_of w1) (cands_of w1)) in
     let fire2 := OProduce ch v false (PFire [ch] w2 (kidx_of w2) (cands_of w2)) in
     w_persist w1 = false -> w_peeks w1 <> [] -> w_persist w2 = false -> w_peeks w2 = [] ->
     wmatch w1 d = true -> wmatch w2 d = true ->
     offer wmatch [w1; w2] [] d = ([], []) /\
     offer_ops wmatch kidx_of cands_of ch v false [w1; w2] [] = [fire1; fire2] /\
     offer wmatch [w1] [] d = ([], [d]) /\
     offer_ops wmatch kidx_of cands_of ch v false [w1] [] = [fire1; OProduce ch v false PStore]) /\
  (forall wmatch w1 w2 d,
     d_persist d = false -> w_persist w1 = false -> w_peeks w1 <> [] ->
     wmatch w1 d = true -> wmatch w2 d = true ->
     no_pending_match wmatch [w1; w2] [] /\
     offer_p1 wmatch [w1; w2] [] d = ([w2], [d]) /\
     ~ no_pending_match wmatch [w2] [d]).
Proof.
  split; [exact peek_restores_sim |].
  split; [exact fire_stores_no_datum |].
  split; [exact receipt_flags_are_persist |].
  split; [exact fire_view_removes |].
  split; [exact restores_are_produce_peeks |].
  split; [exact restore_same_identity |].
  split; [exact restore_store_staged |].
  split; [exact peek_restore_keeps_data |].
  split; [exact stage_keeps_stored_sources |].
  split.
  { intros T ch v persist chs wk kidx cands T1 Hst.
    unfold stage in Hst. destruct (t_installed T ch); [discriminate |].
    destruct (pfire_ok T ch v persist chs wk cands) eqn:Hok; cbn [negb] in Hst; [| discriminate].
    exact (head_reproduces_produce T ch v persist chs wk cands Hok). }
  split; [exact reoffer_same_identity |].
  split; [exact reoffer_store_staged |].
  split; [exact offer_keeps_no_pending_match |].
  split; [exact offer_ops_reoffers |].
  split; [exact offer_scenarios |].
  exact offer_p1_breaks_no_pending_match.
Qed.

(* ═══════════════════ Event coverage (H10, design §1.4) ════════════════════ *)

(* Every COMM follows the IO event that triggered it. *)
Definition comm_after_trigger (evs : list Ev) : Prop :=
  forall pre sd cm post, evs = pre ++ EvComm sd cm :: post ->
    match sd with
    | ProduceSide => exists p, In p (c_produces cm) /\ In (EvProduce p) pre
    | ConsumeSide => In (EvConsume (c_consume cm)) pre
    end.

(* The six rules of the debug event-coverage check. *)
Definition covered (evs : list Ev) (fx : list Fx) : Prop :=
  (forall c d, In (FxPutDatum c d) fx -> In (EvProduce (d_src d)) evs) /\
  (forall c d, In (FxRemoveDatum c d) fx ->
     exists sd cm, In (EvComm sd cm) evs /\ In (d_src d) (c_produces cm)) /\
  (forall g w, In (FxPutCont g w) fx -> In (EvConsume (w_src w)) evs) /\
  (forall g w, In (FxRemoveCont g w) fx ->
     exists sd cm, In (EvComm sd cm) evs /\ c_consume cm = w_src w) /\
  (forall c g, In (FxPutJoin c g) fx -> exists w, In (FxPutCont g w) fx) /\
  (forall c g, In (FxRemoveJoin c g) fx -> exists w, In (FxRemoveCont g w) fx) /\
  comm_after_trigger evs.

Definition not_comm (e : Ev) : Prop := match e with EvComm _ _ => False | _ => True end.

Lemma cat_nil : comm_after_trigger [].
Proof. intros pre sd cm post E. destruct pre; discriminate. Qed.

Lemma cat_one : forall e, not_comm e -> comm_after_trigger [e].
Proof.
  intros e He pre sd cm post E.
  destruct pre as [| x pre']; simpl in E; injection E as E1 E2.
  - subst e. contradiction.
  - destruct pre'; discriminate.
Qed.

Lemma cat_two :
  forall e sd0 cm0, not_comm e ->
    (match sd0 with
     | ProduceSide => exists p, In p (c_produces cm0) /\ In (EvProduce p) [e]
     | ConsumeSide => In (EvConsume (c_consume cm0)) [e]
     end) ->
    comm_after_trigger [e; EvComm sd0 cm0].
Proof.
  intros e sd0 cm0 He Hc pre sd cm post E.
  destruct pre as [| x [| y pre']]; simpl in E.
  - injection E as E1 E2. subst e. contradiction.
  - injection E as E1 E2 E3 E4. subst. exact Hc.
  - injection E as _ _ E3. destruct pre'; discriminate.
Qed.

Lemma covered_nil : covered [] [].
Proof.
  repeat split; intros; try contradiction; try (destruct H0 as (? & ? & ? & ?); contradiction).
  apply cat_nil.
Qed.

Lemma put_joins_fx :
  forall cs S g f, In f (snd (put_joins S cs g)) ->
    exists c, f = FxPutJoin c g /\ In c cs /\ ~ In g (s_joins S c).
Proof.
  induction cs as [| c0 rest IH]; intros S g f Hin; simpl in Hin; [contradiction |].
  apply in_app_or in Hin as [Hin | Hin].
  - unfold put_join in Hin. destruct (in_dec lc_eq_dec g (s_joins S c0)) as [_ | Hn]; simpl in Hin; [contradiction |].
    destruct Hin as [E | []]. exists c0. split; [symmetry; exact E |]. split; [left; reflexivity | exact Hn].
  - destruct (IH _ g f Hin) as (c & Ef & Hc & Hn). exists c. split; [exact Ef |]. split; [right; exact Hc |].
    intro Hg. apply Hn. unfold put_join.
    destruct (in_dec lc_eq_dec g (s_joins S c0)); simpl; [exact Hg |].
    destruct (chan_eq_dec c c0); [right; subst; exact Hg | exact Hg].
Qed.

Lemma swc_fx :
  forall S chs w f, In f (snd (store_waiting_continuation S chs w)) ->
    (f = FxPutCont chs w /\ existsb (ident_eqb w) (s_conts S chs) = false) \/
    (exists c, f = FxPutJoin c chs /\ In c chs /\ ~ In chs (s_joins S c)).
Proof.
  intros S chs w f Hin. unfold store_waiting_continuation in Hin. simpl in Hin.
  apply in_app_or in Hin as [Hin | Hin].
  - left. unfold put_continuation in Hin.
    destruct (existsb (ident_eqb w) (s_conts S chs)) eqn:Ex; simpl in Hin; [contradiction |].
    destruct Hin as [E | []]. split; [symmetry; exact E | reflexivity].
  - right. destruct (put_joins_fx chs _ chs f Hin) as (c & Ef & Hc & Hn).
    exists c. split; [exact Ef |]. split; [exact Hc |].
    destruct (put_continuation_fields S chs w) as (_ & _ & Pj & _). rewrite Pj in Hn. exact Hn.
Qed.

Lemma swc_has_putcont :
  forall S chs w, existsb (ident_eqb w) (s_conts S chs) = false ->
    In (FxPutCont chs w) (snd (store_waiting_continuation S chs w)).
Proof.
  intros S chs w Ex. unfold store_waiting_continuation, put_continuation. rewrite Ex. simpl. left. reflexivity.
Qed.

Lemma space_pf_loop_fx :
  forall cs S chs f, In f (snd (space_pf_loop S chs cs)) ->
    (exists cd, In cd cs /\ f = FxRemoveDatum (cd_chan cd) (cd_rec cd)) \/
    (exists c, f = FxRemoveJoin c chs /\ s_conts S chs = []).
Proof.
  induction cs as [| cd rest IH]; intros S chs f Hin; simpl in Hin; [contradiction |].
  set (p1 := if removable cd then space_remove_datum S (cd_chan cd) (cd_rec cd) else (S, [])) in Hin.
  assert (C1 : forall g, s_conts (fst p1) g = s_conts S g).
  { intros g. subst p1. destruct (removable cd); [| reflexivity].
    destruct (space_remove_datum_fields S (cd_chan cd) (cd_rec cd)) as (_ & X & _). apply X. }
  apply in_app_or in Hin as [Hin | Hin].
  - left. exists cd. split; [left; reflexivity |]. subst p1.
    destruct (removable cd); [| contradiction].
    unfold space_remove_datum in Hin. destruct (in_dec datum_eq_dec (cd_rec cd) (s_data S (cd_chan cd)));
      simpl in Hin; [destruct Hin as [E | []]; symmetry; exact E | contradiction].
  - apply in_app_or in Hin as [Hin | Hin].
    + right. unfold remove_join in Hin.
      destruct (s_conts (fst p1) chs) as [| w ws] eqn:Ec; [| contradiction].
      destruct (in_dec lc_eq_dec chs (s_joins (fst p1) (cd_chan cd))); simpl in Hin; [| contradiction].
      destruct Hin as [E | []]. exists (cd_chan cd). split; [symmetry; exact E | rewrite <- C1; exact Ec].
    + destruct (IH _ chs f Hin) as [(cd' & Hcd' & Ef) | (c & Ef & Ec)].
      * left. exists cd'. split; [right; exact Hcd' | exact Ef].
      * right. exists c. split; [exact Ef |].
        destruct (remove_join_fields (fst p1) (cd_chan cd) chs) as (_ & Rc & _).
        rewrite Rc, C1 in Ec. exact Ec.
Qed.

Lemma space_cf_loop_fx :
  forall cs S f, In f (snd (space_cf_loop S cs)) ->
    exists cd, In cd cs /\ f = FxRemoveDatum (cd_chan cd) (cd_rec cd).
Proof.
  induction cs as [| cd rest IH]; intros S f Hin; simpl in Hin; [contradiction |].
  apply in_app_or in Hin as [Hin | Hin].
  - exists cd. split; [left; reflexivity |].
    destruct (negb (d_persist (cd_rec cd))); [| contradiction].
    unfold space_remove_datum in Hin. destruct (in_dec datum_eq_dec (cd_rec cd) (s_data S (cd_chan cd)));
      simpl in Hin; [destruct Hin as [E | []]; symmetry; exact E | contradiction].
  - destruct (IH _ f Hin) as (cd' & Hcd' & Ef). exists cd'. split; [right; exact Hcd' | exact Ef].
Qed.

Lemma cand_src_in_comm :
  forall cands cd consume peeks closure,
    In cd cands -> In (d_src (cd_rec cd)) (c_produces (mk_comm cands consume peeks closure)).
Proof.
  intros cands cd consume peeks closure Hin. unfold mk_comm. cbn [c_produces].
  eapply Permutation_in; [apply Permutation_sym, comm_sort_perm |].
  unfold cand_srcs. apply in_map_iff. exists cd. split; [reflexivity | exact Hin].
Qed.

(* H10: for every committed operation, each store change is covered by an
   event of the same operation, and every COMM follows its trigger. *)
Theorem coverage_total :
  forall T op T' H S rd,
    stage T op = Some T' ->
    store_eq (view_store (t_view T)) S ->
    WF S ->
    exists sg, t_staged T' = t_staged T ++ [sg] /\
      covered (r_ev (commit_op (t_mode T) H S rd sg)) (r_fx (commit_op (t_mode T) H S rd sg)).
Proof.
  intros T op T' H S rd Hst Heq Hwf.
  pose proof Heq as (Ed & Ec & Ej & En).
  destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]].
  - (* produce, Store *)
    unfold stage in Hst. destruct (t_installed T ch); [discriminate |]. injection Hst as <-.
    eexists. split; [reflexivity |].
    destruct (coh_produce_store (t_mode T) H S rd ch v persist (vjoins (t_view T) ch) false)
      as [(_ & _ & _ & Hev & Hfx) | (_ & _ & _ & Hev & Hfx)]; rewrite Hev, Hfx; [| apply covered_nil].
    simpl. repeat split; intros; simpl in *;
      repeat match goal with H0 : _ \/ _ |- _ => destruct H0 end; try discriminate; try contradiction.
    + injection H0 as <- <-. left. reflexivity.
    + apply cat_one. exact I.
  - (* produce, Fire *)
    unfold stage in Hst. destruct (t_installed T ch); [discriminate |].
    destruct (pfire_ok T ch v persist chs wk cands) eqn:Hok; cbn [negb] in Hst; [| discriminate].
    set (cm := mk_comm cands (w_src wk) (w_peeks wk)
                 (play_counters_closure (vcount (t_view T)) (produce_id ch v persist) persist)) in Hst.
    apply stage_fire_some in Hst. subst T'.
    destruct (pfire_ok_spec T ch v persist chs wk cands Hok) as (_ & _ & Hmap & Hex & Hinc & Hwk & _).
    eexists. split; [reflexivity |].
    assert (Hcm : cm = mk_comm cands (w_src wk) (w_peeks wk)
                         (play_counters_closure (s_count S) (produce_id ch v persist) persist)).
    { subst cm. apply mk_comm_play_ext. intros p. apply En. }
    destruct (coh_produce_fire (t_mode T) H S rd ch v persist chs wk kidx cands (vjoins (t_view T) ch) cm
                (snd (pair_step T (KProduce (produce_id ch v persist)) cm)) Hcm)
      as [(_ & _ & _ & Hev & Hfx) | (_ & _ & _ & _ & _ & Hev & Hfx)]; rewrite Hev, Hfx; [| apply covered_nil].
    set (src := produce_id ch v persist).
    assert (Hwk' : In wk (s_conts S chs)) by (rewrite <- Ec; exact Hwk).
    set (S1 := bump S src persist).
    set (p1 := if w_persist wk then (S1, []) else space_remove_continuation S1 chs wk).
    set (p2 := space_pf_loop (fst p1) chs (sort_desc vkey cands)).
    assert (Hsrc : In src (c_produces cm)) by (subst cm; apply incoming_src_in_produces; assumption).
    assert (Fp1 : forall f, In f (snd p1) -> f = FxRemoveCont chs wk).
    { intros f Hf. subst p1. destruct (w_persist wk); [contradiction |].
      unfold space_remove_continuation in Hf. destruct (in_dec wk_eq_dec wk (s_conts S1 chs)); simpl in Hf;
        [destruct Hf as [E | []]; symmetry; exact E | contradiction]. }
    assert (Rjoin : forall c, In (FxRemoveJoin c chs) (snd p2) -> In (FxRemoveCont chs wk) (snd p1)).
    { intros c Hf. destruct (space_pf_loop_fx _ _ _ _ Hf) as [(cd & _ & E) | (c' & _ & Ec0)]; [discriminate |].
      subst p1. destruct (w_persist wk) eqn:Ep.
      - exfalso. simpl in Ec0. subst S1. unfold bump, upd_count in Ec0. simpl in Ec0. rewrite Ec0 in Hwk'. contradiction.
      - unfold space_remove_continuation. subst S1. unfold bump, upd_count. cbn [s_conts snd].
        destruct (in_dec wk_eq_dec wk (s_conts S chs)) as [_ | Cn]; [left; reflexivity | contradiction]. }
    (* FR-10: a fire puts no datum, so its effects are the continuation
       removal (p1) and the loop of row 6 (p2). *)
    split; [| split; [| split; [| split; [| split; [| split]]]]].
    + intros c d Hf. apply in_app_or in Hf as [Hf | Hf]; [apply Fp1 in Hf; discriminate |].
      destruct (space_pf_loop_fx _ _ _ _ Hf) as [(cd & _ & E) | (c' & E & _)]; discriminate.
    + intros c d Hf. apply in_app_or in Hf as [Hf | Hf]; [apply Fp1 in Hf; discriminate |].
      destruct (space_pf_loop_fx _ _ _ _ Hf) as [(cd & Hcd & E) | (c' & E & _)]; [| discriminate].
      injection E as _ Hd. subst d. exists ProduceSide, cm. split; [right; left; reflexivity |].
      subst cm. apply cand_src_in_comm. eapply Permutation_in; [apply sort_desc_perm | exact Hcd].
    + intros g w Hf. apply in_app_or in Hf as [Hf | Hf]; [apply Fp1 in Hf; discriminate |].
      destruct (space_pf_loop_fx _ _ _ _ Hf) as [(cd & _ & E) | (c' & E & _)]; discriminate.
    + intros g w Hf. apply in_app_or in Hf as [Hf | Hf].
      * apply Fp1 in Hf. injection Hf as Hg Hw. subst g w. exists ProduceSide, cm. split; [right; left; reflexivity |].
        subst cm. reflexivity.
      * destruct (space_pf_loop_fx _ _ _ _ Hf) as [(cd & _ & E) | (c' & E & _)]; discriminate.
    + intros c g Hf. apply in_app_or in Hf as [Hf | Hf]; [apply Fp1 in Hf; discriminate |].
      destruct (space_pf_loop_fx _ _ _ _ Hf) as [(cd & _ & E) | (c' & E & _)]; discriminate.
    + intros c g Hf. apply in_app_or in Hf as [Hf | Hf]; [apply Fp1 in Hf; discriminate |].
      destruct (space_pf_loop_fx _ _ _ _ Hf) as [(cd & _ & E) | (c' & E & _)]; [discriminate |].
      injection E as Hc Hg. subst c' g. exists wk. apply in_or_app. left. apply (Rjoin c). exact Hf.
    + apply cat_two; [exact I |]. exists src. split; [exact Hsrc | left; reflexivity].
  - (* consume, Park *)
    unfold stage in Hst. destruct (consume_args_ok T chs pats peeks); simpl in Hst; [| discriminate].
    injection Hst as <-. eexists. split; [reflexivity |].
    destruct (coh_consume_park (t_mode T) H S rd chs pats k persist peeks false)
      as [(_ & _ & _ & Hev & Hfx) | (_ & _ & _ & Hev & Hfx)]; rewrite Hev, Hfx; [| apply covered_nil].
    set (src := consume_id chs pats k persist).
    set (w := mkWK pats k persist peeks src).
    destruct Hwf as (L & _ & _). unfold lockstep in L.
    split; [| split; [| split; [| split; [| split; [| split]]]]].
    + intros c d Hf. destruct (swc_fx _ _ _ _ Hf) as [(E & _) | (c' & E & _)]; discriminate.
    + intros c d Hf. destruct (swc_fx _ _ _ _ Hf) as [(E & _) | (c' & E & _)]; discriminate.
    + intros g w' Hf. destruct (swc_fx _ _ _ _ Hf) as [(E & _) | (c' & E & _)]; [| discriminate].
      injection E as Hg Hw. subst w'. left. reflexivity.
    + intros g w' Hf. destruct (swc_fx _ _ _ _ Hf) as [(E & _) | (c' & E & _)]; discriminate.
    + intros c g Hf. destruct (swc_fx _ _ _ _ Hf) as [(E & _) | (c' & E & Hc' & Hn)]; [discriminate |].
      injection E as Hc Hg. subst c' g. exists w. apply swc_has_putcont.
      destruct (s_conts S chs) as [| w0 ws] eqn:Es; [reflexivity |].
      exfalso. apply Hn. apply L. split; [exact Hc' | rewrite Es; discriminate].
    + intros c g Hf. destruct (swc_fx _ _ _ _ Hf) as [(E & _) | (c' & E & _)]; discriminate.
    + apply cat_one. exact I.
  - (* consume, Fire *)
    unfold stage in Hst. destruct (consume_args_ok T chs pats peeks); cbn [negb] in Hst; [| discriminate].
    destruct (cfire_ok T chs cands) eqn:Hcf; cbn [negb] in Hst; [| discriminate].
    set (cm := mk_comm cands (consume_id chs pats k persist) peeks (counters_of (vcount (t_view T)))) in Hst.
    apply stage_fire_some in Hst. subst T'.
    eexists. split; [reflexivity |].
    assert (Hcm : cm = mk_comm cands (consume_id chs pats k persist) peeks (counters_of (s_count S))).
    { subst cm. apply mk_comm_counters_ext. intros p. apply En. }
    destruct (coh_consume_fire (t_mode T) H S rd chs pats k persist peeks cands cm
                (snd (pair_step T (KConsume (consume_id chs pats k persist)) cm)) Hcm)
      as [(_ & _ & _ & Hev & Hfx) | (_ & _ & _ & _ & _ & Hev & Hfx)]; rewrite Hev, Hfx; [| apply covered_nil].
    split; [| split; [| split; [| split; [| split; [| split]]]]].
    + intros c d Hf. destruct (space_cf_loop_fx _ _ _ Hf) as (cd & _ & E). discriminate.
    + intros c d Hf. destruct (space_cf_loop_fx _ _ _ Hf) as (cd & Hcd & E).
      injection E as _ Hd. subst d. exists ConsumeSide, cm. split; [right; left; reflexivity |].
      subst cm. apply cand_src_in_comm. eapply Permutation_in; [apply sort_desc_perm | exact Hcd].
    + intros g w Hf. destruct (space_cf_loop_fx _ _ _ Hf) as (cd & _ & E). discriminate.
    + intros g w Hf. destruct (space_cf_loop_fx _ _ _ Hf) as (cd & _ & E). discriminate.
    + intros c g Hf. destruct (space_cf_loop_fx _ _ _ Hf) as (cd & _ & E). discriminate.
    + intros c g Hf. destruct (space_cf_loop_fx _ _ _ Hf) as (cd & _ & E). discriminate.
    + apply cat_two; [exact I |]. subst cm. left. reflexivity.
Qed.

(* H3: the play closure on κ (it counts the produce before log_produce bumps
   the counter) gives the same COMM as the replay rule (increment first, then
   count). A rejected replay fire rolls the increment back exactly. The
   staged counter view κ_v = κ_host + δ advances like the host counter. *)
Theorem counter_timing_play_replay :
  forall (k : PId -> nat) src persist cands consume peeks,
    mk_comm cands consume peeks (play_counters_closure k src persist) =
    mk_comm cands consume peeks (counters_of (incr k src persist)) /\
    (forall p, rollback (incr k src persist) src persist p = k p) /\
    (forall S, store_eq (unbump (bump S src persist) src persist) S) /\
    (forall V, store_eq (view_store (vbump V src persist)) (bump (view_store V) src persist)).
Proof.
  intros k src persist cands consume peeks.
  split; [unfold mk_comm; f_equal; apply play_closure_is_replay_closure |].
  split; [| split; [intros S; apply bump_unbump_eq | intros V; apply view_bump]].
  intros p. unfold rollback, incr. destruct (negb persist && pid_eqb p src); simpl; lia.
Qed.

Lemma verify_sound :
  forall rd io rem paired k c,
    binding_inv rd io rem -> verify rd paired k c = true -> count_occ comm_eq_dec paired c < rem c.
Proof.
  intros rd io rem paired k c Hinv Hv.
  apply (verify_spec rd io rem paired k c Hinv) in Hv as (_ & _ & Hlt). exact Hlt.
Qed.

(* Decision 36: a replay fire whose trigger event the log lacks does not
   pair, so it stages unpaired. Its staging adds no pairing and leaves the
   recorded bindings. The commit fails it after the calls of play
   (unpaired_fire_inert). Under decision 19 it failed at staging. *)
Lemma stage_without_trigger_unpaired :
  forall T op io rem T',
    backend (t_mode T) <> Play ->
    binding_inv (t_rd T) io rem ->
    ~ In (trigger_key op) io ->
    is_fire op = true ->
    stage T op = Some T' ->
    exists sg, t_staged T' = t_staged T ++ [sg] /\ sg_op sg = op /\ sg_paired sg = false /\
      t_paired T' = t_paired T /\ t_rd T' = t_rd T.
Proof.
  intros T op io rem T' Hnp Hinv Hni _ Hst.
  destruct (commit_op_coherent T op T' [] (view_store (t_view T)) (fun _ _ => 0) Hst (store_eq_refl _))
    as (sg & Hsg & Hop & _ & _ & Hrd & Hpn & Hps & _).
  exists sg. split; [exact Hsg |]. split; [exact Hop |].
  destruct (sg_comm sg) as [cm |] eqn:Hc.
  - destruct (Hps cm eq_refl) as (Hp & Hsp & _ & _).
    assert (Hv : verify (t_rd T) (t_paired T) (trigger_key op) cm = false).
    { apply Bool.not_true_iff_false. intros Hv.
      apply (verify_spec (t_rd T) io rem _ _ _ Hinv) in Hv as (Hio & _). exact (Hni Hio). }
    assert (Hsp' : sg_paired sg = false).
    { rewrite Hsp. destruct (backend (t_mode T)); [reflexivity | exact Hv | exact Hv]. }
    split; [exact Hsp' |]. split; [rewrite Hp, Hsp', app_nil_r; reflexivity | exact Hrd].
  - destruct (Hpn eq_refl) as (Hp & Hsp). split; [exact Hsp |]. split; [exact Hp | exact Hrd].
Qed.

(* Decision 36: an unpaired replay fire never commits. When the observer
   accepts every call of the operation, it fails with ReplayDivergence
   (StDiverged); otherwise the rejected call stops it first, as in play. It
   emits no event and no effect, it leaves the bindings, and the store keeps
   its state: the counter step rolls back. *)
Lemma unpaired_fire_inert :
  forall m H S rd sg,
    backend m <> Play -> is_fire (sg_op sg) = true -> sg_paired sg = false ->
    let r := commit_op m H S rd sg in
    r_status r <> StOk /\ r_ev r = [] /\ r_fx r = [] /\ r_rd r = rd /\ store_eq (r_st r) S.
Proof.
  intros m H S rd [op j0 scomm sp] Hnp Hf Hsp r. subst r.
  cbn [sg_op sg_paired] in Hf, Hsp.
  unfold commit_op. cbn [sg_op sg_joins0 sg_comm sg_paired].
  destruct (backend m) eqn:Hb; [contradiction | |];
    (destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]];
     cbn [is_fire] in Hf; try discriminate);
    [unfold commit_replay_produce | unfold commit_replay_consume
    | unfold commit_replay_produce | unfold commit_replay_consume];
    cbn zeta; destruct scomm as [cm |]; commit_split; cbn [r_status r_st r_rd r_ev r_fx];
    first
      [ match goal with
        | E : space_prelude _ _ _ _ _ = PreStop _ ?st |- _ => apply space_prelude_status in E; subst st
        | |- _ => idtac
        end;
        split; [discriminate | split; [reflexivity | split; [reflexivity | split;
          [reflexivity | first [apply store_eq_refl | apply bump_unbump_eq]]]]]
      | exfalso; match goal with E : negb sp = false |- _ => rewrite Hsp in E; discriminate E end ].
Qed.

(* H4 (FR-3, decision 36): verify looks up only the trigger key, and it
   accepts a COMM exactly when the log holds the trigger event and an
   unpaired recorded occurrence remains. recorded_comms reports exactly the
   COMMs that verify accepts under the same key. A replay fire whose log
   lacks the trigger event stages unpaired: it adds no pairing and leaves
   the bindings. An unpaired fire never commits: it emits no event and no
   effect, leaves the bindings and keeps the store. The rule before FR-3
   pairs such a fire (review D3), and the literal design rule over-pairs a
   repeated produce. *)
Theorem verify_sound_complete :
  (forall rd paired k c, verify rd paired k c = true <-> 0 < available rd paired k c) /\
  (forall rd io rem paired k c,
     binding_inv rd io rem ->
     (verify rd paired k c = true <->
        In k io /\ In k (keys c) /\ count_occ comm_eq_dec paired c < rem c)) /\
  (forall T op io rem T',
     backend (t_mode T) <> Play -> binding_inv (t_rd T) io rem ->
     ~ In (trigger_key op) io -> is_fire op = true -> stage T op = Some T' ->
     exists sg, t_staged T' = t_staged T ++ [sg] /\ sg_op sg = op /\ sg_paired sg = false /\
       t_paired T' = t_paired T /\ t_rd T' = t_rd T) /\
  (forall cons p peeks times rd,
     let c := mkComm cons [p] peeks times in
     binding_inv rd [KConsume cons] (fun c' => if comm_eq_dec c' c then 1 else 0) ->
     verify_any_key rd [] c = true /\ verify rd [] (KProduce p) c = false) /\
  (forall cons p peeks times io rd,
     In (KProduce p) io ->
     let c := mkComm cons [p; p] peeks times in
     binding_inv rd io (fun c' => if comm_eq_dec c' c then 1 else 0) ->
     verify_literal rd [c] c = true /\ verify rd [c] (KProduce p) c = false /\
     verify_any_key rd [c] c = false /\ count_occ comm_eq_dec [c] c = 1) /\
  (forall m H S rd sg,
     backend m <> Play -> is_fire (sg_op sg) = true -> sg_paired sg = false ->
     let r := commit_op m H S rd sg in
     r_status r <> StOk /\ r_ev r = [] /\ r_fx r = [] /\ r_rd r = rd /\ store_eq (r_st r) S).
Proof.
  split; [| split; [| split; [| split; [| split]]]].
  - exact available_spec.
  - exact verify_spec.
  - exact stage_without_trigger_unpaired.
  - exact any_key_pairs_without_trigger.
  - exact design_verify_overpairs.
  - exact unpaired_fire_inert.
Qed.

(* ── Pairing over a whole transaction (H5) ─────────────────────────────── *)

(* The COMMs that the staged operations pair in replay: the COMM of each
   paired fire. An unpaired fire pairs nothing (decision 36). *)
Definition paired_of (m : Mode) (sgs : list Staged) : list Comm :=
  match backend m with
  | Play => []
  | _ => flat_map (fun sg => match sg_comm sg with
                             | Some cm => if sg_paired sg then [cm] else []
                             | None => []
                             end) sgs
  end.

(* A staged store or park never carries a COMM. *)
Definition sg_wf (sg : Staged) : Prop :=
  match sg_op sg with
  | OProduce _ _ _ PStore | OConsume _ _ _ _ _ CPark => sg_comm sg = None
  | _ => True
  end.

Lemma stage_sg_wf :
  forall T op T', stage T op = Some T' -> exists sg, t_staged T' = t_staged T ++ [sg] /\ sg_wf sg.
Proof.
  intros T op T' Hst.
  destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]];
    unfold stage in Hst.
  - destruct (t_installed T ch); [discriminate |]. injection Hst as <-.
    eexists. split; [reflexivity | reflexivity].
  - destruct (t_installed T ch); [discriminate |].
    destruct (pfire_ok T ch v persist chs wk cands); cbn [negb] in Hst; [| discriminate].
    apply stage_fire_some in Hst. subst T'.
    eexists. split; [reflexivity | exact I].
  - destruct (consume_args_ok T chs pats peeks); simpl in Hst; [| discriminate].
    injection Hst as <-. eexists. split; [reflexivity | reflexivity].
  - destruct (consume_args_ok T chs pats peeks); cbn [negb] in Hst; [| discriminate].
    destruct (cfire_ok T chs cands); cbn [negb] in Hst; [| discriminate].
    apply stage_fire_some in Hst. subst T'.
    eexists. split; [reflexivity | exact I].
Qed.

Lemma stage_all_sg_wf :
  forall ops T T', stage_all T ops = Some T' -> Forall sg_wf (t_staged T) -> Forall sg_wf (t_staged T').
Proof.
  induction ops as [| op rest IH]; intros T T' Hst Hwf; simpl in Hst.
  - injection Hst as <-. exact Hwf.
  - destruct (stage T op) as [T1 |] eqn:H1; [| discriminate].
    destruct (stage_sg_wf T op T1 H1) as (sg & Hsg & Hw).
    apply (IH T1 T' Hst). rewrite Hsg. apply Forall_app. split; [exact Hwf | constructor; [exact Hw | constructor]].
Qed.

(* A well-formed staged operation with a COMM is a fire. *)
Lemma sg_wf_comm_fire : forall sg cm, sg_wf sg -> sg_comm sg = Some cm -> is_fire (sg_op sg) = true.
Proof.
  intros [op j0 scomm sp] cm Hw Hc. unfold sg_wf in Hw. cbn [sg_op sg_comm] in *.
  destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]];
    cbn in Hw |- *; congruence.
Qed.

Lemma commit_op_rd :
  forall m H S rd sg, sg_wf sg ->
    r_rd (commit_op m H S rd sg) = rd_after m (r_status (commit_op m H S rd sg)) (sg_comm sg) rd.
Proof.
  intros m H S rd [op j0 scomm sp] Hwf. unfold sg_wf in Hwf. cbn [sg_op sg_comm] in Hwf.
  unfold commit_op, rd_after. cbn [sg_op sg_joins0 sg_comm sg_paired].
  destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]];
    cbn iota in Hwf; try subst scomm;
    destruct (backend m) eqn:Hb;
    unfold commit_play_produce, commit_replay_produce, commit_play_consume, commit_replay_consume;
    cbn zeta; try destruct scomm as [cm |]; commit_split; cbn [r_rd r_status];
    first [ reflexivity
          | match goal with
            | E : space_prelude _ _ _ _ _ = PreStop _ ?st |- _ =>
                apply space_prelude_status in E; subst st; reflexivity
            end ].
Qed.

(* A committed replay fire paired (decision 36, unpaired_fire_inert). *)
Lemma commit_run_rd :
  forall sgs m H S rd, Forall sg_wf sgs -> exists n,
    r_rd (commit_run m H S rd sgs) = fold_left unbind (paired_of m (firstn n sgs)) rd.
Proof.
  induction sgs as [| sg rest IH]; intros m H S rd Hwf; simpl.
  - exists 0. unfold paired_of. destruct (backend m); reflexivity.
  - inversion Hwf as [| sg' rest' Hw Hwr]; subst.
    destruct (r_status (commit_op m H S rd sg)) eqn:Hs.
    + destruct (IH m (H ++ r_obs (commit_op m H S rd sg)) (r_st (commit_op m H S rd sg))
                  (r_rd (commit_op m H S rd sg)) Hwr) as (n & Hn).
      exists (Datatypes.S n). simpl. rewrite Hn, (commit_op_rd m H S rd sg Hw), Hs.
      unfold paired_of, rd_after. simpl.
      destruct (backend m) eqn:Hb; [reflexivity | |];
        (destruct (sg_comm sg) as [cm |] eqn:Hc; [| reflexivity]);
        (destruct (sg_paired sg) eqn:Hp; [reflexivity |]);
        exfalso;
        apply (proj1 (unpaired_fire_inert m H S rd sg ltac:(first [discriminate | rewrite Hb; discriminate])
                        (sg_wf_comm_fire sg cm Hw Hc) Hp));
        exact Hs.
    + exists 0. rewrite (commit_op_rd m H S rd sg Hw), Hs. unfold paired_of, rd_after. simpl.
      destruct (backend m); [reflexivity | |]; destruct (sg_comm sg); reflexivity.
    + exists 0. rewrite (commit_op_rd m H S rd sg Hw), Hs. unfold paired_of, rd_after. simpl.
      destruct (backend m); [reflexivity | |]; destruct (sg_comm sg); reflexivity.
    + exists 0. rewrite (commit_op_rd m H S rd sg Hw), Hs. unfold paired_of, rd_after. simpl.
      destruct (backend m); [reflexivity | |]; destruct (sg_comm sg); reflexivity.
    + exists 0. rewrite (commit_op_rd m H S rd sg Hw), Hs. unfold paired_of, rd_after. simpl.
      destruct (backend m); [reflexivity | |]; destruct (sg_comm sg); reflexivity.
Qed.

Lemma paired_of_app : forall m l1 l2, paired_of m (l1 ++ l2) = paired_of m l1 ++ paired_of m l2.
Proof. intros m l1 l2. unfold paired_of. destruct (backend m); try reflexivity; apply flat_map_app. Qed.

Lemma stage_all_paired :
  forall ops T T', stage_all T ops = Some T' ->
    exists new, t_staged T' = t_staged T ++ new /\
                t_paired T' = t_paired T ++ paired_of (t_mode T) new.
Proof.
  induction ops as [| op rest IH]; intros T T' Hst; simpl in Hst.
  - injection Hst as <-. exists []. rewrite !app_nil_r. split; [reflexivity |].
    unfold paired_of. destruct (backend (t_mode T)); rewrite app_nil_r; reflexivity.
  - destruct (stage T op) as [T1 |] eqn:H1; [| discriminate].
    destruct (commit_op_coherent T op T1 [] (view_store (t_view T)) (fun _ _ => 0) H1 (store_eq_refl _))
      as (sg & Hsg & _ & Hmode & _ & _ & Hpn & Hps & _).
    destruct (IH T1 T' Hst) as (new & Hnew & Hp).
    exists (sg :: new). split; [rewrite Hnew, Hsg, <- app_assoc; reflexivity |].
    rewrite Hp, Hmode. change (sg :: new) with ([sg] ++ new). rewrite paired_of_app, app_assoc. f_equal.
    unfold paired_of. simpl.
    destruct (sg_comm sg) as [cm |] eqn:Hc.
    + destruct (Hps cm eq_refl) as (Hpt & Hsp & _ & _). rewrite Hpt. revert Hsp.
      destruct (backend (t_mode T)); intros Hsp; rewrite Hsp; simpl; rewrite ?app_nil_r;
        [reflexivity | |]; destruct (verify _ _ _ _); reflexivity.
    + rewrite (proj1 (Hpn eq_refl)). destruct (backend (t_mode T)); rewrite ?app_nil_r; reflexivity.
Qed.

Lemma stage_all_paired_bound :
  forall ops T T' io rem,
    backend (t_mode T) <> Play ->
    binding_inv (t_rd T) io rem ->
    (forall c, count_occ comm_eq_dec (t_paired T) c <= rem c) ->
    stage_all T ops = Some T' ->
    forall c, count_occ comm_eq_dec (t_paired T') c <= rem c.
Proof.
  induction ops as [| op rest IH]; intros T T' io rem Hnp Hinv Hb Hst; simpl in Hst.
  - injection Hst as <-. exact Hb.
  - destruct (stage T op) as [T1 |] eqn:H1; [| discriminate].
    destruct (commit_op_coherent T op T1 [] (view_store (t_view T)) (fun _ _ => 0) H1 (store_eq_refl _))
      as (sg & _ & _ & Hmode & _ & Hrd & Hpn & Hps & _).
    apply (IH T1 T' io rem); [rewrite Hmode; exact Hnp | rewrite Hrd; exact Hinv | | exact Hst].
    intros c. destruct (sg_comm sg) as [cm |] eqn:Hc.
    + destruct (Hps cm eq_refl) as (Hpt & Hsp & _ & _). rewrite Hpt.
      destruct (sg_paired sg) eqn:Hp.
      * assert (Hv : verify (t_rd T) (t_paired T) (trigger_key op) cm = true).
        { revert Hsp. destruct (backend (t_mode T)) eqn:Hb';
            [exfalso; apply Hnp; first [reflexivity | exact Hb'] | |]; intros Hsp; symmetry; exact Hsp. }
        assert (Hlt := verify_sound _ _ _ _ _ _ Hinv Hv).
        rewrite count_occ_app. simpl.
        destruct (comm_eq_dec cm c) as [E | N]; [subst; lia | specialize (Hb c); lia].
      * rewrite app_nil_r. apply Hb.
    + rewrite (proj1 (Hpn eq_refl)). apply Hb.
Qed.

Lemma count_firstn_le :
  forall m n sgs c,
    count_occ comm_eq_dec (paired_of m (firstn n sgs)) c <= count_occ comm_eq_dec (paired_of m sgs) c.
Proof.
  intros m n sgs c. rewrite <- (firstn_skipn n sgs) at 2. rewrite paired_of_app, count_occ_app. lia.
Qed.

(* H5: rig establishes the binding invariant from the play log. A replay
   transaction never stages more pairings of a COMM than remain recorded.
   Its commit removes exactly the committed pairings. When
   check_replay_data passes, replay paired every triggered COMM fully. *)
Theorem replay_pairs_each_comm_once :
  (forall log, binding_inv (rig log) (io_of_log log)
                 (fun c => count_occ comm_eq_dec (comms_of_log log) c)) /\
  (forall ops T0 T H S io rem,
     backend (t_mode T0) <> Play -> t_staged T0 = [] -> t_paired T0 = [] ->
     binding_inv (t_rd T0) io rem ->
     stage_all T0 ops = Some T ->
     (forall c, count_occ comm_eq_dec (t_paired T) c <= rem c) /\
     exists applied,
       (forall c, count_occ comm_eq_dec applied c <= count_occ comm_eq_dec (t_paired T) c) /\
       binding_inv (r_rd (commit_run (t_mode T0) H S (t_rd T0) (t_staged T))) io
                   (fun c => rem c - count_occ comm_eq_dec applied c)) /\
  (forall rd io rem, binding_inv rd io rem -> (forall k c, rd k c = 0) ->
     forall c, (exists k, In k (keys c) /\ In k io) -> rem c = 0).
Proof.
  split; [exact rig_binding_inv |]. split.
  - intros ops T0 T H S io rem Hnp Hs0 Hp0 Hinv Hst.
    assert (Hbound : forall c, count_occ comm_eq_dec (t_paired T) c <= rem c).
    { apply (stage_all_paired_bound ops T0 T io rem Hnp Hinv); [| exact Hst].
      intros c. rewrite Hp0. simpl. lia. }
    split; [exact Hbound |].
    destruct (stage_all_paired ops T0 T Hst) as (new & Hnew & Hp).
    rewrite Hs0 in Hnew. simpl in Hnew. rewrite Hp0 in Hp. simpl in Hp.
    assert (Hwfs : Forall sg_wf (t_staged T)) by (apply (stage_all_sg_wf ops T0 T Hst); rewrite Hs0; constructor).
    destruct (commit_run_rd (t_staged T) (t_mode T0) H S (t_rd T0) Hwfs) as (n & Hn).
    exists (paired_of (t_mode T0) (firstn n (t_staged T))).
    split.
    + intros c. rewrite Hp, <- Hnew. apply count_firstn_le.
    + rewrite Hn. apply binding_inv_fold_unbind. exact Hinv.
  - intros rd io rem Hinv Hzero c (k & Hk & Hio).
    specialize (Hzero k c). rewrite Hinv in Hzero.
    destruct (in_dec iokey_eq_dec k io) as [_ | Cn]; [| contradiction].
    assert (Hm := mult_pos k c Hk).
    apply Nat.mul_eq_0 in Hzero as [E | E]; [exact E | lia].
Qed.

(* ── FR-1': canonical channel groups (H17) ─────────────────────────────── *)

(* FR-1' (round 2): chan_leb x y is the comparison hash(x) <= hash(y) of the
   stable_hash_provider::hash bytes of two channels. Any comparison function
   is allowed. The order lemmas state their premises (transitive,
   antisymmetric, total) explicitly. A group is canonical when it is
   non-decreasing, so a group can repeat a channel. FR-1 (round 1) required
   a strictly increasing group and rejected a repeated channel with
   DuplicateChannelInGroup. FR-1' removes that error. *)
Variable chan_leb : Chan -> Chan -> bool.

(* Non-decreasing by the hash bytes. *)
Fixpoint nondecreasing (g : list Chan) : bool :=
  match g with
  | [] => true
  | x :: t => match t with [] => true | y :: _ => chan_leb x y && nondecreasing t end
  end.

(* The FR-1' error: SpaceTxnError::NonCanonicalChannelOrder. It is the only
   group error, because FR-1' removes DuplicateChannelInGroup. *)
Inductive GroupErr := NonCanonicalChannelOrder.

Definition group_check (g : list Chan) : option GroupErr :=
  if nondecreasing g then None else Some NonCanonicalChannelOrder.

Definition canonical (g : list Chan) : bool :=
  match group_check g with None => true | Some _ => false end.

Fixpoint first_error (gs : list (list Chan)) : option GroupErr :=
  match gs with
  | [] => None
  | g :: rest => match group_check g with Some e => Some e | None => first_error rest end
  end.

(* The groups that staging checks (FR-1 and FR-1'): the continuation key of
   a produce Fire, and the channels of a Park or a consume Fire. A produce
   Store uses only its own channel, which is always canonical. *)
Definition op_groups (op : Op) : list (list Chan) :=
  match op with
  | OProduce _ _ _ PStore => []
  | OProduce _ _ _ (PFire chs _ _ _) => [chs]
  | OConsume chs _ _ _ _ _ => [chs]
  end.

(* ── Hot-store read cache (hot_store.rs:671-718, 865-877, 987-1015) ────── *)

(* The keys of the three hot-store maps. *)
Inductive CKey := CData (c : Chan) | CCont (g : list Chan) | CJoins (c : Chan).

Definition ckey_eq_dec : forall x y : CKey, {x = y} + {x <> y}.
Proof. decide equality; first [apply chan_eq_dec | apply lc_eq_dec]. Defined.

Definition key_canonical (k : CKey) : bool :=
  match k with CCont g => canonical g | _ => true end.

(* A first read of a key caches it: get_continuations, get_data and
   get_joins insert the history value into the map (hot_store.rs:671-718,
   865-877, 987-1015). The list stands for the cached keys in the iteration
   order of changes(). *)
Definition touch (k : CKey) (cache : list CKey) : list CKey :=
  if in_dec ckey_eq_dec k cache then cache else cache ++ [k].

Definition touch_all (ks : list CKey) (cache : list CKey) : list CKey :=
  fold_left (fun acc k => touch k acc) ks cache.

Definition chan_keys (c : Chan) : list CKey := [CData c; CJoins c].

(* The keys that staging loads into the view through TxnView::load_data,
   load_continuations and load_joins (stage_produce, stage_consume and
   require_loaded_join). *)
Definition stage_keys (op : Op) : list CKey :=
  match op with
  | OProduce ch _ _ PStore => [CJoins ch; CData ch]
  | OProduce ch _ _ (PFire chs _ _ _) => [CJoins ch; CData ch] ++ flat_map chan_keys chs ++ [CCont chs]
  | OConsume chs _ _ _ _ CPark => CCont chs :: map CJoins chs
  | OConsume chs _ _ _ _ (CFire _) => map CData chs
  end.

(* The public reads of SpaceTxn: data, continuations and joins. *)
Inductive Read := RData (c : Chan) | RConts (g : list Chan) | RJoins (c : Chan).

Inductive Access := ARead (r : Read) | AStage (op : Op).

(* An access error: an FR-1 rejection, or another staging error. *)
Inductive AccErr := EGroup (e : GroupErr) | EStage.

Inductive Outcome := Continue (T : Txn) | Fail (e : AccErr).

(* One access of a transaction. The FR-1 check of the input groups runs
   before any hot-store access. A joins read loads its own key, which is
   canonical, and then checks each returned group. *)
Definition access (cache : list CKey) (T : Txn) (a : Access) : list CKey * Outcome :=
  match a with
  | ARead (RData c) => (touch (CData c) cache, Continue T)
  | ARead (RConts g) =>
      match group_check g with
      | Some e => (cache, Fail (EGroup e))
      | None => (touch (CCont g) cache, Continue T)
      end
  | ARead (RJoins c) =>
      let cache' := touch (CJoins c) cache in
      match first_error (vjoins (t_view T) c) with
      | Some e => (cache', Fail (EGroup e))
      | None => (cache', Continue T)
      end
  | AStage op =>
      match first_error (op_groups op) with
      | Some e => (cache, Fail (EGroup e))
      | None =>
          let cache' := touch_all (stage_keys op) cache in
          match stage T op with
          | Some T' => (cache', Continue T')
          | None => (cache', Fail EStage)
          end
      end
  end.

(* The transaction after an access. A failed access is a validation error:
   the code returns it before any view mutation and keeps the transaction
   open. TxnCore::poison closes only a transaction whose view mutation
   failed after validation, and the view operations of the model are
   total. *)
Definition after_access (T : Txn) (o : Outcome) : Txn :=
  match o with Continue T' => T' | Fail _ => T end.

(* A session of accesses. The agent sees each error and continues. *)
Fixpoint session (cache : list CKey) (T : Txn) (accs : list Access) : list CKey * Txn :=
  match accs with
  | [] => (cache, T)
  | a :: rest =>
      let r := access cache T a in
      session (fst r) (after_access T (snd r)) rest
  end.

(* The operations that a session stages: those whose access succeeds. *)
Fixpoint session_ops (cache : list CKey) (T : Txn) (accs : list Access) : list Op :=
  match accs with
  | [] => []
  | a :: rest =>
      let r := access cache T a in
      let staged := match a, snd r with AStage op, Continue _ => [op] | _, _ => [] end in
      staged ++ session_ops (fst r) (after_access T (snd r)) rest
  end.

(* ── Checkpoint of the hot store ───────────────────────────────────────── *)

(* [hkey] is the prefixed Blake2b history key of a cached key
   (history_repository_impl.rs:306-348). hash_from_vec sorts the channel
   hashes, so a permuted group has the history key of its canonical group.
   The encoders stand for the serializers of the three leaf kinds. *)
Variable HKey : Type.
Variable hkey_eq_dec : forall x y : HKey, {x = y} + {x <> y}.
Variable hkey : CKey -> HKey.
Variable Leaf : Type.
Variable encode_data : list Datum -> Leaf.
Variable encode_conts : list WK -> Leaf.
Variable encode_joins : list (list Chan) -> Leaf.

(* changes() (hot_store.rs:1115-1170): an empty cached list becomes a
   Delete (None), a non-empty list an Insert of its leaf. *)
Definition leaf_at (S : Store) (k : CKey) : option Leaf :=
  match k with
  | CData c => match s_data S c with [] => None | l => Some (encode_data l) end
  | CCont g => match s_conts S g with [] => None | l => Some (encode_conts l) end
  | CJoins c => match s_joins S c with [] => None | l => Some (encode_joins l) end
  end.

(* The history at a root, as a map from history keys to leaves. *)
Definition History := HKey -> option Leaf.

Definition hkey_eqb (x y : HKey) : bool := if hkey_eq_dec x y then true else false.

Fixpoint dup_free (l : list HKey) : bool :=
  match l with
  | [] => true
  | x :: t => negb (existsb (hkey_eqb x) t) && dup_free t
  end.

Definition ck_actions (S : Store) (cache : list CKey) : list (HKey * option Leaf) :=
  map (fun k => (hkey k, leaf_at S k)) cache.

Definition apply_actions (h : History) (acts : list (HKey * option Leaf)) : History :=
  fun x => match find (fun a => hkey_eqb (fst a) x) acts with
           | Some a => snd a
           | None => h x
           end.

(* The checkpoint: changes(), then RadixHistory::process, which rejects two
   actions on one history key (radix_history.rs:100-105). *)
Definition checkpoint (h : History) (S : Store) (cache : list CKey) : option History :=
  let acts := ck_actions S cache in
  if dup_free (map fst acts) then Some (apply_actions h acts) else None.

(* An uncached key reads through to the history: its value encodes to the
   leaf at its history key (get_*_from_history_store). *)
Definition read_through (h : History) (S : Store) (cache : list CKey) : Prop :=
  forall k, ~ In k cache -> leaf_at S k = h (hkey k).

(* Collision freedom of the history keys on canonical keys (Blake2b). *)
Definition key_injective : Prop :=
  forall k1 k2, key_canonical k1 = true -> key_canonical k2 = true -> hkey k1 = hkey k2 -> k1 = k2.

(* A well-formed checkpoint state: a duplicate-free canonical cache over a
   read-through store. *)
Definition ck_wf (h : History) (S : Store) (cache : list CKey) : Prop :=
  NoDup cache /\ Forall (fun k => key_canonical k = true) cache /\ read_through h S cache.

(* ── Abort (H9) ────────────────────────────────────────────────────────── *)

Record World := mkWorld {
  wd_st : Store; wd_cache : list CKey; wd_rd : Bindings; wd_obs : list Obs; wd_ev : list Ev;
  wd_gen : nat }.

Definition txn_begin (W : World) (m : Mode) (inst : Chan -> bool) : Txn :=
  mkTxn m (view_init (wd_st W)) inst (wd_rd W) [] [].

Inductive Decision := DCommit | DDrop.

Definition with_cache (W : World) (cache : list CKey) : World :=
  mkWorld (wd_st W) cache (wd_rd W) (wd_obs W) (wd_ev W) (wd_gen W).

(* The transaction lifecycle: begin, a session of reads and stages, then
   commit or drop. An access error leaves the transaction open, and the
   agent decides: it can stage more, commit or drop. Commit fails closed
   with StoreSwapped when the store generation changed after begin. The
   keys that the session read stay in the hot-store cache in every case. *)
Definition txn_lifecycle (W : World) (m : Mode) (inst : Chan -> bool) (accs : list Access)
    (d : Decision) (gen_now : nat) : World * Status :=
  let r := session (wd_cache W) (txn_begin W m inst) accs in
  let W1 := with_cache W (fst r) in
  match d with
  | DDrop => (W1, StOk)
  | DCommit =>
      if Nat.eqb gen_now (wd_gen W)
      then let c := commit_run m (wd_obs W) (wd_st W) (wd_rd W) (t_staged (snd r)) in
           (mkWorld (r_st c) (fst r) (r_rd c) (wd_obs W ++ r_obs c) (wd_ev W ++ r_ev c) (wd_gen W),
            r_status c)
      else (W1, StRejected)
  end.

(* The abort paths: a drop (also after an access error), and a commit after
   a store swap. *)
Definition aborts (W : World) (d : Decision) (gen_now : nat) : Prop :=
  d = DDrop \/ gen_now <> wd_gen W.

(* The cache grows only by fresh canonical keys. *)
Definition grows (cache cache' : list CKey) : Prop :=
  exists added, cache' = cache ++ added /\ NoDup added /\
    Forall (fun k => key_canonical k = true /\ ~ In k cache) added.

Lemma grows_refl : forall cache, grows cache cache.
Proof. intros cache. exists []. rewrite app_nil_r. split; [reflexivity | split; constructor]. Qed.

Lemma grows_trans : forall c1 c2 c3, grows c1 c2 -> grows c2 c3 -> grows c1 c3.
Proof.
  intros c1 c2 c3 (a1 & E1 & N1 & F1) (a2 & E2 & N2 & F2). exists (a1 ++ a2).
  split; [rewrite E2, E1, app_assoc; reflexivity |]. split.
  - apply NoDup_app; [exact N1 | exact N2 |].
    intros x Hx1 Hx2. rewrite Forall_forall in F2. destruct (F2 x Hx2) as [_ Hn].
    apply Hn. rewrite E1. apply in_or_app. right. exact Hx1.
  - apply Forall_app. split; [exact F1 |].
    rewrite Forall_forall in F2 |- *. intros x Hx. destruct (F2 x Hx) as [Hc Hn].
    split; [exact Hc |]. intros Hin. apply Hn. rewrite E1. apply in_or_app. left. exact Hin.
Qed.

Lemma touch_grows : forall k cache, key_canonical k = true -> grows cache (touch k cache).
Proof.
  intros k cache Hk. unfold touch. destruct (in_dec ckey_eq_dec k cache) as [_ | Hn].
  - apply grows_refl.
  - exists [k]. split; [reflexivity |]. split; [repeat constructor; intros [] |].
    constructor; [split; assumption | constructor].
Qed.

Lemma touch_all_grows :
  forall ks cache, Forall (fun k => key_canonical k = true) ks -> grows cache (touch_all ks cache).
Proof.
  induction ks as [| k ks IH]; intros cache Hks; [apply grows_refl |].
  inversion Hks as [| k' ks' Hk Hks']; subst. unfold touch_all. simpl.
  apply grows_trans with (c2 := touch k cache); [apply touch_grows, Hk |].
  apply IH, Hks'.
Qed.

Lemma group_check_canonical : forall g, group_check g = None -> canonical g = true.
Proof. intros g H. unfold canonical. rewrite H. reflexivity. Qed.

Lemma stage_keys_canonical :
  forall op, first_error (op_groups op) = None -> Forall (fun k => key_canonical k = true) (stage_keys op).
Proof.
  intros op Hf.
  assert (Hck : forall chs, Forall (fun k => key_canonical k = true) (flat_map chan_keys chs)).
  { intros chs. rewrite Forall_forall. intros k Hk. apply in_flat_map in Hk as (c & _ & Hk).
    destruct Hk as [E | [E | []]]; subst k; reflexivity. }
  destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]];
    simpl in Hf; simpl.
  - repeat constructor.
  - destruct (group_check chs) eqn:Hg; [discriminate |].
    constructor; [reflexivity |]. constructor; [reflexivity |].
    apply Forall_app. split; [apply Hck |]. constructor; [| constructor].
    simpl. apply group_check_canonical, Hg.
  - destruct (group_check chs) eqn:Hg; [discriminate |].
    constructor; [apply group_check_canonical, Hg |].
    rewrite Forall_forall. intros x Hx. apply in_map_iff in Hx as (c & <- & _). reflexivity.
  - rewrite Forall_forall. intros x Hx. apply in_map_iff in Hx as (c & <- & _). reflexivity.
Qed.

Lemma access_grows : forall cache T a, grows cache (fst (access cache T a)).
Proof.
  intros cache T [[c | g | c] | op]; simpl.
  - apply touch_grows. reflexivity.
  - destruct (group_check g) eqn:Hg; simpl; [apply grows_refl |].
    apply touch_grows. simpl. apply group_check_canonical, Hg.
  - destruct (first_error (vjoins (t_view T) c)); simpl; apply touch_grows; reflexivity.
  - destruct (first_error (op_groups op)) eqn:Hf; simpl; [apply grows_refl |].
    destruct (stage T op); simpl; apply touch_all_grows, stage_keys_canonical, Hf.
Qed.

Lemma session_grows : forall accs cache T, grows cache (fst (session cache T accs)).
Proof.
  induction accs as [| a rest IH]; intros cache T; simpl; [apply grows_refl |].
  apply grows_trans with (c2 := fst (access cache T a)); [apply access_grows | apply IH].
Qed.

Lemma dup_free_nodup : forall l, dup_free l = true -> NoDup l.
Proof.
  induction l as [| x t IH]; simpl; intros H; [constructor |].
  apply andb_true_iff in H as [Hn Ht]. constructor; [| apply IH, Ht].
  intros Hin. apply negb_true_iff in Hn. rewrite <- Bool.not_true_iff_false in Hn. apply Hn.
  apply existsb_exists. exists x. split; [exact Hin |]. unfold hkey_eqb.
  destruct (hkey_eq_dec x x); [reflexivity | contradiction].
Qed.

Lemma nodup_dup_free : forall l, NoDup l -> dup_free l = true.
Proof.
  induction l as [| x t IH]; simpl; intros H; [reflexivity |].
  inversion H as [| x' t' Hn Ht]; subst. apply andb_true_iff. split; [| apply IH, Ht].
  apply negb_true_iff, Bool.not_true_iff_false. intros Hex.
  apply existsb_exists in Hex as (y & Hy & Ey). unfold hkey_eqb in Ey.
  destruct (hkey_eq_dec x y) as [E | _]; [subst y; contradiction | discriminate].
Qed.

Lemma nodup_map_hkey :
  forall cache, key_injective -> NoDup cache -> Forall (fun k => key_canonical k = true) cache ->
    NoDup (map hkey cache).
Proof.
  intros cache Hinj. induction cache as [| k t IH]; simpl; intros Hnd Hc; [constructor |].
  inversion Hnd as [| k' t' Hn Ht]; subst. inversion Hc as [| k' t' Hk Hct]; subst.
  constructor; [| apply IH; assumption].
  intros Hin. apply in_map_iff in Hin as (k2 & E & Hk2). apply Hn.
  rewrite Forall_forall in Hct.
  replace k with k2; [exact Hk2 |]. apply Hinj; [apply Hct, Hk2 | exact Hk | exact E].
Qed.

Lemma checkpoint_some :
  forall h S cache, key_injective -> NoDup cache -> Forall (fun k => key_canonical k = true) cache ->
    checkpoint h S cache = Some (apply_actions h (ck_actions S cache)).
Proof.
  intros h S cache Hinj Hnd Hc. unfold checkpoint.
  replace (map fst (ck_actions S cache)) with (map hkey cache).
  - rewrite (nodup_dup_free _ (nodup_map_hkey cache Hinj Hnd Hc)). reflexivity.
  - unfold ck_actions. rewrite map_map. reflexivity.
Qed.

Lemma find_app_none :
  forall (A : Type) (f : A -> bool) l1 l2, find f l1 = None -> find f (l1 ++ l2) = find f l2.
Proof. intros A f l1 l2. induction l1 as [| x t IH]; simpl; [auto |]. destruct (f x); [discriminate | exact IH]. Qed.

Lemma find_app_some :
  forall (A : Type) (f : A -> bool) l1 l2 a, find f l1 = Some a -> find f (l1 ++ l2) = Some a.
Proof. intros A f l1 l2 a. induction l1 as [| x t IH]; simpl; [discriminate |]. destruct (f x); [auto | exact IH]. Qed.

(* Fresh canonical keys in the cache do not change the checkpoint. *)
Lemma cache_extension_neutral :
  forall h S cache added,
    key_injective -> ck_wf h S cache ->
    NoDup added -> Forall (fun k => key_canonical k = true /\ ~ In k cache) added ->
    ck_wf h S (cache ++ added) /\
    exists h0 h1, checkpoint h S cache = Some h0 /\ checkpoint h S (cache ++ added) = Some h1 /\
      forall x, h1 x = h0 x.
Proof.
  intros h S cache added Hinj (Hnd & Hc & Hrt) Hnda Ha.
  rewrite Forall_forall in Ha.
  assert (Hwf : ck_wf h S (cache ++ added)).
  { split; [| split].
    - apply NoDup_app; [exact Hnd | exact Hnda |]. intros x Hx Hx'. exact (proj2 (Ha x Hx') Hx).
    - apply Forall_app. split; [exact Hc |]. rewrite Forall_forall. intros x Hx. exact (proj1 (Ha x Hx)).
    - intros k Hk. apply Hrt. intros Hin. apply Hk. apply in_or_app. left. exact Hin. }
  split; [exact Hwf |].
  destruct Hwf as (Hnd' & Hc' & _).
  exists (apply_actions h (ck_actions S cache)), (apply_actions h (ck_actions S (cache ++ added))).
  split; [apply checkpoint_some; assumption |]. split; [apply checkpoint_some; assumption |].
  intros x. unfold apply_actions, ck_actions. rewrite map_app.
  destruct (find (fun a => hkey_eqb (fst a) x) (map (fun k => (hkey k, leaf_at S k)) cache))
    as [a |] eqn:Hf.
  - rewrite (find_app_some _ _ _ _ a Hf). reflexivity.
  - rewrite (find_app_none _ _ _ _ Hf).
    destruct (find (fun a => hkey_eqb (fst a) x) (map (fun k => (hkey k, leaf_at S k)) added))
      as [a |] eqn:Hf2; [| reflexivity].
    apply find_some in Hf2 as (Hin & Ex). apply in_map_iff in Hin as (k & <- & Hk).
    simpl in Ex |- *. unfold hkey_eqb in Ex. destruct (hkey_eq_dec (hkey k) x) as [<- | _]; [| discriminate].
    apply Hrt. exact (proj2 (Ha k Hk)).
Qed.

Lemma vloops_base :
  (forall cs V chs, v_base (vpf_loop V chs cs) = v_base V) /\
  (forall cs V, v_base (vcf_loop V cs) = v_base V) /\
  (forall cs V g, v_base (vput_joins V cs g) = v_base V).
Proof.
  split; [| split].
  - induction cs as [| cd rest IH]; intros V chs; simpl; [reflexivity |].
    rewrite IH. unfold vremove_join. destruct (vconts _ chs); [| ];
      destruct (removable cd); reflexivity.
  - induction cs as [| cd rest IH]; intros V; simpl; [reflexivity |].
    rewrite IH. destruct (negb (d_persist (cd_rec cd))); reflexivity.
  - induction cs as [| c rest IH]; intros V g; simpl; [reflexivity |].
    rewrite IH. unfold vput_join. destruct (in_dec lc_eq_dec g (vjoins V c)); reflexivity.
Qed.

Lemma stage_base :
  forall T op T', stage T op = Some T' -> v_base (t_view T') = v_base (t_view T).
Proof.
  intros T op T' Hst. destruct vloops_base as (Bpf & Bcf & Bpj).
  destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]];
    unfold stage in Hst.
  - destruct (t_installed T ch); [discriminate |]. injection Hst as <-. simpl.
    unfold vput_datum, vbump. destruct persist; reflexivity.
  - destruct (t_installed T ch); [discriminate |].
    destruct (pfire_ok T ch v persist chs wk cands); cbn [negb] in Hst; [| discriminate].
    apply stage_fire_some in Hst. subst T'. simpl.
    unfold vpf_effects. rewrite Bpf.
    destruct (w_persist wk); unfold vbump, vremove_continuation; destruct persist; reflexivity.
  - destruct (consume_args_ok T chs pats peeks); simpl in Hst; [| discriminate].
    injection Hst as <-. simpl. unfold vstore_wk. rewrite Bpj.
    unfold vput_continuation. destruct (existsb _ _); reflexivity.
  - destruct (consume_args_ok T chs pats peeks); cbn [negb] in Hst; [| discriminate].
    destruct (cfire_ok T chs cands); cbn [negb] in Hst; [| discriminate].
    apply stage_fire_some in Hst. subst T'. simpl.
    unfold vcf_effects. rewrite Bcf. reflexivity.
Qed.

Lemma session_stage_all :
  forall accs cache T, stage_all T (session_ops cache T accs) = Some (snd (session cache T accs)).
Proof.
  induction accs as [| a rest IH]; intros cache T; [reflexivity |].
  destruct a as [[c | g | c] | op]; cbn [session session_ops access snd fst after_access app].
  - apply IH.
  - destruct (group_check g); cbn [snd fst after_access app]; apply IH.
  - destruct (first_error (vjoins (t_view T) c)); cbn [snd fst after_access app]; apply IH.
  - destruct (first_error (op_groups op)); cbn [snd fst after_access app]; [apply IH |].
    destruct (stage T op) as [T1 |] eqn:H1; cbn [snd fst after_access app]; [| apply IH].
    cbn [stage_all]. rewrite H1. apply IH.
Qed.

Lemma session_ops_canonical :
  forall accs cache T, Forall (fun op => first_error (op_groups op) = None) (session_ops cache T accs).
Proof.
  induction accs as [| a rest IH]; intros cache T; [constructor |].
  destruct a as [[c | g | c] | op]; cbn [session_ops access snd fst after_access app].
  - apply IH.
  - destruct (group_check g); cbn [snd fst after_access app]; apply IH.
  - destruct (first_error (vjoins (t_view T) c)); cbn [snd fst after_access app]; apply IH.
  - destruct (first_error (op_groups op)) eqn:Hf; cbn [snd fst after_access app]; [apply IH |].
    destruct (stage T op) as [T1 |]; cbn [snd fst after_access app]; [| apply IH].
    constructor; [exact Hf | apply IH].
Qed.

Lemma session_staged_canonical :
  forall accs cache T,
    exists new, t_staged (snd (session cache T accs)) = t_staged T ++ new /\
      Forall (fun sg => first_error (op_groups (sg_op sg)) = None) new.
Proof.
  induction accs as [| a rest IH]; intros cache T.
  - exists []. rewrite app_nil_r. split; [reflexivity | constructor].
  - destruct a as [[c | g | c] | op]; cbn [session access snd fst after_access].
    + apply IH.
    + destruct (group_check g); cbn [snd fst after_access]; apply IH.
    + destruct (first_error (vjoins (t_view T) c)); cbn [snd fst after_access]; apply IH.
    + destruct (first_error (op_groups op)) eqn:Hf; cbn [snd fst after_access]; [apply IH |].
      destruct (stage T op) as [T1 |] eqn:H1; cbn [snd fst after_access]; [| apply IH].
      destruct (stage_props T op T1 H1) as (sg & Hsg & Hop & _).
      destruct (IH (touch_all (stage_keys op) cache) T1) as (new & Hnew & Hf').
      exists (sg :: new). split; [rewrite Hnew, Hsg, <- app_assoc; reflexivity |].
      constructor; [rewrite Hop; exact Hf | exact Hf'].
Qed.

(* ── FR-1': the canonical form of a group ──────────────────────────────── *)

(* The order premises. chan_leb compares the hash bytes of two store
   identities, and the byte order of Blake2b256Hash is a total order. So
   chan_leb is transitive, antisymmetric and total. The lemma
   order_premises_satisfiable shows the three premises for one order. *)
Definition leb_transitive : Prop :=
  forall x y z, chan_leb x y = true -> chan_leb y z = true -> chan_leb x z = true.

Definition leb_antisymmetric : Prop :=
  forall x y, chan_leb x y = true -> chan_leb y x = true -> x = y.

Definition leb_total : Prop :=
  forall x y, chan_leb x y = true \/ chan_leb y x = true.

Lemma canonical_nondecreasing : forall g, canonical g = nondecreasing g.
Proof. intros g. unfold canonical, group_check. destruct (nondecreasing g); reflexivity. Qed.

Lemma nondecreasing_sorted :
  forall g, nondecreasing g = true <-> Sorted (fun x y => chan_leb x y = true) g.
Proof.
  induction g as [| x t IH]; [split; [intros _; constructor | reflexivity] |].
  destruct t as [| y t'].
  - split; [intros _; repeat constructor | reflexivity].
  - change (nondecreasing (x :: y :: t')) with (chan_leb x y && nondecreasing (y :: t')).
    rewrite andb_true_iff, IH. split.
    + intros [Hxy Hs]. constructor; [exact Hs | constructor; exact Hxy].
    + intros Hs. apply Sorted_inv in Hs as [Hs Hhd]. apply HdRel_inv in Hhd. split; assumption.
Qed.

Lemma canonical_strongly_sorted :
  leb_transitive -> forall g, canonical g = true -> StronglySorted (fun x y => chan_leb x y = true) g.
Proof.
  intros Htr g Hg. apply Sorted_StronglySorted; [intros x y z; apply Htr |].
  apply nondecreasing_sorted. rewrite <- canonical_nondecreasing. exact Hg.
Qed.

(* Two strongly sorted permutations of one list are equal when the relation
   is antisymmetric. *)
Lemma sorted_perm_unique :
  forall (R : Chan -> Chan -> Prop), (forall x y, R x y -> R y x -> x = y) ->
    forall l1 l2, StronglySorted R l1 -> StronglySorted R l2 -> Permutation l1 l2 -> l1 = l2.
Proof.
  intros R Hanti l1. induction l1 as [| x t1 IH]; intros l2 S1 S2 P.
  - apply Permutation_nil in P. subst. reflexivity.
  - destruct l2 as [| y t2]; [apply Permutation_sym, Permutation_nil in P; discriminate |].
    apply StronglySorted_inv in S1 as [S1 F1]. apply StronglySorted_inv in S2 as [S2 F2].
    rewrite Forall_forall in F1, F2.
    assert (Exy : x = y).
    { assert (Hy : In y (x :: t1)) by (apply (Permutation_in _ (Permutation_sym P)); left; reflexivity).
      assert (Hx : In x (y :: t2)) by (apply (Permutation_in _ P); left; reflexivity).
      destruct Hy as [Ey | Hy]; [exact Ey |].
      destruct Hx as [Ex | Hx]; [symmetry; exact Ex |].
      exact (Hanti x y (F1 y Hy) (F2 x Hx)). }
    subst y. f_equal. apply IH; [exact S1 | exact S2 |]. apply Permutation_cons_inv in P. exact P.
Qed.

(* FR-1': a sorted list is the unique canonical member of its permutation
   class. *)
Lemma sorted_unique_canonical :
  leb_transitive -> leb_antisymmetric ->
  forall g g', canonical g = true -> Permutation g g' -> canonical g' = true -> g' = g.
Proof.
  intros Htr Hanti g g' Hg P Hg'. symmetry.
  apply (sorted_perm_unique (fun x y => chan_leb x y = true)); [exact Hanti | | | exact P];
    apply canonical_strongly_sorted; assumption.
Qed.

(* Every permutation class has a canonical member: the insertion sort of any
   member. *)
Lemma isort_canonical :
  leb_total -> forall g, canonical (isort chan_leb g) = true /\ Permutation (isort chan_leb g) g.
Proof.
  intros Htot g. split; [| apply isort_perm].
  rewrite canonical_nondecreasing. apply nondecreasing_sorted. apply isort_sorted. exact Htot.
Qed.

(* H17 (FR-1'): a group that is not non-decreasing fails with
   NonCanonicalChannelOrder, and a non-decreasing group passes, also when it
   repeats a channel. Single-channel groups pass. Under a total order, each
   permutation class has exactly one member that passes: its sorted list.
   Every other order of a canonical group fails. A read of a non-canonical
   continuation key and a stage with a non-canonical group fail before any
   hot-store access, so the cache does not change. Every access adds only
   fresh canonical keys to the cache. A successful session stages exactly
   the operations that stage_all stages (H1, H2), and each of them has
   canonical groups, so the commit-time lookups are canonical. *)
Theorem non_canonical_fails_closed :
  (forall g, nondecreasing g = false -> group_check g = Some NonCanonicalChannelOrder) /\
  (forall g, nondecreasing g = true -> group_check g = None) /\
  (forall c, group_check [c] = None) /\
  (forall c, chan_leb c c = true -> group_check [c; c] = None) /\
  (forall g g', leb_transitive -> leb_antisymmetric ->
     canonical g = true -> Permutation g g' -> g' <> g ->
     group_check g' = Some NonCanonicalChannelOrder) /\
  (leb_total -> forall g, group_check (isort chan_leb g) = None /\ Permutation (isort chan_leb g) g) /\
  (forall cache T g e, group_check g = Some e ->
     access cache T (ARead (RConts g)) = (cache, Fail (EGroup e))) /\
  (forall cache T op e, first_error (op_groups op) = Some e ->
     access cache T (AStage op) = (cache, Fail (EGroup e))) /\
  (forall cache T a, grows cache (fst (access cache T a))) /\
  (forall accs cache T,
     stage_all T (session_ops cache T accs) = Some (snd (session cache T accs)) /\
     Forall (fun op => first_error (op_groups op) = None) (session_ops cache T accs) /\
     exists new, t_staged (snd (session cache T accs)) = t_staged T ++ new /\
       Forall (fun sg => first_error (op_groups (sg_op sg)) = None) new).
Proof.
  split; [intros g H; unfold group_check; rewrite H; reflexivity |].
  split; [intros g H; unfold group_check; rewrite H; reflexivity |].
  split; [intros c; reflexivity |].
  split; [intros c H; unfold group_check; cbn [nondecreasing]; rewrite H; reflexivity |].
  split.
  { intros g g' Htr Hanti Hg P N. unfold group_check.
    destruct (nondecreasing g') eqn:E; [| reflexivity].
    exfalso. apply N. apply (sorted_unique_canonical Htr Hanti g g' Hg P).
    rewrite canonical_nondecreasing. exact E. }
  split.
  { intros Htot g. destruct (isort_canonical Htot g) as [C P]. split; [| exact P].
    rewrite canonical_nondecreasing in C. unfold group_check. rewrite C. reflexivity. }
  split; [intros cache T g e H; simpl; rewrite H; reflexivity |].
  split; [intros cache T op e H; simpl; rewrite H; reflexivity |].
  split; [exact access_grows |].
  intros accs cache T. split; [apply session_stage_all |].
  split; [apply session_ops_canonical | apply session_staged_canonical].
Qed.

Lemma lifecycle_abort_world :
  forall W m inst accs d g, aborts W d g ->
    fst (txn_lifecycle W m inst accs d g) =
    with_cache W (fst (session (wd_cache W) (txn_begin W m inst) accs)).
Proof.
  intros W m inst accs d g Hab. unfold txn_lifecycle.
  destruct d; [| reflexivity].
  destruct Hab as [C | Hg]; [discriminate |].
  destruct (Nat.eqb g (wd_gen W)) eqn:E; [apply Nat.eqb_eq in E; contradiction | reflexivity].
Qed.

(* H9: a dropped transaction (also after access errors) and a commit after a
   store swap leave the store, the bindings, the observer history, the
   events and the generation unchanged. The reads and stages of the session
   stay in the hot-store cache, but FR-1 admits only fresh canonical keys
   there. Such keys do not change the checkpoint: it succeeds before and
   after, with the same history map (and so the same root). Staging never
   writes to the captured store, the bindings, the installed set or the
   mode. *)
Theorem abort_inert :
  (forall W m inst accs d g, aborts W d g ->
     let W' := fst (txn_lifecycle W m inst accs d g) in
     wd_st W' = wd_st W /\ wd_rd W' = wd_rd W /\ wd_obs W' = wd_obs W /\
     wd_ev W' = wd_ev W /\ wd_gen W' = wd_gen W /\ grows (wd_cache W) (wd_cache W')) /\
  (forall h S cache added,
     key_injective -> ck_wf h S cache ->
     NoDup added -> Forall (fun k => key_canonical k = true /\ ~ In k cache) added ->
     ck_wf h S (cache ++ added) /\
     exists h0 h1, checkpoint h S cache = Some h0 /\ checkpoint h S (cache ++ added) = Some h1 /\
       forall x, h1 x = h0 x) /\
  (forall W m inst accs d g h,
     key_injective -> ck_wf h (wd_st W) (wd_cache W) -> aborts W d g ->
     let W' := fst (txn_lifecycle W m inst accs d g) in
     ck_wf h (wd_st W') (wd_cache W') /\
     exists h0 h1, checkpoint h (wd_st W) (wd_cache W) = Some h0 /\
       checkpoint h (wd_st W') (wd_cache W') = Some h1 /\ forall x, h1 x = h0 x) /\
  (forall ops T T', stage_all T ops = Some T' ->
     v_base (t_view T') = v_base (t_view T) /\ t_rd T' = t_rd T /\
     t_installed T' = t_installed T /\ t_mode T' = t_mode T).
Proof.
  split; [| split; [| split]].
  - intros W m inst accs d g Hab W'. subst W'. rewrite (lifecycle_abort_world W m inst accs d g Hab).
    unfold with_cache. simpl. repeat (split; [reflexivity |]). apply session_grows.
  - exact cache_extension_neutral.
  - intros W m inst accs d g h Hinj Hwf Hab W'. subst W'. rewrite (lifecycle_abort_world W m inst accs d g Hab).
    destruct (session_grows accs (wd_cache W) (txn_begin W m inst)) as (added & Ec & Hnd & Ha).
    unfold with_cache. simpl. rewrite Ec.
    exact (cache_extension_neutral h (wd_st W) (wd_cache W) added Hinj Hwf Hnd Ha).
  - induction ops as [| op rest IH]; intros T T' Hst; simpl in Hst.
    + injection Hst as <-. auto.
    + destruct (stage T op) as [T1 |] eqn:H1; [| discriminate].
      destruct (IH T1 T' Hst) as (B & R & I & M).
      destruct (stage_props T op T1 H1) as (_ & _ & _ & M1 & I1 & R1).
      rewrite B, R, I, M, (stage_base T op T1 H1), R1, I1, M1. auto.
Qed.

(* ── Reporting (H12) ───────────────────────────────────────────────────── *)

Definition with_mode (T : Txn) (m : Mode) : Txn :=
  mkTxn m (t_view T) (t_installed T) (t_rd T) (t_paired T) (t_staged T).

(* A staged fire depends on the mode only through its backend. *)
Lemma stage_fire_mode_indep :
  forall T m1 m2 V k cm mk, backend m1 = backend m2 ->
    option_map (fun T' => (t_view T', t_paired T', t_staged T')) (stage_fire (with_mode T m1) V k cm mk) =
    option_map (fun T' => (t_view T', t_paired T', t_staged T')) (stage_fire (with_mode T m2) V k cm mk).
Proof.
  intros T m1 m2 V k cm mk Hb.
  assert (Hps : pair_step (with_mode T m1) k cm = pair_step (with_mode T m2) k cm).
  { unfold pair_step. simpl. rewrite Hb. reflexivity. }
  unfold stage_fire. cbv zeta. rewrite Hps. cbn [t_mode with_mode]. rewrite ?Hb.
  destruct (backend m2); destruct (snd (pair_step (with_mode T m2) k cm)); reflexivity.
Qed.

Lemma stage_mode_indep :
  forall T op m1 m2, backend m1 = backend m2 ->
    option_map (fun T' => (t_view T', t_paired T', t_staged T')) (stage (with_mode T m1) op) =
    option_map (fun T' => (t_view T', t_paired T', t_staged T')) (stage (with_mode T m2) op).
Proof.
  intros T op m1 m2 Hb.
  destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]];
    unfold stage.
  - simpl. destruct (t_installed T ch); reflexivity.
  - change (t_installed (with_mode T m1)) with (t_installed T).
    change (t_installed (with_mode T m2)) with (t_installed T).
    destruct (t_installed T ch); [reflexivity |].
    change (pfire_ok (with_mode T m1) ch v persist chs wk cands) with (pfire_ok T ch v persist chs wk cands).
    change (pfire_ok (with_mode T m2) ch v persist chs wk cands) with (pfire_ok T ch v persist chs wk cands).
    destruct (pfire_ok T ch v persist chs wk cands); [| reflexivity]. cbn [negb].
    apply stage_fire_mode_indep, Hb.
  - change (consume_args_ok (with_mode T m1) chs pats peeks) with (consume_args_ok T chs pats peeks).
    change (consume_args_ok (with_mode T m2) chs pats peeks) with (consume_args_ok T chs pats peeks).
    destruct (consume_args_ok T chs pats peeks); reflexivity.
  - change (consume_args_ok (with_mode T m1) chs pats peeks) with (consume_args_ok T chs pats peeks).
    change (consume_args_ok (with_mode T m2) chs pats peeks) with (consume_args_ok T chs pats peeks).
    destruct (consume_args_ok T chs pats peeks); [| reflexivity]. simpl.
    change (cfire_ok (with_mode T m1) chs cands) with (cfire_ok T chs cands).
    change (cfire_ok (with_mode T m2) chs cands) with (cfire_ok T chs cands).
    destruct (cfire_ok T chs cands); [| reflexivity]. cbn [negb].
    apply stage_fire_mode_indep, Hb.
Qed.

(* H12: the reporting space delegates to its inner replay space, so its
   commits, its host operations and its staging are those of replay. *)
Theorem reporting_trace_is_replay_trace :
  (forall H S rd sgs, commit_run Reporting H S rd sgs = commit_run Replay H S rd sgs) /\
  (forall H S rd ops, host_run Reporting H S rd ops = host_run Replay H S rd ops) /\
  (forall T op,
     option_map (fun T' => (t_view T', t_paired T', t_staged T')) (stage (with_mode T Reporting) op) =
     option_map (fun T' => (t_view T', t_paired T', t_staged T')) (stage (with_mode T Replay) op)).
Proof.
  split; [| split].
  - intros H S rd sgs. revert H S rd. induction sgs as [| sg rest IH]; intros H S rd; [reflexivity |].
    simpl. assert (E : commit_op Reporting H S rd sg = commit_op Replay H S rd sg).
    { unfold commit_op. destruct (sg_op sg); reflexivity. }
    rewrite E. destruct (r_status (commit_op Replay H S rd sg)); try reflexivity. rewrite IH. reflexivity.
  - intros H S rd ops. revert H S rd. induction ops as [| op rest IH]; intros H S rd; [reflexivity |].
    simpl. assert (E : host_op Reporting H S rd op = host_op Replay H S rd op).
    { unfold host_op. destruct op; reflexivity. }
    rewrite E. destruct (r_status (host_op Replay H S rd op)); try reflexivity. rewrite IH. reflexivity.
  - intros T op. apply stage_mode_indep. reflexivity.
Qed.

(* ── Replay directive (H19, decision 19 D4) ──────────────────────────── *)

(* The source and the observe_operation_start call of a staged operation. *)
Definition sg_src (sg : Staged) : Src :=
  match sg_op sg with
  | OProduce ch v persist _ => SrcP (produce_id ch v persist)
  | OConsume chs pats k persist _ _ => SrcC (consume_id chs pats k persist)
  end.

Definition sg_start (sg : Staged) : Obs :=
  match sg_op sg with
  | OProduce ch v persist _ => ObStart (SrcP (produce_id ch v persist)) [ch] (sg_joins0 sg)
  | OConsume chs pats k persist _ _ => ObStart (SrcC (consume_id chs pats k persist)) chs []
  end.

(* H19: in replay and reporting, a replay directive (native or failed) at
   the directive position rejects the operation: reject_directive returns
   NativeReplayDirectiveUnsupported, observe_operation_finish reports
   Rejected, and the store, the bindings, the events and the effects do not
   change. The commit stops there (decision D6). For a native directive the
   host would hand the operation to the native session instead. *)
Theorem directive_fails_closed :
  forall m H S rd sg rest,
    backend m = Replay ->
    ob_accept H (ObBefore (sg_src sg)) = true ->
    ob_accept (H ++ [ObBefore (sg_src sg)]) (sg_start sg) = true ->
    ob_directive (H ++ [ObBefore (sg_src sg); sg_start sg]) (sg_src sg) <> DirNone ->
    let fin := [ObBefore (sg_src sg); sg_start sg; ObDirective (sg_src sg);
                ObFinish (sg_src sg) Rejected] in
    commit_op m H S rd sg = mkRes S rd fin [] [] StRejected /\
    commit_run m H S rd (sg :: rest) = mkRes S rd fin [] [] StRejected /\
    (ob_directive (H ++ [ObBefore (sg_src sg); sg_start sg]) (sg_src sg) = DirSome ->
       forall observe, host_prelude Replay H (sg_src sg) (sg_start sg) observe =
         PreStop [ObBefore (sg_src sg); sg_start sg; ObDirective (sg_src sg)] StNative).
Proof.
  intros m H S rd sg rest Hb Ha1 Ha2 Hd fin.
  assert (Hpre : forall observe,
            space_prelude Replay H (sg_src sg) (sg_start sg) observe = PreStop fin StRejected).
  { intros observe. unfold space_prelude. rewrite Ha1, Ha2. simpl.
    destruct (ob_directive (H ++ [ObBefore (sg_src sg); sg_start sg]) (sg_src sg));
      first [contradiction | reflexivity]. }
  assert (Hop : commit_op m H S rd sg = mkRes S rd fin [] [] StRejected).
  { destruct sg as [op j0 scomm sp].
    destruct op as [ch v persist out | chs pats k persist peeks out];
      unfold commit_op; cbn [sg_op sg_joins0 sg_comm]; rewrite Hb.
    - unfold commit_replay_produce. cbv zeta.
      assert (E := Hpre (ObProduce (produce_id ch v persist) ch v persist)).
      unfold sg_src, sg_start in E. simpl in E. rewrite E. reflexivity.
    - unfold commit_replay_consume. cbv zeta.
      assert (E := Hpre (ObConsume (consume_id chs pats k persist) chs pats k persist peeks)).
      unfold sg_src, sg_start in E. simpl in E. rewrite E. reflexivity. }
  split; [exact Hop |]. split.
  - simpl. rewrite Hop. reflexivity.
  - intros Hs observe. unfold host_prelude. rewrite Ha1, Ha2, Hs. reflexivity.
Qed.

(* ═══════════ Decision 36: replay reproduces the run of play (H23) ═══════════ *)

(* The directive query (replay_operation_directive) is a replay-only call:
   play does not make it. erase_dir removes the directive queries from a
   list of observer calls. *)
Definition is_directive (o : Obs) : bool :=
  match o with ObDirective _ => true | _ => false end.

Definition erase_dir (h : list Obs) : list Obs := filter (fun o => negb (is_directive o)) h.

(* The premises of H23. The observer decisions do not depend on the
   directive queries in the history: the query is stateless (design §2.2,
   decision 36). The observer gives no replay directive, as the production
   RhoCommObserver gives none for a space operation (H19 covers a
   directive). *)
Definition ob_ignores_directive : Prop := forall h o, ob_accept h o = ob_accept (erase_dir h) o.

Definition dir_always_none : Prop := forall h s, ob_directive h s = DirNone.

(* sg_strip forgets the pairing flag of a staged operation. *)
Definition sg_strip (sg : Staged) : Op * list (list Chan) * option Comm :=
  (sg_op sg, sg_joins0 sg, sg_comm sg).

Lemma erase_dir_app : forall a b, erase_dir (a ++ b) = erase_dir a ++ erase_dir b.
Proof. intros a b. unfold erase_dir. apply filter_app. Qed.

Lemma erase_dir_idem : forall l, erase_dir (erase_dir l) = erase_dir l.
Proof.
  intros l. unfold erase_dir. induction l as [| o t IH]; [reflexivity |].
  cbn [filter]. destruct (negb (is_directive o)) eqn:E; [| exact IH].
  cbn [filter]. rewrite E, IH. reflexivity.
Qed.

Lemma erase_dir_keep : forall o l, is_directive o = false -> erase_dir (o :: l) = o :: erase_dir l.
Proof. intros o l H. unfold erase_dir. cbn [filter]. rewrite H. reflexivity. Qed.

Lemma erase_dir_drop : forall s l, erase_dir (ObDirective s :: l) = erase_dir l.
Proof. reflexivity. Qed.

(* erase_simpl computes erase_dir on a list of known calls. *)
Ltac erase_simpl :=
  repeat first
    [ rewrite erase_dir_app
    | rewrite erase_dir_drop
    | rewrite erase_dir_keep by (first [reflexivity | assumption]) ].

Lemma ob_accept_erase :
  ob_ignores_directive ->
  forall h1 h2 o, erase_dir h1 = erase_dir h2 -> ob_accept h1 o = ob_accept h2 o.
Proof. intros Hig h1 h2 o E. rewrite (Hig h1), (Hig h2), E. reflexivity. Qed.

(* The call prefix of replay is the prefix of play with the directive query,
   and it makes the same decisions. *)
Lemma prelude_play_replay :
  ob_ignores_directive -> dir_always_none ->
  forall Hp Hr s a b, erase_dir Hr = erase_dir Hp ->
    is_directive a = false -> is_directive b = false ->
    match space_prelude Replay Hr s a b, space_prelude Play Hp s a b with
    | PreStop o1 st1, PreStop o2 st2 => erase_dir o1 = o2 /\ erase_dir o2 = o2 /\ st1 = st2
    | PreGo o1, PreGo o2 => erase_dir o1 = o2 /\ erase_dir o2 = o2
    | _, _ => False
    end.
Proof.
  intros Hig Hdn Hp Hr s a b EH Ha Hb.
  assert (E1 : erase_dir (Hr ++ [ObBefore s]) = erase_dir (Hp ++ [ObBefore s]))
    by (rewrite !erase_dir_app, EH; reflexivity).
  assert (E2 : erase_dir (Hr ++ [ObBefore s; a; ObDirective s]) = erase_dir (Hp ++ [ObBefore s; a])).
  { rewrite !erase_dir_app, EH. reflexivity. }
  unfold space_prelude. rewrite (ob_accept_erase Hig Hr Hp _ EH).
  destruct (ob_accept Hp (ObBefore s)); cbn [negb].
  - rewrite (ob_accept_erase Hig _ _ a E1).
    destruct (ob_accept (Hp ++ [ObBefore s]) a); cbn [negb].
    + rewrite Hdn. rewrite (ob_accept_erase Hig _ _ b E2).
      destruct (ob_accept (Hp ++ [ObBefore s; a]) b); cbn [negb];
        repeat split; erase_simpl; reflexivity.
    + repeat split; erase_simpl; reflexivity.
  - repeat split; erase_simpl; reflexivity.
Qed.

(* The leaf of a commit that both sides reject: the same status, the calls
   of play (with the directive query erased), no event, and a store equal to
   the store before the operation. *)
Ltac sim_reject ES P1 P2 :=
  cbn [r_status r_obs r_ev r_st];
  split; [erase_simpl; rewrite P2; reflexivity |];
  split; [intros C; discriminate C |];
  intros _; split; [reflexivity |]; split; [erase_simpl; rewrite P1; reflexivity |];
  split; [reflexivity |];
  first [exact ES | eapply store_eq_trans; [apply bump_unbump_eq | exact ES]].

(* The commit of a produce in replay makes the decisions of play. Both make
   the same calls (replay adds the directive query), events and status, and
   leave equal stores. When play commits a fire, replay commits it if it
   paired. *)
Lemma replay_produce_sim :
  ob_ignores_directive -> dir_always_none ->
  forall Hp Hr Sp Sr rdp rdr ch v persist out j0 scomm sp,
    erase_dir Hr = erase_dir Hp -> store_eq Sr Sp ->
    let rp := commit_play_produce Hp Sp rdp ch v persist out j0 scomm in
    let rr := commit_replay_produce Hr Sr rdr ch v persist out j0 scomm sp in
    erase_dir (r_obs rp) = r_obs rp /\
    (r_status rp = StOk -> (match out with PStore => True | PFire _ _ _ _ => sp = true end) ->
       r_status rr = StOk /\ erase_dir (r_obs rr) = r_obs rp /\ r_ev rr = r_ev rp /\
       store_eq (r_st rr) (r_st rp)) /\
    (r_status rp <> StOk ->
       r_status rr = r_status rp /\ erase_dir (r_obs rr) = r_obs rp /\ r_ev rr = r_ev rp /\
       store_eq (r_st rr) (r_st rp)).
Proof.
  intros Hig Hdn Hp Hr Sp Sr rdp rdr ch v persist out j0 scomm sp EH ES rp rr. subst rp rr.
  set (src := produce_id ch v persist).
  pose proof (prelude_play_replay Hig Hdn Hp Hr (SrcP src) (ObStart (SrcP src) [ch] j0)
                (ObProduce src ch v persist) EH eq_refl eq_refl) as P.
  unfold commit_play_produce, commit_replay_produce. cbn zeta. fold src.
  revert P.
  destruct (space_prelude Replay Hr _ _ _) as [o1 st1 | o1];
    destruct (space_prelude Play Hp _ _ _) as [o2 st2 | o2]; intros P; try contradiction.
  - (* both stop in the call prefix *)
    destruct P as (P1 & P2 & P3). subst st1. cbn [r_status r_obs r_ev r_st].
    split; [exact P2 |].
    split; [intros Hs _; split; [exact Hs | split; [exact P1 | split; [reflexivity | exact ES]]] |].
    intros _. split; [reflexivity | split; [exact P1 | split; [reflexivity | exact ES]]].
  - destruct P as (P1 & P2).
    assert (Hacc : forall o, ob_accept (Hr ++ o1) o = ob_accept (Hp ++ o2) o).
    { intros o. apply (ob_accept_erase Hig). rewrite !erase_dir_app, EH, P1, P2. reflexivity. }
    destruct out as [| chs wk kidx cands].
    + (* store *)
      cbn [r_status r_obs r_ev r_st].
      split; [erase_simpl; rewrite P2; reflexivity |].
      split; [intros _ _ | intros C; exfalso; apply C; reflexivity].
      split; [reflexivity |]. split; [erase_simpl; rewrite P1; reflexivity |]. split; [reflexivity |].
      apply (proj1 (put_datum_cong _ _ _ _ (bump_cong _ _ _ _ ES))).
    + (* fire *)
      assert (Hlive : mk_comm cands (w_src wk) (w_peeks wk) (counters_of (s_count (bump Sr src persist))) =
                      mk_comm cands (w_src wk) (w_peeks wk) (play_counters_closure (s_count Sp) src persist)).
      { unfold mk_comm. cbv zeta. f_equal.
        change (s_count (bump Sr src persist)) with (incr (s_count Sr) src persist).
        rewrite <- play_closure_is_replay_closure. apply play_closure_ext.
        intros p. destruct ES as (_ & _ & _ & En). apply En. }
      destruct scomm as [cm |].
      * rewrite Hlive.
        destruct (comm_eqb (mk_comm cands (w_src wk) (w_peeks wk)
                              (play_counters_closure (s_count Sp) src persist)) cm); cbn [negb].
        -- rewrite Hacc.
           destruct (ob_accept (Hp ++ o2) (ObComm cm (w_kont wk) (w_persist wk) (comm_data cands)));
             cbn [negb].
           ++ (* play commits the fire *)
              cbn [r_status r_obs r_ev r_st].
              split; [erase_simpl; rewrite P2; reflexivity |].
              split; [| intros C; exfalso; apply C; reflexivity].
              intros _ Hsp. rewrite Hsp. cbn [negb r_status r_obs r_ev r_st].
              split; [reflexivity |]. split; [erase_simpl; rewrite P1; reflexivity |].
              split; [reflexivity |].
              assert (E1 : store_eq
                             (fst (if w_persist wk then (bump Sr src persist, [])
                                   else space_remove_continuation (bump Sr src persist) chs wk))
                             (fst (if w_persist wk then (bump Sp src persist, [])
                                   else space_remove_continuation (bump Sp src persist) chs wk))).
              { destruct (w_persist wk); cbn [fst];
                  [apply bump_cong, ES
                  | apply (proj1 (space_remove_continuation_cong _ _ _ _ (bump_cong _ _ _ _ ES)))]. }
              exact (proj1 (space_pf_loop_cong (sort_desc vkey cands) _ _ chs E1)).
           ++ (* the observer rejects the COMM *) sim_reject ES P1 P2.
        -- (* the staged COMM is not the live COMM *) sim_reject ES P1 P2.
      * (* a fire with no COMM *) sim_reject ES P1 P2.
Qed.

(* The commit of a consume in replay makes the decisions of play. *)
Lemma replay_consume_sim :
  ob_ignores_directive -> dir_always_none ->
  forall Hp Hr Sp Sr rdp rdr chs pats k persist peeks out scomm sp,
    erase_dir Hr = erase_dir Hp -> store_eq Sr Sp ->
    let rp := commit_play_consume Hp Sp rdp chs pats k persist peeks out scomm in
    let rr := commit_replay_consume Hr Sr rdr chs pats k persist peeks out scomm sp in
    erase_dir (r_obs rp) = r_obs rp /\
    (r_status rp = StOk -> (match out with CPark => True | CFire _ => sp = true end) ->
       r_status rr = StOk /\ erase_dir (r_obs rr) = r_obs rp /\ r_ev rr = r_ev rp /\
       store_eq (r_st rr) (r_st rp)) /\
    (r_status rp <> StOk ->
       r_status rr = r_status rp /\ erase_dir (r_obs rr) = r_obs rp /\ r_ev rr = r_ev rp /\
       store_eq (r_st rr) (r_st rp)).
Proof.
  intros Hig Hdn Hp Hr Sp Sr rdp rdr chs pats k persist peeks out scomm sp EH ES rp rr. subst rp rr.
  set (src := consume_id chs pats k persist).
  pose proof (prelude_play_replay Hig Hdn Hp Hr (SrcC src) (ObStart (SrcC src) chs [])
                (ObConsume src chs pats k persist peeks) EH eq_refl eq_refl) as P.
  unfold commit_play_consume, commit_replay_consume. cbn zeta. fold src.
  revert P.
  destruct (space_prelude Replay Hr _ _ _) as [o1 st1 | o1];
    destruct (space_prelude Play Hp _ _ _) as [o2 st2 | o2]; intros P; try contradiction.
  - destruct P as (P1 & P2 & P3). subst st1. cbn [r_status r_obs r_ev r_st].
    split; [exact P2 |].
    split; [intros Hs _; split; [exact Hs | split; [exact P1 | split; [reflexivity | exact ES]]] |].
    intros _. split; [reflexivity | split; [exact P1 | split; [reflexivity | exact ES]]].
  - destruct P as (P1 & P2).
    assert (Hacc : forall o, ob_accept (Hr ++ o1) o = ob_accept (Hp ++ o2) o).
    { intros o. apply (ob_accept_erase Hig). rewrite !erase_dir_app, EH, P1, P2. reflexivity. }
    destruct out as [| cands].
    + (* park *)
      cbn [r_status r_obs r_ev r_st].
      split; [erase_simpl; rewrite P2; reflexivity |].
      split; [intros _ _ | intros C; exfalso; apply C; reflexivity].
      split; [reflexivity |]. split; [erase_simpl; rewrite P1; reflexivity |]. split; [reflexivity |].
      apply (proj1 (store_waiting_continuation_cong _ _ _ _ ES)).
    + (* fire *)
      assert (Hlive : mk_comm cands src peeks (counters_of (s_count Sr)) =
                      mk_comm cands src peeks (counters_of (s_count Sp))).
      { apply mk_comm_counters_ext. intros p. destruct ES as (_ & _ & _ & En). apply En. }
      destruct scomm as [cm |].
      * rewrite Hlive.
        destruct (comm_eqb (mk_comm cands src peeks (counters_of (s_count Sp))) cm); cbn [negb].
        -- rewrite Hacc.
           destruct (ob_accept (Hp ++ o2) (ObComm cm k persist (comm_data cands))); cbn [negb].
           ++ cbn [r_status r_obs r_ev r_st].
              split; [erase_simpl; rewrite P2; reflexivity |].
              split; [| intros C; exfalso; apply C; reflexivity].
              intros _ Hsp. rewrite Hsp. cbn [negb r_status r_obs r_ev r_st].
              split; [reflexivity |]. split; [erase_simpl; rewrite P1; reflexivity |].
              split; [reflexivity |].
              exact (proj1 (space_cf_loop_cong (sort_desc vkey cands) _ _ ES)).
           ++ sim_reject ES P1 P2.
        -- sim_reject ES P1 P2.
      * sim_reject ES P1 P2.
Qed.

(* H23 for one staged operation: replay commits it as play does. A fire
   that play commits commits in replay when it paired. *)
Lemma replay_commit_op_sim :
  ob_ignores_directive -> dir_always_none ->
  forall m Hp Hr Sp Sr rdp rdr sgp sgr,
    backend m = Replay -> sg_strip sgr = sg_strip sgp ->
    erase_dir Hr = erase_dir Hp -> store_eq Sr Sp ->
    let rp := commit_op Play Hp Sp rdp sgp in
    let rr := commit_op m Hr Sr rdr sgr in
    erase_dir (r_obs rp) = r_obs rp /\
    (r_status rp = StOk -> (is_fire (sg_op sgr) = true -> sg_paired sgr = true) ->
       r_status rr = StOk /\ erase_dir (r_obs rr) = r_obs rp /\ r_ev rr = r_ev rp /\
       store_eq (r_st rr) (r_st rp)) /\
    (r_status rp <> StOk ->
       r_status rr = r_status rp /\ erase_dir (r_obs rr) = r_obs rp /\ r_ev rr = r_ev rp /\
       store_eq (r_st rr) (r_st rp)).
Proof.
  intros Hig Hdn m Hp Hr Sp Sr rdp rdr [opp jp cp spp] [opr jr cr spr] Hbm Hstrip EH ES rp rr.
  unfold sg_strip in Hstrip. cbn [sg_op sg_joins0 sg_comm] in Hstrip. injection Hstrip as -> -> ->.
  subst rp rr. unfold commit_op. cbn [sg_op sg_joins0 sg_comm sg_paired backend]. rewrite Hbm.
  destruct opp as [ch v persist out | chs pats k persist peeks out].
  - destruct (replay_produce_sim Hig Hdn Hp Hr Sp Sr rdp rdr ch v persist out jp cp spr EH ES)
      as (A & B & C).
    split; [exact A |]. split; [| exact C].
    intros Hok Hfp. apply B; [exact Hok |]. destruct out; [exact I | apply Hfp; reflexivity].
  - destruct (replay_consume_sim Hig Hdn Hp Hr Sp Sr rdp rdr chs pats k persist peeks out cp spr EH ES)
      as (A & B & C).
    split; [exact A |]. split; [| exact C].
    intros Hok Hfp. apply B; [exact Hok |]. destruct out; [exact I | apply Hfp; reflexivity].
Qed.


(* The log of a committed operation: its trigger IO event, then the COMM of
   a fire. *)
Lemma commit_op_ok_log :
  forall m H S rd sg, r_status (commit_op m H S rd sg) = StOk ->
    let r := commit_op m H S rd sg in
    io_of_log (r_ev r) = [trigger_key (sg_op sg)] /\
    (is_fire (sg_op sg) = true -> exists cm, sg_comm sg = Some cm /\ comms_of_log (r_ev r) = [cm]) /\
    (is_fire (sg_op sg) = false -> comms_of_log (r_ev r) = []).
Proof.
  intros m H S rd [op j0 scomm sp] Hs r. subst r. revert Hs.
  unfold commit_op. cbn [sg_op sg_joins0 sg_comm sg_paired].
  destruct (backend m);
    (destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]]);
    unfold commit_play_produce, commit_replay_produce, commit_play_consume, commit_replay_consume;
    cbn zeta; try destruct scomm as [cm |]; commit_split; cbn [r_status r_ev]; intros Hs;
    first
      [ discriminate Hs
      | match goal with
        | E : space_prelude _ _ _ _ _ = PreStop _ ?st |- _ =>
            apply space_prelude_status in E; subst st; discriminate Hs
        end
      | split; [reflexivity |];
        split; [intros Hf; first [discriminate Hf | eexists; split; reflexivity]
               | intros Hf; first [discriminate Hf | reflexivity]] ].
Qed.

Lemma io_of_log_app : forall a b, io_of_log (a ++ b) = io_of_log a ++ io_of_log b.
Proof. intros a b. unfold io_of_log. apply flat_map_app. Qed.

Lemma comms_of_log_app : forall a b, comms_of_log (a ++ b) = comms_of_log a ++ comms_of_log b.
Proof. intros a b. unfold comms_of_log. apply flat_map_app. Qed.

(* Decision 36: replay stages exactly the operations of play. With the view
   and the installed set of play, a staging that succeeds in play succeeds
   in replay, with the same operation, joins, COMM and view. Only the
   pairing flag and the paired list can differ. The rule of decision 19
   fails here: it rejects an unpaired replay fire at staging. *)
Lemma replay_stage_sim :
  forall Tp Tr op Tp',
    t_view Tr = t_view Tp -> t_installed Tr = t_installed Tp ->
    stage Tp op = Some Tp' ->
    exists Tr', stage Tr op = Some Tr' /\ t_view Tr' = t_view Tp' /\
      (forall sgp sgr, t_staged Tp' = t_staged Tp ++ [sgp] -> t_staged Tr' = t_staged Tr ++ [sgr] ->
         sg_strip sgr = sg_strip sgp).
Proof.
  intros Tp Tr op Tp' Hv Hi Hst.
  destruct op as [ch v persist [| chs wk kidx cands] | chs pats k persist peeks [| cands]];
    unfold stage in Hst.
  - (* produce, store *)
    destruct (t_installed Tp ch) eqn:Hi'; [discriminate |]. injection Hst as <-.
    eexists. split.
    { unfold stage. rewrite Hv, Hi, Hi'. reflexivity. }
    split; [reflexivity |].
    intros sgp sgr Ep Er. cbn [t_staged txn_push] in Ep, Er.
    apply app_inj_tail in Ep as [_ <-]. apply app_inj_tail in Er as [_ <-]. reflexivity.
  - (* produce, fire *)
    destruct (t_installed Tp ch) eqn:Hi'; [discriminate |].
    destruct (pfire_ok Tp ch v persist chs wk cands) eqn:Hok; cbn [negb] in Hst; [| discriminate].
    apply stage_fire_some in Hst. subst Tp'.
    assert (Hok' : pfire_ok Tr ch v persist chs wk cands = true).
    { unfold pfire_ok, not_installed in *. rewrite Hv, Hi. exact Hok. }
    eexists. split.
    { unfold stage. rewrite Hv, Hi, Hi', Hok'. cbn [negb]. unfold stage_fire. reflexivity. }
    split; [reflexivity |].
    intros sgp sgr Ep Er. cbn [t_staged txn_push] in Ep, Er.
    apply app_inj_tail in Ep as [_ <-]. apply app_inj_tail in Er as [_ <-]. reflexivity.
  - (* consume, park *)
    destruct (consume_args_ok Tp chs pats peeks) eqn:Hok; cbn [negb] in Hst; [| discriminate].
    injection Hst as <-.
    assert (Hok' : consume_args_ok Tr chs pats peeks = true).
    { unfold consume_args_ok, not_installed in *. rewrite Hi. exact Hok. }
    eexists. split.
    { unfold stage. rewrite Hv, Hok'. reflexivity. }
    split; [reflexivity |].
    intros sgp sgr Ep Er. cbn [t_staged txn_push] in Ep, Er.
    apply app_inj_tail in Ep as [_ <-]. apply app_inj_tail in Er as [_ <-]. reflexivity.
  - (* consume, fire *)
    destruct (consume_args_ok Tp chs pats peeks) eqn:Hok; cbn [negb] in Hst; [| discriminate].
    destruct (cfire_ok Tp chs cands) eqn:Hcf; cbn [negb] in Hst; [| discriminate].
    apply stage_fire_some in Hst. subst Tp'.
    assert (Hok' : consume_args_ok Tr chs pats peeks = true).
    { unfold consume_args_ok, not_installed in *. rewrite Hi. exact Hok. }
    assert (Hcf' : cfire_ok Tr chs cands = true).
    { unfold cfire_ok in *. rewrite Hv. exact Hcf. }
    eexists. split.
    { unfold stage. rewrite Hv, Hok', Hcf'. cbn [negb]. unfold stage_fire. reflexivity. }
    split; [reflexivity |].
    intros sgp sgr Ep Er. cbn [t_staged txn_push] in Ep, Er.
    apply app_inj_tail in Ep as [_ <-]. apply app_inj_tail in Er as [_ <-]. reflexivity.
Qed.


(* Replay stages every operation that play stages (replay_stage_sim). *)
Lemma replay_stage_all_sim :
  forall ops Tp Tp' Tr,
    t_view Tr = t_view Tp -> t_installed Tr = t_installed Tp ->
    stage_all Tp ops = Some Tp' ->
    exists Tr', stage_all Tr ops = Some Tr' /\
      map sg_strip (skipn (length (t_staged Tr)) (t_staged Tr')) =
      map sg_strip (skipn (length (t_staged Tp)) (t_staged Tp')).
Proof.
  induction ops as [| op rest IH]; intros Tp Tp' Tr Hv Hi Hst; simpl in Hst.
  - injection Hst as <-. exists Tr. split; [reflexivity |]. rewrite !skipn_len_self. reflexivity.
  - destruct (stage Tp op) as [Tp1 |] eqn:H1; [| discriminate].
    destruct (replay_stage_sim Tp Tr op Tp1 Hv Hi H1) as (Tr1 & H1r & Hv1 & Hs1).
    destruct (stage_props Tp op Tp1 H1) as (sgp & Hsgp & _ & _ & Hip & _).
    destruct (stage_props Tr op Tr1 H1r) as (sgr & Hsgr & _ & _ & Hir & _).
    assert (Hi1 : t_installed Tr1 = t_installed Tp1) by congruence.
    destruct (IH Tp1 Tp' Tr1 Hv1 Hi1 Hst) as (Tr' & Hst' & Hmap).
    exists Tr'. split; [simpl; rewrite H1r; exact Hst' |].
    destruct (stage_all_props rest Tp1 Tp' Hst) as ((newp & Hnewp & _) & _).
    destruct (stage_all_props rest Tr1 Tr' Hst') as ((newr & Hnewr & _) & _).
    rewrite (skipn_step Tp Tp1 Tp' sgp Hsgp (ex_intro _ newp Hnewp)),
            (skipn_step Tr Tr1 Tr' sgr Hsgr (ex_intro _ newr Hnewr)).
    simpl. rewrite Hmap, (Hs1 sgp sgr Hsgp Hsgr). reflexivity.
Qed.

(* The simulation of H23 over a run of staged operations. The bindings of
   replay hold the COMMs that play commits, so replay pairs each fire that
   play commits. An operation that play rejects stops both runs with the
   same calls, and replay stages the remaining operations as play does. *)
Lemma replay_sim_gen :
  ob_ignores_directive -> dir_always_none ->
  forall ops Tp Tp' Tr Hp Hr Sp Sr rdp rdr io rem,
    t_mode Tp = Play -> backend (t_mode Tr) = Replay ->
    t_view Tr = t_view Tp -> t_installed Tr = t_installed Tp ->
    stage_all Tp ops = Some Tp' ->
    erase_dir Hr = erase_dir Hp -> store_eq Sr Sp ->
    binding_inv (t_rd Tr) io rem ->
    incl (io_of_log (r_ev (commit_run Play Hp Sp rdp (skipn (length (t_staged Tp)) (t_staged Tp'))))) io ->
    (forall c, count_occ comm_eq_dec (t_paired Tr) c +
               count_occ comm_eq_dec
                 (comms_of_log (r_ev (commit_run Play Hp Sp rdp (skipn (length (t_staged Tp)) (t_staged Tp'))))) c
               <= rem c) ->
    exists Tr', stage_all Tr ops = Some Tr' /\
      map sg_strip (skipn (length (t_staged Tr)) (t_staged Tr')) =
      map sg_strip (skipn (length (t_staged Tp)) (t_staged Tp')) /\
      let rp := commit_run Play Hp Sp rdp (skipn (length (t_staged Tp)) (t_staged Tp')) in
      let rr := commit_run (t_mode Tr) Hr Sr rdr (skipn (length (t_staged Tr)) (t_staged Tr')) in
      r_status rr = r_status rp /\ erase_dir (r_obs rr) = r_obs rp /\ r_ev rr = r_ev rp /\
      store_eq (r_st rr) (r_st rp).
Proof.
  intros Hig Hdn. induction ops as [| op rest IH];
    intros Tp Tp' Tr Hp Hr Sp Sr rdp rdr io rem Hmp Hbr Hv Hi Hst EH ES Hinv Hio Hcnt; simpl in Hst.
  - injection Hst as <-. exists Tr. split; [reflexivity |].
    rewrite !skipn_len_self. split; [reflexivity |]. cbv zeta. cbn [commit_run r_status r_obs r_ev r_st].
    split; [reflexivity |]. split; [reflexivity |]. split; [reflexivity | exact ES].
  - destruct (stage Tp op) as [Tp1 |] eqn:H1; [| discriminate].
    destruct (replay_stage_sim Tp Tr op Tp1 Hv Hi H1) as (Tr1 & H1r & Hv1 & Hs1).
    destruct (stage_props Tp op Tp1 H1) as (sgp & Hsgp & Hopp & Hmp1 & Hip & _).
    destruct (stage_props Tr op Tr1 H1r) as (sgr & Hsgr & Hopr & Hmr1 & Hir & Hrdr).
    pose proof (Hs1 sgp sgr Hsgp Hsgr) as Hstrip.
    destruct (commit_op_coherent Tr op Tr1 [] (view_store (t_view Tr)) (fun _ _ => 0) H1r (store_eq_refl _))
      as (sgr' & Hsgr' & _ & _ & _ & _ & Hpn & Hps & _).
    rewrite Hsgr in Hsgr'. apply app_inj_tail in Hsgr' as [_ <-].
    assert (Hi1 : t_installed Tr1 = t_installed Tp1) by congruence.
    destruct (stage_all_props rest Tp1 Tp' Hst) as ((newp & Hnewp & _) & _).
    rewrite (skipn_step Tp Tp1 Tp' sgp Hsgp (ex_intro _ newp Hnewp)) in Hio, Hcnt |- *.
    cbn [commit_run] in Hio, Hcnt |- *. cbv zeta.
    set (restp := skipn (length (t_staged Tp1)) (t_staged Tp')) in *.
    set (rp1 := commit_op Play Hp Sp rdp sgp) in *.
    destruct (replay_commit_op_sim Hig Hdn (t_mode Tr) Hp Hr Sp Sr rdp rdr sgp sgr Hbr Hstrip EH ES)
      as (Hnd & Hok & Hnok).
    fold rp1 in Hnd, Hok, Hnok.
    set (rr1 := commit_op (t_mode Tr) Hr Sr rdr sgr) in *.
    revert Hio Hcnt.
    destruct (r_status rp1) eqn:Hs; intros Hio Hcnt.
    + (* play commits the operation: replay pairs it and commits it *)
      cbn [r_status r_obs r_ev r_st] in Hio, Hcnt |- *.
      destruct (commit_op_ok_log Play Hp Sp rdp sgp Hs) as (Lio & Lf & Ln).
      fold rp1 in Lio, Lf, Ln. rewrite Hopp in Lio, Lf, Ln.
      assert (Hcomm : sg_comm sgr = sg_comm sgp).
      { unfold sg_strip in Hstrip. injection Hstrip as _ _ E. exact E. }
      assert (Hpaired : is_fire (sg_op sgr) = true -> sg_paired sgr = true).
      { intros Hf. rewrite Hopr in Hf.
        destruct (Lf Hf) as (cm & Hcp & Lc).
        destruct (Hps cm ltac:(rewrite Hcomm; exact Hcp)) as (_ & Hsp & Hkey & _).
        rewrite Hsp, Hbr.
        apply (proj2 (verify_spec (t_rd Tr) io rem (t_paired Tr) (trigger_key op) cm Hinv)).
        split; [| split; [exact Hkey |]].
        - apply Hio. rewrite io_of_log_app, Lio. left. reflexivity.
        - specialize (Hcnt cm). rewrite comms_of_log_app, Lc, count_occ_app in Hcnt.
          cbn [count_occ] in Hcnt. destruct (comm_eq_dec cm cm) as [_ | C]; [lia | contradiction]. }
      destruct (Hok eq_refl Hpaired) as (Rs & Robs & Rev & Rst).
      assert (Hio1 : incl (io_of_log (r_ev (commit_run Play (Hp ++ r_obs rp1) (r_st rp1) (r_rd rp1) restp))) io).
      { intros k Hk. apply Hio. rewrite io_of_log_app. apply in_or_app. right. exact Hk. }
      assert (Hcnt1 : forall c, count_occ comm_eq_dec (t_paired Tr1) c +
                        count_occ comm_eq_dec
                          (comms_of_log (r_ev (commit_run Play (Hp ++ r_obs rp1) (r_st rp1) (r_rd rp1) restp))) c
                        <= rem c).
      { intros c. specialize (Hcnt c). rewrite comms_of_log_app, count_occ_app in Hcnt.
        destruct (is_fire op) eqn:Hf.
        - destruct (Lf eq_refl) as (cm & Hcp & Lc).
          destruct (Hps cm ltac:(rewrite Hcomm; exact Hcp)) as (Hpt & _ & _ & _).
          rewrite Hpt, (Hpaired ltac:(rewrite Hopr; exact Hf)), count_occ_app.
          rewrite Lc in Hcnt. lia.
        - destruct (sg_comm sgr) as [cm |] eqn:Hc.
          + destruct (Hps cm eq_refl) as (_ & _ & _ & Hf'). congruence.
          + destruct (Hpn eq_refl) as (Hpt & _). rewrite Hpt. rewrite (Ln eq_refl) in Hcnt. exact Hcnt. }
      assert (EH1 : erase_dir (Hr ++ r_obs rr1) = erase_dir (Hp ++ r_obs rp1)).
      { rewrite !erase_dir_app, EH, Robs, Hnd. reflexivity. }
      destruct (IH Tp1 Tp' Tr1 (Hp ++ r_obs rp1) (Hr ++ r_obs rr1) (r_st rp1) (r_st rr1) (r_rd rp1) (r_rd rr1)
                   io rem ltac:(congruence) ltac:(rewrite Hmr1; exact Hbr) Hv1 Hi1 Hst EH1 Rst
                   ltac:(rewrite Hrdr; exact Hinv) Hio1 Hcnt1)
        as (Tr' & Hst' & Hmap & Rs' & Robs' & Rev' & Rst').
      exists Tr'. split; [simpl; rewrite H1r; exact Hst' |].
      destruct (stage_all_props rest Tr1 Tr' Hst') as ((newr & Hnewr & _) & _).
      rewrite (skipn_step Tr Tr1 Tr' sgr Hsgr (ex_intro _ newr Hnewr)).
      split; [simpl; rewrite Hmap, Hstrip; reflexivity |].
      cbn [commit_run]. cbv zeta. fold rr1. rewrite Rs.
      cbn [r_status r_obs r_ev r_st].
      rewrite Hmr1 in Rs', Robs', Rev', Rst'.
      split; [exact Rs' |]. split; [rewrite erase_dir_app, Robs, Robs'; reflexivity |].
      split; [rewrite Rev, Rev'; reflexivity | exact Rst'].
    + (* play rejects the operation: so does replay, and both runs stop *)
      destruct (Hnok ltac:(congruence)) as (Rs & Robs & Rev & Rst).
      destruct (replay_stage_all_sim rest Tp1 Tp' Tr1 Hv1 Hi1 Hst) as (Tr' & Hst' & Hmap).
      exists Tr'. split; [simpl; rewrite H1r; exact Hst' |].
      destruct (stage_all_props rest Tr1 Tr' Hst') as ((newr & Hnewr & _) & _).
      rewrite (skipn_step Tr Tr1 Tr' sgr Hsgr (ex_intro _ newr Hnewr)).
      split; [simpl; rewrite Hmap, Hstrip; reflexivity |].
      cbn [commit_run]. cbv zeta. fold rr1. rewrite Rs, Hs.
      split; [exact Rs |]. split; [exact Robs |]. split; [exact Rev | exact Rst].
    + destruct (Hnok ltac:(congruence)) as (Rs & Robs & Rev & Rst).
      destruct (replay_stage_all_sim rest Tp1 Tp' Tr1 Hv1 Hi1 Hst) as (Tr' & Hst' & Hmap).
      exists Tr'. split; [simpl; rewrite H1r; exact Hst' |].
      destruct (stage_all_props rest Tr1 Tr' Hst') as ((newr & Hnewr & _) & _).
      rewrite (skipn_step Tr Tr1 Tr' sgr Hsgr (ex_intro _ newr Hnewr)).
      split; [simpl; rewrite Hmap, Hstrip; reflexivity |].
      cbn [commit_run]. cbv zeta. fold rr1. rewrite Rs, Hs.
      split; [exact Rs |]. split; [exact Robs |]. split; [exact Rev | exact Rst].
    + destruct (Hnok ltac:(congruence)) as (Rs & Robs & Rev & Rst).
      destruct (replay_stage_all_sim rest Tp1 Tp' Tr1 Hv1 Hi1 Hst) as (Tr' & Hst' & Hmap).
      exists Tr'. split; [simpl; rewrite H1r; exact Hst' |].
      destruct (stage_all_props rest Tr1 Tr' Hst') as ((newr & Hnewr & _) & _).
      rewrite (skipn_step Tr Tr1 Tr' sgr Hsgr (ex_intro _ newr Hnewr)).
      split; [simpl; rewrite Hmap, Hstrip; reflexivity |].
      cbn [commit_run]. cbv zeta. fold rr1. rewrite Rs, Hs.
      split; [exact Rs |]. split; [exact Robs |]. split; [exact Rev | exact Rst].
    + destruct (Hnok ltac:(congruence)) as (Rs & Robs & Rev & Rst).
      destruct (replay_stage_all_sim rest Tp1 Tp' Tr1 Hv1 Hi1 Hst) as (Tr' & Hst' & Hmap).
      exists Tr'. split; [simpl; rewrite H1r; exact Hst' |].
      destruct (stage_all_props rest Tr1 Tr' Hst') as ((newr & Hnewr & _) & _).
      rewrite (skipn_step Tr Tr1 Tr' sgr Hsgr (ex_intro _ newr Hnewr)).
      split; [simpl; rewrite Hmap, Hstrip; reflexivity |].
      cbn [commit_run]. cbv zeta. fold rr1. rewrite Rs, Hs.
      split; [exact Rs |]. split; [exact Robs |]. split; [exact Rev | exact Rst].
Qed.

(* H23 (decision 36): replay reproduces the run of play. Play and replay
   stage the same operations from the same view and installed set, and
   their observer histories agree up to the directive queries. When the
   bindings of replay hold the IO events and the COMMs that play commits
   (an honest log), replay stages exactly the operations of play, and its
   commit makes the observer calls of play (with the directive queries
   erased), the events of play, the status of play and the store of play.
   So an operation that play rejects (an out-of-phlo charge) fails replay
   at the same call, after the same prefix (D6). The second conjunct rigs
   replay with the commit log of play (replay_rspace.rs:480-540). *)
Theorem replay_reproduces_play_run :
  ob_ignores_directive -> dir_always_none ->
  (forall ops Tp0 Tp Tr0 Hp Hr S rdp rdr io rem,
     t_mode Tp0 = Play -> backend (t_mode Tr0) = Replay ->
     t_view Tr0 = t_view Tp0 -> t_installed Tr0 = t_installed Tp0 ->
     t_staged Tp0 = [] -> t_staged Tr0 = [] -> t_paired Tr0 = [] ->
     stage_all Tp0 ops = Some Tp ->
     erase_dir Hr = erase_dir Hp ->
     binding_inv (t_rd Tr0) io rem ->
     let rp := commit_run Play Hp S rdp (t_staged Tp) in
     incl (io_of_log (r_ev rp)) io ->
     (forall c, count_occ comm_eq_dec (comms_of_log (r_ev rp)) c <= rem c) ->
     exists Tr, stage_all Tr0 ops = Some Tr /\
       map sg_strip (t_staged Tr) = map sg_strip (t_staged Tp) /\
       let rr := commit_run (t_mode Tr0) Hr S rdr (t_staged Tr) in
       r_status rr = r_status rp /\ erase_dir (r_obs rr) = r_obs rp /\ r_ev rr = r_ev rp /\
       store_eq (r_st rr) (r_st rp)) /\
  (forall ops Tp0 Tp Tr0 Hp Hr S rdp rdr,
     t_mode Tp0 = Play -> backend (t_mode Tr0) = Replay ->
     t_view Tr0 = t_view Tp0 -> t_installed Tr0 = t_installed Tp0 ->
     t_staged Tp0 = [] -> t_staged Tr0 = [] -> t_paired Tr0 = [] ->
     stage_all Tp0 ops = Some Tp ->
     erase_dir Hr = erase_dir Hp ->
     let rp := commit_run Play Hp S rdp (t_staged Tp) in
     t_rd Tr0 = rig (r_ev rp) ->
     exists Tr, stage_all Tr0 ops = Some Tr /\
       map sg_strip (t_staged Tr) = map sg_strip (t_staged Tp) /\
       let rr := commit_run (t_mode Tr0) Hr S rdr (t_staged Tr) in
       r_status rr = r_status rp /\ erase_dir (r_obs rr) = r_obs rp /\ r_ev rr = r_ev rp /\
       store_eq (r_st rr) (r_st rp)).
Proof.
  intros Hig Hdn.
  assert (G : forall ops Tp0 Tp Tr0 Hp Hr S rdp rdr io rem,
     t_mode Tp0 = Play -> backend (t_mode Tr0) = Replay ->
     t_view Tr0 = t_view Tp0 -> t_installed Tr0 = t_installed Tp0 ->
     t_staged Tp0 = [] -> t_staged Tr0 = [] -> t_paired Tr0 = [] ->
     stage_all Tp0 ops = Some Tp ->
     erase_dir Hr = erase_dir Hp ->
     binding_inv (t_rd Tr0) io rem ->
     let rp := commit_run Play Hp S rdp (t_staged Tp) in
     incl (io_of_log (r_ev rp)) io ->
     (forall c, count_occ comm_eq_dec (comms_of_log (r_ev rp)) c <= rem c) ->
     exists Tr, stage_all Tr0 ops = Some Tr /\
       map sg_strip (t_staged Tr) = map sg_strip (t_staged Tp) /\
       let rr := commit_run (t_mode Tr0) Hr S rdr (t_staged Tr) in
       r_status rr = r_status rp /\ erase_dir (r_obs rr) = r_obs rp /\ r_ev rr = r_ev rp /\
       store_eq (r_st rr) (r_st rp)).
  { intros ops Tp0 Tp Tr0 Hp Hr S rdp rdr io rem Hmp Hbr Hv Hi Hsp0 Hsr0 Hpr0 Hst EH Hinv rp Hio Hcnt.
    destruct (replay_sim_gen Hig Hdn ops Tp0 Tp Tr0 Hp Hr S S rdp rdr io rem Hmp Hbr Hv Hi Hst EH
                (store_eq_refl S) Hinv ltac:(rewrite Hsp0; exact Hio) ltac:(rewrite Hpr0, Hsp0; exact Hcnt))
      as (Tr & Hstr & Hmap & Rs & Robs & Rev & Rst).
    rewrite Hsr0, Hsp0 in Hmap, Rs, Robs, Rev, Rst. cbn [length skipn] in Hmap, Rs, Robs, Rev, Rst.
    exists Tr. split; [exact Hstr |]. split; [exact Hmap |]. cbv zeta.
    split; [exact Rs |]. split; [exact Robs |]. split; [exact Rev | exact Rst]. }
  split; [exact G |].
  intros ops Tp0 Tp Tr0 Hp Hr S rdp rdr Hmp Hbr Hv Hi Hsp0 Hsr0 Hpr0 Hst EH rp Hrig.
  apply (G ops Tp0 Tp Tr0 Hp Hr S rdp rdr (io_of_log (r_ev rp))
           (fun c => count_occ comm_eq_dec (comms_of_log (r_ev rp)) c)
           Hmp Hbr Hv Hi Hsp0 Hsr0 Hpr0 Hst EH).
  - rewrite Hrig. apply rig_binding_inv.
  - intros k Hk. exact Hk.
  - intros c. apply Nat.le_refl.
Qed.

End Substrate.

(* ═══════════════ Sub-orders (H8, decision D3) ════════════════════════════ *)

(* The model compares a causal path through its Vec projection (to_vec).
   That order is the lexicographic order on (u64, u64) segments. The
   persistent CausalPath comparison agrees with it (operation_context.rs
   138-159 and the HEAD proptests at 193-275). *)
Definition seg_lt (x y : nat * nat) : Prop := fst x < fst y \/ (fst x = fst y /\ snd x < snd y).

Fixpoint path_lt (a b : list (nat * nat)) : Prop :=
  match a, b with
  | [], [] => False
  | [], _ :: _ => True
  | _ :: _, [] => False
  | x :: a', y :: b' => seg_lt x y \/ (x = y /\ path_lt a' b')
  end.

(* Paths that the deterministic scheduler creates (deterministic_reduction.rs
   84-135: next_operation and split). The root participant is []. Child i of a
   split at step s appends the one segment (s, i + 1): the compacted split of
   feature/cost-accounted-rho (C9, DR-84), which this campaign merged at
   4d67b5ccc. An operation appends (step, 0) to its participant. A participant
   uses one step counter for its operations and its splits. *)
Inductive sched_part : list (nat * nat) -> Prop :=
| sp_root : sched_part []
| sp_split : forall p s i, sched_part p -> sched_part (p ++ [(s, Datatypes.S i)]).
(* Disabled by the merge of feature/cost-accounted-rho at 4d67b5ccc (DR-84):
   the legacy splitter appended the two segments (s, 1) and (i, 0). The Rust
   program keeps that splitter only as the test oracle split_legacy, so the
   model describes the compacted split.
| sp_split : forall p s i, sched_part p -> sched_part (p ++ [(s, 1); (i, 0)]).
*)

Definition sched_op (q : list (nat * nat)) : Prop :=
  exists p s, sched_part p /\ q = p ++ [(s, 0)].

Definition SUB_OPERATION_TAG : nat := 2.

(* sub_order(parent, i) = parent.path ++ [(i, SUB_OPERATION_TAG)]. *)
Definition sub_order (parent : list (nat * nat)) (i : nat) : list (nat * nat) :=
  parent ++ [(i, SUB_OPERATION_TAG)].

(* OperationOrder derives Ord: the session first, then the path
   (operation_context.rs:200-204). *)
Definition order_lt (a b : nat * list (nat * nat)) : Prop :=
  fst a < fst b \/ (fst a = fst b /\ path_lt (snd a) (snd b)).

Lemma path_lt_ext : forall a r, r <> [] -> path_lt a (a ++ r).
Proof.
  induction a as [| x a IH]; intros r Hr; simpl.
  - destruct r; [contradiction | exact I].
  - right. split; [reflexivity | apply IH, Hr].
Qed.

Lemma path_lt_seg : forall pre x y u v, seg_lt x y -> path_lt (pre ++ x :: u) (pre ++ y :: v).
Proof.
  induction pre as [| z pre IH]; intros x y u v Hs; simpl; [left; exact Hs |].
  right. split; [reflexivity | apply IH, Hs].
Qed.

Lemma path_lt_decompose :
  forall a b, path_lt a b ->
    (exists r, r <> [] /\ b = a ++ r) \/
    (exists pre x y a' b', a = pre ++ x :: a' /\ b = pre ++ y :: b' /\ seg_lt x y).
Proof.
  induction a as [| x a IH]; intros b Hlt.
  - destruct b as [| y b]; [contradiction |]. left. exists (y :: b). split; [discriminate | reflexivity].
  - destruct b as [| y b]; [contradiction |]. simpl in Hlt.
    destruct Hlt as [Hs | (E & Hlt)].
    + right. exists [], x, y, a, b. auto.
    + subst y. destruct (IH b Hlt) as [(r & Hr & E) | (pre & x' & y' & a' & b' & Ea & Eb & Hs)].
      * left. exists r. split; [exact Hr | rewrite E; reflexivity].
      * right. exists (x :: pre), x', y', a', b'. rewrite Ea, Eb. auto.
Qed.

Lemma seg_lt_trans : forall x y z, seg_lt x y -> seg_lt y z -> seg_lt x z.
Proof. intros [a b] [c d] [e f]. unfold seg_lt. simpl. lia. Qed.

Lemma path_lt_trans : forall a b c, path_lt a b -> path_lt b c -> path_lt a c.
Proof.
  induction a as [| x a IH]; intros b c H1 H2.
  - destruct b as [| y b]; [contradiction |]. destruct c as [| z c]; [contradiction | exact I].
  - destruct b as [| y b]; [contradiction |]. destruct c as [| z c]; [contradiction |].
    simpl in *. destruct H1 as [S1 | (E1 & L1)]; destruct H2 as [S2 | (E2 & L2)].
    + left. eapply seg_lt_trans; eassumption.
    + subst z. left. exact S1.
    + subst y. left. exact S2.
    + subst y z. right. split; [reflexivity | eapply IH; eassumption].
Qed.

Lemma path_lt_irrefl : forall a, ~ path_lt a a.
Proof.
  induction a as [| x a IH]; simpl; [auto |].
  intros [Hs | (_ & Hl)]; [destruct x; unfold seg_lt in Hs; simpl in Hs; lia | exact (IH Hl)].
Qed.

(* Every segment of a participant path has a non-zero second component,
   because a split segment is (s, i + 1). An operation segment (s, 0) thus
   never occurs inside a participant path. *)
Lemma sched_part_nonzero :
  forall p, sched_part p -> forall j x, nth_error p j = Some x -> snd x <> 0.
Proof.
  intros p Hp. induction Hp as [| p s i Hp IH]; intros j x Hj.
  - destruct j; discriminate.
  - destruct (Nat.lt_ge_cases j (length p)) as [Hlt | Hge].
    + rewrite nth_error_app1 in Hj by exact Hlt. exact (IH j x Hj).
    + rewrite nth_error_app2 in Hj by exact Hge.
      destruct (j - length p) as [| k].
      * simpl in Hj. injection Hj as <-. simpl. lia.
      * destruct k; simpl in Hj; discriminate.
Qed.

(* Disabled by the merge of feature/cost-accounted-rho at 4d67b5ccc (DR-84):
   the parity shape holds only for the legacy split (s, 1) (i, 0), and
   sched_part_nonzero replaces it in the proof of sched_op_no_extension.
Lemma sched_part_shape :
  forall p, sched_part p ->
    Nat.even (length p) = true /\
    (forall j, j < length p -> Nat.even j = true -> exists s, nth_error p j = Some (s, 1)).
Proof.
  intros p Hp. induction Hp as [| p s i Hp (He & Hj)]; [split; [reflexivity | intros j Hjl; simpl in Hjl; lia] |].
  rewrite length_app. simpl. split.
  - replace (length p + 2) with (Datatypes.S (Datatypes.S (length p))) by lia. simpl. exact He.
  - intros j Hjl Hje.
    destruct (Nat.lt_ge_cases j (length p)) as [Hlt | Hge].
    + rewrite nth_error_app1 by exact Hlt. apply Hj; assumption.
    + rewrite nth_error_app2 by exact Hge.
      destruct (Nat.eq_dec j (length p)) as [E | N].
      * subst j. rewrite Nat.sub_diag. exists s. reflexivity.
      * assert (Ej : j = Datatypes.S (length p)) by lia. subst j.
        exfalso. rewrite Nat.even_succ, <- Nat.negb_even, He in Hje. discriminate.
Qed.
*)

(* No scheduler operation path is a proper prefix of another one. Let
   a = p ++ [(s, 0)] be a prefix of b = p' ++ [(s', 0)]. Then p' is longer
   than p, so the segment of b at the position length p is a segment of p'
   (non-zero by sched_part_nonzero) and is also the last segment (s, 0) of a. *)
Lemma sched_op_no_extension :
  forall a b r, sched_op a -> sched_op b -> r <> [] -> b <> a ++ r.
Proof.
  intros a b r (p & s & Hp & Ea) (p' & s' & Hp' & Eb) Hr E.
  assert (Hlen : length b = length a + length r) by (rewrite E, length_app; reflexivity).
  assert (Hr1 : 1 <= length r) by (destruct r; [contradiction | simpl; lia]).
  assert (Hla : length a = Datatypes.S (length p)) by (rewrite Ea, length_app; simpl; lia).
  assert (Hlb : length b = Datatypes.S (length p')) by (rewrite Eb, length_app; simpl; lia).
  assert (Hlt : length p < length p') by lia.
  assert (Hna : nth_error b (length p) = Some (s, 0)).
  { rewrite E, nth_error_app1 by lia. rewrite Ea, nth_error_app2 by lia. rewrite Nat.sub_diag. reflexivity. }
  assert (Hnb : nth_error p' (length p) = Some (s, 0)).
  { rewrite <- Hna, Eb, nth_error_app1 by exact Hlt. reflexivity. }
  exact (sched_part_nonzero p' Hp' (length p) (s, 0) Hnb eq_refl).
Qed.

(* H8: a sub-order sorts after its parent and before every later scheduler
   order. The sub-orders of one parent strictly increase. No scheduler order
   equals a sub-order. Thus each staged operation gets its own row, and the
   event groups stay together when take_ordered_event_log flattens
   ordered_event_log. *)
Theorem sub_order_between :
  (forall parent i, path_lt parent (sub_order parent i)) /\
  (forall parent i j, i < j -> path_lt (sub_order parent i) (sub_order parent j)) /\
  (forall parent q i, sched_op parent -> sched_op q -> path_lt parent q ->
     path_lt (sub_order parent i) q) /\
  (forall p s i, sched_part p -> path_lt (sub_order (p ++ [(s, 0)]) i) (p ++ [(Datatypes.S s, 0)])) /\
  (forall q parent i, sched_op q -> q <> sub_order parent i) /\
  (forall p1 p2 i j, sched_op p1 -> sched_op p2 -> path_lt p1 p2 ->
     path_lt (sub_order p1 i) (sub_order p2 j)) /\
  (forall sess parent q i, sched_op parent -> sched_op (snd q) -> order_lt (sess, parent) q ->
     order_lt (sess, sub_order parent i) q).
Proof.
  assert (Ha : forall parent i, path_lt parent (sub_order parent i))
    by (intros parent i; apply path_lt_ext; discriminate).
  assert (Hc : forall parent q i, sched_op parent -> sched_op q -> path_lt parent q ->
                 path_lt (sub_order parent i) q).
  { intros parent q i Hp Hq Hlt.
    destruct (path_lt_decompose parent q Hlt) as [(r & Hr & E) | (pre & x & y & a' & b' & Ea & Eb & Hs)].
    - exfalso. exact (sched_op_no_extension parent q r Hp Hq Hr E).
    - unfold sub_order. rewrite Ea, Eb, <- app_assoc. simpl. apply path_lt_seg, Hs. }
  split; [exact Ha |]. split.
  { intros parent i j Hij. unfold sub_order.
    rewrite <- (app_nil_r parent) at 1. rewrite <- (app_nil_r parent) at 2.
    rewrite <- !app_assoc. simpl. apply path_lt_seg. unfold seg_lt. simpl. left. exact Hij. }
  split; [exact Hc |]. split.
  { intros p s i Hp. apply Hc.
    - exists p, s. auto.
    - exists p, (Datatypes.S s). auto.
    - apply path_lt_seg. unfold seg_lt. simpl. left. lia. }
  split.
  { intros q parent i (p & s & Hp & Eq) E. unfold sub_order, SUB_OPERATION_TAG in E. rewrite Eq in E.
    apply app_inj_tail in E as [_ E]. injection E as _ C. discriminate. }
  split.
  { intros p1 p2 i j H1 H2 Hlt. eapply path_lt_trans; [apply (Hc p1 p2 i H1 H2 Hlt) | apply Ha]. }
  { intros sess parent q i Hp Hq [Hs | (Es & Hl)].
    - left. exact Hs.
    - right. split; [exact Es | apply Hc; assumption]. }
Qed.

(* ═════════ One transaction per parent order (H20, FR-5 and FR-5') ═════════ *)

Definition seg_eq_dec : forall x y : nat * nat, {x = y} + {x <> y}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition path_eq_dec : forall x y : list (nat * nat), {x = y} + {x <> y} :=
  list_eq_dec seg_eq_dec.

(* The tracker of committed parents (FR-5 and FR-5'). It records the parent
   paths that committed in the current evaluation scope, with the operation
   session (OperationOrder.session, the deploy id) and the identity of the
   host's store Arc of that scope. None means that the tracker records no
   session or store. The tracker is deterministic and bounded by the
   commits of one scope. *)
Record Tracker := mkTracker {
  tr_session : option nat; tr_store : option nat; tr_parents : list (list (nat * nat)) }.

(* The tracker of a new handle, and the tracker after a scope reset. *)
Definition tracker_reset : Tracker := mkTracker None None [].

(* A commit request: no operation context, or a parent order (session and
   path) with the identity of the host's store at the commit and the number
   of staged operations that the commit applies. A successful commit
   commits on the store of its begin (StoreSwapped otherwise, H9), so the
   store identity is the store of the transaction. *)
Inductive CommitReq :=
| ReqNoParent (n : nat)
| ReqParent (sess store : nat) (parent : list (nat * nat)) (n : nat).

(* A call that reaches the tracker: a scope reset
   (SpaceStoreHandle::begin_evaluation_scope, at the entry of each root
   evaluation in play and in replay) or a commit. *)
Inductive HandleCall :=
| CallScope
| CallCommit (req : CommitReq).

Definition sub_orders (sess : nat) (parent : list (nat * nat)) (n : nat) : list (nat * list (nat * nat)) :=
  map (fun i => (sess, sub_order parent i)) (seq 0 n).

(* The tracker is in the scope of a commit when it records the session and
   the store identity of the commit. *)
Definition same_scope (tr : Tracker) (sess store : nat) : bool :=
  match tr_session tr, tr_store tr with
  | Some s, Some st => Nat.eqb s sess && Nat.eqb st store
  | _, _ => false
  end.

(* The parents that a commit with this session and store must not reuse. A
   session change or a store swap starts a new scope with no parent. *)
Definition scope_parents (tr : Tracker) (sess store : nat) : list (list (nat * nat)) :=
  if same_scope tr sess store then tr_parents tr else [].

(* FR-5 and FR-5': a commit under a parent that already committed in the
   current scope fails with ParentOrderReused before any effect (None). A
   commit with another session or another store identity resets the
   tracker first. A commit with no parent is not tracked and emits no
   sub-order. *)
Definition commit_gate (tr : Tracker) (req : CommitReq)
    : option (Tracker * list (nat * list (nat * nat))) :=
  match req with
  | ReqNoParent _ => Some (tr, [])
  | ReqParent sess store parent n =>
      let base := scope_parents tr sess store in
      if in_dec path_eq_dec parent base then None
      else Some (mkTracker (Some sess) (Some store) (parent :: base), sub_orders sess parent n)
  end.

(* FR-5': begin_evaluation_scope resets the tracker. *)
Definition handle_step (tr : Tracker) (call : HandleCall)
    : option (Tracker * list (nat * list (nat * nat))) :=
  match call with
  | CallScope => Some (tracker_reset, [])
  | CallCommit req => commit_gate tr req
  end.

(* A run of handle calls. A rejected commit emits nothing and keeps the
   tracker. *)
Fixpoint run_calls (tr : Tracker) (calls : list HandleCall) : Tracker * list (nat * list (nat * nat)) :=
  match calls with
  | [] => (tr, [])
  | c :: rest =>
      match handle_step tr c with
      | None => run_calls tr rest
      | Some (tr', out) => let res := run_calls tr' rest in (fst res, out ++ snd res)
      end
  end.

(* A call in one scope: a commit with no parent, or a commit with the
   session [s] and the store identity [st]. A scope reset ends the scope. *)
Definition call_in_scope (s st : nat) (c : HandleCall) : Prop :=
  match c with
  | CallScope => False
  | CallCommit (ReqNoParent _) => True
  | CallCommit (ReqParent sess store _ _) => sess = s /\ store = st
  end.

Lemma sub_order_inj :
  forall p1 p2 i j, sub_order p1 i = sub_order p2 j -> p1 = p2 /\ i = j.
Proof.
  intros p1 p2 i j E. unfold sub_order in E. apply app_inj_tail in E as [E1 E2].
  injection E2 as E2. split; assumption.
Qed.

Lemma sub_orders_nodup : forall sess parent n, NoDup (sub_orders sess parent n).
Proof.
  intros sess parent n. unfold sub_orders. apply FinFun.Injective_map_NoDup; [| apply seq_NoDup].
  intros i j E. injection E as E. apply sub_order_inj in E as [_ E]. exact E.
Qed.

Lemma sub_orders_in :
  forall sess parent n o, In o (sub_orders sess parent n) -> exists i, o = (sess, sub_order parent i).
Proof.
  intros sess parent n o Hin. unfold sub_orders in Hin. apply in_map_iff in Hin as (i & <- & _).
  exists i. reflexivity.
Qed.

Lemma same_scope_self : forall s st ps, same_scope (mkTracker (Some s) (Some st) ps) s st = true.
Proof. intros s st ps. unfold same_scope. simpl. rewrite !Nat.eqb_refl. reflexivity. Qed.

Lemma scope_parents_self : forall s st ps, scope_parents (mkTracker (Some s) (Some st) ps) s st = ps.
Proof. intros s st ps. unfold scope_parents. rewrite same_scope_self. reflexivity. Qed.

(* A commit with a parent that is new in its scope commits and emits its
   sub-orders. *)
Lemma commit_gate_fresh :
  forall tr sess store parent n, ~ In parent (scope_parents tr sess store) ->
    commit_gate tr (ReqParent sess store parent n) =
      Some (mkTracker (Some sess) (Some store) (parent :: scope_parents tr sess store),
            sub_orders sess parent n).
Proof.
  intros tr sess store parent n Hn. simpl.
  destruct (in_dec path_eq_dec parent (scope_parents tr sess store)); [contradiction | reflexivity].
Qed.

Lemma scope_parents_other :
  forall tr sess store, (tr_session tr <> Some sess \/ tr_store tr <> Some store) ->
    scope_parents tr sess store = [].
Proof.
  intros [s st ps] sess store H. unfold scope_parents, same_scope. simpl in *.
  destruct s as [s |]; [| reflexivity]. destruct st as [st |]; [| reflexivity].
  destruct (Nat.eqb s sess) eqn:Es; [| reflexivity]. destruct (Nat.eqb st store) eqn:Et; [| reflexivity].
  apply Nat.eqb_eq in Es, Et. subst. destruct H as [H | H]; contradiction.
Qed.

(* Runs within one scope emit pairwise distinct sub-orders, and none of them
   belongs to a parent that committed earlier in the scope. *)
Lemma run_calls_scope :
  forall s st calls tr, Forall (call_in_scope s st) calls ->
    let out := snd (run_calls tr calls) in
    NoDup out /\
    (forall o, In o out -> exists p i, o = (s, sub_order p i) /\ ~ In p (scope_parents tr s st)).
Proof.
  intros s st calls. induction calls as [| c rest IH]; intros tr Hall; simpl.
  - split; [constructor | intros o []].
  - inversion Hall as [| c' rest' Hc Hrest]; subst.
    destruct c as [| [n | sess store parent n]]; simpl in Hc |- *; [contradiction | |].
    + exact (IH tr Hrest).
    + destruct Hc as [-> ->].
      destruct (in_dec path_eq_dec parent (scope_parents tr s st)) as [Hin | Hnin].
      * exact (IH tr Hrest).
      * simpl. set (tr' := mkTracker (Some s) (Some st) (parent :: scope_parents tr s st)).
        destruct (IH tr' Hrest) as (Hnd & Hfrom).
        assert (Etr' : scope_parents tr' s st = parent :: scope_parents tr s st)
          by apply scope_parents_self.
        rewrite Etr' in Hfrom.
        split.
        -- apply NoDup_app; [apply sub_orders_nodup | exact Hnd |].
           intros o Ho1 Ho2. destruct (sub_orders_in _ _ _ _ Ho1) as (i & ->).
           destruct (Hfrom _ Ho2) as (p & j & E & Hp).
           injection E as E. apply sub_order_inj in E as [<- _]. apply Hp. left. reflexivity.
        -- intros o Ho. apply in_app_or in Ho as [Ho | Ho].
           ++ destruct (sub_orders_in _ _ _ _ Ho) as (i & ->). exists parent, i. split; [reflexivity | exact Hnin].
           ++ destruct (Hfrom _ Ho) as (p & j & E & Hp). exists p, j. split; [exact E |].
              intros Hin. apply Hp. right. exact Hin.
Qed.

(* Two evaluations of one deploy (FR-5'). Each root evaluation starts with
   begin_evaluation_scope. The same parent path then commits in both
   evaluations, and each commit emits its sub-orders. A store swap or a
   session change between the two commits has the same effect. Without a
   reset, the second commit under the parent fails with ParentOrderReused
   and emits nothing. *)
Lemma two_evaluations_commit :
  forall tr s st parent n1 n2,
    snd (run_calls tr [CallScope; CallCommit (ReqParent s st parent n1);
                       CallScope; CallCommit (ReqParent s st parent n2)]) =
      sub_orders s parent n1 ++ sub_orders s parent n2 /\
    (forall st', st' <> st ->
       snd (run_calls tr [CallScope; CallCommit (ReqParent s st parent n1);
                          CallCommit (ReqParent s st' parent n2)]) =
         sub_orders s parent n1 ++ sub_orders s parent n2) /\
    (forall s', s' <> s ->
       snd (run_calls tr [CallScope; CallCommit (ReqParent s st parent n1);
                          CallCommit (ReqParent s' st parent n2)]) =
         sub_orders s parent n1 ++ sub_orders s' parent n2) /\
    snd (run_calls tr [CallScope; CallCommit (ReqParent s st parent n1);
                       CallCommit (ReqParent s st parent n2)]) =
      sub_orders s parent n1.
Proof.
  intros tr s st parent n1 n2.
  assert (First : forall n, commit_gate tracker_reset (ReqParent s st parent n) =
                    Some (mkTracker (Some s) (Some st) [parent], sub_orders s parent n)).
  { intros n. reflexivity. }
  split.
  { cbn [run_calls handle_step]. rewrite First. cbn [run_calls handle_step fst snd].
    rewrite First. cbn [fst snd]. rewrite !app_nil_r, app_nil_l. reflexivity. }
  split.
  { intros st' Hst. cbn [run_calls handle_step]. rewrite First. cbn [run_calls handle_step fst snd].
    rewrite (commit_gate_fresh _ s st' parent n2)
      by (rewrite scope_parents_other by (right; simpl; intros E; injection E as E; exact (Hst (eq_sym E)));
          intros []).
    cbn [fst snd]. rewrite !app_nil_r, app_nil_l. reflexivity. }
  split.
  { intros s' Hs. cbn [run_calls handle_step]. rewrite First. cbn [run_calls handle_step fst snd].
    rewrite (commit_gate_fresh _ s' st parent n2)
      by (rewrite scope_parents_other by (left; simpl; intros E; injection E as E; exact (Hs (eq_sym E)));
          intros []).
    cbn [fst snd]. rewrite !app_nil_r, app_nil_l. reflexivity. }
  cbn [run_calls handle_step]. rewrite First. cbn [run_calls handle_step fst snd].
  assert (Dup : commit_gate (mkTracker (Some s) (Some st) [parent]) (ReqParent s st parent n2) = None).
  { simpl. rewrite scope_parents_self. destruct (in_dec path_eq_dec parent [parent]) as [_ | C];
      [reflexivity | exfalso; apply C; left; reflexivity]. }
  rewrite Dup. cbn [run_calls fst snd]. rewrite !app_nil_r, app_nil_l. reflexivity.
Qed.

(* H20 (FR-5 and FR-5'): sub_order is injective, so two parents never share
   a sub-order. A second commit under one parent in one scope fails before
   any effect. A commit with no parent emits nothing. A scope reset
   (begin_evaluation_scope), a store swap and a session change each reset
   the tracker. Every run of calls within one scope emits pairwise distinct
   sub-orders, and none of them belongs to a parent that committed earlier
   in the scope. With H8, the sub-orders of a scope are unique and never
   equal a scheduler order. Two evaluations of one deploy, separated by a
   reset, both commit under the same parent path. *)
Theorem parent_order_single_txn :
  (forall p1 p2 i j, sub_order p1 i = sub_order p2 j -> p1 = p2 /\ i = j) /\
  (forall tr sess store parent n, same_scope tr sess store = true -> In parent (tr_parents tr) ->
     commit_gate tr (ReqParent sess store parent n) = None) /\
  (forall tr n, commit_gate tr (ReqNoParent n) = Some (tr, [])) /\
  (forall tr, handle_step tr CallScope = Some (tracker_reset, [])) /\
  (forall tr sess store parent n, tr_session tr <> Some sess ->
     commit_gate tr (ReqParent sess store parent n) =
       Some (mkTracker (Some sess) (Some store) [parent], sub_orders sess parent n)) /\
  (forall tr sess store parent n, tr_store tr <> Some store ->
     commit_gate tr (ReqParent sess store parent n) =
       Some (mkTracker (Some sess) (Some store) [parent], sub_orders sess parent n)) /\
  (forall sess store parent n,
     commit_gate tracker_reset (ReqParent sess store parent n) =
       Some (mkTracker (Some sess) (Some store) [parent], sub_orders sess parent n)) /\
  (forall s st calls tr, Forall (call_in_scope s st) calls ->
     let out := snd (run_calls tr calls) in
     NoDup out /\
     (forall o, In o out -> exists p i, o = (s, sub_order p i) /\ ~ In p (scope_parents tr s st))) /\
  (forall tr s st parent n1 n2,
     snd (run_calls tr [CallScope; CallCommit (ReqParent s st parent n1);
                        CallScope; CallCommit (ReqParent s st parent n2)]) =
       sub_orders s parent n1 ++ sub_orders s parent n2 /\
     (forall st', st' <> st ->
        snd (run_calls tr [CallScope; CallCommit (ReqParent s st parent n1);
                           CallCommit (ReqParent s st' parent n2)]) =
          sub_orders s parent n1 ++ sub_orders s parent n2) /\
     (forall s', s' <> s ->
        snd (run_calls tr [CallScope; CallCommit (ReqParent s st parent n1);
                           CallCommit (ReqParent s' st parent n2)]) =
          sub_orders s parent n1 ++ sub_orders s' parent n2) /\
     snd (run_calls tr [CallScope; CallCommit (ReqParent s st parent n1);
                        CallCommit (ReqParent s st parent n2)]) =
       sub_orders s parent n1).
Proof.
  split; [exact sub_order_inj |]. split.
  { intros tr sess store parent n Hs Hin. simpl. unfold scope_parents. rewrite Hs.
    destruct (in_dec path_eq_dec parent (tr_parents tr)); [reflexivity | contradiction]. }
  split; [intros tr n; reflexivity |].
  split; [intros tr; reflexivity |].
  split.
  { intros tr sess store parent n Hne. rewrite commit_gate_fresh;
      rewrite scope_parents_other by (left; exact Hne); [reflexivity | intros []]. }
  split.
  { intros tr sess store parent n Hne. rewrite commit_gate_fresh;
      rewrite scope_parents_other by (right; exact Hne); [reflexivity | intros []]. }
  split; [intros sess store parent n; reflexivity |].
  split; [exact run_calls_scope |].
  exact two_evaluations_commit.
Qed.

(* ══════════════ Store identity (H21, FR-2: review D2) ══════════════════════ *)

Section StoreIdentity.

(* Rust channel values, their store identities (stable_hash_provider::hash,
   a Blake2b256Hash of the bincode bytes), the keys that C: Ord compares,
   and the stored values. The history trie keys by the store identity, and
   after FR-2 so does every map and set of the substrate. The hot store
   keys its maps by C: Eq and Hash. For Par these ignore locally_free, as
   bincode does (review par-ord.log), so they agree with the store
   identity. *)
Variables CVal SId OKey B : Type.
Variable sid : CVal -> SId.
Variable sid_eq_dec : forall x y : SId, {x = y} + {x <> y}.
Variable okey : CVal -> OKey.
Variable okey_eq_dec : forall x y : OKey, {x = y} + {x <> y}.

Fixpoint klookup {K : Type} (eqd : forall x y : K, {x = y} + {x <> y}) (k : K) (m : list (K * B))
    : option B :=
  match m with
  | [] => None
  | (k', b) :: t => if eqd k k' then Some b else klookup eqd k t
  end.

(* The store: a map from store identities to values. *)
Definition store_get (st : SId -> B) (c : CVal) : B := st (sid c).
Definition store_put (st : SId -> B) (c : CVal) (b : B) : SId -> B :=
  fun x => if sid_eq_dec x (sid c) then b else st x.
Definition commit_writes (st : SId -> B) (ws : list (CVal * B)) : SId -> B :=
  fold_left (fun st w => store_put st (fst w) (snd w)) ws st.

(* FR-2: the staged view is an overlay keyed by store identity. *)
Definition view_get (ov : list (SId * B)) (st : SId -> B) (c : CVal) : B :=
  match klookup sid_eq_dec (sid c) ov with Some b => b | None => st (sid c) end.
Definition stage_writes (ov : list (SId * B)) (ws : list (CVal * B)) : list (SId * B) :=
  fold_left (fun ov w => (sid (fst w), snd w) :: ov) ws ov.

(* The view before FR-2: an overlay keyed by C: Ord (BTreeMap<C, _>). *)
Definition ord_view_get (ov : list (OKey * B)) (st : SId -> B) (c : CVal) : B :=
  match klookup okey_eq_dec (okey c) ov with Some b => b | None => st (sid c) end.

(* FR-2: the installed-channel guard is keyed by store identity. *)
Definition installed_by_sid (inst : list CVal) (c : CVal) : bool :=
  existsb (fun i => if sid_eq_dec (sid i) (sid c) then true else false) inst.

(* The store that an overlay denotes over a base store. *)
Definition overlay (ov : list (SId * B)) (st : SId -> B) : SId -> B :=
  fun x => match klookup sid_eq_dec x ov with Some b => b | None => st x end.

Lemma commit_writes_ext :
  forall ws st1 st2, (forall x, st1 x = st2 x) -> forall x, commit_writes st1 ws x = commit_writes st2 ws x.
Proof.
  induction ws as [| w ws IH]; intros st1 st2 E x; simpl; [apply E |].
  apply IH. intros y. unfold store_put. destruct (sid_eq_dec y (sid (fst w))); [reflexivity | apply E].
Qed.

Lemma overlay_stage_writes :
  forall ws ov st x, overlay (stage_writes ov ws) st x = commit_writes (overlay ov st) ws x.
Proof.
  induction ws as [| w ws IH]; intros ov st x; simpl; [reflexivity |].
  rewrite IH. apply commit_writes_ext. intros y. unfold overlay, store_put. simpl.
  destruct (sid_eq_dec y (sid (fst w))); reflexivity.
Qed.

(* H21 (FR-2): view lookups respect store identity, and after the same
   writes the staged view agrees with the committed store on every
   channel. The installed-channel guard respects store identity too. A
   view keyed by C: Ord (before FR-2) disagrees with the store for two
   channels that have one store identity but different Ord keys, such as
   two Par values that differ only in locally_free (review D2). *)
Theorem view_identity_is_store_identity :
  (forall ov st c1 c2, sid c1 = sid c2 -> view_get ov st c1 = view_get ov st c2) /\
  (forall ws st c, view_get (stage_writes [] ws) st c = store_get (commit_writes st ws) c) /\
  (forall inst c1 c2, sid c1 = sid c2 -> installed_by_sid inst c1 = installed_by_sid inst c2) /\
  (forall st p q b, sid p = sid q -> okey p <> okey q -> st (sid q) <> b ->
     view_get [(sid p, b)] st q = b /\
     store_get (store_put st p b) q = b /\
     ord_view_get [(okey p, b)] st q <> store_get (store_put st p b) q).
Proof.
  split; [intros ov st c1 c2 E; unfold view_get; rewrite E; reflexivity |].
  split.
  { intros ws st c. unfold view_get, store_get.
    change (match klookup sid_eq_dec (sid c) (stage_writes [] ws) with Some b => b | None => st (sid c) end)
      with (overlay (stage_writes [] ws) st (sid c)).
    rewrite overlay_stage_writes. apply commit_writes_ext. intros x. reflexivity. }
  split; [intros inst c1 c2 E; unfold installed_by_sid; rewrite E; reflexivity |].
  intros st p q b Esid Eok Hst.
  assert (Hput : store_get (store_put st p b) q = b).
  { unfold store_get, store_put. rewrite Esid. destruct (sid_eq_dec (sid q) (sid q)); [reflexivity | contradiction]. }
  split; [unfold view_get; simpl; rewrite Esid; destruct (sid_eq_dec (sid q) (sid q)); [reflexivity | contradiction] |].
  split; [exact Hput |].
  rewrite Hput. unfold ord_view_get. simpl.
  destruct (okey_eq_dec (okey q) (okey p)) as [E | _]; [symmetry in E; contradiction | exact Hst].
Qed.

End StoreIdentity.

(* ══════════ Native replay of space operations (H24-H26, decision 47) ══════════ *)

(* Design v2 of decision 47, section 4.3. Native play stages the operations
   of a space transaction as legacy play does (S2), and the native budget
   records one journal row for each operation. Native replay stages the
   same operations, because the agent runs the same plan over the same view.
   NativeBackend::apply_op (rspace++/src/rspace/replay_rspace/native_session/
   space.rs) then replays each operation against its row:
     1. begin_row: a ticket for the next row, or NotReady with no wait (P1);
     2. authenticate_footprint: the locked footprint equals the row's;
     3. the recorded outcome fits the staged kind;
     4. observe_produce or observe_consume: the introduction, where a
        recorded denial must be a denial (publish_denial);
     5. a fire: check_candidate compares the staged COMM with the recorded
        candidate, then observe_comm observes the COMM;
     6. prepare, publish, and the effects of play (the shared store helpers).
   Native replay runs the contract of play, so one decision function of the
   call history gives the budget decisions of play and replay. *)

Section NativeReplay.

(* The staged operations of the agent, the store, the COMM of a fire and the
   locked footprint of an operation. *)
Variables Op Store Comm Foot : Type.
Variable comm_eq_dec : forall x y : Comm, {x = y} + {x <> y}.
Variable foot_eq_dec : forall x y : Foot, {x = y} + {x <> y}.

(* The COMM of a staged fire, and none for a store or a park. *)
Variable staged_comm : Op -> option Comm.

(* The footprint that the operation locks (authenticate_footprint). *)
Variable foot : Op -> Foot.

(* The effects of an operation on the store: the shared helpers store_data,
   remove_matched_datum_and_join, store_waiting_continuation and
   store_persistent_data, which play and native replay both call. *)
Variable apply : Store -> Op -> Store.

(* An observer call: the introduction of an operation, or the COMM of a
   fire. *)
Inductive NCall := NIntro (o : Op) | NComm (c : Comm).

(* The budget decision of a call after the calls before it. Native replay
   runs the contract of play (check_operation_journal binds the recording),
   so play and replay share this function. *)
Variable grant : list NCall -> NCall -> bool.

(* The outcome that a journal row records, and the row. *)
Inductive NOutcome :=
  | NStored
  | NMatched (c : Comm)
  | NDeniedIntro
  | NDeniedComm (c : Comm).

Record NRow := mkNRow { nr_out : NOutcome; nr_foot : Foot }.

Inductive NStatus := NOk | NOutOfPhlo | NMismatch | NNotReady.

(* Native play of the staged operations of a transaction. Each operation
   observes its introduction, and a fire then observes its COMM. A denial
   publishes its row and fails the transaction with OutOfPhlogistons. The
   operations before it stay applied (D6). Otherwise the effects apply and
   the row records the outcome. *)
Fixpoint native_play (h : list NCall) (s : Store) (ops : list Op)
    : list NCall * Store * list NRow * NStatus :=
  match ops with
  | [] => (h, s, [], NOk)
  | o :: rest =>
      if grant h (NIntro o) then
        match staged_comm o with
        | None =>
            match native_play (h ++ [NIntro o]) (apply s o) rest with
            | (h', s', rows, st) => (h', s', mkNRow NStored (foot o) :: rows, st)
            end
        | Some c =>
            if grant (h ++ [NIntro o]) (NComm c) then
              match native_play ((h ++ [NIntro o]) ++ [NComm c]) (apply s o) rest with
              | (h', s', rows, st) => (h', s', mkNRow (NMatched c) (foot o) :: rows, st)
              end
            else ((h ++ [NIntro o]) ++ [NComm c], s, [mkNRow (NDeniedComm c) (foot o)], NOutOfPhlo)
        end
      else (h ++ [NIntro o], s, [mkNRow NDeniedIntro (foot o)], NOutOfPhlo)
  end.

(* The recorded outcome fits the staged kind (R4 step 7). *)
Definition fits (o : Op) (out : NOutcome) : bool :=
  match staged_comm o, out with
  | None, NStored => true
  | None, NDeniedIntro => true
  | Some _, NMatched _ => true
  | Some _, NDeniedIntro => true
  | Some _, NDeniedComm _ => true
  | _, _ => false
  end.

(* Native replay of the same operations against the journal rows
   (NativeBackend::apply_op). *)
Fixpoint native_replay (h : list NCall) (s : Store) (ops : list Op) (rows : list NRow)
    : list NCall * Store * NStatus :=
  match ops with
  | [] => (h, s, NOk)
  | o :: rest =>
      match rows with
      | [] => (h, s, NNotReady)
      | r :: rows' =>
          if foot_eq_dec (foot o) (nr_foot r) then
            if fits o (nr_out r) then
              match nr_out r with
              | NDeniedIntro =>
                  (h ++ [NIntro o], s, if grant h (NIntro o) then NMismatch else NOutOfPhlo)
              | out =>
                  if grant h (NIntro o) then
                    match staged_comm o, out with
                    | None, NStored => native_replay (h ++ [NIntro o]) (apply s o) rest rows'
                    | Some c, NMatched rc =>
                        if comm_eq_dec c rc then
                          if grant (h ++ [NIntro o]) (NComm c) then
                            native_replay ((h ++ [NIntro o]) ++ [NComm c]) (apply s o) rest rows'
                          else ((h ++ [NIntro o]) ++ [NComm c], s, NMismatch)
                        else (h ++ [NIntro o], s, NMismatch)
                    | Some c, NDeniedComm rc =>
                        if comm_eq_dec c rc then
                          ((h ++ [NIntro o]) ++ [NComm c], s,
                           if grant (h ++ [NIntro o]) (NComm c) then NMismatch else NOutOfPhlo)
                        else (h ++ [NIntro o], s, NMismatch)
                    | _, _ => (h ++ [NIntro o], s, NMismatch)
                    end
                  else (h ++ [NIntro o], s, NMismatch)
              end
            else (h, s, NMismatch)
          else (h, s, NMismatch)
      end
  end.

(* H24: native replay over the rows of native play makes the observer calls
   of play in the order of play, applies the effects of play and ends with
   the status of play. Play records at most one row per operation, and one
   row per operation when it completes. *)
Theorem native_replay_reproduces_play :
  forall ops h s,
    match native_play h s ops with
    | (h', s', rows, st) =>
        native_replay h s ops rows = (h', s', st) /\
        length rows <= length ops /\
        (st = NOk -> length rows = length ops)
    end.
Proof.
  induction ops as [| o rest IH]; intros h s; cbn [native_play].
  - cbn. split; [reflexivity |]. split; [lia | reflexivity].
  - destruct (grant h (NIntro o)) eqn:Gi.
    + destruct (staged_comm o) as [c |] eqn:Sc.
      * destruct (grant (h ++ [NIntro o]) (NComm c)) eqn:Gc.
        -- specialize (IH ((h ++ [NIntro o]) ++ [NComm c]) (apply s o)).
           destruct (native_play ((h ++ [NIntro o]) ++ [NComm c]) (apply s o) rest)
             as [[[h' s'] rows] st].
           destruct IH as [IHr [IHl IHc]].
           split; [| split; [cbn; lia | intros E; cbn; rewrite (IHc E); reflexivity]].
           cbn [native_replay nr_out nr_foot].
           destruct (foot_eq_dec (foot o) (foot o)) as [_ | NE]; [| contradiction NE; reflexivity].
           unfold fits. rewrite Sc, Gi. simpl.
           destruct (comm_eq_dec c c) as [_ | NE]; [| contradiction NE; reflexivity].
           rewrite Gc. exact IHr.
        -- cbn [native_replay nr_out nr_foot].
           split; [| split; [cbn; lia | discriminate]].
           destruct (foot_eq_dec (foot o) (foot o)) as [_ | NE]; [| contradiction NE; reflexivity].
           unfold fits. rewrite Sc, Gi. simpl.
           destruct (comm_eq_dec c c) as [_ | NE]; [| contradiction NE; reflexivity].
           rewrite Gc. reflexivity.
      * specialize (IH (h ++ [NIntro o]) (apply s o)).
        destruct (native_play (h ++ [NIntro o]) (apply s o) rest) as [[[h' s'] rows] st].
        destruct IH as [IHr [IHl IHc]].
        split; [| split; [cbn; lia | intros E; cbn; rewrite (IHc E); reflexivity]].
        cbn [native_replay nr_out nr_foot].
        destruct (foot_eq_dec (foot o) (foot o)) as [_ | NE]; [| contradiction NE; reflexivity].
        unfold fits. rewrite Sc, Gi. simpl. exact IHr.
    + cbn [native_replay nr_out nr_foot].
      split; [| split; [cbn; lia | discriminate]].
      destruct (foot_eq_dec (foot o) (foot o)) as [_ | NE]; [| contradiction NE; reflexivity].
      unfold fits. destruct (staged_comm o); rewrite Gi; reflexivity.
Qed.

(* H25: when play runs out of phlo at operation k, play keeps the effects of
   the operations before k and records k + 1 rows, and native replay over
   those rows ends with the same calls, the same store and OutOfPhlogistons. *)
Theorem native_replay_denial_keeps_prefix :
  forall ops h s,
    match native_play h s ops with
    | (h', s', rows, st) =>
        st = NOutOfPhlo ->
        exists k,
          k < length ops /\ length rows = S k /\
          s' = fold_left apply (firstn k ops) s /\
          native_replay h s ops rows = (h', s', NOutOfPhlo)
    end.
Proof.
  induction ops as [| o rest IH]; intros h s.
  - cbn. discriminate.
  - pose proof (native_replay_reproduces_play (o :: rest) h s) as Hrep.
    cbn [native_play] in Hrep |- *.
    destruct (grant h (NIntro o)) eqn:Gi.
    + destruct (staged_comm o) as [c |] eqn:Sc.
      * destruct (grant (h ++ [NIntro o]) (NComm c)) eqn:Gc.
        -- specialize (IH ((h ++ [NIntro o]) ++ [NComm c]) (apply s o)).
           destruct (native_play ((h ++ [NIntro o]) ++ [NComm c]) (apply s o) rest)
             as [[[h' s'] rows] st].
           intros E. destruct (IH E) as [k [Hk [Hl [Hs _]]]].
           destruct Hrep as [Hr _]. rewrite E in Hr.
           exists (S k). split; [cbn; lia |]. split; [cbn; lia |].
           split; [exact Hs | exact Hr].
        -- intros _. destruct Hrep as [Hr _].
           exists 0. split; [cbn; lia |]. split; [reflexivity |].
           split; [reflexivity | exact Hr].
      * specialize (IH (h ++ [NIntro o]) (apply s o)).
        destruct (native_play (h ++ [NIntro o]) (apply s o) rest) as [[[h' s'] rows] st].
        intros E. destruct (IH E) as [k [Hk [Hl [Hs _]]]].
        destruct Hrep as [Hr _]. rewrite E in Hr.
        exists (S k). split; [cbn; lia |]. split; [cbn; lia |].
        split; [exact Hs | exact Hr].
    + intros _. destruct Hrep as [Hr _].
      exists 0. split; [cbn; lia |]. split; [reflexivity |].
      split; [reflexivity | exact Hr].
Qed.

(* H26: native replay fails closed before any effect. With no ready row, an
   operation ends NotReady with no call (P1: a space transaction never
   waits). A forged footprint and an outcome that does not fit the staged
   kind end Mismatch before any call. A fire whose recorded candidate is
   another COMM ends Mismatch after its introduction only: the COMM is
   not observed, and the store keeps no effect of the operation. *)
Theorem native_replay_fails_closed :
  forall h s o rest r rows,
    native_replay h s (o :: rest) [] = (h, s, NNotReady) /\
    (foot o <> nr_foot r ->
     native_replay h s (o :: rest) (r :: rows) = (h, s, NMismatch)) /\
    (foot o = nr_foot r -> fits o (nr_out r) = false ->
     native_replay h s (o :: rest) (r :: rows) = (h, s, NMismatch)) /\
    (forall c rc,
       foot o = nr_foot r -> staged_comm o = Some c -> nr_out r = NMatched rc -> c <> rc ->
       grant h (NIntro o) = true ->
       native_replay h s (o :: rest) (r :: rows) = (h ++ [NIntro o], s, NMismatch)).
Proof.
  intros h s o rest r rows.
  split; [reflexivity |].
  split.
  { intros NE. cbn [native_replay].
    destruct (foot_eq_dec (foot o) (nr_foot r)) as [E | _]; [contradiction | reflexivity]. }
  split.
  { intros E F. cbn [native_replay].
    destruct (foot_eq_dec (foot o) (nr_foot r)) as [_ | NE]; [| contradiction].
    rewrite F. reflexivity. }
  intros c rc E Sc Out NE Gi. cbn [native_replay].
  destruct (foot_eq_dec (foot o) (nr_foot r)) as [_ | NE']; [| contradiction].
  rewrite Out. unfold fits. rewrite Sc, Gi. simpl.
  destruct (comm_eq_dec c rc) as [Ec | _]; [contradiction | reflexivity].
Qed.

End NativeReplay.

(* ═════════ The charge of a view load (P5 of decision 47, I-12) ═════════ *)

Section ViewCharges.

(* The keys of view loads (a channel, a channel group, or the joins of a
   channel) and their values (the loaded records). *)
Variables Key Val : Type.

(* NativeViewReads::key: native_backing::inspect and native_source::hash. *)
Variable key_charge : Key -> nat.

(* NativeViewReads::data, continuations and joins: the copy reservation of
   the loaded records (reserve_slice_copy_and_cleanup). *)
Variable value_charge : Val -> nat.

(* The meter of the native history reader of a cold load: accept_all. *)
Definition accept_all (_ : Key) : nat := 0.

(* A view load: the hot store serves a cached value. On a miss, the native
   history reader reads the value at the root of the transaction through
   accept_all, and the hot store caches it. The load charges its key and
   its value. *)
Definition view_load (cache : Key -> option Val) (history : Key -> Val) (k : Key) : Val * nat :=
  match cache k with
  | Some v => (v, key_charge k + value_charge v)
  | None => (history k, key_charge k + accept_all k + value_charge (history k))
  end.

(* A coherent cache holds the value of the history at the root of the
   transaction, or nothing. *)
Definition coherent (cache : Key -> option Val) (history : Key -> Val) : Prop :=
  forall k v, cache k = Some v -> v = history k.

(* P5: the charge of a view load is a function of its key and its value
   only: a hit and a miss load the same value and charge the same amount,
   so a warm load and a cold load reserve the same work. *)
Theorem view_charge_cache_independent :
  forall cache history k,
    coherent cache history ->
    view_load cache history k = (history k, key_charge k + value_charge (history k)).
Proof.
  intros cache history k Hc. unfold view_load.
  destruct (cache k) as [v |] eqn:Hk.
  - rewrite (Hc k v Hk). reflexivity.
  - unfold accept_all. rewrite Nat.add_0_r. reflexivity.
Qed.

End ViewCharges.

(* ═══════════ The premises of H9 are satisfiable ══════════════════════════════ *)

(* History keys that are the cache keys themselves satisfy key_injective for
   every channel type and order. The history that reads every key of a
   store back gives a well-formed checkpoint state with an empty cache. So
   the checkpoint conjuncts of H9 do not hold vacuously. *)
Lemma abort_premises_satisfiable :
  forall (Chan Val Pat Kont PId CId Leaf : Type) (chan_leb : Chan -> Chan -> bool)
    (encode_data : list (Datum Val PId) -> Leaf) (encode_conts : list (WK Pat Kont CId) -> Leaf)
    (encode_joins : list (list Chan) -> Leaf) (S : Store Chan Val Pat Kont PId CId),
    key_injective Chan chan_leb (CKey Chan) (fun k => k) /\
    ck_wf Chan Val Pat Kont PId CId chan_leb (CKey Chan) (fun k => k) Leaf
      encode_data encode_conts encode_joins
      (leaf_at Chan Val Pat Kont PId CId Leaf encode_data encode_conts encode_joins S) S [].
Proof.
  intros Chan Val Pat Kont PId CId Leaf chan_leb encode_data encode_conts encode_joins S.
  split; [intros k1 k2 _ _ E; exact E |].
  split; [constructor |]. split; [constructor |].
  intros k _. reflexivity.
Qed.

(* The order premises of the uniqueness conjuncts of H17 hold for the order
   of the natural numbers, so those conjuncts do not hold vacuously. *)
Lemma order_premises_satisfiable :
  leb_transitive nat Nat.leb /\ leb_antisymmetric nat Nat.leb /\ leb_total nat Nat.leb.
Proof.
  split; [intros x y z Hxy Hyz; apply Nat.leb_le in Hxy, Hyz; apply Nat.leb_le; lia |].
  split; [intros x y Hxy Hyx; apply Nat.leb_le in Hxy, Hyx; lia |].
  intros x y. destruct (Nat.le_ge_cases x y) as [H | H]; [left | right]; apply Nat.leb_le, H.
Qed.

(* The order premises of H17 are needed. Without antisymmetry, two orders of
   one group both pass the check: under the comparison that holds for every
   pair, [true; false] and [false; true] are both non-decreasing. Without
   totality, the insertion sort can give a group that fails the check. *)
Lemma order_premises_needed :
  (group_check bool (fun _ _ => true) [true; false] = None /\
   group_check bool (fun _ _ => true) [false; true] = None /\
   Permutation [true; false] [false; true] /\ [true; false] <> [false; true]) /\
  group_check bool (fun _ _ => false) (isort (fun _ _ => false) [true; false]) = Some NonCanonicalChannelOrder.
Proof.
  split; [| reflexivity].
  split; [reflexivity |]. split; [reflexivity |]. split; [apply perm_swap | discriminate].
Qed.

(* The premises of H23 are satisfiable, so H23 does not hold vacuously. The
   observer of a phlo limit of B charges accepts a call while fewer than B
   calls came before it, and it does not count the directive queries. So
   its decisions ignore them. The observer that gives no directive gives
   DirNone. *)
Lemma replay_premises_satisfiable :
  forall (Chan Val Pat Kont PId CId : Type) (B : nat),
    let phlo := fun (h : list (Obs Chan Val Pat Kont PId CId)) (_ : Obs Chan Val Pat Kont PId CId) =>
                  length (erase_dir Chan Val Pat Kont PId CId h) <? B in
    ob_ignores_directive Chan Val Pat Kont PId CId phlo /\
    dir_always_none Chan Val Pat Kont PId CId (fun _ _ => DirNone).
Proof.
  intros Chan Val Pat Kont PId CId B phlo. split.
  - intros h o. subst phlo. cbv beta. rewrite erase_dir_idem. reflexivity.
  - intros h s. reflexivity.
Qed.

(* The premise of H25 is satisfiable: a budget that denies every call makes
   play run out of phlo at its first operation. *)
Lemma native_denial_premise_satisfiable :
  forall (Op Store Comm Foot : Type) (staged_comm : Op -> option Comm) (foot : Op -> Foot)
    (apply : Store -> Op -> Store) (o : Op) (s : Store),
    match native_play Op Store Comm Foot staged_comm foot apply (fun _ _ => false) [] s [o] with
    | (_, _, _, st) => st = NOutOfPhlo
    end.
Proof. intros. reflexivity. Qed.

(* ═══════════ Headline trust base: every headline is axiom-free ═══════════ *)

Print Assumptions commit_trace_eq_host.
Print Assumptions commit_state_eq_host.
Print Assumptions counter_timing_play_replay.
Print Assumptions verify_sound_complete.
Print Assumptions replay_pairs_each_comm_once.
Print Assumptions view_commit_coherent.
Print Assumptions joins_lockstep_preserved.
Print Assumptions sub_order_between.
Print Assumptions abort_inert.
Print Assumptions coverage_total.
(* Disabled by decision 45 (Rule E): H11 is peek_reproduces_trace_eq_host.
Print Assumptions peek_retention_trace_eq_host.
*)
Print Assumptions peek_reproduces_trace_eq_host.
Print Assumptions reporting_trace_is_replay_trace.
Print Assumptions non_canonical_fails_closed.
Print Assumptions directive_fails_closed.
Print Assumptions parent_order_single_txn.
Print Assumptions view_identity_is_store_identity.
Print Assumptions replay_reproduces_play_run.
Print Assumptions native_replay_reproduces_play.
Print Assumptions native_replay_denial_keeps_prefix.
Print Assumptions native_replay_fails_closed.
Print Assumptions view_charge_cache_independent.
