use std::sync::{Arc, RwLock};
use std::time::Duration;

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::pending_blocks::PendingUsage;
use comm::rust::peer_node::PeerNode;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinSet;

use super::dependency_tracker::Work;
use super::fetch_scheduler::FetchAction;
use super::ingest_state::{
    Effect, IngestInput, IngestState, StateFetchDrop, StateFetchResult, StoreReads,
};
use super::ingest_view::IngestView;
use super::intake_worker::Intake;
use super::peer_reputation::StrikeOutcome;
use super::validation_worker::Validation;
use crate::rust::casper::{Casper, MultiParentCasper};
use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    BLOCK_INGEST_DISK_ENTRIES_METRIC, BLOCK_INGEST_HELD_METRIC, BLOCK_INGEST_INSERTS_PENDING_METRIC,
    BLOCK_INGEST_LANE_DROPS_METRIC, BLOCK_INGEST_METRICS_SOURCE, BLOCK_INGEST_NEEDS_METRIC,
    BLOCK_INGEST_OUTBOUND_DROPS_METRIC, BLOCK_INGEST_PEER_REPUTATION_IGNORED_PEERS_METRIC,
    BLOCK_INGEST_PEER_REPUTATION_STRIKES_METRIC,
    BLOCK_INGEST_ROOT_NEEDS_METRIC, BLOCK_INGEST_UNCITED_NEEDS_METRIC, BLOCK_INGEST_VALIDATING_METRIC,
    BLOCK_PROCESSING_PARALLEL_LIMIT_METRIC, BLOCK_PROCESSOR_METRICS_SOURCE,
};
use crate::rust::util::proto_util;

/// Network deliveries of a block the node asked for. Twice the outstanding
/// fetch window: a broadcast can draw replies from several peers.
pub const AWAITED_LANE_CAPACITY: usize = 512;
pub const UNSOLICITED_LANE_CAPACITY: usize = 1_024;
/// Hashes the owner asks intake to read from the local block store.
pub const LOCAL_LANE_CAPACITY: usize = 256;
pub const INPUT_CAPACITY: usize = 4_096;
pub const OUTBOUND_CAPACITY: usize = 1_024;
pub const INTAKE_TASKS: usize = 4;
pub const TICK_INTERVAL: Duration = Duration::from_millis(100);

pub type SharedCasper = Arc<dyn MultiParentCasper + Send + Sync>;

/// A block a peer sent.
pub struct Delivery {
    pub block: BlockMessage,
    pub from: PeerNode,
}

enum Outbound {
    Request { hash: BlockHash, peer: PeerNode },
    Broadcast(BlockHash),
}

/// Casper and the intake checks that need its approved block, set once the
/// node runs casper.
#[derive(Clone)]
struct Attached {
    casper: SharedCasper,
    intake: Arc<Intake>,
}

type AttachedSlot = Arc<RwLock<Option<Attached>>>;

/// What the rest of the node holds to hand blocks and needs to block ingest.
#[derive(Clone)]
pub struct IngestHandle {
    view: IngestView,
    awaited: mpsc::Sender<Delivery>,
    unsolicited: mpsc::Sender<Delivery>,
    inputs: mpsc::Sender<IngestInput>,
    attached: AttachedSlot,
    block_store: KeyValueBlockStore,
    dag: BlockDagKeyValueStorage,
}

/// The receiving ends of an `IngestHandle`'s channels, for the task that owns
/// block ingest's state.
pub struct IngestPorts {
    pub awaited: mpsc::Receiver<Delivery>,
    pub unsolicited: mpsc::Receiver<Delivery>,
    pub inputs: mpsc::Receiver<IngestInput>,
    pub inserted: mpsc::UnboundedReceiver<BlockHash>,
}

impl IngestHandle {
    /// A handle and the ports its producers feed. Every block `dag` inserts
    /// from now on is reported on `inserted`.
    pub fn open(
        block_store: KeyValueBlockStore,
        dag: BlockDagKeyValueStorage,
    ) -> Result<(IngestHandle, IngestPorts), CasperError> {
        let (awaited_tx, awaited) = mpsc::channel(AWAITED_LANE_CAPACITY);
        let (unsolicited_tx, unsolicited) = mpsc::channel(UNSOLICITED_LANE_CAPACITY);
        let (inputs_tx, inputs) = mpsc::channel(INPUT_CAPACITY);
        let (inserted_tx, inserted) = mpsc::unbounded_channel();
        dag.set_insert_observer(inserted_tx)?;
        let handle = IngestHandle {
            view: IngestView::new(),
            awaited: awaited_tx,
            unsolicited: unsolicited_tx,
            inputs: inputs_tx,
            attached: Arc::new(RwLock::new(None)),
            block_store,
            dag,
        };
        let ports = IngestPorts {
            awaited,
            unsolicited,
            inputs,
            inserted,
        };
        Ok((handle, ports))
    }

