use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_request::DeployRequest;
use models::rust::deploy_window::{is_open, last_includable_block};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::error::{EngineStopped, IntakeError};
use crate::events::{NodeEvent, VerdictState};
use crate::node::{ChainSnapshot, NodeDeployStatus, NodeError, NodeHttp, SubmitOutcome};
use crate::pace::Pace;
use crate::ready::ReadyQueue;
use crate::record::{DeployView, QueueRecord, QueueState, Sig};
use crate::store::{QueueStore, StoreError};

/// Heights of measured inclusion the queue keeps in the node's pool: enough
/// that the node's next block never waits on the queue, little enough that
/// a backlog waits here rather than in the pool, where every proposal reads
/// and sorts it.
const IN_FLIGHT_HEIGHTS: f64 = 3.0;
/// The in-flight bound never exceeds this many of the shard's full blocks.
const IN_FLIGHT_CEILING_BLOCKS: usize = 2;
const RETRY_FLOOR: Duration = Duration::from_secs(1);
const RETRY_CEILING: Duration = Duration::from_secs(30);
/// Retry-After for a refusal made before the queue has timed a height.
const UNTIMED_RETRY_SECS: u64 = 5;
const NODE_UNAVAILABLE_RETRY_SECS: u64 = 5;
const TRANSITION_BUFFER: usize = 4096;
pub const COMMAND_BUFFER: usize = 1024;

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
    pub in_flight_limit: usize,
    pub in_block: usize,
    pub held: usize,
    pub deploys_per_height: Option<f64>,
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

