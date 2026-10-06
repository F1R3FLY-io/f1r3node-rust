mod application;
pub mod api_compat;
pub(crate) mod assembly;
mod ingress;

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::blocks::block_processor::BlockProcessor;
use casper::rust::blocks::proposer::proposer::{ProductionProposer, ProposerResult};
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::engine::casper_launch::CasperLaunch;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::errors::CasperError;
use casper::rust::state::instances::ProposerState;
use casper::rust::ProposeFunction;
use comm::rust::p2p::packet_handler::PacketHandler;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::transport::transport_layer::TransportLayer;
use consensus_api::{AdapterContext, ConsensusAdapter, ConsensusCommand, ConsensusError, ObjectId};
use consensus_runtime::TaskGroup;
use futures::future::BoxFuture;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use prost::Message;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
use tokio::sync::{mpsc, oneshot, watch, RwLock};

use super::manifest::ManifestGuard;
use crate::rust::instances::block_processor_instance::BlockProcessorInstance;
use crate::rust::instances::heartbeat_proposer::HeartbeatProposer;
use crate::rust::instances::proposer_instance::ProposerInstance;

type ProposerQueueEntry = (
    Arc<dyn Casper + Send + Sync>,
    bool,
    oneshot::Sender<ProposerResult>,
    u8,
);
type BlockQueueEntry = (Arc<dyn MultiParentCasper + Send + Sync>, BlockMessage);
type CasperLoop =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>> + Send + Sync>;
type NativeTask = BoxFuture<'static, Result<(), CasperError>>;