    pub fn view(&self) -> &IngestView { &self.view }

    /// Start ingesting for `casper`. Blocks arriving before this are dropped.
    pub async fn attach_casper(&self, casper: SharedCasper) -> Result<(), CasperError> {
        let approved_block = casper.get_approved_block()?.clone();
        let anchor_height = proto_util::block_number(&approved_block);
        let intake = Arc::new(Intake::new(
            self.block_store.clone(),
            self.dag.clone(),
            self.view.clone(),
            approved_block,
        ));
        *self
            .attached
            .write()
            .map_err(|_| CasperError::RuntimeError("block ingest casper slot poisoned".to_string()))? =
            Some(Attached { casper, intake });
        self.inputs
            .send(IngestInput::Attached { anchor_height })
            .await
            .map_err(|_| CasperError::RuntimeError("block ingest has stopped".to_string()))
    }

    /// A peer sent a block. It never waits: when its lane is full the block
    /// is dropped, and a block the node needs is asked for again.
    pub fn deliver(&self, block: BlockMessage, from: PeerNode) {
        let (lane, label) = match self.view.is_awaited(&block.block_hash) {
            true => (&self.awaited, "awaited"),
            false => (&self.unsolicited, "unsolicited"),
        };
        if let Err(error) = lane.try_send(Delivery { block, from }) {
            metrics::counter!(BLOCK_INGEST_LANE_DROPS_METRIC, "source" => BLOCK_INGEST_METRICS_SOURCE, "lane" => label)
                .increment(1);
            tracing::debug!(lane = label, "block ingest lane refused a delivery: {error}");
        }
    }

    /// A peer says it holds `hash`.
    pub fn announce(&self, hash: BlockHash, peer: PeerNode) {
        if self
            .inputs
            .try_send(IngestInput::Announced { hash, peer })
            .is_err()
        {
            metrics::counter!(BLOCK_INGEST_LANE_DROPS_METRIC, "source" => BLOCK_INGEST_METRICS_SOURCE, "lane" => "announcements")
                .increment(1);
        }
    }

    /// The node needs `hash`, which no held block cites.
    pub async fn need(&self, hash: BlockHash) -> Result<(), CasperError> {
        self.inputs
            .send(IngestInput::Need { hash })
            .await
            .map_err(|_| CasperError::RuntimeError("block ingest has stopped".to_string()))
    }
}

/// What block ingest runs on.
pub struct IngestSetup<T: TransportLayer + Send + Sync> {
    pub block_store: KeyValueBlockStore,
    pub dag: BlockDagKeyValueStorage,
    pub transport: Arc<T>,
    pub connections: ConnectionsCell,
    pub conf: RPConf,
    pub validation_width: usize,
    pub state_root_fetch_tx: Option<mpsc::Sender<Blake2b256Hash>>,
    pub state_root_results: Option<mpsc::UnboundedReceiver<(Blake2b256Hash, StateFetchResult)>>,
}

/// The node's stores, as block ingest's state reads them.
pub struct Stores<'a> {
    pub dag: &'a BlockDagKeyValueStorage,
    pub block_store: &'a KeyValueBlockStore,
}

impl StoreReads for Stores<'_> {
    fn in_dag(&self, hash: &BlockHash) -> bool { self.dag.contains(hash) }

    fn is_satisfied(&self, hash: &BlockHash) -> bool {
        match self.dag.is_dependency_satisfied(hash) {
            Ok(satisfied) => satisfied,
            // Unproven satisfaction is waited on; the insert observer
            // releases the waiter if the block is in the DAG after all.
            Err(error) => {
                tracing::warn!(
                    hash = %PrettyPrinter::build_string_bytes(hash),
                    %error,
                    "dependency check failed; the block waits on it"
                );
                false
            }
        }
    }

    fn pending_usage(&self) -> PendingUsage { self.block_store.pending_usage() }
}

