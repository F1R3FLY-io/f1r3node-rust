use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::BlockMessage;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

fn carrier_block(
    number: i64,
    parents: Vec<models::rust::block_hash::BlockHash>,
    deploys: Option<Vec<models::rust::casper::protocol::casper_message::ProcessedDeploy>>,
) -> BlockMessage {
    get_random_block(
        Some(number),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(parents),
        None,
        deploys,
        None,
        None,
        None,
        None,
    )
}

fn block_at(number: i64, seq: i32) -> BlockMessage {
    get_random_block(
        Some(number),
        Some(seq),
        None,
        None,
        None,
        None,
        Some(number),
        Some(vec![]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        None,
        Some("test".to_string()),
        None,
    )
}

/// The carrier index proves a deploy has no carrier only for scan windows at
/// or above the watermark. A restored node holds nothing below its anchor, so
/// a watermark of 0 claims coverage over history it never downloaded and turns
/// an index miss into "no carrier exists" — admitting a repeat deploy that a
/// full node rejects.
///
/// The watermark cannot be written at startup, where an empty database cannot
/// distinguish a node about to run a genesis ceremony from one about to
/// restore.
#[tokio::test]
async fn a_restore_does_not_claim_carrier_coverage_below_what_it_indexed() {
    let mut kvm = InMemoryStoreManager::new();
    let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
        .await
        .expect("dag storage");

    assert_eq!(
        dag_storage
            .ensure_carrier_watermark()
            .expect("startup watermark"),
        None,
        "an empty database has no history root yet, so it must claim nothing"
    );

    // A restore that indexed heights 140343 and up, plus the shipped genesis.
    let genesis = block_at(0, 1);
    let anchor = block_at(140_343, 2);
    dag_storage
        .insert(&genesis, InsertMode::Approved)
        .expect("shipped genesis");
    dag_storage
        .insert(&anchor, InsertMode::Normal)
        .expect("restored band");

    dag_storage
        .record_carrier_coverage_from(140_343)
        .expect("record coverage");

    let watermark = dag_storage
        .get_representation()
        .expect("dag")
        .carrier_index_watermark()
        .expect("read watermark")
        .expect("restore recorded coverage");
    assert_eq!(
        watermark, 140_343,
        "coverage starts at what the restore indexed, not at the shipped \
         genesis sitting at height 0"
    );

    assert_eq!(
        dag_storage
            .ensure_carrier_watermark()
            .expect("restart watermark"),
        Some(140_343),
        "a restart must not lower coverage already recorded"
    );
}

/// The consequence of the watermark being truthful. A deploy's only carrier
/// sits below the restore anchor, so the index cannot distinguish "no carrier
/// exists" from "the carrier is in history I never downloaded". With coverage
/// recorded at the anchor the fast path stays shut, the exact scan runs, and
/// the node DEFERS — where a full node holding the carrier rejects outright.
/// Admitting it would fork: same deploy, two verdicts.
#[tokio::test]
async fn a_restored_node_defers_a_repeat_deploy_rather_than_admitting_it() {
    use casper::rust::block_status::{BlockError, InvalidBlock};
    use casper::rust::util::construct_deploy;
    use casper::rust::validate::Validate;
    use rspace_plus_plus::rspace::history::Either;

    use crate::differential::view_holding;
    use crate::helper::block_dag_storage_fixture::with_storage;

    with_storage(|block_store, _dag| async move {
        let deploy = construct_deploy::basic_processed_deploy(7, None).unwrap();
        let sig = deploy.deploy.sig.clone();

        // carrier(9) <- held(10) <- repeat(11); the restore starts at 10.
        let carrier = carrier_block(9, vec![], Some(vec![deploy.clone()]));
        let held = carrier_block(10, vec![carrier.block_hash.clone()], Some(vec![]));
        let repeat = carrier_block(11, vec![held.block_hash.clone()], Some(vec![deploy]));
        block_store.put_block_message(&held).unwrap();
        block_store.put_block_message(&carrier).unwrap();

        let judge = |carrier_held: bool, coverage_from: i64| {
            let mut snapshot = if carrier_held {
                view_holding(&[&held, &carrier])
            } else {
                view_holding(&[&held])
            };
            if carrier_held {
                snapshot
                    .dag
                    .carrier_index
                    .read()
                    .record_once(&sig, 9, carrier.block_hash.to_vec())
                    .unwrap();
            }
            snapshot
                .dag
                .carrier_index
                .read()
                .set_watermark_if_absent(coverage_from)
                .unwrap();
            Validate::repeat_deploy(&repeat, &mut snapshot, &block_store, 50, None)
        };

        let full = judge(true, 0);
        assert!(
            matches!(
                full,
                Either::Left(BlockError::Invalid(InvalidBlock::InvalidRepeatDeploy))
            ),
            "a genesis-rooted node holding the carrier rejects the repeat; got {full:?}"
        );

        let restored = judge(false, 10);
        assert!(
            matches!(restored, Either::Left(BlockError::Undecidable(_))),
            "coverage starting at the anchor keeps the fast path shut, so the \
             node defers instead of admitting; got {restored:?}"
        );
    })
    .await
}
