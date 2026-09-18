use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::validator::Validator;

use crate::differential::view_holding;
use crate::helper::block_util::generate_validator;

fn block_by(number: i64, parents: Vec<BlockHash>, sender: Option<Validator>) -> BlockMessage {
    get_random_block(
        Some(number),
        None,
        None,
        None,
        sender,
        None,
        None,
        Some(parents),
        None,
        Some(vec![]),
        None,
        None,
        None,
        None,
    )
}

/// Genesis is held, so it survives the unheld-slot abstention; only the
/// sender check catches it. A height-0 parent candidate bounds every walk at
/// zero, which on a restored node reaches below the restore horizon.
#[tokio::test]
async fn a_genesis_placeholder_is_not_a_parent_candidate() {
    let silent = generate_validator(Some("silent"));
    let active = generate_validator(Some("active"));

    // Genesis: height 0, no sender — the placeholder every node seeds.
    let genesis = block_by(0, vec![], Some(Validator::new()));
    let tip = block_by(102, vec![], Some(active.clone()));

    let view = view_holding(&[&genesis, &tip]);

    // The silent validator's slot points at genesis; the active one's at its
    // own block.
    assert!(
        view.dag
            .slot_is_own_testimony(&active, &tip.block_hash)
            .expect("held lookup"),
        "a validator's own signed block is testimony"
    );
    assert!(
        !view
            .dag
            .slot_is_own_testimony(&silent, &genesis.block_hash)
            .expect("held lookup"),
        "the genesis placeholder is a seed, not testimony: it is held, so the \
         unheld-slot abstention does not catch it, and citing it makes a \
         height-0 block a parent candidate that bounds every walk at zero"
    );
}
