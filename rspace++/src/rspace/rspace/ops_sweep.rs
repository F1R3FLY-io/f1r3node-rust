// Scope-trapped unforgeable garbage collection. Called from an ISpace
// impl's `sweep_channels` override with a list of channels to retire.
// See docs/rholang/garbage-collection.md for the heuristic and proof.

use std::collections::HashSet;
use std::fmt::Debug;
use std::hash::Hash;

use serde::Serialize;

use crate::rspace::errors::RSpaceError;
use crate::rspace::hot_store::HotStore;

/// Drain every hot-store entry keyed on a channel in `trapped_channels`:
///
/// 1. Single-channel data rows keyed on a trapped channel.
/// 2. Continuations whose channel list contains any trapped channel. For each
///    removed continuation, every channel in the list has its join registration
///    pruned — matching the contract of `remove_matched_datum_and_join` at
///    ops_produce.rs:179.
/// 3. Join index entries keyed directly on a trapped channel.
///
/// Iterates `to_map()`'s snapshot in a deterministic (sorted) order so
/// validators agree on the delta that reaches the radix trie.
pub fn locked_sweep<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
    trapped_channels: Vec<C>,
) -> Result<(), RSpaceError>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + PartialEq + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + PartialEq + Serialize + 'static + Sync + Send,
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

        let any_trapped = channels.iter().any(|c| trapped.contains(c));
        if any_trapped && !row.wks.is_empty() {
            for idx in (0..row.wks.len() as i32).rev() {
                store.remove_continuation(channels, idx);
            }
            for c in channels {
                store.remove_join(c, channels);
            }
        }
    }

    for chan in &trapped {
        for join in store.get_joins(chan) {
            store.remove_join(chan, &join);
        }
    }

    Ok(())
}
