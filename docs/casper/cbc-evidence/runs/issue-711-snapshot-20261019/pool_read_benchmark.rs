use std::collections::HashSet;
use std::hint::black_box;
use std::time::Instant;

use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

#[tokio::main]
async fn main() {
    println!("pool_size,scope_match,repeats,old_ms,new_ms,speedup,old_full_reads,new_full_reads");
    for size in [2_100, 15_100] {
        let mut kvm = InMemoryStoreManager::new();
        let mut store = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let deploys: Vec<_> = (0..size)
            .map(|time_stamp| {
                Signed::create(
                    DeployData {
                        term: "Nil".to_string(),
                        time_stamp,
                        phlo_price: 1,
                        phlo_limit: 100_000,
                        valid_after_block_number: 0,
                        shard_id: "benchmark".to_string(),
                        expiration_timestamp: None,
                        parameters: Vec::new(),
                    },
                    Box::new(Secp256k1),
                    PrivateKey::from_bytes(&[1; 32]),
                )
                .unwrap()
            })
            .collect();
        let signatures: HashSet<_> = deploys.iter().map(|deploy| deploy.sig.clone()).collect();
        store.add(deploys).unwrap();
        black_box(store.read_all().unwrap());
        for scope_match in [true, false] {
            let repeats = 3;
            let in_scope =
                |deploy: &Signed<DeployData>| scope_match && signatures.contains(&deploy.sig);
            let before = Instant::now();
            for _ in 0..repeats {
                black_box(store.read_all().unwrap().iter().any(in_scope));
                for _ in 0..3 {
                    black_box(store.read_all().unwrap());
                }
            }
            let old_ms = before.elapsed().as_secs_f64() * 1_000.0 / f64::from(repeats);
            let before = Instant::now();
            for _ in 0..repeats {
                black_box(store.any(|deploy| Ok(in_scope(deploy))).unwrap());
                black_box(store.read_all().unwrap());
            }
            let new_ms = before.elapsed().as_secs_f64() * 1_000.0 / f64::from(repeats);
            println!(
                "{size},{scope_match},{repeats},{old_ms:.3},{new_ms:.3},{:.3},4,1",
                old_ms / new_ms
            );
        }
    }
}
