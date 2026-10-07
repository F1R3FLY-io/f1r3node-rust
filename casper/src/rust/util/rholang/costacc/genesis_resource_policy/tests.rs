use models::rust::phlo_schedule::PhloScheduleV1;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativePhloDimension,
};

use super::*;

fn policy(minimum: u64, version: u64, shard: &str) -> GenesisResourcePolicy {
    let schedule = PhloScheduleV1 {
        protocol_version: version,
        network: b"test",
        shard: shard.as_bytes(),
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: vec![NativePhloDimension::Compute.resource_class(b"compute", 1)],
        actual_price: 10,
        compatibility_rule: native_resource_compatibility_rule(),
    };
    GenesisResourcePolicy {
        genesis_root: vec![9; 32].into(),
        record: PhloGenesisPolicy::from_schedule(&schedule).unwrap(),
        minimum_price: minimum,
    }
}

fn shard_conf(minimum: i64, version: i64, shard: &str) -> CasperShardConf {
    CasperShardConf {
        min_phlo_price: minimum,
        casper_version: version,
        shard_name: shard.into(),
        ..CasperShardConf::new()
    }
}

pub(in crate::rust::util::rholang::costacc) fn retained_record_context() -> AdoptedResourcePolicy {
    policy(0, 6, "root")
        .adopt(&shard_conf(0, 6, "root"))
        .unwrap()
}

#[test]
fn adoption_requires_the_authenticated_genesis_context() {
    let adopted = policy(10, 6, "root")
        .adopt(&shard_conf(10, 6, "root"))
        .unwrap();
    assert_eq!(adopted.genesis().genesis_root().as_ref(), &[9; 32]);
    assert!(!adopted.offered_funded_v6_active());
    let mut fresh = policy(10, 6, "root");
    fresh.record = fresh.record.with_offered_funded_v6_active();
    assert!(fresh
        .adopt(&shard_conf(10, 6, "root"))
        .unwrap()
        .offered_funded_v6_active());
    for (minimum, version, shard) in [
        (-1, 6, "root"),
        (11, 6, "root"),
        (10, -1, "root"),
        (10, 7, "root"),
        (10, 6, "other"),
    ] {
        assert!(policy(10, 6, "root")
            .adopt(&shard_conf(minimum, version, shard))
            .is_err());
    }
}

#[test]
fn genesis_policy_commitment_binds_exact_v1_or_v2_record() {
    let historical = policy(10, 6, "root");
    let historical_bytes = historical.record().encode().unwrap();
    let historical_commitment = historical.commitment().unwrap();
    assert_eq!(
        historical_commitment,
        <[u8; 32]>::try_from(Blake2b256::hash_parts([
            GENESIS_POLICY_COMMITMENT_DOMAIN,
            historical_bytes.as_slice(),
        ]))
        .unwrap()
    );
    assert_eq!(
        historical
            .clone()
            .adopt(&shard_conf(10, 6, "root"))
            .unwrap()
            .genesis_policy_commitment()
            .unwrap(),
        historical_commitment
    );
    let mut fresh = historical;
    fresh.record = fresh.record.with_offered_funded_v6_active();
    assert_ne!(fresh.record().encode().unwrap(), historical_bytes);
    assert_ne!(fresh.commitment().unwrap(), historical_commitment);
}

#[test]
fn acquisition_terms_bind_policy_and_reject_changed_components() {
    let adopted = policy(10, 6, "root")
        .adopt(&shard_conf(10, 6, "root"))
        .unwrap();
    let original = adopted.genesis().record().schedule().unwrap();
    let bytes = original.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let checked = adopted.check_acquisition_terms(&bytes).unwrap();
    assert_eq!(checked.bytes(), bytes);
    assert_eq!(checked.schedule(), &original);

    let mut changed = original.clone();
    changed.shard = b"other";
    let changed_bytes = changed.encode(PhloGenesisPolicy::LIMITS).unwrap();
    assert!(adopted.check_acquisition_terms(&changed_bytes).is_err());

    changed = original.clone();
    changed.actual_price = 25;
    let repriced = changed.encode(PhloGenesisPolicy::LIMITS).unwrap();
    assert_eq!(
        adopted
            .check_acquisition_terms(&repriced)
            .unwrap()
            .schedule()
            .actual_price,
        25
    );
}

#[test]
fn acquisition_terms_reject_truncated_and_oversized_records() {
    let adopted = policy(10, 6, "root")
        .adopt(&shard_conf(10, 6, "root"))
        .unwrap();
    let bytes = adopted
        .genesis()
        .record()
        .schedule()
        .unwrap()
        .encode(PhloGenesisPolicy::LIMITS)
        .unwrap();
    for length in 0..bytes.len() {
        assert!(adopted.check_acquisition_terms(&bytes[..length]).is_err());
    }
    let mut trailing = bytes;
    trailing.push(0);
    assert!(adopted.check_acquisition_terms(&trailing).is_err());
    assert!(adopted
        .check_acquisition_terms(&vec![0; PhloGenesisPolicy::LIMITS.wire.total_bytes + 1])
        .is_err());
}

