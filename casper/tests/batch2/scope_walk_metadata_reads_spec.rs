use std::sync::atomic::Ordering;

use block_storage::rust::dag::block_dag_key_value_storage::InsertMode;
use casper::rust::util::proto_util::{walk_blocks_in_scope, UnheldParent};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond, Justification};
use models::rust::validator::Validator;

use super::clique_oracle_metadata_reads_spec::{block, counting_dag_storage};
use crate::helper::block_util::generate_validator;

const ROUNDS: i64 = 6;

/// Metadata rows one in-scope walk reads, and the blocks it visits, over a
/// DAG where every block of a round has the whole previous round as parents
/// — the shape of a merging committee of `validator_count` validators.
async fn metadata_reads_for_one_walk(validator_count: usize) -> (usize, usize) {
    let (dag_storage, metadata_reads) = counting_dag_storage().await;

    let validators: Vec<Validator> = (0..validator_count)
        .map(|i| generate_validator(Some(&format!("scope-walk {}", i))))
        .collect();
    let bonds: Vec<Bond> = validators
        .iter()
        .map(|validator| Bond {
            validator: validator.clone(),
            stake: 10,
        })
        .collect();

    let genesis = block(0, &validators[0], Vec::new(), Vec::new(), &bonds);
    dag_storage
        .insert(&genesis, InsertMode::Approved)
        .expect("insert genesis");

    let mut latest: Vec<BlockHash> = vec![genesis.block_hash.clone(); validator_count];
    for height in 1..=ROUNDS {
        let justifications: Vec<Justification> = validators
            .iter()
            .zip(latest.iter())
            .map(|(validator, hash)| Justification {
                validator: validator.clone(),
                latest_block_hash: hash.clone(),
            })
            .collect();
        let mut parents: Vec<BlockHash> = Vec::new();
        for hash in &latest {
            if !parents.contains(hash) {
                parents.push(hash.clone());
            }
        }
        let round: Vec<BlockMessage> = validators
            .iter()
            .map(|validator| {
                block(
                    height,
                    validator,
                    parents.clone(),
                    justifications.clone(),
                    &bonds,
                )
            })
            .collect();
        for b in &round {
            dag_storage
                .insert(b, InsertMode::Normal)
                .expect("insert block");
        }
        latest = round.iter().map(|b| b.block_hash.clone()).collect();
    }

    let dag = dag_storage
        .get_representation()
        .expect("dag representation");
    metadata_reads.store(0, Ordering::SeqCst);
    let mut visited = 0;
    walk_blocks_in_scope(&dag, latest, 1, UnheldParent::Surface, |_| {
        visited += 1;
        Ok(false)
    })
    .expect("in-scope walk");
    (metadata_reads.load(Ordering::SeqCst), visited)
}

#[tokio::test]
async fn an_in_scope_walk_reads_each_block_metadata_once() {
    for validator_count in [4, 16] {
        let (reads, visited) = metadata_reads_for_one_walk(validator_count).await;
        assert_eq!(
            visited,
            validator_count * ROUNDS as usize,
            "the walk must visit every block of the window once"
        );
        assert!(
            reads <= visited,
            "an in-scope walk over {} validators read {} block-metadata rows for {} blocks: it \
             must not deserialize metadata per parent edge",
            validator_count,
            reads,
            visited
        );
    }
}
