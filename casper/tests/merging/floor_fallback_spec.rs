use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::interpreter_util::{
    compute_deploys_checkpoint, compute_parents_post_state,
};
use casper::rust::util::rholang::runtime_manager::{MergedPreState, RuntimeManager};
use casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::signed::Signed;
use dashmap::DashSet;
use models::rust::block::state_hash::StateHash;
use models::rust::block_implicits;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData, ProcessedDeploy};
use prost::bytes::Bytes;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::system_processes::BlockData;

use super::support::channel_values;
use crate::util::genesis_builder::GenesisContext;
use crate::util::rholang::resources::{
    block_dag_storage_from_dyn, genesis_context, mergeable_store_from_dyn,
    mk_test_rnode_store_manager_from_genesis,
};

fn deploy(source: &str, key: &PrivateKey) -> Signed<DeployData> {
    construct_deploy::source_deploy_now_full(
        source.to_string(),
        Some(5_000_000),
        None,
        Some(key.clone()),
        None,
        None,
    )
    .unwrap()
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .expect("system clock after the epoch")
}

struct Chain {
    context: GenesisContext,
    runtime_manager: RuntimeManager,
    block_store: KeyValueBlockStore,
    dag_storage: BlockDagKeyValueStorage,
    genesis: BlockMessage,
    validator: Bytes,
    seq: i32,
}

impl Chain {
    async fn new() -> Self {
        let context = genesis_context().await.unwrap();
        let genesis = context.genesis_block.clone();
        let validator = context.validator_pks()[0].bytes.clone();
        let mut kvm = mk_test_rnode_store_manager_from_genesis(&context);
        let rspace_store = kvm.r_space_stores().await.unwrap();
        let mergeable_store = mergeable_store_from_dyn(&mut *kvm).await.unwrap();
        let (runtime_manager, _) = RuntimeManager::create_with_history(
            rspace_store,
            mergeable_store,
            Genesis::default_mergeable_tags_arc(),
            ExternalServices::noop(),
        );
        let block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
            .await
            .unwrap();
        let dag_storage = block_dag_storage_from_dyn(&mut *kvm).await.unwrap();
        block_store.put_block_message(&genesis).unwrap();
        dag_storage.insert(&genesis, InsertMode::Approved).unwrap();
        Self {
            context,
            runtime_manager,
            block_store,
            dag_storage,
            genesis,
            validator,
            seq: 0,
        }
    }

    fn vault(&self, i: usize) -> PrivateKey {
        let vaults = &self.context.genesis_vaults;
        vaults[i % vaults.len()].0.clone()
    }

    fn snapshot(&self) -> CasperSnapshot {
        let mut snapshot = CasperSnapshot::new(
            self.dag_storage.get_representation().unwrap(),
            self.genesis.block_hash.clone(),
        );
        snapshot.last_finalized_block = self.genesis.block_hash.clone();
        snapshot.max_seq_nums = HashMap::from([(self.validator.clone(), 0)]);
        let mut shard_conf = CasperShardConf::new();
        shard_conf.shard_name = self.genesis.shard_id.clone();
        shard_conf.max_parent_depth = 0;
        shard_conf.deploy_lifespan = 50;
        snapshot.on_chain_state = OnChainCasperState {
            shard_conf,
            bonds_map: HashMap::from([(self.validator.clone(), 100.into())]),
            active_validators: vec![self.validator.clone()],
        };
        snapshot.deploys_in_scope = std::sync::Arc::new(DashSet::new());
        snapshot
    }