#[test]
fn token_metadata_policy_getter_is_opt_in() {
    use crate::rust::genesis::contracts::standard_deploys;

    let record = policy(10, 6, "root").record;
    let historic = standard_deploys::token_metadata("F1R3", "REV", 8, "root");
    let absent = standard_deploys::token_metadata_with_policy("F1R3", "REV", 8, "root", None);
    assert_eq!(historic.data.term, absent.data.term);
    assert!(!historic.data.term.contains("resourcePolicy"));

    let present =
        standard_deploys::token_metadata_with_policy("F1R3", "REV", 8, "root", Some(&record));
    let encoded = hex::encode(record.encode().unwrap());
    assert!(present.data.term.contains("resourcePolicy"));
    assert!(present
        .data
        .term
        .contains(&format!("\"{encoded}\".hexToBytes()")));
}

/// DR-99: `resolve_policy_genesis` returns the authenticated genesis block of
/// the approved block's chain, or fails closed. It never returns the anchor of
/// a joined node.
mod resolve_policy_genesis_layouts {
    use block_storage::rust::dag::block_dag_key_value_storage::{
        BlockDagKeyValueStorage, InsertMode,
    };
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use models::rust::block_hash::BlockHash;
    use models::rust::block_implicits::get_random_block;
    use models::rust::casper::protocol::casper_message::BlockMessage;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::super::resolve_policy_genesis;
    use crate::rust::util::proto_util;

    fn hashed_block(number: i64, parents: Vec<BlockHash>, shard: &str) -> BlockMessage {
        let mut block = get_random_block(
            Some(number),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(parents),
            Some(vec![]),
            Some(vec![]),
            None,
            Some(vec![]),
            Some(shard.to_string()),
            None,
        );
        block.block_hash = proto_util::hash_block(&block);
        block
    }

    struct JoinedLayout {
        dag: BlockDagKeyValueStorage,
        store: KeyValueBlockStore,
        genesis: BlockMessage,
        anchor: BlockMessage,
    }

    /// A restored node: the anchor is the approved block of the DAG, and the
    /// genesis block is neither registered nor stored yet.
    async fn joined_layout() -> JoinedLayout {
        let mut kvm = InMemoryStoreManager::new();
        let dag = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("in-memory DAG storage");
        let store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("in-memory block store");
        let genesis = hashed_block(0, vec![], "root");
        let anchor = hashed_block(5, vec![genesis.block_hash.clone()], "root");
        dag.insert(&anchor, InsertMode::Approved)
            .expect("the anchor enters the DAG");
        store
            .put_block_message(&anchor)
            .expect("the anchor is stored");
        JoinedLayout {
            dag,
            store,
            genesis,
            anchor,
        }
    }

    fn refusal(layout: &JoinedLayout) -> String {
        resolve_policy_genesis(&layout.anchor, &layout.dag, &layout.store)
            .expect_err("the resolver must fail closed")
            .to_string()
    }

    #[tokio::test]
    async fn a_parentless_approved_block_is_its_own_policy_genesis() {
        let layout = joined_layout().await;
        let resolved = resolve_policy_genesis(&layout.genesis, &layout.dag, &layout.store)
            .expect("a genesis participant needs no lookup");
        assert_eq!(resolved, layout.genesis);
    }

    #[tokio::test]
    async fn a_joined_node_resolves_its_shipped_genesis() {
        let layout = joined_layout().await;
        layout
            .dag
            .record_genesis_hash(layout.genesis.block_hash.clone())
            .expect("the register records the learned hash");
        layout
            .store
            .put_block_message(&layout.genesis)
            .expect("the shipped genesis is stored");
        let resolved = resolve_policy_genesis(&layout.anchor, &layout.dag, &layout.store)
            .expect("a joined node resolves its verified genesis copy");
        assert_eq!(resolved, layout.genesis);
        assert_ne!(resolved, layout.anchor);
    }

    #[tokio::test]
    async fn a_node_without_the_register_uses_its_height_zero_block() {
        let layout = joined_layout().await;
        layout
            .dag
            .insert(&layout.genesis, InsertMode::Approved)
            .expect("a ceremony node holds genesis at height zero");
        layout
            .store
            .put_block_message(&layout.genesis)
            .expect("the genesis is stored");
        let resolved = resolve_policy_genesis(&layout.anchor, &layout.dag, &layout.store)
            .expect("the height-zero block is the learned genesis");
        assert_eq!(resolved, layout.genesis);
    }

