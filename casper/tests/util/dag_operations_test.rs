// See casper/src/test/scala/coop/rchain/casper/util/DagOperationsTest.scala

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage;
use casper::rust::util::dag_operations::DagOperations;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::BlockMessage;
use shared::rust::dag::dag_ops;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::{create_block, create_genesis_block};

#[test]
fn bf_traverse_f_should_lazily_breadth_first_traverse_a_dag_with_effectful_neighbours() {
    // Port of Scala test: val stream = DagOps.bfTraverseF[Id, Int](List(1))(i => List(i * 2, i * 3))
    // stream.take(10).toList shouldBe List(1, 2, 3, 4, 6, 9, 8, 12, 18, 27)
    //
    // Key difference: Scala's StreamT is lazy - it generates elements only when needed.
    // When .take(10) is called, the stream stops after producing exactly 10 elements.
    // Rust's bf_traverse is eager - it tries to traverse the entire graph before returning.
    // Since the graph i -> [i*2, i*3] is infinite, we need to limit neighbor generation
    // to simulate the lazy behavior and prevent infinite traversal/overflow.

    let neighbors = |i: &i32| vec![i * 2, i * 3];

    let mut count = 0;
    let result = dag_ops::bf_traverse(vec![1], |node| {
        count += 1;
        if count > 10 {
            vec![]
        } else {
            neighbors(node)
        }
    })
    .into_iter()
    .take(10)
    .collect::<Vec<_>>();

    let expected = vec![1, 2, 3, 4, 6, 9, 8, 12, 18, 27];
    assert_eq!(result, expected);
}

