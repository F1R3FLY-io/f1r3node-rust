use std::collections::{BTreeSet, HashMap, HashSet};

use cordial_miners_core::Block;
use cordial_miners_core::blocklace::Blocklace;
use cordial_miners_core::consensus::{
    CordialEquivocationEvidence, EvidencePool, select_predecessors, select_predecessors_strict,
};
use cordial_miners_core::crypto::{SignatureScheme, hash_content};
use cordial_miners_core::types::{BlockContent, BlockIdentity, NodeId};

#[derive(Debug, Clone, PartialEq)]
pub enum ProposeError {
    NoTips,
    Payload(String),
    Sign(String),
    Broadcast(String),
}

impl std::fmt::Display for ProposeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTips => write!(f, "no honest tips available for proposal"),
            Self::Payload(msg) => write!(f, "payload construction failed: {msg}"),
            Self::Sign(msg) => write!(f, "signing failed: {msg}"),
            Self::Broadcast(msg) => write!(f, "broadcast failed: {msg}"),
        }
    }
}

impl std::error::Error for ProposeError {}

pub trait TipSelector {
    fn select_tips(
        &self,
        blocklace: &Blocklace,
        bonds: &HashMap<NodeId, u64>,
    ) -> HashSet<BlockIdentity>;
}

/// Exposes equivocation evidence retained by the pure Cordial core.
pub trait EvidenceSource {
    fn pending_evidence(&mut self) -> Vec<CordialEquivocationEvidence>;
}

pub trait PayloadBuilder {
    fn build_payload(&mut self, predecessors: &HashSet<BlockIdentity>) -> Result<Vec<u8>, String>;
}

pub trait BlockSigner {
    fn sign_block(&self, content: &BlockContent, creator: &NodeId)
    -> Result<BlockIdentity, String>;
}

pub trait BlockBroadcaster {
    fn broadcast(&self, block: &Block) -> Result<(), String>;
}

/// Tip selector that uses compatibility-mode predecessor selection.
///
/// This is the default selector. It includes all honest validator tips and, in addition,
/// re-adds any known equivocator branch not yet transitively visible through those tips.
/// Use this for adapter / snapshot paths that need full DAG coverage and inter-node
/// compatibility.
///
/// For paper-native strict excommunication, use [`StrictTipSelector`] instead.
#[derive(Debug, Clone, Copy, Default)]
pub struct DisseminationTipSelector;

impl TipSelector for DisseminationTipSelector {
    fn select_tips(
        &self,
        blocklace: &Blocklace,
        bonds: &HashMap<NodeId, u64>,
    ) -> HashSet<BlockIdentity> {
        select_predecessors(blocklace, bonds)
    }
}

/// Tip selector that uses strict (paper-native) excommunication mode.
///
/// In strict mode no equivocator branch ever appears as a direct predecessor of a
/// newly proposed block. Only honest validator tips are selected. The proposer relies
/// on transitive observation through those tips for equivocation visibility, which is
/// the behaviour described in Cordial Miners §6.1.
///
/// Use this for proposer paths operating in fully protocol-faithful configuration.
/// For adapter / snapshot paths that need full DAG coverage, use
/// [`DisseminationTipSelector`] instead.
#[derive(Debug, Clone, Copy, Default)]
pub struct StrictTipSelector;