    #[tokio::test]
    async fn a_node_that_has_not_learned_the_genesis_fails_closed() {
        let layout = joined_layout().await;
        assert!(refusal(&layout).contains("has not learned"));
    }

    #[tokio::test]
    async fn a_node_without_a_genesis_copy_fails_closed() {
        let layout = joined_layout().await;
        layout
            .dag
            .record_genesis_hash(layout.genesis.block_hash.clone())
            .expect("the register records the learned hash");
        assert!(refusal(&layout).contains("holds no copy"));
    }

    #[tokio::test]
    async fn a_copy_that_does_not_rehash_to_the_register_fails_closed() {
        let layout = joined_layout().await;
        layout
            .dag
            .record_genesis_hash(layout.genesis.block_hash.clone())
            .expect("the register records the learned hash");
        let mut forged = layout.genesis.clone();
        forged.body.state.post_state_hash = vec![0x5a; 32].into();
        layout
            .store
            .put(layout.genesis.block_hash.clone(), &forged)
            .expect("the forged copy is stored under the learned hash");
        assert!(refusal(&layout).contains("not an authenticated genesis"));
    }

    #[tokio::test]
    async fn a_valid_block_stored_under_another_hash_fails_closed() {
        let layout = joined_layout().await;
        layout
            .dag
            .record_genesis_hash(layout.genesis.block_hash.clone())
            .expect("the register records the learned hash");
        let other = hashed_block(0, vec![], "root");
        assert_ne!(other.block_hash, layout.genesis.block_hash);
        layout
            .store
            .put(layout.genesis.block_hash.clone(), &other)
            .expect("another genesis is stored under the learned hash");
        assert!(refusal(&layout).contains("not an authenticated genesis"));
    }

    #[tokio::test]
    async fn a_learned_block_with_parents_fails_closed() {
        let layout = joined_layout().await;
        let middle = hashed_block(3, vec![layout.genesis.block_hash.clone()], "root");
        layout
            .dag
            .record_genesis_hash(middle.block_hash.clone())
            .expect("the register records a block with parents");
        layout
            .store
            .put_block_message(&middle)
            .expect("the block is stored");
        assert!(refusal(&layout).contains("not an authenticated genesis"));
    }

    #[tokio::test]
    async fn a_genesis_of_another_shard_fails_closed() {
        let layout = joined_layout().await;
        let foreign = hashed_block(0, vec![], "other");
        layout
            .dag
            .record_genesis_hash(foreign.block_hash.clone())
            .expect("the register records a genesis of another shard");
        layout
            .store
            .put_block_message(&foreign)
            .expect("the foreign genesis is stored");
        assert!(refusal(&layout).contains("not an authenticated genesis"));
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

    /// Extracted from `GenesisVersionAdoption.v`: `adopted_policy_passes_adopt_check`
    /// and `unadopted_mismatch_fails_adopt_check`. The approved header version equals
    /// the schedule version (`context_valid`), so the adopted running version always
    /// passes `adopt`, and an unadopted mismatched local version always fails it.
    #[test]
    fn adopted_version_passes_adopt_and_unadopted_mismatch_fails(
        schedule_version in 1u64..64,
        local in -8i64..72,
    ) {
        let header = i64::try_from(schedule_version).expect("small schedule version");
        let genesis = policy(10, schedule_version, "root");
        let running = crate::rust::casper::adopted_casper_version(Some(&genesis), header, local);
        proptest::prop_assert!(policy(10, schedule_version, "root")
            .adopt(&shard_conf(10, running, "root"))
            .is_ok());
        if local != header {
            proptest::prop_assert!(policy(10, schedule_version, "root")
                .adopt(&shard_conf(10, local, "root"))
                .is_err());
        }
    }

    /// Extracted from `GenesisVersionAdoption.v`: `joined_node_runs_the_genesis_version`
    /// and `joined_anchor_version_mismatch_fails_adopt_check` (DR-99). A joined node
    /// adopts the header version of its anchor, and the adopt check against the sealed
    /// genesis policy passes exactly when that version is the genesis schedule
    /// version. So a joined node runs the genesis version or does not start.
    #[test]
    fn joined_anchor_version_adopts_only_the_genesis_version(
        schedule_version in 1u64..64,
        anchor_version in -8i64..72,
        local in -8i64..72,
    ) {
        let genesis = policy(10, schedule_version, "root");
        let running =
            crate::rust::casper::adopted_casper_version(Some(&genesis), anchor_version, local);
        proptest::prop_assert_eq!(running, anchor_version);
        let accepted = policy(10, schedule_version, "root")
            .adopt(&shard_conf(10, running, "root"))
            .is_ok();
        proptest::prop_assert_eq!(
            accepted,
            u64::try_from(anchor_version).ok() == Some(schedule_version)
        );
    }
}