/// Spawn block ingest: the owner task, its intake and validation tasks, and
/// the task that sends block requests. Returns the handle producers use and
/// the tasks, which must run for the node's lifetime.
pub fn spawn<T: TransportLayer + Send + Sync + 'static>(
    setup: IngestSetup<T>,
) -> Result<(IngestHandle, JoinSet<()>), CasperError> {
    let (handle, ports) = IngestHandle::open(setup.block_store.clone(), setup.dag.clone())?;
    let mut tasks = JoinSet::new();
    let (local_tx, local_rx) = mpsc::channel(LOCAL_LANE_CAPACITY);
    let (outbound_tx, outbound_rx) = mpsc::channel(OUTBOUND_CAPACITY);
    let (work_tx, work_rx) = mpsc::channel(setup.validation_width);
    metrics::gauge!(BLOCK_PROCESSING_PARALLEL_LIMIT_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
        .set(setup.validation_width as f64);

    let lanes = Arc::new(Mutex::new(IntakeLanes {
        awaited: ports.awaited,
        local: local_rx,
        unsolicited: ports.unsolicited,
    }));
    for _ in 0..INTAKE_TASKS {
        tasks.spawn(run_intake(
            lanes.clone(),
            handle.attached.clone(),
            setup.block_store.clone(),
            handle.inputs.clone(),
        ));
    }

    let validation = Arc::new(Validation::new(
        setup.block_store.clone(),
        setup.dag.clone(),
        setup.transport.clone(),
        setup.connections.clone(),
        setup.conf.clone(),
        setup.state_root_fetch_tx.clone(),
    ));
    let work_rx = Arc::new(Mutex::new(work_rx));
    for _ in 0..setup.validation_width {
        tasks.spawn(run_validation(
            work_rx.clone(),
            handle.attached.clone(),
            validation.clone(),
            handle.inputs.clone(),
        ));
    }

    tasks.spawn(run_outbound(
        outbound_rx,
        setup.transport.clone(),
        setup.connections.clone(),
        setup.conf.clone(),
    ));

    let owner = Owner {
        state: IngestState::new(handle.view.clone()),
        view: handle.view.clone(),
        block_store: setup.block_store.clone(),
        dag: setup.dag.clone(),
        connections: setup.connections,
        local: local_tx,
        outbound: outbound_tx,
        work: work_tx,
        validating: 0,
        width: setup.validation_width,
        state_root_fetch_tx: setup.state_root_fetch_tx,
    };
    tasks.spawn(owner.run(ports.inputs, ports.inserted, setup.state_root_results));
    Ok((handle, tasks))
}

struct IntakeLanes {
    awaited: mpsc::Receiver<Delivery>,
    local: mpsc::Receiver<BlockHash>,
    unsolicited: mpsc::Receiver<Delivery>,
}

enum IntakeJob {
    Delivered(Box<Delivery>),
    Local(BlockHash),
}

fn attached(slot: &AttachedSlot) -> Option<Attached> {
    match slot.read() {
        Ok(guard) => guard.clone(),
        Err(_) => {
            tracing::error!("block ingest casper slot poisoned");
            None
        }
    }
}

/// Take the next job, requested deliveries first, then local reads, then
/// unsolicited gossip, and report what intake found to the owner. Reports
/// are awaited: a dropped report would orphan a body already stored.
async fn run_intake(
    lanes: Arc<Mutex<IntakeLanes>>,
    slot: AttachedSlot,
    block_store: KeyValueBlockStore,
    inputs: mpsc::Sender<IngestInput>,
) {
    loop {
        let job = {
            let mut lanes = lanes.lock().await;
            let IntakeLanes {
                awaited,
                local,
                unsolicited,
            } = &mut *lanes;
            tokio::select! {
                biased;
                Some(delivery) = awaited.recv() => IntakeJob::Delivered(Box::new(delivery)),
                Some(hash) = local.recv() => IntakeJob::Local(hash),
                Some(delivery) = unsolicited.recv() => IntakeJob::Delivered(Box::new(delivery)),
                else => return,
            }
        };
        let Some(attached) = attached(&slot) else {
            tracing::debug!("block ingest has no casper yet; intake job dropped");
            continue;
        };
        let report = match job {
            IntakeJob::Delivered(delivery) => {
                let Delivery { block, from } = *delivery;
                attached.intake.admit(&block, Some(from))
            }
            IntakeJob::Local(hash) => match block_store.get(&hash) {
                Ok(Some(block)) => attached.intake.admit(&block, None),
                Ok(None) => Ok(IngestInput::LocalMiss { hash }),
                Err(error) => {
                    tracing::warn!(
                        hash = %PrettyPrinter::build_string_bytes(&hash),
                        %error,
                        "local block read failed; asking the network"
                    );
                    Ok(IngestInput::LocalMiss { hash })
                }
            },
        };
        match report {
            Ok(input) => {
                if inputs.send(input).await.is_err() {
                    return;
                }
            }
            Err(error) => tracing::error!(%error, "block intake failed"),
        }
    }
}

async fn run_validation<T: TransportLayer + Send + Sync + 'static>(
    work: Arc<Mutex<mpsc::Receiver<(BlockHash, Work)>>>,
    slot: AttachedSlot,
    validation: Arc<Validation<T>>,
    inputs: mpsc::Sender<IngestInput>,
) {
    loop {
        let Some((hash, kind)) = work.lock().await.recv().await else {
            return;
        };
        let outcome = match (attached(&slot), validation.block_store().get(&hash)) {
            (Some(attached), Ok(Some(block))) => {
                let casper = attached.casper.clone() as Arc<dyn Casper + Send + Sync>;
                tracing::info!("Block {} processing started.", PrettyPrinter::build_string_bytes(&hash));
                match kind {
                    Work::Validate => validation.validate(casper, &block).await,
                    Work::AdmitSettled => validation.admit_settled(casper, &block).await,
                }
            }
            (None, _) => {
                tracing::error!("a block was dispatched before casper was attached");
                super::ingest_state::WorkOutcome::Failed
            }
            (_, Ok(None)) => {
                tracing::error!(
                    hash = %PrettyPrinter::build_string_bytes(&hash),
                    "a dispatched block has no stored body"
                );
                super::ingest_state::WorkOutcome::Failed
            }
            (_, Err(error)) => {
                tracing::error!(
                    hash = %PrettyPrinter::build_string_bytes(&hash),
                    %error,
                    "reading a dispatched block failed"
                );
                super::ingest_state::WorkOutcome::Failed
            }
        };
        if inputs
            .send(IngestInput::Outcome { hash, outcome })
            .await
            .is_err()
        {
            return;
        }
    }
}