pub(crate) struct CasperConsensusAdapter<T: TransportLayer + Send + Sync + 'static> {
    launch: Arc<dyn CasperLaunch + Send + Sync>,
    native_tasks: Arc<consensus_runtime::TaskScope>,
    task_spawner: casper::rust::background_tasks::BackgroundTaskSpawner,
    engine: EngineCell,
    initialize: CasperLoop,
    loops: Vec<(&'static str, CasperLoop)>,
    background: Vec<(&'static str, NativeTask)>,
    packet_handler: Arc<dyn PacketHandler>,
    proposer: Option<ProductionProposer<T>>,
    proposer_rx: mpsc::Receiver<ProposerQueueEntry>,
    proposer_tx: mpsc::Sender<ProposerQueueEntry>,
    proposer_pending: Arc<AtomicUsize>,
    proposer_capacity: usize,
    proposer_state: Option<Arc<RwLock<ProposerState>>>,
    block_processor: BlockProcessor<T>,
    block_state: Arc<dashmap::DashSet<BlockHash>>,
    block_tx: mpsc::Sender<BlockQueueEntry>,
    block_rx: mpsc::Receiver<BlockQueueEntry>,
    propose: Option<Arc<ProposeFunction>>,
    validator: Option<casper::rust::validator_identity::ValidatorIdentity>,
    heartbeat_conf: casper::rust::casper_conf::HeartbeatConf,
    max_parents: i32,
    heartbeat_signal: casper::rust::heartbeat_signal::HeartbeatSignalRef,
    standalone: bool,
    autopropose: bool,
    shard: String,
    connections: ConnectionsCell,
    events: F1r3flyEvents,
    ready: Arc<AtomicBool>,
    manifest: ManifestGuard,
    block_store: KeyValueBlockStore,
    dag: BlockDagKeyValueStorage,
    store_manager: Box<dyn KeyValueStoreManager>,
}

fn native_error(error: impl std::fmt::Display) -> ConsensusError {
    ConsensusError::Protocol(error.to_string())
}

struct NativeScopeGuard {
    tasks: Arc<consensus_runtime::TaskScope>,
    ready: Arc<AtomicBool>,
}
impl Drop for NativeScopeGuard {
    fn drop(&mut self) {
        self.ready.store(false, Ordering::Release);
        self.tasks.abort();
    }
}

struct OwnedTask<T>(tokio::task::JoinHandle<T>);
impl<T> Drop for OwnedTask<T> {
    fn drop(&mut self) { self.0.abort(); }
}

#[async_trait::async_trait]
impl<T: TransportLayer + Send + Sync + Clone + 'static> ConsensusAdapter
    for CasperConsensusAdapter<T>
{
    async fn run(self: Box<Self>, mut context: AdapterContext) -> Result<(), ConsensusError> {
        let Self {
            launch,
            native_tasks,
            task_spawner,
            engine,
            initialize,
            loops,
            background,
            packet_handler,
            proposer,
            proposer_rx,
            proposer_tx,
            proposer_pending,
            proposer_capacity,
            proposer_state,
            block_processor,
            block_state,
            block_tx,
            block_rx,
            propose,
            validator,
            heartbeat_conf,
            max_parents,
            heartbeat_signal,
            standalone,
            autopropose,
            shard,
            connections,
            events,
            ready,
            manifest,
            block_store,
            dag,
            mut store_manager,
        } = *self;
        let _scope_guard = NativeScopeGuard {
            tasks: native_tasks.clone(),
            ready: ready.clone(),
        };
        tokio::select! {
            result = launch.launch() => result.map_err(native_error)?,
            _ = context.control.cancelled() => return Ok(()),
        }
        let mut tasks = TaskGroup::default();
        let mut workers = TaskGroup::default();
        let mut packet_requests = TaskGroup::default();
        let mut command_requests = TaskGroup::default();
        let (stop_workers, worker_shutdown) = watch::channel(false);
        for (name, task) in background {
            tasks.spawn(name, async move { task.await.map_err(native_error) });
        }
        for (name, operation) in loops {
            tasks.spawn(name, async move {
                loop {
                    if let Err(error) = operation().await {
                        tracing::warn!(task = name, %error, "Consensus maintenance iteration failed");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            });
        }
        let (mut block_results, block_task) = BlockProcessorInstance::new(
            (block_rx, block_tx),
            Arc::new(block_processor),
            block_state,
        )
        .supervised(worker_shutdown.clone());
        workers.spawn("block processor", async move {
            block_task.await.map_err(native_error)
        });
        workers.spawn("block results", async move {
            while block_results.recv().await.is_some() {}
            Ok(())
        });
        if let (Some(proposer), Some(state)) = (proposer, proposer_state) {
            let (mut results, task) = ProposerInstance::new(
                (proposer_rx, proposer_tx),
                Arc::new(tokio::sync::Mutex::new(proposer)),
                state,
                proposer_pending,
                proposer_capacity,
            )
            .supervised(worker_shutdown);
            workers.spawn("proposer", async move { task.await.map_err(native_error) });
            workers.spawn("proposal results", async move {
                while results.recv().await.is_some() {}
                Ok(())
            });
        }
        if let Some(validator) = validator {
            if let Some(task) = HeartbeatProposer::create(
                Arc::new(engine.clone()),
                propose.clone(),
                validator,
                heartbeat_conf,
                max_parents,
                heartbeat_signal,
                standalone,
            ) {
                let mut task = OwnedTask(task);
                tasks.spawn("heartbeat", async move {
                    (&mut task.0).await.map_err(native_error)
                });
            }
        }
        let initialized = Arc::new(AtomicBool::new(false));
        let initialization_done = initialized.clone();
        tasks.spawn("initialization", async move {
            if !standalone {
                loop {
                    if !connections.read().map_err(native_error)?.is_empty() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
            initialize().await.map_err(native_error)?;
            events.seal_startup();
            initialization_done.store(true, Ordering::Release);
            std::future::pending().await
        });
        let ready_engine = engine.clone();
        let ready_flag = ready.clone();
        let control = context.control.clone();
        tasks.spawn("readiness", async move {
            loop {
                if initialized.load(Ordering::Acquire)
                    && ready_engine.get().await.with_casper().is_some()
                {
                    let genesis = match dag.genesis_hash().map_err(native_error)? {
                        Some(hash) => hash,
                        None => {
                            let approved = block_store
                                .get_approved_block()
                                .map_err(native_error)?
                                .ok_or_else(|| {
                                    native_error("Ready Casper has no approved block")
                                })?;
                            if approved.candidate.block.body.state.block_number != 0 {
                                return Err(native_error(
                                    "Cannot identify genesis from a truncated Casper store",
                                ));
                            }
                            approved.candidate.block.block_hash
                        }
                    };
                    manifest.record(&genesis).map_err(native_error)?;
                    ready_flag.store(true, Ordering::Release);
                    control.ready();
                    std::future::pending::<()>().await;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
        let result = loop {
            tokio::select! {
                _ = context.control.cancelled() => break Ok(()),
                outcome = native_tasks.join_next() => { if let Err(error) = outcome { break Err(error); } },
                outcome = tasks.join_next() => break match outcome {
                    Err(error) => Err(error),
                    Ok(task) => Err(ConsensusError::TaskFailed { task, reason: "completed unexpectedly".into() }),
                },
                outcome = workers.join_next() => break match outcome {
                    Err(error) => Err(error),
                    Ok(task) => Err(ConsensusError::TaskFailed { task, reason: "completed unexpectedly".into() }),
                },
                outcome = packet_requests.join_next() => { if let Err(error) = outcome { break Err(error); } },
                outcome = command_requests.join_next() => { if let Err(error) = outcome { break Err(error); } },
                Some(request) = context.packets.recv(), if packet_requests.len() < 32 => {
                    let handler = packet_handler.clone();
                    packet_requests.spawn("packet", async move {
                        if request.reply.is_closed() { return Ok(()); }
                        let packet = request.packet;
                        let peer = PeerNode { id: NodeIdentifier { key: packet.peer.id.into() }, endpoint: Endpoint::new(packet.peer.host, packet.peer.tcp_port, packet.peer.udp_port) };
                        let packet = models::routing::Packet { type_id: packet.kind, content: packet.payload.into() };
                        let result = handler.handle_packet(&peer, &packet).await.map_err(native_error);
                        let _ = request.reply.send(result);
                        Ok(())
                    });
                },
                Some(command) = context.commands.recv(), if command_requests.len() < 16 => {
                    let engine = engine.clone();
                    let propose = propose.clone();
                    let shard = shard.clone();
                    let task_spawner = task_spawner.clone();
                    command_requests.spawn("command", async move {
                        use casper::rust::api::block_api::BlockAPI;
                        match command {
                            ConsensusCommand::Submit { payload, reply } => {
                                if reply.is_closed() { return Ok(()); }
                                let result = match models::casper::DeployDataProto::decode(payload.as_slice()) {
                                    Ok(proto) => match DeployData::from_proto(proto) {
                                        Ok(deploy) => BlockAPI::deploy_supervised(&engine, deploy, &if autopropose { propose.clone() } else { None }, propose.is_none(), &shard, &Some(task_spawner)).await.map_err(api_compat::command_error),
                                        Err(error) => Err(native_error(error)),
                                    },
                                    Err(error) => Err(native_error(error)),
                                };
                                let _ = reply.send(result);
                            }
                            ConsensusCommand::Propose { is_async, reply } => {
                                if reply.is_closed() { return Ok(()); }
                                let result = match propose {
                                    Some(propose) => BlockAPI::create_block(&engine, &propose, is_async).await.map_err(api_compat::command_error),
                                    None => Err(ConsensusError::UnsupportedCapability("propose")),
                                };
                                let _ = reply.send(result);
                            }
                            ConsensusCommand::Finalized { reply } => {
                                if reply.is_closed() { return Ok(()); }
                                let result = match engine.get().await.with_casper() {
                                    Some(casper) => casper.last_finalized_block().await.map(|block| ObjectId(block.block_hash.to_vec())).map_err(native_error),
                                    None => Err(ConsensusError::NotReady),
                                };
                                let _ = reply.send(result);
                            }
                        }
                        Ok(())
                    });
                },
            }
        };
        ready.store(false, Ordering::Release);
        context.control.draining();
        context.commands.close();
        context.packets.close();
        while context.commands.try_recv().is_ok() {}
        while context.packets.try_recv().is_ok() {}
        let drain = async {
            tasks.shutdown().await;
            while !packet_requests.is_empty() {
                packet_requests.join_next().await?;
            }
            while !command_requests.is_empty() {
                command_requests.join_next().await?;
            }
            native_tasks.shutdown().await;
            stop_workers.send_replace(true);
            while !workers.is_empty() {
                workers.join_next().await?;
            }
            store_manager.shutdown().await.map_err(native_error)?;
            Ok::<(), ConsensusError>(())
        };
        let drained = tokio::time::timeout(context.drain_timeout, drain)
            .await
            .map_err(|_| ConsensusError::ShutdownTimeout)
            .and_then(|result| result);
        result.and(drained)
    }
}
