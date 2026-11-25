// A send or receive pays, before it searches, for every candidate it could
// examine: a receive for the data waiting on its channels, a send for the data
// each waiting receive on its channel could take. Replay does not search; it
// recomputes the same charge from the deploy's log.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;

use super::errors::RSpaceError;
use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::hashing::stable_hash_provider::hash;
use super::hot_store::HotStore;
use super::trace::event::{COMM, Consume, Event, IOEvent, Produce};

/// Pays for a search over the given number of candidates; `false` when the
/// deploy cannot.
pub type SearchCharger = Arc<dyn Fn(u64) -> bool + Send + Sync>;

tokio::task_local! {
    static CHARGER: SearchCharger;
}

/// Runs `future` with `charger` paying for the searches its sends and
/// receives make.
pub async fn charged<F: Future>(charger: SearchCharger, future: F) -> F::Output {
    CHARGER.scope(charger, future).await
}

/// Charges the running operation for `candidates`. An operation run outside
/// [[charged]] belongs to a space that charges nothing, such as replay's.
pub(crate) fn charge(candidates: u64) -> Result<(), RSpaceError> {
    match CHARGER.try_with(|charger| charger(candidates)) {
        Ok(true) | Err(_) => Ok(()),
        Ok(false) => Err(RSpaceError::OutOfPhlogistons),
    }
}

/// The candidates a receive on `channels` pays for, given how much data waits
/// on each.
pub(crate) fn consume_candidates(data_on: impl Iterator<Item = usize>) -> u64 {
    data_on.map(|n| n as u64).sum()
}

/// The candidates a send pays for in one join group: each waiting receive
/// could take the data on every channel of the group, the sent datum included.
pub(crate) fn join_candidates(receives: usize, data_on_group: usize) -> u64 {
    receives as u64 * data_on_group as u64
}

/// A receive's channel hashes in the order its channels are stored under.
/// Receives on the same channels in another order wait in another group.
type Group = Vec<Blake2b256Hash>;

/// The waiting data and receives a deploy's term saw, rebuilt in log order.
struct Walk<'a, C, P, A, K> {
    store: &'a dyn HotStore<C, P, A, K>,
    channels: HashMap<Blake2b256Hash, C>,
    groups: HashMap<Blake2b256Hash, Group>,
    data: HashMap<Blake2b256Hash, HashMap<Blake2b256Hash, usize>>,
    receives: HashMap<Group, HashMap<Blake2b256Hash, usize>>,
    joins: HashMap<Blake2b256Hash, HashSet<Group>>,
    latest: HashMap<Blake2b256Hash, usize>,
}

/// The candidates the searches of `log` paid for: `log` is the events one
/// deploy's term logged, in the order play performed them, `store` the state
/// the term started from, `channels` each channel the term used, by hash, and
/// `receive_channels` the channels of each receive the term made, in the
/// order it named them, by the receive's hash.
pub fn walk_candidates<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
    channels: &HashMap<Blake2b256Hash, C>,
    receive_channels: &HashMap<Blake2b256Hash, Vec<C>>,
    log: &[Event],
) -> Result<u64, RSpaceError>
where
    C: Clone + serde::Serialize + std::hash::Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    let mut walk = Walk {
        store,
        channels: channels.clone(),
        groups: receive_channels
            .iter()
            .map(|(receive, channels)| (receive.clone(), channels.iter().map(hash).collect()))
            .collect(),
        data: HashMap::new(),
        receives: HashMap::new(),
        joins: HashMap::new(),
        latest: HashMap::new(),
    };
    let mut candidates = 0u64;
    for (position, event) in log.iter().enumerate() {
        match event {
            Event::IoEvent(IOEvent::Produce(produce)) => {
                candidates += walk.produce(produce)?;
                walk.latest.insert(produce.hash.clone(), position);
            }
            Event::IoEvent(IOEvent::Consume(consume)) => {
                candidates += walk.consume(consume)?;
                walk.latest.insert(consume.hash.clone(), position);
            }
            Event::Comm(comm) => walk.comm(comm)?,
        }
    }
    Ok(candidates)
}

fn mismatch(detail: String) -> RSpaceError { RSpaceError::SearchMismatch(detail) }

