use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_request::DeployRequest;
use models::rust::deploy_window::{
    is_open, is_too_far_ahead, last_includable_block, too_far_ahead_message,
};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::error::{EngineStopped, IntakeError};
use crate::events::{NodeEvent, VerdictState};
use crate::node::{
    ChainSnapshot, NodeDeployStatus, NodeError, NodeHttp, ShardValues, SubmitOutcome,
};
use crate::pace::Pace;
use crate::record::{DeployView, QueueRecord, QueueState, Sig};
use crate::store::{QueueStore, StoreError};
use crate::window::ReleaseWindow;

/// Released deploys never exceed this many of the shard's full blocks.
const RELEASED_CEILING_BLOCKS: usize = 2;
const RETRY_FLOOR: Duration = Duration::from_secs(1);
const RETRY_CEILING: Duration = Duration::from_secs(30);
/// Retry-After for a refusal made before the queue has timed a height.
const UNTIMED_RETRY_SECS: u64 = 5;
const NODE_UNAVAILABLE_RETRY_SECS: u64 = 5;
const TRANSITION_BUFFER: usize = 4096;
pub const COMMAND_BUFFER: usize = 1024;
/// Commands handled before their changes are committed together.
const COMMIT_BATCH: usize = 256;

const ALL_STATES: [QueueState; 7] = [
    QueueState::Queued,
    QueueState::Submitted,
    QueueState::InBlock,
    QueueState::Finalized,
    QueueState::Failed,
    QueueState::Expired,
    QueueState::Rejected,
];

/// One deploy's new state, as watchers receive it.
#[derive(Debug, Clone)]
pub struct Transition {
    pub sig: Sig,
    pub view: DeployView,
}

/// The state of each watched deploy at subscription, and every transition
/// after it. Subscribing inside the engine leaves no gap between the two.
pub struct WatchStart {
    pub current: Vec<(Sig, Option<DeployView>)>,
    pub updates: broadcast::Receiver<Transition>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueueSnapshot {
    pub connected: bool,
    pub tip: Option<i64>,
    pub last_finalized: Option<i64>,
    pub release_open: bool,
    pub ready: usize,
    pub waiting: usize,
    pub closed: usize,
    pub submitted: usize,
    pub in_block: usize,
    pub released_limit: usize,
    pub release_window: Option<f64>,
    pub release_cuts: Option<u64>,
    pub late_share: Option<f64>,
    pub settle_delay: Option<i64>,
    pub inclusion_delay: Option<i64>,
    pub release_rate: Option<f64>,
    pub held: usize,
    pub settled_per_height: Option<f64>,
    pub secs_per_height: Option<f64>,
}

pub enum Command {
    Intake {
        deploy: Signed<DeployData>,
        reply: oneshot::Sender<Result<Sig, IntakeError>>,
    },
    Status {
        sig: Sig,
        reply: oneshot::Sender<Option<DeployView>>,
    },
    Watch {
        sigs: Vec<Sig>,
        reply: oneshot::Sender<WatchStart>,
    },
    Snapshot {
        reply: oneshot::Sender<QueueSnapshot>,
    },
    Connected(ChainSnapshot),
    Disconnected,
    Node(NodeEvent),
    Submitted {
        sig: Sig,
        outcome: SubmitOutcome,
    },
    Resynced {
        sig: Sig,
        result: Result<NodeDeployStatus, NodeError>,
    },
    ClosedChecked {
        sig: Sig,
        result: Result<NodeDeployStatus, NodeError>,
    },
}

#[derive(Clone)]
pub struct EngineHandle {
    commands: mpsc::Sender<Command>,
}

impl EngineHandle {
    pub fn new(commands: mpsc::Sender<Command>) -> Self { Self { commands } }

    async fn ask<T>(
        &self,
        command: impl FnOnce(oneshot::Sender<T>) -> Command,
    ) -> Result<T, EngineStopped> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(command(reply))
            .await
            .map_err(|_| EngineStopped)?;
        answer.await.map_err(|_| EngineStopped)
    }

    pub async fn intake(&self, deploy: Signed<DeployData>) -> Result<Sig, IntakeError> {
        self.ask(|reply| Command::Intake { deploy, reply })
            .await
            .map_err(|_| IntakeError::Stopped)?
    }

    pub async fn status(&self, sig: Sig) -> Result<Option<DeployView>, EngineStopped> {
        self.ask(|reply| Command::Status { sig, reply }).await
    }

    pub async fn watch(&self, sigs: Vec<Sig>) -> Result<WatchStart, EngineStopped> {
        self.ask(|reply| Command::Watch { sigs, reply }).await
    }

    pub async fn snapshot(&self) -> Result<QueueSnapshot, EngineStopped> {
        self.ask(|reply| Command::Snapshot { reply }).await
    }
}

/// Where a `Pending` answer from the node leaves a deploy that names no
/// block.
#[derive(Clone, Copy, PartialEq, Eq)]
enum UnplacedPending {
    /// The deploy may still be in the node's pool: keep waiting.
    KeepWaiting,
    /// The deploy's window closed before the queue released it, and no
    /// block names it: it can never land.
    Expire,
}

/// Where a record sits in the engine's indexes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct IndexKeys {
    released: Option<(i64, u64)>,
    resend: Option<(i64, u64)>,
    decided: Option<(i64, u64)>,
    holder: Option<Vec<u8>>,
}

impl IndexKeys {
    fn of(record: &QueueRecord) -> Self {
        let sequence = record.sequence;
        let state = record.state;
        Self {
            released: matches!(state, QueueState::Submitted | QueueState::InBlock)
                .then_some(record.submitted_at_height)
                .flatten()
                .map(|height| (height, sequence)),
            resend: (state == QueueState::Submitted)
                .then_some(record.last_sent_height)
                .flatten()
                .map(|height| (height, sequence)),
            decided: state
                .is_terminal()
                .then_some(record.decided_at_height)
                .flatten()
                .map(|height| (height, sequence)),
            holder: (!state.is_terminal()).then(|| record.deployer.clone()),
        }
    }
}

type Effect = Box<dyn FnOnce() + Send>;
type IntakeResult = Result<Sig, IntakeError>;

