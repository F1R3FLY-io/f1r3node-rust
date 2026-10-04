use cordial_consensus::{Chain, ChainSpec, DurableBlocklace, StoreConfig, Validator};
use cordial_rholang::{
    CommittedRholang, GenesisSettings, VaultAllocation, encode_batch, initialize_runtime,
};
use k256::ecdsa::SigningKey;

/// Programs whose outcome depends on which COMM fires first.
fn race(round: usize) -> String {
    let out = format!("@\"race-{round}\"");
    match round % 6 {
        0 => format!("new x in {{ x!(1) | x!(2) | x!(3) | for (@v <- x) {{ {out}!(v) }} }}"),
        1 => format!("new x, y in {{ x!(1) | x!(2) | y!(3) | y!(4) | for (@a <- x & @b <- y) {{ {out}!([a, b]) }} | for (@c <- x) {{ {out}!(c) }} }}"),
        2 => format!("new x in {{ for (@v <= x) {{ {out}!(v) }} | x!(1) | x!(2) | x!(3) | for (@w <- x) {{ {out}!(-w) }} }}"),
        3 => format!("new x in {{ x!(1) | x!(2) | for (@v <<- x) {{ {out}!(v) }} | for (@w <- x) {{ {out}!(w * 10) }} }}"),
        4 => format!("new x, done in {{ contract x(@n) = {{ if (n > 0) {{ x!(n - 1) | {out}!(n) }} else {{ done!(n) }} }} | x!(12) | x!(7) | for (@a <- done) {{ for (@b <- done) {{ {out}!([a, b]) }} }} }}"),
        _ => format!("new x, y in {{ x!(1) | x!(2) | y!(1) | for (@v <- x) {{ y!(v) }} | for (@v <- y) {{ x!(v) }} | for (@a <- x & @b <- y) {{ {out}!(a + b) }} }}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn independently_executed_competing_receives_produce_identical_receipts() {
    let directory = tempfile::tempdir().unwrap();
    let seed = [3; 32];
    let key = SigningKey::from_slice(&seed).unwrap();
    let public = key.verifying_key().to_sec1_bytes().to_vec();
    let mut spec = ChainSpec {
        network: "execution-determinism".into(),
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
    let (mut first_manager, first_vm, root) =
        initialize_runtime(&directory.path().join("vm-1"), &spec, &settings)
            .await
            .unwrap();
    let (mut second_manager, second_vm, second_root) =
        initialize_runtime(&directory.path().join("vm-2"), &spec, &settings)
            .await
            .unwrap();
    assert_eq!(root, second_root);
    spec.execution_genesis = root;
    let chain = Chain::new(spec).unwrap();
    let mut first = DurableBlocklace::open(
        &directory.path().join("consensus-1"),
        chain.clone(),
        StoreConfig::default(),
    )
    .unwrap();
    let mut second = DurableBlocklace::open(
        &directory.path().join("consensus-2"),
        chain.clone(),
        StoreConfig::default(),
    )
    .unwrap();
    let first_executor = CommittedRholang::new(first_vm, chain.clone()).unwrap();
    let second_executor = CommittedRholang::new(second_vm, chain.clone()).unwrap();
    let mut count = 0;
    for round in 0..32 {
        let payload = if (1..25).contains(&round) {
            let deploy = casper::rust::util::construct_deploy::source_deploy_now(
                race(round),
                Some(crypto::rust::private_key::PrivateKey::from_bytes(&seed)), Some(0), Some("root".into()),
            ).unwrap();
            encode_batch(deploy.data.time_stamp, vec![deploy]).unwrap()
        } else {
            vec![]
        };
        let block = first.propose(&key, payload).unwrap().unwrap();
        second.admit(&chain.encode_block(&block).unwrap()).unwrap();
        first.advance_output().unwrap();
        second.advance_output().unwrap();
        loop {
            let left = first_executor.execute_next(&mut first).await.unwrap();
            let right = second_executor.execute_next(&mut second).await.unwrap();
            assert_eq!(left, right, "execution diverged at round {round}");
            let Some(receipt) = left else {
                break;
            };
            let summary: cordial_rholang::ExecutionSummary =
                serde_json::from_slice(&receipt.result).unwrap();
            assert!(summary.rejection.is_none(), "round {round}: {summary:?}");
            assert!(summary.deploys.iter().all(|deploy| deploy.outcome == cordial_rholang::DeployOutcome::Succeeded), "round {round}: {summary:?}");
            count += summary
                .deploys
                .iter()
                .filter(|deploy| deploy.outcome == cordial_rholang::DeployOutcome::Succeeded)
                .count();
        }
    }
    assert_eq!(count, 24);
    drop(first_executor);
    drop(second_executor);
    first_manager.shutdown().await.unwrap();
    second_manager.shutdown().await.unwrap();
}

