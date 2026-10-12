//! The merge-level event coverage of space writes (P9 design v2, section 5, amendment A4).
//!
//! Casper merges the state change of each branch, and `StateChange::new` reads only the channels
//! that the event log of the branch names (`produces_affected` and `consumes_affected` of
//! `rspace++/src/rspace/merger/merging_logic.rs`). A record that a deploy changes on a channel that
//! its log does not name is therefore lost in every merge. The check merges the deploy's own log
//! over its pre-state and requires its post-state root.
//!
//! The diagnostic lists the changed keys that the state change does not cover. A key is a
//! candidate when the hot-store snapshot of the play holds it ([`SnapshotKeys`]). It is changed
//! when its binary records at the pre-state and at the post-state differ as multisets.

use std::collections::BTreeSet;

use rholang::rust::interpreter::rho_runtime::RhoHistoryRepository;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash_from_hashes;
use rspace_plus_plus::rspace::merger::state_change::StateChange;
use rspace_plus_plus::rspace::trace::event::Event;

use super::merge::{index, merged_root};
use super::native::{Played, SnapshotKeys};

/// The outcome of the coverage check of one deploy.
#[derive(Debug, PartialEq, Eq)]
pub struct Coverage {
    /// True when the merge of the deploy's log over its pre-state gives its post-state root.
    pub reproduced: bool,
    /// The changed keys that the state change of the log does not cover.
    pub uncovered: SnapshotKeys,
}

/// The coverage of the log `log` of a deploy that went from `pre` to `post`, over the candidate
/// keys `keys`.
pub fn coverage(
    history: &RhoHistoryRepository,
    pre: &Blake2b256Hash,
    post: &Blake2b256Hash,
    log: &[Event],
    keys: &SnapshotKeys,
) -> Coverage {
    let log_index = index(history, pre, log);
    let reproduced = merged_root(history, pre, &[(post, &log_index)]) == *post;
    let change = StateChange::new(
        history
            .get_history_reader_struct(pre)
            .expect("the pre-state reader"),
        history
            .get_history_reader_struct(post)
            .expect("the post-state reader"),
        &log_index,
    )
    .expect("the state change of the log");
    let covered_data: BTreeSet<Blake2b256Hash> = change
        .datums_changes
        .iter()
        .map(|entry| entry.key().clone())
        .collect();
    let mut covered_continuations = BTreeSet::new();
    let mut covered_joins = BTreeSet::new();
    for entry in change.cont_changes.iter() {
        covered_continuations.insert(hash_from_hashes(entry.key()));
        covered_joins.extend(entry.key().iter().cloned());
    }
    for entry in change.consume_channels_to_join_serialized_map.iter() {
        covered_joins.extend(entry.key().iter().cloned());
    }
    let before = history
        .get_history_reader(pre)
        .expect("read the history at the pre-state root");
    let after = history
        .get_history_reader(post)
        .expect("read the history at the post-state root");
    let differs = |left: Vec<Vec<u8>>, right: Vec<Vec<u8>>| {
        let (mut left, mut right) = (left, right);
        left.sort();
        right.sort();
        left != right
    };
    let uncovered = SnapshotKeys {
        data: keys
            .data
            .iter()
            .filter(|key| !covered_data.contains(*key))
            .filter(|key| {
                differs(
                    before
                        .get_data_proj_binary(key)
                        .expect("the pre-state records"),
                    after
                        .get_data_proj_binary(key)
                        .expect("the post-state records"),
                )
            })
            .cloned()
            .collect(),
        continuations: keys
            .continuations
            .iter()
            .filter(|key| !covered_continuations.contains(*key))
            .filter(|key| {
                differs(
                    before
                        .get_continuations_proj_binary(key)
                        .expect("the pre-state continuations"),
                    after
                        .get_continuations_proj_binary(key)
                        .expect("the post-state continuations"),
                )
            })
            .cloned()
            .collect(),
        joins: keys
            .joins
            .iter()
            .filter(|key| !covered_joins.contains(*key))
            .filter(|key| {
                differs(
                    before
                        .get_joins_proj_binary(key)
                        .expect("the pre-state joins"),
                    after
                        .get_joins_proj_binary(key)
                        .expect("the post-state joins"),
                )
            })
            .cloned()
            .collect(),
    };
    Coverage {
        reproduced,
        uncovered,
    }
}

/// Asserts that `played` passes the coverage check: the merge of its log over its pre-state
/// gives its post-state root, and no changed key stays uncovered.
pub fn assert_merge_reproduces(label: &str, played: &Played) {
    let outcome = coverage(
        &played.history,
        &played.pre_root,
        &played.root,
        &played.log,
        &played.snapshot_keys,
    );
    assert!(
        outcome.reproduced && outcome.uncovered == SnapshotKeys::default(),
        "{label}: the merge of the deploy's own log does not reproduce its post-state: {outcome:?}"
    );
}
