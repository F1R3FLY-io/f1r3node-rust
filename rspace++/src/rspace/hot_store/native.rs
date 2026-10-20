use super::*;
use crate::rspace::rspace_interface::RSpaceResult;

struct PreparedDataRetirement<'a, C, A: Clone> {
    shards: Vec<(usize, std::sync::RwLockWriteGuard<'a, imbl::HashMap<C, Vec<Datum<A>>>>)>,
    updates: Vec<(usize, C, Vec<Datum<A>>)>,
}

impl<C: Clone + Hash + Eq, A: Clone> PreparedDataRetirement<'_, C, A> {
    fn publish(&mut self) {
        for (index, channel, values) in std::mem::take(&mut self.updates) {
            let (_, shard) = self
                .shards
                .iter_mut()
                .find(|(candidate, _)| *candidate == index)
                .expect("requested data shard");
            shard.insert(channel, values);
        }
    }
}

struct PreparedJoinRetirement<'a, C> {
    shards: Vec<(usize, std::sync::RwLockWriteGuard<'a, imbl::HashMap<C, Vec<Vec<C>>>>)>,
    updates: Vec<(usize, C, Vec<Vec<C>>)>,
}

impl<C: Clone + Hash + Eq> PreparedJoinRetirement<'_, C> {
    fn publish(mut self) {
        for (index, channel, values) in self.updates {
            let (_, shard) = self
                .shards
                .iter_mut()
                .find(|(candidate, _)| *candidate == index)
                .expect("requested join shard");
            shard.insert(channel, values);
        }
    }
}

