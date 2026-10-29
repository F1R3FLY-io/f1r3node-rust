use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::block_status::BlockStatus;
use casper::rust::blocks::ingest::block_ingest::{IngestHandle, IngestPorts, Stores};
use casper::rust::blocks::ingest::dependency_tracker::Work;
use casper::rust::blocks::ingest::fetch_scheduler::FetchAction;
use casper::rust::blocks::ingest::ingest_state::{
    Effect, IngestInput, IngestState, Refusal, WorkOutcome,
};
use casper::rust::blocks::ingest::ingest_view::IngestView;
use casper::rust::blocks::ingest::intake_worker::{Intake, SenderAuthorization};
use casper::rust::blocks::ingest::validation_worker::Validation;
use casper::rust::casper::Casper;
use casper::rust::errors::CasperError;
use casper::rust::util::proto_util;
use casper::rust::validate::Validate;
use casper::rust::ValidBlockProcessing;
use comm::rust::peer_node::PeerNode;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use rspace_plus_plus::rspace::history::Either;

use crate::util::comm::transport_layer_test_impl::TransportLayerTestImpl;

/// Block ingest driven inline: the same intake, state machine and validation
/// a node runs, stepped synchronously so a test sees each block's verdict.
pub struct InlineIngest {
    handle: IngestHandle,
    ports: IngestPorts,
    state: IngestState,
    intake: Intake,
    validation: Validation<TransportLayerTestImpl>,
    block_store: KeyValueBlockStore,
    dag: BlockDagKeyValueStorage,
    transport: Arc<TransportLayerTestImpl>,
    connections: ConnectionsCell,
    conf: RPConf,
}

impl InlineIngest {
    pub fn new(
        block_store: KeyValueBlockStore,
        dag: BlockDagKeyValueStorage,
        approved_block: BlockMessage,
        transport: Arc<TransportLayerTestImpl>,
        connections: ConnectionsCell,
        conf: RPConf,
    ) -> Result<Self, CasperError> {
        let (handle, ports) = IngestHandle::open(block_store.clone(), dag.clone())?;
        let view = handle.view().clone();
        let mut state = IngestState::new(view.clone());
        let anchor_height = proto_util::block_number(&approved_block);
        let stores = Stores {
            dag: &dag,
            block_store: &block_store,
        };
        state.handle(IngestInput::Attached { anchor_height }, view.now_ms(), &stores);
        Ok(Self {
            intake: Intake::new(block_store.clone(), dag.clone(), view, approved_block),
            validation: Validation::new(
                block_store.clone(),
                dag.clone(),
                transport.clone(),
                connections.clone(),
                conf.clone(),
                None,
            ),
            handle,
            ports,
            state,
            block_store,
            dag,
            transport,
            connections,
            conf,
        })
    }

    pub fn handle(&self) -> IngestHandle { self.handle.clone() }

    pub fn view(&self) -> &IngestView { self.handle.view() }

    pub fn block_needs(&self) -> usize { self.state.block_needs() }

    /// Run `block`, delivered by `from` (none for a block handed over
    /// locally), through block ingest, after what producers handed over
    /// before it. Every block it makes ready is validated before this
    /// returns; the verdict is `block`'s own, or `MissingBlocks` while it
    /// waits on what it cites.
    pub async fn process(
        &mut self,
        casper: Arc<dyn Casper + Send + Sync>,
        block: BlockMessage,
        from: Option<PeerNode>,
    ) -> Result<ValidBlockProcessing, CasperError> {
        self.pump(casper.clone()).await?;
        let target = block.block_hash.clone();
        let input = self.intake.admit(&block, from)?;
        let refused = match &input {
            IngestInput::IntakeRefused(refused) => Some(refusal_status(refused.reason)),
            _ => None,
        };
        let mut queue = VecDeque::from([input]);
        let verdicts = self.settle(casper, &mut queue).await?;
        Ok(refused
            .or_else(|| verdicts.get(&target).cloned())
            .unwrap_or_else(|| Either::Left(BlockStatus::missing_blocks())))
    }

    /// Validate `block` past intake's of-interest filter (shard, version,
    /// age), so a check validation repeats can be reached directly. The
    /// sender, format and dependency checks still apply.
    pub async fn validate_past_interest_filter(
        &mut self,
        casper: Arc<dyn Casper + Send + Sync>,
        block: BlockMessage,
    ) -> Result<ValidBlockProcessing, CasperError> {
        if self.intake.sender_authorization(&block)? == SenderAuthorization::Unauthorized {
            return Ok(Either::Left(BlockStatus::invalid_sender()));
        }
        if !(Validate::format_of_fields(&block) && Validate::block_signature(&block)) {
            return Ok(Either::Left(BlockStatus::invalid_format()));
        }
        let all_held = proto_util::dependencies_hashes_of(&block)
            .iter()
            .all(|dependency| self.dag.contains(dependency));
        if !all_held {
            return Ok(Either::Left(BlockStatus::missing_blocks()));
        }
        self.block_store.put_block_message(&block)?;
        let (verdict, _) = self.validation.judge(casper.clone(), &block).await?;
        self.pump(casper).await?;
        Ok(verdict)
    }

