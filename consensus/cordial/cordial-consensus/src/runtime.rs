use std::path::PathBuf;

use consensus_api::{
    AdapterContext, Capabilities, ConsensusAdapter, ConsensusCommand, ConsensusError,
    ProtocolDescriptor,
};
use consensus_runtime::{PreparedConsensus, RuntimeBuilder, RuntimeConfig};
use cordial_miners_core::crypto::CONTENT_HASH_VERSION;

use crate::{Chain, DurableBlocklace, Error, MAX_PACKET_BYTES, StoreConfig};

pub const BLOCK_PACKET_KIND: &str = "cordial/block/v2";

pub struct CordialIngressAdapter {
    path: PathBuf,
    chain: Chain,
    store_config: StoreConfig,
    executor: Option<Box<dyn crate::execution::CommittedExecutor>>,
}

impl CordialIngressAdapter {
    pub fn prepare(
        path: PathBuf,
        chain: Chain,
        store_config: StoreConfig,
        runtime_config: RuntimeConfig,
    ) -> Result<PreparedConsensus, ConsensusError> {
        Self::prepare_inner(path, chain, store_config, runtime_config, None)
    }

    pub fn prepare_with_executor(
        path: PathBuf,
        chain: Chain,
        store_config: StoreConfig,
        runtime_config: RuntimeConfig,
        executor: Box<dyn crate::execution::CommittedExecutor>,
    ) -> Result<PreparedConsensus, ConsensusError> {
        Self::prepare_inner(path, chain, store_config, runtime_config, Some(executor))
    }

    fn prepare_inner(
        path: PathBuf,
        chain: Chain,
        store_config: StoreConfig,
        mut runtime_config: RuntimeConfig,
        executor: Option<Box<dyn crate::execution::CommittedExecutor>>,
    ) -> Result<PreparedConsensus, ConsensusError> {
        runtime_config.max_payload_bytes = runtime_config.max_payload_bytes.min(MAX_PACKET_BYTES);
        let builder = RuntimeBuilder::new(
            ProtocolDescriptor {
                id: "cordial-miners".into(),
                version: CONTENT_HASH_VERSION,
                capabilities: Capabilities::NONE,
            },
            runtime_config,
        )?;
        Ok(builder.build(Box::new(Self {
            path,
            chain,
            store_config,
            executor,
        })))
    }
}

fn fatal(error: Error) -> ConsensusError {
    ConsensusError::Protocol(error.to_string())
}

fn reject_command(command: ConsensusCommand, stopping: bool) {
    let unsupported = |name| {
        if stopping {
            ConsensusError::Stopped
        } else {
            ConsensusError::UnsupportedCapability(name)
        }
    };
    match command {
        ConsensusCommand::Submit { reply, .. } => {
            let _ = reply.send(Err(unsupported("submit")));
        }
        ConsensusCommand::Propose { reply, .. } => {
            let _ = reply.send(Err(unsupported("propose")));
        }
        ConsensusCommand::Finalized { reply } => {
            let _ = reply.send(Err(unsupported("finalized progress")));
        }
    }
}

#[async_trait::async_trait]
impl ConsensusAdapter for CordialIngressAdapter {
    async fn run(self: Box<Self>, mut context: AdapterContext) -> Result<(), ConsensusError> {
        if context.control.is_cancelled() {
            return Ok(());
        }
        let mut state =
            DurableBlocklace::open(&self.path, self.chain, self.store_config).map_err(fatal)?;
        if context.control.is_cancelled() {
            return Ok(());
        }
        state.advance_output().map_err(fatal)?;
        if let Some(executor) = &self.executor {
            while !context.control.is_cancelled()
                && executor.execute_next(&mut state).await.map_err(fatal)?
            {}
        }
        if context.control.is_cancelled() {
            return Ok(());
        }
        context.control.ready();
        loop {
            tokio::select! {
                biased;
                _ = context.control.cancelled() => {
                    context.control.draining();
                    context.commands.close();
                    context.packets.close();
                    while let Ok(command) = context.commands.try_recv() { reject_command(command, true); }
                    while let Ok(request) = context.packets.try_recv() { let _ = request.reply.send(Err(ConsensusError::Stopped)); }
                    return Ok(());
                }
                Some(command) = context.commands.recv() => reject_command(command, false),
                Some(request) = context.packets.recv() => {
                    if request.reply.is_closed() { continue; }
                    if request.packet.kind != BLOCK_PACKET_KIND {
                        let _ = request.reply.send(Err(ConsensusError::InvalidInput("unsupported Cordial packet kind".into())));
                        continue;
                    }
                    match state.admit(&request.packet.payload) {
                        Ok(_) => {
                            if let Err(error) = state.advance_output() {
                                let error = fatal(error);
                                let _ = request.reply.send(Err(error.clone()));
                                return Err(error);
                            }
                            if let Some(executor) = &self.executor {
                                while !context.control.is_cancelled() {
                                    match executor.execute_next(&mut state).await {
                                        Ok(true) => {},
                                        Ok(false) => break,
                                        Err(error) => {
                                            let error = fatal(error);
                                            let _ = request.reply.send(Err(error.clone()));
                                            return Err(error);
                                        }
                                    }
                                }
                            }
                            if context.control.is_cancelled() {
                                let _ = request.reply.send(Err(ConsensusError::Stopped));
                                continue;
                            }
                            let _ = request.reply.send(Ok(()));
                        }
                        Err(Error::Capacity) => { let _ = request.reply.send(Err(ConsensusError::QueueFull)); }
                        Err(error @ (Error::Packet(_) | Error::Authentication | Error::ChainMismatch | Error::UnknownValidator | Error::Native(_))) => {
                            let _ = request.reply.send(Err(ConsensusError::InvalidInput(error.to_string())));
                        }
                        Err(error) => {
                            let error = fatal(error);
                            let _ = request.reply.send(Err(error.clone()));
                            return Err(error);
                        }
                    }
                }
            }
        }
    }
}