async fn run_outbound<T: TransportLayer + Send + Sync>(
    mut outbound: mpsc::Receiver<Outbound>,
    transport: Arc<T>,
    connections: ConnectionsCell,
    conf: RPConf,
) {
    while let Some(request) = outbound.recv().await {
        let (hash, result) = match request {
            Outbound::Request { hash, peer } => {
                let result = transport.request_for_block(&conf, &peer, hash.clone()).await;
                (hash, result)
            }
            Outbound::Broadcast(hash) => {
                let result = transport
                    .broadcast_has_block_request(&connections, &conf, &hash)
                    .await;
                (hash, result)
            }
        };
        if let Err(error) = result {
            tracing::debug!(
                hash = %PrettyPrinter::build_string_bytes(&hash),
                %error,
                "block request send failed; the request is retried on its pacing"
            );
        }
    }
}

struct Owner {
    state: IngestState,
    view: IngestView,
    block_store: KeyValueBlockStore,
    dag: BlockDagKeyValueStorage,
    connections: ConnectionsCell,
    local: mpsc::Sender<BlockHash>,
    outbound: mpsc::Sender<Outbound>,
    work: mpsc::Sender<(BlockHash, Work)>,
    validating: usize,
    width: usize,
    state_root_fetch_tx: Option<mpsc::Sender<Blake2b256Hash>>,
}

impl Owner {
    fn now_ms(&self) -> u64 { self.view.now_ms() }

    async fn run(
        mut self,
        mut inputs: mpsc::Receiver<IngestInput>,
        mut inserted: mpsc::UnboundedReceiver<BlockHash>,
        mut state_root_results: Option<mpsc::UnboundedReceiver<(Blake2b256Hash, StateFetchResult)>>,
    ) {
        let mut tick = tokio::time::interval(TICK_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let input = tokio::select! {
                biased;
                Some(hash) = inserted.recv() => IngestInput::Inserted { hash },
                Some(input) = inputs.recv() => input,
                Some((root, result)) = async {
                    match state_root_results.as_mut() {
                        Some(results) => results.recv().await,
                        None => std::future::pending().await,
                    }
                } => IngestInput::StateFetch { root, result },
                _ = tick.tick() => {
                    metrics::gauge!(BLOCK_INGEST_INSERTS_PENDING_METRIC, "source" => BLOCK_INGEST_METRICS_SOURCE)
                        .set(inserted.len() as f64);
                    self.tick_input()
                }
                else => return,
            };
            if matches!(input, IngestInput::Outcome { .. }) {
                self.validating = self.validating.saturating_sub(1);
            }
            self.handle(input).await;
            self.dispatch().await;
        }
    }

