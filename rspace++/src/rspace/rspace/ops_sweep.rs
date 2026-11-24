// Scope-trapped unforgeable garbage collection. Called from an ISpace
// impl's `sweep_channels` override with a list of channels to retire.
// See docs/rholang/garbage-collection.md for the heuristic and proof.

use std::collections::HashSet;
use std::fmt::Debug;
use std::hash::Hash;

use serde::Serialize;

use crate::rspace::errors::RSpaceError;
use crate::rspace::hot_store::HotStore;

/// Retire single-channel data rows keyed on a trapped channel.
///
/// Scope: datums only. Earlier revisions of this sweep also deleted
/// continuations whose channel list contained a trapped channel, and the
/// join-index entries keyed on a trapped channel (both as sibling pruning
/// after continuation removal and as a defensive second pass). That was
/// unsafe in a multi-parent setting: the casper merger
/// (`rspace++/src/rspace/merger/state_change.rs:StateChange::new`) walks
/// each deploy's recorded consumes, computes a history pointer from the
/// hashed channel list, and looks up the matching join record in the pre-
/// or post-state. Deleting joins made that lookup fail — the merger
/// panics with "Tuple space inconsistency found: channel of consume does
/// not contain join record corresponding to the consume channels." See
/// `multi_parent_casper_should_estimate_parent_properly` regression.
///
/// Data rows are safe to drop because the merger reaches them by the
/// channel's hash directly (no event-log replay needed) and treats a
/// missing row as "no data on this channel" — consistent with the
/// post-COMM state the H1-safety proof holds for. The join and
/// continuation lookups are the ones with the lookup-via-event-log
/// inverse, and those we now leave alone.
///
/// Iterates `to_map()`'s snapshot in a deterministic (sorted) order so
/// validators agree on the delta that reaches the radix trie.
pub fn locked_sweep<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
    trapped_channels: Vec<C>,
) -> Result<(), RSpaceError>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    if trapped_channels.is_empty() {
        return Ok(());
    }
    let trapped: HashSet<C> = trapped_channels.into_iter().collect();

    let snapshot = store.to_map();
    let mut channel_lists: Vec<Vec<C>> = snapshot.keys().cloned().collect();
    channel_lists.sort();

    for channels in &channel_lists {
        let row = match snapshot.get(channels) {
            Some(r) => r,
            None => continue,
        };

        if channels.len() == 1 && trapped.contains(&channels[0]) && !row.data.is_empty() {
            for idx in (0..row.data.len() as i32).rev() {
                store.remove_datum(&channels[0], idx)?;
            }
        }
    }

    Ok(())
}