/// The single owner of the queue's state. Changes are committed to the
/// store in batches; nothing is acknowledged, broadcast or sent to the node
/// before the change behind it is committed.
pub struct Engine {
    store: QueueStore,
    node: NodeHttp,
    commands: mpsc::Sender<Command>,
    records: HashMap<Sig, QueueRecord>,
    counts: HashMap<QueueState, usize>,
    /// Queued deploys whose window is open, by (last includable block,
    /// sequence): released earliest deadline first.
    ready: BTreeMap<(i64, u64), Sig>,
    /// Queued deploys not yet valid, by (valid after, sequence).
    waiting: BTreeMap<(i64, u64), Sig>,
    /// Every placed queued deploy, by (last includable block, sequence).
    expiry: BTreeMap<(i64, u64), Sig>,
    /// Queued deploys whose window closed before release, by (height past
    /// which no block can include them, sequence).
    closed: BTreeMap<(i64, u64), Sig>,
    closed_checking: HashSet<Sig>,
    /// Released deploys, by (release height, sequence).
    released_index: BTreeMap<(i64, u64), Sig>,
    /// Submitted deploys, by (height last sent, sequence).
    resend_index: BTreeMap<(i64, u64), Sig>,
    /// Terminal deploys, by (height decided, sequence).
    decided_index: BTreeMap<(i64, u64), Sig>,
    /// Deploys each deployer holds that have no verdict yet.
    held_by_deployer: HashMap<Vec<u8>, usize>,
    chain: Option<ChainSnapshot>,
    connected: bool,
    pace: Option<Pace>,
    window: Option<ReleaseWindow>,
    next_sequence: u64,
    retry_at: Option<Instant>,
    retry_delay: Duration,
    transitions: broadcast::Sender<Transition>,
    /// Records changed since the last commit.
    dirty: HashSet<Sig>,
    /// Records deleted since the last commit.
    removed: Vec<Sig>,
    /// Intake answers waiting for the commit of what they acknowledge.
    intake_replies: Vec<(oneshot::Sender<IntakeResult>, IntakeResult)>,
    /// Broadcasts, replies and node requests waiting for the commit.
    effects: Vec<Effect>,
}

impl Engine {
    pub fn new(
        store: QueueStore,
        node: NodeHttp,
        commands: mpsc::Sender<Command>,
    ) -> Result<Self, StoreError> {
        let records: HashMap<Sig, QueueRecord> = store.load()?.into_iter().collect();
        let mut counts: HashMap<QueueState, usize> =
            ALL_STATES.iter().map(|state| (*state, 0)).collect();
        for record in records.values() {
            *counts
                .get_mut(&record.state)
                .expect("counts hold every state") += 1;
        }
        // An empty store starts the sequence at zero.
        let next_sequence = records
            .values()
            .map(|record| record.sequence + 1)
            .max()
            .unwrap_or(0);
        let (transitions, _) = broadcast::channel(TRANSITION_BUFFER);
        tracing::info!(
            target: "deploy_queue::engine",
            held = records.len(),
            queued = counts[&QueueState::Queued],
            submitted = counts[&QueueState::Submitted],
            in_block = counts[&QueueState::InBlock],
            "deploy queue loaded its store"
        );
        let mut engine = Self {
            store,
            node,
            commands,
            records: HashMap::new(),
            counts,
            ready: BTreeMap::new(),
            waiting: BTreeMap::new(),
            expiry: BTreeMap::new(),
            closed: BTreeMap::new(),
            closed_checking: HashSet::new(),
            released_index: BTreeMap::new(),
            resend_index: BTreeMap::new(),
            decided_index: BTreeMap::new(),
            held_by_deployer: HashMap::new(),
            chain: None,
            connected: false,
            pace: None,
            window: None,
            next_sequence,
            retry_at: None,
            retry_delay: RETRY_FLOOR,
            transitions,
            dirty: HashSet::new(),
            removed: Vec::new(),
            intake_replies: Vec::new(),
            effects: Vec::new(),
        };
        for (sig, record) in records {
            engine.reindex(&sig, None, Some(IndexKeys::of(&record)));
            engine.records.insert(sig, record);
        }
        Ok(engine)
    }

    /// Runs until the command channel closes. Each pass handles the commands
    /// already waiting, up to a batch, and commits their changes in one
    /// transaction. A store failure stops the engine: the queue's contract
    /// is that what it acknowledged is durable.
    pub async fn run(mut self, mut commands: mpsc::Receiver<Command>) -> Result<(), StoreError> {
        loop {
            let retry_at = self.retry_at;
            tokio::select! {
                command = commands.recv() => match command {
                    Some(command) => {
                        self.handle(command)?;
                        for _ in 1..COMMIT_BATCH {
                            let Ok(command) = commands.try_recv() else {
                                break;
                            };
                            self.handle(command)?;
                        }
                    }
                    None => return self.commit(),
                },
                _ = wait_until(retry_at) => {
                    self.retry_at = None;
                    self.release()?;
                }
            }
            self.commit()?;
            self.publish_gauges();
        }
    }

    /// Commits every change since the last commit, then answers, broadcasts
    /// and sends what waited for it.
    fn commit(&mut self) -> Result<(), StoreError> {
        if !self.dirty.is_empty() || !self.removed.is_empty() {
            let puts: Vec<(&Sig, &QueueRecord)> = self
                .dirty
                .iter()
                .map(|sig| {
                    self.records
                        .get_key_value(sig)
                        .expect("a dirty record is held; a deleted one leaves the dirty set")
                })
                .collect();
            if let Err(e) = self.store.commit(&puts, &self.removed) {
                for (reply, _) in self.intake_replies.drain(..) {
                    let _ = reply.send(Err(IntakeError::Store(e.to_string())));
                }
                return Err(e);
            }
            self.dirty.clear();
            self.removed.clear();
        }
        for (reply, result) in self.intake_replies.drain(..) {
            // A client that hung up does not undo an accepted deploy.
            let _ = reply.send(result);
        }
        for effect in self.effects.drain(..) {
            effect();
        }
        Ok(())
    }

    fn after_commit(&mut self, effect: impl FnOnce() + Send + 'static) {
        self.effects.push(Box::new(effect));
    }

