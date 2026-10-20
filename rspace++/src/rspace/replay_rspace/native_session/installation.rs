use std::collections::BTreeSet;

use super::*;
use crate::rspace::hashing::native_source::{self, GroupKeys};
use crate::rspace::internal::WaitingContinuation;

impl<C, P, A, K, E> NativeReplaySession<C, P, A, K, E>
where
    C: Clone
        + Debug
        + Default
        + Serialize
        + StableHashSerialize
        + serde::de::DeserializeOwned
        + ColdStoreDecode
        + Hash
        + Ord
        + Eq
        + 'static
        + Sync
        + Send,
    P: Clone
        + Debug
        + Default
        + Serialize
        + StableHashSerialize
        + serde::de::DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + StableHashSerialize
        + serde::de::DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + StableHashSerialize
        + serde::de::DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    E: NativeReplayEpoch,
{
    pub(super) fn install(
        &self,
        channels: Vec<C>,
        install: Install<P, K>,
    ) -> Result<(), RSpaceError> {
        if channels.is_empty() || channels.len() != install.patterns.len() {
            return Err(RSpaceError::BugFoundError(
                "native replay installation requires nonempty paired channels and patterns"
                    .to_owned(),
            ));
        }
        let count = channels.len();
        let mut matched = Vec::with_capacity(count);
        let mut chosen: Vec<(C, usize)> = Vec::with_capacity(count);
        // D-C2c (D-S1, DR-96): the source and the channel keys come first,
        // so the data reads use the keys.
        let (source, channel_keys) = native_source::consume_keys_for_epoch(
            self.epoch.codec_epoch(),
            &channels,
            &install.patterns,
            &install.continuation,
            true,
        )?;
        let keys = GroupKeys::from_channel_keys(channel_keys);
        let mut complete = true;
        for ((channel, pattern), channel_key) in
            channels.iter().zip(&install.patterns).zip(&keys.channels)
        {
            // Changed by D-C2c (D-S1, DR-96): the store reads by the channel
            // key. let data = self.read_data_with(channel,
            // &meter)?;
            let data = self.read_data_with(channel, *channel_key)?;
            let mut found = false;
            for (index, datum) in data.iter().enumerate() {
                let mut consumed = false;
                if !datum.persist {
                    for (previous, selected) in &chosen {
                        // Changed by D-O1 (DR-108): block accounting charges inline bytes
                        // once per enclosing block.
                        // native_backing::inspect(channel, &meter)?;
                        // native_backing::inspect(previous, &meter)?;
                        if previous == channel && *selected == index {
                            consumed = true;
                            break;
                        }
                    }
                }
                if consumed {
                    continue;
                }
                // Disabled by D-M1 (DR-88): get_metered reserves every read of
                // the pattern and the datum; this walk read nothing.
                // native_backing::inspect(pattern, &meter)?;
                // native_backing::inspect(&datum.a, &meter)?;
                let Some(value) = self.space.matcher.get(pattern, &datum.a) else {
                    continue;
                };
                matched.push(value);
                if !datum.persist {
                    // Changed by D-O1 (DR-108): block accounting charges inline bytes
                    // once per enclosing block.
                    // native_backing::reserve_copy_and_cleanup(channel, &meter)?;
                    chosen.push((channel.clone(), index));
                }
                found = true;
                break;
            }
            if !found {
                complete = false;
            }
        }
        if complete {
            // Disabled by D-M1 (DR-88): check_commit_metered reserves every
            // read of the continuation (its guard); this walk read nothing.
            // native_backing::inspect(&install.continuation, &meter)?;
            // Changed by D-M6 (DR-88): the commit check reads the matched data
            // by reference.
            let mut matched_refs = Vec::with_capacity(count);
            matched_refs.extend(matched.iter());
            if self
                .space
                .matcher
                .check_commit_by_reference(&install.continuation, &matched_refs)
            {
                return Err(RSpaceError::BugFoundError(
                    "native replay installation cannot execute a COMM".to_owned(),
                ));
            }
        }
        // Changed by D-C2c (D-S1, DR-96): the source is built before the
        // reads, and the native session uses its digest-keyed store.
        // let source = native_source::consume(
        //     &channels,
        //     &install.patterns,
        //     &install.continuation,
        //     true,
        //     &meter,
        // )?;
        // let store = self.space.get_store();
        // store.install_continuation_metered(
        //     &channels,
        //     WaitingContinuation {
        //         patterns: Arc::new(install.patterns),
        //         continuation: Arc::new(install.continuation),
        //         persist: true,
        //         peeks: BTreeSet::new(),
        //         source,
        //     },
        //     &meter,
        // )?;
        // for channel in &channels {
        //     store.install_join_metered(channel, &channels, &meter)?;
        // }
        self.store
            .install_continuation(&channels, &keys, WaitingContinuation {
                patterns: Arc::new(install.patterns),
                continuation: Arc::new(install.continuation),
                persist: true,
                peeks: BTreeSet::new(),
                source,
            })?;
        for (channel, channel_key) in channels.iter().zip(&keys.channels) {
            self.store.install_join(channel, *channel_key, &channels)?;
        }
        Ok(())
    }
}
