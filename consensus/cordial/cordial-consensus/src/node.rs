use consensus_api::{
    AdapterContext, Capabilities, ConsensusAdapter, ConsensusCommand, ConsensusError, Peer,
    ProtocolDescriptor,
};
use consensus_runtime::{PreparedConsensus, RuntimeBuilder, RuntimeConfig};
use cordial_miners_core::BlockIdentity;
use k256::ecdsa::SigningKey;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};

use crate::{
    Application, BLOCK_PACKET_KIND, Chain, DurableBlocklace, EquivocationRecord, Error,
    ExecutionReceipt, HistoryPage, MAX_PACKET_BYTES, StoreConfig, decode, encode,
};

pub const SYNC_PACKET_KIND: &str = "cordial/sync/v1";
pub const SUBMIT_PACKET_KIND: &str = "cordial/submit/v1";

#[async_trait::async_trait]
pub trait PeerNetwork: Send + Sync + 'static {
    fn peers(&self) -> Vec<Peer>;
    async fn send(&self, peer: &Peer, kind: &str, payload: Vec<u8>) -> Result<(), ConsensusError>;
}

pub struct NodeOptions {
    pub key: Option<SigningKey>,
    pub tick: Duration,
    pub max_peers: usize,
}

impl Default for NodeOptions {
    fn default() -> Self {
        Self {
            key: None,
            tick: Duration::from_secs(1),
            max_peers: 64,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Progress {
    pub chain: [u8; 32],
    pub admitted: u64,
    pub committed: u64,
    pub executed: u64,
    pub state_root: [u8; 32],
    pub pending: usize,
}

enum Query {
    Progress(oneshot::Sender<Result<Progress, ConsensusError>>),
    Output(
        u64,
        usize,
        oneshot::Sender<Result<Vec<BlockIdentity>, ConsensusError>>,
    ),
    Object(
        BlockIdentity,
        oneshot::Sender<Result<Option<Vec<u8>>, ConsensusError>>,
    ),
    Receipt(
        u64,
        oneshot::Sender<Result<Option<ExecutionReceipt>, ConsensusError>>,
    ),
    Equivocations(oneshot::Sender<Result<Vec<EquivocationRecord>, ConsensusError>>),
}

#[derive(Clone)]
pub struct CordialQueries {
    sender: mpsc::Sender<Query>,
    timeout: Duration,
}

impl CordialQueries {
    async fn request<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<Result<T, ConsensusError>>) -> Query,
    ) -> Result<T, ConsensusError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .try_send(make(tx))
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => ConsensusError::QueueFull,
                mpsc::error::TrySendError::Closed(_) => ConsensusError::Stopped,
            })?;
        tokio::time::timeout(self.timeout, rx)
            .await
            .map_err(|_| ConsensusError::DeadlineExceeded)?
            .map_err(|_| ConsensusError::Stopped)?
    }
    pub async fn progress(&self) -> Result<Progress, ConsensusError> {
        self.request(Query::Progress).await
    }
    pub async fn output(
        &self,
        start: u64,
        limit: usize,
    ) -> Result<Vec<BlockIdentity>, ConsensusError> {
        self.request(|reply| Query::Output(start, limit, reply))
            .await
    }
    pub async fn object(&self, id: BlockIdentity) -> Result<Option<Vec<u8>>, ConsensusError> {
        self.request(|reply| Query::Object(id, reply)).await
    }
    pub async fn receipt(&self, index: u64) -> Result<Option<ExecutionReceipt>, ConsensusError> {
        self.request(|reply| Query::Receipt(index, reply)).await
    }
    pub async fn equivocations(&self) -> Result<Vec<EquivocationRecord>, ConsensusError> {
        self.request(Query::Equivocations).await
    }
}

#[derive(Serialize, Deserialize)]
struct SyncEnvelope {
    chain: [u8; 32],
    message: SyncMessage,
}