    fn handle(&mut self, command: Command) -> Result<(), StoreError> {
        match command {
            Command::Intake { deploy, reply } => {
                let result = self.intake(deploy)?;
                match &result {
                    Ok(_) => metrics::counter!("deploy_queue.accepted").increment(1),
                    Err(refusal) => {
                        metrics::counter!("deploy_queue.refused", "reason" => refusal.kind())
                            .increment(1)
                    }
                }
                self.intake_replies.push((reply, result));
            }
            Command::Status { sig, reply } => {
                let view = self.records.get(&sig).map(QueueRecord::view);
                self.after_commit(move || {
                    let _ = reply.send(view);
                });
            }
            Command::Watch { sigs, reply } => {
                let current = sigs
                    .into_iter()
                    .map(|sig| {
                        let view = self.records.get(&sig).map(QueueRecord::view);
                        (sig, view)
                    })
                    .collect();
                let updates = self.transitions.subscribe();
                self.after_commit(move || {
                    let _ = reply.send(WatchStart { current, updates });
                });
            }
            Command::Snapshot { reply } => {
                let snapshot = self.snapshot();
                self.after_commit(move || {
                    let _ = reply.send(snapshot);
                });
            }
            Command::Connected(chain) => self.on_connected(chain)?,
            Command::Disconnected => {
                self.connected = false;
                tracing::warn!(target: "deploy_queue::engine", "lost the node; release paused");
            }
            Command::Node(event) => self.on_event(event)?,
            Command::Submitted { sig, outcome } => self.on_submitted(sig, outcome)?,
            Command::Resynced { sig, result } => self.on_resynced(sig, result)?,
            Command::ClosedChecked { sig, result } => self.on_closed_checked(sig, result),
        }
        Ok(())
    }

    fn intake(
        &mut self,
        deploy: Signed<DeployData>,
    ) -> Result<Result<Sig, IntakeError>, StoreError> {
        let chain = match (&self.chain, self.connected) {
            (Some(chain), true) => chain.clone(),
            _ => {
                return Ok(Err(IntakeError::NodeUnavailable {
                    retry_after_secs: NODE_UNAVAILABLE_RETRY_SECS,
                }))
            }
        };
        let shard = &chain.shard;
        let sig = deploy.sig.to_vec();
        if self.records.contains_key(&sig) {
            return Ok(Err(IntakeError::Duplicate(sig)));
        }
        if deploy.data.shard_id != shard.shard_id {
            return Ok(Err(IntakeError::IllegalArgument(format!(
                "Deploy shardId '{}' is not as expected network shard '{}'.",
                deploy.data.shard_id, shard.shard_id
            ))));
        }
        if deploy.data.phlo_price < shard.min_phlo_price {
            return Ok(Err(IntakeError::IllegalArgument(format!(
                "Phlo price {} is less than minimum price {}.",
                deploy.data.phlo_price, shard.min_phlo_price
            ))));
        }
        let now_ms = now_millis();
        if deploy.data.is_expired_at(now_ms) {
            return Ok(Err(IntakeError::DeployExpired(format!(
                "Deploy has expired: expirationTimestamp={:?} is in the past.",
                deploy.data.expiration_timestamp
            ))));
        }
        let valid_after = deploy.data.valid_after_block_number;
        let next_block = chain.tip + 1;
        if !window_open(valid_after, next_block, shard.deploy_lifespan) {
            return Ok(Err(IntakeError::DeployExpired(format!(
                "Deploy validAfterBlockNumber {} has expired at block {} with deploy lifespan {}.",
                valid_after, next_block, shard.deploy_lifespan
            ))));
        }
        if is_too_far_ahead(valid_after, chain.tip, shard.max_parent_depth) {
            return Ok(Err(IntakeError::IllegalArgument(too_far_ahead_message(
                valid_after,
                chain.tip,
                shard.max_parent_depth,
            ))));
        }

        // The rate is the slower of what the shard has been settling and what
        // the release window now lets through, so a window just cut tightens
        // intake at once. The depth covers a pace that slows after a deploy
        // was accepted.
        let pace = self.pace.as_ref().expect("a connected engine has a pace");
        let rate = match self.window.as_ref().and_then(ReleaseWindow::release_rate) {
            Some(releasable) => pace.settled_per_height().min(releasable),
            None => pace.settled_per_height(),
        };
        let depth = citable_depth(shard);
        let fresh_span = last_includable(chain.tip, shard.deploy_lifespan) - chain.tip;

        let deployer = deploy.pk.bytes.to_vec();
        let (held, others) = match self.held_by_deployer.get(&deployer) {
            Some(&held) => (held, self.held_by_deployer.len() - 1),
            None => (0, self.held_by_deployer.len()),
        };
        let limit = deployer_limit(capacity(rate, fresh_span, depth), others);
        if held >= limit {
            let excess = (held + 1 - limit) * (others + 1);
            let retry_after_secs = heights_to_settle(excess, rate)
                .and_then(|heights| pace.seconds_for(heights))
                .unwrap_or(UNTIMED_RETRY_SECS);
            return Ok(Err(IntakeError::DeployerShareFull {
                held,
                limit,
                retry_after_secs,
            }));
        }

        let last = last_includable(valid_after, shard.deploy_lifespan);
        let key = (last, self.next_sequence);
        let slot = |record: &QueueRecord, last: i64| Slot {
            releasable_from: record.valid_after.max(chain.tip),
            last,
        };
        let admitted = admits(
            self.released(),
            self.expiry.range(..key).count(),
            Slot {
                releasable_from: valid_after.max(chain.tip),
                last,
            },
            self.expiry
                .range(key..)
                .map(|((last, _), sig)| slot(&self.records[sig], *last)),
            rate,
            depth,
        );
        if !admitted {
            // A deploy signed again now has the latest deadline of all, so
            // it waits behind every deploy held.
            let ahead = self.released() + self.expiry.len();
            let retry_after_secs = heights_to_settle(ahead + 1, rate)
                .and_then(|heights| {
                    pace.seconds_for(heights_until_a_fresh_window_fits(
                        heights, depth, fresh_span,
                    ))
                })
                .unwrap_or(UNTIMED_RETRY_SECS);
            return Ok(Err(IntakeError::QueueFull { retry_after_secs }));
        }

        let record = QueueRecord::new(&deploy, self.next_sequence, now_ms);
        self.next_sequence += 1;
        self.enter(QueueState::Queued);
        self.reindex(&sig, None, Some(IndexKeys::of(&record)));
        self.records.insert(sig.clone(), record);
        self.dirty.insert(sig.clone());
        self.place(&sig);
        self.emit(&sig);
        self.release()?;
        Ok(Ok(sig))
    }

