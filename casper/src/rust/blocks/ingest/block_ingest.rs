use std::collections::{HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::pending_blocks::PendingUsage;
use comm::rust::peer_node::PeerNode;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use futures::{FutureExt, StreamExt};
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinSet;
use tokio_stream::wrappers::ReceiverStream;

use super::checked_block::{CheckedBlock, IntegrityFailure};
use super::dependency_tracker::Work;
use super::fetch_scheduler::FetchAction;
use super::ingest_state::{
    Effect, IngestInput, IngestState, StateFetchDrop, StateFetchResult, StoreReads,
};
use super::ingest_view::IngestView;
use super::intake_worker::Intake;
use super::peer_reputation::{Strike, StrikeOutcome};
use super::validation_worker::Validation;
use crate::rust::casper::{Casper, MultiParentCasper};
use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    BLOCK_INGEST_DISK_ENTRIES_METRIC, BLOCK_INGEST_HELD_METRIC,
    BLOCK_INGEST_INSERTS_PENDING_METRIC, BLOCK_INGEST_LANE_DROPS_METRIC,
    BLOCK_INGEST_METRICS_SOURCE, BLOCK_INGEST_NEEDS_METRIC, BLOCK_INGEST_OUTBOUND_DROPS_METRIC,
    BLOCK_INGEST_PEER_REPUTATION_IGNORED_PEERS_METRIC, BLOCK_INGEST_PEER_REPUTATION_STRIKES_METRIC,
    BLOCK_INGEST_REQUESTS_OUTSTANDING_METRIC, BLOCK_INGEST_ROOT_NEEDS_METRIC,
    BLOCK_INGEST_UNCITED_NEEDS_METRIC, BLOCK_PROCESSING_ACTIVE_METRIC,
    BLOCK_PROCESSING_IN_FLIGHT_METRIC, BLOCK_PROCESSING_IN_FLIGHT_OLDEST_AGE_METRIC,
    BLOCK_PROCESSING_PARALLEL_LIMIT_METRIC, BLOCK_PROCESSING_QUEUE_PENDING_METRIC,
    BLOCK_PROCESSOR_METRICS_SOURCE,
};

/// Network deliveries of a block the node asked for. Twice the outstanding
/// fetch window: a broadcast can draw replies from several peers.
pub const AWAITED_LANE_CAPACITY: usize = 512;
pub const UNSOLICITED_LANE_CAPACITY: usize = 1_024;
/// Hashes the owner asks intake to read from the local block store.
pub const LOCAL_LANE_CAPACITY: usize = 256;
pub const INPUT_CAPACITY: usize = 4_096;
pub const OUTBOUND_CAPACITY: usize = 1_024;
/// Block requests in flight at once.
pub const OUTBOUND_CONCURRENCY: usize = 16;
pub const INTAKE_TASKS: usize = 4;
pub const TICK_INTERVAL: Duration = Duration::from_millis(100);
pub const STUCK_VALIDATION_WARN_MS: u64 = 10 * 60 * 1000;

pub type SharedCasper = Arc<dyn MultiParentCasper + Send + Sync>;

