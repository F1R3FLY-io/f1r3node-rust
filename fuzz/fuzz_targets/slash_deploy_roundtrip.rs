//! `slash_deploy_roundtrip` — proto idempotency for `SystemDeployData::Slash`.
//!
//! Invariant: `from_proto(slash.to_proto()) == slash` for every slash whose
//! evidence is two arbitrary block commitments. No input is filtered: every
//! commitment field takes any value of its type, and the slash carries it
//! unchecked.

#![no_main]

use crypto::rust::public_key::PublicKey;
use libfuzzer_sys::fuzz_target;
use models::rust::casper::protocol::casper_message::{
    BlockCommitment, EquivocationEvidence, Header, ProcessedSystemDeploy, SystemDeployData,
};
use prost::bytes::Bytes;

#[derive(arbitrary::Arbitrary, Debug)]
struct CommitmentInput {
    parents: Vec<Vec<u8>>,
    timestamp: i64,
    version: i64,
    header_extra_bytes: Vec<u8>,
    chain_id: Vec<u8>,
    pre_state_hash: Vec<u8>,
    post_state_hash: Vec<u8>,
    bonds_root: [u8; 32],
    justifications_root: [u8; 32],
    block_number: i64,
    rejected_root: [u8; 32],
    applied_from_scope_root: [u8; 32],
    merge_base: Vec<u8>,
    body_extra_bytes: Vec<u8>,
    sender: Vec<u8>,
    sig_algorithm: String,
    seq_num: i32,
    shard_id: String,
    extra_bytes: Vec<u8>,
    deploys_root: [u8; 32],
    sig: Vec<u8>,
}

impl CommitmentInput {
    fn commitment(self) -> BlockCommitment {
        BlockCommitment {
            header: Header {
                parents_hash_list: self.parents.into_iter().map(Bytes::from).collect(),
                timestamp: self.timestamp,
                version: self.version,
                extra_bytes: self.header_extra_bytes.into(),
                chain_id: self.chain_id.into(),
            },
            pre_state_hash: self.pre_state_hash.into(),
            post_state_hash: self.post_state_hash.into(),
            bonds_root: self.bonds_root,
            justifications_root: self.justifications_root,
            block_number: self.block_number,
            rejected_root: self.rejected_root,
            applied_from_scope_root: self.applied_from_scope_root,
            merge_base: self.merge_base.into(),
            body_extra_bytes: self.body_extra_bytes.into(),
            v6: None,
            sender: self.sender.into(),
            sig_algorithm: self.sig_algorithm,
            seq_num: self.seq_num,
            shard_id: self.shard_id,
            extra_bytes: self.extra_bytes.into(),
            deploys_root: self.deploys_root,
            sig: self.sig.into(),
        }
    }
}

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
    first: CommitmentInput,
    second: CommitmentInput,
    issuer_public_key: Vec<u8>,
}

fuzz_target!(|input: Input| {
    let processed = ProcessedSystemDeploy::Succeeded {
        event_list: Vec::new(),
        system_deploy: SystemDeployData::create_slash(
            EquivocationEvidence {
                first: input.first.commitment(),
                second: input.second.commitment(),
            },
            PublicKey::from_bytes(&Bytes::from(input.issuer_public_key)),
        ),
    };

    let decoded = ProcessedSystemDeploy::from_proto(processed.clone().to_proto())
        .expect("slash deploy roundtrip");

    assert_eq!(decoded, processed);
});