    fn on_connected(&mut self, mut chain: ChainSnapshot) -> Result<(), StoreError> {
        if let Some(previous) = &self.chain {
            chain.tip = chain.tip.max(previous.tip);
            chain.lfb = chain.lfb.max(previous.lfb);
        }
        tracing::info!(
            target: "deploy_queue::engine",
            tip = chain.tip,
            lfb = chain.lfb,
            shard = %chain.shard.shard_id,
            deploy_lifespan = chain.shard.deploy_lifespan,
            max_user_deploys_per_block = chain.shard.max_user_deploys_per_block,
            max_parent_depth = chain.shard.max_parent_depth,
            "connected to the node"
        );
        if self.pace.is_none() {
            self.pace = Some(Pace::new(citable_depth(&chain.shard)));
        }
        if self.window.is_none() {
            let mut window = ReleaseWindow::new(
                citable_depth(&chain.shard),
                RELEASED_CEILING_BLOCKS * chain.shard.max_user_deploys_per_block as usize,
            );
            window.on_tip(chain.tip, None);
            self.window = Some(window);
        }
        self.chain = Some(chain);
        self.connected = true;
        self.rebuild_placement();
        self.resync();
        self.release()
    }

    fn on_event(&mut self, event: NodeEvent) -> Result<(), StoreError> {
        match event {
            NodeEvent::Block {
                number,
                hash,
                deploys,
            } => {
                let block_hash = decode_hex("block hash", &hash);
                for id in deploys {
                    let Some(sig) = decode_hex("deploy id", &id) else {
                        continue;
                    };
                    let Some(state) = self.records.get(&sig).map(|record| record.state) else {
                        continue;
                    };
                    if state.is_terminal() {
                        continue;
                    }
                    if state == QueueState::Queued {
                        self.unplace(&sig);
                    }
                    if state == QueueState::Submitted {
                        let released_at = self.records[&sig].submitted_at_height;
                        if let (Some(released_at), Some(window)) =
                            (released_at, self.window.as_mut())
                        {
                            window.on_inclusion(number - released_at);
                        }
                    }
                    let hash = block_hash.clone();
                    self.update(&sig, |record| {
                        record.state = QueueState::InBlock;
                        if hash.is_some() {
                            record.latest_block_hash = hash;
                        }
                    });
                }
                self.observe_tip(number)?;
            }
            NodeEvent::Finalised { number } => self.observe_lfb(number),
            NodeEvent::Verdicts(verdicts) => {
                // Before these verdicts settle any released deploy.
                if self.released() > 0 {
                    if let Some(pace) = self.pace.as_mut() {
                        pace.record_unsettled();
                    }
                }
                for verdict in verdicts {
                    let Some(sig) = decode_hex("deploy id", &verdict.deploy_id) else {
                        continue;
                    };
                    let held_open = self
                        .records
                        .get(&sig)
                        .is_some_and(|record| !record.state.is_terminal());
                    if !held_open {
                        continue;
                    }
                    let state = match verdict.state {
                        VerdictState::Finalized => QueueState::Finalized,
                        VerdictState::Expired => QueueState::Expired,
                        VerdictState::Failed => QueueState::Failed,
                    };
                    let block_hash = verdict
                        .block_hash
                        .and_then(|hash| decode_hex("block hash", &hash));
                    self.decide(
                        &sig,
                        state,
                        Some((verdict.rejection_count, block_hash)),
                        None,
                    );
                }
            }
            NodeEvent::Lagged { skipped } => {
                metrics::counter!("deploy_queue.node_events_lost").increment(skipped);
                tracing::warn!(
                    target: "deploy_queue::engine",
                    skipped,
                    "the node dropped events for this queue; resynchronizing in-flight deploys"
                );
                self.resync();
            }
            NodeEvent::Running | NodeEvent::Ignored => {}
        }
        self.release()
    }

    fn on_submitted(&mut self, sig: Sig, outcome: SubmitOutcome) -> Result<(), StoreError> {
        let submitted = self
            .records
            .get(&sig)
            .is_some_and(|record| record.state == QueueState::Submitted);
        if !submitted {
            return Ok(());
        }
        match outcome {
            SubmitOutcome::Accepted => {
                self.retry_delay = RETRY_FLOOR;
            }
            SubmitOutcome::Duplicate => {
                self.retry_delay = RETRY_FLOOR;
                // The node may have decided this deploy before the queue took
                // it (a client resending one the queue already pruned); its
                // verdict event is long past, so read the verdict instead.
                self.spawn_status_read(sig.clone());
            }
            SubmitOutcome::Expired(message) => {
                self.decide(&sig, QueueState::Expired, None, Some(message));
            }
            SubmitOutcome::Rejected { kind, message } => {
                tracing::warn!(
                    target: "deploy_queue::engine",
                    sig = %hex::encode(&sig),
                    kind = %kind,
                    message = %message,
                    "the node refused a deploy the queue accepted"
                );
                self.decide(
                    &sig,
                    QueueState::Rejected,
                    None,
                    Some(format!("{}: {}", kind, message)),
                );
            }
            SubmitOutcome::Retry {
                reason,
                retry_after,
            } => {
                metrics::counter!("deploy_queue.node_retries").increment(1);
                let delay = match retry_after {
                    Some(asked) => asked.max(self.retry_delay),
                    None => self.retry_delay,
                };
                tracing::warn!(
                    target: "deploy_queue::engine",
                    sig = %hex::encode(&sig),
                    reason = %reason,
                    delay_ms = delay.as_millis() as u64,
                    "the node could not take a deploy; release paused"
                );
                self.update(&sig, |record| {
                    record.state = QueueState::Queued;
                    record.submitted_at_height = None;
                    record.last_sent_height = None;
                });
                self.place(&sig);
                self.retry_at = Some(Instant::now() + delay);
                self.retry_delay = (self.retry_delay * 2).min(RETRY_CEILING);
            }
        }
        self.release()
    }

    fn on_resynced(
        &mut self,
        sig: Sig,
        result: Result<NodeDeployStatus, NodeError>,
    ) -> Result<(), StoreError> {
        let in_flight = self.records.get(&sig).is_some_and(|record| {
            matches!(record.state, QueueState::Submitted | QueueState::InBlock)
        });
        if !in_flight {
            return Ok(());
        }
        match result {
            Ok(status) => self.adopt(&sig, status, UnplacedPending::KeepWaiting),
            Err(e) => tracing::warn!(
                target: "deploy_queue::engine",
                sig = %hex::encode(&sig),
                error = %e,
                "resync read failed; the next reconnect or lag retries it"
            ),
        }
        self.release()
    }

    fn on_closed_checked(&mut self, sig: Sig, result: Result<NodeDeployStatus, NodeError>) {
        self.closed_checking.remove(&sig);
        let queued = self
            .records
            .get(&sig)
            .is_some_and(|record| record.state == QueueState::Queued);
        if !queued {
            return;
        }
        match result {
            Ok(status) => self.adopt(&sig, status, UnplacedPending::Expire),
            Err(e) => tracing::warn!(
                target: "deploy_queue::engine",
                sig = %hex::encode(&sig),
                error = %e,
                "closed-window check failed; the next finalization retries it"
            ),
        }
    }