impl<C, P, A, K> Walk<'_, C, P, A, K>
where
    C: Clone + serde::Serialize + std::hash::Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    fn channel(&self, channel_hash: &Blake2b256Hash) -> Result<&C, RSpaceError> {
        self.channels.get(channel_hash).ok_or_else(|| {
            mismatch(format!("the log names channel {channel_hash} that replay never used"))
        })
    }

    fn data_on(
        &mut self,
        channel_hash: &Blake2b256Hash,
    ) -> Result<&mut HashMap<Blake2b256Hash, usize>, RSpaceError> {
        if !self.data.contains_key(channel_hash) {
            let mut waiting = HashMap::new();
            for datum in self.store.get_data_arc(self.channel(channel_hash)?) {
                *waiting.entry(datum.source.hash.clone()).or_default() += 1;
            }
            self.data.insert(channel_hash.clone(), waiting);
        }
        Ok(self.data.get_mut(channel_hash).expect("loaded above"))
    }

    fn data_count(&mut self, channel_hash: &Blake2b256Hash) -> Result<usize, RSpaceError> {
        Ok(self.data_on(channel_hash)?.values().sum())
    }

    /// The receives waiting in `group`. Loading them from the store records
    /// the group of each.
    fn receives_on(
        &mut self,
        group: &Group,
    ) -> Result<&mut HashMap<Blake2b256Hash, usize>, RSpaceError> {
        if !self.receives.contains_key(group) {
            let channels = group
                .iter()
                .map(|h| self.channel(h).cloned())
                .collect::<Result<Vec<C>, _>>()?;
            let mut waiting = HashMap::new();
            for receive in self.store.get_continuations_arc(&channels) {
                self.groups
                    .entry(receive.source.hash.clone())
                    .or_insert_with(|| group.clone());
                *waiting.entry(receive.source.hash.clone()).or_default() += 1;
            }
            self.receives.insert(group.clone(), waiting);
        }
        Ok(self.receives.get_mut(group).expect("loaded above"))
    }

    /// The group a receive waits in: one the term made, or one waiting
    /// before it in a group a send has since loaded.
    fn group_of(&self, receive: &Blake2b256Hash) -> Result<Group, RSpaceError> {
        self.groups.get(receive).cloned().ok_or_else(|| {
            mismatch(format!(
                "the log names receive {receive}, which replay never made and no send found \
                 waiting"
            ))
        })
    }

    /// The join groups on a channel, each with its waiting receives loaded.
    fn joins_on(
        &mut self,
        channel_hash: &Blake2b256Hash,
    ) -> Result<&mut HashSet<Group>, RSpaceError> {
        if !self.joins.contains_key(channel_hash) {
            let mut groups = HashSet::new();
            for channels in self.store.get_joins(self.channel(channel_hash)?) {
                let group: Group = channels.iter().map(hash).collect();
                for (joined_hash, joined) in group.iter().zip(channels) {
                    self.channels.entry(joined_hash.clone()).or_insert(joined);
                }
                self.receives_on(&group)?;
                groups.insert(group);
            }
            self.joins.insert(channel_hash.clone(), groups);
        }
        Ok(self.joins.get_mut(channel_hash).expect("loaded above"))
    }

    fn produce(&mut self, produce: &Produce) -> Result<u64, RSpaceError> {
        let groups: Vec<Group> = self
            .joins_on(&produce.channel_hash)?
            .iter()
            .cloned()
            .collect();
        let mut candidates = 0;
        for group in groups {
            let receives: usize = self
                .receives
                .get(&group)
                .expect("every join group is loaded with its receives")
                .values()
                .sum();
            let mut data = 0;
            for channel_hash in &group {
                data += self.data_count(channel_hash)? +
                    usize::from(*channel_hash == produce.channel_hash);
            }
            candidates += join_candidates(receives, data);
        }
        *self
            .data_on(&produce.channel_hash)?
            .entry(produce.hash.clone())
            .or_default() += 1;
        Ok(candidates)
    }

    fn consume(&mut self, consume: &Consume) -> Result<u64, RSpaceError> {
        let mut data = Vec::with_capacity(consume.channel_hashes.len());
        for channel_hash in &consume.channel_hashes {
            data.push(self.data_count(channel_hash)?);
        }
        let group = self.group_of(&consume.hash)?;
        *self
            .receives_on(&group)?
            .entry(consume.hash.clone())
            .or_default() += 1;
        for channel_hash in &group {
            self.joins_on(channel_hash)?.insert(group.clone());
        }
        Ok(consume_candidates(data.into_iter()))
    }

    /// Removes what a COMM consumed. The participant logged last is the
    /// operation that found the match on arrival and so was never stored;
    /// every other participant was waiting and stays only if it persists.
    fn comm(&mut self, comm: &COMM) -> Result<(), RSpaceError> {
        let consume_position = self.latest.get(&comm.consume.hash).copied();
        let produce_positions: Vec<Option<usize>> = comm
            .produces
            .iter()
            .map(|produce| self.latest.get(&produce.hash).copied())
            .collect();
        let trigger = produce_positions
            .iter()
            .copied()
            .chain(std::iter::once(consume_position))
            .flatten()
            .max();
        let triggered = |position: Option<usize>| trigger.is_some() && position == trigger;

        if !comm.consume.persistent || triggered(consume_position) {
            let group = self.group_of(&comm.consume.hash)?;
            take(self.receives_on(&group)?, &comm.consume.hash, "receive")?;
        }
        for (produce, position) in comm.produces.iter().zip(produce_positions) {
            if !produce.persistent || triggered(position) {
                take(self.data_on(&produce.channel_hash)?, &produce.hash, "datum")?;
            }
        }
        Ok(())
    }
}

fn take(
    waiting: &mut HashMap<Blake2b256Hash, usize>,
    hash: &Blake2b256Hash,
    what: &str,
) -> Result<(), RSpaceError> {
    match waiting.get_mut(hash) {
        Some(count) if *count > 1 => {
            *count -= 1;
            Ok(())
        }
        Some(_) => {
            waiting.remove(hash);
            Ok(())
        }
        None => Err(mismatch(format!(
            "a COMM takes {what} {hash}, which is not waiting at that point in the log"
        ))),
    }
}