    async fn block(
        &mut self,
        parents: &[&BlockMessage],
        deploys: Vec<Signed<DeployData>>,
        number: i64,
    ) -> BlockMessage {
        self.seq += 1;
        let raw = block_implicits::get_random_block(
            Some(number),
            Some(self.seq),
            Some(StateHash::default()),
            Some(StateHash::default()),
            Some(self.validator.clone()),
            Some(1),
            Some(now_millis()),
            Some(parents.iter().map(|p| p.block_hash.clone()).collect()),
            Some(Vec::new()),
            Some(
                deploys
                    .iter()
                    .cloned()
                    .map(ProcessedDeploy::empty)
                    .collect(),
            ),
            Some(Vec::new()),
            Some(self.genesis.body.state.bonds.clone()),
            Some(self.genesis.shard_id.clone()),
            None,
        );
        let snapshot = self.snapshot();
        let checkpoint = compute_deploys_checkpoint(
            &mut self.block_store,
            parents.iter().map(|p| (*p).clone()).collect(),
            deploys,
            Vec::<SystemDeployEnum>::new(),
            &snapshot,
            &self.runtime_manager,
            BlockData::from_block(&raw),
            HashMap::new(),
            None,
            None,
            None,
        )
        .await
        .expect("block checkpoint");
        assert!(
            checkpoint.deploys.iter().all(|d| !d.is_failed),
            "every deploy of block #{number} must execute"
        );
        let mut block = raw;
        block.body.state.pre_state_hash = checkpoint.pre_state_hash;
        block.body.state.post_state_hash = checkpoint.post_state_hash;
        block.body.deploys = checkpoint.deploys;
        block.body.rejected_deploys = checkpoint.rejected_deploys;
        block.body.system_deploys = checkpoint.system_deploys;
        block.body.state.bonds = checkpoint.bonds;
        block.body.applied_from_scope = checkpoint.applied_from_scope;
        block.body.merge_base = checkpoint.merge_base.unwrap_or_default();
        self.block_store.put_block_message(&block).unwrap();
        self.dag_storage.insert(&block, InsertMode::Normal).unwrap();
        block
    }

    async fn merge(&self, parents: &[&BlockMessage]) -> MergedPreState {
        compute_parents_post_state(
            &self.block_store,
            parents.iter().map(|p| (*p).clone()).collect(),
            &self.snapshot(),
            &self.runtime_manager,
            &BTreeMap::new(),
            None,
            None,
            None,
        )
        .await
        .expect("parents post-state")
    }

    async fn holds(&self, state: &StateHash, channel: &str) -> bool {
        !channel_values(&self.runtime_manager, state, channel)
            .await
            .is_empty()
    }
}

/// M merges C and F on base C, so F's consume of the token loses and M's
/// state lacks it. Once F is the floor M's child inherits, the child's merge
/// re-bases on F, whose state holds neither C's nor M's deploy: each must
/// either reach the merged state or leave a rejection record.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_merge_re_based_on_the_floor_accounts_for_the_main_parents_work() {
    let mut chain = Chain::new().await;
    let genesis = chain.genesis.clone();
    let p = chain
        .block(
            &[&genesis],
            vec![deploy(r#"@"ff-token"!(0)"#, &chain.vault(0))],
            1,
        )
        .await;
    let f_deploy = deploy(r#"for (_ <- @"ff-token") { @"ff-f"!(1) }"#, &chain.vault(1));
    let c_deploy = deploy(r#"for (_ <- @"ff-token") { @"ff-c"!(1) }"#, &chain.vault(2));
    let m_deploy = deploy(r#"@"ff-m"!(1)"#, &chain.vault(3));
    let (f_sig, c_sig, m_sig) = (
        f_deploy.sig.clone(),
        c_deploy.sig.clone(),
        m_deploy.sig.clone(),
    );
    let f = chain.block(&[&p], vec![f_deploy], 2).await;
    let c = chain.block(&[&p], vec![c_deploy], 2).await;
    let m = chain.block(&[&c, &f], vec![m_deploy], 3).await;
    assert!(
        m.body.rejected_deploys.iter().any(|r| r.sig == f_sig),
        "M's merge must reject F's consume"
    );

    chain
        .dag_storage
        .get_representation()
        .unwrap()
        .put_cached_floor(m.block_hash.clone(), f.block_hash.clone())
        .unwrap();
    let merged = chain.merge(&[&m]).await;

    assert_eq!(merged.merge_base.as_ref(), Some(&f.block_hash));
    assert!(chain.holds(&merged.state, "ff-f").await);
    let recorded: HashSet<Bytes> = merged
        .rejected_user
        .iter()
        .filter(|r| !r.duplicate)
        .map(|r| r.sig.clone())
        .collect();
    assert!(recorded.contains(&c_sig));
    assert!(!chain.holds(&merged.state, "ff-c").await);
    let m_applied = merged.applied_from_scope.contains(&m_sig);
    assert_eq!(m_applied, chain.holds(&merged.state, "ff-m").await);
    assert!(m_applied || recorded.contains(&m_sig));
}
