// Install registry: startup-time continuation installs and their
// re-application after store swaps.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::hash::Hash;

use serde::Serialize;

use super::RSpace;
use crate::rspace::errors::RSpaceError;
use crate::rspace::internal::*;
use crate::rspace::space_matcher::SpaceMatcher;
use crate::rspace::trace::event::Consume;

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    pub(super) fn restore_installs(&self) {
        // Move out the install map to avoid cloning the whole structure on each
        // restore.
        let installs = std::mem::take(&mut *self.installs.lock().unwrap());
        for (channels, install) in installs {
            self.locked_install_internal(channels, install.patterns, install.continuation)
                .unwrap();
        }
    }

    pub(super) fn locked_install_internal(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
    ) -> Result<Option<(K, Vec<A>)>, RSpaceError> {
        if channels.len() != patterns.len() {
            panic!("RUST ERROR: channels.length must equal patterns.length");
        }
        let consume_ref = Consume::create(&channels, &patterns, &continuation, true);
        let mut channel_to_indexed_data = self.fetch_channel_to_index_data(&channels);
        let zipped: Vec<(C, P)> = channels
            .iter()
            .cloned()
            .zip(patterns.iter().cloned())
            .collect();
        let options: Option<Vec<ConsumeCandidate<C, A>>> = self
            .extract_data_candidates(&self.matcher, &zipped, &mut channel_to_indexed_data)
            .into_iter()
            .collect();

        match options {
            None => {
                self.installs
                    .lock()
                    .unwrap()
                    .insert(channels.clone(), Install {
                        patterns: patterns.clone(),
                        continuation: continuation.clone(),
                    });

                let store = self.get_store();
                store.install_continuation(&channels, WaitingContinuation {
                    patterns,
                    continuation,
                    persist: true,
                    peeks: BTreeSet::default(),
                    source: consume_ref,
                });

                for channel in channels.iter() {
                    store.install_join(channel, &channels);
                }
                Ok(None)
            }
            Some(_) => Err(RSpaceError::BugFoundError(
                "RUST ERROR: Installing can be done only on startup".to_string(),
            )),
        }
    }
}