    /// Takes the node's answer for a deploy the queue has lost track of.
    fn adopt(&mut self, sig: &Sig, status: NodeDeployStatus, unplaced_pending: UnplacedPending) {
        let block_hash = status
            .latest_block_hash
            .as_deref()
            .and_then(|hash| decode_hex("block hash", hash));
        let node = Some((status.rejection_count, block_hash.clone()));
        match status.state.as_str() {
            "Finalized" => self.decide(sig, QueueState::Finalized, node, None),
            "Failed" => self.decide(sig, QueueState::Failed, node, None),
            "Expired" => self.decide(sig, QueueState::Expired, node, None),
            "Pending" => match block_hash {
                Some(hash) => {
                    if self.records[sig].state == QueueState::Queued {
                        self.unplace(sig);
                    }
                    self.update(sig, |record| {
                        record.state = QueueState::InBlock;
                        record.latest_block_hash = Some(hash);
                    })
                }
                None if unplaced_pending == UnplacedPending::Expire => self.decide(
                    sig,
                    QueueState::Expired,
                    None,
                    Some("validity window closed before the queue released the deploy".into()),
                ),
                None => {}
            },
            other => {
                tracing::error!(
                    target: "deploy_queue::engine",
                    sig = %hex::encode(sig),
                    state = %other,
                    "the node answered a deploy state the queue does not know"
                );
            }
        }
    }

    /// Releases deploys to the node while the shard is finalizing and the
    /// node's pool holds less than it can include soon.
    fn release(&mut self) -> Result<(), StoreError> {
        let Some(chain) = self.chain.clone() else {
            return Ok(());
        };
        if !self.release_open() {
            return Ok(());
        }
        let shard = &chain.shard;
        let limit = self.released_limit();
        while self.released() < limit {
            let Some((_, sig)) = self.ready.pop_first() else {
                break;
            };
            // A deploy with less window left than the shard takes to include
            // one would only expire in the node's pool.
            let valid_after = self.records[&sig].valid_after;
            let headroom = self
                .window
                .as_ref()
                .map_or(1, ReleaseWindow::release_headroom);
            if last_includable(valid_after, shard.deploy_lifespan) - chain.tip < headroom {
                self.close(&sig);
                continue;
            }
            self.submit(&sig, chain.tip)?;
        }
        if !self.ready.is_empty() {
            if let Some(pace) = self.pace.as_mut() {
                pace.record_held_back();
            }
            if let Some(window) = self.window.as_mut() {
                window.record_held_back();
            }
        }
        Ok(())
    }

    /// Released deploys still waiting for a verdict: in the node's pool or in
    /// a block.
    fn released(&self) -> usize {
        self.count(QueueState::Submitted) + self.count(QueueState::InBlock)
    }

    fn released_limit(&self) -> usize { self.window.as_ref().map_or(0, ReleaseWindow::limit) }

    /// Release height of the longest-waiting released deploy.
    fn oldest_unsettled_release(&self) -> Option<i64> {
        self.released_index.keys().next().map(|(height, _)| *height)
    }

    fn release_open(&self) -> bool {
        let Some(chain) = &self.chain else {
            return false;
        };
        self.connected
            && self.retry_at.is_none_or(|at| at <= Instant::now())
            && chain.tip - chain.lfb <= i64::from(chain.shard.max_parent_depth)
    }

    fn submit(&mut self, sig: &Sig, tip: i64) -> Result<(), StoreError> {
        let request = self.request_for(sig)?;
        self.remove_expiry(sig);
        self.update(sig, |record| {
            record.state = QueueState::Submitted;
            record.submitted_at_height = Some(tip);
            record.last_sent_height = Some(tip);
            record.submissions += 1;
        });
        self.spawn_submit(sig.clone(), request);
        Ok(())
    }

    fn request_for(&self, sig: &Sig) -> Result<DeployRequest, StoreError> {
        let deploy = self.records[sig]
            .signed_deploy()
            .map_err(|reason| StoreError::Corrupt {
                sig: hex::encode(sig),
                reason,
            })?;
        Ok(DeployRequest::from_signed(&deploy))
    }

    fn spawn_submit(&mut self, sig: Sig, request: DeployRequest) {
        let node = self.node.clone();
        let commands = self.commands.clone();
        self.after_commit(move || {
            tokio::spawn(async move {
                let outcome = node.submit(&request).await;
                // The engine stopping ends the process; the outcome has no reader.
                let _ = commands.send(Command::Submitted { sig, outcome }).await;
            });
        });
    }

    fn observe_tip(&mut self, number: i64) -> Result<(), StoreError> {
        let Some(chain) = self.chain.as_mut() else {
            return Ok(());
        };
        if number <= chain.tip {
            return Ok(());
        }
        chain.tip = number;
        let unsettled = self.released() > 0;
        if let Some(pace) = self.pace.as_mut() {
            if unsettled {
                pace.record_unsettled();
            }
            pace.on_tip(number, Instant::now());
        }
        let oldest_unsettled_release = self.oldest_unsettled_release();
        if let Some(window) = self.window.as_mut() {
            if let Some(cut) = window.on_tip(number, oldest_unsettled_release) {
                metrics::counter!("deploy_queue.release_cuts", "reason" => cut.name()).increment(1);
                tracing::info!(
                    target: "deploy_queue::engine",
                    tip = number,
                    reason = cut.name(),
                    window = window.window(),
                    "release window halved"
                );
            }
        }
        let became_valid = self.waiting.split_off(&(number + 1, 0));
        let valid_now = std::mem::replace(&mut self.waiting, became_valid);
        let lifespan = self
            .chain
            .as_ref()
            .map(|chain| chain.shard.deploy_lifespan)
            .expect("the tip moved on a known chain");
        for ((valid_after, sequence), sig) in valid_now {
            self.ready
                .insert((last_includable(valid_after, lifespan), sequence), sig);
        }
        self.close_expired(number);
        self.resubmit_stale(number)?;
        self.prune(number)
    }