/// The single owner of the queue's state. Every change is written to the
/// store before it is acknowledged or broadcast.
pub struct Engine {
    store: QueueStore,
    node: NodeHttp,
    commands: mpsc::Sender<Command>,
    records: HashMap<Sig, QueueRecord>,
    counts: HashMap<QueueState, usize>,
    /// Queued deploys whose window is open, in release order.
    ready: ReadyQueue,
    /// Queued deploys not yet valid, by (valid after, sequence).
    waiting: BTreeMap<(i64, u64), Sig>,
    /// Every placed queued deploy, by (last includable block, sequence).
    expiry: BTreeMap<(i64, u64), Sig>,
    /// Queued deploys whose window closed before release, by (height past
    /// which no block can include them, sequence).
    closed: BTreeMap<(i64, u64), Sig>,
    closed_checking: HashSet<Sig>,
    chain: Option<ChainSnapshot>,
    connected: bool,
    pace: Option<Pace>,
    next_sequence: u64,
    retry_at: Option<Instant>,
    retry_delay: Duration,
    transitions: broadcast::Sender<Transition>,
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
        Ok(Self {
            store,
            node,
            commands,
            records,
            counts,
            ready: ReadyQueue::default(),
            waiting: BTreeMap::new(),
            expiry: BTreeMap::new(),
            closed: BTreeMap::new(),
            closed_checking: HashSet::new(),
            chain: None,
            connected: false,
            pace: None,
            next_sequence,
            retry_at: None,
            retry_delay: RETRY_FLOOR,
            transitions,
        })
    }

    /// Runs until the command channel closes. A store failure stops the
    /// engine: the queue's contract is that what it acknowledged is durable.
    pub async fn run(mut self, mut commands: mpsc::Receiver<Command>) -> Result<(), StoreError> {
        loop {
            let retry_at = self.retry_at;
            tokio::select! {
                command = commands.recv() => match command {
                    Some(command) => self.handle(command)?,
                    None => return Ok(()),
                },
                _ = wait_until(retry_at) => {
                    self.retry_at = None;
                    self.release()?;
                }
            }
            self.publish_gauges();
        }
    }

    fn handle(&mut self, command: Command) -> Result<(), StoreError> {
        match command {
            Command::Intake { deploy, reply } => match self.intake(deploy) {
                Ok(result) => {
                    match &result {
                        Ok(_) => metrics::counter!("deploy_queue.accepted").increment(1),
                        Err(refusal) => {
                            metrics::counter!("deploy_queue.refused", "reason" => refusal.kind())
                                .increment(1)
                        }
                    }
                    // A client that hung up does not undo an accepted deploy.
                    let _ = reply.send(result);
                }
                Err(e) => {
                    let _ = reply.send(Err(IntakeError::Store(e.to_string())));
                    return Err(e);
                }
            },
            Command::Status { sig, reply } => {
                let _ = reply.send(self.records.get(&sig).map(QueueRecord::view));
            }
            Command::Watch { sigs, reply } => {
                let current = sigs
                    .into_iter()
                    .map(|sig| {
                        let view = self.records.get(&sig).map(QueueRecord::view);
                        (sig, view)
                    })
                    .collect();
                let _ = reply.send(WatchStart {
                    current,
                    updates: self.transitions.subscribe(),
                });
            }
            Command::Snapshot { reply } => {
                let _ = reply.send(self.snapshot());
            }
            Command::Connected(chain) => self.on_connected(chain)?,
            Command::Disconnected => {
                self.connected = false;
                tracing::warn!(target: "deploy_queue::engine", "lost the node; release paused");
            }
            Command::Node(event) => self.on_event(event)?,
            Command::Submitted { sig, outcome } => self.on_submitted(sig, outcome)?,
            Command::Resynced { sig, result } => self.on_resynced(sig, result)?,
            Command::ClosedChecked { sig, result } => self.on_closed_checked(sig, result)?,
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

        // A deploy is accepted only when the deploys ahead of it reach blocks
        // within the citable depth at the measured pace, and it then still
        // lands inside its own window. Beyond that it would wait minutes; the
        // client is told to come back instead.
        let pace = self
            .pace
            .as_ref()
            .expect("a connected engine has a pace");
        let depth = i64::from(shard.max_parent_depth);
        let ahead = self.ready.len() + self.waiting.len() + self.count(QueueState::Submitted);
        let releasable_from = valid_after.max(chain.tip);
        let last = last_includable(valid_after, shard.deploy_lifespan);
        let wait = pace.heights_to_include(ahead + 1);
        let fits = wait.is_some_and(|heights| {
            heights <= depth && releasable_from + heights + depth <= last
        });
        if !fits {
            let excess = wait.map(|heights| (heights - depth).max(1));
            let retry_after_secs = excess
                .and_then(|heights| pace.seconds_for(heights))
                .unwrap_or(UNTIMED_RETRY_SECS);
            return Ok(Err(IntakeError::QueueFull { retry_after_secs }));
        }

        let record = QueueRecord::new(&deploy, self.next_sequence, now_ms);
        if !self.store.insert_new(&sig, &record)? {
            return Ok(Err(IntakeError::Duplicate(sig)));
        }
        self.next_sequence += 1;
        self.enter(QueueState::Queued);
        self.records.insert(sig.clone(), record);
        self.place(&sig, false);
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
            self.pace = Some(Pace::new(chain.shard.max_user_deploys_per_block));
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
                // Before this block moves any deploy out of the pool.
                if self.count(QueueState::Submitted) > 0 {
                    if let Some(pace) = self.pace.as_mut() {
                        pace.record_outstanding();
                    }
                }
                let block_hash = decode_hex("block hash", &hash);
                let mut included = 0;
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
                        included += 1;
                    }
                    let hash = block_hash.clone();
                    self.update(&sig, |record| {
                        record.state = QueueState::InBlock;
                        if hash.is_some() {
                            record.latest_block_hash = hash;
                        }
                    })?;
                }
                if let Some(pace) = self.pace.as_mut() {
                    pace.record_inclusions(included);
                }
                self.observe_tip(number)?;
            }
            NodeEvent::Finalised { number } => self.observe_lfb(number),
            NodeEvent::Verdicts(verdicts) => {
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
                    self.decide(&sig, state, Some((verdict.rejection_count, block_hash)), None)?;
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
                self.decide(&sig, QueueState::Expired, None, Some(message))?;
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
                )?;
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
                })?;
                self.place(&sig, true);
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
            Ok(status) => self.adopt(&sig, status, UnplacedPending::KeepWaiting)?,
            Err(e) => tracing::warn!(
                target: "deploy_queue::engine",
                sig = %hex::encode(&sig),
                error = %e,
                "resync read failed; the next reconnect or lag retries it"
            ),
        }
        self.release()
    }

    fn on_closed_checked(
        &mut self,
        sig: Sig,
        result: Result<NodeDeployStatus, NodeError>,
    ) -> Result<(), StoreError> {
        self.closed_checking.remove(&sig);
        let queued = self
            .records
            .get(&sig)
            .is_some_and(|record| record.state == QueueState::Queued);
        if !queued {
            return Ok(());
        }
        match result {
            Ok(status) => self.adopt(&sig, status, UnplacedPending::Expire)?,
            Err(e) => tracing::warn!(
                target: "deploy_queue::engine",
                sig = %hex::encode(&sig),
                error = %e,
                "closed-window check failed; the next finalization retries it"
            ),
        }
        Ok(())
    }

    /// Takes the node's answer for a deploy the queue has lost track of.
    fn adopt(
        &mut self,
        sig: &Sig,
        status: NodeDeployStatus,
        unplaced_pending: UnplacedPending,
    ) -> Result<(), StoreError> {
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
                None => Ok(()),
            },
            other => {
                tracing::error!(
                    target: "deploy_queue::engine",
                    sig = %hex::encode(sig),
                    state = %other,
                    "the node answered a deploy state the queue does not know"
                );
                Ok(())
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
        let limit = self.in_flight_limit();
        while self.count(QueueState::Submitted) < limit {
            let Some(sig) = self.ready.pop() else {
                break;
            };
            let valid_after = self.records[&sig].valid_after;
            if !window_open(valid_after, chain.tip + 1, shard.deploy_lifespan) {
                self.close(&sig);
                continue;
            }
            self.submit(&sig, chain.tip)?;
        }
        if !self.ready.is_empty() {
            if let Some(pace) = self.pace.as_mut() {
                pace.record_held_back();
            }
        }
        Ok(())
    }

    /// Deploys the node's pool may hold from this queue: the measured
    /// inclusion of a few heights, at least one so a stalled pace is still
    /// probed, at most a couple of the shard's full blocks.
    fn in_flight_limit(&self) -> usize {
        let (Some(chain), Some(pace)) = (&self.chain, &self.pace) else {
            return 0;
        };
        let full_blocks =
            IN_FLIGHT_CEILING_BLOCKS * chain.shard.max_user_deploys_per_block as usize;
        let measured = (IN_FLIGHT_HEIGHTS * pace.deploys_per_height()).ceil() as usize;
        measured.clamp(full_blocks.min(1), full_blocks)
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
            record.submissions += 1;
        })?;
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

    fn spawn_submit(&self, sig: Sig, request: DeployRequest) {
        let node = self.node.clone();
        let commands = self.commands.clone();
        tokio::spawn(async move {
            let outcome = node.submit(&request).await;
            // The engine stopping ends the process; the outcome has no reader.
            let _ = commands.send(Command::Submitted { sig, outcome }).await;
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
        let outstanding = self.count(QueueState::Submitted) > 0;
        if let Some(pace) = self.pace.as_mut() {
            if outstanding {
                pace.record_outstanding();
            }
            pace.on_tip(number, Instant::now());
        }
        let became_valid = self.waiting.split_off(&(number + 1, 0));
        let valid_now = std::mem::replace(&mut self.waiting, became_valid);
        for sig in valid_now.into_values() {
            let deployer = self.records[&sig].deployer.clone();
            self.ready.push_back(&deployer, sig);
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
            tokio::spawn(async move {
                let result = node.deploy_status(&sig).await;
                let _ = commands.send(Command::ClosedChecked { sig, result }).await;
            });
        }
    }

    /// Asks the node about every deploy it may hold, after a gap in the
    /// event stream (a reconnect or a lag).
    fn resync(&mut self) {
        let in_flight: Vec<Sig> = self
            .records
            .iter()
            .filter(|(_, record)| {
                matches!(record.state, QueueState::Submitted | QueueState::InBlock)
            })
            .map(|(sig, _)| sig.clone())
            .collect();
        for sig in in_flight {
            self.spawn_status_read(sig);
        }
    }

    fn spawn_status_read(&self, sig: Sig) {
        let node = self.node.clone();
        let commands = self.commands.clone();
        tokio::spawn(async move {
            let result = node.deploy_status(&sig).await;
            let _ = commands.send(Command::Resynced { sig, result }).await;
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
        let stale: Vec<Sig> = self
            .records
            .iter()
            .filter(|(_, record)| {
                record.state == QueueState::Submitted
                    && record
                        .submitted_at_height
                        .is_some_and(|height| tip - height > depth)
            })
            .map(|(sig, _)| sig.clone())
            .collect();
        for sig in stale {
            let request = self.request_for(&sig)?;
            self.update(&sig, |record| {
                record.submitted_at_height = Some(tip);
                record.submissions += 1;
            })?;
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
            .records
            .iter()
            .filter(|(_, record)| {
                record
                    .decided_at_height
                    .is_some_and(|height| tip - height > lifespan)
            })
            .map(|(sig, _)| sig.clone())
            .collect();
        if done.is_empty() {
            return Ok(());
        }
        self.store.remove(done.clone())?;
        for sig in done {
            if let Some(record) = self.records.remove(&sig) {
                self.discount(record.state);
            }
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
            self.place(&sig, false);
        }
        if let Some(tip) = self.chain.as_ref().map(|chain| chain.tip) {
            self.close_expired(tip);
        }
    }

    /// Files a queued deploy: ready if its window has opened, waiting if not.
    fn place(&mut self, sig: &Sig, at_front: bool) {
        let chain = self.chain.as_ref().expect("deploys are placed once the chain is known");
        let record = &self.records[sig];
        let key = (record.valid_after, record.sequence);
        let last = last_includable(record.valid_after, chain.shard.deploy_lifespan);
        let waiting = record.valid_after > chain.tip;
        let deployer = record.deployer.clone();
        self.expiry.insert((last, key.1), sig.clone());
        if waiting {
            self.waiting.insert(key, sig.clone());
        } else if at_front {
            self.ready.push_front(&deployer, sig.clone());
        } else {
            self.ready.push_back(&deployer, sig.clone());
        }
    }

    /// Removes a queued deploy from every placement index.
    fn unplace(&mut self, sig: &Sig) {
        let record = &self.records[sig];
        let (valid_after, sequence, deployer) =
            (record.valid_after, record.sequence, record.deployer.clone());
        self.ready.remove(&deployer, sig);
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
        let chain = self.chain.as_ref().expect("deploys close once the chain is known");
        let depth = i64::from(chain.shard.max_parent_depth);
        let record = &self.records[sig];
        let (valid_after, sequence, deployer) =
            (record.valid_after, record.sequence, record.deployer.clone());
        let last = last_includable(valid_after, chain.shard.deploy_lifespan);
        self.ready.remove(&deployer, sig);
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
    ) -> Result<(), StoreError> {
        let tip = self
            .chain
            .as_ref()
            .map(|chain| chain.tip)
            .expect("verdicts arrive once the chain is known");
        if self.records[sig].state == QueueState::Queued {
            self.unplace(sig);
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
        })?;
        metrics::counter!("deploy_queue.terminal", "state" => state.name()).increment(1);
        Ok(())
    }

    fn update(
        &mut self,
        sig: &Sig,
        change: impl FnOnce(&mut QueueRecord),
    ) -> Result<(), StoreError> {
        let record = self
            .records
            .get_mut(sig)
            .expect("updates name a record the engine holds");
        let before = record.state;
        change(record);
        let after = record.state;
        self.store.put(sig, record)?;
        if before != after {
            self.discount(before);
            self.enter(after);
        }
        self.emit(sig);
        Ok(())
    }

    fn emit(&self, sig: &Sig) {
        let view = self.records[sig].view();
        // No watcher is listening when the send finds no receiver.
        let _ = self.transitions.send(Transition {
            sig: sig.clone(),
            view,
        });
    }

    fn count(&self, state: QueueState) -> usize { self.counts[&state] }

    fn discount(&mut self, state: QueueState) {
        *self.counts.get_mut(&state).expect("counts hold every state") -= 1;
    }

    fn enter(&mut self, state: QueueState) {
        *self.counts.get_mut(&state).expect("counts hold every state") += 1;
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
            in_flight_limit: self.in_flight_limit(),
            in_block: self.count(QueueState::InBlock),
            held: self.records.len(),
            deploys_per_height: self.pace.as_ref().map(Pace::deploys_per_height),
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
