use std::sync::Arc;

use casper::rust::util::construct_deploy::{self, DEFAULT_SEC, DEFAULT_SEC2};
use casper::rust::v6::backend::ExecutionBackend;
use casper::rust::v6::block_record::{BlockRecord, EnvelopeOutcome};
use casper::rust::v6::executor::BlockExecutor;
use casper::rust::v6::record::ExecutionRecord;
use crypto::rust::private_key::PrivateKey;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, F1r3flyState, Header, ProcessedDeploy,
};
use prost::bytes::Bytes;

use crate::util::rholang::resources::with_runtime_manager;

fn envelope(term: &str, ts: i64, sec: &PrivateKey) -> ProcessedDeploy {
    ProcessedDeploy::empty(
        construct_deploy::source_deploy(
            term.to_string(),
            ts,
            Some(100_000),
            Some(0),
            Some(sec.clone()),
            None,
            Some("root".to_string()),
        )
        .unwrap(),
    )
}

fn block(sender: Bytes, deploys: Vec<ProcessedDeploy>) -> BlockMessage {
    BlockMessage {
        block_hash: Bytes::from_static(b"v6-block-executor"),
        header: Header {
            parents_hash_list: vec![],
            timestamp: 1,
            version: 1,
            extra_bytes: Bytes::new(),
        },
        body: Body {
            state: F1r3flyState {
                pre_state_hash: Bytes::new(),
                post_state_hash: Bytes::new(),
                bonds: vec![],
                block_number: 1,
            },
            deploys,
            rejected_deploys: vec![],
            system_deploys: vec![],
            extra_bytes: Bytes::new(),
            applied_from_scope: vec![],
            merge_base: Bytes::new(),
            v6: None,
        },
        justifications: vec![],
        sender,
        seq_num: 1,
        sig: Bytes::new(),
        sig_algorithm: String::new(),
        shard_id: "root".to_string(),
        extra_bytes: Bytes::new(),
    }
}

#[tokio::test]
async fn a_block_record_replays_and_a_tampered_one_does_not() {
    with_runtime_manager(|runtime_manager, context, genesis| async move {
        let executor = BlockExecutor::new(Arc::new(runtime_manager), 2);
        let pre_root = genesis.body.state.post_state_hash.clone();
        let sender = context.validator_key_pairs[0].1.bytes.clone();
        let b = block(sender, vec![
            envelope("new x in { x!(1) | for (_ <- x) { Nil } }", 1, &DEFAULT_SEC),
            envelope("@\"v6-be-race\"!(1)", 2, &DEFAULT_SEC2),
            envelope("for (_ <- @\"v6-be-race\") { Nil }", 3, &DEFAULT_SEC),
        ]);

        let record = executor
            .execute(&pre_root, &b)
            .await
            .expect("the block executes");
        let outcomes: Vec<_> = record.executed().iter().map(|e| e.outcome).collect();
        assert_eq!(outcomes, vec![
            EnvelopeOutcome::Completed,
            EnvelopeOutcome::Completed,
            EnvelopeOutcome::Conflicted,
        ]);
        assert_eq!(record.executed()[0].comm_phlo, 1);
        assert!(record.executed().iter().all(|e| e.compute_phlo > 0));
        assert_eq!(record.pre_root(), &pre_root);
        assert_ne!(record.mid_root(), &pre_root);
        assert!(!record.system_deploys().is_empty());

        let decoded = BlockRecord::decode(&record.encode()).expect("the record decodes");
        assert_eq!(decoded, record);
        let post = executor
            .replay(&decoded, &b)
            .await
            .expect("the record replays");
        assert_eq!(&post, record.post_root());

        let mut proto_bytes = record.encode().to_vec();
        let mid = record.mid_root().clone();
        let at = proto_bytes
            .windows(mid.len())
            .position(|w| w == mid.as_ref())
            .unwrap();
        proto_bytes[at] ^= 1;
        let forged = BlockRecord::decode(&proto_bytes).expect("still well formed");
        assert!(executor.replay(&forged, &b).await.is_err());

        let mut reordered = b.clone();
        reordered.body.deploys.swap(0, 1);
        assert!(executor.replay(&record, &reordered).await.is_err());
    })
    .await
    .unwrap();
}