    fn observe_lfb(&mut self, number: i64) {
        let Some(chain) = self.chain.as_mut() else {
            return;
        };
        if number <= chain.lfb {
            return;
        }
        chain.lfb = number;
        let due: Vec<Sig> = self
            .closed
            .range(..(number, 0))
            .map(|(_, sig)| sig.clone())
            .filter(|sig| !self.closed_checking.contains(sig))
            .collect();
        for sig in due {
            self.closed_checking.insert(sig.clone());
            let node = self.node.clone();
            let commands = self.commands.clone();
            self.after_commit(move || {
                tokio::spawn(async move {
                    let result = node.deploy_status(&sig).await;
                    let _ = commands.send(Command::ClosedChecked { sig, result }).await;
                });
            });
        }
    }

    /// Asks the node about every deploy it may hold, after a gap in the
    /// event stream (a reconnect or a lag).
    fn resync(&mut self) {
        let in_flight: Vec<Sig> = self.released_index.values().cloned().collect();
        for sig in in_flight {
            self.spawn_status_read(sig);
        }
    }

    fn spawn_status_read(&mut self, sig: Sig) {
        let node = self.node.clone();
        let commands = self.commands.clone();
        self.after_commit(move || {
            tokio::spawn(async move {
                let result = node.deploy_status(&sig).await;
                let _ = commands.send(Command::Resynced { sig, result }).await;
            });
        });
    }

    /// Resends a deploy no block has carried within the citable depth since
    /// it was sent. The node answers a duplicate when it still holds it.
    fn resubmit_stale(&mut self, tip: i64) -> Result<(), StoreError> {
        let Some(depth) = self
            .chain
            .as_ref()
            .map(|chain| i64::from(chain.shard.max_parent_depth))
        else {
            return Ok(());
        };
        // The release height stays put: it is what a verdict's settle delay
        // is measured from.
        let stale: Vec<Sig> = self
            .resend_index
            .range(..(tip - depth, 0))
            .map(|(_, sig)| sig.clone())
            .collect();
        for sig in stale {
            let request = self.request_for(&sig)?;
            self.update(&sig, |record| {
                record.last_sent_height = Some(tip);
                record.submissions += 1;
            });
            metrics::counter!("deploy_queue.resubmissions").increment(1);
            self.spawn_submit(sig, request);
        }
        Ok(())
    }

    /// Drops terminal records one lifespan after their verdict. The node
    /// keeps its own verdicts, and the queue forwards a status read for a
    /// deploy it no longer holds.
    fn prune(&mut self, tip: i64) -> Result<(), StoreError> {
        let Some(lifespan) = self.chain.as_ref().map(|chain| chain.shard.deploy_lifespan) else {
            return Ok(());
        };
        let done: Vec<Sig> = self
            .decided_index
            .range(..(tip - lifespan, 0))
            .map(|(_, sig)| sig.clone())
            .collect();
        for sig in done {
            let record = self
                .records
                .remove(&sig)
                .expect("the decided index names held records");
            self.reindex(&sig, Some(IndexKeys::of(&record)), None);
            self.discount(record.state);
            self.dirty.remove(&sig);
            self.removed.push(sig);
        }
        Ok(())
    }

    fn rebuild_placement(&mut self) {
        self.ready.clear();
        self.waiting.clear();
        self.expiry.clear();
        self.closed.clear();
        self.closed_checking.clear();
        let mut queued: Vec<(u64, Sig)> = self
            .records
            .iter()
            .filter(|(_, record)| record.state == QueueState::Queued)
            .map(|(sig, record)| (record.sequence, sig.clone()))
            .collect();
        queued.sort();
        for (_, sig) in queued {
            self.place(&sig);
        }
        if let Some(tip) = self.chain.as_ref().map(|chain| chain.tip) {
            self.close_expired(tip);
        }
    }

    /// Files a queued deploy: ready if its window has opened, waiting if not.
    fn place(&mut self, sig: &Sig) {
        let chain = self
            .chain
            .as_ref()
            .expect("deploys are placed once the chain is known");
        let record = &self.records[sig];
        let sequence = record.sequence;
        let last = last_includable(record.valid_after, chain.shard.deploy_lifespan);
        self.expiry.insert((last, sequence), sig.clone());
        if record.valid_after > chain.tip {
            self.waiting
                .insert((record.valid_after, sequence), sig.clone());
        } else {
            self.ready.insert((last, sequence), sig.clone());
        }
    }

    /// Removes a queued deploy from every placement index.
    fn unplace(&mut self, sig: &Sig) {
        let record = &self.records[sig];
        let (valid_after, sequence) = (record.valid_after, record.sequence);
        let lifespan = self
            .chain
            .as_ref()
            .map(|chain| chain.shard.deploy_lifespan)
            .expect("deploys are placed once the chain is known");
        self.ready
            .remove(&(last_includable(valid_after, lifespan), sequence));
        self.waiting.remove(&(valid_after, sequence));
        self.remove_expiry(sig);
        self.closed.retain(|_, held| held != sig);
        self.closed_checking.remove(sig);
    }

    fn remove_expiry(&mut self, sig: &Sig) {
        let Some(chain) = &self.chain else {
            return;
        };
        let record = &self.records[sig];
        let last = last_includable(record.valid_after, chain.shard.deploy_lifespan);
        self.expiry.remove(&(last, record.sequence));
    }

    /// Moves every queued deploy whose window has closed at `tip` out of
    /// release. Its verdict waits until no block can carry it.
    fn close_expired(&mut self, tip: i64) {
        let open = self.expiry.split_off(&(tip + 1, 0));
        let expired = std::mem::replace(&mut self.expiry, open);
        for sig in expired.into_values() {
            self.close(&sig);
        }
    }

    fn close(&mut self, sig: &Sig) {
        let chain = self
            .chain
            .as_ref()
            .expect("deploys close once the chain is known");
        let depth = i64::from(chain.shard.max_parent_depth);
        let record = &self.records[sig];
        let (valid_after, sequence) = (record.valid_after, record.sequence);
        let last = last_includable(valid_after, chain.shard.deploy_lifespan);
        self.ready.remove(&(last, sequence));
        self.waiting.remove(&(valid_after, sequence));
        self.expiry.remove(&(last, sequence));
        self.closed.insert((last + depth, sequence), sig.clone());
        metrics::counter!("deploy_queue.closed_in_queue").increment(1);
    }

