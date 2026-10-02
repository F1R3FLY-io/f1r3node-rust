//! Per-block deploy facts, decoded from a block body once per process.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::RejectedDeploy;
use prost::bytes::Bytes;
use shared::rust::store::key_value_store::MissingBlockContext;

use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{BLOCK_FACTS_CACHE_CLEARED_METRIC, CASPER_METRICS_SOURCE};

/// Budgeted in measured bytes, not entries: one fat block can carry hundreds of
/// sigs. Overflow clears the whole cache instead of evicting piecewise — entries are
/// pure functions of immutable bodies, so a clear costs only re-reads. Thrash would
/// need one walk's entries to approach the budget; walk depth is bounded by the
/// floor-distance merge backstop (`merge_scope_backstop_exceeded`), and a 128-sig
/// block costs ~20 KB with sigs counted in both `deploy_sigs` and `applied_sigs`.
const CACHE_MAX_BYTES: usize = 32 * 1024 * 1024;
/// Map slot, `Arc` headers and hash key per entry.
const ENTRY_OVERHEAD_BYTES: usize = 128;

/// Where a block's state lineage continues.
pub(crate) enum LineageNext {
    Base(BlockHash),
    Genesis,
    /// Multi-parent block without a recorded `merge_base`. Walks refuse to step
    /// through it; readers of the block's own facts are unaffected.
    MalformedMultiParent,
}

pub(crate) struct BlockFacts {
    pub(crate) block_number: i64,
    pub(crate) sender: Bytes,
    pub(crate) parents: Vec<BlockHash>,
    pub(crate) lineage_next: LineageNext,
    /// Every `body.deploys` sig, failed executions included.
    pub(crate) deploy_sigs: Vec<Bytes>,
    /// Sigs whose effect the block's state holds: non-failed deploys and `applied_from_scope`.
    pub(crate) applied_sigs: HashSet<Bytes>,
    pub(crate) rejected: Arc<Vec<RejectedDeploy>>,
}

impl BlockFacts {
    /// A duplicate-flagged record does not dispute the sig's standing win.
    pub(crate) fn kept_rejected(&self) -> impl Iterator<Item = &RejectedDeploy> {
        self.rejected.iter().filter(|record| !record.duplicate)
    }

    fn approx_bytes(&self) -> usize {
        ENTRY_OVERHEAD_BYTES
            + self.sender.len()
            + self.parents.iter().map(|p| p.len()).sum::<usize>()
            + self.deploy_sigs.iter().map(Bytes::len).sum::<usize>()
            + self.applied_sigs.iter().map(Bytes::len).sum::<usize>()
            + self
                .rejected
                .iter()
                .map(|r| r.sig.len() + r.carrier.len() + 16)
                .sum::<usize>()
    }
}

#[derive(Default)]
struct FactsCache {
    map: HashMap<BlockHash, Arc<BlockFacts>>,
    approx_bytes: usize,
}

impl FactsCache {
    fn insert(&mut self, hash: BlockHash, facts: Arc<BlockFacts>) {
        let entry_bytes = facts.approx_bytes();
        if self.approx_bytes + entry_bytes > CACHE_MAX_BYTES {
            metrics::counter!(BLOCK_FACTS_CACHE_CLEARED_METRIC, "source" => CASPER_METRICS_SOURCE)
                .increment(1);
            self.map.clear();
            self.approx_bytes = 0;
        }
        self.approx_bytes += entry_bytes;
        self.map.insert(hash, facts);
    }
}

fn cache() -> &'static parking_lot::Mutex<FactsCache> {
    static CACHE: OnceLock<parking_lot::Mutex<FactsCache>> = OnceLock::new();
    CACHE.get_or_init(|| parking_lot::Mutex::new(FactsCache::default()))
}

#[cfg(test)]
pub(crate) fn cache_bytes() -> usize { cache().lock().approx_bytes }

#[cfg(test)]
pub(crate) const MAX_CACHE_BYTES: usize = CACHE_MAX_BYTES;

enum Lookup {
    Held(Arc<BlockFacts>),
    Absent,
    /// Cached from a store that held the block, but the caller's store does not.
    CachedNotHeld,
}

/// The cache is process-global, so a hit is revalidated against the caller's store.
fn lookup(block_store: &KeyValueBlockStore, block_hash: &BlockHash) -> Result<Lookup, CasperError> {
    let cached = cache().lock().map.get(block_hash).cloned();
    if let Some(facts) = cached {
        return Ok(if block_store.contains_key(block_hash)? {
            Lookup::Held(facts)
        } else {
            Lookup::CachedNotHeld
        });
    }
    let Some(block) = block_store.get(block_hash)? else {
        return Ok(Lookup::Absent);
    };
    let lineage_next = if !block.body.merge_base.is_empty() {
        LineageNext::Base(block.body.merge_base.clone())
    } else {
        match block.header.parents_hash_list.as_slice() {
            [] => LineageNext::Genesis,
            [parent] => LineageNext::Base(parent.clone()),
            _ => LineageNext::MalformedMultiParent,
        }
    };
    let mut applied_sigs: HashSet<Bytes> = block
        .body
        .deploys
        .iter()
        .filter(|pd| !pd.is_failed)
        .map(|pd| pd.deploy.sig.clone())
        .collect();
    applied_sigs.extend(block.body.applied_from_scope.iter().cloned());
    let facts = Arc::new(BlockFacts {
        block_number: block.body.state.block_number,
        sender: block.sender.clone(),
        deploy_sigs: block
            .body
            .deploys
            .iter()
            .map(|pd| pd.deploy.sig.clone())
            .collect(),
        parents: block.header.parents_hash_list,
        lineage_next,
        applied_sigs,
        rejected: Arc::new(block.body.rejected_deploys),
    });
    cache().lock().insert(block_hash.clone(), facts.clone());
    Ok(Lookup::Held(facts))
}

/// The block's facts, or `None` when `block_store` does not hold it.
pub(crate) fn held_block_facts(
    block_store: &KeyValueBlockStore,
    block_hash: &BlockHash,
) -> Result<Option<Arc<BlockFacts>>, CasperError> {
    Ok(match lookup(block_store, block_hash)? {
        Lookup::Held(facts) => Some(facts),
        Lookup::Absent | Lookup::CachedNotHeld => None,
    })
}

/// The block's facts; a block `block_store` does not hold is `BlockNotHeld`, labelled
/// with `accessor` unless the block was cached from another store.
pub(crate) fn block_facts(
    block_store: &KeyValueBlockStore,
    block_hash: &BlockHash,
    accessor: &'static str,
) -> Result<Arc<BlockFacts>, CasperError> {
    let accessor = match lookup(block_store, block_hash)? {
        Lookup::Held(facts) => return Ok(facts),
        Lookup::Absent => accessor,
        Lookup::CachedNotHeld => "block-facts cache revalidation",
    };
    Err(CasperError::BlockNotHeld(
        block_hash.clone(),
        MissingBlockContext::new(accessor),
    ))
}