impl TipSelector for StrictTipSelector {
    fn select_tips(
        &self,
        blocklace: &Blocklace,
        bonds: &HashMap<NodeId, u64>,
    ) -> HashSet<BlockIdentity> {
        select_predecessors_strict(blocklace, bonds)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoEvidenceSource;

impl EvidenceSource for NoEvidenceSource {
    fn pending_evidence(&mut self) -> Vec<CordialEquivocationEvidence> {
        Vec::new()
    }
}

/// Evidence source backed by a core [`EvidencePool`].
///
/// The adapter supplies the validator set it wants to query; the pool remains a
/// pure core data structure and never sees f1r3node protobuf or RSpace types.
pub struct EvidencePoolSource<'a, P> {
    pool: &'a P,
    validators: Vec<NodeId>,
}

impl<'a, P> EvidencePoolSource<'a, P> {
    pub fn new<I>(pool: &'a P, validators: I) -> Self
    where
        I: IntoIterator<Item = NodeId>,
    {
        let validators = validators.into_iter().collect::<BTreeSet<_>>();
        Self {
            pool,
            validators: validators.into_iter().collect(),
        }
    }
}

impl<P> EvidenceSource for EvidencePoolSource<'_, P>
where
    P: EvidencePool<NodeId, Block, BlockIdentity>,
{
    fn pending_evidence(&mut self) -> Vec<CordialEquivocationEvidence> {
        self.validators
            .iter()
            .flat_map(|validator| self.pool.evidence_for(validator))
            .collect()
    }
}

pub struct Secp256k1BlockSigner {
    private_key: Vec<u8>,
}

impl Secp256k1BlockSigner {
    pub fn new(private_key: Vec<u8>) -> Self {
        Self { private_key }
    }
}

impl BlockSigner for Secp256k1BlockSigner {
    fn sign_block(
        &self,
        content: &BlockContent,
        creator: &NodeId,
    ) -> Result<BlockIdentity, String> {
        use cordial_miners_core::crypto::Secp256k1Scheme;

        let content_hash = hash_content(content);
        let signature = Secp256k1Scheme.sign(&content_hash, &self.private_key)?;
        Ok(BlockIdentity {
            content_hash,
            creator: creator.clone(),
            signature,
        })
    }
}

pub struct FnBroadcaster<F>
where
    F: Fn(&Block) -> Result<(), String>,
{
    f: F,
}

impl<F> FnBroadcaster<F>
where
    F: Fn(&Block) -> Result<(), String>,
{
    pub fn new(f: F) -> Self {
        Self { f }
    }
}

impl<F> BlockBroadcaster for FnBroadcaster<F>
where
    F: Fn(&Block) -> Result<(), String>,
{
    fn broadcast(&self, block: &Block) -> Result<(), String> {
        (self.f)(block)
    }
}

pub struct RecordingBroadcaster {
    pub blocks: std::sync::Arc<std::sync::Mutex<Vec<Block>>>,
}

impl RecordingBroadcaster {
    pub fn new() -> Self {
        Self {
            blocks: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

impl Default for RecordingBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockBroadcaster for RecordingBroadcaster {
    fn broadcast(&self, block: &Block) -> Result<(), String> {
        self.blocks
            .lock()
            .map_err(|e| format!("lock poisoned: {e}"))?
            .push(block.clone());
        Ok(())
    }
}

pub struct CordialProposer<TS, PB, BS, BC> {
    tip_selector: TS,
    payload_builder: PB,
    signer: BS,
    broadcaster: BC,
    creator: NodeId,
    bonds: HashMap<NodeId, u64>,
}

impl<TS, PB, BS, BC> CordialProposer<TS, PB, BS, BC>
where
    TS: TipSelector,
    PB: PayloadBuilder,
    BS: BlockSigner,
    BC: BlockBroadcaster,
{
    pub fn new(
        tip_selector: TS,
        payload_builder: PB,
        signer: BS,
        broadcaster: BC,
        creator: NodeId,
        bonds: HashMap<NodeId, u64>,
    ) -> Self {
        Self {
            tip_selector,
            payload_builder,
            signer,
            broadcaster,
            creator,
            bonds,
        }
    }

    pub fn propose(&mut self, blocklace: &Blocklace) -> Result<Block, ProposeError> {
        let predecessors = self.tip_selector.select_tips(blocklace, &self.bonds);
        if predecessors.is_empty() && !blocklace.blocks_by(&self.creator).is_empty() {
            return Err(ProposeError::NoTips);
        }
        let payload = self
            .payload_builder
            .build_payload(&predecessors)
            .map_err(ProposeError::Payload)?;
        let content = BlockContent {
            payload,
            predecessors,
        };
        let identity = self
            .signer
            .sign_block(&content, &self.creator)
            .map_err(ProposeError::Sign)?;
        let block = Block { identity, content };
        self.broadcaster
            .broadcast(&block)
            .map_err(ProposeError::Broadcast)?;
        Ok(block)
    }
}