    /// A terminal state. `node` carries the node's rejection count and block
    /// when the node decided it.
    fn decide(
        &mut self,
        sig: &Sig,
        state: QueueState,
        node: Option<(u32, Option<Vec<u8>>)>,
        detail: Option<String>,
    ) {
        let tip = self
            .chain
            .as_ref()
            .map(|chain| chain.tip)
            .expect("verdicts arrive once the chain is known");
        let before = self.records[sig].state;
        if before == QueueState::Queued {
            self.unplace(sig);
        }
        if matches!(before, QueueState::Submitted | QueueState::InBlock) {
            if let Some(pace) = self.pace.as_mut() {
                pace.record_settled(1);
            }
            // An expiry measures the deploy's window, not how long the shard
            // took to settle it.
            let released_at = self.records[sig].submitted_at_height;
            if let (true, Some(released_at), Some(window)) = (
                matches!(state, QueueState::Finalized | QueueState::Failed),
                released_at,
                self.window.as_mut(),
            ) {
                let lost_merges = node.as_ref().is_some_and(|(rejections, _)| *rejections > 0);
                if lost_merges {
                    window.on_lost_merge_verdict();
                } else {
                    window.on_verdict(tip - released_at);
                }
            }
        }
        self.update(sig, |record| {
            record.state = state;
            record.decided_at_height = Some(tip);
            record.detail = detail;
            if let Some((rejection_count, block_hash)) = node {
                record.rejection_count = rejection_count;
                if block_hash.is_some() {
                    record.latest_block_hash = block_hash;
                }
            }
        });
        metrics::counter!("deploy_queue.terminal", "state" => state.name()).increment(1);
    }

    fn update(&mut self, sig: &Sig, change: impl FnOnce(&mut QueueRecord)) {
        let record = self
            .records
            .get_mut(sig)
            .expect("updates name a record the engine holds");
        let before = (record.state, IndexKeys::of(record));
        change(record);
        let after = (record.state, IndexKeys::of(record));
        self.dirty.insert(sig.clone());
        if before.0 != after.0 {
            self.discount(before.0);
            self.enter(after.0);
        }
        if before.1 != after.1 {
            self.reindex(sig, Some(before.1), Some(after.1));
        }
        self.emit(sig);
    }

    fn reindex(&mut self, sig: &Sig, before: Option<IndexKeys>, after: Option<IndexKeys>) {
        if let Some(keys) = before {
            if let Some(key) = keys.released {
                self.released_index.remove(&key);
            }
            if let Some(key) = keys.resend {
                self.resend_index.remove(&key);
            }
            if let Some(key) = keys.decided {
                self.decided_index.remove(&key);
            }
            if let Some(deployer) = keys.holder {
                let held = self
                    .held_by_deployer
                    .get_mut(&deployer)
                    .expect("a holder's deploys are counted");
                *held -= 1;
                if *held == 0 {
                    self.held_by_deployer.remove(&deployer);
                }
            }
        }
        if let Some(keys) = after {
            if let Some(key) = keys.released {
                self.released_index.insert(key, sig.clone());
            }
            if let Some(key) = keys.resend {
                self.resend_index.insert(key, sig.clone());
            }
            if let Some(key) = keys.decided {
                self.decided_index.insert(key, sig.clone());
            }
            if let Some(deployer) = keys.holder {
                *self.held_by_deployer.entry(deployer).or_insert(0) += 1;
            }
        }
    }

    fn emit(&mut self, sig: &Sig) {
        let transition = Transition {
            sig: sig.clone(),
            view: self.records[sig].view(),
        };
        let transitions = self.transitions.clone();
        self.after_commit(move || {
            // No watcher is listening when the send finds no receiver.
            let _ = transitions.send(transition);
        });
    }

    fn count(&self, state: QueueState) -> usize { self.counts[&state] }

    fn discount(&mut self, state: QueueState) {
        *self
            .counts
            .get_mut(&state)
            .expect("counts hold every state") -= 1;
    }

    fn enter(&mut self, state: QueueState) {
        *self
            .counts
            .get_mut(&state)
            .expect("counts hold every state") += 1;
    }

    fn snapshot(&self) -> QueueSnapshot {
        QueueSnapshot {
            connected: self.connected,
            tip: self.chain.as_ref().map(|chain| chain.tip),
            last_finalized: self.chain.as_ref().map(|chain| chain.lfb),
            release_open: self.release_open(),
            ready: self.ready.len(),
            waiting: self.waiting.len(),
            closed: self.closed.len(),
            submitted: self.count(QueueState::Submitted),
            in_block: self.count(QueueState::InBlock),
            released_limit: self.released_limit(),
            release_window: self.window.as_ref().map(ReleaseWindow::window),
            release_cuts: self.window.as_ref().map(ReleaseWindow::cuts),
            late_share: self.window.as_ref().and_then(ReleaseWindow::late_share),
            settle_delay: self.window.as_ref().and_then(ReleaseWindow::settle_delay),
            inclusion_delay: self
                .window
                .as_ref()
                .and_then(ReleaseWindow::inclusion_delay),
            release_rate: self.window.as_ref().and_then(ReleaseWindow::release_rate),
            held: self.records.len(),
            settled_per_height: self.pace.as_ref().map(Pace::settled_per_height),
            secs_per_height: self.pace.as_ref().and_then(Pace::secs_per_height),
        }
    }

    fn publish_gauges(&self) {
        for state in ALL_STATES {
            metrics::gauge!("deploy_queue.deploys", "state" => state.name())
                .set(self.count(state) as f64);
        }
        metrics::gauge!("deploy_queue.ready").set(self.ready.len() as f64);
        metrics::gauge!("deploy_queue.waiting").set(self.waiting.len() as f64);
        metrics::gauge!("deploy_queue.release_open").set(f64::from(u8::from(self.release_open())));
        metrics::gauge!("deploy_queue.connected").set(f64::from(u8::from(self.connected)));
    }
}

/// The shard's citable depth: the span, in heights, in which a block may
/// still build on another.
fn citable_depth(shard: &ShardValues) -> i64 { i64::from(shard.max_parent_depth) }

/// Heights until `deploys` more deploys settle at `rate` a height; `None`
/// when the rate is zero: the shard is settling nothing.
fn heights_to_settle(deploys: usize, rate: f64) -> Option<i64> {
    if deploys == 0 {
        return Some(0);
    }
    (rate > 0.0).then(|| (deploys as f64 / rate).ceil() as i64)
}

/// A queued deploy as admission sees it: the first height it can be
/// released at and the last height a block can include it.
#[derive(Debug, Clone, Copy)]
struct Slot {
    releasable_from: i64,
    last: i64,
}

/// Whether the deploy at `position` in release order settles, at `rate`
/// a height, with `depth` heights to spare before its window closes.
fn lands(slot: Slot, position: usize, rate: f64, depth: i64) -> bool {
    heights_to_settle(position, rate)
        .is_some_and(|heights| slot.releasable_from + heights + depth <= slot.last)
}

