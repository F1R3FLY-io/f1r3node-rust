//! Added by decision 102 (plan v4 section 10.5, plan-join-batch Step 7): the native owner-row
//! suite. Each test plays Rholang deploys natively at the v6 limits of the offered funded
//! protocol (`support::native`), on the racing runtimes with 0, 2 and 8 workers, and certifies
//! each play through its own native replay (rule CR-28). The tests cover these properties:
//!
//! * `own_reoffer_of_a_stored_persistent_record_stages_its_anchor_first` (T-C4-23, F22, rule
//!   CR-19 case 3, plan v4 C7.22): a VectorDB settle job whose first operation re-offers a
//!   stored persistent record stages its anchor first, because every re-offer of the record
//!   shares its source, so the row of the re-offer is not unique to the job.
//! * `set_and_vectordb_cascade_finds_the_lift_in_dr75_order` (T-C4-19, added by decision 124,
//!   L1): the cascade of a set, of a VectorDB space and of a PathMap set finds the lift of each
//!   fire in DR-75 order, with no look-ahead, at Following and at an empty transaction, with a
//!   persistent lead and with a shared lead (cases (a) to (e) of plan-join-batch Step 7).
//! * `set_cascade_window_is_dr75_for_a_guarded_waiter` (F3, added by decision 124, L1): the
//!   cascade of a set and of a PathMap set checks each guarded waiter on the canonical tuple of
//!   its window, so a waiter whose guard refuses the first record in DR-75 order does not fire
//!   first, even when it comes first in the DR-75 order of the waiters.
//! * `cascade_r_a_canonical_restage_replays` (rule CR-14, added by decision 124, L1): a send whose
//!   random pick leads to `CapacityExceeded` on a full VectorDB collection restages in DR-75
//!   order (R-A), commits with the cascade of the restage, and replays. The random pick lies
//!   outside the cascade.
//! * `stored_send_row_skips_a_vetoing_join_waiter` (T-C4-40, E3-a H2, decision 124 L3): on a
//!   Stored send row, replay skips each waiter whose commit may veto, one-bind waiters and list
//!   waiters included, on a bag, a set and a VectorDB space, without and with owner funding.
//! * `cascade_anchor_alone_failure_replays_in_dr75` (V4-12, added by plan v4 errata and decision
//!   124, L1): a VectorDB settle job whose staging fails with `CapacityExceeded` after its leading
//!   anchor commits the anchor alone, and native replay meets the error at Free, after a miss.
//! * `owner_funded_client_deploys_fire_and_certify_on_each_kind` (T-P-1, added by decision 124,
//!   ruling OP-TP1-WORKLOAD): on a stack, a queue, a cell, a priority queue, a bag, a set and a
//!   VectorDB space in single mode and in list mode, the owner stores records and parks waiters,
//!   and the client fires them with owner funding, outside the join engine. Each client deploy
//!   fires a fixed count of bodies and certifies through its own native replay. The test is the
//!   workload of the measurement of T-P-1.
//! * `vectordb_refused_cascade_lift_books_its_pair`,
//!   `join_refused_list_cascade_lift_books_its_pair` and
//!   `join_refused_pair_cascade_lift_books_its_pair` (rules CR-16 and CR-20, added by decision
//!   124, ruling RADD3 1.1): with owner funding, a cascade `K_lift` of a record of an owner with
//!   budget 0 fails its item test, parks inert, and books its pair, so no later cascade of the
//!   deploy lifts the record for the waiter again. The cases: the VectorDB cascade, and the list
//!   cascade and the pair cascade of the join engine.
//! * `cell_owner_refusal_reissue_on_a_full_slot_commits_the_anchor_alone` (ADD A4, rule CR-32,
//!   errata V4-12, added by decision 124): the miss rule of T-C4-27 on a cell. A Stop re-issue
//!   with `OwnerRefusal` stages its anchor first, fails to store in the full slot with
//!   `CellOccupied`, and commits its anchor alone. Native replay misses at the take of the
//!   counter, meets the same error, and commits the anchor alone too.
//! * `stop_reissue_stages_its_anchor_first` (T-C4-22, rule CR-19 case 2, added by decision 124,
//!   RIBA 1.1): a Stop re-issue with `OwnerRefusal` stages its anchor first, and a `RoundCap`
//!   re-issue of an ordinary operation stages no anchor first.
//! * `staging_error_after_the_followed_rows_ends_as_record_end` (T-C4-27, rule CR-32, errata
//!   V4-12, added by decision 124): the error rule. On a VectorDB space, which has no counter, a
//!   Stop re-issue whose first step fails commits its anchor alone, and native replay follows
//!   the rows of play up to row d and ends its staging as a record end.
//! * `space_r_a_restages_the_anchor_first_of_a_stop_reissue` (F6, rules CR-14 item 7 and CR-19
//!   case 2, errata V4-12, added by decision 124): R-A restages a Stop re-issue in DR-75 order
//!   with its anchor first again. In the variant of V4-12 the restage fails, and the deploy
//!   commits the anchor alone.
//! * `return_tail_divergent_pair_reissue_stages_no_anchor_first` (D-6, C2v3 8.7, added by
//!   decision 124, RIBA 1.1): a Return of Tail(actual) whose value is a `DivergentPair` re-issue
//!   stages no anchor first.
//! * `stop_after_a_guarded_two_bind_continuation_replays` (T-C4-35, errata V4-18, rule CR-31,
//!   added by decision 124): a Stop after a deviating row whose refused candidate is a guarded
//!   continuation of two binds replays, because the finish books the item pairs of such a row on
//!   the Stop branch only.
//! * `mark_free_finish_equals_replay_for_each_discipline` (T-C4-21, rules CR-28 and CR-31, added
//!   by decision 124, rulings RADD2v2 7.1 and 7.2): a deviating row ends Tail(actual) in play as
//!   native replay ends it, for each kind and each case of ADD A2.9 that a chain reaches.
//! * `vdb_search_is_none_without_owner_funding`,
//!   `vdb_receive_selection_hands_out_its_pick_order` and
//!   `vdb_builder_skips_an_inert_machinery_waiter` (rules CR-16 and CR-28, added by decision 124,
//!   RIBA 2.4): with owner funding, each VectorDB site stages a row search, a single-mode receive
//!   whose best record the commit refuses binds the next entry of the score order, and the
//!   search of an offer skips an inert machinery waiter.
//! * `scan_search_is_none_without_owner_funding`, `scan_offer_search_counts_the_staged_retires`,
//!   `scan_receive_search_counts_the_staged_retires` and
//!   `scan_builder_skips_an_inert_machinery_waiter` (rules CR-16, CR-28 and CR-33, added by
//!   decision 124, RIBA 3.4): with owner funding, each SCAN site stages a row search, a refused
//!   fire skips the retire rows that its staging put after it, and the search of an offer skips
//!   an inert machinery waiter.
//! * `head_search_is_none_without_owner_funding` (rule CR-28, added by decision 124, RIBA 4.3):
//!   with owner funding, the offer and the receive of the head of a queue stage the head search,
//!   so a refused head takes the E3-a outcome in play and in native replay.
//! * `join_refused_cascade_lift_after_a_divergent_pair_replays` and
//!   `join_refused_lift_before_the_plan_keeps_the_operation` (rules CR-16 and CR-19 case 2, added
//!   by decision 124, ruling RADD2v2 6.4): with owner funding, a cascade `K_lift` of the join
//!   engine that the commit refuses ends the operation with the stop of the engine at the lift.
//!   After a divergent pair, play and native replay re-issue the same settle job. In the settle
//!   before the plan, the plan after the lift runs again, so its store is kept.
//! * `pathmap_refused_fire_commits_its_finish`, `pathmap_refused_cascade_lift_replays` and
//!   `pathmap_followed_bump_replays` (rules CR-18, CR-31 and CR-33, added by decision 124, ruling
//!   RADD2v2 6.6 (f)): a cut of a PathMap space commits the finish of its state after its group,
//!   a refused cascade lift of a PathMap priority queue ends with the stop of the engine at the
//!   lift, and native replay follows the finish rows of the record.
//! * `pathmap_record_end_after_a_followed_restore_replays` (rules CR-18 and CR-33, added by
//!   decision 124, rulings RADD2v2 6.6 (f) and RADD2v4 6.2): after a record end of native replay,
//!   the last row with a search is the cut row when its later rows follow its finish rows, so
//!   replay re-issues the rest of a send on a PathMap bag as play does.
//! * `pathmap_refused_send_stores_its_element` (rules CR-31 and CR-33, added by decision 124,
//!   ruling RADD2 1.2): a PathMap send that the commit refuses stores its element through the
//!   store unit of its row, and every run certifies through its own native replay.

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{CostSignature, Expr, ListParWithRandom, Par};
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::accounting::authority::{
    cost_signature_to_sig, sig_to_cost_signature,
};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::owner_funding::{
    OwnerFundingBinding, OwnerPurse, OwnerPurseSet, OwnerTable, OwnerTableEntry,
};
use rholang::rust::interpreter::accounting::{
    NativeBudgetRecording, NativeOperationRecord, NativeOperationSource, NativeProduceSource,
    NativeRuntimeConfig, RuntimeBudget, Sig,
};
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::io::handle_table::FileHandleTable;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
use rholang::rust::interpreter::reified::costs::FIRE_ROUND_CAP;
use rholang::rust::interpreter::rho_runtime::{
    create_native_replay_env_with_logger, RhoRuntime, RhoRuntimeImpl,
};
use rholang::rust::interpreter::spaces::namespace::{self, Durability, ReifiedId};
use rholang::rust::interpreter::spaces::record;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use support::family_programs::space_program;
use support::native::{
    assert_reproduces, certify, committed_evidence, config, finish_injected, footprint_channels,
    fresh_space_runtime_with, hot_store_residue, is_space_row, journal_limits, snapshot_keys,
    trace_bind_limits, with_contract, Certification, NativeChain, PlaySpace, Played, AMPLE_LIMIT,
    DEPLOY, OUT,
};
use support::props::{racing_runtime, RACING_WORKERS};
use support::session::Injections;
use tokio::sync::RwLock;

// ─── Harness ───────────────────────────────────────────────────────────────────────────────────

/// Added by decision 102 (plan v4 T-C4-23, F22): the rows of the journal of `played`, with its
/// recording, as `journal_of` of `reified_native_spaces.rs` reads them.
fn journal_of(played: &Played) -> (&NativeBudgetRecording, &[NativeOperationRecord]) {
    let recording = played
        .result
        .native_budget_recording
        .as_ref()
        .expect("native play records its budget");
    let journal = played
        .result
        .native_operation_recording
        .as_ref()
        .expect("native play records its operations");
    (recording, journal.as_ref())
}

/// Added by decision 102 (plan v4 T-C4-23, F22): a deploy that creates a space of the family
/// `urn` with the constructor `ctor`, stores the handle and the names `a` and `b` on the public
/// channel `stash`, and runs `state`, as `stashing` of `reified_native_spaces.rs` does.
fn stashing(urn: &str, ctor: &str, stash: &str, state: &str) -> String {
    space_program(
        urn,
        ctor,
        &format!(r#"@"{stash}"!(*space, *a, *b) | {state}"#),
    )
}

/// Added by decision 102 (plan v4 T-C4-23, F22): a deploy that runs `body` over the handle
/// `space` and the names `a` and `b` on the public channel `stash`, as `with_stash` of
/// `reified_native_spaces.rs` does. The values on `out` go to the public channel [`OUT`].
fn with_stash(stash: &str, body: &str) -> String {
    format!(
        r#"new out in {{
  for (space, a, b <<- @"{stash}") {{
    {body}
  }} |
  for (@value <= out) {{ @"{OUT}"!(value) }}
}}"#
    )
}

/// Added by decision 102 (plan v4 T-C4-23, F22): the number of space rows of `journal` with a
/// COMM.
fn space_comms(journal: &[NativeOperationRecord]) -> usize {
    journal
        .iter()
        .filter(|row| is_space_row(row) && row.comm.is_some())
        .count()
}

/// Added by decision 124 (batch B, step 6): the number of space rows of `journal` with a COMM
/// that fire a waiter: every space COMM but the take of a counter, which each operation on an
/// ordered collection stages for each of its channels, a park included (`spaces/counter.rs`).
fn waiter_comms(journal: &[NativeOperationRecord]) -> usize {
    let mut comms = 0_usize;
    for row in journal {
        match &row.comm {
            Some(_) if is_space_row(row) && !is_counter_take(row) => comms += 1,
            Some(_) | None => {}
        }
    }
    comms
}

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): the number of space produce
/// rows of `journal` without a COMM: the records that the journal stores.
fn space_stores(journal: &[NativeOperationRecord]) -> usize {
    let mut stores = 0_usize;
    for row in journal {
        match (&row.source, &row.comm) {
            (NativeOperationSource::Produce(_), None) if is_space_row(row) => stores += 1,
            (NativeOperationSource::Produce(_), _) | (NativeOperationSource::Consume(_), _) => {}
        }
    }
    stores
}

/// Added by decision 102 (plan v4 T-C4-23, F22): the kinds of the channels of the footprint of
/// `row`, sorted: `C` for a config channel, `S` for a space counter, and `?` for every other
/// channel, as `footprint_kinds` of `reified_native_spaces.rs` reads them.
fn footprint_kinds(row: &NativeOperationRecord) -> Vec<&'static str> {
    let channels = footprint_channels(row);
    let mut kinds = Vec::<&'static str>::with_capacity(channels.len());
    for channel in &channels {
        kinds.push(match namespace::reified_id_of(channel) {
            Some(ReifiedId::Config(_)) => "C",
            Some(ReifiedId::SpaceCounter(_)) => "S",
            _ => "?",
        });
    }
    kinds.sort_unstable();
    kinds
}

/// Added by decision 102 (plan v4 T-C4-23, F22): the journal indices of the anchor rows of
/// `journal` (decision 92): the consumes without a peek whose footprint names `C(sid)` and
/// `S(sid)` only, as `anchors_of` of `reified_native_spaces.rs` finds them.
fn anchor_indices(journal: &[NativeOperationRecord]) -> Vec<usize> {
    let mut anchors = Vec::<usize>::new();
    for (index, row) in journal.iter().enumerate() {
        match (&row.source, row.consume_peeks.as_deref()) {
            (NativeOperationSource::Consume(_), None | Some([]))
                if footprint_kinds(row) == ["C", "S"] =>
            {
                anchors.push(index)
            }
            _ => {}
        }
    }
    anchors
}

/// Added by decision 102 (plan v4 T-C4-23, F22): checks that the deploy `played` parks its
/// waiters: it fails nothing, delivers nothing, and fires no space operation.
fn assert_parks(label: &str, played: &Played) {
    assert!(
        played.result.errors.is_empty(),
        "{label}: {:?}",
        played.result.errors
    );
    assert!(
        played.outputs.is_empty(),
        "{label}: a waiter delivers: {:?}",
        played.outputs
    );
    let (_, journal) = journal_of(played);
    assert_eq!(waiter_comms(journal), 0, "{label}: a waiter fires");
}

/// Added by decision 102 (plan v4 T-C4-23, F22): plays the deploy `records`, which stores the
/// records of a chain, then the deploy `waiters`, which parks its waiters, on `chain`. Each
/// play certifies through its own native replay.
async fn store_and_park(chain: &mut NativeChain, label: &str, records: &str, waiters: &str) {
    let stored = chain
        .play(records, &format!("{label}: the records"), AMPLE_LIMIT)
        .await;
    assert!(
        stored.result.errors.is_empty(),
        "{label}: the records: {:?}",
        stored.result.errors
    );
    let certification = certify(&stored, records, AMPLE_LIMIT).await;
    assert_reproduces(&format!("{label}, the records"), &stored, &certification);
    let parked = chain
        .play(waiters, &format!("{label}: the waiters"), AMPLE_LIMIT)
        .await;
    assert_parks(&format!("{label}, the waiters"), &parked);
    let certification = certify(&parked, waiters, AMPLE_LIMIT).await;
    assert_reproduces(&format!("{label}, the waiters"), &parked, &certification);
}

/// Added by decision 124 (L1, plan-join-batch Step 7): the runs of a chain whose play picks at
/// random (PLAN Step 7 item 3). An outcome with a chance of `p` or more in each run is missing
/// from every run with a chance of `(1 - p)^64` or less.
const PICK_RUNS: usize = 64;

/// Added by decision 124 (L1, plan-join-batch Step 7): the salts that a search for the label of
/// a deploy tries, in a fixed order. The hashes of a deploy follow from its label and its term
/// alone, so every run of a test finds the same salt.
const SALTS: usize = 64;

/// Added by decision 124 (L1, plan-join-batch Step 7): plays `term` on `chain` with the random
/// state of `label`, and certifies the play through its own native replay (rule CR-28). A
/// failed deploy certifies too: replay must fail it with the errors of play.
async fn play_certified(chain: &mut NativeChain, term: &str, label: &str) -> Played {
    let played = chain.play(term, label, AMPLE_LIMIT).await;
    let certification = certify(&played, term, AMPLE_LIMIT).await;
    assert_reproduces(label, &played, &certification);
    played
}

/// Added by decision 124 (L1, plan-join-batch Step 7): checks that the deploy `played` runs to
/// its end.
fn assert_runs(label: &str, played: &Played) {
    assert!(
        played.result.errors.is_empty(),
        "{label}: {:?}",
        played.result.errors
    );
}

/// Added by decision 124 (L1, plan-join-batch Step 7): the source hashes of the waiters that the
/// deploy `played` parks: its space consume rows without a COMM.
fn parked_waiters(played: &Played) -> BTreeSet<[u8; 32]> {
    let (_, journal) = journal_of(played);
    let mut waiters = BTreeSet::<[u8; 32]>::new();
    for row in journal {
        match (&row.source, &row.comm) {
            (NativeOperationSource::Consume(waiter), None) if is_space_row(row) => {
                waiters.insert(waiter.hash);
            }
            (NativeOperationSource::Consume(_), _) | (NativeOperationSource::Produce(_), _) => {}
        }
    }
    waiters
}

/// Added by decision 124 (L1, plan-join-batch Step 7): a fire of a stored waiter in a journal:
/// the space produce row that offers a record again, with the COMM that names the waiter.
struct WaiterFire {
    /// The journal index of the row.
    index: usize,
    /// The source hash of the waiter that the COMM names.
    waiter: [u8; 32],
    /// The source of the row: the offered record (HashMap), or its vehicle (PathMap).
    record: NativeProduceSource,
    /// The sources that the COMM binds, in the order of its group.
    produces: Arc<[NativeProduceSource]>,
}

/// Added by decision 124 (L1, plan-join-batch Step 7): the fires of the waiters `waiters` in
/// `journal`, in journal order: the space produce rows whose COMM names one of them.
fn waiter_fires(
    journal: &[NativeOperationRecord],
    waiters: &BTreeSet<[u8; 32]>,
) -> Vec<WaiterFire> {
    let mut fires = Vec::<WaiterFire>::with_capacity(waiters.len());
    for (index, row) in journal.iter().enumerate() {
        match (&row.source, &row.comm) {
            (NativeOperationSource::Produce(record), Some(comm))
                if is_space_row(row) && waiters.contains(&comm.source.consume.hash) =>
            {
                fires.push(WaiterFire {
                    index,
                    waiter: comm.source.consume.hash,
                    record: record.clone(),
                    produces: comm.source.produces.clone(),
                })
            }
            _ => {}
        }
    }
    fires
}

/// Added by decision 124 (L1, plan-join-batch Step 7): the journal index of the last space row
/// of `journal` before the row `index`.
fn space_row_before(journal: &[NativeOperationRecord], index: usize) -> Option<usize> {
    journal[..index].iter().rposition(is_space_row)
}

/// Added by decision 124 (L1, plan-join-batch Step 7): the record that `row` lifts when `row` is
/// a `K_lift` or a `K_retire`: a consume whose COMM binds one record alone.
fn lifted_by(row: &NativeOperationRecord) -> Option<&NativeProduceSource> {
    match (&row.source, &row.comm) {
        (NativeOperationSource::Consume(_), Some(comm)) => match comm.source.produces.as_ref() {
            [lifted] => Some(lifted),
            _ => None,
        },
        (NativeOperationSource::Consume(_), None) | (NativeOperationSource::Produce(_), _) => None,
    }
}

/// Added by decision 124 (L1, plan-join-batch Step 7): checks that the fires `fires` take their
/// waiters in the canonical order of DR-75: their source hashes ascend. This holds when each
/// waiter that the cascade meets first passes its guard on the canonical tuple.
fn assert_ascending_waiters(label: &str, fires: &[WaiterFire]) {
    for (index, pair) in fires.windows(2).enumerate() {
        assert!(
            pair[0].waiter < pair[1].waiter,
            "{label}: fire {index} and fire {} take their waiters in DR-75 order",
            index + 1
        );
    }
}

/// Added by decision 124 (L1, plan-join-batch Step 7): checks that each fire of `fires` whose
/// record is not persistent follows the `K_lift` of that record (LIFT-AND-REOFFER): the space
/// row before the fire lifts the record. A persistent record is never lifted.
fn assert_lifted(label: &str, journal: &[NativeOperationRecord], fires: &[WaiterFire]) {
    for (index, fire) in fires.iter().enumerate() {
        match fire.record.persistent {
            true => {}
            false => {
                let before = space_row_before(journal, fire.index)
                    .unwrap_or_else(|| panic!("{label}: a space row precedes fire {index}"));
                assert_eq!(
                    lifted_by(&journal[before]),
                    Some(&fire.record),
                    "{label}: the K_lift of its record precedes fire {index}"
                );
            }
        }
    }
}

/// Added by decision 124 (L1, plan-join-batch Step 7): checks that the fires `fires` bind the
/// records of each channel in the canonical order of DR-75: on each channel, the source hash of
/// the record that a fire binds exceeds the source hash that an earlier fire bound there.
fn assert_ascending_records(label: &str, fires: &[WaiterFire]) {
    let mut last = BTreeMap::<[u8; 32], [u8; 32]>::new();
    for (index, fire) in fires.iter().enumerate() {
        for bound in fire.produces.iter() {
            if let Some(earlier) = last.insert(bound.channel, bound.hash) {
                assert!(
                    earlier < bound.hash,
                    "{label}: fire {index} binds a record after the record of an earlier fire on \
                     its channel, in DR-75 order"
                );
            }
        }
    }
}

/// Added by decision 124 (L1, plan-join-batch Step 7): the data channel on which the deploy
/// `played` stores `count` records: its space produce rows without a COMM. Each chain of this
/// suite stores its records in a deploy of their own, so these rows are its records alone.
fn stored_channel(played: &Played, count: usize) -> [u8; 32] {
    let (_, journal) = journal_of(played);
    let mut held = BTreeMap::<[u8; 32], usize>::new();
    for row in journal {
        match (&row.source, &row.comm) {
            (NativeOperationSource::Produce(record), None) if is_space_row(row) => {
                *held.entry(record.channel).or_insert(0) += 1;
            }
            (NativeOperationSource::Produce(_), _) | (NativeOperationSource::Consume(_), _) => {}
        }
    }
    let mut channels = Vec::<[u8; 32]>::with_capacity(held.len());
    for (channel, records) in held {
        if records == count {
            channels.push(channel);
        }
    }
    let [channel] = channels.as_slice() else {
        panic!("one channel holds {count} stored records: {channels:?}");
    };
    *channel
}

/// Added by decision 124 (L1, plan-join-batch Step 7): the records on the data channel `channel`
/// in the post-state of `played`, in the canonical order of DR-75: each source hash with the
/// value that `value_of` reads from the record. The records of the chains of this suite have
/// distinct source hashes, so the source hash alone gives the order.
fn stored_values(
    played: &Played,
    channel: &[u8; 32],
    value_of: fn(&ListParWithRandom) -> Par,
) -> Vec<([u8; 32], Par)> {
    let data = played
        .history
        .get_history_reader(&played.root)
        .expect("read the post-state")
        .get_data(&Blake2b256Hash::from_bytes(channel.to_vec()))
        .expect("the records of the channel");
    let mut values = Vec::<([u8; 32], Par)>::with_capacity(data.len());
    for datum in &data {
        let source: [u8; 32] = datum
            .source
            .hash
            .bytes()
            .try_into()
            .expect("a source hash has 32 bytes");
        values.push((source, value_of(&datum.a)));
    }
    values.sort_by_key(|(source, _)| *source);
    values
}

/// Added by decision 124 (L1, plan-join-batch Step 7): the value of a stored record of a set:
/// the one datum of its send.
fn set_value(record: &ListParWithRandom) -> Par {
    let unwrapped = record::unwrap_datum(record).expect("a set record unwraps");
    match unwrapped.datum.pars.as_slice() {
        [value] => value.clone(),
        values => panic!(
            "a set record of this suite holds one value, not {}",
            values.len()
        ),
    }
}

/// Added by decision 124 (L1, plan-join-batch Step 7): the integer `par`.
fn int_of(par: &Par) -> i64 {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }] => *value,
        _ => panic!("an integer: {par:?}"),
    }
}

/// Added by decision 124 (L1, plan-join-batch Step 7): the string `par`.
fn string_of(par: &Par) -> &str {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::GString(value)),
        }] => value,
        _ => panic!("a string: {par:?}"),
    }
}

/// Added by decision 124 (L1, plan-join-batch Step 7): the elements of the list `par`.
fn list_items(par: &Par) -> &[Par] {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::EListBody(list)),
        }] => &list.ps,
        _ => panic!("a list: {par:?}"),
    }
}

/// Added by decision 124 (L1, plan-join-batch Step 7): the deliveries `["join", tag, x, y]` on
/// [`OUT`] after the deploy `played`: the values `x` and `y` of each tag.
fn join_outputs(played: &Played) -> BTreeMap<i64, (Par, Par)> {
    let mut joins = BTreeMap::<i64, (Par, Par)>::new();
    for datum in &played.out {
        let [value] = datum.as_slice() else {
            panic!("a datum on OUT holds one value: {datum:?}");
        };
        match list_items(value) {
            [kind, tag, x, y] if string_of(kind) == "join" => {
                joins.insert(int_of(tag), (x.clone(), y.clone()));
            }
            _ => {}
        }
    }
    joins
}

/// Added by decision 124 (L1 and L3, plan-join-batch Step 7): the number of deliveries on
/// [`OUT`] after the deploy `played` whose list starts with the string `kind`.
fn deliveries(played: &Played, kind: &str) -> usize {
    let mut count = 0_usize;
    for datum in &played.out {
        let [value] = datum.as_slice() else {
            panic!("a datum on OUT holds one value: {datum:?}");
        };
        match list_items(value).first() {
            Some(first) if string_of(first) == kind => count += 1,
            Some(_) | None => {}
        }
    }
    count
}

/// Added by decision 124 (L1, plan-join-batch Step 7, rule CR-14): the value of a stored record
/// of a VectorDB space: the one datum of its send.
fn vdb_value(record: &ListParWithRandom) -> Par {
    let unwrapped = record::unwrap_vdb_datum(record).expect("a VectorDB record unwraps");
    match unwrapped.datum.pars.as_slice() {
        [value] => value.clone(),
        values => panic!(
            "a VectorDB record of this suite holds one value, not {}",
            values.len()
        ),
    }
}

/// Added by decision 124 (L1 and L3, plan-join-batch Step 7): the integer values of the records
/// on the data channel of the deploy `played` that stores `count` records, in the canonical
/// order of DR-75, read with `value_of` ([`stored_channel`] and [`stored_values`]).
fn stored_ints(played: &Played, count: usize, value_of: fn(&ListParWithRandom) -> Par) -> Vec<i64> {
    let channel = stored_channel(played, count);
    let mut values = Vec::<i64>::with_capacity(count);
    for (_, value) in stored_values(played, &channel, value_of) {
        values.push(int_of(&value));
    }
    values
}

// ─── T-C4-23: the re-offer of a stored persistent record (F22) ─────────────────────────────────

/// Added by decision 102 (plan v4 T-C4-23, F22): the list waiters of the re-offer chain: more
/// cascade fires than one transaction holds, so a settle job carries the last fires.
const REOFFER_WAITERS: usize = FIRE_ROUND_CAP + 6;

/// Added by decision 102 (plan v4 T-C4-23, F22): the fires that the settle job of the removal
/// carries. The removal fires `FIRE_ROUND_CAP` rounds: its own fire, then one cascade fire for
/// each of the first `FIRE_ROUND_CAP - 1` waiters.
const REOFFER_CARRIED: usize = REOFFER_WAITERS - (FIRE_ROUND_CAP - 1);

// The re-offer chain needs a settle job, so it needs more waiters than one transaction fires.
const _: () = assert!(REOFFER_CARRIED > 0);

/// Added by decision 102 (plan v4 T-C4-23, F22): the stash of the re-offer chain.
const REOFFER_STASH: &str = "nor-reoffer";

/// Added by decision 102 (plan v4 T-C4-23, F22): the constructor of the VectorDB spaces of this
/// suite, with room for every record of a chain.
const VECTORDB_CTOR: &str = r#""default"; capacity=16, dimensions=2"#;

/// Added by decision 102 (plan v4 T-C4-23, F22): the records of the re-offer chain as one
/// deploy: `img` and the persistent document `doc` on `a`. The query `[1.0, 0.0]` ranks `img`
/// first, then `doc`.
const REOFFER_RECORDS: &str =
    r#"a!("img"; embedding=[10.0, 0.0]) | a!!("doc"; embedding=[10.0, 1.0])"#;

/// Added by decision 102 (plan v4 T-C4-23, F22): the removal of the re-offer chain as one
/// deploy: the receive of `img`. LIST-CASCADE then fires each list waiter on `doc`.
const REOFFER_REMOVAL: &str = r#"for (@"img" <- a ~ ([1.0, 0.0])) { out!(["img", 0]) }"#;

/// Added by decision 102 (plan v4 T-C4-23, F22): `count` list waiters on `a` as one deploy, each
/// a receive of the top record for the query `[1.0, 0.0]` whose pattern binds `[{label}]` only.
/// While `img` ranks first, each waiter refuses its list and parks.
fn list_waiters(count: usize, label: &str) -> String {
    let mut waiters = Vec::<String>::with_capacity(count);
    for index in 0..count {
        waiters.push(format!(
            r#"for (@["{label}"] <- a ~ ([1.0, 0.0]; k=1)) {{ out!(["cascade", {index}]) }}"#
        ));
    }
    waiters.join(" |\n")
}

/// Added by decision 102 (plan v4 T-C4-23, F22, rule CR-19 case 3, C7.22): a chain of three
/// deploys on a VectorDB space. The first stores `img` and the persistent document `doc`, the
/// second parks [`REOFFER_WAITERS`] list waiters of `[doc]`, and the third removes `img`. The
/// removal fires `FIRE_ROUND_CAP` rounds, its own fire and one cascade fire for each of the
/// first waiters, and each cascade fire re-offers the stored `doc`, which stays stored. Its
/// settle job carries the other [`REOFFER_CARRIED`] fires. The first operation of the settle
/// job is its re-offer of the stored persistent `doc`, and every re-offer of `doc` shares the
/// source of `doc`, so the row of the re-offer is not unique to the job. So the settle job stages
/// its anchor first. Each runtime of `RACING_WORKERS` plays the chain once (no pick matters: the
/// cascade takes its waiters in DR-75 order, and the scores differ), and each deploy certifies
/// through its own native replay.
#[test]
fn own_reoffer_of_a_stored_persistent_record_stages_its_anchor_first() {
    let records = stashing(
        "rho:space:hashmap:vectordb",
        VECTORDB_CTOR,
        REOFFER_STASH,
        REOFFER_RECORDS,
    );
    let waiters = with_stash(REOFFER_STASH, &list_waiters(REOFFER_WAITERS, "doc"));
    let removal = with_stash(REOFFER_STASH, REOFFER_REMOVAL);
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("the re-offer chain, {workers} workers");
            let mut chain = NativeChain::new().await;
            store_and_park(&mut chain, &label, &records, &waiters).await;
            let played = chain
                .play(&removal, "nor-reoffer-removal", AMPLE_LIMIT)
                .await;
            assert!(
                played.result.errors.is_empty(),
                "{label}: the removal runs to its end: {:?}",
                played.result.errors
            );
            assert_eq!(
                played.outputs.len(),
                REOFFER_WAITERS + 1,
                "{label}: one output for each waiter and one for img: {:?}",
                played.outputs
            );
            let (_, journal) = journal_of(&played);
            // The removal stages its own consume first, so only its settle job anchors.
            let anchors = anchor_indices(journal);
            let [anchor] = anchors.as_slice() else {
                panic!("{label}: one settle job anchors, so one anchor: {anchors:?}");
            };
            let successor = journal
                .iter()
                .skip(anchor + 1)
                .find(|row| is_space_row(row))
                .unwrap_or_else(|| panic!("{label}: a space row follows the anchor"));
            let NativeOperationSource::Produce(reoffer) = &successor.source else {
                panic!("{label}: the anchor is followed by a consume, not by the re-offer");
            };
            assert!(
                reoffer.persistent && successor.comm.is_some(),
                "{label}: the row after the anchor is a fire of a persistent record"
            );
            // Every cascade fire re-offers `doc` with its source: the removal re-offers it for
            // each of the first `FIRE_ROUND_CAP - 1` waiters before the anchor, and the settle
            // job re-offers it for each other waiter after the anchor.
            let mut before = 0_usize;
            let mut after = 0_usize;
            for (index, row) in journal.iter().enumerate() {
                match &row.source {
                    NativeOperationSource::Produce(source) if source == reoffer => {
                        match index < *anchor {
                            true => before += 1,
                            false => after += 1,
                        }
                    }
                    NativeOperationSource::Produce(_) | NativeOperationSource::Consume(_) => {}
                }
            }
            assert_eq!(
                (before, after),
                (FIRE_ROUND_CAP - 1, REOFFER_CARRIED),
                "{label}: the re-offers of doc before and after the anchor"
            );
            // The space COMMs: the own fire and the cascade fires of the removal, then the
            // anchor and the carried fires of the settle job.
            assert_eq!(
                space_comms(journal),
                FIRE_ROUND_CAP + 1 + REOFFER_CARRIED,
                "{label}: the COMMs of the removal and of its settle job"
            );
            let certification = certify(&played, &removal, AMPLE_LIMIT).await;
            assert_reproduces(&label, &played, &certification);
        });
    }
}

// ─── T-C4-19: the cascade finds the lift in DR-75 order ────────────────────────────────────────

/// Added by decision 124 (L1, plan-join-batch Step 7, T-C4-19): the stash of the set chains.
const SET_STASH: &str = "nor-set-chain";

/// Added by decision 124 (L1, T-C4-19): the records of the set chains as one deploy: six members
/// of class 0 and the values 1 and 2 on `a`, and the values 7 and 8 on `b`. A pick of play takes
/// a member of class 0 on `a` with a chance of 3/4.
const SET_RECORDS: &str =
    "a!(0) | a!(0) | a!(0) | a!(0) | a!(0) | a!(0) | a!(1) | a!(2) | b!(7) | b!(8)";

/// Added by decision 124 (L1, T-C4-19): the records of [`SET_RECORDS`] on `a`.
const SET_RECORDS_OF_A: usize = 8;

/// Added by decision 124 (L1, T-C4-19): the removal of the set chains as one deploy: the receive
/// of class 0, which retires the other members of the class. Its cascade then fires the parked
/// guarded joins on the records of class 1 and class 2.
const SET_REMOVAL: &str = r#"for (@0 <- a) { out!(["zero", 0]) }"#;

/// Added by decision 124 (L1, T-C4-19, rule (a)): the guarded join with the tag `tag` and the
/// guard `guard` over `x` on `a` and `y` on `b`. Rule (a): the cascades of a set start from
/// guarded waiters, because a waiter without a guard never parks while a record binds it.
fn guarded_join(tag: i64, guard: &str) -> String {
    format!(r#"for (@x <- a & @y <- b where {guard}) {{ out!(["join", {tag}, x, y]) }}"#)
}

/// Added by decision 124 (L1, T-C4-19, rule (a)): plays the set chain up to its records on
/// `chain`: the creation of a `hashmap:set` space, then [`SET_RECORDS`] with the first salt whose
/// records on `a` put a member of class 0 first in the canonical order of DR-75. So each guard
/// of the chain that refuses class 0 refuses the canonical tuple until the removal of class 0,
/// and a parked guarded join stays parked through the pre-plan drain of each later operation.
/// Returns the play of the records and the values on `a` in DR-75 order.
async fn salted_set_records(chain: &mut NativeChain, label: &str) -> (Played, Vec<i64>) {
    let creation = stashing("rho:space:hashmap:set", r#""default""#, SET_STASH, "Nil");
    let creation_label = format!("{label}: the space");
    let created = play_certified(chain, &creation, &creation_label).await;
    assert_runs(&creation_label, &created);
    let records = with_stash(SET_STASH, SET_RECORDS);
    for salt in 0..SALTS {
        chain.reset(&created.root).await;
        let records_label = format!("{label}: the records, salt {salt}");
        let stored = chain.play(&records, &records_label, AMPLE_LIMIT).await;
        assert_runs(&records_label, &stored);
        let channel = stored_channel(&stored, SET_RECORDS_OF_A);
        let mut values = Vec::<i64>::with_capacity(SET_RECORDS_OF_A);
        for (_, value) in stored_values(&stored, &channel, set_value) {
            values.push(int_of(&value));
        }
        if let Some(0) = values.first() {
            let certification = certify(&stored, &records, AMPLE_LIMIT).await;
            assert_reproduces(&records_label, &stored, &certification);
            return (stored, values);
        }
    }
    panic!("{label}: no salt below {SALTS} puts class 0 first on a in DR-75 order");
}

/// Added by decision 124 (L1, plan-join-batch Step 7, T-C4-19, cases (a) and (d)): the set
/// chain with two guarded joins `x != 0` on `a` and `b`, run [`PICK_RUNS`] times from its
/// records. Each join picks at random in play, so it parks when its pick takes class 0, and
/// fires at once otherwise. The removal of class 0 then fires each parked join in its cascade,
/// at Following: the own consume of the removal comes first, so no anchor precedes the lifts.
/// Case (a): when both joins park, the cascade fires two distinct leads that are not persistent,
/// the records of class 1 and class 2. Case (d): both joins share the lead, the first record of
/// class 1 or 2 in DR-75 order, and the re-offer of that lead names the first waiter in DR-75
/// order. Each fire follows the `K_lift` of its lead, the waiters fire in DR-75 order, and the
/// records of each channel are bound in DR-75 order. Returns the runs with two fires.
async fn set_cascade_case(workers: usize) -> usize {
    let label = format!("T-C4-19 (a, d), {workers} workers");
    let mut chain = NativeChain::new().await;
    let (stored, values) = salted_set_records(&mut chain, &label).await;
    let shared_lead = values
        .iter()
        .copied()
        .find(|value| *value != 0)
        .expect("a holds the values 1 and 2");
    let joins = [
        with_stash(SET_STASH, &guarded_join(1, "x != 0")),
        with_stash(SET_STASH, &guarded_join(2, "x != 0")),
    ];
    let removal = with_stash(SET_STASH, SET_REMOVAL);
    let mut two_fires = 0_usize;
    for run in 0..PICK_RUNS {
        chain.reset(&stored.root).await;
        let mut parked = BTreeMap::<[u8; 32], i64>::new();
        for (tag, join) in (1_i64..).zip(&joins) {
            let join_label = format!("{label}: join {tag}, run {run}");
            let played = play_certified(&mut chain, join, &join_label).await;
            assert_runs(&join_label, &played);
            for waiter in parked_waiters(&played) {
                parked.insert(waiter, tag);
            }
        }
        let removal_label = format!("{label}: the removal, run {run}");
        let played = play_certified(&mut chain, &removal, &removal_label).await;
        assert_runs(&removal_label, &played);
        let (_, journal) = journal_of(&played);
        let waiters: BTreeSet<[u8; 32]> = parked.keys().copied().collect();
        let fires = waiter_fires(journal, &waiters);
        assert_eq!(
            fires.len(),
            parked.len(),
            "{removal_label}: the cascade fires each parked join once"
        );
        assert!(
            anchor_indices(journal).is_empty(),
            "{removal_label}: the removal stages its own consume first, so it stages no anchor"
        );
        assert_ascending_waiters(&removal_label, &fires);
        assert_lifted(&removal_label, journal, &fires);
        assert_ascending_records(&removal_label, &fires);
        for fire in &fires {
            let lift = space_row_before(journal, fire.index).expect("the lift of a fire");
            assert!(
                space_row_before(journal, lift).is_some(),
                "{removal_label}: the cascade lifts at Following, after the own consume"
            );
        }
        if let [first, second] = fires.as_slice() {
            two_fires += 1;
            assert_ne!(
                first.record, second.record,
                "{removal_label}: (a) the two fires have distinct leads"
            );
            assert!(
                !first.record.persistent && !second.record.persistent,
                "{removal_label}: (a) the leads are not persistent"
            );
            assert_eq!(
                Some(&first.waiter),
                waiters.first(),
                "{removal_label}: (d) the re-offer of the shared lead names the first waiter"
            );
            let tag = parked
                .get(&first.waiter)
                .expect("the tag of the first waiter");
            let outputs = join_outputs(&played);
            let (x, _) = outputs.get(tag).expect("the delivery of the first waiter");
            assert_eq!(
                int_of(x),
                shared_lead,
                "{removal_label}: (d) the first waiter binds the shared lead"
            );
        }
    }
    two_fires
}

/// Added by decision 124 (L1, T-C4-19, case (b)): the stash of the two-query chain.
const TWO_QUERY_STASH: &str = "nor-two-queries";

/// Added by decision 124 (L1, T-C4-19, case (b)): the constructor of the two-query chain, with
/// room for each of its records.
const TWO_QUERY_CTOR: &str = r#""default"; capacity=128, dimensions=3"#;

/// Added by decision 124 (L1, T-C4-19, case (b)): the list waiters of the two-query chain whose
/// lead is the persistent document `pdoc`.
const PDOC_WAITERS: usize = 40;

/// Added by decision 124 (L1, T-C4-19, case (b)): the documents of the two-query chain, and its
/// list waiters whose lead is a document.
const TWO_QUERY_DOCUMENTS: usize = 30;

// The two-query chain needs a settle job, so it needs more waiters than one transaction fires.
const _: () = assert!(PDOC_WAITERS + TWO_QUERY_DOCUMENTS > FIRE_ROUND_CAP - 1);

/// Added by decision 124 (L1, T-C4-19, case (b)): the query of the waiters of `pdoc`. It ranks
/// `img` first, then `pdoc`, then the documents.
const PDOC_QUERY: &str = "[1.0, 0.0, 0.1]";

/// Added by decision 124 (L1, T-C4-19, case (b)): the query of the waiters of the documents. It
/// ranks `img` first, then the documents in the order of their second coordinate, then `pdoc`.
const DOCUMENT_QUERY: &str = "[1.0, 0.1, 0.0]";

/// Added by decision 124 (L1, T-C4-19, case (b)): the removal of the two-query chain as one
/// deploy: the receive of `img`. LIST-CASCADE then fires each list waiter.
const TWO_QUERY_REMOVAL: &str = r#"for (@"img" <- a ~ ([1.0, 0.0, 0.0])) { out!(["img", 0]) }"#;

/// Added by decision 124 (L1, T-C4-19, case (b)): the records of the two-query chain: `img`, the
/// persistent document `pdoc`, and [`TWO_QUERY_DOCUMENTS`] documents with distinct second
/// coordinates from 2.50 to 3.95.
fn two_query_records() -> String {
    let mut records = Vec::<String>::with_capacity(TWO_QUERY_DOCUMENTS + 2);
    records.push(r#"a!("img"; embedding=[10.0, 0.0, 0.0])"#.to_string());
    records.push(r#"a!!("pdoc"; embedding=[10.0, 0.0, 3.0])"#.to_string());
    for index in 0..TWO_QUERY_DOCUMENTS {
        let hundredths = 250 + 5 * index;
        records.push(format!(
            r#"a!("doc"; embedding=[10.0, {}.{:02}, 0.0])"#,
            hundredths / 100,
            hundredths % 100
        ));
    }
    records.join(" |\n")
}

/// Added by decision 124 (L1, T-C4-19, case (b)): `count` list waiters on `a` as one deploy, each
/// a receive of the top record for the query `query` whose pattern binds `[{label}]` only. While
/// `img` ranks first, each waiter refuses its list and parks.
fn query_waiters(count: usize, label: &str, query: &str) -> String {
    let mut waiters = Vec::<String>::with_capacity(count);
    for index in 0..count {
        waiters.push(format!(
            r#"for (@["{label}"] <- a ~ ({query}; k=1)) {{ out!(["{label}", {index}]) }}"#
        ));
    }
    waiters.join(" |\n")
}

/// Added by decision 124 (L1, plan-join-batch Step 7, T-C4-19, case (b)): a persistent lead next
/// to leads that are not persistent, at an empty transaction. The two-query chain stores `img`,
/// `pdoc` and the documents, parks [`PDOC_WAITERS`] waiters of `pdoc` and one waiter for each
/// document, and removes `img`. The removal fires `FIRE_ROUND_CAP` rounds and its settle job
/// carries the other fires, all in DR-75 order of their waiters. A fire of `pdoc` re-offers the
/// persistent `pdoc` without a lift, and a fire of a document lifts it first. The salts vary
/// the hashes of the waiters until the first fire of the settle job, at its empty transaction,
/// is a fire of `pdoc` and the job also fires a document. That fire stages the anchor first,
/// because the re-offer of `pdoc` is not the job's own (rule CR-19 case 3). Each salt with a
/// document first shows the other order: its lift is the job's own, so the job stages no anchor.
/// Returns the salt of the run that opens the settle job with `pdoc`.
async fn persistent_lead_case(workers: usize) -> usize {
    let label = format!("T-C4-19 (b), {workers} workers");
    let records = stashing(
        "rho:space:hashmap:vectordb",
        TWO_QUERY_CTOR,
        TWO_QUERY_STASH,
        &two_query_records(),
    );
    let pdoc_waiters = with_stash(
        TWO_QUERY_STASH,
        &query_waiters(PDOC_WAITERS, "pdoc", PDOC_QUERY),
    );
    let document_waiters = with_stash(
        TWO_QUERY_STASH,
        &query_waiters(TWO_QUERY_DOCUMENTS, "doc", DOCUMENT_QUERY),
    );
    let removal = with_stash(TWO_QUERY_STASH, TWO_QUERY_REMOVAL);
    let mut chain = NativeChain::new().await;
    let records_label = format!("{label}: the records");
    let stored = play_certified(&mut chain, &records, &records_label).await;
    assert_runs(&records_label, &stored);
    for salt in 0..SALTS {
        chain.reset(&stored.root).await;
        let pdoc_label = format!("{label}: the waiters of pdoc, salt {salt}");
        let parked = play_certified(&mut chain, &pdoc_waiters, &pdoc_label).await;
        assert_parks(&pdoc_label, &parked);
        let leads_of_pdoc = parked_waiters(&parked);
        assert_eq!(
            leads_of_pdoc.len(),
            PDOC_WAITERS,
            "{pdoc_label}: the parked waiters"
        );
        let document_label = format!("{label}: the waiters of the documents, salt {salt}");
        let parked = play_certified(&mut chain, &document_waiters, &document_label).await;
        assert_parks(&document_label, &parked);
        let mut waiters = parked_waiters(&parked);
        assert_eq!(
            waiters.len(),
            TWO_QUERY_DOCUMENTS,
            "{document_label}: the parked waiters"
        );
        waiters.extend(leads_of_pdoc.iter().copied());
        let removal_label = format!("{label}: the removal, salt {salt}");
        let played = play_certified(&mut chain, &removal, &removal_label).await;
        assert_runs(&removal_label, &played);
        assert_eq!(
            played.outputs.len(),
            waiters.len() + 1,
            "{removal_label}: one output for each waiter and one for img"
        );
        let (_, journal) = journal_of(&played);
        let fires = waiter_fires(journal, &waiters);
        assert_eq!(
            fires.len(),
            waiters.len(),
            "{removal_label}: each waiter fires once"
        );
        assert_ascending_waiters(&removal_label, &fires);
        assert_lifted(&removal_label, journal, &fires);
        let mut documents = BTreeSet::<[u8; 32]>::new();
        for fire in &fires {
            match (leads_of_pdoc.contains(&fire.waiter), fire.record.persistent) {
                (true, true) => {}
                (false, false) => assert!(
                    documents.insert(fire.record.hash),
                    "{removal_label}: each waiter of a document binds a document of its own"
                ),
                (pdoc, persistent) => panic!(
                    "{removal_label}: a waiter of pdoc ({pdoc}) fires the persistent lead \
                     ({persistent})"
                ),
            }
        }
        // The removal fires `FIRE_ROUND_CAP` rounds: its own fire, then the first
        // `FIRE_ROUND_CAP - 1` fires of the cascade. Its settle job carries the other fires.
        let carried = &fires[FIRE_ROUND_CAP - 1..];
        let opening = &carried[0];
        let before = space_row_before(journal, opening.index).expect("a row before the job");
        assert_eq!(
            space_row_before(journal, before),
            Some(fires[FIRE_ROUND_CAP - 2].index),
            "{removal_label}: the settle job starts after the last fire of the removal"
        );
        let anchors = anchor_indices(journal);
        match opening.record.persistent {
            true => {
                assert_eq!(
                    anchors,
                    [before],
                    "{removal_label}: the settle job stages its anchor first, then the \
                     re-offer of the persistent lead"
                );
                let mixed = carried.iter().any(|fire| !fire.record.persistent);
                if mixed {
                    return salt;
                }
            }
            false => assert!(
                anchors.is_empty(),
                "{removal_label}: the settle job lifts its own lead first, so no anchor"
            ),
        }
    }
    panic!("{label}: no salt below {SALTS} opens the settle job with pdoc next to a document lead");
}

/// Added by decision 124 (L1, T-C4-19, case (c)): the stash of the document chain.
const DOCUMENT_STASH: &str = "nor-documents";

/// Added by decision 124 (L1, T-C4-19, case (c)): the constructor of the document chain, with
/// room for each of its records.
const DOCUMENT_CTOR: &str = r#""default"; capacity=128, dimensions=2"#;

/// Added by decision 124 (L1, T-C4-19, case (c)): the documents of the document chain, and its
/// list waiters: more cascade fires than one transaction holds.
const DOCUMENT_WAITERS: usize = FIRE_ROUND_CAP + 6;

/// Added by decision 124 (L1, T-C4-19, case (c)): the records of the document chain: `img` and
/// [`DOCUMENT_WAITERS`] documents. The query `[1.0, 0.0]` ranks `img` first, then the documents
/// in the order of their second coordinate.
fn document_records() -> String {
    let mut records = Vec::<String>::with_capacity(DOCUMENT_WAITERS + 1);
    records.push(r#"a!("img"; embedding=[10.0, 0.0])"#.to_string());
    for index in 1..=DOCUMENT_WAITERS {
        records.push(format!(r#"a!("doc"; embedding=[10.0, {index}.0])"#));
    }
    records.join(" |\n")
}

/// Added by decision 124 (L1, plan-join-batch Step 7, T-C4-19, case (c)): VectorDB lists with
/// distinct first members in the cascade of the removal (Following) and in its settle job
/// (Unread). The document chain stores `img` and the documents, parks one list waiter for each
/// document, and removes `img`. Each fire lifts the top document, so the next waiter in DR-75
/// order lists the next document. The removal fires `FIRE_ROUND_CAP` rounds after its own
/// consume, and its settle job carries the other fires from an empty transaction. Each lift is
/// the job's own, so no job stages an anchor. No pick matters: the scores differ, and the
/// cascade takes its waiters in DR-75 order.
async fn document_list_case(workers: usize) {
    let label = format!("T-C4-19 (c), {workers} workers");
    let records = stashing(
        "rho:space:hashmap:vectordb",
        DOCUMENT_CTOR,
        DOCUMENT_STASH,
        &document_records(),
    );
    let waiters = with_stash(DOCUMENT_STASH, &list_waiters(DOCUMENT_WAITERS, "doc"));
    let removal = with_stash(DOCUMENT_STASH, REOFFER_REMOVAL);
    let mut chain = NativeChain::new().await;
    let records_label = format!("{label}: the records");
    let stored = play_certified(&mut chain, &records, &records_label).await;
    assert_runs(&records_label, &stored);
    let waiters_label = format!("{label}: the waiters");
    let parked = play_certified(&mut chain, &waiters, &waiters_label).await;
    assert_parks(&waiters_label, &parked);
    let waiting = parked_waiters(&parked);
    assert_eq!(
        waiting.len(),
        DOCUMENT_WAITERS,
        "{waiters_label}: the parked waiters"
    );
    let removal_label = format!("{label}: the removal");
    let played = play_certified(&mut chain, &removal, &removal_label).await;
    assert_runs(&removal_label, &played);
    assert_eq!(
        played.outputs.len(),
        DOCUMENT_WAITERS + 1,
        "{removal_label}: one output for each waiter and one for img"
    );
    let (_, journal) = journal_of(&played);
    let fires = waiter_fires(journal, &waiting);
    assert_eq!(
        fires.len(),
        DOCUMENT_WAITERS,
        "{removal_label}: each waiter fires once"
    );
    assert_ascending_waiters(&removal_label, &fires);
    assert_lifted(&removal_label, journal, &fires);
    let mut documents = BTreeSet::<[u8; 32]>::new();
    for fire in &fires {
        assert!(
            !fire.record.persistent && documents.insert(fire.record.hash),
            "{removal_label}: each waiter lists a document of its own as its first member"
        );
    }
    assert!(
        anchor_indices(journal).is_empty(),
        "{removal_label}: each job lifts its own lead first, so no job stages an anchor"
    );
    let first_lift = space_row_before(journal, fires[0].index).expect("the first lift");
    assert!(
        space_row_before(journal, first_lift).is_some(),
        "{removal_label}: the cascade of the removal lifts at Following, after its own consume"
    );
    let carried_lift =
        space_row_before(journal, fires[FIRE_ROUND_CAP - 1].index).expect("the carried lift");
    assert_eq!(
        space_row_before(journal, carried_lift),
        Some(fires[FIRE_ROUND_CAP - 2].index),
        "{removal_label}: the settle job opens at Unread with its lift, after the last fire of \
         the removal"
    );
}

/// Added by decision 124 (L1, T-C4-19, case (e)): the stash of the PathMap set chain.
const PATHMAP_STASH: &str = "nor-pathmap-set";

/// Added by decision 124 (L1, T-C4-19, case (e)): the records of the PathMap set chain, with the
/// creation of its space: two members of class 0 on the path `a`, the value 1 on `[a, 1]`, the
/// value 2 on `[a, 2]`, and the values 7 and 8 on `b`. A bind on `a` delivers the record on
/// `[a, s]` with the value `v` as `[s, v]`.
const PATHMAP_RECORDS: &str =
    "a!(0) | a!(0) | use space { @[*a, 1]!(1) | @[*a, 2]!(2) } | b!(7) | b!(8)";

/// Added by decision 124 (L1, T-C4-19, case (e)): checks that the deploy `played` parks its join
/// on a PathMap space: it fails nothing, delivers nothing, parks one waiter, and fires no space
/// operation but the bump of the space counter, a COMM whose row names `S(sid)` alone.
fn assert_parks_on_paths(label: &str, played: &Played) {
    assert_runs(label, played);
    assert!(
        played.outputs.is_empty(),
        "{label}: a waiter delivers: {:?}",
        played.outputs
    );
    let (_, journal) = journal_of(played);
    for (index, row) in journal.iter().enumerate() {
        if let (Some(_), true) = (&row.comm, is_space_row(row)) {
            assert_eq!(
                footprint_kinds(row),
                ["S"],
                "{label}: row {index} fires a space operation other than the bump of the space \
                 counter"
            );
        }
    }
}

/// Added by decision 124 (L1, plan-join-batch Step 7, T-C4-19, case (e)): the PathMap set chain
/// with two guarded joins `x != 0` on `a` and `b`, run [`PICK_RUNS`] times from its records. A
/// PathMap window keeps path-key order and DR-75 in both roles (decision 124, L4), so the
/// window of `a` puts the path `a` and class 0 first, and both joins park (each park bumps the
/// space counter, [`assert_parks_on_paths`]). The removal of class 0 then fires both joins in
/// its cascade, in DR-75 order of their waiters. The first lists
/// `[a, 1]` first, so it binds `x = [1, 1]`, and the second binds `x = [2, 2]`. Each fire
/// re-offers the vehicle of its record on the index channel of `a`, after the `K_lift` of the
/// marker of the record on that channel.
async fn pathmap_set_case(workers: usize) {
    let label = format!("T-C4-19 (e), {workers} workers");
    let records = stashing(
        "rho:space:pathmap:set",
        r#""default""#,
        PATHMAP_STASH,
        PATHMAP_RECORDS,
    );
    let joins = [
        with_stash(PATHMAP_STASH, &guarded_join(1, "x != 0")),
        with_stash(PATHMAP_STASH, &guarded_join(2, "x != 0")),
    ];
    let removal = with_stash(PATHMAP_STASH, SET_REMOVAL);
    let mut chain = NativeChain::new().await;
    let records_label = format!("{label}: the records");
    let stored = play_certified(&mut chain, &records, &records_label).await;
    assert_runs(&records_label, &stored);
    for run in 0..PICK_RUNS {
        chain.reset(&stored.root).await;
        let mut parked = BTreeMap::<[u8; 32], i64>::new();
        for (tag, join) in (1_i64..).zip(&joins) {
            let join_label = format!("{label}: join {tag}, run {run}");
            let played = play_certified(&mut chain, join, &join_label).await;
            assert_parks_on_paths(&join_label, &played);
            let waiting: Vec<[u8; 32]> = parked_waiters(&played).into_iter().collect();
            let [waiter] = waiting.as_slice() else {
                panic!("{join_label}: one parked join: {}", waiting.len());
            };
            parked.insert(*waiter, tag);
        }
        let removal_label = format!("{label}: the removal, run {run}");
        let played = play_certified(&mut chain, &removal, &removal_label).await;
        assert_runs(&removal_label, &played);
        let (_, journal) = journal_of(&played);
        let waiters: BTreeSet<[u8; 32]> = parked.keys().copied().collect();
        let fires = waiter_fires(journal, &waiters);
        let [first, second] = fires.as_slice() else {
            panic!(
                "{removal_label}: the cascade fires both joins: {}",
                fires.len()
            );
        };
        assert_ascending_waiters(&removal_label, &fires);
        for fire in &fires {
            let before = space_row_before(journal, fire.index).expect("a row before a fire");
            assert_eq!(
                lifted_by(&journal[before]).map(|lifted| lifted.channel),
                Some(fire.record.channel),
                "{removal_label}: the K_lift of the marker on its channel precedes each fire"
            );
        }
        let outputs = join_outputs(&played);
        for (fire, expected) in [(first, [1_i64, 1]), (second, [2, 2])] {
            let tag = parked.get(&fire.waiter).expect("the tag of a fired join");
            let (x, _) = outputs.get(tag).expect("the delivery of a fired join");
            let bound: Vec<i64> = list_items(x).iter().map(int_of).collect();
            assert_eq!(
                bound, expected,
                "{removal_label}: the cascade binds the records of a in path-key order"
            );
        }
    }
}

/// Added by decision 124 (L1, plan-join-batch Step 7, T-C4-19; PLAN:1201): the cascade of a set
/// and of a VectorDB space finds the lift of each fire in DR-75 order, with no look-ahead. The
/// cases: (a) a set with a join group and two distinct leads that are not persistent, at
/// Following (`set_cascade_case`); (b) a persistent lead next to leads that are not persistent,
/// at an empty transaction (`persistent_lead_case`); (c) VectorDB lists in a settle job (Unread)
/// and in the cascade of the removal (Following) (`document_list_case`); (d) a shared lead
/// named by the re-offer (`set_cascade_case`); (e) the PathMap set (`pathmap_set_case`). Each
/// case runs on each runtime of `RACING_WORKERS`, and each deploy certifies through its own
/// native replay. Non-vacuity: on each runtime, case (b) has a run that fires the persistent
/// lead first in a settle job, and case (a) has a run with two fires.
#[test]
fn set_and_vectordb_cascade_finds_the_lift_in_dr75_order() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let two_fires = set_cascade_case(workers).await;
            assert!(
                two_fires > 0,
                "T-C4-19 (a), {workers} workers: a run parks both joins, so its cascade fires two \
                 leads"
            );
            let salt = persistent_lead_case(workers).await;
            println!(
                "T-C4-19, {workers} workers: (a) {two_fires} of {PICK_RUNS} runs fire two leads; \
                 (b) salt {salt} opens the settle job with the persistent lead"
            );
            document_list_case(workers).await;
            pathmap_set_case(workers).await;
        });
    }
}

// ─── F3: the cascade window keeps DR-75 for a guarded waiter ───────────────────────────────────

/// Added by decision 124 (L1, plan-join-batch Step 7, F3, arbiter-cr30-B gap (b)): the set chain
/// of T-C4-19 with two guarded joins, run [`PICK_RUNS`] times from its records. The values `v1`
/// and `v2` follow class 0 on `a` in the canonical order of DR-75, `v1` first. The late join,
/// with the tag 1, admits `x == v2` only, and the early join, with the tag 2, admits `x == v1`
/// only. In play each join picks at random, so it fires at once when its pick passes its guard,
/// and it parks otherwise.
/// The removal of class 0 then fires each parked join in its cascade. The cascade window keeps
/// DR-75 (F3): each waiter checks its guard on the canonical tuple, whose `x` is `v1` while `v1`
/// is stored. So the late join refuses that tuple and waits, even when it comes first in the
/// DR-75 order of the waiters, and the early join fires first, with `v1`. The late join fires
/// next, with `v2`. In every run the late join binds `v2` and the early join binds `v1`, and each
/// deploy certifies through its own native replay. Returns the runs in which both joins park and
/// the late join comes first in DR-75 order: the runs in which a random cascade window could
/// fire the late join first.
async fn guarded_window_set_case(workers: usize) -> usize {
    let label = format!("F3 (set), {workers} workers");
    let mut chain = NativeChain::new().await;
    let (stored, values) = salted_set_records(&mut chain, &label).await;
    let mut leads = values.iter().copied().filter(|value| *value != 0);
    let (Some(first), Some(second)) = (leads.next(), leads.next()) else {
        panic!("{label}: a holds two values after class 0: {values:?}");
    };
    let joins = [
        with_stash(SET_STASH, &guarded_join(1, &format!("x == {second}"))),
        with_stash(SET_STASH, &guarded_join(2, &format!("x == {first}"))),
    ];
    let removal = with_stash(SET_STASH, SET_REMOVAL);
    let mut late_first = 0_usize;
    for run in 0..PICK_RUNS {
        chain.reset(&stored.root).await;
        let mut parked = BTreeMap::<i64, [u8; 32]>::new();
        for (tag, join) in (1_i64..).zip(&joins) {
            let join_label = format!("{label}: join {tag}, run {run}");
            let played = play_certified(&mut chain, join, &join_label).await;
            assert_runs(&join_label, &played);
            for waiter in parked_waiters(&played) {
                parked.insert(tag, waiter);
            }
        }
        let removal_label = format!("{label}: the removal, run {run}");
        let played = play_certified(&mut chain, &removal, &removal_label).await;
        assert_runs(&removal_label, &played);
        let outputs = join_outputs(&played);
        for (tag, admitted) in [(1_i64, second), (2, first)] {
            let (x, _) = outputs
                .get(&tag)
                .unwrap_or_else(|| panic!("{removal_label}: join {tag} delivers"));
            assert_eq!(
                int_of(x),
                admitted,
                "{removal_label}: join {tag} binds the value that its guard admits"
            );
        }
        let (_, journal) = journal_of(&played);
        let waiters: BTreeSet<[u8; 32]> = parked.values().copied().collect();
        let fires = waiter_fires(journal, &waiters);
        assert_eq!(
            fires.len(),
            waiters.len(),
            "{removal_label}: the cascade fires each parked join once"
        );
        assert_lifted(&removal_label, journal, &fires);
        assert_ascending_records(&removal_label, &fires);
        if let (Some(late), Some(early), [opening, closing]) =
            (parked.get(&1), parked.get(&2), fires.as_slice())
        {
            assert_eq!(
                (&opening.waiter, &closing.waiter),
                (early, late),
                "{removal_label}: the early join fires first, with v1, and the late join next"
            );
            if late < early {
                late_first += 1;
            }
        }
    }
    late_first
}

/// Added by decision 124 (L1, plan-join-batch Step 7, F3): the PathMap set chain of T-C4-19 (e)
/// with two guarded joins, run [`PICK_RUNS`] times from its records. A PathMap window keeps
/// path-key order and DR-75 in both roles (decision 124, L4), so both joins park while class 0
/// is stored on the path `a`. The late join (tag 1) admits `x == [2, 2]` only, the record of
/// `[a, 2]`, and the early join (tag 2) admits `x == [1, 1]` only, the record of `[a, 1]`, which
/// comes first in the window of `a` once the removal of class 0 retires the class. So the
/// cascade of the removal fires the early join first and the late join next, whatever the DR-75
/// order of the waiters. Each fire re-offers the vehicle of its record after the `K_lift` of the
/// marker of the record on its channel. Returns the runs in which the late join comes first in
/// DR-75 order.
async fn guarded_window_pathmap_case(workers: usize) -> usize {
    let label = format!("F3 (PathMap set), {workers} workers");
    let records = stashing(
        "rho:space:pathmap:set",
        r#""default""#,
        PATHMAP_STASH,
        PATHMAP_RECORDS,
    );
    let joins = [
        with_stash(PATHMAP_STASH, &guarded_join(1, "x == [2, 2]")),
        with_stash(PATHMAP_STASH, &guarded_join(2, "x == [1, 1]")),
    ];
    let removal = with_stash(PATHMAP_STASH, SET_REMOVAL);
    let mut chain = NativeChain::new().await;
    let records_label = format!("{label}: the records");
    let stored = play_certified(&mut chain, &records, &records_label).await;
    assert_runs(&records_label, &stored);
    let mut late_first = 0_usize;
    for run in 0..PICK_RUNS {
        chain.reset(&stored.root).await;
        let mut parked = Vec::<[u8; 32]>::with_capacity(joins.len());
        for (tag, join) in (1_i64..).zip(&joins) {
            let join_label = format!("{label}: join {tag}, run {run}");
            let played = play_certified(&mut chain, join, &join_label).await;
            assert_parks_on_paths(&join_label, &played);
            let waiting: Vec<[u8; 32]> = parked_waiters(&played).into_iter().collect();
            let [waiter] = waiting.as_slice() else {
                panic!("{join_label}: one parked join: {}", waiting.len());
            };
            parked.push(*waiter);
        }
        let [late, early] = parked.as_slice() else {
            panic!("{label}: run {run} parks two joins: {}", parked.len());
        };
        let removal_label = format!("{label}: the removal, run {run}");
        let played = play_certified(&mut chain, &removal, &removal_label).await;
        assert_runs(&removal_label, &played);
        let (_, journal) = journal_of(&played);
        let waiters: BTreeSet<[u8; 32]> = parked.iter().copied().collect();
        let fires = waiter_fires(journal, &waiters);
        let [opening, closing] = fires.as_slice() else {
            panic!(
                "{removal_label}: the cascade fires both joins: {}",
                fires.len()
            );
        };
        assert_eq!(
            (&opening.waiter, &closing.waiter),
            (early, late),
            "{removal_label}: the early join fires first, with [1, 1], and the late join next"
        );
        for fire in &fires {
            let before = space_row_before(journal, fire.index).expect("a row before a fire");
            assert_eq!(
                lifted_by(&journal[before]).map(|lifted| lifted.channel),
                Some(fire.record.channel),
                "{removal_label}: the K_lift of the marker on its channel precedes each fire"
            );
        }
        let outputs = join_outputs(&played);
        for (tag, admitted) in [(1_i64, [2_i64, 2]), (2, [1, 1])] {
            let (x, _) = outputs
                .get(&tag)
                .unwrap_or_else(|| panic!("{removal_label}: join {tag} delivers"));
            let bound: Vec<i64> = list_items(x).iter().map(int_of).collect();
            assert_eq!(
                bound, admitted,
                "{removal_label}: join {tag} binds the record that its guard admits"
            );
        }
        if late < early {
            late_first += 1;
        }
    }
    late_first
}

/// Added by decision 124 (L1, plan-join-batch Step 7, F3; PLAN:1202): the cascade window of a set
/// and of a PathMap set keeps DR-75 for a guarded waiter (arbiter-cr30-B gap (b), the scenario
/// "cascadewin"): a waiter whose guard refuses the first record of the window does not fire
/// first, even when it comes first in the DR-75 order of the waiters (`guarded_window_set_case`
/// and `guarded_window_pathmap_case`). Each case runs on each runtime of `RACING_WORKERS`, and
/// each deploy certifies through its own native replay. Non-vacuity: on each runtime, each case
/// has a run in which the late join comes first in the DR-75 order of the waiters and still
/// fires second.
#[test]
fn set_cascade_window_is_dr75_for_a_guarded_waiter() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let set_runs = guarded_window_set_case(workers).await;
            let path_runs = guarded_window_pathmap_case(workers).await;
            assert!(
                set_runs > 0 && path_runs > 0,
                "F3, {workers} workers: a run of each case puts the late join first in DR-75 \
                 order (set {set_runs}, PathMap set {path_runs})"
            );
            println!(
                "F3, {workers} workers: the late join comes first in DR-75 order and fires second \
                 in {set_runs} set runs and in {path_runs} PathMap set runs of {PICK_RUNS}"
            );
        });
    }
}

// ─── CR-14: the DR-75 restage of R-A replays ───────────────────────────────────────────────────

/// Added by decision 124 (L1, plan-join-batch Step 7, rule CR-14): the stash of the restage
/// chain.
const RESTAGE_STASH: &str = "nor-restage";

/// Added by decision 124 (L1, rule CR-14): the constructor of the restage chain: each collection
/// holds three records at most.
const RESTAGE_CTOR: &str = r#""default"; capacity=3, dimensions=2"#;

/// Added by decision 124 (L1, rule CR-14): the records of `a` in the deploy that creates the
/// space of the restage chain: three records, so `a` is full, and a store on `a` fails with
/// `CapacityExceeded`.
const RESTAGE_FULL: &str = r#"a!("img"; embedding=[10.0, 0.0]) | a!("doc"; embedding=[10.0, 1.0]) |
a!("pdf"; embedding=[10.0, 2.0])"#;

/// Added by decision 124 (L1, rule CR-14): the partners of the restage chain on `b` as one
/// deploy: the values 1, 2 and 3, which the query `[1.0, 0.0]` ranks in this order.
const RESTAGE_PARTNERS: &str =
    "b!(1; embedding=[10.0, 0.0]) | b!(2; embedding=[10.0, 1.0]) | b!(3; embedding=[10.0, 2.0])";

/// Added by decision 124 (L1, rule CR-14): the records of [`RESTAGE_PARTNERS`].
const RESTAGE_PARTNER_COUNT: usize = 3;

/// Added by decision 124 (L1, rule CR-14): the list waiter of the restage chain: the top record
/// of `b` for the query `[1.0, 0.0]`, whose pattern binds `[2]` only. While 1 is stored on `b`
/// it refuses its list and parks. Once a fire takes 1, LIST-CASCADE fires it with 2.
const RESTAGE_LIST: &str = r#"for (@[2] <- b ~ ([1.0, 0.0]; k=1)) { out!(["list", 2]) }"#;

/// Added by decision 124 (L1, rule CR-14): the send of the restage chain: `over` on the full `a`.
const RESTAGE_SEND: &str = r#"a!("over"; embedding=[10.0, 0.5])"#;

/// Added by decision 124 (L1, rule CR-14): the guarded join of the restage chain: `over` on `a`
/// and any partner on `b` but `refused`. It parks while `a` holds no `over`. The channel of `a`
/// then holds a join group, so the send of `over` runs in the join engine.
fn restage_join(refused: i64) -> String {
    format!(
        r#"for (@"over" <- a & @y <- b where y != {refused}) {{ out!(["join", 1, "over", y]) }}"#
    )
}

/// Added by decision 124 (L1, rule CR-14): plays the restage chain up to its partners on
/// `chain`: the creation of the space with the full `a`, then [`RESTAGE_PARTNERS`] with the
/// first salt whose partners put 1 first in the canonical order of DR-75. So the DR-75 tuple of
/// the join takes 1, the top record of the list waiter. Returns the values on `b` in DR-75
/// order.
async fn salted_restage_partners(chain: &mut NativeChain, label: &str) -> Vec<i64> {
    let creation = stashing(
        "rho:space:hashmap:vectordb",
        RESTAGE_CTOR,
        RESTAGE_STASH,
        RESTAGE_FULL,
    );
    let creation_label = format!("{label}: the space");
    let created = play_certified(chain, &creation, &creation_label).await;
    assert_runs(&creation_label, &created);
    let partners = with_stash(RESTAGE_STASH, RESTAGE_PARTNERS);
    for salt in 0..SALTS {
        chain.reset(&created.root).await;
        let partners_label = format!("{label}: the partners, salt {salt}");
        let stored = chain.play(&partners, &partners_label, AMPLE_LIMIT).await;
        assert_runs(&partners_label, &stored);
        let values = stored_ints(&stored, RESTAGE_PARTNER_COUNT, vdb_value);
        if let Some(1) = values.first() {
            let certification = certify(&stored, &partners, AMPLE_LIMIT).await;
            assert_reproduces(&partners_label, &stored, &certification);
            return values;
        }
    }
    panic!("{label}: no salt below {SALTS} puts 1 first on b in DR-75 order");
}

/// Added by decision 124 (L1, plan-join-batch Step 7, rule CR-14): R-A in the join engine. The
/// restage chain fills `a`, stores the partners 1, `m` and `r` on `b` in DR-75 order, parks the
/// join that refuses `r` and the list waiter of `[2]`, and sends `over` on the full `a`
/// [`PICK_RUNS`] times. The offer of `over` meets the join, whose partner on `b` play picks at
/// random among three, outside the cascade, so the pick sets the pick flag (generic.rs
/// `pick_order`). A pick of 1 or `m` passes the guard, and the join fires. A pick of `r` fails
/// it, so the offer stores `over`, which fails with `CapacityExceeded`: a positional error
/// after a random pick (`Plan::restages`). R-A then restages the send in DR-75 order
/// (`restage_after_pick`): the join takes 1, its DR-75 tuple, and the cascade of the restage
/// lifts 2 and fires the list waiter. So every run commits: the join binds 1 and the cascade
/// fires the list waiter, or the join binds `m` and the list waiter stays parked. Each deploy
/// certifies through its own native replay. Returns the runs that bind 1 and the runs that bind
/// `m`.
async fn restage_case(workers: usize) -> (usize, usize) {
    let label = format!("CR-14, {workers} workers");
    let mut chain = NativeChain::new().await;
    let values = salted_restage_partners(&mut chain, &label).await;
    let [_, middle, refused] = values.as_slice() else {
        panic!("{label}: b holds three partners: {values:?}");
    };
    let join = with_stash(RESTAGE_STASH, &restage_join(*refused));
    let join_label = format!("{label}: the join");
    let joined = play_certified(&mut chain, &join, &join_label).await;
    assert_parks(&join_label, &joined);
    let list = with_stash(RESTAGE_STASH, RESTAGE_LIST);
    let list_label = format!("{label}: the list waiter");
    let listed = play_certified(&mut chain, &list, &list_label).await;
    assert_parks(&list_label, &listed);
    let mut waiting = Vec::<[u8; 32]>::with_capacity(2);
    waiting.extend(parked_waiters(&joined));
    waiting.extend(parked_waiters(&listed));
    let [join_waiter, list_waiter] = waiting.as_slice() else {
        panic!(
            "{label}: the join and the list waiter park: {}",
            waiting.len()
        );
    };
    let waiters: BTreeSet<[u8; 32]> = waiting.iter().copied().collect();
    let send = with_stash(RESTAGE_STASH, RESTAGE_SEND);
    let (mut tops, mut middles) = (0_usize, 0_usize);
    for run in 0..PICK_RUNS {
        chain.reset(&listed.root).await;
        let send_label = format!("{label}: the send, run {run}");
        let played = play_certified(&mut chain, &send, &send_label).await;
        assert_runs(&send_label, &played);
        let outputs = join_outputs(&played);
        let (x, y) = outputs
            .get(&1)
            .unwrap_or_else(|| panic!("{send_label}: the join delivers"));
        assert_eq!(string_of(x), "over", "{send_label}: the join binds over");
        let (_, journal) = journal_of(&played);
        let fires = waiter_fires(journal, &waiters);
        match int_of(y) {
            1 => {
                tops += 1;
                let [join_fire, list_fire] = fires.as_slice() else {
                    panic!(
                        "{send_label}: the join and the list waiter fire: {}",
                        fires.len()
                    );
                };
                assert_eq!(
                    (&join_fire.waiter, &list_fire.waiter),
                    (join_waiter, list_waiter),
                    "{send_label}: the offer fires the join, then the cascade fires the list"
                );
                assert_lifted(&send_label, journal, std::slice::from_ref(list_fire));
                assert_eq!(
                    deliveries(&played, "list"),
                    1,
                    "{send_label}: the list waiter delivers 2"
                );
            }
            value if value == *middle => {
                middles += 1;
                let [join_fire] = fires.as_slice() else {
                    panic!("{send_label}: the join fires alone: {}", fires.len());
                };
                assert_eq!(
                    &join_fire.waiter, join_waiter,
                    "{send_label}: the offer fires the join"
                );
                assert_eq!(
                    deliveries(&played, "list"),
                    0,
                    "{send_label}: the list waiter stays parked while 1 leads b"
                );
            }
            value => panic!("{send_label}: the join binds {value}, which its guard refuses"),
        }
    }
    (tops, middles)
}

/// Added by decision 124 (L1, plan-join-batch Step 7, rule CR-14; PLAN:1202): the DR-75 restage
/// of R-A replays, and its random pick lies outside the cascade (`restage_case`). The case runs
/// on each runtime of `RACING_WORKERS`, and each deploy certifies through its own native replay.
/// Non-vacuity: on each runtime a run binds `m`, so play picks the partner at random, and a run
/// binds 1, so the cascade of a staging fires. No run fails, although a pick of the refused
/// partner has a chance of 1/3 in each run and fails at the capacity of `a`: R-A restages it.
/// Without R-A such a run fails with `CapacityExceeded`, and the chance that no run of
/// [`PICK_RUNS`] picks the refused partner is `(2/3)^64`.
#[test]
fn cascade_r_a_canonical_restage_replays() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let (tops, middles) = restage_case(workers).await;
            assert!(
                tops > 0 && middles > 0,
                "CR-14, {workers} workers: runs bind 1 ({tops}) and the middle partner \
                 ({middles})"
            );
            println!(
                "CR-14, {workers} workers: {tops} runs bind 1 and fire the cascade, {middles} \
                 runs bind the middle partner, of {PICK_RUNS}; no run fails"
            );
        });
    }
}

// ─── T-C4-40: a Stored send row skips a vetoing waiter (E3-a H2) ───────────────────────────────

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3, plan-join-batch Step 7): the
/// role of a deploy in a chain of T-C4-40. The owner creates the space, stores the records and
/// parks the waiters, and the client sends the element whose offer the test checks.
#[derive(Clone, Copy)]
enum Role {
    Owner,
    Client,
    /// Added by decision 124 (ruling RADD3 1.1): the client of a chain whose owner has budget 0.
    /// Each COMM of its deploys that holds an owner region fails its item test (R5).
    UnfundedClient,
}

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): a chain of native deploys that
/// certifies each deploy through its own native replay: [`NativeChain`] without owner funding,
/// and [`OwnedChain`] with it.
trait CertifiedChain {
    /// Plays `term` as a deploy of `role` with the random state of `label`, and certifies the
    /// play through its own native replay.
    async fn deploy(&mut self, role: Role, term: &str, label: &str) -> Played;

    /// Resets the chain to the root `root`.
    async fn rewind(&mut self, root: &Blake2b256Hash);
}

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): without owner funding, one
/// signer plays every role, as in the other tests of this suite.
impl CertifiedChain for NativeChain {
    async fn deploy(&mut self, _role: Role, term: &str, label: &str) -> Played {
        play_certified(self, term, label).await
    }

    async fn rewind(&mut self, root: &Blake2b256Hash) { self.reset(root).await }
}

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): the signer of the owner.
fn owner_signer() -> Sig { Sig::Ground(vec![0xA0, 1]) }

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): the signer of the client.
fn client_signer() -> Sig { Sig::Ground(vec![0xA0, 2]) }

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): the budget of the owner. It
/// pays for every fire of the chains of T-C4-40.
const OWNER_BUDGET: u64 = u64::MAX / 4;

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): the purse locator of the
/// owner chain: each signature has one purse, whose custody is the lane of the signature.
fn single_purse(signature: &CostSignature) -> Option<OwnerPurseSet> {
    let custody = cost_signature_to_sig(signature).ok()?.lane_hash();
    Some(OwnerPurseSet::Single(OwnerPurse {
        custody,
        signature: signature.clone(),
    }))
}

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): the owner binding of a deploy
/// of the client. Its client lane is the lane of the client, and its table lists the owner with
/// [`OWNER_BUDGET`]. So the records, the waiters and the bodies of the owner are owner regions,
/// which the owner pays for (DR-135). Each play and each replay builds its own binding, as each
/// validator does.
fn owner_binding() -> Arc<OwnerFundingBinding> { owner_binding_with(OWNER_BUDGET) }

/// Added by decision 124 (ruling RADD3 1.1): the owner binding of a deploy of the client, as
/// [`owner_binding`] describes it, with `budget` for the owner in its table.
/// [`Role::UnfundedClient`] plays with budget 0.
fn owner_binding_with(budget: u64) -> Arc<OwnerFundingBinding> {
    let owner = owner_signer();
    let entry = OwnerTableEntry {
        signature: sig_to_cost_signature(&owner).expect("a ground signature"),
        budget,
    };
    let table = OwnerTable::new(vec![(owner.lane_hash(), entry)], 1).expect("one owner");
    Arc::new(OwnerFundingBinding::new(
        [client_signer().lane_hash()].into_iter().collect(),
        single_purse,
        table,
    ))
}

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): the signer and the owner
/// binding of `role`. The owner plays without a binding, as the owner of a record does when it
/// stores the record, and the client plays with the binding.
fn signer_of(role: Role) -> (Sig, Option<Arc<OwnerFundingBinding>>) {
    match role {
        Role::Owner => (owner_signer(), None),
        Role::Client => (client_signer(), Some(owner_binding())),
        // Added by decision 124 (ruling RADD3 1.1): the client of an owner with budget 0.
        Role::UnfundedClient => (client_signer(), Some(owner_binding_with(0))),
    }
}

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): the native config of a deploy
/// with the owner binding `binding`, at the native phlo limit `limit`.
fn funded_config(binding: Option<Arc<OwnerFundingBinding>>, limit: u64) -> NativeRuntimeConfig {
    match binding {
        Some(binding) => config(limit).with_owner_funding(binding),
        None => config(limit),
    }
}

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): the pars of each datum on
/// [`OUT`] in the state of `runtime`, in the order of their Debug forms, as [`Played::out`] holds
/// them.
async fn out_data(runtime: &RhoRuntimeImpl) -> Vec<Vec<Par>> {
    let channel = new_gstring_par(OUT.to_string(), Vec::new(), false);
    let data = runtime.get_data(&channel).await;
    let mut values = Vec::<Vec<Par>>::with_capacity(data.len());
    values.extend(data.into_iter().map(|datum| datum.a.pars));
    values.sort_by_cached_key(|pars| format!("{pars:?}"));
    values
}

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): a chain of native deploys with
/// owner funding on one runtime. The owner and the client sign their own deploys.
struct OwnedChain {
    runtime: RhoRuntimeImpl,
    space: PlaySpace,
}

impl OwnedChain {
    /// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): a chain over the empty
    /// state.
    async fn new() -> Self {
        let (runtime, space) = fresh_space_runtime_with(&mut Vec::new()).await;
        Self { runtime, space }
    }

    /// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): plays `term` for `role`
    /// with the random state of `label` at the native phlo limit `limit`, as `NativeChain::play`
    /// plays a deploy (`play_on` of `support/native.rs`), with the signer and the owner binding
    /// of `role`.
    async fn play(&mut self, role: Role, term: &str, label: &str, limit: u64) -> Played {
        let (signer, binding) = signer_of(role);
        self.runtime
            .cost
            .set_deploy_signature_funded(DEPLOY, signer);
        let random = Blake2b512Random::create_from_bytes(label.as_bytes());
        let history = self.space.get_history_repository();
        let pre_root = self.runtime.get_root().await;
        let result = self
            .runtime
            .evaluate_with_native_phlo(
                term,
                Injections::new(),
                random.clone(),
                None,
                funded_config(binding, limit),
            )
            .await
            .expect("native play evaluates");
        let soft = self.runtime.create_soft_checkpoint().await;
        let log = soft.log.into_iter().collect();
        let out = out_data(&self.runtime).await;
        let mut outputs = Vec::<String>::with_capacity(out.len());
        outputs.extend(out.iter().map(|pars| format!("{pars:?}")));
        outputs.sort_unstable();
        let residue = hot_store_residue(&soft.cache_snapshot, Durability::Ephemeral);
        let durable = hot_store_residue(&soft.cache_snapshot, Durability::Durable);
        let snapshot_keys = snapshot_keys(&soft.cache_snapshot);
        let root = self.runtime.create_checkpoint().await.root;
        Played {
            result,
            log,
            history,
            root,
            outputs,
            out,
            random,
            residue,
            durable,
            injections: Injections::new(),
            pre_root,
            snapshot_keys,
        }
    }

    /// Added by decision 124 (plan of the batch B tests, B1): plays `term` for `role` at the
    /// native phlo limit `limit`, and certifies the play through its own native replay at that
    /// limit (rule CR-28). A failed deploy certifies too: replay must fail it with the errors of
    /// play.
    async fn deploy_limited(&mut self, role: Role, term: &str, label: &str, limit: u64) -> Played {
        let played = self.play(role, term, label, limit).await;
        let certification = certify_owned(&played, term, role, limit).await;
        assert_reproduces(label, &played, &certification);
        played
    }
}

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): certifies `played`, a deploy
/// of `role` at the native phlo limit `limit`, through its committed evidence, as `certify` of
/// `support/native.rs` does, with the signer and the owner binding of `role`: the journal check
/// with the owners of the binding (`check_operation_journal_with_owners`), and a replay budget
/// with the binding.
async fn certify_owned(played: &Played, term: &str, role: Role, limit: u64) -> Certification {
    let (recording, journal) = committed_evidence(played);
    let (signer, binding) = signer_of(role);
    let trace = with_contract(limit, |contract| {
        contract.check_operation_journal_with_owners(
            recording.session,
            &recording,
            journal,
            journal_limits(),
            binding.as_deref(),
        )
    })
    .expect("the journal check accepts the journal of play")
    .bind_trace(played.log.clone().into(), trace_bind_limits())
    .expect("the trace binding accepts the log of play");
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(DEPLOY, signer);
    budget
        .reset_for_native_execution(funded_config(binding, limit))
        .expect("the replay budget enters native execution");
    let environment = create_native_replay_env_with_logger(
        trace,
        played.history.clone(),
        Arc::new(Box::new(Matcher)),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut Vec::new(),
        budget,
        ExternalServices::noop(),
        None,
        FileHandleTable::new(),
    )
    .await
    .expect("the native replay environment builds");
    finish_injected(
        environment,
        term,
        played.random.clone(),
        played.injections.clone(),
    )
    .await
}

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): with owner funding, the owner
/// and the client sign their own deploys, and the client plays and replays with the binding.
impl CertifiedChain for OwnedChain {
    async fn deploy(&mut self, role: Role, term: &str, label: &str) -> Played {
        self.deploy_limited(role, term, label, AMPLE_LIMIT).await
    }

    async fn rewind(&mut self, root: &Blake2b256Hash) {
        self.runtime
            .reset(root)
            .await
            .expect("reset the owned chain to the root");
    }
}

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): the stash of the chains of
/// T-C4-40.
const VETO_STASH: &str = "nor-veto";

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): the partners of the join
/// chains on `b` as one deploy: the values 1 and 2.
const VETO_PARTNERS: &str = "b!(1) | b!(2)";

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): the records of
/// [`VETO_PARTNERS`].
const VETO_PARTNER_COUNT: usize = 2;

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): the send of the join chains:
/// the element 5 on `a`.
const VETO_SEND: &str = "a!(5)";

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): the vetoing waiters of the
/// join chains as one deploy: the join of `x` on `a` and `y` on `b` with the guard `y == lead`,
/// and the one-bind waiter of `v` on `a` with the guard `v == 7`. The commit of each may veto
/// (E3-a, `Cx::may_veto`), and both park while `a` is empty.
fn vetoing_waiters(lead: i64) -> String {
    format!(
        r#"for (@x <- a & @y <- b where y == {lead}) {{ out!(["join", 1, x, y]) }} |
for (@v <- a where v == 7) {{ out!(["one", v]) }}"#
    )
}

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): the runs of a chain of
/// T-C4-40 whose offer stores the element of the send, and the runs whose offer fires a waiter.
struct VetoRuns {
    stored: usize,
    fired: usize,
}

/// Added by plan v4 errata (V4-18, T-C4-40) and decision 124 (L3): the row of the offer of a
/// deploy that sends one element on a space: its first space produce row. It has a COMM when the
/// offer fires a waiter, and none when the offer stores the element (a Stored row).
fn offer_row(journal: &[NativeOperationRecord]) -> &NativeOperationRecord {
    journal
        .iter()
        .find(|row| matches!(row.source, NativeOperationSource::Produce(_)) && is_space_row(row))
        .expect("the send stages a space produce row")
}

/// Added by plan v4 errata (V4-18, T-C4-40, H2) and decision 124 (L3): the join chain on a bag
/// or a set of the family `urn` on `chain`, run [`PICK_RUNS`] times. The owner creates the
/// space, stores the partners 1 and 2 on `b`, and parks the vetoing waiters, whose join admits
/// `lead`, the first partner in DR-75 order. The records of a bag have the datum envelope of a
/// set ([`set_value`]). Each run sends 5 on `a` as the client. The channel of `a` holds the
/// join, so the send runs in the join engine, and its offer meets the join and the one-bind
/// waiter in a random order. The join picks its partner at random: a pick of `lead` passes its
/// guard, and the join fires. A pick of the other partner fails it, the one-bind waiter refuses
/// 5, and the offer stores 5, while the DR-75 tuple `(5, lead)` passes the guard. Replay of that
/// Stored row skips each waiter whose commit may veto, before its search (E3-a, the skip arm of
/// `Engine::offer`), so replay stores 5 too. Without the skip, replay would fire the join with
/// the DR-75 tuple at the offer. On a set (`cascade`), the cascade of the same transaction then
/// fires the join with the DR-75 tuple, after the `K_lift` of 5 (a bag has no cascade,
/// `Engine::cascade_order`), so 5 stays stored on a bag only. Returns the runs whose offer stores
/// 5 and the runs whose offer fires the join.
async fn vetoing_join_case<C: CertifiedChain>(
    chain: &mut C,
    urn: &str,
    label: &str,
    cascade: bool,
) -> VetoRuns {
    let creation = stashing(urn, r#""default""#, VETO_STASH, "Nil");
    let creation_label = format!("{label}: the space");
    let created = chain.deploy(Role::Owner, &creation, &creation_label).await;
    assert_runs(&creation_label, &created);
    let partners = with_stash(VETO_STASH, VETO_PARTNERS);
    let partners_label = format!("{label}: the partners");
    let stored = chain.deploy(Role::Owner, &partners, &partners_label).await;
    assert_runs(&partners_label, &stored);
    let lead = *stored_ints(&stored, VETO_PARTNER_COUNT, set_value)
        .first()
        .expect("b holds the partners");
    let waiters = with_stash(VETO_STASH, &vetoing_waiters(lead));
    let waiters_label = format!("{label}: the waiters");
    let parked = chain.deploy(Role::Owner, &waiters, &waiters_label).await;
    assert_parks(&waiters_label, &parked);
    let waiters = parked_waiters(&parked);
    assert_eq!(
        waiters.len(),
        2,
        "{waiters_label}: the join and the one-bind waiter park"
    );
    let send = with_stash(VETO_STASH, VETO_SEND);
    let mut runs = VetoRuns {
        stored: 0,
        fired: 0,
    };
    for run in 0..PICK_RUNS {
        chain.rewind(&parked.root).await;
        let send_label = format!("{label}: the send, run {run}");
        let played = chain.deploy(Role::Client, &send, &send_label).await;
        assert_runs(&send_label, &played);
        assert_eq!(
            deliveries(&played, "one"),
            0,
            "{send_label}: the one-bind waiter refuses 5"
        );
        let (_, journal) = journal_of(&played);
        let fires = waiter_fires(journal, &waiters);
        let joined = join_outputs(&played)
            .get(&1)
            .map(|(x, y)| (int_of(x), int_of(y)));
        match (&offer_row(journal).comm, cascade) {
            (Some(_), _) => {
                assert_eq!(
                    (fires.len(), joined),
                    (1, Some((5, lead))),
                    "{send_label}: the offer fires the join with the partner that its guard admits"
                );
                runs.fired += 1;
            }
            (None, false) => {
                assert_eq!(
                    (space_comms(journal), space_stores(journal), joined),
                    (0, 1, None),
                    "{send_label}: the offer stores 5, and 5 stays stored"
                );
                runs.stored += 1;
            }
            (None, true) => {
                assert_eq!(
                    (fires.len(), joined),
                    (1, Some((5, lead))),
                    "{send_label}: the offer stores 5, and the cascade fires the join with the \
                     DR-75 tuple"
                );
                assert_lifted(&send_label, journal, &fires);
                runs.stored += 1;
            }
        }
    }
    runs
}

/// Added by decision 124 (L3, J1 ruling 1, T-C4-40): the constructor of the VectorDB chain of
/// T-C4-40, with room for its records.
const VETO_LIST_CTOR: &str = r#""default"; capacity=8, dimensions=2"#;

/// Added by decision 124 (L3, J1 ruling 1, T-C4-40): the records of the VectorDB chain in the
/// deploy that creates its space: `s` on `a`, and the partner 1 on `b`.
const VETO_LIST_RECORDS: &str = r#"a!("s"; embedding=[10.0, 0.0]) | b!(1; embedding=[10.0, 0.0])"#;

/// Added by decision 124 (L3, J1 ruling 1, T-C4-40): the waiters of the VectorDB chain as one
/// deploy. The join of `none` on `a` and any partner on `b` never fires, and its group puts a
/// join on the channel of `a`, so a send on `a` runs in the join engine. The guarded list waiter
/// lists the top two records of `a` for the query `[1.0, 0.0]` and admits a list led by `s`.
/// While `a` holds `s` alone, its list has one member, which its pattern refuses, so it parks.
const VETO_LIST_WAITERS: &str = r#"for (@"none" <- a & @y <- b) { out!(["join", 0, "none", y]) } |
for (@[v, w] <- a ~ ([1.0, 0.0]; k=2) where v == "s") { out!(["list", v, w]) }"#;

/// Added by decision 124 (L3, J1 ruling 1, T-C4-40): the send of the VectorDB chain: `x`, whose
/// score ties with the score of `s`.
const VETO_LIST_SEND: &str = r#"a!("x"; embedding=[10.0, 0.0])"#;

/// Added by decision 124 (L3, J1 ruling 1, T-C4-40): the VectorDB chain on `chain`, run
/// [`PICK_RUNS`] times. The owner stores `s` and the partner, and parks the join and the guarded
/// list waiter. Each run sends `x` as the client, in the join engine. The scores of `s` and `x`
/// tie, so the list of the waiter is `[s, x]` or `[x, s]`: a list keeps the canonical order of
/// DR-75 in play too (`vectordb::list_selection`, `EligibleOrder::Canonical`), and the source of
/// `x`, which the label of the run gives, decides the tie. With `[s, x]` the guard passes and the
/// list waiter fires. With `[x, s]` it fails, the join refuses `x`, and the offer stores `x`.
/// Replay of that Stored row skips the list waiter, whose commit may veto, before its search
/// (E3-a, the skip arm of `Engine::offer`), and stores `x` too. Returns the runs that store `x`
/// and the runs that fire the list waiter.
async fn vetoing_list_case<C: CertifiedChain>(chain: &mut C, label: &str) -> VetoRuns {
    let creation = stashing(
        "rho:space:hashmap:vectordb",
        VETO_LIST_CTOR,
        VETO_STASH,
        VETO_LIST_RECORDS,
    );
    let creation_label = format!("{label}: the records");
    let created = chain.deploy(Role::Owner, &creation, &creation_label).await;
    assert_runs(&creation_label, &created);
    let waiters = with_stash(VETO_STASH, VETO_LIST_WAITERS);
    let waiters_label = format!("{label}: the waiters");
    let parked = chain.deploy(Role::Owner, &waiters, &waiters_label).await;
    assert_parks(&waiters_label, &parked);
    assert_eq!(
        parked_waiters(&parked).len(),
        2,
        "{waiters_label}: the join and the list waiter park"
    );
    let send = with_stash(VETO_STASH, VETO_LIST_SEND);
    let mut runs = VetoRuns {
        stored: 0,
        fired: 0,
    };
    for run in 0..PICK_RUNS {
        chain.rewind(&parked.root).await;
        let send_label = format!("{label}: the send, run {run}");
        let played = chain.deploy(Role::Client, &send, &send_label).await;
        assert_runs(&send_label, &played);
        assert_eq!(
            deliveries(&played, "join"),
            0,
            "{send_label}: the join never fires"
        );
        let (_, journal) = journal_of(&played);
        match (&offer_row(journal).comm, deliveries(&played, "list")) {
            (None, 0) => {
                assert_eq!(
                    (space_comms(journal), space_stores(journal)),
                    (0, 1),
                    "{send_label}: the offer stores x, and x stays stored"
                );
                runs.stored += 1;
            }
            (Some(_), 1) => {
                let listed: Vec<&str> = played
                    .out
                    .iter()
                    .filter_map(|datum| match datum.as_slice() {
                        [value] => Some(list_items(value)),
                        _ => None,
                    })
                    .filter_map(|items| match items {
                        [kind, v, w] if string_of(kind) == "list" => {
                            Some([string_of(v), string_of(w)])
                        }
                        _ => None,
                    })
                    .flatten()
                    .collect();
                assert_eq!(
                    listed,
                    ["s", "x"],
                    "{send_label}: the list waiter fires with the list that its guard admits"
                );
                runs.fired += 1;
            }
            (row, count) => panic!(
                "{send_label}: the offer row has a COMM ({}), and the list waiter delivers {count} \
                 times",
                row.is_some()
            ),
        }
    }
    runs
}

/// Added by plan v4 errata (V4-18, T-C4-40, arbiter-v4-17-18-B H2) and decision 124 (L3 and J1
/// ruling 1; PLAN:1203): a Stored send row skips a vetoing join waiter, and every other waiter
/// whose commit may veto. The cases: a bag and a set, each with a guarded join and a guarded
/// one-bind waiter (`vetoing_join_case`), and a VectorDB space with a guarded list waiter on a
/// collection whose channel also holds a join, with tied scores (`vetoing_list_case`). Each case
/// runs on each runtime of `RACING_WORKERS`, without owner funding ([`NativeChain`]) and with it
/// ([`OwnedChain`]), and each deploy certifies through its own native replay. Non-vacuity: in
/// each case, on each runtime and with each funding, the offer of a run stores the element (a
/// Stored row, which replay meets with the skip) and the offer of a run fires. On the bag and
/// the set each such Stored row stores 5 while the DR-75 tuple `(5, lead)` passes the guard of
/// the join.
#[test]
fn stored_send_row_skips_a_vetoing_join_waiter() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let mut tallies = Vec::<String>::with_capacity(6);
            for (urn, family, cascade) in [
                ("rho:space:hashmap:bag", "bag", false),
                ("rho:space:hashmap:set", "set", true),
            ] {
                let label = format!("T-C4-40 ({family}), {workers} workers");
                let plain =
                    vetoing_join_case(&mut NativeChain::new().await, urn, &label, cascade).await;
                let owned_label = format!("{label}, owner funding");
                let mut owned_chain = OwnedChain::new().await;
                let owned = vetoing_join_case(&mut owned_chain, urn, &owned_label, cascade).await;
                for (funding, runs) in [("without", &plain), ("with", &owned)] {
                    assert!(
                        runs.stored > 0 && runs.fired > 0,
                        "{label}, {funding} owner funding: runs store 5 ({}) and fire ({})",
                        runs.stored,
                        runs.fired
                    );
                    tallies.push(format!(
                        "{family} {funding} owner funding: {} stored, {} fired",
                        runs.stored, runs.fired
                    ));
                }
            }
            let label = format!("T-C4-40 (VectorDB), {workers} workers");
            let plain = vetoing_list_case(&mut NativeChain::new().await, &label).await;
            let owned_label = format!("{label}, owner funding");
            let owned = vetoing_list_case(&mut OwnedChain::new().await, &owned_label).await;
            for (funding, runs) in [("without", &plain), ("with", &owned)] {
                assert!(
                    runs.stored > 0 && runs.fired > 0,
                    "{label}, {funding} owner funding: runs store x ({}) and fire ({})",
                    runs.stored,
                    runs.fired
                );
                tallies.push(format!(
                    "VectorDB {funding} owner funding: {} stored, {} fired",
                    runs.stored, runs.fired
                ));
            }
            println!(
                "T-C4-40, {workers} workers, {PICK_RUNS} runs each: {}",
                tallies.join("; ")
            );
        });
    }
}

// ─── V4-12: a settle job commits its anchor alone and replays ──────────────────────────────────

/// Added by plan v4 errata (V4-12) and decision 124 (L1, plan-join-batch Step 7): the stash of the
/// anchor-alone chain.
const V412_STASH: &str = "nor-v412";

/// Added by plan v4 errata (V4-12) and decision 124 (L1, plan-join-batch Step 7): the constructor
/// of the anchor-alone chain: each collection holds two records at most.
const V412_CTOR: &str = r#""default"; capacity=2, dimensions=2"#;

/// Added by plan v4 errata (V4-12) and decision 124 (L1, plan-join-batch Step 7): the records of
/// `a` in the deploy that creates the space of the anchor-alone chain: `r` and `f`, so `a` is full.
/// No send of the chain has a vector, and no bind has a query.
const V412_RECORDS: &str = r#"a!("r") | a!("f")"#;

/// Added by plan v4 errata (V4-12) and decision 124 (L1, plan-join-batch Step 7): the produce of
/// the anchor-alone chain: `x` on `a`.
const V412_PRODUCE: &str = r#"a!("x")"#;

/// Added by plan v4 errata (V4-12) and decision 124 (L1, plan-join-batch Step 7): `count` peek
/// joins of `r`, `x` and `f` on `a` as one deploy body. The normalizer refuses a join that names
/// one channel twice in its text, so the contract `mk` names `a` three times through its
/// parameters. Each join parks while `a` holds no `x`.
fn peek_joins_on_a(count: usize) -> String {
    const HEAD: &str = r#"new mk in {
  contract mk(c1, c2, c3) = { for (@"r" <<- c1 & @"x" <<- c2 & @"f" <<- c3) { Nil } } |
  "#;
    const TAIL: &str = "\n}";
    let calls = vec!["mk!(*a, *a, *a)"; count].join(" | ");
    let mut term = String::with_capacity(HEAD.len() + calls.len() + TAIL.len());
    term.push_str(HEAD);
    term.push_str(&calls);
    term.push_str(TAIL);
    term
}

/// Added by plan v4 errata (V4-12) and decision 124 (L1, plan-join-batch Step 7): a chain of three
/// deploys on a VectorDB space of capacity 2. The first stores `r` and `f`, the second parks
/// [`FIRE_ROUND_CAP`] peek joins of `r`, `x` and `f` on `a`, and the third sends `x`. Each fire
/// removes two records and owes them, and the produce stops at the cap with `r`, `x` and `f` owed.
/// Its settle job stages its anchor first, stores `r` and `x`, and fails on `f` with
/// `CapacityExceeded`. So it commits the anchor alone, and native replay meets the error at Free,
/// after a miss. Each runtime of `RACING_WORKERS` plays the chain once. No pick of play changes the
/// outcome, because the joins differ only in their sources and each pattern binds one record. Each
/// deploy certifies through its own native replay, the failed produce included.
#[test]
fn cascade_anchor_alone_failure_replays_in_dr75() {
    let records = stashing(
        "rho:space:hashmap:vectordb",
        V412_CTOR,
        V412_STASH,
        V412_RECORDS,
    );
    let waiters = with_stash(V412_STASH, &peek_joins_on_a(FIRE_ROUND_CAP));
    let produce = with_stash(V412_STASH, V412_PRODUCE);
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("the anchor-alone chain, {workers} workers");
            let mut chain = NativeChain::new().await;
            store_and_park(&mut chain, &label, &records, &waiters).await;
            let played = play_certified(&mut chain, &produce, "nor-v412-produce").await;
            let (_, journal) = journal_of(&played);
            let errors = format!("{:?}", played.result.errors);
            let anchors = anchor_indices(journal);
            // The evidence of the play comes before the checks, so a failed check shows it. The
            // journal splits at the first anchor, or stays whole when no row anchors.
            let first = anchors.first().copied().unwrap_or(journal.len());
            let (before, from_anchor) = journal.split_at(first);
            let comms_before = space_comms(before);
            let after: Vec<&NativeOperationSource> = from_anchor
                .iter()
                .skip(1)
                .filter(|row| is_space_row(row))
                .map(|row| &row.source)
                .collect();
            let stores = space_stores(journal);
            println!(
                "{label}: anchors {anchors:?}, space rows after the first anchor {after:?}, space \
                 COMMs before it {comms_before}, space stores {stores}, native phlo usage {:?}, \
                 errors {errors}",
                played.result.native_phlo_usage
            );
            assert!(
                errors.contains("CapacityExceeded"),
                "{label}: the settle job fails on f at the capacity of a: {errors}"
            );
            let [anchor] = anchors.as_slice() else {
                panic!("{label}: only the settle job anchors, so one anchor: {anchors:?}");
            };
            assert!(
                after.is_empty(),
                "{label}: the anchor at {anchor} commits alone, but space rows follow it: \
                 {after:?}"
            );
            assert_eq!(
                comms_before, FIRE_ROUND_CAP,
                "{label}: the produce fires FIRE_ROUND_CAP rounds before the anchor"
            );
            assert_eq!(
                stores,
                2 * (FIRE_ROUND_CAP - 1),
                "{label}: r and x store again after each fire but the last, and the settle job \
                 stores nothing"
            );
        });
    }
}

/// Added by decision 124 (ADD A4, plan of the batch B tests, B2): the chain of the full cell. The
/// owner parks a join of `r` on `a` and any record on `b`, so each operation on `a` runs in the
/// join engine. The client stores `r` on `a`, and then `y` on `b`. The owner has budget 0 in the
/// binding of the client, so the commit refuses the fire of the join with the pair `(r, y)`, and
/// `y` stores.
const FULL_CELL_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:cell",
    ctor: r#""default""#,
    client: "Nil",
    steps: &[
        (
            Role::Owner,
            r#"for (@"r" <- a & @y <- b) { out!(["pair", y]) }"#,
        ),
        (Role::UnfundedClient, r#"a!("r")"#),
        (Role::UnfundedClient, r#"b!("y")"#),
    ],
};

/// Added by decision 124 (ADD A4, plan of the batch B tests, B2, rule CR-32, errata V4-12): the
/// miss rule of T-C4-27 on a cell. The client sends `x` on the full `a`. The settle of the join
/// engine before its plan offers the stored pair `(r, y)` to the join of the owner, and the
/// commit refuses the fire, so the row deviates and Tail(actual) is Stop. The Stop owes the
/// cascade, so a settle job anchors first, takes the counter, and skips the booked pair. The
/// settle job re-issues the send of `x` with `OwnerRefusal`, which anchors first, takes the
/// counter, and fails to store `x` in the full slot with `CellOccupied`. So the re-issue commits
/// its anchor alone (`prepare_failure`). Native replay follows the anchor, misses at the take of
/// the counter (`Free`), meets the same error, and commits the anchor alone too. Each runtime of
/// `RACING_WORKERS` plays the chain once: a cell shows a join its slot only, so play picks
/// nothing at random.
#[test]
fn cell_owner_refusal_reissue_on_a_full_slot_commits_the_anchor_alone() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("the full cell chain, {workers} workers");
            let mut chain = OwnedChain::new().await;
            let setup = refusal_setup(&mut chain, &label, &FULL_CELL_CHAIN).await;
            let trigger = format!("{label}: the send of x");
            let played = refusal_trigger(&mut chain, &setup.root, r#"a!("x")"#, &trigger).await;
            assert_anchor_alone(&trigger, &played, "CellOccupied", 1);
            assert_eq!(
                deliveries(&played, "pair"),
                0,
                "{trigger}: the owner refuses every fire of its join"
            );
        });
    }
}

// ─── T-P-1: owner-funded client deploys on each kind of space ──────────────────────────────────

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the client deploys of each
/// chain of T-P-1. A run of the test plays 24 chains (8 kinds on 3 runtimes), so 768 of its 843
/// deploys are client deploys. Only the client deploys play with the owner binding, so they hold
/// most of the time of a run, and a run lasts long enough for a stable mean of `hyperfine`.
const WORKLOAD_RUNS: usize = 32;

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the stash of the chains of
/// T-P-1.
const WORKLOAD_STASH: &str = "nor-workload";

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): a chain of T-P-1 on one
/// space. The owner creates the space, plays each deploy of `records` in order, and parks the
/// waiters of `waiters`. Each run then rewinds to the parked state, and the client plays `client`
/// with the owner binding, so the owner pays for the fires of its records and of its waiters
/// (DR-135). No channel of a chain holds a join, so each operation runs in the plan of its kind,
/// outside the join engine (`join::classify`).
struct WorkloadKind {
    /// The kind of the space in the labels of the chain.
    name: &'static str,
    /// The URN of the family of the space.
    urn: &'static str,
    /// The constructor of the space.
    ctor: &'static str,
    /// The deploys of the owner that store the records, in order.
    records: &'static [&'static str],
    /// The deploy of the owner that parks the waiters.
    waiters: &'static str,
    /// The waiters that `waiters` parks.
    parked: usize,
    /// The deploy of the client.
    client: &'static str,
    /// The fires of a body in each client deploy. Each body delivers one value on [`OUT`].
    fires: usize,
    /// The space COMMs of each client deploy.
    comms: usize,
    /// The waiters that each client deploy parks.
    parks: usize,
}

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): four records on `b`, the
/// records of the queue and of the bag.
const WORKLOAD_RECORDS: &str = "b!(1) | b!(2) | b!(3) | b!(4)";

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): four waiters on `a`. The
/// queue, the priority queue, the bag and the set park them.
const WORKLOAD_WAITERS: &str = r#"for (@x <- a) { out!(["a", 1, x]) } |
for (@x <- a) { out!(["a", 2, x]) } |
for (@x <- a) { out!(["a", 3, x]) } |
for (@x <- a) { out!(["a", 4, x]) }"#;

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the waiters of the stack:
/// four waiters on `a`, as [`WORKLOAD_WAITERS`], and the waiter of 1 on `b`. The top of `b` is 2
/// while the owner parks them, so the waiter of 1 parks.
const STACK_WAITERS: &str = r#"for (@x <- a) { out!(["a", 1, x]) } |
for (@x <- a) { out!(["a", 2, x]) } |
for (@x <- a) { out!(["a", 3, x]) } |
for (@x <- a) { out!(["a", 4, x]) } |
for (@1 <- b) { out!(["settled", 1]) }"#;

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the client deploy of the
/// stack: four sends on `a`, and the receive of 2, the top of `b`. The settle after the receive
/// lifts 1, the new top, and offers it to the waiter of 1.
const STACK_CLIENT: &str = r#"a!(1) | a!(2) | a!(3) | a!(4) | for (@2 <- b) { out!(["b", 2]) }"#;

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the client deploy of the
/// queue: four sends on `a`, each of which fires a waiter of the owner, and four receives on `b`,
/// each of which takes a record of the owner.
const QUEUE_CLIENT: &str = r#"a!(1) | a!(2) | a!(3) | a!(4) |
for (@x <- b) { out!(["b", 1, x]) } |
for (@x <- b) { out!(["b", 2, x]) } |
for (@x <- b) { out!(["b", 3, x]) } |
for (@x <- b) { out!(["b", 4, x]) }"#;

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the client deploy of the
/// cell: a send on `a`, which fires the waiter of the owner, and a receive on `b`, which takes the
/// record of the owner.
const CELL_CLIENT: &str = r#"a!(1) | for (@x <- b) { out!(["b", 1, x]) }"#;

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the records of the priority
/// queue: four records on `b` in the levels 0 and 1 and in the unprioritized level.
const PRIORITY_RECORDS: &str = "b!(1; priority=0) | b!(2; priority=1) | b!(3) | b!(4; priority=0)";

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the client deploy of the
/// priority queue: four sends on `a` in the levels 0 and 1 and in the unprioritized level, which
/// fire the waiters of the owner in the unrestricted lane, and four receives on `b`, which take
/// the heads of the levels in ascending level order.
const PRIORITY_CLIENT: &str = r#"a!(1) | a!(2; priority=0) | a!(3; priority=1) | a!(4) |
for (@x <- b) { out!(["b", 1, x]) } |
for (@x <- b) { out!(["b", 2, x]) } |
for (@x <- b) { out!(["b", 3, x]) } |
for (@x <- b) { out!(["b", 4, x]) }"#;

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the records of the set: two
/// members of each of the classes 1 to 4 on `b`.
const SET_WORKLOAD_RECORDS: &str = "b!(1) | b!(1) | b!(2) | b!(2) | b!(3) | b!(3) | b!(4) | b!(4)";

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the client deploy of the
/// bag and of the set: the deploy of the queue ([`QUEUE_CLIENT`]), and a receive whose guard
/// refuses every record, so it parks. Its commit may veto, so replay of its Stored row parks it
/// without a search (E3-a).
const SCAN_CLIENT: &str = r#"a!(1) | a!(2) | a!(3) | a!(4) |
for (@x <- b) { out!(["b", 1, x]) } |
for (@x <- b) { out!(["b", 2, x]) } |
for (@x <- b) { out!(["b", 3, x]) } |
for (@x <- b) { out!(["b", 4, x]) } |
for (@x <- b where x > 100) { out!(["guarded", x]) }"#;

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the records of the VectorDB
/// space in single mode: four records on `b`, ranked from 1 to 4 by the query `[1.0, 0.0]`.
const VECTORDB_RECORDS: &str = "b!(1; embedding=[10.0, 0.0]) | b!(2; embedding=[10.0, 1.0]) |
b!(3; embedding=[10.0, 2.0]) | b!(4; embedding=[10.0, 3.0])";

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): four single-mode waiters on
/// `a` for the query `[1.0, 0.0]`.
const VECTORDB_WAITERS: &str = r#"for (@x <- a ~ ([1.0, 0.0])) { out!(["a", 1, x]) } |
for (@x <- a ~ ([1.0, 0.0])) { out!(["a", 2, x]) } |
for (@x <- a ~ ([1.0, 0.0])) { out!(["a", 3, x]) } |
for (@x <- a ~ ([1.0, 0.0])) { out!(["a", 4, x]) }"#;

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the client deploy of the
/// VectorDB space in single mode: four sends on `a`, four single-mode receives on `b`, and a
/// receive whose guard refuses every record, so it parks. Its commit may veto, so replay of its
/// Stored row parks it without a search (E3-a).
const VECTORDB_CLIENT: &str = r#"a!(1; embedding=[10.0, 0.0]) | a!(2; embedding=[10.0, 0.0]) |
a!(3; embedding=[10.0, 0.0]) | a!(4; embedding=[10.0, 0.0]) |
for (@x <- b ~ ([1.0, 0.0])) { out!(["b", 1, x]) } |
for (@x <- b ~ ([1.0, 0.0])) { out!(["b", 2, x]) } |
for (@x <- b ~ ([1.0, 0.0])) { out!(["b", 3, x]) } |
for (@x <- b ~ ([1.0, 0.0])) { out!(["b", 4, x]) } |
for (@x <- b ~ ([1.0, 0.0]) where x > 100) { out!(["guarded", x]) }"#;

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the records of the VectorDB
/// space in list mode: `s` on `a`, and `img`, `doc` and `pdf` on `b`, ranked in this order by the
/// query `[1.0, 0.0]`.
const VECTORDB_LIST_RECORDS: &str = r#"a!("s"; embedding=[10.0, 0.0]) |
b!("img"; embedding=[10.0, 0.0]) | b!("doc"; embedding=[10.0, 1.0]) |
b!("pdf"; embedding=[10.0, 2.0])"#;

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the list waiters of the
/// VectorDB space: the top two records of `a`, and the top record of `b` when it is `pdf`. While
/// `a` holds `s` alone and `img` leads `b`, both park.
const VECTORDB_LIST_WAITERS: &str = r#"for (@[v, w] <- a ~ ([1.0, 0.0]; k=2)) {
  out!(["a", v, w])
} |
for (@["pdf"] <- b ~ ([1.0, 0.0]; k=1)) { out!(["cascade", "pdf"]) }"#;

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the client deploy of the
/// VectorDB space in list mode: the send of `x`, whose score ties with the score of `s`, and the
/// receive of the top two records of `b` when `img` leads them.
const VECTORDB_LIST_CLIENT: &str = r#"a!("x"; embedding=[10.0, 0.0]) |
for (@["img", w] <- b ~ ([1.0, 0.0]; k=2)) { out!(["b", "img", w]) }"#;

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the stack chain. The owner
/// stores 1, then 2, on `b`, so 2 is the top. The sends and the receive of the client fire at the
/// head (`offer_head` and `receive_head`), and the settle offers the lifted 1 to the waiter of 1.
/// Each operation of the client takes and puts the counter of its collection, with one COMM. So
/// the client fires 6 bodies with 12 space COMMs: 2 for each send and for the receive, and the
/// lift and the offer of the settle.
const STACK_WORKLOAD: WorkloadKind = WorkloadKind {
    name: "stack",
    urn: "rho:space:hashmap:stack",
    ctor: r#""default""#,
    records: &["b!(1)", "b!(2)"],
    waiters: STACK_WAITERS,
    parked: 5,
    client: STACK_CLIENT,
    fires: 6,
    comms: 12,
    parks: 0,
};

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the queue chain. The sends
/// and the receives of the client fire at the head (`offer_head` and `receive_head`), and each
/// takes and puts the counter of its collection, with one COMM. So the client fires 8 bodies with
/// 16 space COMMs.
const QUEUE_WORKLOAD: WorkloadKind = WorkloadKind {
    name: "queue",
    urn: "rho:space:hashmap:queue",
    ctor: r#""default""#,
    records: &[WORKLOAD_RECORDS],
    waiters: WORKLOAD_WAITERS,
    parked: 4,
    client: QUEUE_CLIENT,
    fires: 8,
    comms: 16,
    parks: 0,
};

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the cell chain, with one
/// record on `b` and one waiter on `a`. The send and the receive of the client fire at the head
/// (`offer_head` and `receive_head`), and each takes and puts the counter of its collection, with
/// one COMM. So the client fires 2 bodies with 4 space COMMs.
const CELL_WORKLOAD: WorkloadKind = WorkloadKind {
    name: "cell",
    urn: "rho:space:hashmap:cell",
    ctor: r#""default""#,
    records: &["b!(1)"],
    waiters: r#"for (@x <- a) { out!(["a", 1, x]) }"#,
    parked: 1,
    client: CELL_CLIENT,
    fires: 2,
    comms: 4,
    parks: 0,
};

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the priority queue chain.
/// The sends and the receives of the client fire at the heads of the levels (`offer_pq` and
/// `receive_pq`), and each takes and puts the counter of its collection, with one COMM. So the
/// client fires 8 bodies with 16 space COMMs.
const PRIORITY_QUEUE_WORKLOAD: WorkloadKind = WorkloadKind {
    name: "priority queue",
    urn: "rho:space:hashmap:priorityqueue",
    ctor: r#""default""#,
    records: &[PRIORITY_RECORDS],
    waiters: WORKLOAD_WAITERS,
    parked: 4,
    client: PRIORITY_CLIENT,
    fires: 8,
    comms: 16,
    parks: 0,
};

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the bag chain. The sends
/// and the receives of the client fire in the scan driver (`offer_scan` and `fire_class`), and a
/// bag has no counter. So the client fires 8 bodies with 8 space COMMs, and its guarded receive
/// parks.
const BAG_WORKLOAD: WorkloadKind = WorkloadKind {
    name: "bag",
    urn: "rho:space:hashmap:bag",
    ctor: r#""default""#,
    records: &[WORKLOAD_RECORDS],
    waiters: WORKLOAD_WAITERS,
    parked: 4,
    client: SCAN_CLIENT,
    fires: 8,
    comms: 8,
    parks: 1,
};

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the set chain. Each receive
/// of the client fires one member of a class in the scan driver and retires the other member
/// (`K_retire`). So the client fires 8 bodies with 12 space COMMs, and its guarded receive parks.
const SET_WORKLOAD: WorkloadKind = WorkloadKind {
    name: "set",
    urn: "rho:space:hashmap:set",
    ctor: r#""default""#,
    records: &[SET_WORKLOAD_RECORDS],
    waiters: WORKLOAD_WAITERS,
    parked: 4,
    client: SCAN_CLIENT,
    fires: 8,
    comms: 12,
    parks: 1,
};

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the VectorDB chain in
/// single mode. Each send and each receive of the client fires one record (`offer_vdb` and
/// `fire_members`), and VectorDB has no counter. So the client fires 8 bodies with 8 space COMMs,
/// and its guarded receive parks.
const VECTORDB_SINGLE_WORKLOAD: WorkloadKind = WorkloadKind {
    name: "VectorDB single",
    urn: "rho:space:hashmap:vectordb",
    ctor: VECTORDB_CTOR,
    records: &[VECTORDB_RECORDS],
    waiters: VECTORDB_WAITERS,
    parked: 4,
    client: VECTORDB_CLIENT,
    fires: 8,
    comms: 8,
    parks: 1,
};

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the VectorDB chain in list
/// mode. The send fires the list waiter on `a` with `s` and `x`, and retires `s`. The receive
/// fires with `img` and `doc`, and retires `doc`. Its cascade lifts `pdf` and offers it to the
/// list waiter on `b` (`lift_and_reoffer`). So the client fires 3 bodies with 6 space COMMs.
const VECTORDB_LIST_WORKLOAD: WorkloadKind = WorkloadKind {
    name: "VectorDB list",
    urn: "rho:space:hashmap:vectordb",
    ctor: VECTORDB_CTOR,
    records: &[VECTORDB_LIST_RECORDS],
    waiters: VECTORDB_LIST_WAITERS,
    parked: 2,
    client: VECTORDB_LIST_CLIENT,
    fires: 3,
    comms: 6,
    parks: 0,
};

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the chains of T-P-1, one
/// for each kind of space whose plan stages owner-funded fires outside the join engine.
const WORKLOAD_KINDS: [WorkloadKind; 8] = [
    STACK_WORKLOAD,
    QUEUE_WORKLOAD,
    CELL_WORKLOAD,
    PRIORITY_QUEUE_WORKLOAD,
    BAG_WORKLOAD,
    SET_WORKLOAD,
    VECTORDB_SINGLE_WORKLOAD,
    VECTORDB_LIST_WORKLOAD,
];

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the chain of `kind` on
/// `chain`, run [`WORKLOAD_RUNS`] times. The owner creates the space, stores the records and
/// parks the waiters. Each run rewinds to the parked state and plays the client deploy with the
/// owner binding. Each deploy certifies through its own native replay (`certify_owned`), and each
/// client deploy fires the fixed counts of `kind`.
async fn workload_case(chain: &mut OwnedChain, kind: &WorkloadKind, label: &str) {
    let creation = stashing(kind.urn, kind.ctor, WORKLOAD_STASH, "Nil");
    let creation_label = format!("{label}: the space");
    let created = chain.deploy(Role::Owner, &creation, &creation_label).await;
    assert_runs(&creation_label, &created);
    for (index, records) in kind.records.iter().enumerate() {
        let records = with_stash(WORKLOAD_STASH, records);
        let records_label = format!("{label}: the records {index}");
        let stored = chain.deploy(Role::Owner, &records, &records_label).await;
        assert_runs(&records_label, &stored);
    }
    let waiters = with_stash(WORKLOAD_STASH, kind.waiters);
    let waiters_label = format!("{label}: the waiters");
    let parked = chain.deploy(Role::Owner, &waiters, &waiters_label).await;
    assert_runs(&waiters_label, &parked);
    assert_eq!(
        parked_waiters(&parked).len(),
        kind.parked,
        "{waiters_label}: each waiter parks"
    );
    let client = with_stash(WORKLOAD_STASH, kind.client);
    for run in 0..WORKLOAD_RUNS {
        chain.rewind(&parked.root).await;
        let client_label = format!("{label}: the client, run {run}");
        let played = chain.deploy(Role::Client, &client, &client_label).await;
        assert_runs(&client_label, &played);
        let (_, journal) = journal_of(&played);
        let fires = played.out.len();
        let comms = space_comms(journal);
        let parks = parked_waiters(&played).len();
        assert_eq!(
            (fires, comms, parks),
            (kind.fires, kind.comms, kind.parks),
            "{client_label}: the fires, the space COMMs and the parks: {:?}",
            played.outputs
        );
    }
}

/// Added by decision 124 (plan v4 T-P-1, F11, ruling OP-TP1-WORKLOAD): the workload of the
/// measurement of T-P-1. Each chain of [`WORKLOAD_KINDS`] runs on each runtime of
/// `RACING_WORKERS` with owner funding ([`OwnedChain`]), and each deploy certifies through its own
/// native replay. It plays owner-funded staging end to end on each kind outside the join engine:
/// the fires at the head, the settle offer, the machinery fires of `K_retire` and `K_lift`, and
/// the E3-a parks of replay. Non-vacuity: each client deploy fires the waiters of the owner and
/// its own receives, with the fixed counts of its kind.
#[test]
fn owner_funded_client_deploys_fire_and_certify_on_each_kind() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            for kind in &WORKLOAD_KINDS {
                let label = format!("T-P-1 ({}), {workers} workers", kind.name);
                workload_case(&mut OwnedChain::new().await, kind, &label).await;
            }
        });
    }
}

// ─── RADD3 1.1: a refused cascade lift books its pair (rules CR-16 and CR-20) ──────────────────

/// Added by decision 124 (ruling RADD3 1.1): the stash of the chains of a refused cascade lift.
const LIFT_STASH: &str = "nor-lift";

/// Added by decision 124 (ruling RADD3 1.1): the record of the client in the deploy that creates
/// the VectorDB space of a list chain: `taken` on `a`. The query `[1.0, 0.0]` lists it last.
const LIFT_LIST_TAKEN: &str = r#"a!("taken"; embedding=[0.0, 1.0])"#;

/// Added by decision 124 (ruling RADD3 1.1): the records of the owner in a list chain: `first`
/// and `second` on `a`.
const LIFT_LIST_RECORDS: &str =
    r#"a!("first"; embedding=[1.0, 0.0]) | a!("second"; embedding=[1.0, 1.0])"#;

/// Added by decision 124 (ruling RADD3 1.1): the list waiter of the owner in a list chain. It
/// lists every record of `a` for the query `[1.0, 0.0]`, and its pattern binds a list of two
/// members, so it parks while `a` holds three records.
const LIFT_LIST_WAITER: &str =
    r#"for (@[v, w] <- a ~ ([1.0, 0.0]; results="all")) { out!(["list", v, w]) }"#;

/// Added by decision 124 (ruling RADD3 1.1): the join of the owner in the joined list chain, as
/// in [`VETO_LIST_WAITERS`]. It never fires, because `b` holds no record, and its group puts a
/// join on the channel of `a`, so each operation on `a` runs in the join engine.
const LIFT_LIST_JOIN: &str = r#"for (@"none" <- a & @y <- b) { out!(["join", y]) }"#;

/// Added by decision 124 (ruling RADD3 1.1): the take of the client in a list chain. It takes
/// `taken`, and then its body sends `third` on `a`, a later operation of the same deploy.
const LIFT_LIST_TAKE: &str = r#"for (@"taken" <- a) {
  a!("third"; embedding=[0.0, 1.0]) | out!(["took", "taken"])
}"#;

/// Added by decision 124 (ruling RADD3 1.1): the part of the client in the deploy that creates
/// the queue of the pair chain: `r1`, the head of `a`, the take of the client, and the stash of
/// the name `c`. A queue parks a receive that arrives behind a parked waiter
/// (`Discipline::reaches_head`), so the take parks here, on `a` and `c`, before the join of the
/// owner. It takes `r1`, and then its body sends `r3` on `a`, a later operation of its deploy.
const LIFT_PAIR_HEAD: &str = r#"a!("r1") |
for (@"r1" <- a & @"go" <- c) { a!("r3") | out!(["took", "r1"]) } |
@"nor-lift-c"!(*c)"#;

/// Added by decision 124 (ruling RADD3 1.1): the records of the owner in the pair chain: `r2`
/// behind `r1` on `a`, and `y` on `b`.
const LIFT_PAIR_RECORDS: &str = r#"a!("r2") | b!("y")"#;

/// Added by decision 124 (ruling RADD3 1.1): the join of the owner in the pair chain. A queue
/// shows a join its head only (`JoinWindow::Head`), so the join parks while `r1` heads `a`.
const LIFT_PAIR_JOIN: &str = r#"for (@"r2" <- a & @y <- b) { out!(["pair", y]) }"#;

/// Added by decision 124 (ruling RADD3 1.1): the send of the client on `c` in the pair chain,
/// which fires the take that [`LIFT_PAIR_HEAD`] parks.
const LIFT_PAIR_TAKE: &str = r#"for (c <<- @"nor-lift-c") { c!("go") }"#;

/// Added by decision 124 (ruling RADD3 1.1): the deploys of a chain of a refused cascade lift.
/// The client creates the space and the stash with its part `taken`, so its peeks of the stash
/// hold no owner region. The owner stores its records and parks its waiters, and the client then
/// plays `take`. The take fires on the record of the client, and its removal starts the cascade,
/// which lifts a record of the owner for a waiter of the owner.
struct LiftChain {
    /// The family of the space.
    urn: &'static str,
    /// The constructor of the space.
    ctor: &'static str,
    /// The record of the client, which the deploy that creates the space stores, and in the pair
    /// chain the take that the deploy parks.
    taken: &'static str,
    /// The records of the owner, as one deploy body.
    records: &'static str,
    /// The waiters of the owner, as one deploy body.
    waiters: String,
    /// The take of the client, or the send that fires its parked take, as one deploy body.
    take: &'static str,
    /// The first string of each delivery of the waiter of the re-offer.
    kind: &'static str,
}

/// Added by decision 124 (ruling RADD3 1.1): the list chain on a VectorDB space, with the join of
/// [`LIFT_LIST_JOIN`] when `joined`. Without the join, the cascade runs in VDB
/// (`vectordb::lift_and_reoffer`). With it, each operation on `a` runs in the join engine, and
/// the cascade is its list cascade (`Engine::lift_and_reoffer_list`).
fn list_lift_chain(joined: bool) -> LiftChain {
    let waiters = match joined {
        false => LIFT_LIST_WAITER.to_string(),
        true => format!("{LIFT_LIST_WAITER} |\n{LIFT_LIST_JOIN}"),
    };
    LiftChain {
        urn: "rho:space:hashmap:vectordb",
        ctor: VETO_LIST_CTOR,
        taken: LIFT_LIST_TAKEN,
        records: LIFT_LIST_RECORDS,
        waiters,
        take: LIFT_LIST_TAKE,
        kind: "list",
    }
}

/// Added by decision 124 (ruling RADD3 1.1): the pair chain on a queue. The channel of `a` holds
/// the join of [`LIFT_PAIR_JOIN`], so the take runs in the join engine, and the cascade fires the
/// join as a stored pair (`Engine::lift_and_reoffer`).
fn pair_lift_chain() -> LiftChain {
    LiftChain {
        urn: "rho:space:hashmap:queue",
        ctor: r#""default""#,
        taken: LIFT_PAIR_HEAD,
        records: LIFT_PAIR_RECORDS,
        waiters: LIFT_PAIR_JOIN.to_string(),
        take: LIFT_PAIR_TAKE,
        kind: "pair",
    }
}

/// Added by decision 124 (ruling RADD3 1.1): the number of space consume rows of `journal`
/// without a COMM: the waiters that the journal parks, and each `K_lift` that the commit refuses,
/// which parks inert (rule CR-16).
fn space_parks(journal: &[NativeOperationRecord]) -> usize {
    let mut parks = 0_usize;
    for row in journal {
        match (&row.source, &row.comm) {
            (NativeOperationSource::Consume(_), None) if is_space_row(row) => parks += 1,
            (NativeOperationSource::Consume(_), _) | (NativeOperationSource::Produce(_), _) => {}
        }
    }
    parks
}

/// Added by decision 124 (ruling RADD3 1.1, rules CR-16 and CR-20): plays the chain `lift` on
/// `chain` and checks its take. The owner has budget 0, so the commit refuses the `K_lift` of the
/// cascade: the item test of the lifted record, a record of the owner, fails (R5), and the
/// `K_lift` parks inert. The finish books the pair of the lifted record and the waiter
/// (`refused_lift`). In the join engine, the refused lift ends with a settle job (ruling
/// RADD2v2 6.4), whose cascade skips the booked pair. The send in the body of the take is a
/// later operation of the same deploy, and its cascade meets the same waiter again. The take
/// runs to its end and fires, the waiter never delivers, and one space row parks: a cascade that
/// lifted the record for the waiter again would park one more `K_lift`. Each deploy certifies
/// through its own native replay.
async fn refused_lift_case(chain: &mut OwnedChain, label: &str, lift: &LiftChain) {
    let creation = stashing(lift.urn, lift.ctor, LIFT_STASH, lift.taken);
    let creation_label = format!("{label}: the space");
    let created = chain
        .deploy(Role::UnfundedClient, &creation, &creation_label)
        .await;
    assert_runs(&creation_label, &created);
    let records = with_stash(LIFT_STASH, lift.records);
    let records_label = format!("{label}: the records");
    let stored = chain.deploy(Role::Owner, &records, &records_label).await;
    assert_runs(&records_label, &stored);
    let waiters = with_stash(LIFT_STASH, &lift.waiters);
    let waiters_label = format!("{label}: the waiters");
    let parked = chain.deploy(Role::Owner, &waiters, &waiters_label).await;
    assert_parks(&waiters_label, &parked);
    let take = with_stash(LIFT_STASH, lift.take);
    let take_label = format!("{label}: the take");
    let played = chain.deploy(Role::UnfundedClient, &take, &take_label).await;
    assert_runs(&take_label, &played);
    let (_, journal) = journal_of(&played);
    assert_eq!(
        (
            deliveries(&played, "took"),
            deliveries(&played, lift.kind),
            space_parks(journal)
        ),
        (1, 0, 1),
        "{take_label}: the take fires, the waiter does not, and one refused K_lift parks"
    );
}

/// Added by decision 124 (ruling RADD3 1.1): plays the chain `lift` of the cascade `site` once on
/// each runtime of `RACING_WORKERS` (decision 111), each time on a fresh owned chain.
fn refused_lift_on_each_runtime(site: &str, lift: &LiftChain) {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("RADD3 1.1 ({site}), {workers} workers");
            let mut chain = OwnedChain::new().await;
            refused_lift_case(&mut chain, &label, lift).await;
        });
    }
}

/// Added by decision 124 (ruling RADD3 1.1, ADD3 A3.2.1): a `K_lift` of the VectorDB cascade that
/// the commit refuses books its pair, so the re-issue of the refused lift (B.6 row 8) does not
/// lift the record for the waiter again.
#[test]
fn vectordb_refused_cascade_lift_books_its_pair() {
    refused_lift_on_each_runtime("VectorDB cascade", &list_lift_chain(false));
}

/// Added by decision 124 (ruling RADD3 1.1, ADD3 A3.2.2): a `K_lift` of the list cascade of the
/// join engine that the commit refuses books its pair, so the cascade of the later send does not
/// lift the record for the waiter again.
#[test]
fn join_refused_list_cascade_lift_books_its_pair() {
    refused_lift_on_each_runtime("join list cascade", &list_lift_chain(true));
}

/// Added by decision 124 (ruling RADD3 1.1, ADD3 A3.2.3): a `K_lift` of the pair cascade of the
/// join engine that the commit refuses books its pair, so the cascade of the later send does not
/// lift the record for the waiter again.
#[test]
fn join_refused_pair_cascade_lift_books_its_pair() {
    refused_lift_on_each_runtime("join pair cascade", &pair_lift_chain());
}

// ─── Batch B: the tail of a deviating row (C2 E4.5, ADD A2.9, RIBA 1.1, 2.4, 3.4, 4.3) ─────────

/// Added by decision 124 (plan of the batch B tests, B1): the stash of the chains of batch B.
const REFUSAL_STASH: &str = "nor-refusal";

/// Added by decision 124 (B1): a chain of an owner-funded deviation. The client creates the
/// space with its own records `client`, so its peeks of the stash hold no owner region, as in
/// `refused_lift_case`. Then each step plays in order: the role of its deploy and the body of
/// the deploy. The client plays as [`Role::UnfundedClient`]: its owner has budget 0, so each
/// COMM of a client deploy that holds a record or a waiter of the owner fails its item test at
/// the commit (R5), and the row search of that row deviates (K11).
struct RefusalChain {
    /// The family of the space.
    urn: &'static str,
    /// The constructor of the space.
    ctor: &'static str,
    /// The records of the client in the deploy that creates the space, or `Nil`.
    client: &'static str,
    /// The deploys after the creation, in order: the role and the body of each.
    steps: &'static [(Role, &'static str)],
}

/// Added by decision 124 (B1): plays the chain `setup` on `chain` up to its trigger: the
/// creation of the space by the client, then each step. Each deploy runs to its end and
/// certifies through its own native replay. Returns the play of the last deploy, whose root is
/// the state before the trigger.
async fn refusal_setup<C: CertifiedChain>(
    chain: &mut C,
    label: &str,
    setup: &RefusalChain,
) -> Played {
    let creation = stashing(setup.urn, setup.ctor, REFUSAL_STASH, setup.client);
    steps_setup(chain, label, &creation, setup.steps).await
}

/// Added by decision 124 (B1): plays the deploy `creation` of the client, which creates a space
/// and the stash of batch B, and then each step of `steps` in order, on `chain`. Each deploy runs
/// to its end and certifies through its own native replay. Returns the play of the last deploy.
/// The chains of [`RefusalChain`] and the chains whose steps a test builds share it.
async fn steps_setup<C: CertifiedChain>(
    chain: &mut C,
    label: &str,
    creation: &str,
    steps: &[(Role, &str)],
) -> Played {
    let creation_label = format!("{label}: the space");
    let mut last = chain
        .deploy(Role::UnfundedClient, creation, &creation_label)
        .await;
    assert_runs(&creation_label, &last);
    for (index, (role, body)) in steps.iter().enumerate() {
        let term = with_stash(REFUSAL_STASH, body);
        let step_label = format!("{label}: step {index}");
        last = chain.deploy(*role, &term, &step_label).await;
        assert_runs(&step_label, &last);
    }
    last
}

/// Added by decision 124 (B1): rewinds `chain` to `root` and plays the trigger `body` of a chain
/// of batch B as the client, with the random state of `label`. The deploy certifies through its
/// own native replay.
async fn refusal_trigger<C: CertifiedChain>(
    chain: &mut C,
    root: &Blake2b256Hash,
    body: &str,
    label: &str,
) -> Played {
    chain.rewind(root).await;
    let term = with_stash(REFUSAL_STASH, body);
    chain.deploy(Role::UnfundedClient, &term, label).await
}

/// Added by decision 124 (B1): the first space row of `journal` after the row `index`.
fn space_row_after(
    journal: &[NativeOperationRecord],
    index: usize,
) -> Option<&NativeOperationRecord> {
    journal.iter().skip(index + 1).find(|row| is_space_row(row))
}

/// Added by decision 124 (B1): true when `row` is the take of a counter: a space consume without
/// a peek whose footprint names counters only. Every row of a space transaction names `S(sid)`
/// (decision 92), and the take of a bump consumes `S(sid)` for the first operation on a
/// collection and the counter `S(sid,k)` of the collection after it (`spaces/counter.rs`).
fn is_counter_take(row: &NativeOperationRecord) -> bool {
    match (&row.source, row.consume_peeks.as_deref()) {
        (NativeOperationSource::Consume(_), None | Some([])) if is_space_row(row) => {
            footprint_channels(row).iter().all(|channel| {
                matches!(
                    namespace::reified_id_of(channel),
                    Some(ReifiedId::SpaceCounter(_) | ReifiedId::CollectionCounter(..))
                )
            })
        }
        _ => false,
    }
}

/// Added by decision 124 (B1, V4-12): checks that the deploy `played` failed with `error` and
/// committed its last transaction as its anchor alone: the journal holds `settle_jobs + 1`
/// anchors and no space row after the last one. A Stop that owes a cascade re-issues a settle
/// job, and that job re-issues the operation, so `settle_jobs` counts the anchored settle jobs
/// before the failing re-issue. Returns the journal index of the last anchor.
fn assert_anchor_alone(label: &str, played: &Played, error: &str, settle_jobs: usize) -> usize {
    let errors = format!("{:?}", played.result.errors);
    assert!(
        errors.contains(error),
        "{label}: the deploy fails with {error}: {errors}"
    );
    let (_, journal) = journal_of(played);
    let anchors = anchor_indices(journal);
    let Some((anchor, earlier)) = anchors.split_last() else {
        panic!("{label}: the failing transaction anchors: {anchors:?}");
    };
    assert_eq!(
        earlier.len(),
        settle_jobs,
        "{label}: the anchors of the settle jobs and of the failing re-issue: {anchors:?}"
    );
    match space_row_after(journal, *anchor) {
        None => *anchor,
        Some(row) => panic!(
            "{label}: the anchor at {anchor} commits alone, but {:?} follows it",
            row.source
        ),
    }
}

/// Added by decision 124 (B1, C2 E4.5): the branch of Tail(actual) of a deviating row, as the
/// journal of a deploy shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TailBranch {
    /// Return(v): no re-issue for an owner refusal, so no anchor. A re-issue of v, if any,
    /// carries `RoundCap` or `DivergentPair`, which stage no anchor first.
    Return,
    /// Stop: the re-issue for the owner refusal stages its anchor first (CR-19 case 2).
    Stop,
}

/// Added by decision 124 (B1): the branch of Tail(actual) that the deploy `played` took, by its
/// anchors: none for Return, one for Stop. The chains of its callers stop at most once, and no
/// other rule anchors in them (CR-19 cases 1 and 3 need a settle job that re-offers a stored
/// persistent record, and an empty transaction needs no deviation).
fn tail_branch(label: &str, played: &Played) -> TailBranch {
    let (_, journal) = journal_of(played);
    match anchor_indices(journal).as_slice() {
        [] => TailBranch::Return,
        [_] => TailBranch::Stop,
        anchors => panic!("{label}: a chain of batch B stops once at most: {anchors:?}"),
    }
}

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21, rule CR-19): the branch of
/// Tail(actual) of the deploy `played`, whose re-issues hold `most` anchors at most
/// (`TailCase::most_anchors`).
fn tail_branch_up_to(label: &str, played: &Played, most: usize) -> TailBranch {
    let (_, journal) = journal_of(played);
    match anchor_indices(journal).as_slice() {
        [] => TailBranch::Return,
        anchors if anchors.len() <= most => TailBranch::Stop,
        anchors => panic!("{label}: at most {most} anchors: {anchors:?}"),
    }
}

/// Added by decision 124 (B1): the second items of the deliveries on [`OUT`] after the deploy
/// `played` whose list starts with the string `kind`, sorted. The second item of each such list
/// is a string.
fn delivered_strings(played: &Played, kind: &str) -> Vec<String> {
    let mut values = Vec::<String>::with_capacity(played.out.len());
    for datum in &played.out {
        let [value] = datum.as_slice() else {
            panic!("a datum on OUT holds one value: {datum:?}");
        };
        match list_items(value) {
            [first, second, ..] if string_of(first) == kind => {
                values.push(string_of(second).to_string())
            }
            _ => {}
        }
    }
    values.sort_unstable();
    values
}

/// Added by decision 124 (B1): the source hash of the one waiter that the deploy `played` parks.
fn parked_waiter(label: &str, played: &Played) -> [u8; 32] {
    let waiters: Vec<[u8; 32]> = parked_waiters(played).into_iter().collect();
    let [waiter] = waiters.as_slice() else {
        panic!("{label}: one parked waiter: {}", waiters.len());
    };
    *waiter
}

/// Added by decision 124 (B1): the number of space COMMs of `journal` that name the waiter
/// `waiter`.
fn comms_of(journal: &[NativeOperationRecord], waiter: &[u8; 32]) -> usize {
    let mut comms = 0_usize;
    for row in journal {
        match &row.comm {
            Some(comm) if is_space_row(row) && comm.source.consume.hash == *waiter => comms += 1,
            Some(_) | None => {}
        }
    }
    comms
}

/// Added by decision 124 (plan of the batch B tests, B3, T-C4-22): the stack chain of an owner
/// refusal. The owner parks a waiter of 1 on `a`. The client sends 1, and the owner has budget 0
/// in the binding of the client, so the commit refuses the fire of the waiter, and 1 stores.
const STACK_REFUSAL_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:stack",
    ctor: r#""default""#,
    client: "Nil",
    steps: &[
        (Role::Owner, r#"for (@1 <- a) { out!(["owner", 1]) }"#),
        (Role::UnfundedClient, "a!(1)"),
    ],
};

/// Added by decision 124 (B3, T-C4-22, rule CR-19 case 2): the owner refusal case of T-C4-22 on
/// a fresh owned chain. The send of 2 settles the stack before its plan: the settle offers the
/// stored 1 to the waiter of the owner, the commit refuses the fire, and Tail(actual) is Stop.
/// The re-issue with `OwnerRefusal` stages its anchor, then the take of the counter of `a`, and
/// stores 2. So the journal holds one anchor, and the first space row after it is the take of
/// the counter.
async fn owner_refusal_reissue_case(workers: usize) {
    let label = format!("T-C4-22, {workers} workers");
    let mut chain = OwnedChain::new().await;
    let setup = refusal_setup(&mut chain, &label, &STACK_REFUSAL_CHAIN).await;
    let trigger = format!("{label}: the send of 2");
    let played = refusal_trigger(&mut chain, &setup.root, "a!(2)", &trigger).await;
    assert_runs(&trigger, &played);
    let (_, journal) = journal_of(&played);
    let anchors = anchor_indices(journal);
    let [anchor] = anchors.as_slice() else {
        panic!("{trigger}: the re-issue anchors once: {anchors:?}");
    };
    let Some(first) = space_row_after(journal, *anchor) else {
        panic!("{trigger}: the re-issue stages rows after its anchor");
    };
    assert!(
        is_counter_take(first),
        "{trigger}: the take of the counter follows the anchor: {:?}",
        first.source
    );
    assert_eq!(
        deliveries(&played, "owner"),
        0,
        "{trigger}: the owner refuses every fire of its waiter"
    );
}

/// Added by decision 124 (B3, RIBA 1.1): `count` waiters on `a` as one deploy body, each a
/// receive of any value that delivers `["capped", index]`.
fn capped_waiters(count: usize) -> String {
    let mut waiters = Vec::<String>::with_capacity(count);
    for index in 0..count {
        waiters.push(format!(r#"for (@v <- a) {{ out!(["capped", {index}]) }}"#));
    }
    waiters.join(" |\n")
}

/// Added by decision 124 (B3, RIBA 1.1): the `RoundCap` control of T-C4-22 on a fresh native
/// chain. A persistent send on a queue with one waiter more than one transaction fires stops at
/// the round cap, and the agent re-issues the rest of the send with `RoundCap`. RH gives that
/// re-issue `anchor_first: false`, so it stages no anchor, and its first row is the take of the
/// counter. So the journal holds no anchor and two takes of the counter of `a`, one for the send
/// and one for its re-issue, and every waiter delivers once.
async fn round_cap_reissue_case(workers: usize) {
    let label = format!("T-C4-22, the RoundCap control, {workers} workers");
    let waiting = FIRE_ROUND_CAP + 1;
    let creation = stashing(
        "rho:space:hashmap:queue",
        r#""default""#,
        REFUSAL_STASH,
        "Nil",
    );
    let waiters = with_stash(REFUSAL_STASH, &capped_waiters(waiting));
    let mut chain = NativeChain::new().await;
    store_and_park(&mut chain, &label, &creation, &waiters).await;
    let send = with_stash(REFUSAL_STASH, "a!!(1)");
    let send_label = format!("{label}: the send of 1 forever");
    let played = play_certified(&mut chain, &send, &send_label).await;
    assert_runs(&send_label, &played);
    let (_, journal) = journal_of(&played);
    let takes = journal.iter().filter(|row| is_counter_take(row)).count();
    assert_eq!(
        (
            anchor_indices(journal).len(),
            takes,
            deliveries(&played, "capped")
        ),
        (0, 2, waiting),
        "{send_label}: no anchor, one take of the counter for the send and one for its re-issue, \
         and one delivery for each waiter"
    );
}

/// Added by decision 124 (plan of the batch B tests, B3, T-C4-22, rule CR-19 case 2, RIBA 1.1): a
/// Stop re-issue with `OwnerRefusal` stages its anchor first, and a `RoundCap` re-issue of an
/// ordinary operation stages no anchor first. Each runtime of `RACING_WORKERS` plays each case
/// once: a stack and a queue pick nothing at random. Each deploy certifies through its own
/// native replay.
#[test]
fn stop_reissue_stages_its_anchor_first() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            owner_refusal_reissue_case(workers).await;
            round_cap_reissue_case(workers).await;
        });
    }
}

/// Added by decision 124 (plan of the batch B tests, B4, T-C4-27): the VectorDB chain of the error
/// rule. The client creates the space with `r` on `a`. The owner parks a list waiter of `pdf` on
/// `a`, because the cascade of a VectorDB collection fires list waiters only. The client parks a
/// join of `none` on `a` and any record on `b`, which never fires and puts the channel of `a` in
/// the join engine. The client then sends `pdf`, which fills `a` at the capacity of
/// [`V412_CTOR`]. The owner has budget 0 in the binding of the client, so the commit refuses the
/// fire of the list waiter, and `pdf` stores.
const ERROR_RULE_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:vectordb",
    ctor: V412_CTOR,
    client: r#"a!("r"; embedding=[0.0, 1.0])"#,
    steps: &[
        (
            Role::Owner,
            r#"for (@["pdf"] <- a ~ ([1.0, 0.0]; k=1)) { out!(["owner", 0]) }"#,
        ),
        (
            Role::UnfundedClient,
            r#"for (@"none" <- a & @y <- b) { Nil }"#,
        ),
        (Role::UnfundedClient, r#"a!("pdf"; embedding=[1.0, 0.0])"#),
    ],
};

/// Added by decision 124 (plan of the batch B tests, B4, T-C4-27): the send of `over` on the
/// full `a`. Its vector is far from the query of the list waiter, so it never joins the list.
const ERROR_RULE_SEND: &str = r#"a!("over"; embedding=[0.0, 1.0])"#;

/// Added by decision 124 (plan of the batch B tests, B4, T-C4-27, rules CR-6 and CR-32, errata
/// V4-12): the error rule of a Stop re-issue on a discipline without a counter. The client sends
/// `over` on the full `a`. The settle of the join engine before its plan lifts the stored `pdf`
/// and offers it to the list waiter of the owner, the commit refuses the fire, and Tail(actual)
/// is Stop with a settle job, which anchors first and skips the booked pair. The settle job
/// re-issues the send, which anchors first, and the store of `over` fails with
/// `CapacityExceeded` before any stage call, so play commits the anchor alone. Native replay
/// follows the rows of play up to row d, and its next stage call answers `RecordEnded` (CR-6).
/// Its re-issue follows the anchor, meets the same error before any stage call (V4-12), and
/// commits the anchor alone. Each runtime of `RACING_WORKERS` plays the
/// chain once: each offer of the chain has one candidate, so play picks nothing at random.
#[test]
fn staging_error_after_the_followed_rows_ends_as_record_end() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("T-C4-27, {workers} workers");
            let mut chain = OwnedChain::new().await;
            let setup = refusal_setup(&mut chain, &label, &ERROR_RULE_CHAIN).await;
            let trigger = format!("{label}: the send of over");
            let played = refusal_trigger(&mut chain, &setup.root, ERROR_RULE_SEND, &trigger).await;
            let anchor = assert_anchor_alone(&trigger, &played, "CapacityExceeded", 1);
            let (_, journal) = journal_of(&played);
            assert!(
                space_row_before(journal, anchor).is_some(),
                "{trigger}: the rows of the settle up to row d commit before the anchor"
            );
            assert_eq!(
                deliveries(&played, "owner"),
                0,
                "{trigger}: the owner refuses every fire of its take"
            );
        });
    }
}

/// Added by decision 124 (plan of the batch B tests, B5, F6): the constructor of the chain of F6:
/// each collection holds four records at most.
const F6_CTOR: &str = r#""default"; capacity=4, dimensions=2"#;

/// Added by decision 124 (B5, F6): the records of `a` in the deploy that creates the space of the
/// chain of F6: three records, so `a` holds one more.
const F6_RECORDS: &str = r#"a!("img"; embedding=[10.0, 0.0]) | a!("doc"; embedding=[10.0, 1.0]) |
a!("txt"; embedding=[10.0, 2.0])"#;

/// Added by decision 124 (B5, F6): the chain of F6 up to its partners. The client creates the
/// space with [`F6_RECORDS`], and then stores the partners of [`RESTAGE_PARTNERS`] on `b`.
const F6_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:vectordb",
    ctor: F6_CTOR,
    client: F6_RECORDS,
    steps: &[(Role::UnfundedClient, RESTAGE_PARTNERS)],
};

/// Added by decision 124 (B5, F6): the list waiter of the owner in the chain of F6, because the
/// cascade of a VectorDB collection fires list waiters only. Its query ranks `pdf` first, so it
/// parks while `a` holds no `pdf`.
const F6_TAKE: &str = r#"for (@["pdf"] <- a ~ ([10.0, 3.0]; k=1)) { out!(["owner", 0]) }"#;

/// Added by decision 124 (B5, F6): the send of the client that fills `a` in the chain of F6. The
/// owner has budget 0 in the binding of the client, so the commit refuses its fire of the take
/// of the owner, and `pdf` stores.
const F6_FILL: &str = r#"a!("pdf"; embedding=[10.0, 3.0])"#;

/// Added by decision 124 (B5, F6): checks a run of the chain of F6 that commits: its journal holds
/// the anchor of the settle job and the anchor of the re-issue, the fire of the join `join`
/// follows the second anchor, and the join binds `over` and a partner other than `refused`.
fn assert_anchored_join_fire(label: &str, played: &Played, join: [u8; 32], refused: i64) {
    let (_, journal) = journal_of(played);
    let anchors = anchor_indices(journal);
    let [_, anchor] = anchors.as_slice() else {
        panic!("{label}: the settle job and the re-issue anchor once each: {anchors:?}");
    };
    let fires = waiter_fires(journal, &BTreeSet::from([join]));
    let [fire] = fires.as_slice() else {
        panic!("{label}: the join fires once: {}", fires.len());
    };
    assert!(
        fire.index > *anchor,
        "{label}: the anchor at {anchor} precedes the fire of the join at {}",
        fire.index
    );
    let outputs = join_outputs(played);
    let Some((x, y)) = outputs.get(&1) else {
        panic!("{label}: the join delivers");
    };
    assert_eq!(
        (string_of(x), int_of(y) == refused),
        ("over", false),
        "{label}: the join binds over and a partner that its guard admits"
    );
}

/// Added by decision 124 (plan of the batch B tests, B5, F6, rules CR-14 and CR-19, errata
/// V4-12): plays the chain of F6 on a fresh owned chain with the join that refuses the partner
/// at `refused_at` in the DR-75 order of `b`, and sends `over` on the full `a` [`PICK_RUNS`]
/// times. Each run that commits holds two anchors, the second before the fire of the join, and
/// each run that fails commits its anchor alone with `CapacityExceeded`. Returns the runs that
/// fail.
async fn stop_restage_case(workers: usize, refused_at: usize) -> usize {
    let label = format!("F6, refused partner {refused_at}, {workers} workers");
    let mut chain = OwnedChain::new().await;
    let partners = refusal_setup(&mut chain, &label, &F6_CHAIN).await;
    let values = stored_ints(&partners, RESTAGE_PARTNER_COUNT, vdb_value);
    let Some(refused) = values.get(refused_at).copied() else {
        panic!("{label}: b holds a partner at {refused_at}: {values:?}");
    };
    let take = with_stash(REFUSAL_STASH, F6_TAKE);
    let take_label = format!("{label}: the take of the owner");
    let taking = chain.deploy(Role::Owner, &take, &take_label).await;
    assert_parks(&take_label, &taking);
    let join = with_stash(REFUSAL_STASH, &restage_join(refused));
    let join_label = format!("{label}: the join");
    let joined = chain.deploy(Role::UnfundedClient, &join, &join_label).await;
    assert_parks(&join_label, &joined);
    let join_waiter = parked_waiter(&join_label, &joined);
    let fill = with_stash(REFUSAL_STASH, F6_FILL);
    let fill_label = format!("{label}: the send of pdf");
    let filled = chain.deploy(Role::UnfundedClient, &fill, &fill_label).await;
    assert_runs(&fill_label, &filled);
    let mut failures = 0_usize;
    for run in 0..PICK_RUNS {
        let trigger = format!("{label}: the send of over, run {run}");
        let played = refusal_trigger(&mut chain, &filled.root, RESTAGE_SEND, &trigger).await;
        match played.result.errors.is_empty() {
            false => {
                assert_anchor_alone(&trigger, &played, "CapacityExceeded", 1);
                failures += 1;
            }
            true => assert_anchored_join_fire(&trigger, &played, join_waiter, refused),
        }
        assert_eq!(
            deliveries(&played, "owner"),
            0,
            "{trigger}: the owner refuses every fire of its take"
        );
    }
    failures
}

/// Added by decision 124 (plan of the batch B tests, B5, F6, rules CR-14 item 7 and CR-19 case 2,
/// errata V4-12): R-A restages the anchor first of a Stop re-issue. The settle of the join
/// engine before its plan lifts the stored `pdf` for the list waiter of the owner, the commit
/// refuses the fire, a settle job anchors first, and the re-issue of the send of `over` stages
/// its anchor first. Its offer picks the
/// partner of the join at random, and a pick of the refused partner fails at the capacity of
/// `a`, so R-A restages the re-issue in DR-75 order with its anchor first again. In the main
/// case the join refuses the last partner in DR-75 order, so the restage binds the first one,
/// and every run commits with the anchor of the re-issue before the fire of the join. A run
/// picks the refused partner with a chance of 1/3, so all runs miss the restage with a chance
/// of `(2/3)^64`. In
/// the variant of V4-12 the join refuses the first partner in DR-75 order, so the restage fails
/// again: play commits the anchor alone, and replay repeats it by the error rule. Some run on
/// each runtime fails so, and all runs miss it with a chance of `(2/3)^64`. Each deploy
/// certifies through its own native replay.
#[test]
fn space_r_a_restages_the_anchor_first_of_a_stop_reissue() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let failures = stop_restage_case(workers, RESTAGE_PARTNER_COUNT - 1).await;
            assert_eq!(
                failures, 0,
                "F6, {workers} workers: the restage binds the first partner"
            );
            let failures = stop_restage_case(workers, 0).await;
            assert!(
                failures > 0,
                "F6, the variant of V4-12, {workers} workers: some run fails both stagings"
            );
        });
    }
}

/// Added by decision 124 (plan of the batch B tests, B6, D-6): the native phlo limit of a deploy
/// that plays a divergent pair, as `DIVERGENT_PHLO` of `reified_queue.rs`. The pair re-issues
/// after each round, and the rounds run until this phlo runs out (A10).
const DIVERGENT_LIMIT: u64 = 20_000;

/// Added by decision 124 (B6, D-6): the bag chain of a divergent pair. The owner parks a take of
/// any value on `a`, and the client parks a persistent receive of any value on `a`. The body of
/// the receive sends nothing, as in the divergent pairs of the queue tests.
const DIVERGENT_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:bag",
    ctor: r#""default""#,
    client: "Nil",
    steps: &[
        (Role::Owner, r#"for (@v <- a) { out!(["owner", v]) }"#),
        (Role::UnfundedClient, r#"for (@v <= a) { Nil }"#),
    ],
};

/// Added by decision 124 (B6, D-6): the chain of D-6. The owner parks a take of any value at the
/// level 0 on `a`, the client parks a persistent receive of any value on `a`, and the client parks
/// the join [`LIFT_LIST_JOIN`], which puts each operation on `a` in the join engine and never
/// fires. A priority queue uses a counter, so each round bumps the counter of the collection and
/// pays for a new row. The body of the receive sends nothing.
const DIVERGENT_LANES_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:priorityqueue",
    ctor: r#""default""#,
    client: "Nil",
    steps: &[
        (
            Role::Owner,
            r#"for (@v <- a ~ (; priority=0)) { out!(["owner", v]) }"#,
        ),
        (Role::UnfundedClient, r#"for (@v <= a) { Nil }"#),
        (Role::UnfundedClient, LIFT_LIST_JOIN),
    ],
};

/// Added by decision 124 (plan of the batch B tests, B6, D-6, C2v3 8.7, RIBA 1.1): a Return of
/// Tail(actual) whose value is a `DivergentPair` re-issue stages no anchor first. The client
/// sends 1 forever on the priority queue [`PICK_RUNS`] times, at the limit [`DIVERGENT_LIMIT`].
/// The join engine meets the lane heads older first (`offer_order`), so it stages the fire of
/// the take of the owner, the commit refuses that fire, and the search fires the persistent
/// receive of the client instead. The send and the receive both stay, so the round re-issues the
/// send with `DivergentPair`, and Tail(actual) returns that re-issue. RH gives it `anchor_first:
/// false`, so no round stages an anchor. Each round bumps the counter of the collection and pays
/// for a new row, so the rounds run until the phlo runs out, which commits nothing. So every run
/// fails with `OutOfPhlogistonsError`, holds no anchor and some space COMM, and certifies through
/// its own native replay. A kind without a counter pays nothing for a round, so this test runs
/// on the join route of a kind with a counter.
#[test]
fn return_tail_divergent_pair_reissue_stages_no_anchor_first() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("D-6, {workers} workers");
            let mut chain = OwnedChain::new().await;
            let setup = refusal_setup(&mut chain, &label, &DIVERGENT_LANES_CHAIN).await;
            let send = with_stash(REFUSAL_STASH, "a!!(1; priority=0)");
            for run in 0..PICK_RUNS {
                let trigger = format!("{label}: the send of 1 forever, run {run}");
                chain.rewind(&setup.root).await;
                let played = chain
                    .deploy_limited(Role::UnfundedClient, &send, &trigger, DIVERGENT_LIMIT)
                    .await;
                let errors = format!("{:?}", played.result.errors);
                assert!(
                    errors.contains("OutOfPhlogistonsError"),
                    "{trigger}: the pair runs until the phlo is spent: {errors}"
                );
                let (_, journal) = journal_of(&played);
                assert_eq!(
                    (anchor_indices(journal).len(), deliveries(&played, "owner")),
                    (0, 0),
                    "{trigger}: no round anchors, and the owner refuses every fire of its take"
                );
                assert!(
                    space_comms(journal) > 0,
                    "{trigger}: the receive of the client fires"
                );
            }
        });
    }
}

/// Added by decision 124 (plan of the batch B tests, B8 and B9, T-C4-35 and T-C4-21): a case of an
/// owner-funded deviation: its chain, its trigger, the branches of Tail(actual) that its runs
/// may take, the branch that some run on each runtime must take, its runs, the checks of its
/// deliveries, and its control.
struct TailCase {
    /// The name of the case in the labels: the test, the kind and the case.
    name: &'static str,
    /// The chain up to the trigger.
    chain: RefusalChain,
    /// The body of the trigger, which the client plays.
    trigger: &'static str,
    /// The branches of Tail(actual) that a run may take.
    allowed: &'static [TailBranch],
    /// The branch that some run on each runtime takes. A Stop shows its deviation by its anchor.
    needed: TailBranch,
    /// The runs of the trigger: 1 when play picks nothing at random, else [`PICK_RUNS`].
    runs: usize,
    /// The most anchors that a run holds: 1 for a chain that stops once at most. A Matched row
    /// books no item pair (rule CR-20), so the re-issue of a persistent send, or of a send past a
    /// peek, can meet the refused waiter again and stop once more. A Stop at the fire of a
    /// two-bind peek re-issues two jobs, the restore of its stored record and the second send,
    /// and each job anchors first, so such a chain holds 2.
    most_anchors: usize,
    /// The checks of the deliveries after each run, with the label of the run.
    check: fn(&str, &Played),
    /// The control of a case whose runs can end without an anchor, or whose Stop can come by two
    /// paths: a predicate on a run of the same deploys on a `NativeChain`, true when the candidate
    /// that owner funding refuses comes first and fires. Some run of the control satisfies it.
    /// None when every run takes Stop by one path, which its anchor shows.
    control: Option<fn(&Played) -> bool>,
}

/// Added by decision 124 (B8 and B9): plays the case `case` on a fresh owned chain: its chain once,
/// and then its trigger `case.runs` times from the state before the trigger. Each run runs to
/// its end, takes an allowed branch of Tail(actual), passes the checks of the case, and
/// certifies through its own native replay, so replay takes the Tail(actual) of play (D-OBL-1).
/// Some run takes the needed branch. The control of the case, if any, plays the same deploys
/// with the same labels on a fresh native chain, and some run of it fires the candidate that
/// owner funding refuses.
async fn tail_case(workers: usize, case: &TailCase) {
    let label = format!("{}, {workers} workers", case.name);
    let mut chain = OwnedChain::new().await;
    let setup = refusal_setup(&mut chain, &label, &case.chain).await;
    let mut needed = 0_usize;
    for run in 0..case.runs {
        let trigger = format!("{label}: the trigger, run {run}");
        let played = refusal_trigger(&mut chain, &setup.root, case.trigger, &trigger).await;
        assert_runs(&trigger, &played);
        let branch = tail_branch_up_to(&trigger, &played, case.most_anchors);
        assert!(
            case.allowed.contains(&branch),
            "{trigger}: Tail(actual) takes {branch:?}, not one of {:?}",
            case.allowed
        );
        needed += usize::from(branch == case.needed);
        (case.check)(&trigger, &played);
    }
    assert!(needed > 0, "{label}: some run takes {:?}", case.needed);
    let Some(refused_fires) = case.control else {
        return;
    };
    let mut control = NativeChain::new().await;
    let unfunded = refusal_setup(&mut control, &label, &case.chain).await;
    let mut fired = 0_usize;
    for run in 0..case.runs {
        let trigger = format!("{label}: the trigger, run {run}");
        let played = refusal_trigger(&mut control, &unfunded.root, case.trigger, &trigger).await;
        assert_runs(&trigger, &played);
        fired += usize::from(refused_fires(&played));
    }
    assert!(
        fired > 0,
        "{label}: without owner funding, the refused candidate fires in some run"
    );
}

/// Added by decision 124 (plan of the batch B tests, B8, T-C4-35): the bag chain of case (a). The
/// client creates the space with 1 on `b`. The client parks a peek of any value on `a` and `b`,
/// and then the owner parks a guarded join of any value on `a` and 1 on `b`. Both waiters are of
/// one join group, and play shuffles the waiters of a group (`pick_group_order`), so an offer on
/// `a` meets the join before the peek in about half of the runs.
const GUARDED_JOIN_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:bag",
    ctor: r#""default""#,
    client: "b!(1)",
    steps: &[
        (
            Role::UnfundedClient,
            r#"for (@v <<- a & @y <<- b) { out!(["peek", v]) }"#,
        ),
        (
            Role::Owner,
            r#"for (@v <- a & @y <- b where y == 1) { out!(["owner", v]) }"#,
        ),
    ],
};

/// Added by decision 124 (B8, T-C4-35): the bag chain of case (b). The client creates the space
/// with `c` on `a` and 1 on `b`, and the owner stores `o` on `a`.
const GUARDED_PEEK_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:bag",
    ctor: r#""default""#,
    client: r#"a!("c") | b!(1)"#,
    steps: &[(Role::Owner, r#"a!("o")"#)],
};

/// Added by decision 124 (B8, T-C4-35): the checks of case (a) after the run `played`: the peek
/// of the client delivers once, and the join of the owner never delivers.
fn peek_fires_once(label: &str, played: &Played) {
    assert_eq!(
        (deliveries(played, "peek"), deliveries(played, "owner")),
        (1, 0),
        "{label}: the peek of the client fires once, and the owner refuses its join"
    );
}

/// Added by decision 124 (B8, T-C4-35): the checks of case (b) after the run `played`: the join of
/// the client binds the record `c` of the client, once.
fn join_binds_the_client_record(label: &str, played: &Played) {
    assert_eq!(
        delivered_strings(played, "pair"),
        ["c"],
        "{label}: the join binds the record of the client"
    );
}

/// Added by decision 124 (B8, T-C4-35): the two cases of T-C4-35 on a bag in the join engine.
const GUARDED_CASES: [TailCase; 2] = [
    TailCase {
        name: "T-C4-35 (a)",
        chain: GUARDED_JOIN_CHAIN,
        trigger: "a!(5)",
        allowed: &[TailBranch::Return, TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: PICK_RUNS,
        most_anchors: 2,
        check: peek_fires_once,
        control: None,
    },
    TailCase {
        name: "T-C4-35 (b)",
        chain: GUARDED_PEEK_CHAIN,
        trigger: r#"for (@v <<- a & @y <<- b where y == 1) { out!(["pair", v]) }"#,
        allowed: &[TailBranch::Return, TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: PICK_RUNS,
        most_anchors: 1,
        check: join_binds_the_client_record,
        control: None,
    },
];

/// Added by decision 124 (plan of the batch B tests, B8, T-C4-35, errata V4-18, rule CR-31): a Stop
/// after a deviating row whose refused candidate is a guarded continuation of two binds replays.
/// The finish books the item pairs of such a row on the Stop branch only, so the re-issue skips
/// the continuation by its pair and not by its guard. Case (a): the commit refuses the
/// guarded join of the owner, and the search fires the peek of the client, so Tail(actual) is
/// Stop. Case (b): the commit refuses the record of the owner for the guarded join of the client,
/// and the search binds the record of the client, whose peek owes the restore of Rule E, so
/// Tail(actual) is Stop. Play picks at random, so a run that meets the candidates in the other
/// order takes Return. Each case runs [`PICK_RUNS`] times on each runtime, every run certifies
/// through its own native replay, and some run on each runtime takes Stop. A run takes Stop with
/// a chance of 1/2 [Inference], so all runs miss it with a chance of `(1/2)^64`.
#[test]
fn stop_after_a_guarded_two_bind_continuation_replays() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            for case in &GUARDED_CASES {
                tail_case(workers, case).await;
            }
        });
    }
}

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21): the waiter of the owner in the
/// chains of T-C4-21: a take of any value on `a`.
const OWNER_WAITER: &str = r#"for (@v <- a) { out!(["owner", v]) }"#;

/// Added by decision 124 (B9, T-C4-21): the waiter of the client in the chains of T-C4-21: a take
/// of any value on `a`.
const CLIENT_WAITER: &str = r#"for (@v <- a) { out!(["client", v]) }"#;

/// Added by decision 124 (B9, T-C4-21): the peek of the client in the chains of T-C4-21. It
/// leaves the value that it reads in flight.
const CLIENT_PEEK: &str = r#"for (@v <<- a) { out!(["peek", v]) }"#;

/// Added by decision 124 (B9, T-C4-21): the waiter of the client in the lane of the level 0 of a
/// priority queue.
const LANE_ZERO_WAITER: &str = r#"for (@v <- a ~ (; priority=0)) { out!(["client", v]) }"#;

/// Added by decision 124 (B9, T-C4-21): the waiter of the owner on the path `[a, 1]` of a
/// PathMap space. Its body is `Nil`, so a check reads its fires in the journal (`comms_of`).
const PREFIX_OWNER_WAITER: &str = r#"use space { for (@v <- @[*a, 1]) { Nil } }"#;

/// Added by decision 124 (B9, T-C4-21): the send of 5 forever on the level 0 of a priority queue.
const LANES_SEND_FOREVER: &str = "a!!(5; priority=0)";

/// Added by decision 124 (B9, T-C4-21): the send of 5 on the path `[a, 1]` of a PathMap space.
const PREFIX_SEND: &str = "use space { @[*a, 1]!(5) }";

/// Added by decision 124 (B9, T-C4-21): the send of 5 forever on the path `[a, 1]` of a PathMap
/// space.
const PREFIX_SEND_FOREVER: &str = "use space { @[*a, 1]!!(5) }";

/// Added by decision 124 (B9, T-C4-21 VDB case 4): the take of the client in the re-offer chain.
/// It takes `taken`, a record of the client, and its removal starts the cascade.
const REOFFER_TAKE: &str = r#"for (@"taken" <- a) { out!(["took", "taken"]) }"#;

/// Added by decision 124 (B9, T-C4-21): the steps of a chain whose refused candidate has no
/// alternative: the owner parks [`OWNER_WAITER`].
const OWNER_STEPS: &[(Role, &str)] = &[(Role::Owner, OWNER_WAITER)];

/// Added by decision 124 (B9, T-C4-21): [`OWNER_STEPS`], then the join of the client
/// [`LIFT_LIST_JOIN`], which puts each operation on `a` in the join engine and never fires.
const JOINED_OWNER_STEPS: &[(Role, &str)] = &[
    (Role::Owner, OWNER_WAITER),
    (Role::UnfundedClient, LIFT_LIST_JOIN),
];

/// Added by decision 124 (B9, T-C4-21): the list waiter of the owner of J Similarity 4. The
/// cascade of a VectorDB collection fires list waiters only, so only a list waiter shows the
/// refused pair to the tail read.
const OWNER_LIST_WAITER: &str = r#"for (@[v] <- a ~ ([1.0, 0.0]; k=1)) { out!(["owner", v]) }"#;

/// Added by decision 124 (B9, T-C4-21): [`OWNER_LIST_WAITER`], then the join of the client.
const JOINED_LIST_OWNER_STEPS: &[(Role, &str)] = &[
    (Role::Owner, OWNER_LIST_WAITER),
    (Role::UnfundedClient, LIFT_LIST_JOIN),
];

/// Added by decision 124 (B9, T-C4-21): the send of J Similarity 4, on the query of the waiter.
const SIMILARITY_LIST_SEND: &str = r#"a!(5; embedding=[1.0, 0.0])"#;

/// Added by decision 124 (B9, T-C4-21): the waiter of the owner, the waiter of the client, and
/// the join of the client.
const JOINED_PAIR_STEPS: &[(Role, &str)] = &[
    (Role::Owner, OWNER_WAITER),
    (Role::UnfundedClient, CLIENT_WAITER),
    (Role::UnfundedClient, LIFT_LIST_JOIN),
];

/// Added by decision 124 (B9, T-C4-21): the waiter of the owner, the peek of the client, and the
/// join of the client.
const JOINED_PEEK_STEPS: &[(Role, &str)] = &[
    (Role::Owner, OWNER_WAITER),
    (Role::UnfundedClient, CLIENT_PEEK),
    (Role::UnfundedClient, LIFT_LIST_JOIN),
];

/// Added by decision 124 (B9, T-C4-21): the waiter of the owner in the lane `Any` of a priority
/// queue, the waiter of the client in the lane of the level 0, and the join of the client.
const LANES_PAIR_STEPS: &[(Role, &str)] = &[
    (Role::Owner, OWNER_WAITER),
    (Role::UnfundedClient, LANE_ZERO_WAITER),
    (Role::UnfundedClient, LIFT_LIST_JOIN),
];

/// Added by decision 124 (B9, T-C4-21): the waiter of the owner and the waiter of the client,
/// with no join, so each operation on `a` runs on the route of its discipline.
const OWN_PAIR_STEPS: &[(Role, &str)] = &[
    (Role::Owner, OWNER_WAITER),
    (Role::UnfundedClient, CLIENT_WAITER),
];

/// Added by decision 124 (B9, T-C4-21): the waiter of the owner and the peek of the client, with
/// no join.
const OWN_PEEK_STEPS: &[(Role, &str)] = &[
    (Role::Owner, OWNER_WAITER),
    (Role::UnfundedClient, CLIENT_PEEK),
];

/// Added by decision 124 (B9, T-C4-21): the waiter of the owner on `[a, 1]` and the waiter of the
/// client on `a` of a PathMap space.
const PREFIX_PAIR_STEPS: &[(Role, &str)] = &[
    (Role::Owner, PREFIX_OWNER_WAITER),
    (Role::UnfundedClient, CLIENT_WAITER),
];

/// Added by decision 124 (B9, T-C4-21 VDB case 4): the client stores `first` and `second` beside
/// its record `taken`, and the owner parks its list waiter [`LIFT_LIST_WAITER`], which parks
/// while `a` holds three records.
const REOFFER_STEPS: &[(Role, &str)] = &[
    (Role::UnfundedClient, LIFT_LIST_RECORDS),
    (Role::Owner, LIFT_LIST_WAITER),
];

/// Added by decision 124 (B9, T-C4-21): the checks of a case whose refused waiter of the owner
/// has no alternative: the owner never delivers.
fn owner_never_delivers(label: &str, played: &Played) {
    assert_eq!(
        deliveries(played, "owner"),
        0,
        "{label}: the owner refuses every fire of its waiter"
    );
}

/// Added by decision 124 (B9, T-C4-21): the checks of a case whose alternative is the waiter of
/// the client: the client delivers once, and the owner never delivers.
fn client_fires_once(label: &str, played: &Played) {
    assert_eq!(
        (deliveries(played, "client"), deliveries(played, "owner")),
        (1, 0),
        "{label}: the waiter of the client fires once, and the owner refuses its waiter"
    );
}

/// Added by decision 124 (B9, T-C4-21 VDB case 4): the checks of the re-offer case: the take of
/// the client fires once, and the list waiter of the owner never delivers.
fn took_without_the_list(label: &str, played: &Played) {
    assert_eq!(
        (deliveries(played, "took"), deliveries(played, "list")),
        (1, 0),
        "{label}: the take fires, and the owner refuses the re-offer to its list waiter"
    );
}

/// Added by decision 124 (B9, T-C4-21): the control of a case whose refused candidate is the
/// waiter of the owner: true when it fires.
fn owner_fires(played: &Played) -> bool { deliveries(played, "owner") > 0 }

/// Added by decision 124 (B9, T-C4-21): the control of a case whose alternative is the peek of
/// the client: true when the waiter of the owner meets the value first and takes it, so the peek
/// never reads it.
fn peek_starves(played: &Played) -> bool { deliveries(played, "peek") == 0 }

/// Added by decision 124 (B9, T-C4-21): the control of a PathMap case: true when the waiter of
/// the owner on `[a, 1]` takes the value first, so the waiter of the client on `a` never sees it.
fn client_starves(played: &Played) -> bool { deliveries(played, "client") == 0 }

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21): the cases of T-C4-21 that the
/// runner of B8 plays, for each kind and each case of ADD A2.9 that a chain reaches (B9.3 of the
/// plan).
const T21_CASES: [TailCase; 19] = [
    TailCase {
        name: "T-C4-21 STK 4",
        chain: RefusalChain {
            urn: "rho:space:hashmap:stack",
            ctor: r#""default""#,
            client: "Nil",
            steps: OWNER_STEPS,
        },
        trigger: "a!(5)",
        allowed: &[TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: 1,
        most_anchors: 1,
        check: owner_never_delivers,
        control: None,
    },
    TailCase {
        name: "T-C4-21 PQ 4",
        chain: RefusalChain {
            urn: "rho:space:hashmap:priorityqueue",
            ctor: r#""default""#,
            client: "Nil",
            steps: OWNER_STEPS,
        },
        trigger: "a!(5)",
        allowed: &[TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: 1,
        most_anchors: 1,
        check: owner_never_delivers,
        control: None,
    },
    TailCase {
        name: "T-C4-21 J Head 4",
        chain: RefusalChain {
            urn: "rho:space:hashmap:queue",
            ctor: r#""default""#,
            client: "Nil",
            steps: JOINED_OWNER_STEPS,
        },
        trigger: "a!(5)",
        allowed: &[TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: 1,
        most_anchors: 1,
        check: owner_never_delivers,
        control: None,
    },
    TailCase {
        name: "T-C4-21 J Lanes 1",
        chain: RefusalChain {
            urn: "rho:space:hashmap:priorityqueue",
            ctor: r#""default""#,
            client: "Nil",
            steps: LANES_PAIR_STEPS,
        },
        trigger: "a!(5; priority=0)",
        allowed: &[TailBranch::Return],
        needed: TailBranch::Return,
        runs: 1,
        most_anchors: 1,
        check: client_fires_once,
        control: Some(owner_fires),
    },
    TailCase {
        name: "T-C4-21 J Lanes 2",
        chain: RefusalChain {
            urn: "rho:space:hashmap:priorityqueue",
            ctor: r#""default""#,
            client: "Nil",
            steps: LANES_PAIR_STEPS,
        },
        trigger: LANES_SEND_FOREVER,
        allowed: &[TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: 1,
        most_anchors: 3,
        check: client_fires_once,
        control: None,
    },
    TailCase {
        name: "T-C4-21 J Lanes 4",
        chain: RefusalChain {
            urn: "rho:space:hashmap:priorityqueue",
            ctor: r#""default""#,
            client: "Nil",
            steps: JOINED_OWNER_STEPS,
        },
        trigger: "a!(5)",
        allowed: &[TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: 1,
        most_anchors: 1,
        check: owner_never_delivers,
        control: None,
    },
    TailCase {
        name: "T-C4-21 J Classes 1",
        chain: RefusalChain {
            urn: "rho:space:hashmap:set",
            ctor: r#""default""#,
            client: "Nil",
            steps: JOINED_PAIR_STEPS,
        },
        trigger: "a!(5)",
        allowed: &[TailBranch::Return],
        needed: TailBranch::Return,
        runs: PICK_RUNS,
        most_anchors: 1,
        check: client_fires_once,
        control: Some(owner_fires),
    },
    TailCase {
        name: "T-C4-21 J Classes 2",
        chain: RefusalChain {
            urn: "rho:space:hashmap:set",
            ctor: r#""default""#,
            client: "Nil",
            steps: JOINED_PEEK_STEPS,
        },
        trigger: "a!(5)",
        allowed: &[TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: PICK_RUNS,
        most_anchors: 3,
        check: peek_fires_once,
        control: Some(peek_starves),
    },
    TailCase {
        name: "T-C4-21 J Classes 4",
        chain: RefusalChain {
            urn: "rho:space:hashmap:set",
            ctor: r#""default""#,
            client: "Nil",
            steps: JOINED_OWNER_STEPS,
        },
        trigger: "a!(5)",
        allowed: &[TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: 1,
        most_anchors: 1,
        check: owner_never_delivers,
        control: None,
    },
    TailCase {
        name: "T-C4-21 J Similarity 1",
        chain: RefusalChain {
            urn: "rho:space:hashmap:vectordb",
            ctor: VECTORDB_CTOR,
            client: "Nil",
            steps: JOINED_PAIR_STEPS,
        },
        trigger: "a!(5)",
        allowed: &[TailBranch::Return],
        needed: TailBranch::Return,
        runs: PICK_RUNS,
        most_anchors: 1,
        check: client_fires_once,
        control: Some(owner_fires),
    },
    TailCase {
        name: "T-C4-21 J Similarity 2",
        chain: RefusalChain {
            urn: "rho:space:hashmap:vectordb",
            ctor: VECTORDB_CTOR,
            client: "Nil",
            steps: JOINED_PEEK_STEPS,
        },
        trigger: "a!(5)",
        // A peek met first leaves a store that no cascade reads: Return, as VDB 2.
        allowed: &[TailBranch::Return, TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: PICK_RUNS,
        most_anchors: 2,
        check: peek_fires_once,
        control: Some(peek_starves),
    },
    TailCase {
        name: "T-C4-21 J Similarity 4",
        chain: RefusalChain {
            urn: "rho:space:hashmap:vectordb",
            ctor: VECTORDB_CTOR,
            client: "Nil",
            steps: JOINED_LIST_OWNER_STEPS,
        },
        trigger: SIMILARITY_LIST_SEND,
        allowed: &[TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: 1,
        most_anchors: 1,
        check: owner_never_delivers,
        control: None,
    },
    TailCase {
        name: "T-C4-21 PMJ 1",
        chain: RefusalChain {
            urn: "rho:space:pathmap:queue",
            ctor: r#""default""#,
            client: "Nil",
            steps: PREFIX_PAIR_STEPS,
        },
        trigger: PREFIX_SEND,
        allowed: &[TailBranch::Return],
        needed: TailBranch::Return,
        runs: 1,
        most_anchors: 1,
        check: client_fires_once,
        control: Some(client_starves),
    },
    TailCase {
        name: "T-C4-21 PMJ 2",
        chain: RefusalChain {
            urn: "rho:space:pathmap:queue",
            ctor: r#""default""#,
            client: "Nil",
            steps: PREFIX_PAIR_STEPS,
        },
        trigger: PREFIX_SEND_FOREVER,
        allowed: &[TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: 1,
        most_anchors: 3,
        check: client_fires_once,
        control: None,
    },
    TailCase {
        name: "T-C4-21 PMJ 4",
        chain: RefusalChain {
            urn: "rho:space:pathmap:queue",
            ctor: r#""default""#,
            client: "Nil",
            steps: OWNER_STEPS,
        },
        trigger: "a!(5)",
        allowed: &[TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: 1,
        most_anchors: 1,
        check: owner_never_delivers,
        control: None,
    },
    TailCase {
        name: "T-C4-21 VDB 1",
        chain: RefusalChain {
            urn: "rho:space:hashmap:vectordb",
            ctor: VECTORDB_CTOR,
            client: "Nil",
            steps: OWN_PAIR_STEPS,
        },
        trigger: "a!(5)",
        allowed: &[TailBranch::Return],
        needed: TailBranch::Return,
        runs: PICK_RUNS,
        most_anchors: 1,
        check: client_fires_once,
        control: Some(owner_fires),
    },
    TailCase {
        name: "T-C4-21 VDB 2",
        chain: RefusalChain {
            urn: "rho:space:hashmap:vectordb",
            ctor: VECTORDB_CTOR,
            client: "Nil",
            steps: OWN_PEEK_STEPS,
        },
        trigger: "a!(5)",
        allowed: &[TailBranch::Return, TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: PICK_RUNS,
        most_anchors: 1,
        check: peek_fires_once,
        control: None,
    },
    TailCase {
        name: "T-C4-21 VDB 3",
        chain: RefusalChain {
            urn: "rho:space:hashmap:vectordb",
            ctor: VECTORDB_CTOR,
            client: "Nil",
            steps: OWNER_STEPS,
        },
        trigger: "a!(5)",
        allowed: &[TailBranch::Return],
        needed: TailBranch::Return,
        runs: 1,
        most_anchors: 1,
        check: owner_never_delivers,
        control: Some(owner_fires),
    },
    TailCase {
        name: "T-C4-21 VDB 4",
        chain: RefusalChain {
            urn: "rho:space:hashmap:vectordb",
            ctor: VETO_LIST_CTOR,
            client: LIFT_LIST_TAKEN,
            steps: REOFFER_STEPS,
        },
        trigger: REOFFER_TAKE,
        allowed: &[TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: 1,
        most_anchors: 1,
        check: took_without_the_list,
        control: None,
    },
];

/// Added by decision 124 (B9, T-C4-21 case 6 on a PathMap queue): `count` waiters of the client
/// on the path `[a, 1]` of a PathMap space as one deploy body, each a receive of any value with
/// the body `Nil`.
fn capped_prefix_waiters(count: usize) -> String {
    // Changed by CD-2 (clippy -D warnings, the final verification of merge 2): `vec!` with a
    // count fills the vector with `count` copies at its exact capacity
    // (`clippy::same_item_push`). The text before:
    // let mut waiters = Vec::<&str>::with_capacity(count);
    // for _ in 0..count {
    //     waiters.push("for (@v <- @[*a, 1]) { Nil }");
    // }
    let waiters = vec!["for (@v <- @[*a, 1]) { Nil }"; count];
    format!("use space {{ {} }}", waiters.join(" |\n"))
}

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21 case 6 in the join engine): a
/// persistent send of the level 0 on a priority queue with the join of the client. Its first
/// `FIRE_ROUND_CAP - 1` rounds fire the older waiters of the client in the lane `Any`. Then the
/// commit refuses the waiter of the owner, the head of that lane, and the search fires the
/// waiter of the client in the lane of the level 0, which reaches the round cap. So Tail(actual)
/// is Return with the `RoundCap` re-issue, which stages no anchor. A Matched row books no item
/// pair (rule CR-20), so the re-issue meets the waiter of the owner again, the commit refuses it,
/// 5 stores, and the settle job of that Stop alone anchors, after the fire at the cap. The
/// control plays the same deploys on a native chain, where the
/// waiter of the owner fires at the cap. Every deploy certifies through its own native replay.
async fn lanes_cap_case(workers: usize) {
    let label = format!("T-C4-21 J Lanes 6, {workers} workers");
    let creation = stashing(
        "rho:space:hashmap:priorityqueue",
        r#""default""#,
        REFUSAL_STASH,
        "Nil",
    );
    let capped = capped_waiters(FIRE_ROUND_CAP - 1);
    let steps = [
        (Role::UnfundedClient, capped.as_str()),
        (Role::Owner, OWNER_WAITER),
        (Role::UnfundedClient, LIFT_LIST_JOIN),
        (Role::UnfundedClient, LANE_ZERO_WAITER),
    ];
    let trigger = format!("{label}: the send of 5 forever");
    let mut chain = OwnedChain::new().await;
    let setup = steps_setup(&mut chain, &label, &creation, &steps).await;
    let zero = parked_waiter(&label, &setup);
    let played = refusal_trigger(&mut chain, &setup.root, LANES_SEND_FOREVER, &trigger).await;
    assert_runs(&trigger, &played);
    let (_, journal) = journal_of(&played);
    let before_anchor = match anchor_indices(journal).first() {
        Some(&anchor) => anchor,
        None => journal.len(),
    };
    assert_eq!(
        comms_of(&journal[..before_anchor], &zero),
        1,
        "{trigger}: the waiter of lane 0 fires at the cap, before any anchor"
    );
    assert_eq!(
        (
            deliveries(&played, "capped"),
            deliveries(&played, "client"),
            deliveries(&played, "owner")
        ),
        (FIRE_ROUND_CAP - 1, 1, 0),
        "{trigger}: the older waiters fire, then the owner refuses its waiter and the waiter of \
         lane 0 fires at the cap"
    );
    let mut control = NativeChain::new().await;
    let unfunded = steps_setup(&mut control, &label, &creation, &steps).await;
    let played = refusal_trigger(&mut control, &unfunded.root, LANES_SEND_FOREVER, &trigger).await;
    assert_runs(&trigger, &played);
    assert_eq!(
        deliveries(&played, "owner"),
        1,
        "{trigger}: without owner funding, the waiter of the owner fires at the cap"
    );
}

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21 case 6 on a PathMap queue): a
/// persistent send on `[a, 1]`. Its first `FIRE_ROUND_CAP - 1` rounds fire the older waiters of
/// the client on `[a, 1]`. Then the commit refuses the waiter of the owner, the head of `[a, 1]`,
/// and the search fires the waiter of the client on the prefix `a`, which reaches the round cap.
/// So Tail(actual) is Return with the `RoundCap` re-issue, which stages no anchor. The re-issue
/// meets the waiter of the owner again, the commit refuses it, and the settle job of that Stop
/// alone anchors, after the fire at the cap. The waiter of the owner never fires. Its body is
/// `Nil`, so the check reads its COMMs in the journal. The control plays the same deploys on a
/// native chain, where the waiter of the owner fires at the cap. The control reads the source
/// hash of that waiter from its own chain.
async fn prefix_cap_case(workers: usize) {
    let label = format!("T-C4-21 PMJ 6, {workers} workers");
    let creation = stashing(
        "rho:space:pathmap:queue",
        r#""default""#,
        REFUSAL_STASH,
        "Nil",
    );
    let capped = capped_prefix_waiters(FIRE_ROUND_CAP - 1);
    let steps = [
        (Role::UnfundedClient, capped.as_str()),
        (Role::Owner, PREFIX_OWNER_WAITER),
    ];
    let client = with_stash(REFUSAL_STASH, CLIENT_WAITER);
    let client_label = format!("{label}: the waiter of the client");
    let trigger = format!("{label}: the send of 5 forever");
    let mut chain = OwnedChain::new().await;
    let parked = steps_setup(&mut chain, &label, &creation, &steps).await;
    let owner = parked_waiter(&label, &parked);
    let setup = chain
        .deploy(Role::UnfundedClient, &client, &client_label)
        .await;
    assert_runs(&client_label, &setup);
    let client_waiter = parked_waiter(&client_label, &setup);
    let played = refusal_trigger(&mut chain, &setup.root, PREFIX_SEND_FOREVER, &trigger).await;
    assert_runs(&trigger, &played);
    let (_, journal) = journal_of(&played);
    let before_anchor = match anchor_indices(journal).first() {
        Some(&anchor) => anchor,
        None => journal.len(),
    };
    assert_eq!(
        (
            comms_of(&journal[..before_anchor], &client_waiter),
            comms_of(journal, &owner),
            deliveries(&played, "client")
        ),
        (1, 0, 1),
        "{trigger}: the waiter of the client fires at the cap before any anchor, and the owner \
         refuses its waiter"
    );
    let mut control = NativeChain::new().await;
    let control_parked = steps_setup(&mut control, &label, &creation, &steps).await;
    let control_owner = parked_waiter(&label, &control_parked);
    let unfunded = control
        .deploy(Role::UnfundedClient, &client, &client_label)
        .await;
    let played = refusal_trigger(&mut control, &unfunded.root, PREFIX_SEND_FOREVER, &trigger).await;
    assert_runs(&trigger, &played);
    let (_, journal) = journal_of(&played);
    assert_eq!(
        comms_of(journal, &control_owner),
        1,
        "{trigger}: without owner funding, the waiter of the owner fires at the cap"
    );
}

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21 case 6, ruling RADD2v2 8.4 item
/// 5): the tries of a band search for one waiter. A try succeeds with a chance of 1/4 or more, so
/// the `FIRE_ROUND_CAP` searches of a case all succeed but with a chance below
/// `64 · (3/4)^90`, about `4·10^-10`.
const BAND_SALTS: usize = 90;

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21 case 6, ruling RADD2v2 8.4 item
/// 5): the band of a source hash in the canonical order of DR-75, by its first byte. `Below`
/// holds the hashes under 1/2 of the hash space, `Middle` those from 1/2 to 3/4, and `Above`
/// the rest. Each hash of `Below` comes before each hash of `Middle`, and each hash of `Middle`
/// before each hash of `Above`.
#[derive(Clone, Copy)]
enum Band {
    Below,
    Middle,
    Above,
}

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21 case 6, ruling RADD2v2 8.4 item
/// 5): true when the source hash `hash` falls in `band`.
fn in_band(hash: &[u8; 32], band: Band) -> bool {
    match band {
        Band::Below => hash[0] < 0x80,
        Band::Middle => (0x80..0xC0).contains(&hash[0]),
        Band::Above => hash[0] >= 0xC0,
    }
}

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21 case 6, ruling RADD2v2 8.4 item
/// 5): parks the waiter `body` of `role` on `chain` with the first salt of `label` whose source
/// hash falls in `band`. Each try rewinds to `root` and certifies. Returns the play and the
/// salted label. The hashes of a deploy follow from its label and its term alone,
/// so the label holds on every runtime.
async fn park_in_band<C: CertifiedChain>(
    chain: &mut C,
    root: &Blake2b256Hash,
    (role, body): (Role, &str),
    label: &str,
    band: Band,
) -> (Played, String) {
    let term = with_stash(REFUSAL_STASH, body);
    for salt in 0..BAND_SALTS {
        chain.rewind(root).await;
        let salted = format!("{label}, salt {salt}");
        let parked = chain.deploy(role, &term, &salted).await;
        assert_parks(&salted, &parked);
        match in_band(&parked_waiter(&salted, &parked), band) {
            false => {}
            true => return (parked, salted),
        }
    }
    panic!("{label}: no salt below {BAND_SALTS} parks the waiter in its band");
}

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21 case 6, ruling RADD2v2 8.4 item
/// 5): parks the waiter `body` of `role` on `chain` with the label `salted` of a band search.
async fn park_salted<C: CertifiedChain>(
    chain: &mut C,
    (role, body): (Role, &str),
    salted: &str,
) -> Played {
    let term = with_stash(REFUSAL_STASH, body);
    let parked = chain.deploy(role, &term, salted).await;
    assert_parks(salted, &parked);
    parked
}

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21 case 6): the list waiter of the
/// owner in a cascade cap case. Its pattern binds `[doc]`, as the waiters of `list_waiters` do.
const CAP_OWNER_WAITER: &str = r#"for (@["doc"] <- a ~ ([1.0, 0.0]; k=1)) { out!(["owner", 0]) }"#;

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21 case 6): the list waiter of the
/// client after the waiter of the owner in a cascade cap case.
const CAP_ALTERNATIVE_WAITER: &str =
    r#"for (@["doc"] <- a ~ ([1.0, 0.0]; k=1)) { out!(["alternative", 0]) }"#;

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21 case 6 of the VDB list, ruling
/// RADD2v2 8.4 items 4, 5 and 7): plays case 6 of the VDB list on an owned chain. The client
/// creates the VectorDB with `img` and the persistent `doc` on `a`, as T-C4-23 does. Then
/// `FIRE_ROUND_CAP` list waiters of `[doc]` park, one deploy each, in the canonical order of
/// DR-75: `FIRE_ROUND_CAP - 2` waiters of the client, the waiter of the owner, and the
/// alternative. The removal of `img` fires its own round and one cascade round for each waiter
/// in that order, so the waiter of the owner meets the round cap. The commit
/// refuses it, and the row search of the re-offer fires the alternative in its place, at the cap
/// with nothing in flight. A settle job that stages nothing stages an anchor alone
/// (`anchor_if_empty`), so the test reads the place of the fire and not the count of the
/// anchors. The settle job re-offers `doc` to the waiter of the owner, the commit stores no
/// second copy of `doc`, and CR-20 books the refused pair, so the next job ends the deploy.
/// The first runtime finds the salted labels, and the other runtimes reuse them from
/// `labels`. The control plays the same deploys on a native chain, where the waiter of the owner
/// fires at the cap.
async fn vdb_list_cap_case(workers: usize, labels: &mut Vec<String>) {
    let label = format!("T-C4-21 VDB list 6, {workers} workers");
    // Added by decision 112 (plan-112-protobuf.md 1.5 and 1.6, ruling RADD2v2 8.4 item 5): every
    // runtime creates the space with the label of the first runtime. The space id and the names
    // `a` and `b` follow from that label, and the hash of each parked waiter follows from them.
    // So the salted labels that the first runtime finds keep each waiter in its band on every
    // runtime.
    let setup = format!("T-C4-21 VDB list 6, {} workers", RACING_WORKERS[0]);
    let creation = stashing(
        "rho:space:hashmap:vectordb",
        VECTORDB_CTOR,
        REFUSAL_STASH,
        REOFFER_RECORDS,
    );
    let client = list_waiters(1, "doc");
    let mut waiters = Vec::<(Role, &str, Band)>::with_capacity(FIRE_ROUND_CAP);
    for _ in 0..FIRE_ROUND_CAP - 2 {
        waiters.push((Role::UnfundedClient, client.as_str(), Band::Below));
    }
    waiters.push((Role::Owner, CAP_OWNER_WAITER, Band::Middle));
    waiters.push((Role::UnfundedClient, CAP_ALTERNATIVE_WAITER, Band::Above));
    let mut chain = OwnedChain::new().await;
    // Changed by decision 112 (plan-112-protobuf.md 1.5 and 1.6, ruling RADD2v2 8.4 item 5): the
    // space of the first runtime. The text before:
    // let mut last = steps_setup(&mut chain, &label, &creation, &[]).await;
    let mut last = steps_setup(&mut chain, &setup, &creation, &[]).await;
    for (index, &(role, body, band)) in waiters.iter().enumerate() {
        last = match labels.get(index) {
            Some(salted) => park_salted(&mut chain, (role, body), salted).await,
            None => {
                let waiter = format!("{label}: waiter {index}");
                let (parked, salted) =
                    park_in_band(&mut chain, &last.root, (role, body), &waiter, band).await;
                labels.push(salted);
                parked
            }
        };
    }
    let alternative = parked_waiter(&label, &last);
    let removal = with_stash(REFUSAL_STASH, REOFFER_REMOVAL);
    let trigger = format!("{label}: the removal of img");
    let played = chain.deploy(Role::UnfundedClient, &removal, &trigger).await;
    assert_runs(&trigger, &played);
    assert_eq!(
        (
            deliveries(&played, "cascade"),
            deliveries(&played, "alternative"),
            deliveries(&played, "owner")
        ),
        (FIRE_ROUND_CAP - 2, 1, 0),
        "{trigger}: the waiters of the client before the owner fire, the owner refuses its waiter \
         at the cap, and the alternative fires in its place"
    );
    let (_, journal) = journal_of(&played);
    let before_anchor = match anchor_indices(journal).first() {
        Some(&anchor) => anchor,
        None => journal.len(),
    };
    assert_eq!(
        comms_of(&journal[..before_anchor], &alternative),
        1,
        "{trigger}: the alternative fires at the cap, before any anchor"
    );
    assert_eq!(
        stored_record_copies(&played),
        1,
        "{trigger}: the re-offer of the stored persistent doc leaves its one copy"
    );
    let mut control = NativeChain::new().await;
    // Changed by decision 112 (plan-112-protobuf.md 1.5 and 1.6, ruling RADD2v2 8.4 item 5): the
    // space of the first runtime. The text before:
    // steps_setup(&mut control, &label, &creation, &[]).await;
    steps_setup(&mut control, &setup, &creation, &[]).await;
    for (&(role, body, _), salted) in waiters.iter().zip(labels.iter()) {
        park_salted(&mut control, (role, body), salted).await;
    }
    let played = control
        .deploy(Role::UnfundedClient, &removal, &trigger)
        .await;
    assert_runs(&trigger, &played);
    assert_eq!(
        deliveries(&played, "owner"),
        1,
        "{trigger}: without owner funding, the waiter of the owner fires at the cap"
    );
}

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21, ruling RADD2v2 7.1): the
/// trigger of the SCAN cases: a persistent receive without a peek of any value on `a`.
const SCAN_TRIGGER: &str = r#"for (@x <= a) { out!(["each", x]) }"#;

/// Added by decision 124 (B9, T-C4-21): the trigger of the control of a SCAN case: a receive
/// without a peek of any value on `a`. A persistent receive of a persistent record on a kind
/// without a counter diverges for free (decision 43), so the control takes one record only.
const SCAN_CONTROL_TRIGGER: &str = r#"for (@x <- a) { out!(["each", x]) }"#;

/// Added by decision 124 (B9, T-C4-21, ruling RADD2v2 7.1): a case of T-C4-21 on a scan kind.
/// Its trigger is [`SCAN_TRIGGER`].
struct ScanCase {
    /// The name of the case in the labels: the test, the kind and the case.
    name: &'static str,
    /// The chain up to the trigger. The records of the client hold `"c"`, and the records of the
    /// owner hold `"o"`.
    chain: RefusalChain,
    /// The value of a class of two members of a set whose first member in the canonical order of
    /// DR-75 the setup salts to be the member that is not persistent, or None.
    salted: Option<&'static str>,
    /// The native phlo limit of each run.
    limit: u64,
    /// The branches of Tail(actual) that a run may take.
    allowed: &'static [TailBranch],
    /// The branch that some run on each runtime takes.
    needed: TailBranch,
    /// The runs of the trigger.
    runs: usize,
    /// The checks of a run, with its label.
    check: fn(&str, &Played),
}

// Retired by decision 135 (the SCAN divergent cases, ruling RADD2v2 7.1): a divergent pair on a
// kind without a counter cannot end by phlo, because each round is a free retry of the base
// recorder.

/// Added by decision 124 (B9, T-C4-21 SCAN cases 2, ruling RADD2v2 7.1): the checks of a run that
/// fires the record of the client once: the deploy runs to its end, the receive delivers `"c"`
/// once, and the receive ends parked.
fn each_client_once(label: &str, played: &Played) {
    assert_runs(label, played);
    assert_eq!(
        delivered_strings(played, "each"),
        ["c"],
        "{label}: the receive delivers the record of the client once"
    );
    parked_waiter(label, played);
}

/// Added by decision 124 (B9, T-C4-21 SCAN cases 3, ruling RADD2v2 7.1): the checks of a run whose
/// only record is a record of the owner: the deploy runs to its end, the receive delivers
/// nothing, and the receive parks.
fn each_parks(label: &str, played: &Played) {
    assert_runs(label, played);
    assert_eq!(
        deliveries(played, "each"),
        0,
        "{label}: the owner refuses its record, and the receive delivers nothing"
    );
    parked_waiter(label, played);
}

/// Added by decision 124 (B9, T-C4-21, ruling RADD2v2 7.1): the control of a SCAN case: true when
/// the receive delivers the record of the owner.
fn owner_record_fires(played: &Played) -> bool {
    delivered_strings(played, "each")
        .iter()
        .any(|value| value == "o")
}

/// Added by decision 124 (B9, T-C4-21, ruling RADD2v2 7.1): true when the first member of the class
/// `class` of a set, in the canonical order of DR-75, is not persistent, in the post-state of
/// the deploy `played`. The deploy stores its records on one channel, and the class holds the
/// records of that channel whose value is the string `class`.
fn linear_first(played: &Played, class: &str) -> bool {
    let (_, journal) = journal_of(played);
    let mut channels = BTreeSet::<[u8; 32]>::new();
    for row in journal {
        match (&row.source, &row.comm) {
            (NativeOperationSource::Produce(record), None) if is_space_row(row) => {
                channels.insert(record.channel);
            }
            (NativeOperationSource::Produce(_), _) | (NativeOperationSource::Consume(_), _) => {}
        }
    }
    let channels: Vec<[u8; 32]> = channels.into_iter().collect();
    let [channel] = channels.as_slice() else {
        panic!("the deploy stores its records on one channel: {channels:?}");
    };
    let data = played
        .history
        .get_history_reader(&played.root)
        .expect("read the post-state")
        .get_data(&Blake2b256Hash::from_bytes(channel.to_vec()))
        .expect("the records of the channel");
    let mut members = Vec::<([u8; 32], bool)>::with_capacity(data.len());
    for datum in &data {
        let source: [u8; 32] = datum
            .source
            .hash
            .bytes()
            .try_into()
            .expect("a source hash has 32 bytes");
        let value = set_value(&datum.a);
        match string_of(&value) == class {
            false => {}
            true => members.push((source, datum.persist)),
        }
    }
    members.sort_unstable_by_key(|(source, _)| *source);
    match members.first() {
        Some((_, persist)) => !persist,
        None => panic!("the class {class} has no member"),
    }
}

/// Added by decision 124 (rule CR-19 case 3, F22): the copies, in the post-state of the deploy
/// `played`, of its one persistent space record that a row without a COMM stored.
fn stored_record_copies(played: &Played) -> usize {
    let (_, journal) = journal_of(played);
    let mut records = BTreeSet::<([u8; 32], [u8; 32])>::new();
    for row in journal {
        match (&row.source, &row.comm) {
            (NativeOperationSource::Produce(record), None)
                if record.persistent && is_space_row(row) =>
            {
                records.insert((record.channel, record.hash));
            }
            (NativeOperationSource::Produce(_), _) | (NativeOperationSource::Consume(_), _) => {}
        }
    }
    let records: Vec<([u8; 32], [u8; 32])> = records.into_iter().collect();
    let [(channel, hash)] = records.as_slice() else {
        panic!("the deploy stores one persistent space record: {records:?}");
    };
    let data = played
        .history
        .get_history_reader(&played.root)
        .expect("read the post-state")
        .get_data(&Blake2b256Hash::from_bytes(channel.to_vec()))
        .expect("the records of the channel");
    data.iter()
        .filter(|datum| datum.source.hash.bytes() == hash.as_slice())
        .count()
}

/// Added by decision 124 (B9, T-C4-21, ruling RADD2v2 7.1): plays the chain of `case` on a fresh
/// owned chain with the label `label`. When the case salts a class, it tries the labels
/// `"{label}, salt {salt}"` in order, each on a fresh owned chain, until the first member of the
/// class in the canonical order of DR-75 is not persistent. The hashes of a deploy follow from
/// its label and its term alone, so every run finds the same salt. Returns the chain, the play
/// of the last deploy of the setup, and the label of the setup.
async fn scan_setup(case: &ScanCase, label: &str) -> (OwnedChain, Played, String) {
    let Some(class) = case.salted else {
        let mut chain = OwnedChain::new().await;
        let setup = refusal_setup(&mut chain, label, &case.chain).await;
        return (chain, setup, label.to_string());
    };
    for salt in 0..SALTS {
        let salted = format!("{label}, salt {salt}");
        let mut chain = OwnedChain::new().await;
        let setup = refusal_setup(&mut chain, &salted, &case.chain).await;
        match linear_first(&setup, class) {
            false => {}
            true => return (chain, setup, salted),
        }
    }
    panic!("{label}: no salt below {SALTS} puts a member that is not persistent first in {class}");
}

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21, ruling RADD2v2 7.1): plays the
/// SCAN case `case`: its setup once (`scan_setup`), then [`SCAN_TRIGGER`] `case.runs` times from
/// the state before the trigger, at the limit of the case. Each run takes an allowed branch of
/// Tail(actual), passes the checks of the case, and certifies through its own native replay at
/// that limit (`deploy_limited`). Some run takes the needed branch. The control plays the setup
/// with the same labels on a native chain and then [`SCAN_CONTROL_TRIGGER`] at
/// [`DIVERGENT_LIMIT`], and some run of it delivers the record of the owner.
async fn scan_case(workers: usize, case: &ScanCase) {
    let label = format!("{}, {workers} workers", case.name);
    let (mut chain, setup, label) = scan_setup(case, &label).await;
    let term = with_stash(REFUSAL_STASH, SCAN_TRIGGER);
    let mut needed = 0_usize;
    for run in 0..case.runs {
        let trigger = format!("{label}: the trigger, run {run}");
        chain.rewind(&setup.root).await;
        let played = chain
            .deploy_limited(Role::UnfundedClient, &term, &trigger, case.limit)
            .await;
        let branch = tail_branch(&trigger, &played);
        assert!(
            case.allowed.contains(&branch),
            "{trigger}: Tail(actual) takes {branch:?}, not one of {:?}",
            case.allowed
        );
        needed += usize::from(branch == case.needed);
        (case.check)(&trigger, &played);
    }
    assert!(needed > 0, "{label}: some run takes {:?}", case.needed);
    let mut control = NativeChain::new().await;
    let unfunded = refusal_setup(&mut control, &label, &case.chain).await;
    let term = with_stash(REFUSAL_STASH, SCAN_CONTROL_TRIGGER);
    let mut fired = 0_usize;
    for run in 0..case.runs {
        let trigger = format!("{label}: the trigger, run {run}");
        control.reset(&unfunded.root).await;
        let played = control.play(&term, &trigger, DIVERGENT_LIMIT).await;
        let certification = certify(&played, &term, DIVERGENT_LIMIT).await;
        assert_reproduces(&trigger, &played, &certification);
        fired += usize::from(owner_record_fires(&played));
    }
    assert!(
        fired > 0,
        "{label}: without owner funding, the record of the owner fires in some run"
    );
}

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21, ruling RADD2v2 7.1): the four
/// SCAN cases, on a set and on a bag.
const T21_SCAN_CASES: [ScanCase; 4] = [
    // Retired by decision 135 (the SCAN divergent cases, ruling RADD2v2 7.1): a divergent pair
    // on a kind without a counter cannot end by phlo, because each round is a free retry of the
    // base recorder.
    ScanCase {
        name: "T-C4-21 SCAN set S2",
        chain: RefusalChain {
            urn: "rho:space:hashmap:set",
            ctor: r#""default""#,
            client: r#"a!("c")"#,
            steps: &[(Role::Owner, r#"a!("o") | a!!("o")"#)],
        },
        salted: Some("o"),
        limit: AMPLE_LIMIT,
        allowed: &[TailBranch::Return, TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: PICK_RUNS,
        check: each_client_once,
    },
    ScanCase {
        name: "T-C4-21 SCAN set S3",
        chain: RefusalChain {
            urn: "rho:space:hashmap:set",
            ctor: r#""default""#,
            client: "Nil",
            steps: &[(Role::Owner, r#"a!("o")"#)],
        },
        salted: None,
        limit: AMPLE_LIMIT,
        allowed: &[TailBranch::Return],
        needed: TailBranch::Return,
        runs: PICK_RUNS,
        check: each_parks,
    },
    // Retired by decision 135 (the SCAN divergent cases, ruling RADD2v2 7.1): a divergent pair
    // on a kind without a counter cannot end by phlo, because each round is a free retry of the
    // base recorder.
    ScanCase {
        name: "T-C4-21 SCAN bag B2",
        chain: RefusalChain {
            urn: "rho:space:hashmap:bag",
            ctor: r#""default""#,
            client: r#"a!("c")"#,
            steps: &[(Role::Owner, r#"a!("o")"#)],
        },
        salted: None,
        limit: AMPLE_LIMIT,
        allowed: &[TailBranch::Return, TailBranch::Stop],
        needed: TailBranch::Stop,
        runs: PICK_RUNS,
        check: each_client_once,
    },
    ScanCase {
        name: "T-C4-21 SCAN bag B3",
        chain: RefusalChain {
            urn: "rho:space:hashmap:bag",
            ctor: r#""default""#,
            client: "Nil",
            steps: &[(Role::Owner, r#"a!("o")"#)],
        },
        salted: None,
        limit: AMPLE_LIMIT,
        allowed: &[TailBranch::Return],
        needed: TailBranch::Return,
        runs: PICK_RUNS,
        check: each_parks,
    },
];

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21, ruling RADD2v2 7.2): the
/// waiters of a send whose fire the commit re-picks with a step owed after it (case 2 of
/// T-C4-21). The owner parks a take of any value on `a`, and the client parks a take of any
/// value on `a`.
const SEND_REPICK_STOP_STEPS: &[(Role, &str)] = &[
    (Role::Owner, r#"for (@v <- a) { out!(["owner", v]) }"#),
    (
        Role::UnfundedClient,
        r#"for (@v <- a) { out!(["client", v]) }"#,
    ),
];

/// Added by decision 124 (B9, T-C4-21, ruling RADD2v2 7.2): the waiter of a send whose fire the
/// commit refuses with no other waiter, so the commit stores the send (case 3 of T-C4-21).
const SEND_REPICK_STORE_STEPS: &[(Role, &str)] =
    &[(Role::Owner, r#"for (@v <- a) { out!(["owner", v]) }"#)];

// Retired by decision 135 (the SCAN divergent cases, ruling RADD2v2 7.1): a divergent pair on a
// kind without a counter cannot end by phlo, because each round is a free retry of the base
// recorder.

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21, ruling RADD2v2 7.2): cases 2
/// and 3 of T-C4-21 on the scan space of the family `urn` for a send whose fire the commit binds
/// in place of c0. The client sends 1 forever.
/// - Case 2 runs [`PICK_RUNS`] times. When the pick order of play meets the take of the owner
///   first, the commit refuses its fire (R5), and the search fires the take of the client
///   instead. The send stays and the take is spent, so the send goes on, and the finish stops
///   with the anchor first. The re-issued send meets the take of the owner alone, and the commit
///   stores it. A run meets the take of the owner first with a chance of 1/2. The control plays
///   the chain without owner funding, where the take of the owner fires.
/// - Case 3: the take of the owner is the only waiter, so the commit stores the send, and the
///   finish returns nothing.
///
/// Each deploy certifies through its own native replay (rule CR-28).
async fn send_repick_stop_and_store_cases(urn: &'static str, label: &str) {
    let stop_chain = RefusalChain {
        urn,
        steps: SEND_REPICK_STOP_STEPS,
        ..DIVERGENT_CHAIN
    };
    let case_label = format!("{label}, case 2");
    let mut chain = OwnedChain::new().await;
    let setup = refusal_setup(&mut chain, &case_label, &stop_chain).await;
    let mut stops = 0_usize;
    for run in 0..PICK_RUNS {
        let trigger = format!("{case_label}: the send of 1 forever, run {run}");
        let played = refusal_trigger(&mut chain, &setup.root, "a!!(1)", &trigger).await;
        assert_runs(&trigger, &played);
        let (_, journal) = journal_of(&played);
        assert_eq!(
            (
                deliveries(&played, "client"),
                deliveries(&played, "owner"),
                space_stores(journal)
            ),
            (1, 0, 1),
            "{trigger}: the take of the client fires once, the owner refuses, and the send stores"
        );
        match tail_branch(&trigger, &played) {
            TailBranch::Return => {}
            TailBranch::Stop => stops += 1,
        }
    }
    assert!(
        stops > 0,
        "{case_label}: some run meets the take of the owner first and stops"
    );
    let mut control = NativeChain::new().await;
    let control_setup = refusal_setup(&mut control, &case_label, &stop_chain).await;
    let trigger = format!("{case_label}: the send of 1 forever, without owner funding");
    let played = refusal_trigger(&mut control, &control_setup.root, "a!!(1)", &trigger).await;
    assert_runs(&trigger, &played);
    assert_eq!(
        deliveries(&played, "owner"),
        1,
        "{trigger}: without owner funding the take of the owner fires"
    );
    let store_chain = RefusalChain {
        urn,
        steps: SEND_REPICK_STORE_STEPS,
        ..DIVERGENT_CHAIN
    };
    let case_label = format!("{label}, case 3");
    let mut chain = OwnedChain::new().await;
    let setup = refusal_setup(&mut chain, &case_label, &store_chain).await;
    let trigger = format!("{case_label}: the send of 1 forever");
    let played = refusal_trigger(&mut chain, &setup.root, "a!!(1)", &trigger).await;
    assert_runs(&trigger, &played);
    let (_, journal) = journal_of(&played);
    assert_eq!(
        (
            tail_branch(&trigger, &played),
            deliveries(&played, "owner"),
            space_comms(journal),
            space_stores(journal)
        ),
        (TailBranch::Return, 0, 0, 1),
        "{trigger}: the commit refuses the take of the owner and stores the send"
    );
}

/// Added by decision 124 (plan of the batch B tests, B9, T-C4-21, ADD A2.9, ADD2 S9.2, rules
/// CR-28 and CR-31, rulings RADD2v2 7.1 and 7.2): a deviating row ends Tail(actual) in play as
/// replay ends it, for each kind and each case of ADD A2.9 that a chain reaches. Each case plays
/// a chain whose trigger deviates at row d under owner funding, and every run certifies through
/// its own native replay (D-OBL-1). Each run checks the branch of Tail(actual) by the anchors of
/// its journal and checks its deliveries. A case whose runs can end without an anchor, or whose
/// Stop can come by two paths, has a control on a native chain. Play picks at random in the set,
/// the VectorDB and the bag, so those cases run [`PICK_RUNS`] times: a run meets the refused
/// candidate first with a chance of 1/3 or more [Inference], so all runs miss it with a chance of
/// `(2/3)^64`, below `10^-11`.
#[test]
fn mark_free_finish_equals_replay_for_each_discipline() {
    let mut list_labels = Vec::<String>::with_capacity(FIRE_ROUND_CAP);
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            for case in &T21_CASES {
                tail_case(workers, case).await;
            }
            lanes_cap_case(workers).await;
            prefix_cap_case(workers).await;
            vdb_list_cap_case(workers, &mut list_labels).await;
            for case in &T21_SCAN_CASES {
                scan_case(workers, case).await;
            }
            let label = format!("T-C4-21, the set, {workers} workers");
            // Retired by decision 135 (the SCAN divergent cases, ruling RADD2v2 7.1): a divergent
            // pair on a kind without a counter cannot end by phlo, because each round is a free
            // retry of the base recorder.
            send_repick_stop_and_store_cases("rho:space:hashmap:set", &label).await;
            let label = format!("T-C4-21, the bag, {workers} workers");
            send_repick_stop_and_store_cases("rho:space:hashmap:bag", &label).await;
        });
    }
}

/// Added by decision 124 (plan of the batch B tests, B11): plays each case of `cases` with the
/// runner of B8 ([`tail_case`]) on each runtime of `RACING_WORKERS` (decision 111).
fn tail_cases_on_each_runtime(cases: &[TailCase]) {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            for case in cases {
                tail_case(workers, case).await;
            }
        });
    }
}

/// Added by decision 124 (plan of the batch B tests, B11, RIBA 2.4): the receive of the client in
/// the receive case of T1: a take of any value on `a`, with no query.
const VDB_PLAIN_RECEIVE: &str = r#"for (@v <- a) { out!(["each", v]) }"#;

/// Added by decision 124 (B11, RIBA 2.4): the guarded receive of the client in the park case of
/// T1. Its guard admits the record of the owner, so play fires it, and the guard makes its commit
/// one that may veto, so replay parks it at the E3-a park of the receive (CR-7).
const VDB_GUARDED_RECEIVE: &str = r#"for (@v <- a where v == "o") { out!(["each", v]) }"#;

/// Added by decision 124 (B11, RIBA 2.4): the receive of the client in the case of T2: a take of
/// the best record for the query `[1.0, 0.0]`, in single mode.
const VDB_SINGLE_RECEIVE: &str = r#"for (@v <- a ~ ([1.0, 0.0])) { out!(["each", v]) }"#;

/// Added by decision 124 (B11, RIBA 2.4): the checks of the receive case of T1: the receive binds
/// the record of the client, and only it.
fn each_takes_the_client_record(label: &str, played: &Played) {
    assert_eq!(
        delivered_strings(played, "each"),
        ["c"],
        "{label}: the owner refuses its record, and the receive binds the record of the client"
    );
}

/// Added by decision 124 (B11, RIBA 2.4): the checks of the case of T2: the receive binds the
/// second record of the score order, and only it.
fn each_takes_the_second_record(label: &str, played: &Played) {
    assert_eq!(
        delivered_strings(played, "each"),
        ["second"],
        "{label}: the owner refuses the best record, and the receive binds the next one"
    );
}

/// Added by decision 124 (B11, RIBA 2.4): the control of the case of T2: true when the receive
/// binds the best record, the record of the owner.
fn first_record_fires(played: &Played) -> bool {
    delivered_strings(played, "each")
        .iter()
        .any(|value| value == "first")
}

/// Added by decision 124 (plan of the batch B tests, B11, RIBA 2.4, C2v3 1.30): the cases of T1
/// that T-C4-21 does not play: the one-bind receive fire and the E3-a park of replay. VDB 1 and
/// VDB 4 of T-C4-21 play the offer and the cascade re-offer.
const VDB_SITE_CASES: [TailCase; 2] = [
    TailCase {
        name: "RIBA 2.4 T1, the receive fire",
        chain: RefusalChain {
            urn: "rho:space:hashmap:vectordb",
            ctor: VECTORDB_CTOR,
            client: r#"a!("c")"#,
            steps: &[(Role::Owner, r#"a!("o")"#)],
        },
        trigger: VDB_PLAIN_RECEIVE,
        allowed: &[TailBranch::Return],
        needed: TailBranch::Return,
        runs: PICK_RUNS,
        most_anchors: 1,
        check: each_takes_the_client_record,
        control: Some(owner_record_fires),
    },
    TailCase {
        name: "RIBA 2.4 T1, the E3-a park",
        chain: RefusalChain {
            urn: "rho:space:hashmap:vectordb",
            ctor: VECTORDB_CTOR,
            client: "Nil",
            steps: &[(Role::Owner, r#"a!("o")"#)],
        },
        trigger: VDB_GUARDED_RECEIVE,
        allowed: &[TailBranch::Return],
        needed: TailBranch::Return,
        runs: 1,
        most_anchors: 1,
        check: each_parks,
        control: Some(owner_record_fires),
    },
];

/// Added by decision 124 (B11, RIBA 2.4, C2v3 1.28): the case of T2. The client creates the space
/// with `second` and `third`, and the owner stores `first`, which the query `[1.0, 0.0]` ranks
/// first.
const VDB_SINGLE_CASE: TailCase = TailCase {
    name: "RIBA 2.4 T2, the single mode",
    chain: RefusalChain {
        urn: "rho:space:hashmap:vectordb",
        ctor: VECTORDB_CTOR,
        client: r#"a!("second"; embedding=[10.0, 1.0]) | a!("third"; embedding=[10.0, 2.0])"#,
        steps: &[(Role::Owner, r#"a!("first"; embedding=[10.0, 0.0])"#)],
    },
    trigger: VDB_SINGLE_RECEIVE,
    allowed: &[TailBranch::Return],
    needed: TailBranch::Return,
    runs: 1,
    most_anchors: 1,
    check: each_takes_the_second_record,
    control: Some(first_record_fires),
};

/// Added by decision 124 (plan of the batch B tests, B11, RIBA 2.4, rule CR-28): T1.
/// Each owner-funded site of C2v3 1.30 stages a row search, so the commit can refuse a candidate
/// of the owner, whose item test fails at budget 0, and take the outcome of the search. The
/// receive case refuses the record of the owner and binds the record of the client. The park
/// case refuses the only record, and replay parks the guarded receive at its E3-a park with the
/// search. Each run certifies through its own native replay. VDB 1 and VDB 4 of T-C4-21 show
/// the offer and the cascade re-offer. The half "None" is the count part of T-P-1 (RIBA 2.4).
#[test]
fn vdb_search_is_none_without_owner_funding() { tail_cases_on_each_runtime(&VDB_SITE_CASES); }

/// Added by decision 124 (plan of the batch B tests, B11, RIBA 2.4, rule CR-28): T2.
/// A single-mode receive whose best record the commit refuses binds the next entry of the score
/// order. The plain read of the receive case of T1 binds the next entry of its pick order.
#[test]
fn vdb_receive_selection_hands_out_its_pick_order() {
    tail_cases_on_each_runtime(&[VDB_SINGLE_CASE]);
}

/// Added by decision 124 (plan of the batch B tests, B11, RIBA 2.4, C2v3 3.4): the take of the
/// owner in the builder case, which parks beside the inert `K_lift`.
const BUILDER_OWNER_WAITER: &str = r#"for (@"fourth" <- a) { out!(["owner", "fourth"]) }"#;

/// Added by decision 124 (B11, RIBA 2.4): the take of the client in the builder case.
const BUILDER_CLIENT_WAITER: &str = r#"for (@"fourth" <- a) { out!(["client", "fourth"]) }"#;

/// Added by decision 124 (B11, RIBA 2.4): the send of the client in the builder case.
const BUILDER_SEND: &str = r#"a!("fourth"; embedding=[0.0, 1.0])"#;

/// Added by decision 124 (B11, RIBA 2.4): the chain of the control of the builder case: the two
/// takes of `fourth` on a VectorDB space with no inert `K_lift`.
const BUILDER_CONTROL_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:vectordb",
    ctor: VETO_LIST_CTOR,
    client: "Nil",
    steps: &[
        (Role::Owner, BUILDER_OWNER_WAITER),
        (Role::UnfundedClient, BUILDER_CLIENT_WAITER),
    ],
};

/// Added by decision 124 (plan of the batch B tests, B11, RIBA 2.4, rules CR-16 and
/// CR-28): the VDB builder case of `readers_skip_an_inert_machinery_waiter`. The chain of ADD3
/// A3.2.1 parks an inert `K_lift` on `a` (`refused_lift_case`). Then the owner and the client
/// park their takes of `fourth`, and the client sends `fourth` [`PICK_RUNS`] times. When the
/// pick order of play meets the take of the owner first, the commit refuses its fire (R5), and
/// the row search of the offer gives the next waiter of its order. The lift pattern binds every
/// value, so a search that listed the inert `K_lift` could bind it. The search skips it, so the
/// take of the client fires. Each run delivers to the client once and to the owner never, and
/// certifies through its own native replay. The control plays the two takes on a native chain,
/// where the take of the owner fires in some run.
#[test]
fn vdb_builder_skips_an_inert_machinery_waiter() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("RIBA 2.4 (VDB builder), {workers} workers");
            let mut chain = OwnedChain::new().await;
            refused_lift_case(&mut chain, &label, &list_lift_chain(false)).await;
            let owner = with_stash(LIFT_STASH, BUILDER_OWNER_WAITER);
            let owner_label = format!("{label}: the take of the owner");
            let parked = chain.deploy(Role::Owner, &owner, &owner_label).await;
            assert_runs(&owner_label, &parked);
            parked_waiter(&owner_label, &parked);
            let client = with_stash(LIFT_STASH, BUILDER_CLIENT_WAITER);
            let client_label = format!("{label}: the take of the client");
            let parked = chain
                .deploy(Role::UnfundedClient, &client, &client_label)
                .await;
            assert_runs(&client_label, &parked);
            parked_waiter(&client_label, &parked);
            let send = with_stash(LIFT_STASH, BUILDER_SEND);
            for run in 0..PICK_RUNS {
                let trigger = format!("{label}: the send, run {run}");
                chain.rewind(&parked.root).await;
                let played = chain.deploy(Role::UnfundedClient, &send, &trigger).await;
                assert_runs(&trigger, &played);
                client_fires_once(&trigger, &played);
            }
            let mut control = NativeChain::new().await;
            let unfunded = refusal_setup(&mut control, &label, &BUILDER_CONTROL_CHAIN).await;
            let mut fired = 0_usize;
            for run in 0..PICK_RUNS {
                let trigger = format!("{label}: the send, run {run}");
                let played =
                    refusal_trigger(&mut control, &unfunded.root, BUILDER_SEND, &trigger).await;
                assert_runs(&trigger, &played);
                fired += usize::from(owner_fires(&played));
            }
            assert!(
                fired > 0,
                "{label}: without owner funding, the take of the owner fires in some run"
            );
        });
    }
}

/// Added by decision 124 (plan of the batch B tests, B12, RIBA 3.4): the guarded receive of the
/// client in the park cases of `scan_search_is_none_without_owner_funding`. Its guard admits the
/// record of the owner, so play fires its class, and the guard makes its commit one that may
/// veto, so replay parks it at the E3-a park of the receive (CR-7).
const SCAN_GUARDED_RECEIVE: &str = r#"for (@x <- a where x == "o") { out!(["each", x]) }"#;

/// Added by decision 124 (B12, RIBA 3.4): the receive of the client in the receive retire case: a
/// take of any value on `a`.
const SCAN_PLAIN_RECEIVE: &str = r#"for (@x <- a) { out!(["each", x]) }"#;

/// Added by decision 124 (plan of the batch B tests, B12, RIBA 3.4, C2v3 1.26): the park cases of
/// `scan_search_is_none_without_owner_funding`, on a set and on a bag. The owner stores its record
/// alone, so the commit refuses the fire of the guarded receive with no alternative.
const SCAN_PARK_CASES: [TailCase; 2] = [
    TailCase {
        name: "RIBA 3.4, the E3-a park of a set",
        chain: RefusalChain {
            urn: "rho:space:hashmap:set",
            ctor: r#""default""#,
            client: "Nil",
            steps: &[(Role::Owner, r#"a!("o")"#)],
        },
        trigger: SCAN_GUARDED_RECEIVE,
        allowed: &[TailBranch::Return],
        needed: TailBranch::Return,
        runs: 1,
        most_anchors: 1,
        check: each_parks,
        control: Some(owner_record_fires),
    },
    TailCase {
        name: "RIBA 3.4, the E3-a park of a bag",
        chain: RefusalChain {
            urn: "rho:space:hashmap:bag",
            ctor: r#""default""#,
            client: "Nil",
            steps: &[(Role::Owner, r#"a!("o")"#)],
        },
        trigger: SCAN_GUARDED_RECEIVE,
        allowed: &[TailBranch::Return],
        needed: TailBranch::Return,
        runs: 1,
        most_anchors: 1,
        check: each_parks,
        control: Some(owner_record_fires),
    },
];

/// Added by decision 124 (plan of the batch B tests, B12, RIBA 3.4, C2v3 1.25): the offer retire
/// case. The send of 1 by the client stores beside the waiter of the owner, which refuses it.
/// Then the trigger sends 1 again: its offer to that waiter stages one retire row, the stored 1.
const SCAN_OFFER_RETIRE_CASE: TailCase = TailCase {
    name: "RIBA 3.4, the offer retire",
    chain: RefusalChain {
        urn: "rho:space:hashmap:set",
        ctor: r#""default""#,
        client: "Nil",
        steps: &[(Role::Owner, OWNER_WAITER), (Role::UnfundedClient, "a!(1)")],
    },
    trigger: "a!(1)",
    allowed: &[TailBranch::Return],
    needed: TailBranch::Return,
    runs: 1,
    most_anchors: 1,
    check: owner_never_delivers,
    control: None,
};

/// Added by decision 124 (plan of the batch B tests, B12, RIBA 3.4, C2v3 1.26 and 2.16): the
/// receive retire case. The class `"o"` of the owner and the class `"c"` of the client have two
/// members each, so the fire of each class stages one retire row.
const SCAN_RECEIVE_RETIRE_CASE: TailCase = TailCase {
    name: "RIBA 3.4, the receive retire",
    chain: RefusalChain {
        urn: "rho:space:hashmap:set",
        ctor: r#""default""#,
        client: r#"a!("c") | a!("c")"#,
        steps: &[(Role::Owner, r#"a!("o") | a!("o")"#)],
    },
    trigger: SCAN_PLAIN_RECEIVE,
    allowed: &[TailBranch::Return],
    needed: TailBranch::Return,
    runs: PICK_RUNS,
    most_anchors: 1,
    check: each_takes_the_client_record,
    control: Some(owner_record_fires),
};

/// Added by decision 124 (plan of the batch B tests, B12, RIBA 3.4, C2v3 3.4): the builder case.
/// The send of 1 by the client fires the take of the client, which the stored 1 of the owner
/// parked, and the retire row of that record parks its `K_retire` inert. Then the owner and the
/// client park their takes of 2.
const SCAN_BUILDER_CASE: TailCase = TailCase {
    name: "RIBA 3.4, the SCAN builder",
    chain: RefusalChain {
        urn: "rho:space:hashmap:set",
        ctor: r#""default""#,
        client: "Nil",
        steps: &[
            (Role::Owner, "a!(1)"),
            // Its own kind of delivery, so `client_fires_once` counts the trigger alone.
            (
                Role::UnfundedClient,
                r#"for (@v <- a) { out!(["setup", v]) }"#,
            ),
            (Role::UnfundedClient, "a!(1)"),
            (Role::Owner, r#"for (@2 <- a) { out!(["owner", 2]) }"#),
            (
                Role::UnfundedClient,
                r#"for (@2 <- a) { out!(["client", 2]) }"#,
            ),
        ],
    },
    trigger: "a!(2)",
    allowed: &[TailBranch::Return],
    needed: TailBranch::Return,
    runs: PICK_RUNS,
    most_anchors: 1,
    check: client_fires_once,
    control: Some(owner_fires),
};

/// Added by decision 124 (plan of the batch B tests, B12, RIBA 3.4, rule CR-28): each
/// owner-funded site of C2v3 1.25 and 1.26 stages a row search. The park cases refuse the only
/// record, and replay parks the guarded receive at its E3-a park with the search, on a set and
/// on a bag. The NOR send cases and S1 to B3 of T-C4-21 show the offer and the class fire. The
/// half "None" is the count part of T-P-1 (RIBA 2.4).
#[test]
fn scan_search_is_none_without_owner_funding() { tail_cases_on_each_runtime(&SCAN_PARK_CASES); }

/// Added by decision 124 (plan of the batch B tests, B12, RIBA 3.4, rule CR-28): the
/// commit refuses an offer fire whose staging put one retire row after it, skips that row, and
/// stores the send, so the deploy certifies.
#[test]
fn scan_offer_search_counts_the_staged_retires() {
    tail_cases_on_each_runtime(&[SCAN_OFFER_RETIRE_CASE]);
}

/// Added by decision 124 (plan of the batch B tests, B12, RIBA 3.4, C2v3 2.16, rules
/// CR-28 and CR-33): the commit refuses a class fire whose staging put one retire row after it,
/// skips that row, and binds the other class, whose actual fire retires its second member
/// through `RetireRows::of`, so the deploy certifies.
#[test]
fn scan_receive_search_counts_the_staged_retires() {
    tail_cases_on_each_runtime(&[SCAN_RECEIVE_RETIRE_CASE]);
}

/// Added by decision 124 (plan of the batch B tests, B12, RIBA 3.4, rules CR-16 and
/// CR-28): the SCAN builder case of `readers_skip_an_inert_machinery_waiter`. An inert `K_retire`
/// sits on `a` beside the takes of 2 of the owner and of the client. When the pick order of the
/// send of 2 meets the take of the owner first, the commit refuses its fire (R5), and the row
/// search of the offer skips the inert `K_retire`, whose pattern binds every payload, and fires
/// the take of the client.
#[test]
fn scan_builder_skips_an_inert_machinery_waiter() {
    tail_cases_on_each_runtime(&[SCAN_BUILDER_CASE]);
}

/// Added by decision 124 (plan of the batch B tests, B13, RIBA 4.3, C2v3 1.6): the cases of
/// `head_search_is_none_without_owner_funding` on a queue, on its own route: the offer of the head
/// and the receive of the head, each refused with no alternative. STK 4 and PQ 4 of T-C4-21
/// refuse the offer of the head on a stack and on a priority queue.
const HEAD_SITE_CASES: [TailCase; 2] = [
    TailCase {
        name: "RIBA 4.3, the offer of the head",
        chain: RefusalChain {
            urn: "rho:space:hashmap:queue",
            ctor: r#""default""#,
            client: "Nil",
            steps: OWNER_STEPS,
        },
        trigger: "a!(5)",
        allowed: &[TailBranch::Return],
        needed: TailBranch::Return,
        runs: 1,
        most_anchors: 1,
        check: owner_never_delivers,
        control: Some(owner_fires),
    },
    TailCase {
        name: "RIBA 4.3, the receive of the head",
        chain: RefusalChain {
            urn: "rho:space:hashmap:queue",
            ctor: r#""default""#,
            client: "Nil",
            steps: &[(Role::Owner, r#"a!("o")"#)],
        },
        trigger: r#"for (@v <- a) { out!(["each", v]) }"#,
        allowed: &[TailBranch::Return],
        needed: TailBranch::Return,
        runs: 1,
        most_anchors: 1,
        check: each_parks,
        control: Some(owner_record_fires),
    },
];

/// Added by decision 124 (plan of the batch B tests, B13, RIBA 4.3, rule CR-28): an
/// owner-funded `offer_head` and `receive_head` stage the head search, so the commit can refuse
/// the head of the owner, whose item test fails at budget 0, and take the E3-a outcome: the
/// send stores, and the receive parks. Each run certifies through its own native replay. The
/// half "None" is the count part of T-P-1 (RIBA 2.4).
#[test]
fn head_search_is_none_without_owner_funding() { tail_cases_on_each_runtime(&HEAD_SITE_CASES); }

// ─── RADD2v2 6.4: a refused cascade lift of the join engine stops its operation (CR-19) ────────

/// Added by decision 124 (ruling RADD2v2 6.4): the record of the client in the deploy that
/// creates the priority queue of the divergent chain: `y1`, the head of the level 0 of `b`.
const LIFT_DIVERGENT_HEAD: &str = r#"b!("y1"; priority=0)"#;

/// Added by decision 124 (ruling RADD2v2 6.4): the record of the owner in the divergent chain:
/// `y2` behind `y1` in the level 0 of `b`.
const LIFT_DIVERGENT_RECORD: &str = r#"b!("y2"; priority=0)"#;

/// Added by decision 124 (ruling RADD2v2 6.4): the waiter of the owner in the divergent chain, in
/// the lane of the level 0 of `b`. That lane sees the head of its level only, so the waiter parks
/// while `y1` heads the level.
const LIFT_DIVERGENT_WAITER: &str =
    r#"for (@"y2" <- b ~ (; priority=0)) { out!(["owner", "y2"]) }"#;

/// Added by decision 124 (ruling RADD2v2 6.4): the persistent join of the client in the divergent
/// chain, in the lane `Any` of `a` and of `b`: only a linear bind takes a query, so a persistent
/// bind waits in that lane. It parks while `a` holds no record, and its group puts a join on the
/// channel of `a`, so each operation on `a` runs in the join engine.
const LIFT_DIVERGENT_JOIN: &str = r#"for (@x <= a & @"y1" <= b) { out!(["join", x]) }"#;

/// Added by decision 124 (ruling RADD2v2 6.4): the persistent send of the client in the divergent
/// chain. It fires the persistent join, a divergent pair (A10), so its plan owes the send again.
const LIFT_DIVERGENT_SEND: &str = "a!!(1)";

/// Added by decision 124 (ruling RADD2v2 6.4): the send of the client in a later deploy of the
/// pair chain: `r5` on `a`.
const LIFT_PAIR_LATER: &str = r#"a!("r5")"#;

/// Added by decision 124 (ruling RADD2v2 6.4, rules CR-16 and CR-19 case 2, A10): plays the
/// divergent chain on `chain`. The client creates the priority queue with `y1` on `b`, the owner
/// stores `y2` behind it and parks its waiter of `y2`, and the client parks its persistent join.
/// Then the persistent send of the client fires the join with `y1`, a divergent pair, so the plan
/// owes the send again. The cascade after the plan finds `y2` at the head of the level 0 of `b`
/// and lifts it for the waiter of the owner, and the commit refuses that `K_lift`, because the
/// owner has budget 0. The lift row ends Tail(actual) with the stop of the engine at the lift: a
/// settle job, whose cascade skips the booked pair, and then the send again, which stores its
/// record. Native replay ends its record at the lift row and builds the same end, so the deploy
/// certifies. The join delivers once, the waiter of the owner never, and one `K_lift` parks.
/// The chain needs two waiter lanes on `b`. A divergent pair needs a persistent waiter, which
/// keeps its seat at the head of its lane after the fire, and a cascade offers a record only to
/// the head of each lane (`cascade_order`). So a queue, with one lane for its waiters, cannot
/// reach this case. The family of the priority queue is `urn`, so N2 (ruling RADD2v2 6.6 (f))
/// plays the same chain on a PathMap priority queue.
async fn divergent_lift_case(chain: &mut OwnedChain, label: &str, urn: &str) {
    let creation = stashing(urn, r#""default""#, LIFT_STASH, LIFT_DIVERGENT_HEAD);
    let creation_label = format!("{label}: the space");
    let created = chain
        .deploy(Role::UnfundedClient, &creation, &creation_label)
        .await;
    assert_runs(&creation_label, &created);
    let record = with_stash(LIFT_STASH, LIFT_DIVERGENT_RECORD);
    let record_label = format!("{label}: the record");
    let stored = chain.deploy(Role::Owner, &record, &record_label).await;
    assert_runs(&record_label, &stored);
    let waiter = with_stash(LIFT_STASH, LIFT_DIVERGENT_WAITER);
    let waiter_label = format!("{label}: the waiter");
    let parked = chain.deploy(Role::Owner, &waiter, &waiter_label).await;
    assert_parks(&waiter_label, &parked);
    let join = with_stash(LIFT_STASH, LIFT_DIVERGENT_JOIN);
    let join_label = format!("{label}: the join");
    let joined = chain.deploy(Role::UnfundedClient, &join, &join_label).await;
    assert_parks(&join_label, &joined);
    let send = with_stash(LIFT_STASH, LIFT_DIVERGENT_SEND);
    let send_label = format!("{label}: the send");
    let played = chain.deploy(Role::UnfundedClient, &send, &send_label).await;
    assert_runs(&send_label, &played);
    let (_, journal) = journal_of(&played);
    assert_eq!(
        (
            deliveries(&played, "join"),
            deliveries(&played, "owner"),
            space_parks(journal),
            datum_stores(journal)
        ),
        (1, 0, 1, 1),
        "{send_label}: the join fires once, the waiter of the owner does not, one refused \
         K_lift parks, and the send stores its record"
    );
}

/// Added by decision 124 (ruling RADD2v2 6.4, rules CR-16 and CR-19 case 2): plays the pair chain
/// on `chain`, and then the send of `r5` in a later deploy, whose book is empty again. The settle
/// before the plan of the send lifts `r2` for the join of the owner, and the commit refuses that
/// `K_lift`. The lift row ends Tail(actual) with the stop of the engine at the lift: a settle
/// job, whose cascade skips the booked pair, and then the send again, which stores `r5`. Without
/// that end, the cut at the lift row drops the store of `r5`, which the plan staged after the
/// lift.
async fn later_send_case(chain: &mut OwnedChain, label: &str) {
    refused_lift_case(chain, label, &pair_lift_chain()).await;
    let send = with_stash(LIFT_STASH, LIFT_PAIR_LATER);
    let send_label = format!("{label}: the later send");
    let played = chain.deploy(Role::UnfundedClient, &send, &send_label).await;
    assert_runs(&send_label, &played);
    let (_, journal) = journal_of(&played);
    assert_eq!(
        (
            datum_stores(journal),
            space_parks(journal),
            deliveries(&played, "pair")
        ),
        (1, 1, 0),
        "{send_label}: the send stores r5, one refused K_lift parks, and the join does not fire"
    );
}

/// Added by decision 124 (ruling RADD2v2 6.4, A10): a `K_lift` of the pair cascade of the join
/// engine after a divergent pair, which the commit refuses, ends the operation with the stop of
/// the engine at the lift, so play and native replay re-issue the same settle job.
#[test]
fn join_refused_cascade_lift_after_a_divergent_pair_replays() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("RADD2v2 6.4 (divergent pair), {workers} workers");
            let mut chain = OwnedChain::new().await;
            divergent_lift_case(&mut chain, &label, "rho:space:hashmap:priorityqueue").await;
        });
    }
}

/// Added by decision 124 (ruling RADD2v2 6.4): a `K_lift` of the settle before the plan of the
/// join engine, which the commit refuses, ends the operation with the stop of the engine at the
/// lift, so the plan after it runs again and its store is kept.
#[test]
fn join_refused_lift_before_the_plan_keeps_the_operation() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("RADD2v2 6.4 (settle before the plan), {workers} workers");
            let mut chain = OwnedChain::new().await;
            later_send_case(&mut chain, &label).await;
        });
    }
}

// ─── RADD2v2 6.6: the finish rows of a PathMap cut row (P1, and P2 case R-dev) ────────────────

/// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6 (f), N1): the record
/// of the client in the deploy that creates the bag of N1: 1 on `[a, 1]`.
const FINISH_RECORD: &str = "use space { @[*a, 1]!(1) }";

/// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6 (f), N1 and N3): the
/// waiter of the owner on `[b, 1]`, the most specific prefix of the send of N1 and N3. So the
/// send meets it first in play and in replay (`offer_order`).
const FINISH_OWNER_WAITER: &str = "use space { for (@y <- @[*b, 1]) { Nil } }";

/// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6 (f), N1): the peek
/// join of the client on `[a, 1]` and `b`. Its fire peeks the stored 1 and the sent value, so it
/// removes the marker of 1 on `[a, 1]` and owes the second send of 1.
const FINISH_PEEK_JOIN: &str =
    r#"use space { for (@x <<- @[*a, 1] & @y <<- b) { out!(["client", x]) } }"#;

/// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6 (f), N3): the receive
/// of the client on `b`.
const FINISH_CLIENT_WAITER: &str = r#"for (@y <- b) { out!(["client", y]) }"#;

/// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6 (f), N1 and N3): the
/// trigger: the send of 2 on `[b, 1]`.
const FINISH_SEND: &str = "use space { @[*b, 1]!(2) }";

/// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6 (f), N1): the later
/// take of the client. It binds 1 through its marker on `a` and retires every marker of 1, also
/// the marker on `[a, 1]` (RETIRE-PLAN).
const FINISH_TAKE: &str = r#"for (@x <- a) { out!(["take", x]) }"#;

/// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6 (f), N1 and N3): the
/// bumps of the space counter in `rows`: the COMMs whose row names `S(sid)` alone, as
/// `assert_parks_on_paths` reads them.
fn bumps(rows: &[NativeOperationRecord]) -> usize {
    let mut bumps = 0_usize;
    for row in rows {
        match &row.comm {
            Some(_) if is_space_row(row) && footprint_kinds(row) == ["S"] => bumps += 1,
            Some(_) | None => {}
        }
    }
    bumps
}

/// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6 (f), N2): the number
/// of records that `journal` stores: the produce rows without a COMM whose footprint names a data
/// channel `D(..)`. [`space_stores`] also counts the counter puts of an ordered space and the
/// markers of a PathMap store, because every row of a space transaction names `S(sid)`. So this
/// count reads the same on a keyed ordered space and on a PathMap ordered space.
fn datum_stores(journal: &[NativeOperationRecord]) -> usize {
    let mut stores = 0_usize;
    for row in journal {
        let names_data = footprint_channels(row)
            .iter()
            .any(|channel| matches!(namespace::reified_id_of(channel), Some(ReifiedId::Data(..))));
        match (&row.source, &row.comm) {
            (NativeOperationSource::Produce(_), None) if names_data => stores += 1,
            (NativeOperationSource::Produce(_), _) | (NativeOperationSource::Consume(_), _) => {}
        }
    }
    stores
}

/// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6 (f), N1, P1): plays
/// the chain of N1 on `chain`. The client creates the bag with 1 on `[a, 1]`, the owner parks its
/// waiter on `[b, 1]`, and the client parks its peek join. The send of 2 on `[b, 1]` meets the
/// waiter of the owner first, and the commit refuses that fire (R5). The row search fires the
/// peek join in its place, so the cut owes the second send of 1. The finish rows of the search
/// restore the marker of 1 on `[a, 1]` and bump the token, before the anchor of the re-issue of
/// the stop. Then a take of 1 retires every marker of 1.
async fn refused_fire_finish_case(chain: &mut OwnedChain, label: &str) {
    let creation = stashing(
        "rho:space:pathmap:bag",
        r#""default""#,
        REFUSAL_STASH,
        FINISH_RECORD,
    );
    let steps = [
        (Role::Owner, FINISH_OWNER_WAITER),
        (Role::UnfundedClient, FINISH_PEEK_JOIN),
    ];
    let setup = steps_setup(chain, label, &creation, &steps).await;
    let trigger = format!("{label}: the send of 2");
    let played = refusal_trigger(chain, &setup.root, FINISH_SEND, &trigger).await;
    assert_runs(&trigger, &played);
    let (_, journal) = journal_of(&played);
    let anchors = anchor_indices(journal);
    let [anchor, ..] = anchors.as_slice() else {
        panic!("{trigger}: the re-issue of the stop anchors: {anchors:?}");
    };
    assert_eq!(
        (deliveries(&played, "client"), bumps(&journal[..*anchor])),
        (1, 1),
        "{trigger}: the peek join fires once, and the cut bumps the token once"
    );
    let take = with_stash(REFUSAL_STASH, FINISH_TAKE);
    let take_label = format!("{label}: the take of 1");
    let taken = chain.deploy(Role::UnfundedClient, &take, &take_label).await;
    assert_runs(&take_label, &taken);
    assert_eq!(
        deliveries(&taken, "take"),
        1,
        "{take_label}: the take of 1 finds every marker of 1"
    );
}

/// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6 (f), N3, P2 case
/// R-dev): plays the chain of N3 on `chain`. The client creates the bag, the owner parks its
/// waiter on `[b, 1]`, and the client parks its receive on `b`. The send of 2 on `[b, 1]` meets
/// the waiter of the owner first in both roles, the commit refuses that fire, and the row search
/// fires the receive of the client in its place. The finish rows of the cut bump the token.
/// Native replay stages the same fire of the owner, its search deviates as in play, and the bump
/// that its plan stages after the row follows the bump rows of the record (R-dev).
async fn followed_bump_case(chain: &mut OwnedChain, label: &str) {
    let creation = stashing(
        "rho:space:pathmap:bag",
        r#""default""#,
        REFUSAL_STASH,
        "Nil",
    );
    let steps = [
        (Role::Owner, FINISH_OWNER_WAITER),
        (Role::UnfundedClient, FINISH_CLIENT_WAITER),
    ];
    let setup = steps_setup(chain, label, &creation, &steps).await;
    let trigger = format!("{label}: the send of 2");
    let played = refusal_trigger(chain, &setup.root, FINISH_SEND, &trigger).await;
    assert_runs(&trigger, &played);
    let (_, journal) = journal_of(&played);
    assert_eq!(
        (deliveries(&played, "client"), bumps(journal)),
        (1, 1),
        "{trigger}: the receive of the client fires once, and the token moves once"
    );
}

/// Added by decision 124 (ruling RADD2 1.2, J-STORE): plays a chain on `chain` whose send on a
/// PathMap queue deviates to a store. The owner parks its waiter on `[a, 1]`, and the send of 5
/// on that path meets it. The commit refuses that fire, so the row stores the element, with its
/// record on the path and one marker on each prefix. A later waiter of the client on the prefix
/// `a` takes the element. No waiter waits there before it. A queue serves the waiters of one
/// prefix in order, so a waiter on `[a, 1]` would wait behind the waiter of the owner.
async fn refused_send_store_case(chain: &mut OwnedChain, label: &str) {
    let creation = stashing(
        "rho:space:pathmap:queue",
        r#""default""#,
        REFUSAL_STASH,
        "Nil",
    );
    let steps = [(Role::Owner, PREFIX_OWNER_WAITER)];
    let setup = steps_setup(chain, label, &creation, &steps).await;
    let owner = parked_waiter(label, &setup);
    let trigger = format!("{label}: the send of 5");
    let played = refusal_trigger(chain, &setup.root, PREFIX_SEND, &trigger).await;
    assert_runs(&trigger, &played);
    let (_, journal) = journal_of(&played);
    assert_eq!(
        comms_of(journal, &owner),
        0,
        "{trigger}: the commit refuses the waiter of the owner"
    );
    let client = with_stash(REFUSAL_STASH, CLIENT_WAITER);
    let client_label = format!("{label}: the waiter of the client");
    let taken = chain
        .deploy(Role::UnfundedClient, &client, &client_label)
        .await;
    assert_runs(&client_label, &taken);
    assert_eq!(
        deliveries(&taken, "client"),
        1,
        "{client_label}: the store of the refused send holds its element"
    );
}

/// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6 (f), N1, P1): a
/// refused fire of a PathMap bag while the run owes a second send commits the finish of its cut
/// state. So the store keeps every marker of the peeked record, and the token moves once.
#[test]
fn pathmap_refused_fire_commits_its_finish() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("RADD2v2 6.6 (N1), {workers} workers");
            let mut chain = OwnedChain::new().await;
            refused_fire_finish_case(&mut chain, &label).await;
        });
    }
}

/// Added by decision 124 (ruling RADD2 1.2, J-STORE): a PathMap send that the commit refuses
/// stores its element. Each run certifies through its own native replay, and a later take
/// finds the element.
#[test]
fn pathmap_refused_send_stores_its_element() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("RADD2 1.2 (J-STORE), {workers} workers");
            let mut chain = OwnedChain::new().await;
            refused_send_store_case(&mut chain, &label).await;
        });
    }
}

/// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6 (f), N2): the
/// PathMap twin of T7. A refused cascade lift of a PathMap priority queue after a divergent pair
/// ends with the stop of the engine at the lift, so play and native replay re-issue the same
/// settle job.
#[test]
fn pathmap_refused_cascade_lift_replays() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("RADD2v2 6.6 (N2), {workers} workers");
            let mut chain = OwnedChain::new().await;
            divergent_lift_case(&mut chain, &label, "rho:space:pathmap:priorityqueue").await;
        });
    }
}

/// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6 (f), N3, P2 case
/// R-dev): native replay follows the bump that its plan stages after a deviating row of a
/// PathMap bag to the bump rows of the record, so the unit of the row ends after them.
#[test]
fn pathmap_followed_bump_replays() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("RADD2v2 6.6 (N3), {workers} workers");
            let mut chain = OwnedChain::new().await;
            followed_bump_case(&mut chain, &label).await;
        });
    }
}

/// Added by decision 124 (ruling RADD2v2 6.6 (f), ruling RADD2v4 6.2, N4): the join of the owner
/// on `[a, 1]` and `[b, 1]` in the chain of N4. It shares its join group with
/// [`RECORD_END_PEEK_JOIN`], so play meets the two joins in a random order.
const RECORD_END_OWNER_JOIN: &str =
    r#"use space { for (@x <- @[*a, 1] & @y <- @[*b, 1]) { out!(["owner", y]) } }"#;

/// Added by decision 124 (ruling RADD2v2 6.6 (f), ruling RADD2v4 6.2, N4): the peek join of the
/// client on `[a, 1]` and `[b, 1]` in the chain of N4. Its fire peeks the stored 1 and the sent
/// value, so it removes the marker of 1 on `[a, 1]` and owes the second send of 1.
const RECORD_END_PEEK_JOIN: &str =
    r#"use space { for (@x <<- @[*a, 1] & @y <<- @[*b, 1]) { out!(["client", x]) } }"#;

/// Added by decision 124 (ruling RADD2v2 6.6 (f), ruling RADD2v4 6.2, N4, P2 case R-end): plays
/// the chain of N4 on `chain`. The client creates the bag with 1 on `[a, 1]`, the owner parks its
/// join, and the client parks its peek join, so both wait for a record on `[b, 1]`. Then the
/// send of 2 on `[b, 1]` runs [`PICK_RUNS`] times from the state before it. Play meets the two
/// joins in a random order. When it meets the join of the owner first, the commit refuses that
/// fire (R5), and the row search fires the peek join in its place. The cut owes the second send
/// of 1, so the row ends Stop. The finish rows of the search restore the marker of 1 on `[a, 1]`
/// and bump the token, before the anchor of the re-issue. Native replay follows the record: it
/// stages the peek join, the actual fire of play, as its c0, so its row does not deviate. Its
/// engine lands the second send of 1, whose restore follows the finish row of the record, and
/// the next stage call, the offer of 2 to the join of the owner, misses, so the record ends. The
/// row with the search is then the cut row, because its later row follows its finish rows, and
/// replay re-issues the rest of the send as play does (REPLAY-END). When play meets the peek
/// join first, the commit refuses the later fire of the join of the owner, which has no
/// alternative, so the run ends without an anchor. In every run, the peek join fires once and
/// the join of the owner never delivers. Returns the number of runs that stop, as their anchors
/// show: those are the runs that met the refused join first.
async fn record_end_case(chain: &mut OwnedChain, label: &str) -> usize {
    let creation = stashing(
        "rho:space:pathmap:bag",
        r#""default""#,
        REFUSAL_STASH,
        FINISH_RECORD,
    );
    let steps = [
        (Role::Owner, RECORD_END_OWNER_JOIN),
        (Role::UnfundedClient, RECORD_END_PEEK_JOIN),
    ];
    let setup = steps_setup(chain, label, &creation, &steps).await;
    let mut stops = 0_usize;
    for run in 0..PICK_RUNS {
        let trigger = format!("{label}: the send of 2, run {run}");
        let played = refusal_trigger(chain, &setup.root, FINISH_SEND, &trigger).await;
        assert_runs(&trigger, &played);
        assert_eq!(
            (deliveries(&played, "client"), deliveries(&played, "owner")),
            (1, 0),
            "{trigger}: the peek join fires once, and the owner refuses its join"
        );
        // A Stop owes the cascade of the join engine, so a settle job anchors before the re-issue.
        match tail_branch_up_to(&trigger, &played, 2) {
            TailBranch::Return => {}
            TailBranch::Stop => stops += 1,
        }
    }
    stops
}

/// Added by decision 124 (ruling RADD2v2 6.6 (f), ruling RADD2v4 6.2, N4, P2 case R-end): after a
/// record end, the last row with a search is the cut row, because its later rows follow its
/// finish rows (a followed restore). Each run certifies through its own native replay, which must
/// re-issue the rest of the send as play does. A run meets the join of the owner first with a
/// chance of 1/2, so all runs of a runtime miss it with a chance of `(1/2)^64`. Some run on each
/// runtime stops, which is the non-vacuity of the case.
#[test]
fn pathmap_record_end_after_a_followed_restore_replays() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("RADD2v2 6.6 (N4), {workers} workers");
            let mut chain = OwnedChain::new().await;
            let stops = record_end_case(&mut chain, &label).await;
            assert!(
                stops > 0,
                "{label}: some run meets the join of the owner first and stops"
            );
        });
    }
}

// ─── Batch B, step 6: the chain tests of a deviating row (phases C4 and C2) ────────────────────

/// Added by decision 124 (batch B, step 6, T-C4-31 and T-C4-34): a chain whose client plays and
/// replays with the owner binding `binding`, so a test can set the budget of an owner to the
/// exact amount that it needs. A deploy of [`Role::Owner`] plays without a binding, as on
/// [`OwnedChain`]. The two client roles take the binding of this chain, so the helpers of batch
/// B run on it unchanged.
struct BoundChain {
    /// The chain that holds the runtime and the space.
    chain: OwnedChain,
    /// The owner binding of each client deploy.
    binding: Arc<OwnerFundingBinding>,
}

impl BoundChain {
    /// Added by decision 124 (batch B, step 6): a chain over the empty state with `binding`.
    async fn new(binding: Arc<OwnerFundingBinding>) -> Self {
        Self {
            chain: OwnedChain::new().await,
            binding,
        }
    }

    /// Added by decision 124 (batch B, step 6): plays `term` as a deploy of the client with the
    /// random state of `label` at [`AMPLE_LIMIT`], as [`OwnedChain::play`] plays it, with the
    /// binding of this chain.
    async fn play_client(&mut self, term: &str, label: &str) -> Played {
        let chain = &mut self.chain;
        chain
            .runtime
            .cost
            .set_deploy_signature_funded(DEPLOY, client_signer());
        let random = Blake2b512Random::create_from_bytes(label.as_bytes());
        let history = chain.space.get_history_repository();
        let pre_root = chain.runtime.get_root().await;
        let result = chain
            .runtime
            .evaluate_with_native_phlo(
                term,
                Injections::new(),
                random.clone(),
                None,
                funded_config(Some(Arc::clone(&self.binding)), AMPLE_LIMIT),
            )
            .await
            .expect("native play evaluates");
        let soft = chain.runtime.create_soft_checkpoint().await;
        let log = soft.log.into_iter().collect();
        let out = out_data(&chain.runtime).await;
        let mut outputs = Vec::<String>::with_capacity(out.len());
        outputs.extend(out.iter().map(|pars| format!("{pars:?}")));
        outputs.sort_unstable();
        let residue = hot_store_residue(&soft.cache_snapshot, Durability::Ephemeral);
        let durable = hot_store_residue(&soft.cache_snapshot, Durability::Durable);
        let keys = snapshot_keys(&soft.cache_snapshot);
        let root = chain.runtime.create_checkpoint().await.root;
        Played {
            result,
            log,
            history,
            root,
            outputs,
            out,
            random,
            residue,
            durable,
            injections: Injections::new(),
            pre_root,
            snapshot_keys: keys,
        }
    }
}

/// Added by decision 124 (batch B, step 6): certifies `played`, a deploy of the client at
/// [`AMPLE_LIMIT`], through its committed evidence and its own native replay with `binding`, as
/// [`certify_owned`] does for the bindings of [`Role`].
async fn certify_bound(
    played: &Played,
    term: &str,
    binding: &Arc<OwnerFundingBinding>,
) -> Certification {
    let (recording, journal) = committed_evidence(played);
    let trace = with_contract(AMPLE_LIMIT, |contract| {
        contract.check_operation_journal_with_owners(
            recording.session,
            &recording,
            journal,
            journal_limits(),
            Some(&**binding),
        )
    })
    .expect("the journal check accepts the journal of play")
    .bind_trace(played.log.clone().into(), trace_bind_limits())
    .expect("the trace binding accepts the log of play");
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(DEPLOY, client_signer());
    budget
        .reset_for_native_execution(funded_config(Some(Arc::clone(binding)), AMPLE_LIMIT))
        .expect("the replay budget enters native execution");
    let environment = create_native_replay_env_with_logger(
        trace,
        played.history.clone(),
        Arc::new(Box::new(Matcher)),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut Vec::new(),
        budget,
        ExternalServices::noop(),
        None,
        FileHandleTable::new(),
    )
    .await
    .expect("the native replay environment builds");
    finish_injected(
        environment,
        term,
        played.random.clone(),
        played.injections.clone(),
    )
    .await
}

/// Added by decision 124 (batch B, step 6): the client deploys of this chain play with its
/// binding, and each deploy certifies through its own native replay with that binding.
impl CertifiedChain for BoundChain {
    async fn deploy(&mut self, role: Role, term: &str, label: &str) -> Played {
        match role {
            Role::Owner => self.chain.deploy(role, term, label).await,
            Role::Client | Role::UnfundedClient => {
                let played = self.play_client(term, label).await;
                let certification = certify_bound(&played, term, &self.binding).await;
                assert_reproduces(label, &played, &certification);
                played
            }
        }
    }

    async fn rewind(&mut self, root: &Blake2b256Hash) { self.chain.rewind(root).await }
}

/// Added by decision 124 (batch B, step 6): the amount that the owner `owner` pays for each
/// budget attempt of `played`, by attempt index: the usage of each region of the owner, as the
/// meter of the run measures it, times the price of the region (rule R4).
fn owner_amounts(played: &Played, owner: &CostSignature) -> Vec<u128> {
    let (recording, _) = journal_of(played);
    let regions = support::native::trace_limits().regions;
    with_contract(AMPLE_LIMIT, |contract| {
        let mut amounts = Vec::<u128>::with_capacity(recording.attempts.len());
        for attempt in recording.attempts.iter() {
            let charge = contract
                .prepare(Arc::clone(&attempt.observation), regions)
                .expect("the charge of a recorded attempt prepares");
            let mut amount = 0_u128;
            for (region, usage) in attempt
                .observation
                .authority
                .regions
                .iter()
                .zip(charge.region_usages())
            {
                match region.signature.as_ref() {
                    Some(signature) if signature == owner => {
                        amount += u128::from(*usage) * u128::from(region.price);
                    }
                    Some(_) | None => {}
                }
            }
            amounts.push(amount);
        }
        amounts
    })
}

/// Added by decision 124 (batch B, step 6): the amount that the owner `owner` pays for the
/// first COMM of `played` with a region of that owner.
fn first_owner_comm_amount(label: &str, played: &Played, owner: &CostSignature) -> u64 {
    use rholang::rust::interpreter::accounting::authority::AuthorityByteEventKind;

    let (recording, _) = journal_of(played);
    let amounts = owner_amounts(played, owner);
    for (attempt, amount) in recording.attempts.iter().zip(amounts) {
        match (&attempt.observation.kind, amount) {
            (_, 0) => {}
            (AuthorityByteEventKind::Comm, amount) => {
                return u64::try_from(amount).expect("the amount of one COMM fits in a budget")
            }
            (_, _) => {}
        }
    }
    panic!("{label}: no COMM of the probe holds a region of the owner");
}

/// Added by decision 124 (batch B, step 6, T-C4-31 and T-C4-34): the budget that refuses the
/// tuple of a two-bind receive of the chain `setup` at the tuple test (rule R7) and passes each
/// of its item tests (rule R5). The item test of a datum measures the transfer of that datum
/// alone, and the tuple test measures the transfer of every datum, so the item tests are lower
/// bounds of the tuple test (rule R5, errata V4-11). The budget is one less than what the owner
/// pays for that tuple. A probe plays the chain with the funded client: its receive fires the
/// tuple, and each deploy of the chain certifies. The bytes of the data follow from the
/// deploys and their labels, so the tuple of the probe measures what the tuple of the test
/// measures.
async fn refusing_budget(setup: &RefusalChain, trigger: &str, label: &str) -> u64 {
    let mut chain = OwnedChain::new().await;
    refusal_setup(&mut chain, label, setup).await;
    let term = with_stash(REFUSAL_STASH, trigger);
    let probe = format!("{label}: the probe of the budget");
    let played = chain.deploy(Role::Client, &term, &probe).await;
    assert_runs(&probe, &played);
    let owner = sig_to_cost_signature(&owner_signer()).expect("a ground signature");
    first_owner_comm_amount(&probe, &played, &owner) - 1
}

/// Added by decision 124 (batch B, step 6): the number of space rows of `journal` that list a
/// refused COMM (rule R7).
fn refusal_rows(journal: &[NativeOperationRecord]) -> usize {
    let mut rows = 0_usize;
    for row in journal {
        match &*row.refusals {
            [] => {}
            [_, ..] if is_space_row(row) => rows += 1,
            [_, ..] => {}
        }
    }
    rows
}

// ─── T-C4-17: a deviation store above the bound is read back (CR-22) ───────────────────────────

/// Added by decision 124 (batch B, step 6, T-C4-17, rule CR-22): the VectorDB chain with room for
/// one record. The client creates the space with `r` on `a`, so the collection is full, and the
/// owner parks a take of `s` on `a`.
const STORE_BOUND_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:vectordb",
    ctor: r#""default"; capacity=1, dimensions=2"#,
    client: r#"a!("r")"#,
    steps: &[(Role::Owner, r#"for (@"s" <- a) { out!(["owner", "s"]) }"#)],
};

/// Added by decision 124 (batch B, step 6, T-C4-17): the two takes that read the records of `a`
/// back after the store above the bound.
const STORE_BOUND_READ_BACK: &str =
    r#"for (@x <- a) { out!(["each", x]) } | for (@y <- a) { out!(["each", y]) }"#;

/// Added by decision 124 (batch B, step 6, T-C4-17, rule CR-22): a store of a deviating row may
/// go above the bound of its collection, and the record reads back. The send of `s` stages a
/// fire of the waiter of the owner, which needs no room. The owner has budget 0, so the commit
/// refuses the fire and stores `s` in the full collection. A send of `t`, which meets no
/// waiter, fails with `CapacityExceeded` from the same state, so the bound is real. Then two
/// takes read `r` and `s` back. Without owner funding, the waiter fires. Each deploy certifies
/// through its own native replay on each runtime of `RACING_WORKERS`. The space picks nothing
/// at random here, so one run shows each path.
#[test]
fn deviation_store_above_the_bound_is_read_back() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("T-C4-17, {workers} workers");
            let mut chain = OwnedChain::new().await;
            let setup = refusal_setup(&mut chain, &label, &STORE_BOUND_CHAIN).await;
            let trigger = format!("{label}: the send of s");
            let played = refusal_trigger(&mut chain, &setup.root, r#"a!("s")"#, &trigger).await;
            assert_runs(&trigger, &played);
            let (_, journal) = journal_of(&played);
            assert_eq!(
                (deliveries(&played, "owner"), space_stores(journal)),
                (0, 1),
                "{trigger}: the commit refuses the waiter and stores s above the bound"
            );
            let above = format!("{label}: a send above the bound");
            let refused = refusal_trigger(&mut chain, &setup.root, r#"a!("t")"#, &above).await;
            let errors = format!("{:?}", refused.result.errors);
            assert!(
                errors.contains("CapacityExceeded"),
                "{above}: an ordinary store meets the bound: {errors}"
            );
            let read = format!("{label}: the read back");
            let back =
                refusal_trigger(&mut chain, &played.root, STORE_BOUND_READ_BACK, &read).await;
            assert_runs(&read, &back);
            assert_eq!(
                delivered_strings(&back, "each"),
                ["r", "s"],
                "{read}: the takes read both records back"
            );
            let mut control = NativeChain::new().await;
            let unfunded = refusal_setup(&mut control, &label, &STORE_BOUND_CHAIN).await;
            let native =
                refusal_trigger(&mut control, &unfunded.root, r#"a!("s")"#, &trigger).await;
            assert_runs(&trigger, &native);
            assert_eq!(
                deliveries(&native, "owner"),
                1,
                "{trigger}: without owner funding, the waiter fires"
            );
        });
    }
}

/// Added by decision 124 (batch B, step 6, T-C4-17, rules CR-7 and CR-22): the VectorDB chain of
/// a guarded waiter at the bound of the data side. The client creates the space with `r` on
/// `a`, so the collection is full. The owner parks a guarded take of `s`, which refuses `r`, and
/// the client parks a join that never fires and puts `a` in the join engine.
const GUARDED_STORE_BOUND_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:vectordb",
    ctor: r#""default"; capacity=1, dimensions=2"#,
    client: r#"a!("r")"#,
    steps: &[
        (
            Role::Owner,
            r#"for (@v <- a where v == "s") { out!(["owner", v]) }"#,
        ),
        (
            Role::UnfundedClient,
            r#"for (@"none" <- a & @y <- b) { Nil }"#,
        ),
    ],
};

/// Added by decision 124 (batch B, step 6, T-C4-17, rules CR-7 and CR-22): the cell chain of a
/// guarded receive at the bound of the waiter side. The owner stores `d` in the slot of `a`, and
/// the client parks a join that never fires, so the waiter side of `a` is full and `a` runs in
/// the join engine.
const GUARDED_PARK_BOUND_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:cell",
    ctor: r#""default""#,
    client: "Nil",
    steps: &[
        (Role::Owner, r#"a!("d")"#),
        (
            Role::UnfundedClient,
            r#"for (@"none" <- a & @y <- b) { Nil }"#,
        ),
    ],
};

/// Added by decision 124 (batch B, step 6, T-C4-17): the guarded receive of the client, which
/// binds `d` of the owner.
const GUARDED_PARK_RECEIVE: &str = r#"for (@v <- a where v == "d") { out!(["each", v]) }"#;

/// Added by decision 124 (batch B, step 6, T-C4-17, rules CR-7 and CR-22): a deviation that
/// takes a side of the join engine above its bound replays past a guarded waiter. Play stages
/// the fire, which needs no room, and the commit refuses it, because the owner has budget 0.
/// The row is Stored above the bound. Replay skips the waiter on the recorded Stored row (E3-a),
/// so the side takes the store or the park above its bound as the host outcome. Without owner
/// funding, the fire commits. Each deploy certifies through its own native replay on each
/// runtime of `RACING_WORKERS`.
#[test]
fn deviation_above_the_bound_replays_past_a_guarded_waiter() {
    let cases = [
        ("a store", &GUARDED_STORE_BOUND_CHAIN, r#"a!("s")"#, "owner"),
        (
            "a park",
            &GUARDED_PARK_BOUND_CHAIN,
            GUARDED_PARK_RECEIVE,
            "each",
        ),
    ];
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            for (side, setup_chain, term, kind) in cases {
                let label = format!("T-C4-17 {side} past a guard, {workers} workers");
                let mut chain = OwnedChain::new().await;
                let setup = refusal_setup(&mut chain, &label, setup_chain).await;
                let trigger = format!("{label}: the trigger");
                let played = refusal_trigger(&mut chain, &setup.root, term, &trigger).await;
                assert_runs(&trigger, &played);
                assert_eq!(
                    deliveries(&played, kind),
                    0,
                    "{trigger}: the commit refuses the fire, and the side goes above its bound"
                );
                let mut control = NativeChain::new().await;
                let unfunded = refusal_setup(&mut control, &label, setup_chain).await;
                let native = refusal_trigger(&mut control, &unfunded.root, term, &trigger).await;
                assert_runs(&trigger, &native);
                assert_eq!(
                    deliveries(&native, kind),
                    1,
                    "{trigger}: without owner funding, the fire commits"
                );
            }
        });
    }
}

// ─── T-C4-20: a ranked VectorDB deviation takes a worse score (N1, V3-13) ──────────────────────

/// Added by decision 124 (batch B, step 6, T-C4-20): the ranked VectorDB chain. The client
/// creates the space with `second` and `third`, and the owner stores `first`, the best score of
/// the query `[1.0, 0.0]`, so the search picks the record of the owner first.
const RANKED_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:vectordb",
    ctor: VECTORDB_CTOR,
    client: r#"a!("second"; embedding=[10.0, 1.0]) | a!("third"; embedding=[10.0, 2.0])"#,
    steps: &[(Role::Owner, r#"a!("first"; embedding=[10.0, 0.0])"#)],
};

/// Added by decision 124 (batch B, step 6, T-C4-20): the receive whose guard passes `second`
/// (variant 1 of V3-13).
const RANKED_PASSING_RECEIVE: &str =
    r#"for (@v <- a ~ ([1.0, 0.0]) where v != "third") { out!(["each", v]) }"#;

/// Added by decision 124 (batch B, step 6, T-C4-20): the receive whose guard refuses `second`
/// (variant 2 of V3-13).
const RANKED_REFUSING_RECEIVE: &str =
    r#"for (@v <- a ~ ([1.0, 0.0]) where v == "first") { out!(["each", v]) }"#;

/// Added by decision 124 (batch B, step 6, T-C4-20): plays the chain [`RANKED_CHAIN`] and the
/// receive `receive` of the variant `variant` on an owned chain and on a native chain. Returns
/// the play with owner funding and the play without it. Each deploy runs to its end and
/// certifies through its own native replay.
async fn ranked_runs(workers: usize, variant: &str, receive: &str) -> (Played, Played) {
    let label = format!("T-C4-20 ({variant}), {workers} workers");
    let mut chain = OwnedChain::new().await;
    let setup = refusal_setup(&mut chain, &label, &RANKED_CHAIN).await;
    let trigger = format!("{label}: the receive");
    let played = refusal_trigger(&mut chain, &setup.root, receive, &trigger).await;
    assert_runs(&trigger, &played);
    let mut control = NativeChain::new().await;
    let unfunded = refusal_setup(&mut control, &label, &RANKED_CHAIN).await;
    let native = refusal_trigger(&mut control, &unfunded.root, receive, &trigger).await;
    assert_runs(&trigger, &native);
    (played, native)
}

/// Added by decision 124 (batch B, step 6, T-C4-20, N1, V3-13): a ranked VectorDB receive whose
/// best record the owner refuses deviates to a worse score, and the row replays. The owner has
/// budget 0, so its record `first` fails its item test at the commit. In the first variant the
/// guard passes the next record, so `second` fires and replays. In the second variant the guard
/// refuses the next record, so the receive parks, and its Stored row replays by E3-a. Without
/// owner funding, `first` fires in both variants, so the best score is the record of the owner.
/// The scores are distinct, so one run on each runtime shows each path.
#[test]
fn ranked_vectordb_deviation_to_a_worse_score_replays() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let (played, native) = ranked_runs(workers, "fire", RANKED_PASSING_RECEIVE).await;
            assert_eq!(
                delivered_strings(&played, "each"),
                ["second"],
                "the commit refuses first and fires the next record"
            );
            assert_eq!(delivered_strings(&native, "each"), ["first"]);
            let (played, native) = ranked_runs(workers, "park", RANKED_REFUSING_RECEIVE).await;
            assert_parks(
                "the guard refuses the next record, so the receive parks",
                &played,
            );
            assert_eq!(delivered_strings(&native, "each"), ["first"]);
        });
    }
}

// ─── T-C4-25: the retire group of the actual class fire commits (U1 K, V3-12, V4-5) ────────────

/// Added by decision 124 (batch B, step 6, T-C4-25): the records of the client in the set of the
/// chain, two members of the class `c`.
const RETIRE_CLIENT_RECORDS: usize = 2;

/// Added by decision 124 (batch B, step 6, T-C4-25): the receive of the class fire of T-C4-25.
const SET_CLASS_RECEIVE: &str = r#"for (@x <- a) { out!(["each", x]) }"#;

/// Added by decision 124 (batch B, step 6, T-C4-25): the value of the member of the class `o`.
/// It is longer than the list that a retire binds, so its item test fails at the budget of the
/// chain, which passes the item test of the member of the owner in the class `c`.
const RETIRE_OWNER_CLASS: &str = "oooooooooooooooooooooooooooooooo";

/// Added by decision 124 (batch B, step 6, T-C4-25): the receive of the probe of the budget. It
/// binds the class `c` only, so the first COMM with a region of the owner is the retire.
const RETIRE_PROBE_RECEIVE: &str = r#"for (@"c" <- a) { out!(["each", "c"]) }"#;

/// Added by decision 124 (batch B, step 6, T-C4-25): the state before the receive of T-C4-25.
struct RetireGroup {
    /// The root of the state before the receive.
    root: Blake2b256Hash,
    /// The data channel of the set.
    channel: [u8; 32],
    /// The source hash of the member of the owner.
    owner_member: [u8; 32],
}

/// Added by decision 124 (batch B, step 6, T-C4-25): the chain of T-C4-25 on a set. The client
/// creates the space with two members of the class `c`. The owner adds a third member of `c`,
/// with the first salt whose record sits between the members of the client in the order of
/// DR-75, so the receive that binds the first member of the class retires the member of the
/// owner first. Then the owner stores a member of the class `o`. A salt fits with a chance of
/// 1/3, so no salt fits with a chance of `(2/3)^64`. The hashes of a deploy follow from its label
/// and its term alone, so every run finds the same salt.
async fn retire_group_setup<C: CertifiedChain>(chain: &mut C, label: &str) -> RetireGroup {
    let creation = stashing(
        "rho:space:hashmap:set",
        r#""default""#,
        REFUSAL_STASH,
        r#"a!("c") | a!("c")"#,
    );
    let creation_label = format!("{label}: the space");
    let created = chain
        .deploy(Role::UnfundedClient, &creation, &creation_label)
        .await;
    assert_runs(&creation_label, &created);
    let channel = stored_channel(&created, RETIRE_CLIENT_RECORDS);
    let held = stored_values(&created, &channel, set_value);
    let [(first, _), (second, _)] = held.as_slice() else {
        panic!(
            "{creation_label}: the client holds two members: {}",
            held.len()
        );
    };
    let member = with_stash(REFUSAL_STASH, r#"a!("c")"#);
    for salt in 0..SALTS {
        chain.rewind(&created.root).await;
        let salted = format!("{label}: the member of the owner, salt {salt}");
        let stored = chain.deploy(Role::Owner, &member, &salted).await;
        assert_runs(&salted, &stored);
        let members = stored_values(&stored, &channel, set_value);
        let Some((middle, _)) = members.get(1) else {
            panic!("{salted}: the set holds three members: {}", members.len());
        };
        match middle != first && middle != second {
            false => {}
            true => {
                let class_o = with_stash(REFUSAL_STASH, &format!(r#"a!("{RETIRE_OWNER_CLASS}")"#));
                let class_label = format!("{label}: the class o");
                let last = chain.deploy(Role::Owner, &class_o, &class_label).await;
                assert_runs(&class_label, &last);
                return RetireGroup {
                    root: last.root.clone(),
                    channel,
                    owner_member: *middle,
                };
            }
        }
    }
    panic!("{label}: no salt below {SALTS} puts the member of the owner in the middle");
}

/// Added by decision 124 (batch B, step 6, T-C4-25, C4v2 B.4, rule CR-33): the budget of the owner
/// that passes the item test of its member of the class `c` and refuses the retire of that
/// member. A retire binds the whole payload as a list, so it costs more than the bind of the
/// item test. The budget is one less than what the owner pays for the retire, which a probe
/// with the funded client measures.
async fn retire_refusing_budget(label: &str) -> u64 {
    let mut chain = OwnedChain::new().await;
    retire_group_setup(&mut chain, label).await;
    let term = with_stash(REFUSAL_STASH, RETIRE_PROBE_RECEIVE);
    let probe = format!("{label}: the probe of the budget");
    let played = chain.deploy(Role::Client, &term, &probe).await;
    assert_runs(&probe, &played);
    let owner = sig_to_cost_signature(&owner_signer()).expect("a ground signature");
    first_owner_comm_amount(&probe, &played, &owner) - 1
}

/// Added by decision 124 (batch B, step 6, T-C4-25, U1 K, V3-12, errata V4-5): the retire group
/// of the actual class fire commits. The budget of the owner passes the item test of its member
/// of `c` (C4v2 B.4) and refuses its retire, and the item test of the longer value of the class
/// `o` fails, so the receive never binds `o`: when its pick meets `o` first, the commit refuses
/// it, and the search
/// fires the class `c`. The retire of the first other member, the member of the owner, is
/// refused and parks inert, the retire of the last member commits, and replay accepts the unit
/// by CanStop(U), so each run certifies through its own native replay. The class has one member
/// of each kind in the order of DR-75, so the receive delivers `c` once in every run. Play picks
/// the class at random, so the receive runs [`PICK_RUNS`] times on each runtime. Without owner
/// funding, some run fires the class `o`, so the receive meets the record of the owner, with a
/// chance of `(1/2)^64` of no such run.
#[test]
fn retire_group_of_the_actual_class_fire_commits() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("T-C4-25, {workers} workers");
            let budget = retire_refusing_budget(&label).await;
            let mut chain = BoundChain::new(owner_binding_with(budget)).await;
            let group = retire_group_setup(&mut chain, &label).await;
            for run in 0..PICK_RUNS {
                let trigger = format!("{label}: the receive, run {run}");
                let played =
                    refusal_trigger(&mut chain, &group.root, SET_CLASS_RECEIVE, &trigger).await;
                assert_runs(&trigger, &played);
                assert_eq!(
                    delivered_strings(&played, "each"),
                    ["c"],
                    "{trigger}: the receive binds the class of the client once"
                );
                let (_, journal) = journal_of(&played);
                assert_eq!(
                    space_parks(journal),
                    1,
                    "{trigger}: the refused retire parks inert"
                );
                let members = stored_values(&played, &group.channel, set_value);
                assert_eq!(members.len(), 2, "{trigger}: the members that stay");
                assert!(
                    members.iter().any(
                        |(hash, value)| *hash == group.owner_member && string_of(value) == "c"
                    ),
                    "{trigger}: the member of the owner stays stored"
                );
            }
            let mut control = NativeChain::new().await;
            let unfunded = retire_group_setup(&mut control, &label).await;
            let mut owner_fires = 0_usize;
            for run in 0..PICK_RUNS {
                let trigger = format!("{label}: the receive, run {run}");
                let played =
                    refusal_trigger(&mut control, &unfunded.root, SET_CLASS_RECEIVE, &trigger)
                        .await;
                assert_runs(&trigger, &played);
                owner_fires +=
                    usize::from(delivered_strings(&played, "each") == [RETIRE_OWNER_CLASS]);
            }
            assert!(
                owner_fires > 0,
                "{label}: without owner funding, some run fires the class of the owner"
            );
        });
    }
}

// ─── T-C4-26: an item-failed settle pair is booked and not refired (U2 (a)) ────────────────────

/// Added by decision 124 (batch B, step 6, T-C4-26): the waiter of 9 of the client on the stack.
const SETTLE_BOOK_WAITER_NINE: &str = r#"for (@9 <- a) { out!(["each", 9]) }"#;

/// Added by decision 124 (batch B, step 6, T-C4-26): the waiter of 8 of the client on the stack.
/// The take of 8 sends 9, so the second send of the deploy runs after the fire of the first, in
/// the same evaluation.
const SETTLE_BOOK_WAITER_EIGHT: &str = r#"for (@8 <- a) { a!(9) | out!(["each", 8]) }"#;

/// Added by decision 124 (batch B, step 6, T-C4-26): the stack chain of T-C4-26. The owner parks
/// a waiter of 1 on `a`, and the client parks the waiter of 9 and then the waiter of 8, so the
/// waiter of the owner reaches the top last. The client sends 1, which the top waiter does not
/// bind, so 1 stores.
const SETTLE_BOOK_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:stack",
    ctor: r#""default""#,
    client: "Nil",
    steps: &[
        (Role::Owner, r#"for (@1 <- a) { out!(["owner", 1]) }"#),
        (Role::UnfundedClient, SETTLE_BOOK_WAITER_NINE),
        (Role::UnfundedClient, SETTLE_BOOK_WAITER_EIGHT),
        (Role::UnfundedClient, "a!(1)"),
    ],
};

/// Added by decision 124 (batch B, step 6, T-C4-26, U2 (a)): an item-failed settle pair is booked
/// and not refired. The send of 8 fires the top waiter, and its body sends 9, which fires the
/// waiter of 9. The waiter of the owner is then on top, and the settle after the plan of the
/// send of 9 lifts the stored 1 and offers it to that waiter. The commit refuses the fire (R5),
/// the head-order row stores 1, and the tail read reads the book before the finish writes it
/// (V4-13), so Tail(actual) is Stop. The book of the evaluation then holds the pair (U2 (a)), so
/// the anchored settle job skips it, and the journal holds one anchor. Without the book, the
/// settle job would refuse the pair again and stage a second anchor, which `tail_branch` rejects.
/// Without owner funding, that settle fires the waiter of the owner, so the pair is real. A
/// stack picks nothing at random, so one run on each runtime suffices. Each deploy certifies
/// through its own native replay.
#[test]
fn item_failed_settle_pair_is_booked_and_not_refired() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("T-C4-26, {workers} workers");
            let mut chain = OwnedChain::new().await;
            let setup = refusal_setup(&mut chain, &label, &SETTLE_BOOK_CHAIN).await;
            let trigger = format!("{label}: the send of 8");
            let played = refusal_trigger(&mut chain, &setup.root, "a!(8)", &trigger).await;
            assert_runs(&trigger, &played);
            assert_eq!(
                (
                    deliveries(&played, "each"),
                    deliveries(&played, "owner"),
                    tail_branch(&trigger, &played)
                ),
                (2, 0, TailBranch::Stop),
                "{trigger}: both takes fire, the owner refuses its waiter once, and the book \
                 holds the pair for the later settle"
            );
            let mut control = NativeChain::new().await;
            let unfunded = refusal_setup(&mut control, &label, &SETTLE_BOOK_CHAIN).await;
            let native = refusal_trigger(&mut control, &unfunded.root, "a!(8)", &trigger).await;
            assert_runs(&trigger, &native);
            assert_eq!(
                (deliveries(&native, "each"), deliveries(&native, "owner")),
                (2, 1),
                "{trigger}: without owner funding, the waiter of the owner takes the 1"
            );
        });
    }
}

// ─── T-C4-29: a creation with a skipped first row replays (F5, C7.3) ───────────────────────────

/// Added by decision 124 (batch B, step 6, T-C4-29): the surface signature of the creation of
/// T-C4-29. The surface syntax names a signature by its spelling, so the owner of the creation
/// is the owner of this name.
const CREATION_SIGNER: &str = "creationowner";

/// Added by decision 124 (batch B, step 6, T-C4-29): the signature of the surface name
/// [`CREATION_SIGNER`].
fn creation_signature() -> Sig {
    use rholang::rust::interpreter::compiler::normalizer::cost_accounting::sig::canon_ground;

    Sig::Ground(canon_ground(CREATION_SIGNER))
}

/// Added by decision 124 (batch B, step 6, T-C4-29): the owner binding of a deploy of the client,
/// whose table lists the owner of [`creation_signature`] with `budget`.
fn creation_binding(budget: u64) -> Arc<OwnerFundingBinding> {
    let owner = creation_signature();
    let entry = OwnerTableEntry {
        signature: sig_to_cost_signature(&owner).expect("a ground signature"),
        budget,
    };
    let table = OwnerTable::new(vec![(owner.lane_hash(), entry)], 1).expect("one owner");
    Arc::new(OwnerFundingBinding::new(
        [client_signer().lane_hash()].into_iter().collect(),
        single_purse,
        table,
    ))
}

/// Added by decision 124 (batch B, step 6, T-C4-29): a deploy of the client whose creation is
/// signed by the owner of [`CREATION_SIGNER`], so row 0 of the creation keeps the authority of the
/// creation (rule CR-15, C7.3).
fn signed_creation() -> String {
    format!(
        r#"new factory(`rho:space:hashmap:queue`), ret, out in {{
  {{% factory!(*ret, "default") %}}[ {CREATION_SIGNER} ] |
  for (space <- ret) {{ out!(["created", 1]) }} |
  for (@value <= out) {{ @"{OUT}"!(value) }}
}}"#
    )
}

/// Added by decision 124 (batch B, step 6, T-C4-29): the index of the budget attempt of the
/// introduction of `row`.
fn introduction_attempt_of(label: &str, row: &NativeOperationRecord) -> usize {
    use rholang::rust::interpreter::accounting::NativeObservationLink;

    match row.introduction {
        NativeObservationLink::Attempt(index) => index,
        NativeObservationLink::Retry(_) => panic!("{label}: the row introduces itself once"),
    }
}

/// Added by decision 124 (batch B, step 6, T-C4-29): the first space row of `played`, the config
/// row of the creation.
fn first_space_row<'a>(label: &str, played: &'a Played) -> &'a NativeOperationRecord {
    let (_, journal) = journal_of(played);
    match journal.iter().find(|row| is_space_row(row)) {
        Some(row) => row,
        None => panic!("{label}: the deploy holds no space row"),
    }
}

/// Added by decision 124 (batch B, step 6, T-C4-29): the budget that lets the owner of the
/// creation pay for every attempt before row 0 of the creation and for that row minus one, from
/// the probe `played` with an ample budget. So play refuses row 0 and no attempt before it.
fn skipping_budget(label: &str, played: &Played, owner: &CostSignature) -> u64 {
    let index = introduction_attempt_of(label, first_space_row(label, played));
    let amounts = owner_amounts(played, owner);
    let Some(row) = amounts.get(index) else {
        panic!("{label}: the budget trace holds the attempt of row 0");
    };
    assert!(
        *row > 0,
        "{label}: row 0 of the creation charges the owner of the creation"
    );
    let before: u128 = amounts[..index].iter().sum();
    u64::try_from(before + row - 1).expect("the amounts of one deploy fit in a budget")
}

/// Added by decision 124 (batch B, step 6, T-C4-29, F5, errata V4-19): a creation whose first
/// row the owner refuses (rule R8) commits that row alone and replays. The creation runs under a
/// signature that the binding lists, so its row 0 (the config row) keeps the authority of the
/// creation, while the later rows take the payer introduction (C7.3). A probe with an ample
/// budget measures what the owner pays up to that row. With one unit less, the owner pays for
/// the send to the factory and refuses row 0: the budget attempt of row 0 is refused, and the
/// deploy certifies through its own native replay. The probe holds no refused attempt, so the
/// refusal comes from the budget. No part of the creation picks at random, so one run on each
/// runtime suffices.
#[test]
fn creation_with_a_skipped_first_row_replays() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("T-C4-29, {workers} workers");
            let term = signed_creation();
            let owner = sig_to_cost_signature(&creation_signature()).expect("a ground signature");
            let probe_label = format!("{label}: the probe of the budget");
            let mut probe_chain = BoundChain::new(creation_binding(OWNER_BUDGET)).await;
            let probe = probe_chain.deploy(Role::Client, &term, &probe_label).await;
            assert_runs(&probe_label, &probe);
            let (probe_recording, _) = journal_of(&probe);
            assert!(
                probe_recording
                    .attempts
                    .iter()
                    .all(|attempt| !attempt.owner_refused),
                "{probe_label}: an ample budget refuses nothing"
            );
            let budget = skipping_budget(&probe_label, &probe, &owner);
            let mut chain = BoundChain::new(creation_binding(budget)).await;
            let played = chain.deploy(Role::Client, &term, &label).await;
            assert_runs(&label, &played);
            let index = introduction_attempt_of(&label, first_space_row(&label, &played));
            let (recording, _) = journal_of(&played);
            assert!(
                recording.attempts[index].owner_refused,
                "{label}: the owner refuses row 0 of the creation"
            );
        });
    }
}

// ─── T-C4-31 and T-C4-34: the books of the refused and the item-failed pairs ───────────────────

/// Added by decision 124 (batch B, step 6, T-C4-34): the two-bind waiter of the owner of case
/// (a) on `a` and `b`.
const BOOK_PAIR_WAITER: &str = r#"for (@x <- a & @y <- b) { out!(["w1", x]) }"#;

/// Added by decision 124 (batch B, step 6, T-C4-34): the later waiter of case (a): a waiter of
/// the client, so its COMM holds no owner region and no draw of the owner. Its channels are those
/// of [`BOOK_PAIR_WAITER`], so the two waiters share a join group, and play shuffles them: an
/// offer visits the join groups of a channel in turn and shuffles only inside a group
/// (`pick_union_order`).
const BOOK_LATER_WAITER: &str = r#"for (@x <- a & @y <- b) { out!(["w2", x]) }"#;

/// Added by decision 124 (batch B, step 6, T-C4-34): the chain that measures the tuple of the
/// waiter of case (a): the client creates the bag with 2 on `b`, and the owner parks the waiter.
const BOOK_WAITER_PROBE_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:bag",
    ctor: r#""default""#,
    client: "b!(2)",
    steps: &[(Role::Owner, BOOK_PAIR_WAITER)],
};

/// Added by decision 124 (batch B, step 6, T-C4-34): the chain of case (a):
/// [`BOOK_WAITER_PROBE_CHAIN`] and the later waiter of the client.
const BOOK_SEND_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:bag",
    ctor: r#""default""#,
    client: "b!(2)",
    steps: &[
        (Role::Owner, BOOK_PAIR_WAITER),
        (Role::UnfundedClient, BOOK_LATER_WAITER),
    ],
};

/// Added by decision 124 (batch B, step 6, T-C4-34): the chain of the receive cases: the owner
/// stores 1 on `a` and 2 on `b`.
const BOOK_RECORDS_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:bag",
    ctor: r#""default""#,
    client: "Nil",
    steps: &[(Role::Owner, "a!(1) | b!(2)")],
};

/// Added by decision 124 (batch B, step 6, T-C4-31 and T-C4-34): the two-bind receive of the
/// client.
const BOOK_PAIR_RECEIVE: &str = r#"for (@x <- a & @y <- b) { out!(["k", x]) }"#;

/// Added by decision 124 (batch B, step 6, T-C4-34): the guarded form of [`BOOK_PAIR_RECEIVE`],
/// whose guard passes on the only tuple.
const BOOK_GUARDED_RECEIVE: &str = r#"for (@x <- a & @y <- b where y == 2) { out!(["k", x]) }"#;

/// Added by decision 124 (batch B, step 6, T-C4-34): case (a). The owner has a budget that passes
/// the item tests of its waiter and refuses its tuple (rule R7), so a send that meets the waiter
/// first has that fire refused at the commit, and the later waiter of the client fires, with
/// Tail Return. Play shuffles the two waiters of the join group, so about half of the
/// [`PICK_RUNS`] runs try the waiter of the owner first, and all miss it with a chance of
/// `(1/2)^64`. Each run certifies through its own native replay.
async fn refused_send_book_case(workers: usize) {
    let label = format!("T-C4-34 (a), {workers} workers");
    let budget = refusing_budget(&BOOK_WAITER_PROBE_CHAIN, "a!(1)", &label).await;
    let mut chain = BoundChain::new(owner_binding_with(budget)).await;
    let setup = refusal_setup(&mut chain, &label, &BOOK_SEND_CHAIN).await;
    let mut refused = 0_usize;
    for run in 0..PICK_RUNS {
        let trigger = format!("{label}: the send, run {run}");
        let played = refusal_trigger(&mut chain, &setup.root, "a!(1)", &trigger).await;
        assert_runs(&trigger, &played);
        assert_eq!(
            (
                deliveries(&played, "w1"),
                deliveries(&played, "w2"),
                tail_branch(&trigger, &played)
            ),
            (0, 1, TailBranch::Return),
            "{trigger}: the later waiter fires, and Tail takes Return"
        );
        let (_, journal) = journal_of(&played);
        refused += usize::from(refusal_rows(journal) > 0);
    }
    assert!(
        refused > 0,
        "{label}: some run refuses the tuple of the waiter of the owner"
    );
}

/// Added by decision 124 (batch B, step 6, T-C4-34, errata V4-17): cases (b) and (c). The
/// receive `receive` of two binds passes its item tests, and its tuple is refused at the tuple
/// test (rule R7), so it parks with Tail Return, and its row lists the refused pair. Replay
/// verifies the refusal, so the deploy certifies through its own native replay. Without owner
/// funding, the receive fires. Both records are the only members of their channels, so one run
/// on each runtime shows the path.
async fn refused_receive_book_case(workers: usize, case: &str, receive: &str) {
    let label = format!("T-C4-34 ({case}), {workers} workers");
    let budget = refusing_budget(&BOOK_RECORDS_CHAIN, receive, &label).await;
    let mut chain = BoundChain::new(owner_binding_with(budget)).await;
    let setup = refusal_setup(&mut chain, &label, &BOOK_RECORDS_CHAIN).await;
    let trigger = format!("{label}: the receive");
    let played = refusal_trigger(&mut chain, &setup.root, receive, &trigger).await;
    assert_runs(&trigger, &played);
    assert_eq!(
        (
            deliveries(&played, "k"),
            parked_waiters(&played).len(),
            tail_branch(&trigger, &played)
        ),
        (0, 1, TailBranch::Return),
        "{trigger}: the receive parks, and Tail takes Return"
    );
    let (_, journal) = journal_of(&played);
    assert!(
        refusal_rows(journal) > 0,
        "{trigger}: the row of the receive lists the refused pair"
    );
    let mut control = NativeChain::new().await;
    let unfunded = refusal_setup(&mut control, &label, &BOOK_RECORDS_CHAIN).await;
    let native = refusal_trigger(&mut control, &unfunded.root, receive, &trigger).await;
    assert_runs(&trigger, &native);
    assert_eq!(
        deliveries(&native, "k"),
        1,
        "{trigger}: without owner funding, the receive fires"
    );
}

/// Added by decision 124 (batch B, step 6, T-C4-34, errata V4-17): refused pairs of a row that
/// does not stop are listed in the row, and replay verifies them. Case (a): a send whose
/// two-bind waiter of the owner is refused at the tuple test, while a later waiter fires. Case
/// (b): an unguarded two-bind receive that is refused at the tuple test. Case (c): the same
/// receive with a guard, an E3-a park whose refusal replay verifies through its row search. The
/// budget of the owner comes from a probe of the same chain, so the item tests pass and the
/// tuple test refuses. Each deploy certifies through its own native replay on each runtime of
/// `RACING_WORKERS`.
#[test]
fn refused_pairs_are_booked_without_a_replay_deviation() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            refused_send_book_case(workers).await;
            refused_receive_book_case(workers, "b", BOOK_PAIR_RECEIVE).await;
            refused_receive_book_case(workers, "c", BOOK_GUARDED_RECEIVE).await;
        });
    }
}

/// Added by decision 124 (batch B, step 6, T-C4-31): the record of the client on `a` of the
/// consume case, the pick `r2` of errata V4-11.
const BOOK_CLIENT_RECORD: &str = r#"a!("r2")"#;

/// Added by decision 124 (batch B, step 6, T-C4-31): the record of the owner on `b` of the
/// consume case, the second datum of the tuple of the pick.
const BOOK_OWNER_RECORD: &str = r#"b!("y")"#;

/// Added by decision 124 (batch B, step 6, T-C4-31): the length of the record `r1` of the owner
/// on `a`. The record is long enough that its item test costs more than the whole tuple of `r2`
/// and `y`, so `r1` fails its item test at the budget that passes the items of that tuple. The
/// transfer bytes of a datum are its encoded length, and a short record encodes in less than 1
/// KiB.
const BOOK_LARGE_RECORD_BYTES: usize = 4096;

/// Added by decision 124 (batch B, step 6, T-C4-31): the chain that measures the tuple of `r2`
/// and `y`: the client creates the bag with `r2` on `a`, and the owner stores `y` on `b`.
const BOOK_CONSUME_PROBE_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:bag",
    ctor: r#""default""#,
    client: BOOK_CLIENT_RECORD,
    steps: &[(Role::Owner, BOOK_OWNER_RECORD)],
};

/// Added by decision 124 (batch B, step 6, T-C4-31, errata V4-11): the consume case. The receive
/// `K` of two binds meets the records `r1` of the owner and `r2` of the client on `a`, and `y` of
/// the owner on `b`. The budget of the owner passes the item tests of `r2` and `y` and refuses
/// their tuple (rule R7), and `r1` fails its item test (rule R5), so `K` parks in every run,
/// and its row lists the refused pair. Play shuffles the records of `a`, so `K` meets `r1` first
/// in about half of the runs. Replay meets the records in the order of DR-75. Each run certifies
/// through its own native replay. Without owner funding, some run binds `r1`, so both records
/// are candidates, with a chance of `(1/2)^64` of no such run.
async fn consume_book_case(workers: usize) {
    let label = format!("T-C4-31 (consume), {workers} workers");
    let budget = refusing_budget(&BOOK_CONSUME_PROBE_CHAIN, BOOK_PAIR_RECEIVE, &label).await;
    let large = "r".repeat(BOOK_LARGE_RECORD_BYTES);
    let large_send = format!(r#"a!("{large}")"#);
    let steps: [(Role, &str); 2] = [
        (Role::Owner, BOOK_OWNER_RECORD),
        (Role::Owner, large_send.as_str()),
    ];
    let creation = stashing(
        "rho:space:hashmap:bag",
        r#""default""#,
        REFUSAL_STASH,
        BOOK_CLIENT_RECORD,
    );
    let mut chain = BoundChain::new(owner_binding_with(budget)).await;
    let setup = steps_setup(&mut chain, &label, &creation, &steps).await;
    for run in 0..PICK_RUNS {
        let trigger = format!("{label}: the receive, run {run}");
        let played = refusal_trigger(&mut chain, &setup.root, BOOK_PAIR_RECEIVE, &trigger).await;
        assert_runs(&trigger, &played);
        assert_eq!(
            (deliveries(&played, "k"), parked_waiters(&played).len()),
            (0, 1),
            "{trigger}: the receive parks"
        );
        let (_, journal) = journal_of(&played);
        assert!(
            refusal_rows(journal) > 0,
            "{trigger}: the row of the receive lists the refused pair"
        );
    }
    let mut control = NativeChain::new().await;
    let unfunded = steps_setup(&mut control, &label, &creation, &steps).await;
    let mut bound_large = 0_usize;
    for run in 0..PICK_RUNS {
        let trigger = format!("{label}: the receive, run {run}");
        let played =
            refusal_trigger(&mut control, &unfunded.root, BOOK_PAIR_RECEIVE, &trigger).await;
        assert_runs(&trigger, &played);
        bound_large += usize::from(delivered_strings(&played, "k") == [large.as_str()]);
    }
    assert!(
        bound_large > 0,
        "{label}: without owner funding, some run binds the record of the owner"
    );
}

/// Added by decision 124 (batch B, step 6, T-C4-31, errata V4-18): the two-bind guarded waiter
/// of the produce case: its guard passes for one record of its window on `b` and fails for the
/// other.
const BOOK_GUARDED_WAITER: &str = r#"for (@v <- a & @y <- b where y == 1) { out!(["owner", v]) }"#;

/// Added by decision 124 (batch B, step 6, T-C4-31): the check of the Return case after the run
/// `played`: the waiter of the owner never delivers.
fn book_owner_refused(label: &str, played: &Played) {
    assert_eq!(
        deliveries(played, "owner"),
        0,
        "{label}: the owner refuses its waiter"
    );
}

/// Added by decision 124 (batch B, step 6, T-C4-31): the check of the Stop case after the run
/// `played`: the peek of the client delivers once, and the waiter of the owner never delivers.
fn book_peek_fires(label: &str, played: &Played) {
    assert_eq!(
        (deliveries(played, "peek"), deliveries(played, "owner")),
        (1, 0),
        "{label}: the peek of the client fires, and the owner refuses its waiter"
    );
}

/// Added by decision 124 (batch B, step 6, T-C4-31, errata V4-18): the Return branch of the
/// produce case. The send meets only the guarded waiter of the owner, whose items fail at
/// budget 0, so 5 stores, and Tail(actual) is Return.
const BOOK_STORE_CASE: TailCase = TailCase {
    name: "T-C4-31 (produce, Return)",
    chain: RefusalChain {
        urn: "rho:space:hashmap:bag",
        ctor: r#""default""#,
        client: "b!(1) | b!(2)",
        steps: &[(Role::Owner, BOOK_GUARDED_WAITER)],
    },
    trigger: "a!(5)",
    allowed: &[TailBranch::Return],
    needed: TailBranch::Return,
    runs: PICK_RUNS,
    most_anchors: 1,
    check: book_owner_refused,
    control: None,
};

/// Added by decision 124 (batch B, step 6, T-C4-31, errata V4-18): the Stop branch of the
/// produce case. The client first parks a peek on `a` and `b`, so the waiter of the owner joins
/// the same group, and play shuffles the waiters of a group (`pick_group_order`). In a run that
/// meets the waiter of the owner first, the send fires the peek after the refusal of that
/// waiter, and Tail(actual) is Stop, the path of the books of both roles.
const BOOK_STOP_CASE: TailCase = TailCase {
    name: "T-C4-31 (produce, Stop)",
    chain: RefusalChain {
        urn: "rho:space:hashmap:bag",
        ctor: r#""default""#,
        client: "b!(1) | b!(2)",
        steps: &[
            (
                Role::UnfundedClient,
                r#"for (@v <<- a & @y <<- b) { out!(["peek", v]) }"#,
            ),
            (Role::Owner, BOOK_GUARDED_WAITER),
        ],
    },
    trigger: "a!(5)",
    allowed: &[TailBranch::Return, TailBranch::Stop],
    needed: TailBranch::Stop,
    runs: PICK_RUNS,
    most_anchors: 2,
    check: book_peek_fires,
    control: None,
};

/// Added by decision 124 (batch B, step 6, T-C4-31, V3-15, errata V4-11, V4-13 and V4-18): play
/// and replay meet the same item-failed and refused pairs. The consume case: the receive parks
/// in every run, whichever record of `a` play meets first. The produce case: a guarded two-bind
/// waiter fails its items, E3-a skips it in replay, and each branch of Tail(actual) replays.
/// Every run certifies through its own native replay on each runtime of `RACING_WORKERS`, so
/// replay accepts the pairs of each row of play.
#[test]
fn play_and_replay_book_the_same_item_failed_pairs() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            consume_book_case(workers).await;
            tail_case(workers, &BOOK_STORE_CASE).await;
            tail_case(workers, &BOOK_STOP_CASE).await;
        });
    }
}

// ─── CR-31: a record end after a deviation ends the staging ────────────────────────────────────

/// Added by decision 124 (batch B, step 6, rule CR-31): plays the chain `lift` on `chain` as
/// `refused_lift_case` plays it, and returns the play of the take. Each deploy certifies through
/// its own native replay.
async fn cascade_take(chain: &mut OwnedChain, label: &str, lift: &LiftChain) -> Played {
    let creation = stashing(lift.urn, lift.ctor, LIFT_STASH, lift.taken);
    let creation_label = format!("{label}: the space");
    let created = chain
        .deploy(Role::UnfundedClient, &creation, &creation_label)
        .await;
    assert_runs(&creation_label, &created);
    let records = with_stash(LIFT_STASH, lift.records);
    let records_label = format!("{label}: the records");
    let stored = chain.deploy(Role::Owner, &records, &records_label).await;
    assert_runs(&records_label, &stored);
    let waiters = with_stash(LIFT_STASH, &lift.waiters);
    let waiters_label = format!("{label}: the waiters");
    let parked = chain.deploy(Role::Owner, &waiters, &waiters_label).await;
    assert_parks(&waiters_label, &parked);
    let take = with_stash(LIFT_STASH, lift.take);
    let take_label = format!("{label}: the take");
    let played = chain.deploy(Role::UnfundedClient, &take, &take_label).await;
    assert_runs(&take_label, &played);
    played
}

/// Added by decision 124 (batch B, step 6, rule CR-31, plan-v4 CR-31 test): a cascade whose lift
/// the owner refuses deviates at the lift row, and Tail(actual) is Stop (row 8 of the table of
/// C4v2 B.6): the re-issue stages its anchor first. Native replay has no record of a row
/// after the cut, so a record end ends its staging at the same place, and the deploy certifies
/// through its own native replay. The cases are the three cascades of the refused lifts: the
/// VectorDB cascade and the list and pair cascades of the join engine. They pick nothing at
/// random, so one run on each runtime suffices.
#[test]
fn cascade_record_end_after_a_deviation_ends_the_staging() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            for (site, lift) in [
                ("the VectorDB cascade", list_lift_chain(false)),
                ("the join list cascade", list_lift_chain(true)),
                ("the join pair cascade", pair_lift_chain()),
            ] {
                let label = format!("CR-31 ({site}), {workers} workers");
                let mut chain = OwnedChain::new().await;
                let played = cascade_take(&mut chain, &label, &lift).await;
                let (_, journal) = journal_of(&played);
                assert_eq!(
                    (
                        deliveries(&played, "took"),
                        deliveries(&played, lift.kind),
                        space_parks(journal),
                        tail_branch(&label, &played)
                    ),
                    (1, 0, 1, TailBranch::Stop),
                    "{label}: the take fires, the lift parks inert, and Tail takes Stop"
                );
            }
        });
    }
}

// ─── C2v3 8.6: the VectorDB list search ────────────────────────────────────────────────────────

/// Added by decision 124 (batch B, step 6, C2v3 8.6): the records of the list case: the client
/// stores the best record and the owner stores the second. The item test of the first member of
/// a list reads the values of its bind, which hold no authority (ruling B-1), so the record of
/// the owner takes the second place, where its own payload fails its item test.
const LIST_DEVIATION_CHAIN: RefusalChain = RefusalChain {
    urn: "rho:space:hashmap:vectordb",
    ctor: VECTORDB_CTOR,
    client: r#"a!("x"; embedding=[10.0, 0.0])"#,
    steps: &[(Role::Owner, r#"a!("o"; embedding=[10.0, 1.0])"#)],
};

/// Added by decision 124 (batch B, step 6, C2v3 8.6): the list receive of both records.
const LIST_DEVIATION_RECEIVE: &str =
    r#"for (@[v, w] <- a ~ ([1.0, 0.0]; k=2)) { out!(["list", v, w]) }"#;

/// Added by decision 124 (batch B, step 6, C2v3 8.6): a list receive of a VectorDB space whose
/// list holds a record of the owner fails its item test at the commit, so the row deviates and
/// the receive parks, and the deploy certifies through its own native replay. Without owner
/// funding, the list fires. The scores are distinct, so one run on each runtime suffices.
#[test]
fn vectordb_list_search_deviation_replays() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("the VectorDB list deviation, {workers} workers");
            let mut chain = OwnedChain::new().await;
            let setup = refusal_setup(&mut chain, &label, &LIST_DEVIATION_CHAIN).await;
            let trigger = format!("{label}: the receive");
            let played =
                refusal_trigger(&mut chain, &setup.root, LIST_DEVIATION_RECEIVE, &trigger).await;
            assert_runs(&trigger, &played);
            assert_eq!(
                (deliveries(&played, "list"), parked_waiters(&played).len()),
                (0, 1),
                "{trigger}: the commit refuses the record of the owner, so the receive parks"
            );
            let mut control = NativeChain::new().await;
            let unfunded = refusal_setup(&mut control, &label, &LIST_DEVIATION_CHAIN).await;
            let native = refusal_trigger(
                &mut control,
                &unfunded.root,
                LIST_DEVIATION_RECEIVE,
                &trigger,
            )
            .await;
            assert_runs(&trigger, &native);
            assert_eq!(
                deliveries(&native, "list"),
                1,
                "{trigger}: without owner funding, the list fires"
            );
        });
    }
}

// ─── The negative tests of the owner-funded rows (decision 124, plan v4 section 10.5) ──────────

/// Added by decision 124 (plan v4 section 10.5, the negative tests): the event of a play that a
/// forged record edits.
type ForgedEvent = rspace_plus_plus::rspace::trace::event::Event;

/// Added by decision 124 (plan v4 section 10.5): the COMM of a play that a forged record edits.
type ForgedComm = rspace_plus_plus::rspace::trace::event::COMM;

/// Added by decision 124 (plan v4 section 10.5): the produce event of a play that a forged
/// record names.
type ForgedProduce = rspace_plus_plus::rspace::trace::event::Produce;

/// Added by decision 124 (plan v4 section 10.5): the source of a COMM in a journal.
type ForgedCommSource = rholang::rust::interpreter::accounting::NativeCommSource;

/// Added by decision 124 (plan v4 section 10.5): the source of a waiter in a journal.
type ForgedConsumeSource = rholang::rust::interpreter::accounting::NativeConsumeSource;

/// Added by decision 124 (plan v4 section 10.5): the signer and the owner binding of a deploy,
/// as [`signer_of`] gives them.
type ForgedFunding = (Sig, Option<Arc<OwnerFundingBinding>>);

/// Added by decision 124 (plan v4 section 10.5): the family of the plain chains of the negative
/// tests.
const FORGED_BAG: &str = "rho:space:hashmap:bag";

/// Added by decision 124 (plan v4 section 10.5): the family of the chains that bump and allocate
/// the counter of a collection (`spaces/counter.rs`).
const FORGED_QUEUE: &str = "rho:space:hashmap:queue";

/// Added by decision 124 (plan v4 section 10.5): the family of the ranked chains.
const FORGED_VDB: &str = "rho:space:hashmap:vectordb";

/// Added by decision 124 (plan v4 section 10.5): the family of the chain whose store of a record
/// adds marker rows after its record.
const FORGED_PATHMAP: &str = "rho:space:pathmap:set";

/// Added by decision 124 (plan v4 section 10.5): the constructor of the bag, queue and PathMap
/// chains of the negative tests.
const FORGED_DEFAULT_CTOR: &str = r#""default""#;

/// Added by decision 124 (plan v4 section 10.5): the evidence of an honest play in the form that
/// a negative test edits: the budget recording, the rows of the journal and the event log.
struct ForgedEvidence {
    recording: NativeBudgetRecording,
    rows: Vec<NativeOperationRecord>,
    log: Vec<ForgedEvent>,
}

impl ForgedEvidence {
    /// Added by decision 124 (plan v4 section 10.5): the evidence of the honest play `played`.
    fn of(played: &Played) -> Self {
        let (recording, journal) = committed_evidence(played);
        Self {
            recording,
            rows: journal.to_vec(),
            log: played.log.clone(),
        }
    }
}

/// Added by decision 124 (plan v4 section 10.5): a stored record of a chain: its source in the
/// journal and its produce event in the log, which a forged COMM names.
struct ForgedRecord {
    source: NativeProduceSource,
    event: ForgedProduce,
}

/// Added by decision 124 (plan v4 section 10.5): certifies `forged`, the edited evidence of the
/// play `played` of `term`, as `certify_owned` certifies a play, with the signer and the owner
/// binding of `funding`: the journal check with the owners of the binding, the trace binding,
/// and native replay with a replay budget that holds the binding. An `Err` names the layer that
/// refuses the evidence before replay.
async fn certify_forged(
    played: &Played,
    term: &str,
    funding: ForgedFunding,
    forged: ForgedEvidence,
) -> Result<Certification, String> {
    let ForgedEvidence {
        recording,
        rows,
        log,
    } = forged;
    let (signer, binding) = funding;
    let trace = with_contract(AMPLE_LIMIT, |contract| {
        contract.check_operation_journal_with_owners(
            recording.session,
            &recording,
            rows.into(),
            journal_limits(),
            binding.as_deref(),
        )
    })
    .map_err(|error| format!("the journal check rejects the journal: {error}"))?
    .bind_trace(log.into(), trace_bind_limits())
    .map_err(|error| format!("the trace binding rejects the log: {error}"))?;
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(DEPLOY, signer);
    budget
        .reset_for_native_execution(funded_config(binding, AMPLE_LIMIT))
        .expect("the replay budget enters native execution");
    let environment = create_native_replay_env_with_logger(
        trace,
        played.history.clone(),
        Arc::new(Box::new(Matcher)),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut Vec::new(),
        budget,
        ExternalServices::noop(),
        None,
        FileHandleTable::new(),
    )
    .await
    .expect("the native replay environment builds");
    Ok(finish_injected(
        environment,
        term,
        played.random.clone(),
        played.injections.clone(),
    )
    .await)
}

/// Added by decision 124 (plan v4 section 10.5): the errors of the replay `certification`, so a
/// test can name the check that refused the forged record.
fn forged_rejection_text(certification: &Certification) -> String {
    match &certification.evaluation {
        Err(error) => format!("{error:?}"),
        Ok(evaluation) => format!("{:?} {:?}", evaluation.errors, certification.complete),
    }
}

/// Added by decision 124 (plan v4 section 10.5): checks that the forged evidence passes the
/// journal check and the trace binding, so the test reaches the rule that it names, and that
/// native replay fails closed on it.
fn rejected_at_replay(label: &str, outcome: Result<Certification, String>) -> Certification {
    let certification = outcome
        .unwrap_or_else(|error| panic!("{label}: the forged evidence reaches replay: {error}"));
    assert!(
        support::native::rejected(&certification),
        "{label}: native replay fails closed on the forged record"
    );
    certification
}

/// Added by decision 124 (plan v4 section 10.5): the predecessors of each row as the journal
/// check derives them: the last earlier row that touches each channel of its footprint, as
/// `derive_predecessors` of `reified_native_spaces.rs` gives them.
fn forged_rederive_predecessors(rows: &mut [NativeOperationRecord]) {
    let mut last: HashMap<Arc<[u8]>, usize> = HashMap::with_capacity(rows.len());
    for (index, row) in rows.iter_mut().enumerate() {
        let mut expected: Vec<usize> = row
            .footprint
            .iter()
            .filter_map(|channel| last.get(channel).copied())
            .collect();
        expected.sort_unstable();
        expected.dedup();
        row.predecessors = expected.into();
        for channel in row.footprint.iter() {
            last.insert(Arc::clone(channel), index);
        }
    }
}

/// Added by decision 124 (plan v4 section 10.5): the range of the event log that each row of
/// `rows` owns, by the layout of `bind_trace`: the rows in the order of the depth-first walk of
/// the path trie, a row on the root path first, each with the width of its completion.
fn forged_event_ranges(
    recording: &NativeBudgetRecording,
    rows: &[NativeOperationRecord],
) -> Vec<std::ops::Range<usize>> {
    use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion;
    let order = recording.paths.preorder();
    let mut slot_of_path = HashMap::with_capacity(order.len());
    for (slot, path) in order.into_iter().enumerate() {
        slot_of_path.insert(path, slot + 1);
    }
    let mut slots = Vec::<(usize, usize)>::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let slot = match slot_of_path.get(&row.occurrence.path) {
            Some(slot) => *slot,
            None => 0,
        };
        slots.push((slot, index));
    }
    slots.sort_unstable();
    let mut ranges = vec![0..0; rows.len()];
    let mut cursor = 0_usize;
    for (_, index) in slots {
        let width = match rows[index].completion {
            RSpaceOperationCompletion::Stored => 1,
            RSpaceOperationCompletion::Matched => 2,
            RSpaceOperationCompletion::Rejected => 0,
        };
        ranges[index] = cursor..cursor + width;
        cursor += width;
    }
    ranges
}

/// Added by decision 124 (plan v4 section 10.5): removes the budget attempts `gone`, as a forger
/// removes the attempts of what it hides: their charges leave `used`, and every attempt link,
/// interval bound, refusal position and retry of the other rows follows the new numbering.
fn forged_remove_attempts(forged: &mut ForgedEvidence, gone: &BTreeSet<usize>) {
    use rholang::rust::interpreter::accounting::NativeObservationLink;
    let shifted = |position: usize| position - gone.range(..position).count();
    let mut spared = 0_u64;
    let mut attempts = Vec::with_capacity(forged.recording.attempts.len() - gone.len());
    for (index, attempt) in forged.recording.attempts.iter().enumerate() {
        match (gone.contains(&index), attempt.granted) {
            (false, _) => attempts.push(attempt.clone()),
            (true, false) => {}
            (true, true) => {
                spared += with_contract(AMPLE_LIMIT, |contract| {
                    contract.prepare(
                        Arc::clone(&attempt.observation),
                        journal_limits().budget.regions,
                    )
                })
                .expect("a recorded attempt prepares")
                .usage();
            }
        }
    }
    forged.recording.attempts = attempts.into();
    forged.recording.used -= spared;
    let mut retries = forged.recording.retries.to_vec();
    for retry in &mut retries {
        retry.accepted_attempt = shifted(retry.accepted_attempt);
        retry.fresh_before = shifted(retry.fresh_before);
    }
    forged.recording.retries = retries.into();
    let shift_link = |link: NativeObservationLink| match link {
        NativeObservationLink::Attempt(index) => NativeObservationLink::Attempt(shifted(index)),
        NativeObservationLink::Retry(index) => NativeObservationLink::Retry(index),
    };
    for row in &mut forged.rows {
        row.budget_start = shifted(row.budget_start);
        row.budget_end = shifted(row.budget_end);
        row.introduction = shift_link(row.introduction);
        if let Some(comm) = row.comm.as_mut() {
            comm.observation = shift_link(comm.observation);
        }
        let mut refusals = row.refusals.to_vec();
        for refusal in &mut refusals {
            refusal.position = shifted(refusal.position);
        }
        row.refusals = refusals.into();
    }
}

/// Added by decision 124 (plan v4 section 10.5): removes the retries `gone` and renumbers the
/// retry links of the rows.
fn forged_remove_retries(forged: &mut ForgedEvidence, gone: &BTreeSet<usize>) {
    use rholang::rust::interpreter::accounting::NativeObservationLink;
    let shifted = |index: usize| index - gone.range(..index).count();
    let mut retries = Vec::with_capacity(forged.recording.retries.len() - gone.len());
    for (index, retry) in forged.recording.retries.iter().enumerate() {
        match gone.contains(&index) {
            // Changed by CD-2 (clippy -D warnings, the final verification of merge 2): the empty
            // arm comes first (`clippy::single_match`). The text before:
            // false => retries.push(retry.clone()),
            // true => {}
            true => {}
            false => retries.push(retry.clone()),
        }
    }
    forged.recording.retries = retries.into();
    let shift_link = |link: NativeObservationLink| match link {
        NativeObservationLink::Attempt(index) => NativeObservationLink::Attempt(index),
        NativeObservationLink::Retry(index) => NativeObservationLink::Retry(shifted(index)),
    };
    for row in &mut forged.rows {
        row.introduction = shift_link(row.introduction);
        if let Some(comm) = row.comm.as_mut() {
            comm.observation = shift_link(comm.observation);
        }
    }
}

/// Added by decision 124 (plan v4 section 10.5): hides the rows `hidden` of the journal, as a
/// record that was cut holds no trace of them: their rows, their budget attempts and retries,
/// and their events. Every other row keeps its source and its footprint, and the predecessors
/// follow from the rows that remain.
fn forged_hide_rows(forged: &mut ForgedEvidence, hidden: &BTreeSet<usize>) {
    use rholang::rust::interpreter::accounting::NativeObservationLink;
    let ranges = forged_event_ranges(&forged.recording, &forged.rows);
    let mut attempts_gone = BTreeSet::new();
    let mut retries_gone = BTreeSet::new();
    let mut events_gone = Vec::with_capacity(hidden.len());
    for index in hidden {
        let row = &forged.rows[*index];
        let links = [
            Some(row.introduction),
            row.comm.as_ref().map(|comm| comm.observation),
        ];
        for link in links.into_iter().flatten() {
            match link {
                NativeObservationLink::Attempt(attempt) => {
                    attempts_gone.insert(attempt);
                }
                NativeObservationLink::Retry(retry) => {
                    retries_gone.insert(retry);
                }
            }
        }
        events_gone.push(ranges[*index].clone());
    }
    let mut rows = Vec::with_capacity(forged.rows.len() - hidden.len());
    for (index, row) in forged.rows.drain(..).enumerate() {
        match hidden.contains(&index) {
            // Changed by CD-2 (clippy -D warnings, the final verification of merge 2): the empty
            // arm comes first (`clippy::single_match`). The text before:
            // false => rows.push(row),
            // true => {}
            true => {}
            false => rows.push(row),
        }
    }
    forged.rows = rows;
    forged_remove_attempts(forged, &attempts_gone);
    forged_remove_retries(forged, &retries_gone);
    // Changed by CD-2 (clippy -D warnings, the final verification of merge 2): the sort reads
    // the key once for each range (`clippy::unnecessary_sort_by`). The order stays: the latest
    // start first, and a stable sort for an equal start. The text before:
    // events_gone.sort_by(|left, right| right.start.cmp(&left.start));
    events_gone.sort_by_key(|range| std::cmp::Reverse(range.start));
    for range in events_gone {
        drop(forged.log.drain(range));
    }
    forged_rederive_predecessors(&mut forged.rows);
}

/// Added by decision 124 (plan v4 section 10.5): turns the Matched row `index` into a Stored row:
/// the row loses its COMM, its COMM attempt and its COMM event, as a forger who claims that the
/// row found no candidate edits it.
fn forge_stored_row(forged: &mut ForgedEvidence, index: usize) {
    use rholang::rust::interpreter::accounting::NativeObservationLink;
    use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion;
    let ranges = forged_event_ranges(&forged.recording, &forged.rows);
    let comm_event = ranges[index].start + 1;
    let row = &mut forged.rows[index];
    let comm = row.comm.take().expect("the row to forge fires");
    let NativeObservationLink::Attempt(attempt) = comm.observation else {
        panic!("the COMM of the row links its attempt");
    };
    row.completion = RSpaceOperationCompletion::Stored;
    forged.log.remove(comm_event);
    forged_remove_attempts(forged, &BTreeSet::from([attempt]));
}

/// Added by decision 124 (plan v4 section 10.5): the attempt of a granted introduction of the
/// kind of `source`, which a forged row can claim through a retry.
fn forged_granted_introduction(forged: &ForgedEvidence, source: &NativeOperationSource) -> usize {
    use rholang::rust::interpreter::accounting::NativeObservationLink;
    let attempt = forged
        .rows
        .iter()
        .filter(|row| std::mem::discriminant(&row.source) == std::mem::discriminant(source))
        .find_map(|row| match row.introduction {
            NativeObservationLink::Attempt(attempt)
                if forged.recording.attempts[attempt].granted =>
            {
                Some(attempt)
            }
            NativeObservationLink::Attempt(_) | NativeObservationLink::Retry(_) => None,
        });
    attempt.expect("the journal holds a granted introduction of the kind of the forged row")
}

/// Added by decision 124 (plan v4 section 10.5): appends a row that the play never ran: a Stored
/// row of `source` with the footprint `footprint` and the introduction event `event`. The row
/// takes a path of its own after every honest path, so its event is the last of the log, and it
/// claims a retry of a granted introduction of its kind, so it adds no charge and passes the
/// journal check. Returns the index of the row.
fn forged_append_row(
    forged: &mut ForgedEvidence,
    source: NativeOperationSource,
    footprint: Arc<[Arc<[u8]>]>,
    event: ForgedEvent,
) -> usize {
    use rholang::rust::interpreter::accounting::{NativeBudgetRetry, NativeObservationLink};
    use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion;
    let accepted = forged_granted_introduction(forged, &source);
    let paths = Arc::make_mut(&mut forged.recording.paths);
    let mut root = forged.rows[0].occurrence.path;
    while root.depth() > 0 {
        root = paths.parent(root);
    }
    let path = paths
        .child(root, (u64::MAX, u64::MAX))
        .expect("the forged row takes a fresh path");
    let bound = forged.recording.attempts.len();
    let attempt = &forged.recording.attempts[accepted];
    let mut occurrence = attempt.occurrence.clone();
    occurrence.path = path;
    let retry = NativeBudgetRetry {
        occurrence,
        observation: Arc::clone(&attempt.observation),
        accepted_attempt: accepted,
        fresh_before: bound,
    };
    let mut retries = forged.recording.retries.to_vec();
    let retry_index = retries.len();
    retries.push(retry);
    forged.recording.retries = retries.into();
    let mut row = forged.rows[forged.rows.len() - 1].clone();
    row.occurrence.path = path;
    row.consume_peeks = match &source {
        NativeOperationSource::Produce(_) => None,
        NativeOperationSource::Consume(_) => Some(Vec::new().into()),
    };
    row.source = source;
    row.footprint = footprint;
    row.predecessors = Vec::new().into();
    row.introduction = NativeObservationLink::Retry(retry_index);
    row.comm = None;
    row.refusals = Vec::new().into();
    row.completion = RSpaceOperationCompletion::Stored;
    row.budget_start = bound;
    row.budget_end = bound;
    forged.rows.push(row);
    forged.log.push(event);
    forged_rederive_predecessors(&mut forged.rows);
    forged.rows.len() - 1
}

/// Added by decision 124 (plan v4 section 10.5): turns the Stored row `index` into a Matched row
/// with the COMM `source` and its event `comm`. The COMM claims a retry of a granted COMM of an
/// earlier row, so it adds no charge, and its event follows the introduction of the row.
fn forged_attach_comm(
    forged: &mut ForgedEvidence,
    index: usize,
    source: ForgedCommSource,
    comm: ForgedComm,
) {
    use rholang::rust::interpreter::accounting::{
        NativeBudgetRetry, NativeCommRecord, NativeObservationLink,
    };
    use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion;
    let ranges = forged_event_ranges(&forged.recording, &forged.rows);
    let event_position = ranges[index].end;
    let bound = forged.rows[index].budget_end;
    let path = forged.rows[index].occurrence.path;
    let accepted = forged.rows.iter().find_map(|row| match row.comm.as_ref() {
        Some(NativeCommRecord {
            observation: NativeObservationLink::Attempt(attempt),
            ..
        }) if *attempt < bound && forged.recording.attempts[*attempt].granted => Some(*attempt),
        _ => None,
    });
    let accepted = accepted.expect("an earlier row holds a granted COMM");
    let attempt = &forged.recording.attempts[accepted];
    let mut occurrence = attempt.occurrence.clone();
    occurrence.path = path;
    let retry = NativeBudgetRetry {
        occurrence,
        observation: Arc::clone(&attempt.observation),
        accepted_attempt: accepted,
        fresh_before: bound,
    };
    let mut retries = forged.recording.retries.to_vec();
    let retry_index = retries.len();
    retries.push(retry);
    forged.recording.retries = retries.into();
    let row = &mut forged.rows[index];
    row.comm = Some(NativeCommRecord {
        source: Arc::new(source),
        observation: NativeObservationLink::Retry(retry_index),
    });
    row.completion = RSpaceOperationCompletion::Matched;
    forged.log.insert(event_position, ForgedEvent::Comm(comm));
}

/// Added by decision 124 (plan v4 section 10.5): gives the row `row` one forged refusal that
/// names the COMM `source`, at the start of the row. `NativeRefusal` has no public path, so the
/// refusal starts as a copy of the refusal of the row `template` of an honest band run.
fn set_forged_refusal(
    row: &mut NativeOperationRecord,
    template: &NativeOperationRecord,
    source: ForgedCommSource,
) {
    let mut refusal = template.refusals[0].clone();
    refusal.source = Arc::new(source);
    refusal.position = row.budget_start;
    row.refusals = vec![refusal].into();
}

/// Added by decision 124 (plan v4 section 10.5): the COMM that fires the waiter of the Stored
/// consume row `index` of `forged` with the record `record`: its source in a journal and its
/// event in a log.
fn forged_comm_of_record(
    forged: &ForgedEvidence,
    index: usize,
    record: &ForgedRecord,
) -> (ForgedCommSource, ForgedComm) {
    use rspace_plus_plus::rspace::trace::event::IOEvent;
    let NativeOperationSource::Consume(waiter) = &forged.rows[index].source else {
        panic!("the forged row is a receive");
    };
    let ranges = forged_event_ranges(&forged.recording, &forged.rows);
    let ForgedEvent::IoEvent(IOEvent::Consume(consume)) = &forged.log[ranges[index].start] else {
        panic!("the event of the receive row is its consume");
    };
    let source = ForgedCommSource {
        consume: waiter.clone(),
        produces: vec![record.source.clone()].into(),
        peeks: Vec::new().into(),
        repetitions: vec![(record.source.clone(), 1)].into(),
    };
    let comm = ForgedComm {
        consume: consume.clone(),
        produces: vec![record.event.clone()],
        peeks: BTreeSet::new(),
        times_repeated: BTreeMap::from([(record.event.clone(), 1)]),
    };
    (source, comm)
}

/// Added by decision 124 (plan v4 section 10.5): renames the record `old` of the COMM of the
/// Matched row `index` to `new`, in the journal and in the log. The rest of the COMM stays.
fn forged_rename_record(
    forged: &mut ForgedEvidence,
    index: usize,
    old: &ForgedRecord,
    new: &ForgedRecord,
) {
    let ranges = forged_event_ranges(&forged.recording, &forged.rows);
    let position = ranges[index].start + 1;
    let ForgedEvent::Comm(comm) = &forged.log[position] else {
        panic!("the second event of a Matched row is its COMM");
    };
    let produce = |event: &ForgedProduce| match event.hash.0 == old.event.hash.0 {
        false => event.clone(),
        true => new.event.clone(),
    };
    let renamed = ForgedComm {
        consume: comm.consume.clone(),
        // Changed by CD-2 (clippy -D warnings, the final verification of merge 2): the map takes
        // the closure `produce` itself (`clippy::redundant_closure`). The text before:
        // produces: comm.produces.iter().map(|event| produce(event)).collect(),
        produces: comm.produces.iter().map(&produce).collect(),
        peeks: comm.peeks.clone(),
        times_repeated: comm
            .times_repeated
            .iter()
            .map(|(event, count)| (produce(event), *count))
            .collect(),
    };
    let record = forged.rows[index]
        .comm
        .as_mut()
        .expect("the row to rename fires");
    let swap = |source: &NativeProduceSource| match source.hash == old.source.hash {
        false => source.clone(),
        true => new.source.clone(),
    };
    let mut produces = Vec::with_capacity(record.source.produces.len());
    // Changed by CD-2 (clippy -D warnings, the final verification of merge 2): the map takes the
    // closure `swap` itself (`clippy::redundant_closure`). The text before:
    // produces.extend(record.source.produces.iter().map(|source| swap(source)));
    produces.extend(record.source.produces.iter().map(&swap));
    let mut repetitions = Vec::with_capacity(record.source.repetitions.len());
    repetitions.extend(
        record
            .source
            .repetitions
            .iter()
            .map(|(source, count)| (swap(source), *count)),
    );
    record.source = Arc::new(ForgedCommSource {
        consume: record.source.consume.clone(),
        produces: produces.into(),
        peeks: Arc::clone(&record.source.peeks),
        repetitions: repetitions.into(),
    });
    forged.log[position] = ForgedEvent::Comm(renamed);
}

/// Added by decision 124 (plan v4 section 10.5): the index of the first record of `records` that
/// the COMM `source` names.
fn forged_fired_record(source: &ForgedCommSource, records: &[ForgedRecord]) -> usize {
    records
        .iter()
        .position(|record| {
            source
                .produces
                .iter()
                .any(|produce| produce.hash == record.source.hash)
        })
        .expect("the COMM names a record of the chain")
}

/// Added by decision 124 (plan v4 section 10.5): true when the footprint of `row` names a data
/// channel, so the row writes a record.
fn forged_names_data_channel(row: &NativeOperationRecord) -> bool {
    footprint_channels(row)
        .iter()
        .any(|channel| matches!(namespace::reified_id_of(channel), Some(ReifiedId::Data(..))))
}

/// Added by decision 124 (plan v4 section 10.5): the record that the deploy `played` stores: the
/// one Stored space produce row that writes a data channel, with its event of the log.
fn forged_stored_record(label: &str, played: &Played) -> ForgedRecord {
    use rspace_plus_plus::rspace::trace::event::IOEvent;
    let (_, journal) = journal_of(played);
    let mut sources = journal
        .iter()
        .filter_map(|row| match (&row.source, &row.comm) {
            (NativeOperationSource::Produce(source), None)
                if is_space_row(row) && forged_names_data_channel(row) =>
            {
                Some(source.clone())
            }
            _ => None,
        });
    let source = sources
        .next()
        .unwrap_or_else(|| panic!("{label}: the deploy stores a record"));
    assert!(
        sources.next().is_none(),
        "{label}: the deploy stores one record"
    );
    let event = played.log.iter().find_map(|event| match event {
        ForgedEvent::IoEvent(IOEvent::Produce(produce))
            if produce.hash.0.as_slice() == source.hash.as_slice() =>
        {
            Some(produce.clone())
        }
        _ => None,
    });
    ForgedRecord {
        source,
        event: event.unwrap_or_else(|| panic!("{label}: the log holds the produce of the record")),
    }
}

/// Added by decision 124 (plan v4 section 10.5): the index of the space receive row of `rows`,
/// the last space consume row of the deploy.
fn forged_receive_row(rows: &[NativeOperationRecord]) -> usize {
    rows.iter()
        .rposition(|row| {
            matches!(row.source, NativeOperationSource::Consume(_)) && is_space_row(row)
        })
        .expect("the deploy stages a space receive")
}

/// Added by decision 124 (plan v4 section 10.5): the index of the first space produce row of
/// `rows`, the row of the offer of a send ([`offer_row`] gives the row itself).
fn forged_offer_index(rows: &[NativeOperationRecord]) -> usize {
    rows.iter()
        .position(|row| {
            matches!(row.source, NativeOperationSource::Produce(_)) && is_space_row(row)
        })
        .expect("the send stages a space produce row")
}

/// Added by decision 124 (plan v4 section 10.5): the source of a waiter that the deploy `parked`
/// parks and whose hash differs from `fired`.
fn forged_other_waiter(parked: &Played, fired: &[u8; 32]) -> ForgedConsumeSource {
    let (_, journal) = journal_of(parked);
    journal
        .iter()
        .find_map(|row| match (&row.source, &row.comm) {
            (NativeOperationSource::Consume(source), None)
                if is_space_row(row) && source.hash != *fired =>
            {
                Some(source.clone())
            }
            _ => None,
        })
        .expect("the deploy parks another waiter")
}

/// Added by decision 124 (plan v4 section 10.5): true when `row` is the put of a counter: a
/// Stored space produce row whose footprint names counters only. The bump of `spaces/counter.rs`
/// stages one such row after its take, and the allocation of a collection stages two.
fn forged_is_counter_put(row: &NativeOperationRecord) -> bool {
    match (&row.source, &row.comm) {
        (NativeOperationSource::Produce(_), None) if is_space_row(row) => {
            footprint_channels(row).iter().all(|channel| {
                matches!(
                    namespace::reified_id_of(channel),
                    Some(ReifiedId::SpaceCounter(_) | ReifiedId::CollectionCounter(..))
                )
            })
        }
        _ => false,
    }
}

impl OwnedChain {
    /// Added by decision 124 (plan v4 section 10.5): plays `term` with the random state of
    /// `label`, as `OwnedChain::play` does, with the signer and the owner binding of `funding`,
    /// so a test can fund the owner with a budget of its own.
    async fn forged_play(&mut self, funding: ForgedFunding, term: &str, label: &str) -> Played {
        let (signer, binding) = funding;
        self.runtime
            .cost
            .set_deploy_signature_funded(DEPLOY, signer);
        let random = Blake2b512Random::create_from_bytes(label.as_bytes());
        let history = self.space.get_history_repository();
        let pre_root = self.runtime.get_root().await;
        let result = self
            .runtime
            .evaluate_with_native_phlo(
                term,
                Injections::new(),
                random.clone(),
                None,
                funded_config(binding, AMPLE_LIMIT),
            )
            .await
            .expect("native play evaluates");
        let soft = self.runtime.create_soft_checkpoint().await;
        let log = soft.log.into_iter().collect();
        let out = out_data(&self.runtime).await;
        let mut outputs = Vec::<String>::with_capacity(out.len());
        outputs.extend(out.iter().map(|pars| format!("{pars:?}")));
        outputs.sort_unstable();
        let residue = hot_store_residue(&soft.cache_snapshot, Durability::Ephemeral);
        let durable = hot_store_residue(&soft.cache_snapshot, Durability::Durable);
        let snapshot_keys = snapshot_keys(&soft.cache_snapshot);
        let root = self.runtime.create_checkpoint().await.root;
        Played {
            result,
            log,
            history,
            root,
            outputs,
            out,
            random,
            residue,
            durable,
            injections: Injections::new(),
            pre_root,
            snapshot_keys,
        }
    }
}

/// Added by decision 124 (plan v4 section 10.5): the funding of the client of a chain whose owner
/// has `budget`.
fn forged_band_funding(budget: u64) -> ForgedFunding {
    (client_signer(), Some(owner_binding_with(budget)))
}

/// Added by decision 124 (plan v4 section 10.5): the records of the owner in the band chain.
const FORGED_BAND_RECORDS: &str = r#"a!("p") | b!("q")"#;

/// Added by decision 124 (plan v4 section 10.5): the receive of the client in the band chain: it
/// binds both records of the owner, so its COMM holds two owner items.
const FORGED_BAND_RECEIVE: &str = r#"for (@x <- a & @y <- b) { out!([x, y]) }"#;

/// Added by decision 124 (plan v4 section 10.5): an honest play whose journal holds a recorded
/// refusal: the play, its term, the budget of the owner in the play, and the index of the row
/// with the refusal.
struct ForgedBand {
    played: Played,
    term: String,
    budget: u64,
    row: usize,
}

impl ForgedBand {
    /// Added by decision 124 (plan v4 section 10.5): a copy of the row that holds the honest
    /// refusal, the template of each forged refusal.
    fn template(&self) -> NativeOperationRecord {
        let (_, journal) = journal_of(&self.played);
        journal[self.row].clone()
    }
}

/// Added by decision 124 (plan v4 section 10.5): true when the receive of a client whose owner
/// has `budget` fires, from the state `root`.
async fn forged_probe_fires(
    chain: &mut OwnedChain,
    root: &Blake2b256Hash,
    budget: u64,
    term: &str,
    label: &str,
) -> bool {
    chain.rewind(root).await;
    let played = chain
        .forged_play(forged_band_funding(budget), term, label)
        .await;
    assert_runs(label, &played);
    !played.outputs.is_empty()
}

/// Added by decision 124 (plan v4 section 10.5): the least budget of the owner at which the
/// receive `term` fires from the state `root`. Each probe rewinds to `root` and plays with the
/// same random state, so the hashes of the probes agree. A probe at the returned budget fires,
/// and a probe at the budget one lower does not, even if firing were not monotone in the budget.
async fn forged_least_firing_budget(
    chain: &mut OwnedChain,
    root: &Blake2b256Hash,
    term: &str,
    label: &str,
) -> u64 {
    let mut low = 0_u64;
    let mut high = 1_u64;
    // Changed by CD-2 (clippy -D warnings, the final verification of merge 2): the condition tests
    // the probe (`clippy::while_let_loop`, `clippy::redundant_pattern_matching`). The text before:
    // loop {
    //     match forged_probe_fires(chain, root, high, term, label).await {
    //         false => {
    //             low = high;
    //             high = high.checked_mul(2).expect("the owner budget doubles");
    //         }
    //         true => break,
    //     }
    // }
    while !forged_probe_fires(chain, root, high, term, label).await {
        low = high;
        high = high.checked_mul(2).expect("the owner budget doubles");
    }
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        match forged_probe_fires(chain, root, middle, term, label).await {
            false => low = middle,
            true => high = middle,
        }
    }
    high
}

/// Added by decision 124 (plan v4 section 10.5): an honest play with a recorded refusal (R7): the
/// client creates a bag, the owner stores two records, and the two-bind receive of the client
/// plays with the owner budget one below the least budget that pays its COMM. Each item of the
/// COMM still passes its item test at that budget (R5), and the whole charge does not fit, so the
/// row records one refusal and parks. The play certifies through its own native replay.
async fn forged_band_run(label: &str) -> ForgedBand {
    let mut chain = OwnedChain::new().await;
    let creation = stashing(FORGED_BAG, FORGED_DEFAULT_CTOR, REFUSAL_STASH, "Nil");
    let stored = steps_setup(&mut chain, label, &creation, &[(
        Role::Owner,
        FORGED_BAND_RECORDS,
    )])
    .await;
    let term = with_stash(REFUSAL_STASH, FORGED_BAND_RECEIVE);
    let receive_label = format!("{label}: the receive");
    let firing = forged_least_firing_budget(&mut chain, &stored.root, &term, &receive_label).await;
    let budget = firing - 1;
    chain.rewind(&stored.root).await;
    let played = chain
        .forged_play(forged_band_funding(budget), &term, &receive_label)
        .await;
    assert_runs(&receive_label, &played);
    let certification = certify_forged(
        &played,
        &term,
        forged_band_funding(budget),
        ForgedEvidence::of(&played),
    )
    .await
    .unwrap_or_else(|error| panic!("{receive_label}: the honest band run certifies: {error}"));
    assert_reproduces(&receive_label, &played, &certification);
    let (_, journal) = journal_of(&played);
    let row = journal
        .iter()
        .position(|row| !row.refusals.is_empty())
        .unwrap_or_else(|| {
            panic!("{receive_label}: one budget below the fire, the owner refuses the COMM")
        });
    ForgedBand {
        played,
        term,
        budget,
        row,
    }
}

/// Added by decision 124 (plan v4 section 10.5, R7): a dropped refusal fails closed. The honest
/// band run records one refusal for the receive of the client. A forger drops it, so the record
/// says that the receive stored with no refused candidate, and native replay of the Stored row
/// finds the full tuple (`(Some(_), _, Stored)`) and rejects the block.
#[test]
fn forged_dropped_refusal_fails_closed() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("a dropped refusal, {workers} workers");
            let band = forged_band_run(&label).await;
            let mut forged = ForgedEvidence::of(&band.played);
            forged.rows[band.row].refusals = Vec::new().into();
            rejected_at_replay(
                &label,
                certify_forged(
                    &band.played,
                    &band.term,
                    forged_band_funding(band.budget),
                    forged,
                )
                .await,
            );
        });
    }
}

/// Added by decision 124 (plan v4 section 10.5): the two waiters of the owner on `a` in the chain
/// of the forged shortfall.
const FORGED_TWO_WAITERS: &str =
    r#"for (@x <- a) { out!(["one", x]) } | for (@y <- a) { out!(["two", y]) }"#;

/// Added by decision 124 (plan v4 section 10.5): the send of the client on `a` in the chains of
/// the forged refusals.
const FORGED_SEND: &str = "a!(5)";

/// Added by decision 124 (plan v4 section 10.5, R6, R7): a refusal with no shortfall fails closed.
/// The owner parks two waiters on `a` with an ample budget, and the send of the client fires one
/// of them. A forger adds a refusal for the COMM of the other waiter, whose owner can pay it, so
/// the refusal check of replay finds no shortfall (`check_owner_refusal`, part b).
#[test]
fn forged_refusal_with_no_shortfall_fails_closed() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("a refusal with no shortfall, {workers} workers");
            let template = forged_band_run(&label).await.template();
            let mut chain = OwnedChain::new().await;
            let creation = stashing(FORGED_BAG, FORGED_DEFAULT_CTOR, REFUSAL_STASH, "Nil");
            let parked = steps_setup(&mut chain, &label, &creation, &[(
                Role::Owner,
                FORGED_TWO_WAITERS,
            )])
            .await;
            let term = with_stash(REFUSAL_STASH, FORGED_SEND);
            let send_label = format!("{label}: the send");
            let played = chain.deploy(Role::Client, &term, &send_label).await;
            assert_runs(&send_label, &played);
            let mut forged = ForgedEvidence::of(&played);
            let index = forged_offer_index(&forged.rows);
            let honest = forged.rows[index]
                .comm
                .as_ref()
                .expect("the send fires a waiter")
                .source
                .clone();
            let source = ForgedCommSource {
                consume: forged_other_waiter(&parked, &honest.consume.hash),
                produces: Arc::clone(&honest.produces),
                peeks: Arc::clone(&honest.peeks),
                repetitions: Arc::clone(&honest.repetitions),
            };
            set_forged_refusal(&mut forged.rows[index], &template, source);
            rejected_at_replay(
                &label,
                certify_forged(&played, &term, signer_of(Role::Client), forged).await,
            );
        });
    }
}

/// Added by decision 124 (plan v4 section 10.5): the one-bind receive of the client in the chains
/// of the forged Stored rows and the forged item failure.
const FORGED_ONE_RECEIVE: &str = r#"for (@x <- a) { out!(x) }"#;

/// Added by decision 124 (plan v4 section 10.5): the record of the owner on `a` in the same
/// chains.
const FORGED_ONE_RECORD: &str = r#"a!("p")"#;

/// Added by decision 124 (plan v4 section 10.5, R5, R6): a refusal for a candidate that fails its
/// items fails closed. An owner with budget 0 fails the item test of its record, so the receive
/// of the client parks with no refusal (R5). A forger adds a refusal for the COMM of that record,
/// which the same deploy forms when the owner can pay, so the refusal check of replay finds the
/// item failure (`check_owner_refusal`, part a).
#[test]
fn forged_refusal_for_a_candidate_that_fails_its_items_fails_closed() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("a refusal for a failed candidate, {workers} workers");
            let template = forged_band_run(&label).await.template();
            let mut chain = OwnedChain::new().await;
            let creation = stashing(FORGED_BAG, FORGED_DEFAULT_CTOR, REFUSAL_STASH, "Nil");
            let stored = steps_setup(&mut chain, &label, &creation, &[(
                Role::Owner,
                FORGED_ONE_RECORD,
            )])
            .await;
            let term = with_stash(REFUSAL_STASH, FORGED_ONE_RECEIVE);
            let receive_label = format!("{label}: the receive");
            let funded = chain.deploy(Role::Client, &term, &receive_label).await;
            assert_runs(&receive_label, &funded);
            let (_, funded_rows) = journal_of(&funded);
            let source = funded_rows[forged_receive_row(funded_rows)]
                .comm
                .as_ref()
                .expect("the receive of a funded owner fires")
                .source
                .as_ref()
                .clone();
            chain.rewind(&stored.root).await;
            let played = chain
                .deploy(Role::UnfundedClient, &term, &receive_label)
                .await;
            assert_runs(&receive_label, &played);
            let mut forged = ForgedEvidence::of(&played);
            let index = forged_receive_row(&forged.rows);
            assert!(
                forged.rows[index].comm.is_none(),
                "{label}: the item test of the owner refuses the record"
            );
            set_forged_refusal(&mut forged.rows[index], &template, source);
            rejected_at_replay(
                &label,
                certify_forged(&played, &term, signer_of(Role::UnfundedClient), forged).await,
            );
        });
    }
}

/// Added by decision 124 (plan v4 section 10.5, CR-34): the sends of the ranked chains, one per
/// deploy. The query `[1.0, 0.0]` ranks `c1` first, then `c2`, then `c3`.
const FORGED_RANKED_SENDS: [&str; 3] = [
    r#"a!("c1"; embedding=[10.0, 0.0])"#,
    r#"a!("c2"; embedding=[10.0, 1.0])"#,
    r#"a!("c3"; embedding=[10.0, 5.0])"#,
];

/// Added by decision 124 (plan v4 section 10.5): the ranked receive with no guard.
const FORGED_RANKED_PLAIN: &str = r#"for (@x <- a ~ ([1.0, 0.0])) { out!(x) }"#;

/// Added by decision 124 (plan v4 section 10.5): the ranked receive whose guard passes every
/// record.
const FORGED_RANKED_GUARD_ALL: &str =
    r#"for (@x <- a ~ ([1.0, 0.0]) where x != "none") { out!(x) }"#;

/// Added by decision 124 (plan v4 section 10.5): the ranked receive whose guard admits `c1` only.
const FORGED_RANKED_GUARD_C1: &str = r#"for (@x <- a ~ ([1.0, 0.0]) where x == "c1") { out!(x) }"#;

/// Added by decision 124 (plan v4 section 10.5): the ranked receive whose guard admits `c2` only.
const FORGED_RANKED_GUARD_C2: &str = r#"for (@x <- a ~ ([1.0, 0.0]) where x == "c2") { out!(x) }"#;

/// Added by decision 124 (plan v4 section 10.5): the list receive of the two top records.
const FORGED_RANKED_LIST: &str = r#"for (@[v, w] <- a ~ ([1.0, 0.0]; k=2)) { out!([v, w]) }"#;

/// Added by decision 124 (plan v4 section 10.5): a VectorDB chain up to its ranked receive: the
/// chain, and the records `c1`, `c2` and `c3` in that order.
struct ForgedRankedChain {
    chain: OwnedChain,
    records: Vec<ForgedRecord>,
}

/// Added by decision 124 (plan v4 section 10.5): the client creates a VectorDB space, and then
/// each role of `owners` stores its record of [`FORGED_RANKED_SENDS`] in a deploy of its own, so
/// the journal of that deploy names the source of the record. The record of a client passes its
/// item test, and the record of the owner fails it when the owner has budget 0.
async fn forged_ranked_chain(label: &str, owners: [Role; 3]) -> ForgedRankedChain {
    let mut chain = OwnedChain::new().await;
    let creation = stashing(FORGED_VDB, VECTORDB_CTOR, REFUSAL_STASH, "Nil");
    let creation_label = format!("{label}: the space");
    let created = chain
        .deploy(Role::UnfundedClient, &creation, &creation_label)
        .await;
    assert_runs(&creation_label, &created);
    let mut records = Vec::with_capacity(FORGED_RANKED_SENDS.len());
    for (index, (send, owner)) in FORGED_RANKED_SENDS.iter().zip(owners).enumerate() {
        let term = with_stash(REFUSAL_STASH, send);
        let record_label = format!("{label}: the record {index}");
        let stored = chain.deploy(owner, &term, &record_label).await;
        assert_runs(&record_label, &stored);
        records.push(forged_stored_record(&record_label, &stored));
    }
    ForgedRankedChain { chain, records }
}

/// Added by decision 124 (plan v4 section 10.5): the shape of a ranked rename: the role of each
/// record, the receive and its role, the record that the honest receive fires, and the record
/// that the forged record names.
struct ForgedRename {
    owners: [Role; 3],
    receive: &'static str,
    receiver: Role,
    honest: usize,
    named: usize,
}

/// Added by decision 124 (plan v4 section 10.5, CR-34'): the ranked receive of `rename` fires the
/// record `honest` in the honest run, and a forger renames its COMM to the record `named`, in the
/// journal and in the log. Replay runs the search of play over the ranked order, with the recorded
/// record first inside its tie only, so the better record `honest` still comes first, and the row
/// search cannot give the recorded outcome. Returns what replay of the forged record gives.
async fn forged_ranked_rename_case(label: &str, rename: ForgedRename) -> Certification {
    let mut ranked = forged_ranked_chain(label, rename.owners).await;
    let term = with_stash(REFUSAL_STASH, rename.receive);
    let receive_label = format!("{label}: the receive");
    let played = ranked
        .chain
        .deploy(rename.receiver, &term, &receive_label)
        .await;
    assert_runs(&receive_label, &played);
    let mut forged = ForgedEvidence::of(&played);
    let index = forged_receive_row(&forged.rows);
    let honest = forged_fired_record(
        forged.rows[index]
            .comm
            .as_ref()
            .expect("the ranked receive fires")
            .source
            .as_ref(),
        &ranked.records,
    );
    assert_eq!(
        honest, rename.honest,
        "{label}: the ranked receive fires the expected record"
    );
    let old = &ranked.records[honest];
    let new = &ranked.records[rename.named];
    forged_rename_record(&mut forged, index, old, new);
    rejected_at_replay(
        label,
        certify_forged(&played, &term, signer_of(rename.receiver), forged).await,
    )
}

/// Added by decision 124 (plan v4 section 10.5, CR-34'): a ranked site with no guard or with an
/// item failure rejects a name outside the allowed set. With no guard, a forger names `c2` while
/// `c1` passes its pattern and its items. With an item failure, `c1` of the owner with budget 0
/// fails its items, the honest receive fires `c2`, and a forger names `c3` while `c2` passes.
#[test]
fn forged_ranked_name_outside_the_allowed_set_fails_closed() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let plain = format!("a ranked name with no guard, {workers} workers");
            forged_ranked_rename_case(&plain, ForgedRename {
                owners: [Role::Owner; 3],
                receive: FORGED_RANKED_PLAIN,
                receiver: Role::Client,
                honest: 0,
                named: 1,
            })
            .await;
            let failed = format!("a ranked name past an item failure, {workers} workers");
            forged_ranked_rename_case(&failed, ForgedRename {
                owners: [Role::Owner, Role::Client, Role::Client],
                receive: FORGED_RANKED_PLAIN,
                receiver: Role::UnfundedClient,
                honest: 1,
                named: 2,
            })
            .await;
        });
    }
}

/// Added by decision 124 (plan v4 section 10.5): the plain chain of one record of the owner and
/// the receive of `receiver` that fires it: the chain ends after the receive. Returns the play
/// and its term.
async fn forged_receive_play(label: &str, receiver: Role) -> (Played, String) {
    let mut chain = OwnedChain::new().await;
    let creation = stashing(FORGED_BAG, FORGED_DEFAULT_CTOR, REFUSAL_STASH, "Nil");
    steps_setup(&mut chain, label, &creation, &[(
        Role::Owner,
        FORGED_ONE_RECORD,
    )])
    .await;
    let term = with_stash(REFUSAL_STASH, FORGED_ONE_RECEIVE);
    let receive_label = format!("{label}: the receive");
    let played = chain.deploy(receiver, &term, &receive_label).await;
    assert_runs(&receive_label, &played);
    (played, term)
}

/// Added by decision 124 (plan v4 section 10.5, R17): a forged Stored row at an unguarded site
/// fails closed. The receive of the client fires the record of the owner. A forger edits its row
/// to Stored and removes its COMM, its COMM attempt and its COMM event, so the record claims that
/// the receive found no candidate. Replay of the Stored row finds the record, which passes its
/// pattern and its items, and fails closed (rule R17, no E3-a excuse at an unguarded site).
#[test]
fn forged_stored_row_at_an_unguarded_site_fails_closed() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("a forged Stored row, {workers} workers");
            let (played, term) = forged_receive_play(&label, Role::Client).await;
            let mut forged = ForgedEvidence::of(&played);
            let index = forged_receive_row(&forged.rows);
            forge_stored_row(&mut forged, index);
            rejected_at_replay(
                &label,
                certify_forged(&played, &term, signer_of(Role::Client), forged).await,
            );
        });
    }
}

/// Added by decision 102 (R17): the chain of an unguarded waiter and the
/// send of `sender` that fires it. Returns the play and its term.
async fn forged_send_play(label: &str, sender: Role) -> (Played, String) {
    let mut chain = OwnedChain::new().await;
    let creation = stashing(FORGED_BAG, FORGED_DEFAULT_CTOR, REFUSAL_STASH, "Nil");
    steps_setup(&mut chain, label, &creation, &[(
        Role::Owner,
        FORGED_ONE_RECEIVE,
    )])
    .await;
    let term = with_stash(REFUSAL_STASH, FORGED_ONE_RECORD);
    let send_label = format!("{label}: the send");
    let played = chain.deploy(sender, &term, &send_label).await;
    assert_runs(&send_label, &played);
    (played, term)
}

/// Added by decision 102 (R17): a forged Stored row on an unguarded send
/// fails closed. The send of the client fires the waiter of the owner. A forger edits the row to
/// Stored and removes its COMM, its COMM attempt and its COMM event. Replay of the Stored row
/// finds the waiter, which has no guard and no theory, so it has no E3-a excuse.
#[test]
fn forged_stored_row_on_an_unguarded_send_fails_closed() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("a forged Stored send row, {workers} workers");
            let (played, term) = forged_send_play(&label, Role::Client).await;
            let mut forged = ForgedEvidence::of(&played);
            let index = forged_offer_index(&forged.rows);
            assert!(
                forged.rows[index].comm.is_some(),
                "{label}: the send fires the waiter"
            );
            forge_stored_row(&mut forged, index);
            rejected_at_replay(
                &label,
                certify_forged(&played, &term, signer_of(Role::Client), forged).await,
            );
        });
    }
}

/// Added by decision 124 (plan v4 section 10.5): the hash of the rows that the negative tests add
/// to a record.
const FORGED_ROW_HASH: [u8; 32] = [0x5A; 32];

/// Added by decision 124 (plan v4 section 10.5): the Stored produce row that a test adds to a
/// record, with the channel of the produce event `template` and a hash of its own, and its
/// event. The event starts as a copy of `template`, so its telemetry passes the trace binding.
fn forged_added_produce(template: &ForgedProduce) -> (NativeOperationSource, ForgedEvent) {
    use rspace_plus_plus::rspace::trace::event::IOEvent;
    let mut produce = template.clone();
    produce.hash = Blake2b256Hash(FORGED_ROW_HASH.to_vec());
    produce.persistent = false;
    let channel: [u8; 32] = produce
        .channel_hash
        .0
        .as_slice()
        .try_into()
        .expect("a channel hash holds 32 bytes");
    let source = NativeOperationSource::Produce(NativeProduceSource {
        channel,
        hash: FORGED_ROW_HASH,
        persistent: false,
    });
    (source, ForgedEvent::IoEvent(IOEvent::Produce(produce)))
}

/// Added by decision 124 (plan v4 section 10.5): the first produce event of `log`, the template of
/// an added row.
fn forged_produce_template(log: &[ForgedEvent]) -> ForgedProduce {
    use rspace_plus_plus::rspace::trace::event::IOEvent;
    log.iter()
        .find_map(|event| match event {
            ForgedEvent::IoEvent(IOEvent::Produce(produce)) => Some(produce.clone()),
            _ => None,
        })
        .expect("the log holds a produce event")
}

/// Added by decision 124 (plan v4 section 10.5): checks that the row that the test added stays
/// open: the forged record reaches replay, replay does not run the row, and its `check_complete`
/// rejects the block.
fn assert_added_row_stays_open(label: &str, outcome: Result<Certification, String>) {
    let certification = rejected_at_replay(label, outcome);
    assert!(
        certification.complete.is_err(),
        "{label}: the added row stays open: {}",
        forged_rejection_text(&certification)
    );
}

/// Added by decision 124 (plan v4 section 10.5, CR-18): a row added after the unit, outside its
/// group, fails closed. The owner stores one record on a bag, and a forger adds one more Stored
/// row after the unit of that record, on the channels of its footprint. Replay stages no such
/// row, so the added row stays open and `check_complete` rejects the block. The added row has a
/// granted retry of an honest attempt, so the journal check and the trace binding accept it.
#[test]
fn forged_row_after_the_unit_fails_closed() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("a row after the unit, {workers} workers");
            let mut chain = OwnedChain::new().await;
            let creation = stashing(FORGED_BAG, FORGED_DEFAULT_CTOR, REFUSAL_STASH, "Nil");
            steps_setup(&mut chain, &label, &creation, &[]).await;
            let term = with_stash(REFUSAL_STASH, FORGED_ONE_RECORD);
            let store_label = format!("{label}: the store");
            let played = chain.deploy(Role::Owner, &term, &store_label).await;
            assert_runs(&store_label, &played);
            let mut forged = ForgedEvidence::of(&played);
            let unit = forged
                .rows
                .iter()
                .rposition(is_space_row)
                .expect("the deploy stages a space row");
            let footprint = Arc::clone(&forged.rows[unit].footprint);
            let (source, event) = forged_added_produce(&forged_produce_template(&forged.log));
            forged_append_row(&mut forged, source, footprint, event);
            assert_added_row_stays_open(
                &label,
                certify_forged(&played, &term, signer_of(Role::Owner), forged).await,
            );
        });
    }
}

/// Added by decision 124 (plan v4 section 10.5, DR-135): a claim of a granted COMM that the owner
/// fold denies fails closed. The receive of the client fires the record of the owner, which pays
/// the COMM. A forger replays the record under an owner table with budget 0, so the fold of the
/// journal check cannot draw the COMM of the owner and rejects its granted row (`Decision`).
#[test]
fn forged_granted_comm_that_the_fold_denies_fails_closed() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("a granted COMM that the fold denies, {workers} workers");
            let (played, term) = forged_receive_play(&label, Role::Client).await;
            let mut forged = ForgedEvidence::of(&played);
            let mut table = forged.recording.owner_table.to_vec();
            assert!(!table.is_empty(), "{label}: the play holds an owner table");
            for entry in &mut table {
                entry.budget = 0;
            }
            forged.recording.owner_table = table.into();
            let error = certify_forged(&played, &term, signer_of(Role::UnfundedClient), forged)
                .await
                .err()
                .expect("the fold denies the COMM of an owner with budget 0");
            assert!(
                error.starts_with("the journal check rejects the journal"),
                "{label}: {error}"
            );
        });
    }
}

/// Added by decision 124 (plan v4 section 10.5, V4-19): the sends of the counter cuts, in the
/// order of the chain. The first allocates the counter of the collection, and the second bumps
/// it.
const FORGED_COUNTER_SENDS: [(&str, &str); 2] =
    [("the allocation", "a!(1)"), ("the bump", "a!(2)")];

/// Added by decision 124 (plan v4 section 10.5, V3-12, V4-19): a cut after the Matched take of a
/// counter fails closed, for the two shapes of a counter unit. A forger removes the counter puts
/// that follow the take, with their attempts and their events, so the record ends the bump right
/// after its Matched take (the record of the allocation loses both of its puts). The stage of the
/// first put finds no successor row, so the cursor ends the record. The take has no CanStop, so
/// the unit rule of the commit rejects the block instead of committing the prefix.
#[test]
fn forged_cut_after_a_counter_unit_fails_closed() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("a cut after a counter take, {workers} workers");
            let mut chain = OwnedChain::new().await;
            let creation = stashing(FORGED_QUEUE, FORGED_DEFAULT_CTOR, REFUSAL_STASH, "Nil");
            steps_setup(&mut chain, &label, &creation, &[]).await;
            for (case, body) in FORGED_COUNTER_SENDS {
                let case_label = format!("{label}: {case}");
                let term = with_stash(REFUSAL_STASH, body);
                let played = chain.deploy(Role::Owner, &term, &case_label).await;
                assert_runs(&case_label, &played);
                let mut forged = ForgedEvidence::of(&played);
                let take = forged
                    .rows
                    .iter()
                    .position(is_counter_take)
                    .unwrap_or_else(|| panic!("{case_label}: the send takes its counter"));
                let hidden: BTreeSet<usize> = (take + 1..forged.rows.len())
                    .filter(|index| forged_is_counter_put(&forged.rows[*index]))
                    .collect();
                assert!(
                    !hidden.is_empty(),
                    "{case_label}: the take of the counter is followed by its puts"
                );
                forged_hide_rows(&mut forged, &hidden);
                rejected_at_replay(
                    &case_label,
                    certify_forged(&played, &term, signer_of(Role::Owner), forged).await,
                );
            }
        });
    }
}

/// Added by decision 124 (plan v4 section 10.5, V3-13, T3): at a ranked receive whose guard
/// passes every record, a forger names `c2` while `c1` passes its pattern and its items. The
/// honest play fires `c1`, and replay cannot give the recorded outcome (`c1` comes first and its
/// guard passes), so it fails closed. The test asserts the rejection, not the check that rejects.
#[test]
fn forged_ranked_name_past_a_passing_guard_fails_closed() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("a ranked name past a passing guard, {workers} workers");
            forged_ranked_rename_case(&label, ForgedRename {
                owners: [Role::Owner; 3],
                receive: FORGED_RANKED_GUARD_ALL,
                receiver: Role::Client,
                honest: 0,
                named: 1,
            })
            .await;
        });
    }
}

/// Added by decision 124 (plan v4 section 10.5): the ranked receive `receive` of `receiver`,
/// which parks in the honest play, over the ranked chain of `owners`. A forger changes its row to
/// Matched with the record `named`, in the journal and in the log. Returns what replay of the
/// forged record gives, after [`rejected_at_replay`].
async fn forged_parked_rank_case(
    label: &str,
    owners: [Role; 3],
    (receive, receiver): (&str, Role),
    named: usize,
) -> Certification {
    let mut ranked = forged_ranked_chain(label, owners).await;
    let term = with_stash(REFUSAL_STASH, receive);
    let receive_label = format!("{label}: the receive");
    let played = ranked.chain.deploy(receiver, &term, &receive_label).await;
    assert_runs(&receive_label, &played);
    let mut forged = ForgedEvidence::of(&played);
    let index = forged_receive_row(&forged.rows);
    assert!(
        forged.rows[index].comm.is_none(),
        "{label}: the honest receive parks"
    );
    let (source, comm) = forged_comm_of_record(&forged, index, &ranked.records[named]);
    forged_attach_comm(&mut forged, index, source, comm);
    rejected_at_replay(
        label,
        certify_forged(&played, &term, signer_of(receiver), forged).await,
    )
}

/// Added by decision 124 (plan v4 section 10.5, V3-13, side note of arbiter v4): at a ranked
/// receive whose guard refuses `c1`, a forger names `c2` while `c1` passes its pattern and its
/// items. In single mode the receive parks on the first record that its pattern admits, so the
/// honest row is Stored and replay of the forged Matched row fails at the fit of the staged
/// consume, not at the name check (CR-34). The fit failure stops replay at the row, so the row
/// stays open, and `evaluate` reports the completion check before the result of the run. The
/// test asserts the rejection at the completion check and the fit rule of that row, and
/// `forged_parked_rank_case` asserts the honest Stored row that makes the fit fail.
#[test]
fn forged_ranked_name_past_a_refusing_guard_fails_closed() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("a ranked name past a refusing guard, {workers} workers");
            let certification = forged_parked_rank_case(
                &label,
                [Role::Owner; 3],
                (FORGED_RANKED_GUARD_C2, Role::Client),
                1,
            )
            .await;
            let rejection = forged_rejection_text(&certification);
            use rspace_plus_plus::rspace::replay_rspace::native_epoch::NativeReplayOutcome;
            use rspace_plus_plus::rspace::replay_rspace::native_session::space::outcome_fits;
            assert!(
                !outcome_fits(false, NativeReplayOutcome::Matched),
                "{label}: a Matched row does not fit a staged consume that parks"
            );
            assert!(
                rejection.contains("native replay has incomplete operations"),
                "{label}: the fit stops replay at the row, and the row stays open: {rejection}"
            );
        });
    }
}

/// Added by decision 124 (plan v4 section 10.5, V3-12): the hash of the forged retire row of a
/// retire group.
const FORGED_RETIRE_HASH: [u8; 32] = [0x5B; 32];

/// Added by decision 124 (plan v4 section 10.5, V3-12): a retire row of a member outside the
/// actual group fails closed. The list receive of the owner fires the two top records of the
/// ranked chain, and `c3` stays outside the group that the receive retires. A forger adds a
/// Matched `K_retire` row that names `c3`, on its own path after the receive and with retries of
/// honest attempts. Replay stages no retire row for `c3`, so the added row stays open and
/// `check_complete` rejects the block.
#[test]
fn forged_retire_row_outside_the_actual_group_fails_closed() {
    use rspace_plus_plus::rspace::trace::event::{Consume, IOEvent};
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("a retire row outside the group, {workers} workers");
            let mut ranked = forged_ranked_chain(&label, [Role::Owner; 3]).await;
            let term = with_stash(REFUSAL_STASH, FORGED_RANKED_LIST);
            let receive_label = format!("{label}: the receive");
            let played = ranked
                .chain
                .deploy(Role::Owner, &term, &receive_label)
                .await;
            assert_runs(&receive_label, &played);
            let mut forged = ForgedEvidence::of(&played);
            let outside = &ranked.records[2];
            let channel = outside.source.channel;
            let source = NativeOperationSource::Consume(ForgedConsumeSource {
                channels: vec![channel].into(),
                hash: FORGED_RETIRE_HASH,
                persistent: false,
            });
            let event = ForgedEvent::IoEvent(IOEvent::Consume(Consume {
                channel_hashes: vec![Blake2b256Hash(channel.to_vec())],
                hash: Blake2b256Hash(FORGED_RETIRE_HASH.to_vec()),
                persistent: false,
            }));
            let unit = forged
                .rows
                .iter()
                .rposition(is_space_row)
                .expect("the receive stages a space row");
            let footprint = Arc::clone(&forged.rows[unit].footprint);
            let index = forged_append_row(&mut forged, source, footprint, event);
            let (source, comm) = forged_comm_of_record(&forged, index, outside);
            forged_attach_comm(&mut forged, index, source, comm);
            assert_added_row_stays_open(
                &label,
                certify_forged(&played, &term, signer_of(Role::Owner), forged).await,
            );
        });
    }
}

/// Added by decision 124 (plan v4 section 10.5, V4-12): a cut after the leading anchor of a
/// transaction whose DR-75 staging succeeds fails closed. In the re-offer chain, the settle job
/// of the removal stages its anchor first and then fires. A forger hides every space row after
/// the anchor, so the miss rule stages the next operation of the job in DR-75 mode, the operation
/// after it has no ready row, and the commit fails closed.
#[test]
fn forged_cut_after_the_leading_anchor_fails_closed() {
    let records = stashing(FORGED_VDB, VECTORDB_CTOR, REOFFER_STASH, REOFFER_RECORDS);
    let waiters = with_stash(REOFFER_STASH, &list_waiters(REOFFER_WAITERS, "doc"));
    let removal = with_stash(REOFFER_STASH, REOFFER_REMOVAL);
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("a cut after the leading anchor, {workers} workers");
            let mut chain = OwnedChain::new().await;
            for (step, term) in [("the records", &records), ("the waiters", &waiters)] {
                let step_label = format!("{label}: {step}");
                let played = chain.deploy(Role::Owner, term, &step_label).await;
                assert_runs(&step_label, &played);
            }
            let removal_label = format!("{label}: the removal");
            let played = chain.deploy(Role::Owner, &removal, &removal_label).await;
            assert_runs(&removal_label, &played);
            let mut forged = ForgedEvidence::of(&played);
            let anchors = anchor_indices(&forged.rows);
            let first = *anchors
                .first()
                .unwrap_or_else(|| panic!("{removal_label}: the settle job stages an anchor"));
            let hidden: BTreeSet<usize> = (first + 1..forged.rows.len())
                .filter(|index| is_space_row(&forged.rows[*index]))
                .collect();
            assert!(
                !hidden.is_empty(),
                "{removal_label}: space rows follow the leading anchor"
            );
            forged_hide_rows(&mut forged, &hidden);
            rejected_at_replay(
                &label,
                certify_forged(&played, &removal, signer_of(Role::Owner), forged).await,
            );
        });
    }
}

/// Added by decision 124 (plan v4 section 10.5, V4-12): a row added after the anchor of a staging
/// that fails positionally fails closed. In the V4-12 chain, the settle job of the produce fails
/// on `f` with `CapacityExceeded` and commits its anchor alone. A forger adds a Stored row after
/// the anchor. The error rule of replay commits the anchor alone, and the added row stays open.
/// The test asserts that the block fails closed (a stall or `check_complete` rejects it), and not
/// that the commit of that job fails.
#[test]
fn forged_row_after_an_anchor_alone_fails_closed() {
    let records = stashing(FORGED_VDB, V412_CTOR, V412_STASH, V412_RECORDS);
    let waiters = with_stash(V412_STASH, &peek_joins_on_a(FIRE_ROUND_CAP));
    let produce = with_stash(V412_STASH, V412_PRODUCE);
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("a row after an anchor alone, {workers} workers");
            let mut chain = OwnedChain::new().await;
            for (step, term) in [("the records", &records), ("the waiters", &waiters)] {
                let step_label = format!("{label}: {step}");
                let played = chain.deploy(Role::Owner, term, &step_label).await;
                assert_runs(&step_label, &played);
            }
            let produce_label = format!("{label}: the produce");
            let played = chain.deploy(Role::Owner, &produce, &produce_label).await;
            let anchor = assert_anchor_alone(&produce_label, &played, "CapacityExceeded", 0);
            let mut forged = ForgedEvidence::of(&played);
            let footprint = Arc::clone(&forged.rows[anchor].footprint);
            let (source, event) = forged_added_produce(&forged_produce_template(&forged.log));
            forged_append_row(&mut forged, source, footprint, event);
            rejected_at_replay(
                &label,
                certify_forged(&played, &produce, signer_of(Role::Owner), forged).await,
            );
        });
    }
}

/// Added by decision 124 (plan v4 section 10.5, V4-3): a Matched row that its guard refuses fails
/// closed. At the ranked receive, the guard passes `c1` and refuses `c2`, and `c1` of the owner
/// with budget 0 fails its items, so the honest row is Stored. A forger changes the row to
/// Matched `c2`. The consume body runs `commit_check`, the guard refuses `c2`, and replay fails
/// closed.
#[test]
fn forged_matched_row_that_its_guard_refuses_fails_closed() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("a Matched row that its guard refuses, {workers} workers");
            forged_parked_rank_case(
                &label,
                [Role::Owner, Role::Client, Role::Client],
                (FORGED_RANKED_GUARD_C1, Role::UnfundedClient),
                1,
            )
            .await;
        });
    }
}

/// Added by decision 124 (plan v4 section 10.5): the waiter of the owner whose guard refuses the
/// send of the client in the chain of a forged refusal.
const FORGED_GUARDED_WAITER: &str = r#"for (@v <- a where v == 7) { out!(v) }"#;

/// Added by decision 124 (plan v4 section 10.5, V4-3): a refusal of a tuple that its guard
/// refuses fails closed. The owner parks a guarded waiter that admits 7 only, and the client of
/// an owner with budget 0 sends 5, so the honest row is Stored with no refusal. A forger adds a
/// refusal of the tuple of the waiter and the send, whose owner falls short. The finder body runs
/// `commit_check`, the guard refuses the tuple, and the refusal check fails closed.
#[test]
fn forged_refusal_that_its_guard_refuses_fails_closed() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("a refusal that its guard refuses, {workers} workers");
            let template = forged_band_run(&label).await.template();
            let mut chain = OwnedChain::new().await;
            let creation = stashing(FORGED_BAG, FORGED_DEFAULT_CTOR, REFUSAL_STASH, "Nil");
            let parked = steps_setup(&mut chain, &label, &creation, &[(
                Role::Owner,
                FORGED_GUARDED_WAITER,
            )])
            .await;
            let term = with_stash(REFUSAL_STASH, FORGED_SEND);
            let send_label = format!("{label}: the send");
            let played = chain.deploy(Role::UnfundedClient, &term, &send_label).await;
            assert_runs(&send_label, &played);
            let mut forged = ForgedEvidence::of(&played);
            let index = forged_offer_index(&forged.rows);
            assert!(
                forged.rows[index].comm.is_none(),
                "{label}: the guard refuses the send, so the row stores it"
            );
            let NativeOperationSource::Produce(send) = forged.rows[index].source.clone() else {
                panic!("{label}: the row of the offer is a produce");
            };
            let source = ForgedCommSource {
                consume: forged_other_waiter(&parked, &[0_u8; 32]),
                produces: vec![send.clone()].into(),
                peeks: Vec::new().into(),
                repetitions: vec![(send, 1)].into(),
            };
            set_forged_refusal(&mut forged.rows[index], &template, source);
            rejected_at_replay(
                &label,
                certify_forged(&played, &term, signer_of(Role::UnfundedClient), forged).await,
            );
        });
    }
}

/// Added by decision 124 (plan v4 section 10.5, V4-18): the join that a test of rule S1 parks: two
/// binds, a guard, and a pattern on `a` that binds no record of the chain.
const FORGED_VETO_JOIN: &str =
    r#"for (@"none" <- a & @y <- b where y == 1) { out!(["never", y]) }"#;

/// Added by plan v4 errata (V4-18) and decision 124 (plan v4 section 10.5, LP-2, case N2 of
/// arbiter B): a cut after a row whose only candidate is a two-bind waiter that may veto, with
/// no refusal and no item-failed pair that names the waiter, fails closed by CanStop (S1). The
/// join of the owner parks on a PathMap space, and a send stores 5 on `a`. A forger cuts the
/// record after the Stored row of the send and hides the marker rows that follow it. The patterns
/// of the waiter bind no tuple, and no pair names it.
#[test]
fn forged_cut_after_a_two_bind_vetoing_row_without_evidence_fails_closed() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            let label = format!("a cut after a vetoing row, {workers} workers");
            let mut chain = OwnedChain::new().await;
            let creation = stashing(FORGED_PATHMAP, FORGED_DEFAULT_CTOR, REFUSAL_STASH, "Nil");
            steps_setup(&mut chain, &label, &creation, &[(
                Role::Owner,
                FORGED_VETO_JOIN,
            )])
            .await;
            let term = with_stash(REFUSAL_STASH, FORGED_SEND);
            let send_label = format!("{label}: the send");
            let played = chain.deploy(Role::Owner, &term, &send_label).await;
            assert_runs(&send_label, &played);
            let mut forged = ForgedEvidence::of(&played);
            let cut = forged
                .rows
                .iter()
                .position(|row| {
                    matches!(row.source, NativeOperationSource::Produce(_))
                        && row.comm.is_none()
                        && is_space_row(row)
                        && !forged_is_counter_put(row)
                })
                .unwrap_or_else(|| panic!("{send_label}: the send stores its record"));
            let hidden: BTreeSet<usize> = (cut + 1..forged.rows.len())
                .filter(|index| is_space_row(&forged.rows[*index]))
                .collect();
            assert!(
                !hidden.is_empty(),
                "{send_label}: marker rows follow the Stored row of the record"
            );
            forged_hide_rows(&mut forged, &hidden);
            rejected_at_replay(
                &label,
                certify_forged(&played, &term, signer_of(Role::Owner), forged).await,
            );
        });
    }
}