#[tokio::test]
async fn lowest_common_universal_ancestor_should_be_computed_properly() {
    fn create_block_with_meta(
        store: &mut KeyValueBlockStore,
        dag_store: &mut IndexedBlockDagStorage,
        genesis: &BlockMessage,
        bh: &[BlockHash],
    ) -> BlockMetadata {
        let block = create_block(
            store,
            dag_store,
            bh.to_vec(),
            genesis,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        BlockMetadata::from_block(&block, false, None, None)
    }

    fn create_block_with_meta_and_seq(
        store: &mut KeyValueBlockStore,
        dag_store: &mut IndexedBlockDagStorage,
        genesis: &BlockMessage,
        seq_num: i32,
        bh: &[BlockHash],
    ) -> BlockMetadata {
        let block = create_block(
            store,
            dag_store,
            bh.to_vec(),
            genesis,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(seq_num),
            None,
            None,
        );
        BlockMetadata::from_block(&block, false, None, None)
    }

    fn block_metadata_to_block_hash(metadata: &BlockMetadata) -> BlockHash {
        metadata.block_hash.clone()
    }

    with_storage(|mut block_store, mut block_dag_storage| async move {
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let genesis_meta = BlockMetadata::from_block(&genesis, false, None, None);

        // DAG Looks like this:
        //
        //        b9   b10
        //          \ /
        //          b8
        //          / \
        //        b6   b7
        //      |  \ /  \
        //       |   b4  b5
        //       |    \ /
        //       b2    b3
        //         \  /
        //          b1
        //          |
        //         genesis

        let b1 = create_block_with_meta(
            &mut block_store,
            &mut block_dag_storage,
            &genesis,
            std::slice::from_ref(&genesis.block_hash),
        );

        let b2 = create_block_with_meta_and_seq(
            &mut block_store,
            &mut block_dag_storage,
            &genesis,
            2,
            &[block_metadata_to_block_hash(&b1)],
        );

        let b3 = create_block_with_meta_and_seq(
            &mut block_store,
            &mut block_dag_storage,
            &genesis,
            2,
            &[block_metadata_to_block_hash(&b1)],
        );

        let b4 = create_block_with_meta(&mut block_store, &mut block_dag_storage, &genesis, &[
            block_metadata_to_block_hash(&b3),
        ]);

        let b5 = create_block_with_meta(&mut block_store, &mut block_dag_storage, &genesis, &[
            block_metadata_to_block_hash(&b3),
        ]);

        let b6 = create_block_with_meta(&mut block_store, &mut block_dag_storage, &genesis, &[
            block_metadata_to_block_hash(&b2),
            block_metadata_to_block_hash(&b4),
        ]);

        let b7 = create_block_with_meta(&mut block_store, &mut block_dag_storage, &genesis, &[
            block_metadata_to_block_hash(&b4),
            block_metadata_to_block_hash(&b5),
        ]);

        let b8 = create_block_with_meta(&mut block_store, &mut block_dag_storage, &genesis, &[
            block_metadata_to_block_hash(&b6),
            block_metadata_to_block_hash(&b7),
        ]);

        let b9 = create_block_with_meta(&mut block_store, &mut block_dag_storage, &genesis, &[
            block_metadata_to_block_hash(&b8),
        ]);

        let b10 = create_block_with_meta(&mut block_store, &mut block_dag_storage, &genesis, &[
            block_metadata_to_block_hash(&b8),
        ]);

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");

        let result = DagOperations::lowest_universal_common_ancestor(&b1, &b5, &dag, &genesis_meta)
            .await
            .unwrap();
        assert_eq!(result, b1);

        let result = DagOperations::lowest_universal_common_ancestor(&b2, &b3, &dag, &genesis_meta)
            .await
            .unwrap();
        assert_eq!(result, b1);

        let result = DagOperations::lowest_universal_common_ancestor(&b3, &b2, &dag, &genesis_meta)
            .await
            .unwrap();
        assert_eq!(result, b1);

        let result = DagOperations::lowest_universal_common_ancestor(&b6, &b7, &dag, &genesis_meta)
            .await
            .unwrap();
        assert_eq!(result, b1);

        let result = DagOperations::lowest_universal_common_ancestor(&b2, &b2, &dag, &genesis_meta)
            .await
            .unwrap();
        assert_eq!(result, b2);

        let result =
            DagOperations::lowest_universal_common_ancestor(&b10, &b9, &dag, &genesis_meta)
                .await
                .unwrap();
        assert_eq!(result, b8);

        let result = DagOperations::lowest_universal_common_ancestor(&b3, &b7, &dag, &genesis_meta)
            .await
            .unwrap();
        assert_eq!(result, b3);

        let result = DagOperations::lowest_universal_common_ancestor(&b3, &b8, &dag, &genesis_meta)
            .await
            .unwrap();
        assert_eq!(result, b1);

        let result = DagOperations::lowest_universal_common_ancestor(&b4, &b5, &dag, &genesis_meta)
            .await
            .unwrap();
        assert_eq!(result, b3);

        let result = DagOperations::lowest_universal_common_ancestor(&b4, &b6, &dag, &genesis_meta)
            .await
            .unwrap();
        assert_eq!(result, b1);

        let result = DagOperations::lowest_universal_common_ancestor(&b7, &b7, &dag, &genesis_meta)
            .await
            .unwrap();
        assert_eq!(result, b7);

        let result = DagOperations::lowest_universal_common_ancestor(&b7, &b8, &dag, &genesis_meta)
            .await
            .unwrap();
        assert_eq!(result, b1);

        let result = DagOperations::lowest_universal_common_ancestor(&b8, &b9, &dag, &genesis_meta)
            .await
            .unwrap();
        assert_eq!(result, b8);

        let result = DagOperations::lowest_universal_common_ancestor_many(
            &[b8.clone(), b9.clone(), b10.clone()],
            &dag,
            &genesis_meta,
        )
        .await
        .unwrap();
        assert_eq!(result, b8);

        let result = DagOperations::lowest_universal_common_ancestor_many(
            &[b2.clone(), b3.clone(), b4.clone()],
            &dag,
            &genesis_meta,
        )
        .await
        .unwrap();
        assert_eq!(result, b1);

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("Test should complete successfully");
}

/// A DAG restored from LFS holds blocks whose parents were never downloaded:
/// the node syncs a window below its approved block and stops. The LUCA walk
/// descends by parent, so on a braided DAG — where a block's secondary parents
/// reach below the approved block — it walks out of that window and asks for a
/// block that is not there, which fails the whole snapshot.
///
/// The approved block is the floor: it is finalized, so no fork beneath it is
/// live, and it is the oldest block the DAG is guaranteed to hold. Reaching it
/// means the answer is it.
#[tokio::test]
async fn luca_stops_at_the_approved_block_when_the_dag_is_truncated() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        // Never inserted: the history below the sync window.
        let truncated = BlockHash::from(b"parent-below-the-lfs-frontier".to_vec());

        let (f1, f2, anchor, t1, t2) = {
            let mut build = |parents: Vec<BlockHash>, number: i32| -> BlockMetadata {
                let block = create_block(
                    &mut block_store,
                    &mut block_dag_storage,
                    parents,
                    &genesis,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(number),
                    None,
                    None,
                );
                BlockMetadata::from_block(&block, false, None, None)
            };

            // Retained below the approved block, their own parents truncated away.
            let f1 = build(vec![truncated.clone()], 5);
            let f2 = build(vec![truncated.clone()], 5);
            let anchor = build(vec![f1.block_hash.clone()], 10);
            // Braided: each tip reaches the approved block AND a block under it,
            // so the two tips do not converge until the walk is already below.
            let t1 = build(vec![anchor.block_hash.clone(), f1.block_hash.clone()], 11);
            let t2 = build(vec![anchor.block_hash.clone(), f2.block_hash.clone()], 11);
            (f1, f2, anchor, t1, t2)
        };

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");

        let result = DagOperations::lowest_universal_common_ancestor_many(
            &[t1.clone(), t2.clone()],
            &dag,
            &anchor,
        )
        .await
        .expect("the walk must stop at the approved block, not read past the sync window");
        assert_eq!(
            result, anchor,
            "two tips that only converge below the approved block must resolve to it"
        );

        // A stale input already below the floor carries no fork-choice information;
        // the answer is still the floor, not the stale block the walk happened to end on.
        let result = DagOperations::lowest_universal_common_ancestor_many(
            &[t1.clone(), f2.clone()],
            &dag,
            &anchor,
        )
        .await
        .expect("a below-floor input must not drag the walk past the sync window");
        assert_eq!(result, anchor, "a below-floor input resolves to the floor");

        let _ = f1;

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("Test should complete successfully");
}