/// Whether a new deploy may join the queue. Release takes the earliest
/// deadline first, so the deploy waits behind every released deploy and the
/// `earlier` queued deploys whose deadline comes first, and every queued
/// deploy in `later` moves back one place. It is admitted when it lands and
/// no later deploy that lands now would stop landing.
fn admits(
    released: usize,
    earlier: usize,
    new: Slot,
    later: impl Iterator<Item = Slot>,
    rate: f64,
    depth: i64,
) -> bool {
    let position = released + earlier + 1;
    lands(new, position, rate, depth)
        && later.enumerate().all(|(index, slot)| {
            let without = position + index;
            !lands(slot, without, rate, depth) || lands(slot, without + 1, rate, depth)
        })
}

/// Deploys the queue can hold: what settles at `rate` a height in a fresh
/// window, less the depth kept to spare.
fn capacity(rate: f64, fresh_span: i64, depth: i64) -> usize {
    (rate * (fresh_span - depth).max(0) as f64).floor() as usize
}

/// The most one deployer may hold while `others` deployers hold deploys: an
/// equal share that keeps room for one more deployer, and never less than
/// one deploy.
fn deployer_limit(capacity: usize, others: usize) -> usize { (capacity / (others + 2)).max(1) }

/// Heights until a deploy signed now would fit: the line ahead, `wait`
/// heights long, must shrink until it and the depth fit a fresh window of
/// `fresh_span` heights. A refused deploy's own window only closes while
/// it waits, so the client signs again rather than resubmitting it.
fn heights_until_a_fresh_window_fits(wait: i64, depth: i64, fresh_span: i64) -> i64 {
    (wait + depth - fresh_span).max(1)
}

fn window_open(valid_after: i64, reference_block: i64, deploy_lifespan: i64) -> bool {
    is_open(valid_after, reference_block, deploy_lifespan)
        .expect("the node's deploy lifespan was checked non-negative at connect")
}

fn last_includable(valid_after: i64, deploy_lifespan: i64) -> i64 {
    last_includable_block(valid_after, deploy_lifespan)
        .expect("the node's deploy lifespan was checked non-negative at connect")
}

fn decode_hex(what: &str, text: &str) -> Option<Vec<u8>> {
    match hex::decode(text) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            tracing::warn!(
                target: "deploy_queue::engine",
                value = %text,
                error = %e,
                "the node sent a {} that is not hex",
                what
            );
            None
        }
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after the Unix epoch")
        .as_millis() as i64
}

async fn wait_until(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_counts_the_heights_until_a_fresh_window_fits() {
        // A 299-height fresh window with a 25-height depth holds a 274-height line.
        assert_eq!(heights_until_a_fresh_window_fits(300, 25, 299), 26);
        assert_eq!(heights_until_a_fresh_window_fits(275, 25, 299), 1);
    }

    #[test]
    fn a_line_that_fits_a_fresh_window_retries_after_one_height() {
        // Refused only because its own window was signed long ago.
        assert_eq!(heights_until_a_fresh_window_fits(10, 25, 299), 1);
    }

    fn slot(releasable_from: i64, last: i64) -> Slot {
        Slot {
            releasable_from,
            last,
        }
    }

    #[test]
    fn admission_refuses_a_deploy_that_would_push_a_landing_deploy_out() {
        assert!(!admits(0, 0, slot(0, 1), [slot(0, 1)].into_iter(), 1.0, 0));
        assert!(admits(0, 0, slot(0, 1), [slot(0, 2)].into_iter(), 1.0, 0));
    }

    #[test]
    fn admission_ignores_a_deploy_that_already_misses_its_window() {
        assert!(admits(0, 0, slot(0, 1), [slot(0, 0)].into_iter(), 1.0, 0));
    }

    #[test]
    fn admission_counts_released_and_earlier_deploys_ahead() {
        assert!(admits(2, 3, slot(0, 6), std::iter::empty(), 1.0, 0));
        assert!(!admits(2, 3, slot(0, 5), std::iter::empty(), 1.0, 0));
    }

    /// Admits each height's arrivals, then releases one deploy a height,
    /// earliest deadline first, settling it at the next height. Panics when
    /// an admitted deploy settles with less than `depth` heights to spare.
    fn simulate(
        arrivals: impl Fn(i64) -> Vec<i64>,
        heights: i64,
        lifespan: i64,
        depth: i64,
    ) -> (usize, usize) {
        let mut queue: BTreeMap<(i64, u64), i64> = BTreeMap::new();
        let (mut sequence, mut admitted, mut refused) = (0u64, 0, 0);
        for tip in 0..heights {
            for valid_after in arrivals(tip) {
                let last = valid_after + lifespan - 1;
                let key = (last, sequence);
                sequence += 1;
                let new = slot(valid_after.max(tip), last);
                let later = queue
                    .range(key..)
                    .map(|((last, _), valid_after)| slot((*valid_after).max(tip), *last));
                if admits(0, queue.range(..key).count(), new, later, 1.0, depth) {
                    queue.insert(key, valid_after);
                    admitted += 1;
                } else {
                    refused += 1;
                }
            }
            if let Some(((last, _), _)) = queue.pop_first() {
                assert!(
                    tip + 1 + depth <= last,
                    "a deploy admitted with deadline {last} settled at {}",
                    tip + 1
                );
            }
        }
        (admitted, refused)
    }

    #[test]
    fn one_deployers_batch_and_later_single_deploys_all_land() {
        let arrivals = |tip: i64| match tip {
            0 => vec![0; 200],
            1..=268 => vec![tip],
            _ => Vec::new(),
        };
        assert_eq!(simulate(arrivals, 600, 300, 25), (468, 0));
    }

    #[test]
    fn under_sustained_overload_every_admitted_deploy_lands() {
        let (admitted, refused) = simulate(|tip| vec![tip; 3], 800, 300, 25);
        assert!(
            refused > 0,
            "three a height against one settling must refuse"
        );
        assert!(admitted > 0);
    }

    #[test]
    fn one_deployer_keeps_room_for_another() {
        assert_eq!(deployer_limit(274, 0), 137);
        assert_eq!(deployer_limit(274, 2), 68);
        assert_eq!(deployer_limit(0, 5), 1);
        assert_eq!(capacity(2.0, 299, 25), 548);
        assert_eq!(capacity(1.0, 20, 25), 0);
    }
}