    fn tick_input(&self) -> IngestInput {
        let connected = match self.connections.read() {
            Ok(connections) => connections.iter().cloned().collect(),
            Err(error) => {
                tracing::warn!(%error, "could not read connected peers");
                Vec::new()
            }
        };
        IngestInput::Tick { connected }
    }

    async fn handle(&mut self, input: IngestInput) {
        let mut pending = vec![input];
        while let Some(input) = pending.pop() {
            let now_ms = self.now_ms();
            let stores = Stores {
                dag: &self.dag,
                block_store: &self.block_store,
            };
            let effects = self.state.handle(input, now_ms, &stores);
            for effect in effects {
                if let Some(follow_up) = self.execute(effect) {
                    pending.push(follow_up);
                }
            }
        }
        self.record_gauges();
    }

    /// Carry out an effect; an effect that could not be carried out may
    /// produce an input for the state.
    fn execute(&mut self, effect: Effect) -> Option<IngestInput> {
        match effect {
            Effect::Fetch(FetchAction::LookupLocal(hash)) => {
                if self.local.try_send(hash.clone()).is_err() {
                    self.state.defer_local_lookup(&hash);
                }
                None
            }
            Effect::Fetch(FetchAction::RequestBlock { hash, peer }) => {
                self.send_outbound(Outbound::Request { hash, peer });
                None
            }
            Effect::Fetch(FetchAction::BroadcastHasBlockRequest(hash)) => {
                self.send_outbound(Outbound::Broadcast(hash));
                None
            }
            Effect::Fetch(FetchAction::FetchStateRoot(root)) => {
                let sent = self
                    .state_root_fetch_tx
                    .as_ref()
                    .is_some_and(|tx| tx.try_send(root.clone()).is_ok());
                (!sent).then_some(IngestInput::StateFetch {
                    root,
                    result: StateFetchResult::Dropped(StateFetchDrop::QueueFull),
                })
            }
            Effect::DiscardPending(hash) => {
                self.block_store.discard_pending(&hash);
                None
            }
            Effect::Struck {
                peer,
                strike,
                outcome,
            } => {
                metrics::counter!(
                    BLOCK_INGEST_PEER_REPUTATION_STRIKES_METRIC,
                    "source" => BLOCK_INGEST_METRICS_SOURCE,
                    "reason" => strike.label()
                )
                .increment(1);
                if let StrikeOutcome::Ignored { until_ms } = outcome {
                    tracing::warn!(
                        %peer,
                        reason = strike.label(),
                        ignored_for_ms = until_ms.saturating_sub(self.now_ms()),
                        "Ignoring block traffic from a peer that pushed too many invalid blocks"
                    );
                }
                None
            }
        }
    }

    fn send_outbound(&self, request: Outbound) {
        if self.outbound.try_send(request).is_err() {
            metrics::counter!(BLOCK_INGEST_OUTBOUND_DROPS_METRIC, "source" => BLOCK_INGEST_METRICS_SOURCE)
                .increment(1);
        }
    }

    async fn dispatch(&mut self) {
        while self.validating < self.width {
            let Some(work) = self.state.next_dispatch() else {
                return;
            };
            if self.work.send(work).await.is_err() {
                return;
            }
            self.validating += 1;
        }
    }

    fn record_gauges(&self) {
        let gauges = [
            (BLOCK_INGEST_HELD_METRIC, self.state.held()),
            (BLOCK_INGEST_DISK_ENTRIES_METRIC, self.state.disk_entries()),
            (BLOCK_INGEST_NEEDS_METRIC, self.state.block_needs()),
            (BLOCK_INGEST_UNCITED_NEEDS_METRIC, self.state.uncited_needs()),
            (BLOCK_INGEST_ROOT_NEEDS_METRIC, self.state.root_needs()),
            (BLOCK_INGEST_VALIDATING_METRIC, self.validating),
            (
                BLOCK_INGEST_PEER_REPUTATION_IGNORED_PEERS_METRIC,
                self.state.ignored_peers(self.now_ms()),
            ),
        ];
        for (name, value) in gauges {
            metrics::gauge!(name, "source" => BLOCK_INGEST_METRICS_SOURCE).set(value as f64);
        }
    }
}
