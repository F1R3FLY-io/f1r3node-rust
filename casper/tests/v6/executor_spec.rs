use std::collections::HashMap;
use std::sync::Arc;

use casper::rust::rholang::runtime::RuntimeOps;
use casper::rust::util::construct_deploy::{self, DEFAULT_PUB, DEFAULT_SEC, DEFAULT_SEC2};
use casper::rust::v6::executor::EnvelopeExecutor;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

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

#[tokio::test]
async fn envelopes_pay_only_the_storage_they_leave_and_replay_agrees() {
    with_runtime_manager(|runtime_manager, _, genesis| async move {
        let pre_root = genesis.body.state.post_state_hash.clone();
        let matched = "new x in { x!(1) | for (_ <- x) { Nil } | x!(2) | for (_ <- x) { Nil } }";
        let mixed = "new x, p, k in { x!!(1) | for (_ <- x) { Nil } | for (_ <- x) { Nil } | \
                     p!(2) | for (_ <<- p) { Nil } | contract k(@n) = { Nil } | k!(3) | k!(4) }";

        let mut dev = RuntimeOps::new(runtime_manager.spawn_runtime().await.unwrap());
        dev.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(&pre_root))
            .await
            .unwrap();
        dev.runtime.set_block_data(block_data()).await;
        dev.runtime.set_invalid_blocks(HashMap::new()).await;
        let (dev_matched, _) = dev
            .process_deploy(envelope(matched, 1, &DEFAULT_SEC))
            .await
            .unwrap();

        let executor = EnvelopeExecutor::new(Arc::new(runtime_manager), 2);
        let executed = executor
            .execute(
                &pre_root,
                vec![
                    envelope(matched, 1, &DEFAULT_SEC),
                    envelope(mixed, 2, &DEFAULT_SEC2),
                ],
                block_data(),
            )
            .await
            .expect("the block executes");
        assert!(executed.runs.iter().all(|run| !run.failed));
        assert!(executed.runs[0].compute_phlo < dev_matched.cost.cost);

        let replayed = executor
            .replay(
                &pre_root,
                executed.processed.clone(),
                &executed.gate.entries,
                block_data(),
            )
            .await
            .expect("the block replays with the same storage charge");
        assert_eq!(replayed.mid_root, executed.mid_root);
        for (recorded, again) in executed.runs.iter().zip(&replayed.runs) {
            assert_eq!(recorded.recorded().mismatch(again), None);
            assert_eq!(recorded.post_root, again.post_root);
        }
    })
    .await
    .unwrap();
}
