// A send or receive pays, before it searches, for the work its search could
// do: every datum and pattern pair it could try, and how large each pair is. A
// receive pays for the data waiting on its channels; a send for the data each
// waiting receive on its channel could take. Replay does not search; it
// recomputes the same charge from the deploy's log.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::ops::AddAssign;
use std::sync::Arc;

use super::errors::RSpaceError;
use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::hashing::stable_hash_provider::hash;
use super::hot_store::HotStore;
use super::trace::event::{COMM, Consume, Event, IOEvent, Produce};

/// What one search could examine: the datum and pattern pairs it could try,
/// and the sum of each pair's sizes as the matcher measures them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchWork {
    pub candidates: u64,
    pub bytes: u64,
}

impl SearchWork {
    /// `count` data, `size` in all, each tried against a pattern of
    /// `pattern_size`, by `receives` receives.
    pub(crate) fn of(count: usize, size: usize, pattern_size: usize, receives: usize) -> Self {
        let receives = receives as u64;
        SearchWork {
            candidates: receives.saturating_mul(count as u64),
            bytes: receives.saturating_mul(
                (size as u64).saturating_add((count as u64).saturating_mul(pattern_size as u64)),
            ),
        }
    }
}

impl AddAssign for SearchWork {
    fn add_assign(&mut self, other: Self) {
        self.candidates = self.candidates.saturating_add(other.candidates);
        self.bytes = self.bytes.saturating_add(other.bytes);
    }
}

/// Pays for one search; `false` when the deploy cannot.
pub type SearchCharger = Arc<dyn Fn(SearchWork) -> bool + Send + Sync>;

tokio::task_local! {
    static CHARGER: SearchCharger;
}

/// Runs `future` with `charger` paying for the searches its sends and
/// receives make.
pub async fn charged<F: Future>(charger: SearchCharger, future: F) -> F::Output {
    CHARGER.scope(charger, future).await
}

/// Runs `future` with its searches free: a read the node makes for itself,
/// such as a system deploy reading its result back, which no deploy pays for
/// and no deploy's log records.
pub async fn uncharged<F: Future>(future: F) -> F::Output {
    CHARGER.scope(Arc::new(|_| true), future).await
}

/// Charges the running operation for `work`. Outside [[charged]], a space
/// that `required` charging has run a search no deploy pays for, which is a
/// bug; any other space searches for free.
pub(crate) fn charge(work: SearchWork, required: bool) -> Result<(), RSpaceError> {
    match CHARGER.try_with(|charger| charger(work)) {
        Ok(true) => Ok(()),
        Ok(false) => Err(RSpaceError::OutOfPhlogistons),
        Err(_) if required => Err(RSpaceError::BugFoundError(
            "a search ran outside the charging scope its space requires".to_string(),
        )),
        Err(_) => Ok(()),
    }
}

/// A receive's channel hashes in the order its channels are stored under.
/// Receives on the same channels in another order wait in another group.
type Group = Vec<Blake2b256Hash>;

/// The data waiting on one channel, by datum hash, with their count and
/// summed size.
#[derive(Default)]
struct Waiting {
    data: HashMap<Blake2b256Hash, usize>,
    count: usize,
    size: usize,
}

/// What replay saw a deploy's term make: each channel by hash, each receive's
/// channels in the order it named them with its patterns' sizes, and each
/// datum's size, by the receive's and the datum's hashes.
pub struct TermOperations<C> {
    pub channels: HashMap<Blake2b256Hash, C>,
    pub receives: HashMap<Blake2b256Hash, (Vec<C>, Vec<usize>)>,
    pub data_sizes: HashMap<Blake2b256Hash, usize>,
}

/// The waiting data and receives a deploy's term saw, rebuilt in log order.
struct Walk<'a, C, P, A, K> {
    store: &'a dyn HotStore<C, P, A, K>,
    channels: HashMap<Blake2b256Hash, C>,
    groups: HashMap<Blake2b256Hash, Group>,
    pattern_sizes: HashMap<Blake2b256Hash, Vec<usize>>,
    data_sizes: HashMap<Blake2b256Hash, usize>,
    data: HashMap<Blake2b256Hash, Waiting>,
    receives: HashMap<Group, HashMap<Blake2b256Hash, usize>>,
    joins: HashMap<Blake2b256Hash, HashSet<Group>>,
    latest: HashMap<Blake2b256Hash, usize>,
}