    /// Feed what producers have handed over (announcements, needs,
    /// deliveries) and what is due, and validate what becomes ready.
    pub async fn pump(&mut self, casper: Arc<dyn Casper + Send + Sync>) -> Result<(), CasperError> {
        self.settle(casper, &mut VecDeque::new()).await?;
        Ok(())
    }

    async fn settle(
        &mut self,
        casper: Arc<dyn Casper + Send + Sync>,
        queue: &mut VecDeque<IngestInput>,
    ) -> Result<HashMap<BlockHash, ValidBlockProcessing>, CasperError> {
        let mut verdicts = HashMap::new();
        loop {
            self.collect(queue)?;
            queue.push_back(IngestInput::Tick {
                connected: self.connected(),
            });
            let mut progressed = false;
            while let Some(input) = queue.pop_front() {
                let ticked = matches!(input, IngestInput::Tick { .. });
                let stores = Stores {
                    dag: &self.dag,
                    block_store: &self.block_store,
                };
                let effects = self.state.handle(input, self.view().now_ms(), &stores);
                progressed |= !ticked || !effects.is_empty();
                for effect in effects {
                    self.execute(effect, queue).await?;
                }
                self.collect(queue)?;
            }
            while let Some((hash, work)) = self.state.next_dispatch() {
                progressed = true;
                let (verdict, outcome) = self.run(casper.clone(), &hash, work).await?;
                if let Some(verdict) = verdict {
                    verdicts.insert(hash.clone(), verdict);
                }
                self.collect(queue)?;
                queue.push_back(IngestInput::Outcome { hash, outcome });
            }
            if !progressed && queue.is_empty() {
                return Ok(verdicts);
            }
        }
    }

    /// What producers handed over since the last step, inserts first.
    fn collect(&mut self, queue: &mut VecDeque<IngestInput>) -> Result<(), CasperError> {
        while let Ok(hash) = self.ports.inserted.try_recv() {
            queue.push_back(IngestInput::Inserted { hash });
        }
        while let Ok(input) = self.ports.inputs.try_recv() {
            queue.push_back(input);
        }
        while let Ok(delivery) = self
            .ports
            .awaited
            .try_recv()
            .or_else(|_| self.ports.unsolicited.try_recv())
        {
            queue.push_back(self.intake.admit(&delivery.block, Some(delivery.from))?);
        }
        Ok(())
    }

    async fn execute(&mut self, effect: Effect, queue: &mut VecDeque<IngestInput>) -> Result<(), CasperError> {
        match effect {
            Effect::Fetch(FetchAction::LookupLocal(hash)) => match self.block_store.get(&hash)? {
                Some(block) => queue.push_back(self.intake.admit(&block, None)?),
                None => queue.push_back(IngestInput::LocalMiss { hash }),
            },
            Effect::Fetch(FetchAction::RequestBlock { hash, peer }) => {
                self.transport
                    .request_for_block(&self.conf, &peer, hash)
                    .await?;
            }
            Effect::Fetch(FetchAction::BroadcastHasBlockRequest(hash)) => {
                self.transport
                    .broadcast_has_block_request(&self.connections, &self.conf, &hash)
                    .await?;
            }
            // Test nodes run no state requester: a state-root need stays open.
            Effect::Fetch(FetchAction::FetchStateRoot(_)) | Effect::Struck { .. } => {}
            Effect::DiscardPending(hash) => {
                self.block_store.discard_pending(&hash);
            }
        }
        Ok(())
    }

    async fn run(
        &self,
        casper: Arc<dyn Casper + Send + Sync>,
        hash: &BlockHash,
        work: Work,
    ) -> Result<(Option<ValidBlockProcessing>, WorkOutcome), CasperError> {
        let block = self.block_store.get(hash)?.ok_or_else(|| {
            CasperError::RuntimeError("a dispatched block has no stored body".to_string())
        })?;
        Ok(match work {
            Work::Validate => {
                let (verdict, outcome) = self.validation.judge(casper, &block).await?;
                (Some(verdict), outcome)
            }
            Work::AdmitSettled => {
                let outcome = self.validation.admit_settled(casper, &block).await;
                let verdict = (outcome == WorkOutcome::Settled).then(|| Either::Right(BlockStatus::valid()));
                (verdict, outcome)
            }
        })
    }

    fn connected(&self) -> Vec<PeerNode> {
        self.connections
            .read()
            .map(|connections| connections.iter().cloned().collect())
            .expect("the test connections cell is never poisoned")
    }
}

fn refusal_status(reason: Refusal) -> ValidBlockProcessing {
    Either::Left(match reason {
        Refusal::NotOfInterest => BlockStatus::not_of_interest(),
        Refusal::Unauthorized => BlockStatus::invalid_sender(),
        Refusal::Malformed => BlockStatus::invalid_format(),
        Refusal::Pending(_) => BlockStatus::missing_blocks(),
    })
}
