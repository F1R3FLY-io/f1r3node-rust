use cordial_consensus::{
    Application, Chain, ChainSpec, DurableBlocklace, StoreConfig, Submission, Validator,
};
use cordial_rholang::{
    CommittedRholang, DeployOutcome, ExecutionSummary, GenesisSettings, MAX_DEPLOY_BYTES,
    VaultAllocation, initialize_runtime,
};
use k256::ecdsa::SigningKey;
use models::rust::casper::protocol::casper_message::DeployData;
use prost::Message;

fn submission(executor: &CommittedRholang, seed: &[u8; 32], term: &str, phlo: i64) -> Submission {
    let deploy = casper::rust::util::construct_deploy::source_deploy_now_full(
        term.into(),
        Some(phlo),
        Some(1),
        Some(crypto::rust::private_key::PrivateKey::from_bytes(seed)),
        Some(0),
        Some("root".into()),
    )
    .unwrap();
    executor
        .validate_submission(&DeployData::to_proto(deploy).encode_to_vec())
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn proposals_pack_deploys_within_batch_phlo_and_execute_together() {
    let directory = tempfile::tempdir().unwrap();
    let seed = [5; 32];
    let key = SigningKey::from_slice(&seed).unwrap();
    let public = key.verifying_key().to_sec1_bytes().to_vec();
    let mut spec = ChainSpec {
        network: "execution-batching".into(),
        shard: "root".into(),
        execution_genesis: [0; 32],
        wavelength: 3,
        validators: vec![Validator {
            public_key: public.clone(),
            weight: 1,
        }],
    };
    let settings = GenesisSettings {
        vaults: vec![VaultAllocation {
            public_key: public,
            balance: 1_000_000_000,
        }],
        ..GenesisSettings::default()
    };
    let (mut manager, vm, root) = initialize_runtime(&directory.path().join("vm"), &spec, &settings)
        .await
        .unwrap();
    spec.execution_genesis = root;
    let chain = Chain::new(spec).unwrap();
    let executor = CommittedRholang::new(vm, chain.clone()).unwrap();
    let submissions: Vec<_> = (0..3)
        .map(|value| submission(&executor, &seed, &format!("@\"batch\"!({value})"), 4_000_000))
        .collect();
    assert_eq!(executor.proposal_payload(&[]).unwrap(), (vec![], 0));
    let (payload, included) = executor.proposal_payload(&submissions).unwrap();
    assert_eq!(included, 2);
    let oversized = vec![0; MAX_DEPLOY_BYTES + 1];
    assert!(executor.validate_submission(&oversized).is_err());

    let mut store =
        DurableBlocklace::open(&directory.path().join("consensus"), chain, StoreConfig::default())
            .unwrap();
    store.propose(&key, vec![]).unwrap().unwrap();
    store.propose(&key, payload).unwrap().unwrap();
    let mut executed = vec![];
    for _ in 0..16 {
        store.propose(&key, vec![]).unwrap();
        store.advance_output().unwrap();
        while let Some(receipt) = executor.execute_next(&mut store).await.unwrap() {
            let summary: ExecutionSummary = serde_json::from_slice(&receipt.result).unwrap();
            if !summary.deploys.is_empty() {
                executed.push((receipt, summary));
            }
        }
        if !executed.is_empty() {
            break;
        }
    }
    let (receipt, summary) = executed.pop().expect("batch was not executed");
    assert_eq!(
        receipt.deploy_ids,
        submissions[..2].iter().map(|submission| submission.id).collect::<Vec<_>>()
    );
    assert!(summary.deploys.iter().all(|deploy| {
        deploy.outcome == DeployOutcome::Succeeded && deploy.cost > 0
    }));
    assert_ne!(receipt.pre_state, receipt.post_state);
    assert!(store.has_executed_deploy(&submissions[0].id).unwrap());
    assert!(!store.has_executed_deploy(&submissions[2].id).unwrap());
    drop(executor);
    manager.shutdown().await.unwrap();
}
