use std::collections::BTreeSet;

use super::*;
use crate::rspace::hashing::native_source::{self, SourceMeter};
use crate::rspace::internal::WaitingContinuation;
use crate::rspace::native_backing;

impl<C, P, A, K, E> NativeReplaySession<C, P, A, K, E>
where
    C: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
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
        + CloneBacking
        + serde::de::DeserializeOwned
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
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
        let meter =
            |operations, scanned, backing| self.history_reserve(operations, scanned, backing);
        let count = channels.len();
        meter.reserve(1, 0, backing::<A>(count)?)?;
        let mut matched = Vec::new();
        matched
            .try_reserve_exact(count)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        meter.reserve(1, 0, backing::<(C, usize)>(count)?)?;
        let mut chosen = Vec::new();
        chosen
            .try_reserve_exact(count)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        let mut complete = true;
        for (channel, pattern) in channels.iter().zip(&install.patterns) {
            let data = self.read_data_with(channel, &meter)?;
            let mut found = false;
            for (index, datum) in data.iter().enumerate() {
                let mut consumed = false;
                if !datum.persist {
                    for (previous, selected) in &chosen {
                        native_backing::inspect(channel, &meter)?;
                        native_backing::inspect(previous, &meter)?;
                        meter.reserve(1, 0, 0)?;
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
                let Some(value) = self.space.matcher.get_metered(pattern, &datum.a, &meter)? else {
                    continue;
                };
                matched.push(value);
                if !datum.persist {
                    native_backing::reserve_copy_and_cleanup(channel, &meter)?;
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
            meter.reserve(1, 0, backing::<&A>(count)?)?;
            let mut matched_refs = Vec::new();
            matched_refs
                .try_reserve_exact(count)
                .map_err(|_| RSpaceError::HostWorkRejected)?;
            matched_refs.extend(matched.iter());
            if self.space.matcher.check_commit_metered(
                &install.continuation,
                &matched_refs,
                &meter,
            )? {
                return Err(RSpaceError::BugFoundError(
                    "native replay installation cannot execute a COMM".to_owned(),
                ));
            }
        }
        let source = native_source::consume(
            &channels,
            &install.patterns,
            &install.continuation,
            true,
            &meter,
        )?;
        let store = self.space.get_store();
        store.install_continuation_metered(
            &channels,
            WaitingContinuation {
                patterns: install.patterns,
                continuation: install.continuation,
                persist: true,
                peeks: BTreeSet::new(),
                source,
            },
            &meter,
        )?;
        for channel in &channels {
            store.install_join_metered(channel, &channels, &meter)?;
        }
        Ok(())
    }
}
