//! State membership from recorded construction facts. Every block records
//! how its state was built — `merge_base` (the block whose committed state
//! its pre-state derives from), `applied_from_scope` (the user chains its
//! merge re-applied onto that base), and `deploys` (fresh executions) — so
//! whether a sig's effect is in a block's committed state is a walk over
//! recorded pointers, never a re-derivation of lineage and never a probe
//! of the state's shape.

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;

use crate::rust::errors::CasperError;

/// True iff `block_hash`'s committed state contains `sig`'s effect: some
/// block on its base lineage either executed it fresh (a non-failed
/// `deploys` entry) or re-applied its chain from scope
/// (`applied_from_scope`). The lineage is the recorded `merge_base` chain;
/// where the recorded base is empty the header derives it (single parent)
/// or the lineage is exhausted (genesis).
///
/// `min_height` bounds the walk below: blocks under it cannot carry the
/// effect. Callers holding the deploy pass its `valid_after_block_number`
/// (no execution precedes validity); sig-only callers pass the validity
/// window's floor bound (`floor_number - deploy_lifespan` — a scope-live
/// sig's window was open at its execution, so nothing deeper can hold it).
pub(crate) fn effect_in_state_of(
    block_store: &KeyValueBlockStore,
    block_hash: &BlockHash,
    sig: &Bytes,
    min_height: i64,
) -> Result<bool, CasperError> {
    let mut cur = block_hash.clone();
    loop {
        let Some(block) = block_store.get(&cur)? else {
            return Err(CasperError::Other(format!(
                "effect_in_state_of: block {} on the base lineage is absent \
                 from the store — refusing to judge membership from an \
                 incomplete lineage",
                hex::encode(&cur[..8.min(cur.len())]),
            )));
        };
        if block.body.state.block_number < min_height {
            return Ok(false);
        }
        // A failed execution's deploy is in the body while its effect is
        // NOT in the state — only successful executions count.
        if block
            .body
            .deploys
            .iter()
            .any(|pd| pd.deploy.sig == *sig && !pd.is_failed)
        {
            return Ok(true);
        }
        if block.body.applied_from_scope.iter().any(|s| s == sig) {
            return Ok(true);
        }
        cur = if !block.body.merge_base.is_empty() {
            block.body.merge_base.clone()
        } else {
            match block.header.parents_hash_list.as_slice() {
                // Genesis: the lineage is exhausted.
                [] => return Ok(false),
                // Single parent: the base is the sole parent, already
                // consensus data in the header — not re-recorded.
                [parent] => parent.clone(),
                // A multi-parent block's state parent is NOT derivable from
                // the header alone (merged: the floor; fast-path: the
                // covering parent). Its absence is a malformed block, never
                // a guess.
                _ => {
                    return Err(CasperError::Other(format!(
                        "effect_in_state_of: multi-parent block {} carries \
                         no recorded merge_base — refusing to guess its \
                         state lineage",
                        hex::encode(&cur[..8.min(cur.len())]),
                    )))
                }
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use models::rust::block_implicits::get_random_block;
    use models::rust::casper::protocol::casper_message::BlockMessage;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::*;

    fn block_at(height: i64, parents: Vec<BlockHash>, seq: i32) -> BlockMessage {
        get_random_block(
            Some(height),
            Some(seq),
            None,
            None,
            None,
            None,
            Some(i64::from(seq)),
            Some(parents),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("test-shard".to_string()),
            None,
        )
    }

    /// A genuinely signed deploy (the store re-verifies deploy signatures
    /// on decode) with a distinct sig per `n`, wrapped as processed.
    fn processed(
        n: i32,
        failed: bool,
    ) -> (
        Bytes,
        models::rust::casper::protocol::casper_message::ProcessedDeploy,
    ) {
        let deploy = crate::rust::util::construct_deploy::basic_deploy_data(n, None, None)
            .expect("deploy data");
        let sig = deploy.sig.clone();
        let mut pd = models::rust::casper::protocol::casper_message::ProcessedDeploy::empty(deploy);
        pd.is_failed = failed;
        (sig, pd)
    }

    async fn store() -> KeyValueBlockStore {
        let mut kvm = InMemoryStoreManager::new();
        KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store")
    }

    /// genesis(0) <- a(1, fresh sig_a) <- m(2, base=a, applied sig_b):
    /// membership walks the recorded lineage for both fact kinds and
    /// exhausts at genesis for unknown sigs.
    #[tokio::test]
    async fn walks_fresh_and_applied_facts_to_genesis() {
        let store = store().await;
        let genesis = block_at(0, vec![], 0);
        let (sig_a, pd_a) = processed(1, false);
        let mut a = block_at(1, vec![genesis.block_hash.clone()], 1);
        a.body.deploys = vec![pd_a];
        let sig_b = Bytes::from_static(b"applied_sig");
        let mut m = block_at(2, vec![a.block_hash.clone(), genesis.block_hash.clone()], 2);
        m.body.merge_base = a.block_hash.clone();
        m.body.applied_from_scope = vec![sig_b.clone()];
        for b in [&genesis, &a, &m] {
            store.put_block_message(b).expect("store block");
        }

        let sig_c = Bytes::from_static(b"unknown_sig");
        assert!(effect_in_state_of(&store, &m.block_hash, &sig_b, 0).expect("walk"));
        assert!(effect_in_state_of(&store, &m.block_hash, &sig_a, 0).expect("walk"));
        assert!(!effect_in_state_of(&store, &m.block_hash, &sig_c, 0).expect("walk"));
    }

    /// A failed execution's deploy rides the body while its effect is not
    /// in the state: the walk must not count it.
    #[tokio::test]
    async fn a_failed_execution_is_not_membership() {
        let store = store().await;
        let genesis = block_at(0, vec![], 0);
        let (sig_f, pd_f) = processed(1, true);
        let mut a = block_at(1, vec![genesis.block_hash.clone()], 1);
        a.body.deploys = vec![pd_f];
        for b in [&genesis, &a] {
            store.put_block_message(b).expect("store block");
        }
        assert!(!effect_in_state_of(&store, &a.block_hash, &sig_f, 0).expect("walk"));
    }

    /// The bound stops the walk: an execution below `min_height` is
    /// invisible by construction, so the walk need not read it.
    #[tokio::test]
    async fn min_height_bounds_the_walk() {
        let store = store().await;
        let genesis = block_at(0, vec![], 0);
        let (sig_a, pd_a) = processed(1, false);
        let mut a = block_at(1, vec![genesis.block_hash.clone()], 1);
        a.body.deploys = vec![pd_a];
        let b = block_at(2, vec![a.block_hash.clone()], 2);
        for blk in [&genesis, &a, &b] {
            store.put_block_message(blk).expect("store block");
        }
        assert!(effect_in_state_of(&store, &b.block_hash, &sig_a, 1).expect("walk"));
        assert!(!effect_in_state_of(&store, &b.block_hash, &sig_a, 2).expect("walk"));
    }

    /// A multi-parent block with no recorded base is malformed: the walk
    /// refuses to guess its state lineage.
    #[tokio::test]
    async fn multi_parent_without_base_is_an_error() {
        let store = store().await;
        let genesis = block_at(0, vec![], 0);
        let a = block_at(1, vec![genesis.block_hash.clone()], 1);
        let m = block_at(2, vec![a.block_hash.clone(), genesis.block_hash.clone()], 2);
        for b in [&genesis, &a, &m] {
            store.put_block_message(b).expect("store block");
        }
        let sig = Bytes::from_static(b"sig_x");
        assert!(effect_in_state_of(&store, &m.block_hash, &sig, 0).is_err());
    }
}
