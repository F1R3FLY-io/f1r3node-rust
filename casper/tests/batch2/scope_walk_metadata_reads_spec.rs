use std::sync::atomic::Ordering;

use block_storage::rust::dag::block_dag_key_value_storage::InsertMode;
use casper::rust::errors::CasperError;
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
    let start = dag.lookups_unsafe(latest).expect("start metadata");
    metadata_reads.store(0, Ordering::SeqCst);
    let mut visited = 0;
    walk_blocks_in_scope(&dag, start, 1, UnheldParent::Surface, |_| {
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

/// `top` has parents `a` and `b` above genesis. `a` keeps its height in the
/// in-memory index but loses its persisted metadata row.
async fn walk_with_a_missing_metadata_row(
    on_unheld: UnheldParent,
    matches_missing: bool,
) -> (
    Result<Option<BlockHash>, CasperError>,
    Vec<BlockHash>,
    BlockHash,
    BlockHash,
    BlockHash,
) {
    let (dag_storage, _metadata_reads) = counting_dag_storage().await;
    let validator = generate_validator(Some("scope-walk missing row"));
    let bonds = vec![Bond {
        validator: validator.clone(),
        stake: 10,
    }];
    let justification = |hash: &BlockHash| {
        vec![Justification {
            validator: validator.clone(),
            latest_block_hash: hash.clone(),
        }]
    };

    let genesis = block(0, &validator, Vec::new(), Vec::new(), &bonds);
    dag_storage
        .insert(&genesis, InsertMode::Approved)
        .expect("insert genesis");
    let a = block(
        1,
        &validator,
        vec![genesis.block_hash.clone()],
        justification(&genesis.block_hash),
        &bonds,
    );
    let b = block(
        1,
        &validator,
        vec![genesis.block_hash.clone()],
        justification(&genesis.block_hash),
        &bonds,
    );
    let top = block(
        2,
        &validator,
        vec![a.block_hash.clone(), b.block_hash.clone()],
        justification(&a.block_hash),
        &bonds,
    );
    for blk in [&a, &b, &top] {
        dag_storage
            .insert(blk, InsertMode::Normal)
            .expect("insert block");
    }
    dag_storage
        .remove_block_metadata_row_for_tests(&a.block_hash)
        .expect("remove metadata row");

    let dag = dag_storage
        .get_representation()
        .expect("dag representation");
    assert!(
        dag.block_number(&a.block_hash).is_some() && dag.lookup(&a.block_hash).unwrap().is_none(),
        "fixture: the height is indexed, the metadata row is gone"
    );
    let start = dag
        .lookups_unsafe(vec![top.block_hash.clone()])
        .expect("start metadata");
    let missing = a.block_hash.clone();
    let mut visited = Vec::new();
    let result = walk_blocks_in_scope(&dag, start, 1, on_unheld, |hash| {
        visited.push(hash.clone());
        Ok(matches_missing && *hash == missing)
    });
    (result, visited, top.block_hash, a.block_hash, b.block_hash)
}

#[tokio::test]
async fn a_skip_settled_walk_never_visits_a_block_without_a_metadata_row() {
    let (result, visited, top, _a, b) =
        walk_with_a_missing_metadata_row(UnheldParent::SkipSettled, false).await;
    assert_eq!(result.expect("skip-settled walk"), None);
    assert_eq!(
        visited,
        vec![top, b],
        "a parent without a metadata row is skipped, not recorded in scope"
    );
}

#[tokio::test]
async fn a_surface_walk_fails_on_a_block_without_a_metadata_row_before_any_match() {
    let (result, visited, top, a, _b) =
        walk_with_a_missing_metadata_row(UnheldParent::Surface, true).await;
    assert!(
        matches!(&result, Err(CasperError::BlockNotHeld(hash, _)) if *hash == a),
        "the verdict walk must surface the missing row, got {result:?}"
    );
    assert_eq!(
        visited,
        vec![top],
        "the block without a metadata row must never reach the visitor"
    );
}
