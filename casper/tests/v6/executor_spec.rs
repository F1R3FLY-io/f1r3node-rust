use std::sync::Arc;

use casper::rust::util::construct_deploy::{self, DEFAULT_PUB, DEFAULT_SEC, DEFAULT_SEC2};
use casper::rust::v6::executor::EnvelopeExecutor;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use rholang::rust::interpreter::system_processes::BlockData;

use crate::util::rholang::resources::with_runtime_manager;

fn envelope(term: &str, ts: i64, sec: &PrivateKey) -> Signed<DeployData> {
    construct_deploy::source_deploy(
        term.to_string(),
        ts,
        Some(100_000),
        Some(0),
        Some(sec.clone()),
        None,
        Some("root".to_string()),
    )
    .unwrap()
}

fn block_data() -> BlockData {
    BlockData {
        time_stamp: 1,
        block_number: 1,
        sender: DEFAULT_PUB.clone(),
        seq_num: 1,
    }
}

#[tokio::test]
async fn execute_and_replay_agree_and_record_comm_and_compute_phlo() {
    with_runtime_manager(|runtime_manager, _, genesis| async move {
        let executor = EnvelopeExecutor::new(Arc::new(runtime_manager), 2);
        let pre_root = genesis.body.state.post_state_hash.clone();
        let envelopes = vec![
            envelope(
                "new x in { x!(1) | for (_ <- x) { Nil } | x!(2) | for (_ <- x) { Nil } }",
                1,
                &DEFAULT_SEC,
            ),
            envelope("@\"v6-shared\"!(7)", 2, &DEFAULT_SEC2),
            envelope("new y in { y!(3) }", 3, &DEFAULT_SEC),
        ];

        let executed = executor
            .execute(&pre_root, envelopes, block_data())
            .await
            .expect("the block executes");
        assert_eq!(executed.runs.len(), 3);
        assert_eq!(executed.runs[0].comm_phlo, 2);
        assert_eq!(executed.runs[1].comm_phlo, 0);
        assert_eq!(executed.runs[2].comm_phlo, 0);
        assert!(executed.runs.iter().all(|run| run.compute_phlo > 0));
        assert!(executed.runs.iter().all(|run| !run.failed));
        assert_eq!(executed.merged, vec![true, true, true]);
        assert_ne!(executed.mid_root, pre_root);

        let replayed = executor
            .replay(
                &pre_root,
                executed.processed.clone(),
                &executed.gate.entries,
                block_data(),
            )
            .await
            .expect("the block replays");
        assert_eq!(replayed.mid_root, executed.mid_root);
        assert_eq!(replayed.merged, executed.merged);
        for (recorded, again) in executed.runs.iter().zip(&replayed.runs) {
            assert_eq!(recorded.recorded().mismatch(again), None);
            assert_eq!(recorded.post_root, again.post_root);
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_conflicting_envelope_loses_in_block_order() {
    with_runtime_manager(|runtime_manager, _, genesis| async move {
        let executor = EnvelopeExecutor::new(Arc::new(runtime_manager), 2);
        let pre_root = genesis.body.state.post_state_hash.clone();
        let envelopes = vec![
            envelope("@\"v6-race\"!(1)", 1, &DEFAULT_SEC),
            envelope("for (_ <- @\"v6-race\") { Nil }", 2, &DEFAULT_SEC2),
        ];
        let executed = executor
            .execute(&pre_root, envelopes, block_data())
            .await
            .expect("the block executes");
        assert_eq!(executed.merged, vec![true, false]);

        let replayed = executor
            .replay(
                &pre_root,
                executed.processed.clone(),
                &executed.gate.entries,
                block_data(),
            )
            .await
            .expect("the block replays");
        assert_eq!(replayed.mid_root, executed.mid_root);
        assert_eq!(replayed.merged, executed.merged);
    })
    .await
    .unwrap();
}