/// A block a peer sent, checked for integrity.
pub struct Delivery {
    pub block: CheckedBlock,
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
        let anchor_height =
            crate::rust::util::proto_util::block_number(&approved_block);
        let intake = Arc::new(Intake::new(
            self.block_store.clone(),
            self.dag.clone(),
            self.view.clone(),
            approved_block,
        ));
        *self.attached.write().map_err(|_| {
            CasperError::RuntimeError("block ingest casper slot poisoned".to_string())
        })? = Some(Attached { casper, intake });
        self.inputs
            .send(IngestInput::Attached { anchor_height })
            .await
            .map_err(|_| CasperError::RuntimeError("block ingest has stopped".to_string()))
    }

    /// A peer sent a block. It never waits: when its lane is full the block
    /// is dropped, and a block the node needs is asked for again.
    pub fn deliver(&self, block: CheckedBlock, from: PeerNode) {
        let (lane, label) = match self.view.is_solicited(&block.block().block_hash) {
            true => (&self.awaited, "awaited"),
            false => (&self.unsolicited, "unsolicited"),
        };
        if let Err(error) = lane.try_send(Delivery { block, from }) {
            metrics::counter!(BLOCK_INGEST_LANE_DROPS_METRIC, "source" => BLOCK_INGEST_METRICS_SOURCE, "lane" => label)
                .increment(1);
            tracing::debug!(
                lane = label,
                "block ingest lane refused a delivery: {error}"
            );
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

    /// `peer` sent a block message that failed the integrity check. It never
    /// waits: when the owner's input is full the strike is dropped and counted.
    pub fn misbehaved(&self, peer: PeerNode, strike: Strike) {
        if self
            .inputs
            .try_send(IngestInput::Misbehaved { peer, strike })
            .is_err()
        {
            metrics::counter!(BLOCK_INGEST_LANE_DROPS_METRIC, "source" => BLOCK_INGEST_METRICS_SOURCE, "lane" => "strikes")
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
    /// The node's `max-number-of-parents`; `-1` is unlimited.
    pub max_number_of_parents: i32,
    pub state_root_fetch_tx: Option<mpsc::Sender<Blake2b256Hash>>,
    pub state_root_results: Option<mpsc::UnboundedReceiver<(Blake2b256Hash, StateFetchResult)>>,
}

/// The node's stores, as block ingest's state reads them.
pub struct Stores<'a> {
    pub dag: &'a BlockDagKeyValueStorage,
    pub block_store: &'a KeyValueBlockStore,
}

impl StoreReads for Stores<'_> {
    fn in_dag(&self, hash: &BlockHash) -> bool {
        match self.dag.contains(hash) {
            Ok(held) => held,
            // Unproven membership is treated as absent, as `is_satisfied`
            // treats unproven satisfaction.
            Err(error) => {
                tracing::warn!(
                    hash = %PrettyPrinter::build_string_bytes(hash),
                    %error,
                    "DAG membership check failed; the block is treated as absent"
                );
                false
            }
        }
    }

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

    fn unsatisfied(&self, hashes: &[BlockHash]) -> Vec<BlockHash> {
        match self.dag.unsatisfied_dependencies(hashes) {
            Ok(unsatisfied) => unsatisfied,
            // Unproven satisfaction is waited on; the insert observer
            // releases the waiter if a block is in the DAG after all.
            Err(error) => {
                tracing::warn!(%error, "dependency check failed; the block waits on its dependencies");
                hashes
                    .iter()
                    .filter(|hash| !self.in_dag(hash))
                    .cloned()
                    .collect()
            }
        }
    }

    fn pending_usage(&self) -> PendingUsage { self.block_store.pending_usage() }

    fn holds_pending(&self, hash: &BlockHash) -> bool { self.block_store.is_pending(hash) }

    fn bonded_in(&self, hash: &BlockHash) -> Option<HashSet<Validator>> {
        match self.dag.lookup(hash) {
            Ok(meta) => meta.map(|meta| meta.weight_map.keys().cloned().collect()),
            // An unreadable block's bonds are unknown; the block waits on its
            // main parent until its shape can be checked.
            Err(error) => {
                tracing::warn!(
                    hash = %PrettyPrinter::build_string_bytes(hash),
                    %error,
                    "bond lookup failed"
                );
                None
            }
        }
    }

    fn bonded_in_last_finalized(&self) -> Option<HashSet<Validator>> {
        self.bonded_in(&self.dag.last_finalized_block()?)
    }
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
        handle.view.clone(),
    ));

    let max_parents = usize::try_from(setup.max_number_of_parents).ok();
    let owner = Owner {
        state: IngestState::new(handle.view.clone()).with_max_parents(max_parents),
        view: handle.view.clone(),
        block_store: setup.block_store.clone(),
        dag: setup.dag.clone(),
        connections: setup.connections,
        local: local_tx,
        outbound: outbound_tx,
        work: work_tx,
        in_validation: HashMap::new(),
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
                Ok(Some(block)) => match check_off_runtime(block).await {
                    // A local lookup always gets a report, or its need would
                    // never reach the network.
                    Ok(checked) => attached.intake.admit(&checked, None).or_else(|error| {
                        tracing::warn!(
                            hash = %PrettyPrinter::build_string_bytes(&hash),
                            %error,
                            "intake of a stored block failed; asking the network"
                        );
                        Ok(IngestInput::LocalMiss { hash })
                    }),
                    // A bad copy this node stored would shadow the honest
                    // block: delete it, and ask the network.
                    Err((refused, _)) => {
                        if let Err(error) = block_store.discard_copy(&refused) {
                            tracing::warn!(
                                hash = %PrettyPrinter::build_string_bytes(&hash),
                                %error,
                                "could not delete a stored copy that failed the integrity check"
                            );
                        }
                        Ok(IngestInput::LocalMiss { hash })
                    }
                },
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

/// The integrity check of a stored copy, off the async runtime. A check that
/// cannot run counts as a malformed copy, so the block is asked for again.
async fn check_off_runtime(
    block: BlockMessage,
) -> Result<CheckedBlock, (Box<BlockMessage>, IntegrityFailure)> {
    let copy = block.clone();
    tokio::task::spawn_blocking(move || CheckedBlock::check(copy, BLOCK_INGEST_METRICS_SOURCE))
        .await
        .unwrap_or_else(|error| {
            tracing::error!(%error, "integrity check of a stored block failed to run");
            Err((Box::new(block), IntegrityFailure::Malformed))
        })
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
                tracing::info!(
                    "Block {} processing started.",
                    PrettyPrinter::build_string_bytes(&hash)
                );
                let attempt = async {
                    match kind {
                        Work::Validate => validation.validate(casper, &block).await,
                        Work::AdmitSettled => validation.admit_settled(casper, &block).await,
                    }
                };
                match AssertUnwindSafe(attempt).catch_unwind().await {
                    Ok(outcome) => outcome,
                    Err(_) => {
                        tracing::error!(
                            hash = %PrettyPrinter::build_string_bytes(&hash),
                            "block validation panicked; counted as a failed attempt"
                        );
                        super::ingest_state::WorkOutcome::Failed
                    }
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

/// Send queued requests, `OUTBOUND_CONCURRENCY` at a time, skipping any for a
/// block no longer needed by the time its turn comes.
async fn run_outbound<T: TransportLayer + Send + Sync>(
    outbound: mpsc::Receiver<Outbound>,
    transport: Arc<T>,
    connections: ConnectionsCell,
    conf: RPConf,
    view: IngestView,
) {
    ReceiverStream::new(outbound)
        .for_each_concurrent(OUTBOUND_CONCURRENCY, |request| {
            let (transport, connections, conf, view) = (&transport, &connections, &conf, &view);
            async move {
                let hash = match &request {
                    Outbound::Request { hash, .. } | Outbound::Broadcast(hash) => hash.clone(),
                };
                if !view.is_awaited(&hash) {
                    return;
                }
                let result = match request {
                    Outbound::Request { hash, peer } => {
                        transport.request_for_block(conf, &peer, hash).await
                    }
                    Outbound::Broadcast(hash) => {
                        transport
                            .broadcast_has_block_request(connections, conf, &hash)
                            .await
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
        })
        .await;
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
    /// Blocks sent to validation: when, and whether a stuck warning went out.
    in_validation: HashMap<BlockHash, (u64, bool)>,
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
            // Peer traffic comes last: a flood of inputs must not hold off the
            // tick, which sends every request, or the node's own results.
            let input = tokio::select! {
                biased;
                Some(hash) = inserted.recv() => IngestInput::Inserted { hash },
                _ = tick.tick() => {
                    metrics::gauge!(BLOCK_INGEST_INSERTS_PENDING_METRIC, "source" => BLOCK_INGEST_METRICS_SOURCE)
                        .set(inserted.len() as f64);
                    metrics::gauge!(BLOCK_INGEST_REQUESTS_OUTSTANDING_METRIC, "source" => BLOCK_INGEST_METRICS_SOURCE)
                        .set(self.state.outstanding_requests(self.now_ms()) as f64);
                    self.observe_validations();
                    self.tick_input()
                }
                Some((root, result)) = async {
                    match state_root_results.as_mut() {
                        Some(results) => results.recv().await,
                        None => std::future::pending().await,
                    }
                } => IngestInput::StateFetch { root, result },
                Some(input) = inputs.recv() => input,
                else => return,
            };
            if let IngestInput::Outcome { hash, .. } = &input {
                self.in_validation.remove(hash);
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
            Effect::Reserve { hash, sender, size } => {
                self.block_store
                    .pending_blocks()
                    .reserve(&hash, &sender, size);
                None
            }
            Effect::ReleaseReservation(hash) => {
                self.block_store.pending_blocks().release(&hash);
                None
            }
            Effect::DiscardStored(hash) => {
                // A body whose DAG membership cannot be read is kept.
                if matches!(self.dag.contains(&hash), Ok(false)) {
                    if let Err(error) = self.block_store.discard_stored(&hash) {
                        tracing::warn!(
                            hash = %PrettyPrinter::build_string_bytes(&hash),
                            %error,
                            "could not delete the stored body of a block that left block ingest"
                        );
                    }
                }
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
        while self.in_validation.len() < self.width {
            let Some(work) = self.state.next_dispatch() else {
                return;
            };
            let hash = work.0.clone();
            if self.work.send(work).await.is_err() {
                return;
            }
            self.in_validation.insert(hash, (self.now_ms(), false));
        }
    }

    /// Record how long the oldest validation has run, and warn once for each
    /// block whose validation has held a slot for longer than
    /// `STUCK_VALIDATION_WARN_MS`.
    fn observe_validations(&mut self) {
        let now_ms = self.now_ms();
        let mut oldest_ms = None;
        for (hash, (since_ms, warned)) in self.in_validation.iter_mut() {
            let elapsed_ms = now_ms.saturating_sub(*since_ms);
            oldest_ms = oldest_ms.max(Some(elapsed_ms));
            if !*warned && elapsed_ms >= STUCK_VALIDATION_WARN_MS {
                *warned = true;
                tracing::warn!(
                    target: super::LOG_TARGET,
                    hash = %PrettyPrinter::build_string_bytes(hash),
                    elapsed_ms,
                    "block validation still running; it holds one validation slot"
                );
            }
        }
        record_oldest_validation(oldest_ms);
    }

    fn record_gauges(&self) {
        let gauges = [
            (BLOCK_INGEST_HELD_METRIC, self.state.held()),
            (BLOCK_INGEST_DISK_ENTRIES_METRIC, self.state.disk_entries()),
            (BLOCK_INGEST_NEEDS_METRIC, self.state.block_needs()),
            (
                BLOCK_INGEST_UNCITED_NEEDS_METRIC,
                self.state.uncited_needs(),
            ),
            (BLOCK_INGEST_ROOT_NEEDS_METRIC, self.state.root_needs()),
            (
                BLOCK_INGEST_PEER_REPUTATION_IGNORED_PEERS_METRIC,
                self.state.ignored_peers(self.now_ms()),
            ),
        ];
        for (name, value) in gauges {
            metrics::gauge!(name, "source" => BLOCK_INGEST_METRICS_SOURCE).set(value as f64);
        }
        record_processing(self.in_validation.len(), self.state.ready());
    }
}

/// Validations running and ready blocks waiting for a slot, under the
/// block-processing names and source that monitors and soak scripts read.
fn record_processing(active: usize, ready: usize) {
    for (name, value) in [
        (BLOCK_PROCESSING_ACTIVE_METRIC, active),
        (BLOCK_PROCESSING_QUEUE_PENDING_METRIC, ready),
        (BLOCK_PROCESSING_IN_FLIGHT_METRIC, active + ready),
    ] {
        metrics::gauge!(name, "source" => BLOCK_PROCESSOR_METRICS_SOURCE).set(value as f64);
    }
}

/// How long the oldest running validation has run; zero while none runs.
fn record_oldest_validation(oldest_ms: Option<u64>) {
    metrics::gauge!(
        BLOCK_PROCESSING_IN_FLIGHT_OLDEST_AGE_METRIC,
        "source" => BLOCK_PROCESSOR_METRICS_SOURCE
    )
    .set(oldest_ms.map_or(0.0, |ms| ms as f64 / 1_000.0));
}