/// The work each charge for the searches of `log` paid for: one per receive
/// and one per join group a send searched, as play charges them. `log` is the
/// events one deploy's term logged, in the order play
/// performed them, `store` the state the term started from, and `term` what
/// replay saw the term make.
pub fn walk_work<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
    term: &TermOperations<C>,
    log: &[Event],
) -> Result<Vec<SearchWork>, RSpaceError>
where
    C: Clone + serde::Serialize + std::hash::Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    let mut walk = Walk {
        store,
        channels: term.channels.clone(),
        groups: term
            .receives
            .iter()
            .map(|(receive, (channels, _))| (receive.clone(), channels.iter().map(hash).collect()))
            .collect(),
        pattern_sizes: term
            .receives
            .iter()
            .map(|(receive, (_, sizes))| (receive.clone(), sizes.clone()))
            .collect(),
        data_sizes: term.data_sizes.clone(),
        data: HashMap::new(),
        receives: HashMap::new(),
        joins: HashMap::new(),
        latest: HashMap::new(),
    };
    let mut work = Vec::new();
    for (position, event) in log.iter().enumerate() {
        match event {
            Event::IoEvent(IOEvent::Produce(produce)) => {
                work.extend(walk.produce(produce)?);
                walk.latest.insert(produce.hash.clone(), position);
            }
            Event::IoEvent(IOEvent::Consume(consume)) => {
                work.push(walk.consume(consume)?);
                walk.latest.insert(consume.hash.clone(), position);
            }
            Event::Comm(comm) => walk.comm(comm)?,
        }
    }
    Ok(work)
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

    fn data_size(&self, datum: &Blake2b256Hash) -> Result<usize, RSpaceError> {
        self.data_sizes.get(datum).copied().ok_or_else(|| {
            mismatch(format!(
                "the log names datum {datum}, which replay never sent and no search found waiting"
            ))
        })
    }

    fn data_on(&mut self, channel_hash: &Blake2b256Hash) -> Result<&mut Waiting, RSpaceError> {
        if !self.data.contains_key(channel_hash) {
            let mut waiting = Waiting::default();
            for datum in self.store.get_data_arc(self.channel(channel_hash)?) {
                let size = *self
                    .data_sizes
                    .entry(datum.source.hash.clone())
                    .or_insert(datum.size);
                *waiting.data.entry(datum.source.hash.clone()).or_default() += 1;
                waiting.count += 1;
                waiting.size += size;
            }
            self.data.insert(channel_hash.clone(), waiting);
        }
        Ok(self.data.get_mut(channel_hash).expect("loaded above"))
    }

    /// The count and summed size of the data waiting on a channel.
    fn totals(&mut self, channel_hash: &Blake2b256Hash) -> Result<(usize, usize), RSpaceError> {
        let waiting = self.data_on(channel_hash)?;
        Ok((waiting.count, waiting.size))
    }

    /// The receives waiting in `group`. Loading them from the store records
    /// the group and pattern sizes of each.
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
                self.pattern_sizes
                    .entry(receive.source.hash.clone())
                    .or_insert_with(|| receive.pattern_sizes.clone());
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

    fn pattern_sizes_of(&self, receive: &Blake2b256Hash) -> Vec<usize> {
        self.pattern_sizes
            .get(receive)
            .cloned()
            .expect("a receive's pattern sizes are recorded with its group")
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

    fn produce(&mut self, produce: &Produce) -> Result<Vec<SearchWork>, RSpaceError> {
        let sent_size = self.data_size(&produce.hash)?;
        let groups: Vec<Group> = self
            .joins_on(&produce.channel_hash)?
            .iter()
            .cloned()
            .collect();
        let mut charges = Vec::with_capacity(groups.len());
        for group in groups {
            let mut work = SearchWork::default();
            let mut totals = Vec::with_capacity(group.len());
            for channel_hash in &group {
                let (count, size) = self.totals(channel_hash)?;
                totals.push(if *channel_hash == produce.channel_hash {
                    (count + 1, size + sent_size)
                } else {
                    (count, size)
                });
            }
            let receives: Vec<(Blake2b256Hash, usize)> = self
                .receives
                .get(&group)
                .expect("every join group is loaded with its receives")
                .iter()
                .map(|(receive, count)| (receive.clone(), *count))
                .collect();
            for (receive, count) in receives {
                for ((data, size), pattern_size) in
                    totals.iter().zip(self.pattern_sizes_of(&receive))
                {
                    work += SearchWork::of(*data, *size, pattern_size, count);
                }
            }
            charges.push(work);
        }
        let waiting = self.data_on(&produce.channel_hash)?;
        *waiting.data.entry(produce.hash.clone()).or_default() += 1;
        waiting.count += 1;
        waiting.size += sent_size;
        Ok(charges)
    }

    fn consume(&mut self, consume: &Consume) -> Result<SearchWork, RSpaceError> {
        let group = self.group_of(&consume.hash)?;
        let mut work = SearchWork::default();
        for (channel_hash, pattern_size) in group.iter().zip(self.pattern_sizes_of(&consume.hash)) {
            let (count, size) = self.totals(channel_hash)?;
            work += SearchWork::of(count, size, pattern_size, 1);
        }
        *self
            .receives_on(&group)?
            .entry(consume.hash.clone())
            .or_default() += 1;
        for channel_hash in &group {
            self.joins_on(channel_hash)?.insert(group.clone());
        }
        Ok(work)
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
                let size = self.data_size(&produce.hash)?;
                let waiting = self.data_on(&produce.channel_hash)?;
                take(&mut waiting.data, &produce.hash, "datum")?;
                waiting.count -= 1;
                waiting.size -= size;
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