impl<C, P, A, K> InMemHotStore<C, P, A, K>
where
    C: Clone + Debug + Hash + Eq + Send + Sync,
    P: Clone + Debug + Send + Sync,
    A: Clone + Debug + Send + Sync,
    K: Clone + Debug + Send + Sync,
{
    fn prepare_native_retire_data(
        &self,
        data: &[RSpaceResult<C, A>],
        retirement: &[(usize, i32)],
    ) -> Result<PreparedDataRetirement<'_, C, A>, RSpaceError> {
        if retirement.is_empty() {
            return Ok(PreparedDataRetirement {
                shards: Vec::new(),
                updates: Vec::new(),
            });
        }
        let mut requested = [false; NUM_SHARDS];
        for (position, _) in retirement {
            let channel = &data
                .get(*position)
                .ok_or(RSpaceError::ProduceCommitProtocolViolation)?
                .channel;

            requested[shard_of(channel)] = true;
        }

        let shard_count = requested.iter().filter(|needed| **needed).count();
        let mut shards = Vec::with_capacity(shard_count);
        for (index, needed) in requested.into_iter().enumerate() {
            if needed {
                shards.push((index, self.data.shards[index].write().expect("shard write lock")));
            }
        }
        let mut updates = Vec::with_capacity(retirement.len());
        for (position, datum_index) in retirement {
            let channel = &data[*position].channel;
            let mut staged = None;
            for (index, (_, existing, _)) in updates.iter().enumerate() {
                if existing == channel {
                    staged = Some(index);
                    break;
                }
            }
            let staged = if let Some(index) = staged {
                index
            } else {
                let shard_index = shard_of(channel);

                let (_, shard) = shards
                    .iter()
                    .find(|(index, _)| *index == shard_index)
                    .expect("requested data shard");

                let values = shard.get(channel).ok_or_else(|| {
                    RSpaceError::InterpreterError(
                        "native datum cache is absent at retirement".to_owned(),
                    )
                })?;

                updates.push((shard_index, channel.clone(), values.clone()));
                updates.len() - 1
            };
            let values = &mut updates[staged].2;
            if *datum_index < 0 || *datum_index as usize >= values.len() {
                return Err(RSpaceError::InterpreterError(
                    "native datum retirement index is invalid".to_owned(),
                ));
            }
            let index = *datum_index as usize;

            values.remove(index);
        }
        Ok(PreparedDataRetirement { shards, updates })
    }

    pub(super) fn native_retire_resting_match(
        &self,
        channels: &[C],
        continuation_index: i32,
        continuation_persistent: bool,
        data: &[RSpaceResult<C, A>],
        retirement: &[(usize, i32)],
        authorization: &mut dyn RestingRetirementAuthorization,
    ) -> Result<(), RSpaceError> {
        self.native_retire_match(
            channels,
            continuation_index,
            continuation_persistent,
            data,
            retirement,
            Some(authorization),
        )
    }

    fn native_retire_match(
        &self,
        channels: &[C],
        continuation_index: i32,
        continuation_persistent: bool,
        data: &[RSpaceResult<C, A>],
        retirement: &[(usize, i32)],
        authorization: Option<&mut dyn RestingRetirementAuthorization>,
    ) -> Result<(), RSpaceError> {
        let mut data_update = self.prepare_native_retire_data(data, retirement)?;

        let key = channels.to_vec();
        let installed = self.installed_continuations.contains_key(&key);

        let mut continuation_shard = self.continuations.shards[shard_of(&key)]
            .write()
            .expect("shard write lock");

        let existing = continuation_shard.get(&key);
        // Added by the merge of feature/cost-accounted-rho (finding R2-08):
        // HEAD's comment, restored. HEAD named "the metered preflight". After
        // DR-131 the resident-only read `read_resting_hot` (`ops_resting.rs`)
        // refuses a cold miss before this point.
        // An installed-only persistent continuation has no ordinary hot row.
        // It remains installed after this match, so there is no row to retire.
        // Every other candidate must have been brought into the hot cache
        // before any data update may be published.
        if existing.is_none() && !installed {
            return Err(RSpaceError::InterpreterError(
                "native continuation cache is absent at match retirement".to_owned(),
            ));
        }
        let continuation_update = if continuation_persistent {
            None
        } else {
            let existing = existing.ok_or_else(|| {
                RSpaceError::InterpreterError(
                    "native continuation cache is absent at match retirement".to_owned(),
                )
            })?;
            let index = continuation_index
                .checked_sub(i32::from(installed))
                .ok_or(RSpaceError::ProduceCommitProtocolViolation)?;
            if index < 0 || index as usize >= existing.len() {
                return Err(RSpaceError::InterpreterError(
                    "native continuation retirement index is invalid".to_owned(),
                ));
            }

            let mut values = existing.clone();
            let index = index as usize;

            values.remove(index);
            Some(values)
        };
        let regular_count = continuation_update
            .as_ref()
            .map_or_else(|| existing.map_or(0, Vec::len), Vec::len);
        let join_update = if !installed && regular_count == 0 {
            self.prepare_native_retire_joins(data, channels)?
        } else {
            PreparedJoinRetirement {
                shards: Vec::new(),
                updates: Vec::new(),
            }
        };
        // Added by the merge of feature/cost-accounted-rho (finding R2-08):
        // HEAD's comment, restored without "SourceMeter call", because DR-131
        // removed the meter.
        // No validation or fallible staging follows this authorization. A
        // refusal leaves every staged shard unpublished.
        if let Some(authorization) = authorization {
            authorization.authorize()?;
        }
        data_update.publish();
        if let Some(values) = continuation_update {
            continuation_shard.insert(key, values);
        }
        join_update.publish();
        drop(data_update);
        Ok(())
    }

    fn prepare_native_retire_joins(
        &self,
        data: &[RSpaceResult<C, A>],
        join: &[C],
    ) -> Result<PreparedJoinRetirement<'_, C>, RSpaceError> {
        let mut requested = [false; NUM_SHARDS];
        for datum in data {
            requested[shard_of(&datum.channel)] = true;
        }

        let shard_count = requested.iter().filter(|needed| **needed).count();
        let mut shards = Vec::with_capacity(shard_count);
        for (index, needed) in requested.into_iter().enumerate() {
            if needed {
                shards.push((index, self.joins.shards[index].write().expect("shard write lock")));
            }
        }
        let mut updates = Vec::with_capacity(data.len());
        for datum in data {
            let channel = &datum.channel;
            let mut repeated = false;
            for (_, previous, _) in &updates {
                if previous == channel {
                    repeated = true;
                    break;
                }
            }
            if repeated {
                continue;
            }

            let index = shard_of(channel);

            let (_, shard) = shards
                .iter()
                .find(|(candidate, _)| *candidate == index)
                .expect("requested join shard");

            let existing = shard.get(channel).ok_or_else(|| {
                RSpaceError::InterpreterError(
                    "native join cache is absent at match retirement".to_owned(),
                )
            })?;
            let mut position = None;
            for (candidate, value) in existing.iter().enumerate() {
                if value.as_slice() == join {
                    position = Some(candidate);
                    break;
                }
            }
            if let Some(position) = position {
                let mut values = existing.clone();

                values.remove(position);

                updates.push((index, channel.clone(), values));
            }
        }
        Ok(PreparedJoinRetirement { shards, updates })
    }
}

#[cfg(test)]
mod tests;