#[derive(Serialize, Deserialize)]
struct SubmitEnvelope {
    chain: [u8; 32],
    payload: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
enum SyncMessage {
    Request(u64),
    Page(HistoryPage),
    Reset,
}

pub struct CordialNode {
    path: PathBuf,
    chain: Chain,
    store_config: StoreConfig,
    options: NodeOptions,
    network: Arc<dyn PeerNetwork>,
    executor: Option<Box<dyn Application>>,
    queries: mpsc::Receiver<Query>,
}

type Outgoing = (Peer, &'static str, Vec<u8>);

fn classify(error: Error) -> ConsensusError {
    match error {
        Error::Capacity => ConsensusError::QueueFull,
        Error::Packet(_)
        | Error::Authentication
        | Error::ChainMismatch
        | Error::UnknownValidator
        | Error::Native(_) => ConsensusError::InvalidInput(error.to_string()),
        error => ConsensusError::Protocol(error.to_string()),
    }
}

impl CordialNode {
    pub fn prepare(
        path: PathBuf,
        chain: Chain,
        store_config: StoreConfig,
        mut runtime_config: RuntimeConfig,
        options: NodeOptions,
        network: Arc<dyn PeerNetwork>,
        executor: Option<Box<dyn Application>>,
    ) -> Result<(PreparedConsensus, CordialQueries), ConsensusError> {
        if options.tick < Duration::from_millis(10)
            || options.max_peers == 0
            || options.max_peers > 256
        {
            return Err(ConsensusError::InvalidInput(
                "invalid Cordial scheduling limits".into(),
            ));
        }
        if options.key.as_ref().is_some_and(|key| {
            !chain.weights().contains_key(&cordial_miners_core::NodeId(
                key.verifying_key().to_sec1_bytes().to_vec(),
            ))
        }) {
            return Err(ConsensusError::InvalidInput(
                "validator key is not in the fixed committee".into(),
            ));
        }
        runtime_config.max_payload_bytes = runtime_config.max_payload_bytes.min(MAX_PACKET_BYTES);
        let command_capacity = runtime_config.command_capacity;
        let timeout = runtime_config.request_timeout;
        let mut capabilities = if options.key.is_some() {
            Capabilities::PROPOSE
        } else {
            Capabilities::NONE
        };
        if executor.is_some() && options.key.is_some() {
            capabilities = capabilities.union(Capabilities::SUBMIT);
        }
        let builder = RuntimeBuilder::new(
            ProtocolDescriptor {
                id: "cordial-miners".into(),
                version: 2,
                capabilities,
            },
            runtime_config,
        )?;
        let (sender, queries) = mpsc::channel(command_capacity);
        let handle = CordialQueries { sender, timeout };
        Ok((
            builder.build(Box::new(Self {
                path,
                chain,
                store_config,
                options,
                network,
                executor,
                queries,
            })),
            handle,
        ))
    }

    async fn advance(
        &self,
        state: &mut DurableBlocklace,
        context: &AdapterContext,
    ) -> Result<(), ConsensusError> {
        state.advance_output().map_err(classify)?;
        if let Some(executor) = &self.executor {
            while !context.control.is_cancelled()
                && executor.execute_next(state).await.map_err(classify)?
            {}
        }
        Ok(())
    }

    fn propose(
        &self,
        state: &mut DurableBlocklace,
        outbound: &mpsc::Sender<Outgoing>,
    ) -> Result<String, ConsensusError> {
        let key = self
            .options
            .key
            .as_ref()
            .ok_or(ConsensusError::UnsupportedCapability("propose"))?;
        let submissions = state.submissions(64).map_err(classify)?;
        let (payload, included) = if let Some(executor) = &self.executor {
            executor.proposal_payload(&submissions).map_err(classify)?
        } else {
            (vec![], 0)
        };
        if included > submissions.len() {
            return Err(ConsensusError::Protocol(
                "application included unknown submissions".into(),
            ));
        }
        let Some(block) = state.propose(key, payload).map_err(classify)? else {
            return Err(ConsensusError::NotReady);
        };
        state
            .mark_proposed(&block.identity, &submissions[..included])
            .map_err(classify)?;
        let packet = self.chain.encode_block(&block).map_err(classify)?;
        for peer in self
            .network
            .peers()
            .into_iter()
            .take(self.options.max_peers)
        {
            let _ = outbound.try_send((peer, BLOCK_PACKET_KIND, packet.clone()));
        }
        Ok(hex::encode(encode(&block.identity).map_err(classify)?))
    }

    fn submit(
        &self,
        state: &mut DurableBlocklace,
        payload: &[u8],
        outbound: &mpsc::Sender<Outgoing>,
    ) -> Result<String, ConsensusError> {
        if self.options.key.is_none() {
            return Err(ConsensusError::UnsupportedCapability("submit"));
        }
        let executor = self
            .executor
            .as_ref()
            .ok_or(ConsensusError::UnsupportedCapability("submit"))?;
        let submission = executor.validate_submission(payload).map_err(classify)?;
        let id = hex::encode(submission.id);
        if state.submit(submission.clone()).map_err(classify)? {
            let packet = encode(&SubmitEnvelope {
                chain: self.chain.fingerprint(),
                payload: submission.payload,
            })
            .map_err(classify)?;
            for peer in self
                .network
                .peers()
                .into_iter()
                .take(self.options.max_peers)
            {
                let _ = outbound.try_send((peer, SUBMIT_PACKET_KIND, packet.clone()));
            }
        }
        Ok(id)
    }

    fn send_sync(
        &self,
        outbound: &mpsc::Sender<Outgoing>,
        peer: Peer,
        message: SyncMessage,
    ) -> Result<(), ConsensusError> {
        let bytes = encode(&SyncEnvelope {
            chain: self.chain.fingerprint(),
            message,
        })
        .map_err(classify)?;
        let _ = outbound.try_send((peer, SYNC_PACKET_KIND, bytes));
        Ok(())
    }

    fn receive_sync(
        &self,
        state: &mut DurableBlocklace,
        peer: Peer,
        bytes: &[u8],
        cursors: &mut BTreeMap<Vec<u8>, u64>,
        outbound: &mpsc::Sender<Outgoing>,
    ) -> Result<(), ConsensusError> {
        let envelope: SyncEnvelope = decode(bytes).map_err(classify)?;
        if envelope.chain != self.chain.fingerprint() {
            return Err(classify(Error::ChainMismatch));
        }
        match envelope.message {
            SyncMessage::Request(start) => {
                let message = if start > state.admitted_count() {
                    SyncMessage::Reset
                } else {
                    SyncMessage::Page(state.history_page(start, 32).map_err(classify)?)
                };
                self.send_sync(outbound, peer, message)
            }
            SyncMessage::Reset => {
                if let Some(cursor) = cursors.get_mut(&peer.id) {
                    *cursor = 0;
                }
                Ok(())
            }
            SyncMessage::Page(page) => {
                let Some(cursor) = cursors.get_mut(&peer.id) else {
                    return Err(ConsensusError::InvalidInput(
                        "unsolicited history page".into(),
                    ));
                };
                if page.start != *cursor {
                    return Ok(());
                }
                if page.packets.len() > 32
                    || page.next != page.start.saturating_add(page.packets.len() as u64)
                    || page.next > page.total
                {
                    return Err(ConsensusError::InvalidInput("invalid history page".into()));
                }
                for packet in page.packets {
                    state.admit(&packet).map_err(classify)?;
                }
                *cursor = page.next;
                Ok(())
            }
        }
    }

    fn query(&self, state: &DurableBlocklace, query: Query) {
        match query {
            Query::Progress(reply) => {
                let checkpoint = state.execution_checkpoint();
                let result = state
                    .pending_count()
                    .map(|pending| Progress {
                        chain: self.chain.fingerprint(),
                        admitted: state.admitted_count(),
                        committed: state.committed_count(),
                        executed: checkpoint.next_index,
                        state_root: checkpoint.state,
                        pending,
                    })
                    .map_err(classify);
                let _ = reply.send(result);
            }
            Query::Output(start, limit, reply) => {
                let _ = reply.send(state.output_page(start, limit).map_err(classify));
            }
            Query::Object(id, reply) => {
                let _ = reply.send(
                    state
                        .get(&id)
                        .and_then(|block| {
                            block
                                .map(|block| self.chain.encode_block(&block))
                                .transpose()
                        })
                        .map_err(classify),
                );
            }
            Query::Receipt(index, reply) => {
                let _ = reply.send(state.execution_receipt(index).map_err(classify));
            }
            Query::Equivocations(reply) => {
                let _ = reply.send(state.equivocations(64).map_err(classify));
            }
        }
    }
}

#[async_trait::async_trait]
impl ConsensusAdapter for CordialNode {
    async fn run(mut self: Box<Self>, mut context: AdapterContext) -> Result<(), ConsensusError> {
        if context.control.is_cancelled() {
            return Ok(());
        }
        let mut state =
            DurableBlocklace::open(&self.path, self.chain.clone(), self.store_config.clone())
                .map_err(classify)?;
        self.advance(&mut state, &context).await?;
        if context.control.is_cancelled() {
            return Ok(());
        }
        let (outbound, mut messages) = mpsc::channel::<Outgoing>(32);
        let mut tasks = tokio::task::JoinSet::new();
        let network = self.network.clone();
        tasks.spawn(async move {
            while let Some((peer, kind, payload)) = messages.recv().await {
                let _ = tokio::time::timeout(
                    Duration::from_secs(2),
                    network.send(&peer, kind, payload),
                )
                .await;
            }
        });
        if let Some(key) = &self.options.key {
            if let Some(block) = state.propose(key, vec![]).map_err(classify)? {
                let packet = self.chain.encode_block(&block).map_err(classify)?;
                for peer in self
                    .network
                    .peers()
                    .into_iter()
                    .take(self.options.max_peers)
                {
                    let _ = outbound.try_send((peer, BLOCK_PACKET_KIND, packet.clone()));
                }
                self.advance(&mut state, &context).await?;
            }
        }
        let mut cursors = BTreeMap::new();
        let mut interval = tokio::time::interval(self.options.tick);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        context.control.ready();
        let result = async {
            loop {
                tokio::select! {
                    biased;
                    _ = context.control.cancelled() => return Ok(()),
                    result = tasks.join_next() => return Err(ConsensusError::Protocol(format!("Cordial transport worker stopped: {result:?}"))),
                    Some(command) = context.commands.recv() => {
                        match command {
                            ConsensusCommand::Propose { reply, .. } => {
                                let result = self.propose(&mut state, &outbound);
                                if let Err(ConsensusError::Protocol(_)) = &result { let error = result.unwrap_err(); let _ = reply.send(Err(error.clone())); return Err(error); }
                                self.advance(&mut state, &context).await?;
                                let _ = reply.send(result);
                            }
                            ConsensusCommand::Submit { payload, reply } => {
                                let result = self.submit(&mut state, &payload, &outbound);
                                if let Err(ConsensusError::Protocol(_)) = &result { let error = result.unwrap_err(); let _ = reply.send(Err(error.clone())); return Err(error); }
                                let _ = reply.send(result);
                            }
                            ConsensusCommand::Finalized { reply } => { let _ = reply.send(Err(ConsensusError::UnsupportedCapability("finalized progress"))); }
                        }
                    }
                    Some(request) = context.packets.recv() => {
                        let result = match request.packet.kind.as_str() {
                            BLOCK_PACKET_KIND => state.admit(&request.packet.payload).map(|_| ()).map_err(classify),
                            SYNC_PACKET_KIND => self.receive_sync(&mut state, request.packet.peer, &request.packet.payload, &mut cursors, &outbound),
                            SUBMIT_PACKET_KIND => {
                                match decode::<SubmitEnvelope>(&request.packet.payload).map_err(classify) {
                                    Ok(envelope) if envelope.chain == self.chain.fingerprint() => self.submit(&mut state, &envelope.payload, &outbound).map(|_| ()),
                                    Ok(_) => Err(classify(Error::ChainMismatch)),
                                    Err(error) => Err(error),
                                }
                            }
                            _ => Err(ConsensusError::InvalidInput("unsupported Cordial packet kind".into())),
                        };
                        if let Err(ConsensusError::Protocol(_)) = &result { let error = result.unwrap_err(); let _ = request.reply.send(Err(error.clone())); return Err(error); }
                        if result.is_ok() { self.advance(&mut state, &context).await?; }
                        let _ = request.reply.send(result);
                    }
                    Some(query) = self.queries.recv() => self.query(&state, query),
                    _ = interval.tick() => {
                        let peers: Vec<_> = self.network.peers().into_iter().take(self.options.max_peers).collect();
                        cursors.retain(|id, _| peers.iter().any(|peer| &peer.id == id));
                        for peer in peers {
                            let cursor = *cursors.entry(peer.id.clone()).or_insert(0);
                            self.send_sync(&outbound, peer, SyncMessage::Request(cursor))?;
                        }
                        if self.options.key.is_some() {
                            match self.propose(&mut state, &outbound) {
                                Ok(_) => self.advance(&mut state, &context).await?,
                                Err(ConsensusError::NotReady) => {},
                                Err(error) => return Err(error),
                            }
                        }
                    }
                }
            }
        }.await;
        context.control.draining();
        context.commands.close();
        context.packets.close();
        self.queries.close();
        tasks.shutdown().await;
        result
    }
}
