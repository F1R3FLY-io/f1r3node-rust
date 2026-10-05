use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

use cordial_f1r3node_adapter::proposer::{PayloadBuilder, TipSelector};
use cordial_miners_core::blocklace::Blocklace;
use cordial_miners_core::consensus::cordiality::is_weighted_supermajority;
use cordial_miners_core::consensus::round::compute_all_depths;
use cordial_miners_core::consensus::{select_predecessors, validated_received_insert};
use cordial_miners_core::{BlockIdentity, NodeId};

use crate::{Chain, Error};

pub(crate) struct QuorumPrefixTipSelector {
    creator: NodeId,
    failure: RefCell<Option<Error>>,
}

impl QuorumPrefixTipSelector {
    pub(crate) fn new(creator: NodeId) -> Self {
        Self {
            creator,
            failure: RefCell::new(None),
        }
    }

    pub(crate) fn take_failure(&self) -> Option<Error> {
        self.failure.borrow_mut().take()
    }

    fn tips(
        &self,
        view: &Blocklace,
        weights: &HashMap<NodeId, u64>,
    ) -> Result<HashSet<BlockIdentity>, Error> {
        let own = view.blocks_by(&self.creator);
        if own.is_empty() {
            return Ok(HashSet::new());
        }
        let depths = compute_all_depths(view);
        let mut support = BTreeMap::<u64, HashSet<NodeId>>::new();
        for (id, depth) in &depths {
            support
                .entry(*depth)
                .or_default()
                .insert(id.creator.clone());
        }
        let Some(round) = support.into_iter().rev().find_map(|(round, creators)| {
            is_weighted_supermajority(&creators, weights).then_some(round)
        }) else {
            return Ok(HashSet::new());
        };
        if own.iter().any(|block| {
            depths
                .get(&block.identity)
                .is_some_and(|depth| *depth > round)
        }) {
            return Ok(HashSet::new());
        }
        let mut prefix = Blocklace::new();
        let mut identities: Vec<_> = depths
            .into_iter()
            .filter(|(_, depth)| *depth <= round)
            .collect();
        identities.sort_by(|(left, ld), (right, rd)| ld.cmp(rd).then(left.cmp(right)));
        for (id, _) in identities {
            let block = view
                .get(&id)
                .ok_or_else(|| Error::Corrupt("proposal history is missing".into()))?;
            if !validated_received_insert(block, &mut prefix, weights).is_valid() {
                return Err(Error::Corrupt(
                    "proposal prefix failed native validation".into(),
                ));
            }
        }
        Ok(select_predecessors(&prefix, weights))
    }
}

impl TipSelector for &QuorumPrefixTipSelector {
    fn select_tips(
        &self,
        blocklace: &Blocklace,
        bonds: &HashMap<NodeId, u64>,
    ) -> HashSet<BlockIdentity> {
        match self.tips(blocklace, bonds) {
            Ok(tips) => tips,
            Err(error) => {
                *self.failure.borrow_mut() = Some(error);
                HashSet::new()
            }
        }
    }
}

pub(crate) struct ChainPayloadBuilder<'a> {
    chain: &'a Chain,
    data: Vec<u8>,
}

impl<'a> ChainPayloadBuilder<'a> {
    pub(crate) fn new(chain: &'a Chain, data: Vec<u8>) -> Self {
        Self { chain, data }
    }
}

impl PayloadBuilder for ChainPayloadBuilder<'_> {
    fn build_payload(&mut self, predecessors: &HashSet<BlockIdentity>) -> Result<Vec<u8>, String> {
        self.chain
            .payload_bytes(predecessors.is_empty(), std::mem::take(&mut self.data))
            .map_err(|error| error.to_string())
    }
}
