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

mod protocol_mode {
    use models::rust::casper::protocol::casper_message::BlockMessage;
    use models::rust::record_encoding::{scoped_record_encoding, RecordEncoding};

    use super::*;
    use crate::rust::casper::{check_protocol_mode, CASPER_PROTOCOL_VERSION, PROTOCOL_V6_VERSION};
    use crate::rust::genesis::contracts::proof_of_stake::ProofOfStake;
    use crate::rust::genesis::genesis::Genesis;

    fn block(version: i64) -> BlockMessage {
        let mut block = models::rust::block_implicits::get_random_block(
            None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        );
        block.header.version = version;
        block
    }

    fn pos() -> ProofOfStake {
        ProofOfStake {
            minimum_bond: 1,
            maximum_bond: 10,
            validators: vec![crate::rust::genesis::contracts::validator::Validator {
                pk: crypto::rust::public_key::PublicKey::from_bytes(&[4; 65]),
                stake: 5,
            }],
            epoch_length: 10,
            quarantine_length: 10,
            number_of_active_validators: 5,
            fault_tolerance_threshold_ppm: 0,
            max_parent_depth: 5,
            deploy_lifespan: 50,
            min_phlo_price: 1,
            pos_multi_sig_public_keys: Vec::new(),
            pos_multi_sig_quorum: 0,
        }
    }

    fn terms(policy: Option<&PhloGenesisPolicy>) -> Vec<String> {
        Genesis::default_blessed_terms_with_policy(
            &pos(),
            &Vec::new(),
            i64::MAX,
            "root",
            "F1R3CAP",
            "F1R3",
            8,
            &[],
            None,
            policy,
        )
        .into_iter()
        .map(|deploy| deploy.data.term)
        .collect()
    }

    #[test]
    fn the_shard_policy_is_the_active_protocol_6_policy_of_its_shard() {
        let policy = shard_resource_policy("root", 8).unwrap();
        assert!(policy.offered_funded_v6_active());
        policy
            .validate_context(PROTOCOL_V6_VERSION, "root", 8)
            .unwrap();
        assert!(policy
            .validate_context(PROTOCOL_V6_VERSION, "other", 8)
            .is_err());
        assert!(policy
            .validate_context(PROTOCOL_V6_VERSION, "root", 9)
            .is_err());
        assert_eq!(
            PhloGenesisPolicy::decode(&policy.encode().unwrap()).unwrap(),
            policy
        );
        assert!(shard_resource_policy("root", 256).is_err());
    }

    #[test]
    fn only_protocol_6_has_a_resource_policy() {
        assert!(protocol_resource_policy(CASPER_PROTOCOL_VERSION, "root", 8)
            .unwrap()
            .is_none());
        assert_eq!(
            protocol_resource_policy(PROTOCOL_V6_VERSION, "root", 8).unwrap(),
            Some(shard_resource_policy("root", 8).unwrap())
        );
        for version in [0, 2, 5, 7] {
            assert!(protocol_resource_policy(version, "root", 8).is_err());
        }
    }

    #[test]
    fn legacy_genesis_terms_are_dev_terms() {
        let dev = Genesis::default_blessed_terms(
            &pos(),
            &Vec::new(),
            i64::MAX,
            "root",
            "F1R3CAP",
            "F1R3",
            8,
            &[],
            None,
        )
        .into_iter()
        .map(|deploy| deploy.data.term)
        .collect::<Vec<_>>();
        assert_eq!(terms(None), dev);
        assert!(dev.iter().all(|term| !term.contains("resourcePolicy")));
        assert!(dev.iter().all(|term| !term.contains("applyCost")));
    }

    #[test]
    fn protocol_6_genesis_terms_differ_only_in_the_vault_and_the_policy() {
        let policy = shard_resource_policy("root", 8).unwrap();
        let legacy = terms(None);
        let v6 = terms(Some(&policy));
        assert_eq!(legacy.len(), v6.len());
        let changed: Vec<usize> = (0..legacy.len())
            .filter(|index| legacy[*index] != v6[*index])
            .collect();
        assert_eq!(changed.len(), 2, "{changed:?}");
        assert!(changed.iter().any(|index| v6[*index].contains("applyCost")));
        let encoded = hex::encode(policy.encode().unwrap());
        assert!(changed
            .iter()
            .any(|index| v6[*index].contains(&format!("\"{encoded}\".hexToBytes()"))));
    }

    #[test]
    fn a_node_refuses_a_shard_of_the_other_mode() {
        let legacy = shard_conf(1, CASPER_PROTOCOL_VERSION, "root");
        let v6 = shard_conf(1, PROTOCOL_V6_VERSION, "root");
        let _scope = scoped_record_encoding(RecordEncoding::Legacy);
        check_protocol_mode(&block(CASPER_PROTOCOL_VERSION), &legacy).unwrap();
        let refusal = format!(
            "{:?}",
            check_protocol_mode(&block(PROTOCOL_V6_VERSION), &legacy).unwrap_err()
        );
        assert!(refusal.contains("this shard runs protocol 6"), "{refusal}");
        assert!(refusal.contains("protocol-v6 = true"), "{refusal}");
        let refusal = format!(
            "{:?}",
            check_protocol_mode(&block(CASPER_PROTOCOL_VERSION), &v6).unwrap_err()
        );
        assert!(
            refusal.contains("this shard runs legacy protocol"),
            "{refusal}"
        );
        assert!(refusal.contains("protocol-v6 = false"), "{refusal}");
        assert!(check_protocol_mode(&block(PROTOCOL_V6_VERSION), &v6).is_err());
        drop(_scope);
        let _scope = scoped_record_encoding(RecordEncoding::Cost);
        check_protocol_mode(&block(PROTOCOL_V6_VERSION), &v6).unwrap();
        assert!(check_protocol_mode(&block(CASPER_PROTOCOL_VERSION), &legacy).is_err());
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

        #[test]
        fn the_shard_policy_is_adopted_only_at_protocol_6(local in -8i64..72) {
            let genesis = GenesisResourcePolicy {
                genesis_root: vec![9; 32].into(),
                record: shard_resource_policy("root", 8).unwrap(),
                minimum_price: 1,
            };
            proptest::prop_assert_eq!(
                genesis.adopt(&shard_conf(1, local, "root")).is_ok(),
                local == PROTOCOL_V6_VERSION
            );
        }
    }
}
