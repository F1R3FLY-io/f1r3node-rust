use std::collections::{HashMap, HashSet};

use cordial_f1r3node_adapter::deploy_trace::{DeployTraceState, DeployTracer, TraceIngressSource};
use cordial_f1r3node_adapter::proposer::{
    CordialProposer, DisseminationTipSelector, PayloadBuilder, ProposeError, RecordingBroadcaster,
    Secp256k1BlockSigner,
};
use cordial_miners_core::blocklace::Blocklace;
use cordial_miners_core::crypto::{hash_content, verify};
use cordial_miners_core::types::{BlockIdentity, NodeId};
use k256::ecdsa::SigningKey;

struct Fixed(Result<Vec<u8>, String>);

impl PayloadBuilder for Fixed {
    fn build_payload(&mut self, _: &HashSet<BlockIdentity>) -> Result<Vec<u8>, String> {
        self.0.clone()
    }
}

fn validator() -> (Vec<u8>, NodeId, HashMap<NodeId, u64>) {
    let key = SigningKey::from_slice(&[9; 32]).unwrap();
    let creator = NodeId(key.verifying_key().to_sec1_bytes().to_vec());
    let bonds = HashMap::from([(creator.clone(), 1)]);
    (key.to_bytes().to_vec(), creator, bonds)
}

#[test]
fn first_proposal_is_a_signed_initial_block_and_is_broadcast() {
    let (secret, creator, bonds) = validator();
    let broadcaster = RecordingBroadcaster::new();
    let blocks = broadcaster.blocks.clone();
    let mut proposer = CordialProposer::new(
        DisseminationTipSelector,
        Fixed(Ok(vec![1, 2, 3])),
        Secp256k1BlockSigner::new(secret),
        broadcaster,
        creator.clone(),
        bonds,
    );
    let block = proposer.propose(&Blocklace::new()).unwrap();
    assert!(block.content.predecessors.is_empty());
    assert_eq!(block.content.payload, vec![1, 2, 3]);
    assert_eq!(block.identity.creator, creator);
    assert_eq!(block.identity.content_hash, hash_content(&block.content));
    assert!(verify(
        &block.identity.content_hash,
        &creator.0,
        &block.identity.signature
    ));
    assert_eq!(blocks.lock().unwrap().as_slice(), [block]);
}

#[test]
fn validator_with_history_and_no_tips_does_not_propose() {
    struct NoTips;
    impl cordial_f1r3node_adapter::proposer::TipSelector for NoTips {
        fn select_tips(&self, _: &Blocklace, _: &HashMap<NodeId, u64>) -> HashSet<BlockIdentity> {
            HashSet::new()
        }
    }
    let (secret, creator, bonds) = validator();
    let mut initial = CordialProposer::new(
        DisseminationTipSelector,
        Fixed(Ok(vec![])),
        Secp256k1BlockSigner::new(secret.clone()),
        RecordingBroadcaster::new(),
        creator.clone(),
        bonds.clone(),
    );
    let first = initial.propose(&Blocklace::new()).unwrap();
    let mut view = Blocklace::new();
    view.insert(first, &cordial_miners_core::crypto::Secp256k1Scheme)
        .unwrap();
    let mut proposer = CordialProposer::new(
        NoTips,
        Fixed(Ok(vec![])),
        Secp256k1BlockSigner::new(secret),
        RecordingBroadcaster::new(),
        creator,
        bonds,
    );
    assert_eq!(proposer.propose(&view), Err(ProposeError::NoTips));
}

#[test]
fn payload_failure_stops_the_proposal_before_signing() {
    let (secret, creator, bonds) = validator();
    let broadcaster = RecordingBroadcaster::new();
    let blocks = broadcaster.blocks.clone();
    let mut proposer = CordialProposer::new(
        DisseminationTipSelector,
        Fixed(Err("too large".into())),
        Secp256k1BlockSigner::new(secret),
        broadcaster,
        creator,
        bonds,
    );
    assert_eq!(
        proposer.propose(&Blocklace::new()),
        Err(ProposeError::Payload("too large".into()))
    );
    assert!(blocks.lock().unwrap().is_empty());
}

#[test]
fn deploy_tracer_evicts_the_oldest_trace_at_capacity() {
    let tracer = DeployTracer::with_capacity(2);
    tracer.record_observed(&[1], TraceIngressSource::Grpc);
    tracer.record_accepted(&[2]);
    tracer.record_finalized(&[2], &[7]);
    tracer.record_observed(&[3], TraceIngressSource::Http);
    assert!(tracer.get_deploy_trace(&[1]).is_none());
    assert_eq!(
        tracer.get_deploy_trace(&[2]).unwrap().state,
        DeployTraceState::FinalizedOrdered
    );
    assert!(tracer.get_deploy_trace(&[3]).is_some());
    assert_eq!(tracer.list_active_traces().len(), 2);
}
