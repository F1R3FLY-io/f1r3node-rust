//! The merge functions of the merge suites, as free functions over a history repository (P9
//! design v2, section 3, amendment A6). They moved here from `reified_merge_p5.rs`, so the P9
//! suites compute merges in native execution with the same code as the P5 rig.
//!
//! The functions reproduce Casper's event-log conflict edges and its conflict-free merged state
//! (`casper/src/rust/merging/block_index.rs`, `create_event_log_index`, and `dag_merger.rs`).
//! Casper's rejection choice, dedup and same-deploy edges are not reproduced (amendment A7).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use models::rhoapi::{ListParWithRandom, Par};
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rholang::rust::interpreter::rho_runtime::RhoHistoryRepository;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::merging_logic::NumberChannelsDiff;
use rspace_plus_plus::rspace::merger::state_change::StateChange;
use rspace_plus_plus::rspace::merger::state_change_merger::compute_trie_actions;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce};

/// The merge index of a deploy log over the pre-state `root`, with the pre-state closures of
/// `block_index.rs::create_event_log_index`.
pub fn index(
    history: &RhoHistoryRepository,
    root: &Blake2b256Hash,
    log: &[Event],
) -> EventLogIndex {
    let reader = history
        .get_history_reader(root)
        .expect("read the history at the pre-state root");
    let produce_exists_in_pre_state = |produce: &Produce| {
        reader
            .get_data(&produce.channel_hash)
            .is_ok_and(|data| data.iter().any(|datum| datum.source == *produce))
    };
    let produce_touches_pre_state_join = |produce: &Produce| {
        reader
            .get_joins(&produce.channel_hash)
            .is_ok_and(|joins| joins.iter().any(|join| join.len() > 1))
    };
    EventLogIndex::new(
        log.to_vec(),
        produce_exists_in_pre_state,
        produce_touches_pre_state_join,
        NumberChannelsDiff::new(),
    )
}

/// The merged state of branches that the merger accepts, as `dag_merger.rs` computes it. The
/// state changes of the branches are combined, turned into trie actions over the base, and
/// applied.
pub fn merged_root(
    history: &RhoHistoryRepository,
    base: &Blake2b256Hash,
    branches: &[(&Blake2b256Hash, &EventLogIndex)],
) -> Blake2b256Hash {
    let mut combined = StateChange::empty();
    let mut writers: HashMap<Blake2b256Hash, usize> = HashMap::new();
    for (post, index) in branches {
        let change = StateChange::new(
            history
                .get_history_reader_struct(base)
                .expect("the base reader"),
            history
                .get_history_reader_struct(post)
                .expect("the post-state reader"),
            index,
        )
        .expect("the state change of a branch");
        for entry in change.datums_changes.iter() {
            if !entry.value().added.is_empty() {
                *writers.entry(entry.key().clone()).or_insert(0) += 1;
            }
        }
        combined = combined.combine(change);
    }
    let multi_writer: BTreeSet<Blake2b256Hash> = writers
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(channel, _)| channel)
        .collect();
    let reader = history.get_history_reader(base).expect("the base reader");
    let actions = compute_trie_actions(&combined, &reader, &BTreeMap::new(), |hash, changes, _| {
        if !changes.added.is_empty() {
            let base_data = reader.get_data(hash)?;
            if !base_data.is_empty() || multi_writer.contains(hash) {
                let base_binary = reader.get_data_proj_binary(hash)?;
                RholangMergingLogic::check_single_value_cell_not_overfilled(
                    hash,
                    &base_data,
                    &base_binary,
                    changes,
                )?;
            }
        }
        Ok(None)
    })
    .expect("the trie actions of the merge");
    history
        .reset(base)
        .expect("reset the history to the base")
        .do_checkpoint(actions)
        .root()
}

/// The records on the channel with the hash `channel` at the state `root`.
pub fn data_at(
    history: &RhoHistoryRepository,
    root: &Blake2b256Hash,
    channel: &Blake2b256Hash,
) -> Vec<Datum<ListParWithRandom>> {
    history
        .get_history_reader(root)
        .expect("read the history at a root")
        .get_data(channel)
        .expect("read the records of a channel")
}

/// The joins of the channel with the hash `channel` at the state `root`.
pub fn joins_at(
    history: &RhoHistoryRepository,
    root: &Blake2b256Hash,
    channel: &Blake2b256Hash,
) -> Vec<Vec<Par>> {
    history
        .get_history_reader(root)
        .expect("read the history at a root")
        .get_joins(channel)
        .expect("read the joins of a channel")
}

/// `log` without the events on the channels `channels`: the log that the space would make
/// without these channels. It generalizes `without_counters` of `reified_merge.rs`, because a
/// negative control can also strip only some counters or the allocator.
pub fn without_channels(log: &[Event], channels: &[Blake2b256Hash]) -> Vec<Event> {
    log.iter()
        .filter(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => !channels.contains(&produce.channel_hash),
            Event::IoEvent(IOEvent::Consume(consume)) => !consume
                .channel_hashes
                .iter()
                .any(|channel| channels.contains(channel)),
            Event::Comm(comm) => !comm
                .consume
                .channel_hashes
                .iter()
                .any(|channel| channels.contains(channel)),
        })
        .cloned()
        .collect()
}

/// The number of events of `log` that touch the channel with the hash `channel`.
pub fn touching(log: &[Event], channel: &Blake2b256Hash) -> usize {
    log.iter()
        .filter(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => produce.channel_hash == *channel,
            Event::IoEvent(IOEvent::Consume(consume)) => consume.channel_hashes.contains(channel),
            Event::Comm(comm) => comm.consume.channel_hashes.contains(channel),
        })
        .count()
}
