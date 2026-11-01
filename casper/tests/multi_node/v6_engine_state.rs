use std::sync::Arc;
use std::time::Duration;

use casper::rust::api::block_api::BlockAPI;
use casper::rust::errors::CasperError;
use casper::rust::util::construct_deploy;
use casper::rust::v6::backend::ExecutionBackend;
use casper::rust::v6::block_record::{BlockRecord, EnvelopeOutcome};
use casper::rust::v6::body::StateHash;
use casper::rust::v6::engine::V6Parts;
use casper::rust::v6::evidence::MemoryEvidenceSink;
use casper::rust::v6::executor::BlockExecutor;
use casper::rust::v6::inclusion::GateScreen;
use casper::rust::v6::receipt::OfferedStatus;
use casper::rust::v6::store::{LmdbRecordStore, Opened, RecordSource};
use casper::rust::v6::validators::AttestedRootValidators;
use casper::rust::v6::validity::Limits;
use casper::rust::v6::V6Engine;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};

use crate::helper::test_node::TestNode;
use crate::helper::v6_network::{engine, propose_and_broadcast};
use crate::util::genesis_builder::GenesisContext;

async fn genesis() -> GenesisContext {
    crate::helper::v6_network::v6_genesis().await
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn offer(shard_id: &str, source: &str, time_stamp: i64) -> Signed<DeployData> {
    construct_deploy::source_deploy(
        source.to_string(),
        time_stamp,
        Some(500_000),
        None,
        None,
        None,
        Some(shard_id.to_string()),
    )
    .unwrap()
}

async fn settle(nodes: &[TestNode], pause: Duration) {
    for node in nodes {
        engine(node).wake();
    }
    tokio::time::sleep(pause).await;
}

struct CountingBackend {
    inner: BlockExecutor,
    targets: parking_lot::Mutex<Vec<BlockHash>>,
}

#[async_trait::async_trait]
impl ExecutionBackend for CountingBackend {
    type Record = BlockRecord;

    async fn execute(
        &self,
        pre_root: &StateHash,
        block: &BlockMessage,
    ) -> Result<BlockRecord, CasperError> {
        self.targets.lock().push(block.block_hash.clone());
        self.inner.execute(pre_root, block).await
    }

    async fn replay(
        &self,
        record: &BlockRecord,
        block: &BlockMessage,
    ) -> Result<StateHash, CasperError> {
        self.targets.lock().push(block.block_hash.clone());
        self.inner.replay(record, block).await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restarted_engine_resumes_its_canonical_prefix_without_replaying_it() {
    let mut nodes = TestNode::create_v6_network(genesis().await, 3).await.unwrap();
    let mut blocks = vec![nodes[0].genesis.block_hash.clone()];
    for round in 0..240 {
        if engine(&nodes[0]).canonical_len() >= 3 {
            break;
        }
        let block = propose_and_broadcast(&mut nodes, round % 3, &[]).await;
        blocks.push(block.block_hash.clone());
        settle(&nodes, Duration::from_millis(500)).await;
    }
    let first = engine(&nodes[0]);
    let canonical = first.canonical_len();
    assert!(canonical >= 3, "the chain fixed only {canonical} positions");
    let attestation = first.attestation();
    let prefix: Vec<_> = blocks
        .iter()
        .filter_map(|hash| first.executed(hash).map(|root| (hash.clone(), root)))
        .collect();
    assert!(prefix.len() >= canonical);

    let node = &nodes[0];
    let source: Arc<dyn RecordSource<BlockRecord>> = Arc::new(node.block_store.clone());
    let (store, opened) = LmdbRecordStore::<BlockRecord>::open(
        node.block_dag_storage.v6_executor_state(),
        source,
        node.genesis.block_hash.clone(),
        node.genesis.body.state.post_state_hash.clone(),
    )
    .unwrap();
    assert_eq!(opened, Opened::Resumed);
    let runtime_manager = Arc::new(node.runtime_manager.clone());
    let backend = Arc::new(CountingBackend {
        inner: BlockExecutor::new(runtime_manager.clone(), 2),
        targets: parking_lot::Mutex::new(Vec::new()),
    });
    let restarted = V6Engine::with_parts(
        &node.genesis,
        V6Parts {
            backend: backend.clone(),
            validators: Arc::new(AttestedRootValidators::new(runtime_manager.clone())),
            evidence: Arc::new(MemoryEvidenceSink::new()),
            inclusion: Arc::new(GateScreen::new(runtime_manager)),
            store: Some(Box::new(store)),
            limits: Limits::default(),
            params: Default::default(),
            lag_threshold: casper::rust::v6::lag::DEFAULT_LAG_THRESHOLD,
            conservation: None,
        },
        node.block_store.clone(),
        node.block_dag_storage.clone(),
        node.deploy_storage.clone(),
    );
    assert!(restarted.canonical_len() >= canonical);
    for _ in 0..400 {
        if !restarted.tick().await.unwrap() {
            break;
        }
    }
    assert!(restarted.canonical_len() >= canonical);
    if restarted.canonical_len() == canonical {
        assert_eq!(restarted.attestation(), attestation);
    }
    for (hash, root) in &prefix {
        assert_eq!(restarted.executed(hash).as_ref(), Some(root));
    }
    let touched = backend.targets.lock().clone();
    for (hash, _) in prefix.iter().take(canonical) {
        assert!(!touched.contains(hash), "a resumed block was run again");
    }
    assert!(restarted.evidence().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_long_run_keeps_executor_memory_flat_and_old_receipts_available() {
    let genesis = genesis().await;
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_v6_network(genesis, 3).await.unwrap();

    let first = offer(&shard_id, "new x in { x!(1) | for (_ <- x) { Nil } }", now());
    let mut blocks = Vec::new();
    let mut breach = None;
    let mut peak = 0usize;
    for round in 0..72 {
        let deploys = if round == 0 {
            vec![first.clone()]
        } else if round % 12 == 0 {
            vec![offer(&shard_id, r#"@"later"!(1)"#, now() + round as i64)]
        } else {
            Vec::new()
        };
        let block = propose_and_broadcast(&mut nodes, round % 3, &deploys).await;
        blocks.push(block.block_hash.clone());
        settle(&nodes, Duration::from_millis(300)).await;
        for node in &nodes {
            let received = blocks.len() + 1;
            let lag = received.saturating_sub(engine(node).canonical_len());
            let resident = engine(node).resident();
            peak = peak.max(resident);
            if resident > 6 * lag + 48 {
                breach = Some((round, resident, lag));
            }
        }
    }

    let canonical = engine(&nodes[0]).canonical_len();
    eprintln!("canonical={canonical} peak={peak}");
    assert!(canonical >= 30, "only {canonical} positions were fixed");
    assert_eq!(
        breach, None,
        "resident state outgrew the unfixed blocks (round, resident, lag)"
    );

    match BlockAPI::offered_status(&nodes[1].engine_cell, &first.sig)
        .await
        .unwrap()
    {
        OfferedStatus::Executed(receipt) => {
            assert_eq!(receipt.outcome, EnvelopeOutcome::Completed);
            assert_eq!(receipt.comm_phlo, 1);
        }
        other => panic!("the first offer's receipt is not served: {:?}", other),
    }
    for block in &blocks {
        let roots: Vec<_> = nodes.iter().map(|n| engine(n).executed(block)).collect();
        if roots.iter().all(Option::is_some) {
            assert!(roots.iter().all(|r| *r == roots[0]), "validators disagree");
        }
    }
    for node in &nodes {
        assert!(
            engine(node).evidence().is_empty(),
            "{:?}",
            engine(node).evidence()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_validator_audits_conservation_on_its_schedule() {
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use rholang::rust::interpreter::accounting::rev_stack::{wallet_channel_id, TokenKind, WalletOwner};

    let genesis = genesis().await;
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_v6_network_with(
        genesis,
        3,
        crate::helper::test_node::V6TestConfig {
            audit_interval: Some(2),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let (_, bob) = crypto::rust::signatures::secp256k1::Secp256k1.new_key_pair();
    let address = rholang::rust::interpreter::util::vault_address::VaultAddress::from_public_key(&bob)
        .unwrap()
        .to_base58();
    let transfer = offer(
        &shard_id,
        &format!(
            r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`) in {{ w!("transfer", (*d, "{address}", 700), "out") }}"#
        ),
        now(),
    );
    let audited_from = |node: &TestNode| {
        engine(node)
            .conservation()
            .and_then(|c| c.last())
            .map(|(number, _)| number)
    };
    for round in 0..30 {
        let deploys = if round == 0 { vec![transfer.clone()] } else { Vec::new() };
        propose_and_broadcast(&mut nodes, round % 3, &deploys).await;
        settle(&nodes, Duration::from_millis(300)).await;
        if nodes.iter().all(|n| audited_from(n).is_some_and(|number| number >= 6)) {
            break;
        }
    }
    let bob_wallet = wallet_channel_id(&WalletOwner::from_public_key(&bob).unwrap());
    for node in &nodes {
        let conservation = engine(node).conservation().expect("a v6 node audits");
        let (number, audits) = conservation.last().expect("an audit ran");
        assert!(number >= 6, "the last audit was at block {number}");
        assert!(audits.iter().all(|a| a.conserved()), "{audits:?}");
        assert_eq!(conservation.failures(), 0);
        assert!(conservation
            .index()
            .wallets(&TokenKind::rev())
            .any(|w| *w == bob_wallet));
    }

    let node = &nodes[0];
    let reopened = casper::rust::v6::conservation::Conservation::open(
        casper::rust::v6::conservation::AuditSchedule { interval: 2 },
        &node.genesis,
        node.block_dag_storage.v6_executor_state(),
    )
    .unwrap();
    assert!(reopened
        .index()
        .wallets(&TokenKind::rev())
        .any(|w| *w == bob_wallet));
}
